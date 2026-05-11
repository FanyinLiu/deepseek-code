pub mod classifier;
pub mod rules;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

/// Task complexity router — decides whether a task should be executed directly,
/// planned first, or clarified before proceeding.
#[derive(Debug, Clone)]
pub struct ComplexityRouter {
    pub version: String,
    pub use_model_classifier: bool,
    pub conservative_threshold: bool,
    pub simple_threshold: u32,
    pub confidence_threshold: f64,
}

impl Default for ComplexityRouter {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            use_model_classifier: true,
            conservative_threshold: true,
            simple_threshold: 40,
            confidence_threshold: 0.75,
        }
    }
}

impl From<&crate::storage::config::RouterConfig> for ComplexityRouter {
    fn from(cfg: &crate::storage::config::RouterConfig) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            use_model_classifier: cfg.use_model_classifier,
            conservative_threshold: cfg.conservative,
            simple_threshold: cfg.simple_threshold,
            confidence_threshold: cfg.confidence_threshold,
        }
    }
}

impl ComplexityRouter {
    /// Create a new router with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Disable the model-based classifier and rely on rules only.
    #[must_use]
    pub fn rules_only(mut self) -> Self {
        self.use_model_classifier = false;
        self
    }

    /// Assess a task's complexity using the hybrid rules + model approach.
    ///
    /// Phase 1: Local heuristic rules (fast, zero latency, fully auditable).
    /// Phase 2: If rules are inconclusive and model classifier is enabled,
    ///          call `DeepSeek` v4-flash for structured classification.
    pub async fn assess(
        &self,
        task_input: &str,
        project_context: Option<&str>,
        client: Option<&crate::deepseek::client::DeepSeekClient>,
    ) -> ComplexityAssessment {
        // Phase 1: rules engine (always runs)
        let rule_result = rules::assess_with_rules(task_input, project_context);

        // Hard triggers from rules always win — immediately complex
        if rule_result.has_hard_trigger {
            return ComplexityAssessment::from_rules(
                rule_result,
                Route::PlanReview,
                ComplexityLabel::Complex,
                &self.version,
            );
        }

        // Missing critical info → clarify
        if rule_result.is_ambiguous() {
            return ComplexityAssessment::from_rules(
                rule_result,
                Route::Clarify,
                ComplexityLabel::Unclear,
                &self.version,
            );
        }

        // Phase 2: model classifier (if enabled and client available)
        if self.use_model_classifier && rule_result.confidence < self.confidence_threshold {
            if let Some(client) = client {
                match classifier::classify_with_model(task_input, project_context, client).await {
                    Ok(model_result) => {
                        return merge_results(
                            rule_result,
                            model_result,
                            &self.version,
                            self.conservative_threshold,
                            self.simple_threshold,
                            self.confidence_threshold,
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Model classifier failed, falling back to rules: {}", e);
                        // Fall through to rules fallback
                    }
                }
            }
        }

        // Rules-only fallback (or model disabled / unavailable)
        let route = determine_route_from_score(
            rule_result.score,
            rule_result.confidence,
            self.conservative_threshold,
            self.simple_threshold,
            self.confidence_threshold,
        );
        ComplexityAssessment::from_rules(
            rule_result,
            route.clone(),
            ComplexityLabel::from_route(&route),
            &self.version,
        )
    }
}

// ---------------------------------------------------------------------------
// Core types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    #[serde(rename = "direct_execute")]
    DirectExecute,
    #[serde(rename = "plan_review")]
    PlanReview,
    #[serde(rename = "clarify")]
    Clarify,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplexityLabel {
    #[serde(rename = "simple")]
    Simple,
    #[serde(rename = "complex")]
    Complex,
    #[serde(rename = "unclear")]
    Unclear,
}

impl ComplexityLabel {
    fn from_route(route: &Route) -> Self {
        match route {
            Route::DirectExecute => Self::Simple,
            Route::PlanReview => Self::Complex,
            Route::Clarify => Self::Unclear,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum ReasonCode {
    #[serde(rename = "READ_ONLY")]
    ReadOnly,
    #[serde(rename = "SINGLE_FILE_SAFE")]
    SingleFileSafe,
    #[serde(rename = "MULTI_FILE")]
    MultiFile,
    #[serde(rename = "SHELL_REQUIRED")]
    ShellRequired,
    #[serde(rename = "TEST_REQUIRED")]
    TestRequired,
    #[serde(rename = "NETWORK_REQUIRED")]
    NetworkRequired,
    #[serde(rename = "HIGH_RISK")]
    HighRisk,
    #[serde(rename = "AMBIGUOUS_TASK")]
    AmbiguousTask,
}

impl ReasonCode {
    /// User-visible explanation text (zh-CN default, can be i18n-keyed later).
    #[must_use]
    pub fn user_text(&self) -> &'static str {
        match self {
            Self::ReadOnly => "仅阅读与解释，无改动",
            Self::SingleFileSafe => "预计只改 1 个文件，无命令、无联网",
            Self::MultiFile => "预计影响多个文件",
            Self::ShellRequired => "需要执行命令",
            Self::TestRequired => "需要测试或构建验证",
            Self::NetworkRequired => "需要联网或安装依赖",
            Self::HighRisk => "存在删除/迁移/部署/权限等高风险动作",
            Self::AmbiguousTask => "目标或验收标准不清晰",
        }
    }

    /// Whether this reason code permits direct execution.
    #[must_use]
    pub fn is_safe_for_direct(&self) -> bool {
        matches!(self, Self::ReadOnly | Self::SingleFileSafe)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskFlag {
    #[serde(rename = "DELETE")]
    Delete,
    #[serde(rename = "MIGRATION")]
    Migration,
    #[serde(rename = "DEPLOY")]
    Deploy,
    #[serde(rename = "PERMISSION_CHANGE")]
    PermissionChange,
    #[serde(rename = "MAIN_BRANCH_PUSH")]
    MainBranchPush,
}

/// The final assessment produced by the router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityAssessment {
    pub route: Route,
    pub complexity_label: ComplexityLabel,
    pub score: u32,
    pub confidence: f64,
    pub reason_codes: Vec<ReasonCode>,
    pub hard_trigger_codes: Vec<String>,
    pub predicted_write_files: u32,
    pub predicted_commands: u32,
    pub predicted_duration_ms: u32,
    pub risk_flags: Vec<RiskFlag>,
    pub classifier_version: String,
    pub explanation: String,
    pub model_name: Option<String>,
    pub latency_ms: u64,
}

impl ComplexityAssessment {
    /// Build an assessment purely from rule-engine output.
    fn from_rules(
        rules: rules::RuleAssessment,
        route: Route,
        label: ComplexityLabel,
        version: &str,
    ) -> Self {
        let explanation = build_explanation(&route, &rules.reason_codes, rules.score);
        Self {
            route,
            complexity_label: label,
            score: rules.score,
            confidence: rules.confidence,
            reason_codes: rules.reason_codes,
            hard_trigger_codes: rules.hard_trigger_codes,
            predicted_write_files: rules.predicted_write_files,
            predicted_commands: rules.predicted_commands,
            predicted_duration_ms: rules.predicted_duration_ms,
            risk_flags: rules.risk_flags,
            classifier_version: format!("rules-{version}"),
            explanation,
            model_name: None,
            latency_ms: 0,
        }
    }

    /// Whether this assessment allows direct execution.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        self.route == Route::DirectExecute
    }

    /// Whether this assessment requires clarification.
    #[must_use]
    pub fn needs_clarify(&self) -> bool {
        self.route == Route::Clarify
    }

    /// Human-readable summary for CLI/TUI display.
    pub fn display_summary(&self) -> String {
        let route_str = match self.route {
            Route::DirectExecute => "简单任务 → 直接执行",
            Route::PlanReview => "复杂任务 → 先计划后审阅",
            Route::Clarify => "信息不足 → 需要澄清",
        };
        let chips = self
            .reason_codes
            .iter()
            .map(ReasonCode::user_text)
            .collect::<Vec<_>>()
            .join(" | ");
        format!(
            "{}\n  评分: {} | 置信度: {:.0}% | 原因: {}",
            route_str,
            self.score,
            self.confidence * 100.0,
            chips
        )
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn determine_route_from_score(
    score: u32,
    confidence: f64,
    conservative: bool,
    simple_threshold: u32,
    confidence_threshold: f64,
) -> Route {
    let threshold = if conservative {
        simple_threshold
    } else {
        simple_threshold.saturating_add(10)
    };
    let clarify_lower = 0.50;
    if score < threshold && confidence >= confidence_threshold {
        Route::DirectExecute
    } else if score >= threshold || confidence < clarify_lower {
        Route::PlanReview
    } else {
        // clarify_lower <= confidence < confidence_threshold 且 score 在模糊区间
        Route::Clarify
    }
}

fn merge_results(
    rules: rules::RuleAssessment,
    model: classifier::ModelAssessment,
    version: &str,
    conservative: bool,
    simple_threshold: u32,
    confidence_threshold: f64,
) -> ComplexityAssessment {
    // Weighted blend: rules 40%, model 60%
    let blended_score = (f64::from(rules.score) * 0.4 + f64::from(model.score) * 0.6) as u32;
    let blended_confidence = rules.confidence * 0.3 + model.confidence * 0.7;

    // Union reason codes
    let mut reasons: Vec<ReasonCode> = rules.reason_codes;
    for r in &model.reason_codes {
        if !reasons.contains(r) {
            reasons.push(r.clone());
        }
    }

    // Union risk flags
    let mut risks: Vec<RiskFlag> = rules.risk_flags;
    for r in &model.risk_flags {
        if !risks.contains(r) {
            risks.push(r.clone());
        }
    }

    let route = determine_route_from_score(
        blended_score,
        blended_confidence,
        conservative,
        simple_threshold,
        confidence_threshold,
    );
    let label = ComplexityLabel::from_route(&route);
    let explanation = build_explanation(&route, &reasons, blended_score);

    ComplexityAssessment {
        route,
        complexity_label: label,
        score: blended_score,
        confidence: blended_confidence,
        reason_codes: reasons,
        hard_trigger_codes: rules.hard_trigger_codes,
        predicted_write_files: model.predicted_write_files.max(rules.predicted_write_files),
        predicted_commands: model.predicted_commands.max(rules.predicted_commands),
        predicted_duration_ms: model.predicted_duration_ms.max(rules.predicted_duration_ms),
        risk_flags: risks,
        classifier_version: format!("hybrid-{version}"),
        explanation,
        model_name: Some(model.model_name),
        latency_ms: model.latency_ms,
    }
}

fn build_explanation(route: &Route, reasons: &[ReasonCode], score: u32) -> String {
    let action = match route {
        Route::DirectExecute => "直接执行",
        Route::PlanReview => "先生成计划再审阅",
        Route::Clarify => "先澄清需求",
    };
    let reason_text = reasons
        .iter()
        .map(ReasonCode::user_text)
        .collect::<Vec<_>>()
        .join("；");
    format!("判定为{action}（评分{score}），原因：{reason_text}")
}
