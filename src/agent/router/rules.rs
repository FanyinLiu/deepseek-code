use super::{ReasonCode, RiskFlag};

/// Output of the local heuristic rule engine.
#[derive(Debug, Clone)]
pub struct RuleAssessment {
    pub score: u32,
    pub confidence: f64,
    pub reason_codes: Vec<ReasonCode>,
    pub hard_trigger_codes: Vec<String>,
    pub predicted_write_files: u32,
    pub predicted_commands: u32,
    pub predicted_duration_ms: u32,
    pub risk_flags: Vec<RiskFlag>,
    pub has_hard_trigger: bool,
}

impl RuleAssessment {
    #[must_use]
    pub fn is_ambiguous(&self) -> bool {
        self.reason_codes.contains(&ReasonCode::AmbiguousTask) && self.confidence < 0.60
    }
}

/// Run the local rule engine against a task input.
#[must_use]
pub fn assess_with_rules(task_input: &str, _project_context: Option<&str>) -> RuleAssessment {
    let input = task_input.trim();
    let lower = input.to_lowercase();
    let emotional_manipulation = crate::defense::EmotionalShielding.detect(input).is_some();

    let mut score: u32 = 0;
    let mut reasons: Vec<ReasonCode> = Vec::new();
    let mut hard_triggers: Vec<String> = Vec::new();
    let mut risks: Vec<RiskFlag> = Vec::new();

    let mut predicted_cmds: u32 = 0;

    // ── Hard triggers (immediately complex) ──

    if has_network_or_install(&lower) {
        hard_triggers.push("NETWORK_REQUIRED".into());
        reasons.push(ReasonCode::NetworkRequired);
        risks.push(RiskFlag::PermissionChange);
    }

    if has_high_risk_action(&lower) {
        hard_triggers.push("HIGH_RISK".into());
        reasons.push(ReasonCode::HighRisk);
        if lower.contains("delete") || lower.contains("移除") || lower.contains("删除") {
            risks.push(RiskFlag::Delete);
        }
        if lower.contains("deploy") || lower.contains("部署") || lower.contains("发布") {
            risks.push(RiskFlag::Deploy);
        }
        if lower.contains("migrate") || lower.contains("迁移") {
            risks.push(RiskFlag::Migration);
        }
        if lower.contains("permission") || lower.contains("权限") {
            risks.push(RiskFlag::PermissionChange);
        }
    }

    if emotional_manipulation {
        hard_triggers.push("EMOTIONAL_MANIPULATION".into());
        reasons.push(ReasonCode::HighRisk);
        risks.push(RiskFlag::PermissionChange);
        score = score.max(40);
    }

    let has_hard = !hard_triggers.is_empty();

    // ── Scoring signals ──

    // File count estimation
    let file_estimate = estimate_file_changes(input, &lower);
    let mut predicted_files: u32 = file_estimate;
    if file_estimate >= 2 {
        score += 20;
        reasons.push(ReasonCode::MultiFile);
    } else if file_estimate == 1 {
        // single file is neutral / simple-biased
    }

    // Command execution
    if implies_commands(&lower) {
        score += 15;
        predicted_cmds = 1;
        reasons.push(ReasonCode::ShellRequired);
    }

    // Testing / building
    if implies_testing(&lower) {
        score += 10;
        reasons.push(ReasonCode::TestRequired);
    }

    // Cross-module impact
    if implies_cross_module(&lower) {
        score += 10;
        if !reasons.contains(&ReasonCode::MultiFile) {
            reasons.push(ReasonCode::MultiFile);
        }
    }

    // Goal clarity
    let clarity = assess_clarity(input, &lower);
    if clarity < 0.4 && !is_simple_greeting(&lower) {
        score += 15;
        reasons.push(ReasonCode::AmbiguousTask);
    }

    // Duration estimate
    let predicted_duration: u32 = estimate_duration(input, &lower, file_estimate, predicted_cmds);
    if predicted_duration > 120_000 {
        score += 10;
    }

    // Intent keywords
    let intent_boost = intent_complexity_score(&lower);
    score += intent_boost;

    // Read-only detection (overrides toward simple unless the user also asks us to change things).
    // Local file/directory inspection must stay on the direct tool path; otherwise a vague
    // "can you read my computer files?" prompt can be misrouted into clarification before the
    // read tools are even made available.
    let read_only_intent = (is_read_only(&lower) || has_local_context_read_intent(&lower))
        && !has_write_intent(&lower);
    if read_only_intent {
        if !reasons.contains(&ReasonCode::ReadOnly) {
            reasons.push(ReasonCode::ReadOnly);
        }
        predicted_files = 0;
        score = score.saturating_sub(if score < 40 { 15 } else { 25 });
    }

    // Single-file safe detection
    if file_estimate == 1
        && predicted_cmds == 0
        && !has_hard
        && !reasons.contains(&ReasonCode::TestRequired)
    {
        if !reasons.contains(&ReasonCode::SingleFileSafe) {
            reasons.push(ReasonCode::SingleFileSafe);
        }
        score = score.saturating_sub(5);
    }

    // Cap score
    score = score.min(100);

    // Confidence: higher when signals are strong and unambiguous
    let mut confidence = compute_confidence(&reasons, &hard_triggers, clarity);

    // Boost confidence for low-risk, low-score tasks (chat / read-only greetings)
    if (score < 15 || read_only_intent) && hard_triggers.is_empty() && risks.is_empty() {
        confidence = confidence.max(0.85);
    }

    RuleAssessment {
        score,
        confidence,
        reason_codes: reasons,
        hard_trigger_codes: hard_triggers,
        predicted_write_files: predicted_files,
        predicted_commands: predicted_cmds,
        predicted_duration_ms: predicted_duration,
        risk_flags: risks,
        has_hard_trigger: has_hard,
    }
}

// ---------------------------------------------------------------------------
// Rule helpers
// ---------------------------------------------------------------------------

fn has_network_or_install(lower: &str) -> bool {
    lower.contains("install")
        || lower.contains("npm install")
        || lower.contains("pip install")
        || lower.contains("cargo add")
        || lower.contains("apt ")
        || lower.contains("brew ")
        || lower.contains("安装")
        || lower.contains("下载")
        || lower.contains("更新依赖")
        || lower.contains("联网")
        || lower.contains("curl")
        || lower.contains("wget")
        || lower.contains("fetch")
}

fn has_high_risk_action(lower: &str) -> bool {
    lower.contains("delete")
        || lower.contains("remove ")
        || lower.contains("drop ")
        || lower.contains("deploy")
        || lower.contains("migrate")
        || lower.contains("migration")
        || lower.contains("permission")
        || lower.contains("chmod")
        || lower.contains("push to main")
        || lower.contains("force push")
        || lower.contains("删除")
        || lower.contains("移除")
        || lower.contains("迁移")
        || lower.contains("部署")
        || lower.contains("发布")
        || lower.contains("权限")
        || lower.contains("主分支")
}

fn implies_commands(lower: &str) -> bool {
    lower.contains("run ")
        || lower.contains("execute")
        || lower.contains("build")
        || lower.contains("cargo ")
        || lower.contains("npm ")
        || lower.contains("yarn ")
        || lower.contains("make")
        || lower.contains("cmake")
        || lower.contains("docker")
        || lower.contains("执行")
        || lower.contains("运行")
        || lower.contains("构建")
        || lower.contains("编译")
}

fn implies_testing(lower: &str) -> bool {
    lower.contains("test")
        || lower.contains("unit test")
        || lower.contains("integration")
        || lower.contains("ci")
        || lower.contains("验证")
        || lower.contains("补测试")
        || lower.contains("测试用例")
        || lower.contains("assert")
}

fn implies_cross_module(lower: &str) -> bool {
    lower.contains("across")
        || lower.contains("module")
        || lower.contains("shared")
        || lower.contains("global")
        || lower.contains("refactor")
        || lower.contains("架构")
        || lower.contains("跨模块")
        || lower.contains("全局")
        || lower.contains("整体")
        || lower.contains("模块")
}

fn is_read_only(lower: &str) -> bool {
    lower.starts_with("explain")
        || lower.starts_with("review")
        || lower.starts_with("audit")
        || lower.starts_with("what is")
        || lower.starts_with("how does")
        || lower.starts_with("describe")
        || lower.starts_with("summary")
        || lower.starts_with("文档")
        || lower.starts_with("解释")
        || lower.starts_with("说明")
        || lower.starts_with("什么是")
        || lower.contains("解释一下")
        || lower.contains("为什么")
        || lower.contains("怎么看")
        || lower.contains("只读")
        || lower.contains("仅读取")
        || lower.contains("不要改")
        || lower.contains("不要修改")
        || lower.contains("不改文件")
        || lower.contains("审查")
        || lower.contains("检查")
        || lower.contains("检测")
        || lower.contains("review")
        || lower.contains("audit")
}

fn is_simple_greeting(lower: &str) -> bool {
    matches!(
        lower.trim(),
        "hi" | "hello" | "hey" | "你好" | "您好" | "嗨"
    )
}

fn has_local_context_read_intent(lower: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "read file",
        "read this file",
        "read my computer",
        "local file",
        "computer file",
        "computer folder",
        "my computer",
        "absolute path",
        "local folder",
        "folder",
        "list files",
        "list dir",
        "directory",
        "project structure",
        "workspace",
        "codebase",
        "inspect code",
        "search code",
        "analyze repo",
        "读取",
        "读一下",
        "查看",
        "看一下",
        "打开文件",
        "电脑文件",
        "电脑里的文件",
        "电脑里",
        "电脑里面",
        "电脑上的",
        "我的电脑",
        "本机文件",
        "本地文件",
        "本地目录",
        "本地文件夹",
        "绝对路径",
        "文件夹",
        "目录",
        "项目结构",
        "目录结构",
        "文件结构",
        "列出文件",
        "搜索文件",
        "搜索代码",
        "检查代码",
        "分析仓库",
        "分析项目",
    ];
    NEEDLES.iter().any(|needle| lower.contains(needle))
}

fn has_write_intent(lower: &str) -> bool {
    if lower.contains("不要改") || lower.contains("不要修改") || lower.contains("不改文件")
    {
        return false;
    }
    lower.contains("修复")
        || lower.contains("修改")
        || lower.contains("更改")
        || lower.contains("实现")
        || lower.contains("添加")
        || lower.contains("补测试")
        || lower.contains("fix")
        || lower.contains("modify")
        || lower.contains("change")
        || lower.contains("implement")
        || lower.contains("add ")
        || lower.contains("write")
}

fn estimate_file_changes(_input: &str, lower: &str) -> u32 {
    // Very naive heuristic — can be enhanced with AST / git diff heuristics later.
    if lower.contains("refactor")
        || lower.contains("rewrite")
        || lower.contains("redesign")
        || lower.contains("架构")
        || lower.contains("整体")
        || lower.contains("全部")
        || lower.contains("重构")
    {
        return 3;
    }
    if lower.contains("and")
        && (lower.contains("file") || lower.contains("模块") || lower.contains("组件"))
    {
        return 2;
    }
    if lower.contains("file")
        || lower.contains("文件")
        || lower.contains("模块")
        || lower.contains("组件")
    {
        return 1;
    }
    // Default: no explicit file mention, assume 0 for read-only / chat tasks
    0
}

fn estimate_duration(_input: &str, lower: &str, files: u32, cmds: u32) -> u32 {
    let base: u32 = 5_000; // 5 seconds baseline
    let file_ms = files * 8_000;
    let cmd_ms = cmds * 15_000;
    let keyword_ms = if lower.contains("large") || lower.contains("big") || lower.contains("大量")
    {
        60_000
    } else {
        0
    };
    base + file_ms + cmd_ms + keyword_ms
}

fn assess_clarity(input: &str, lower: &str) -> f64 {
    let min_len = 5;
    let max_len = 300;
    let len = input.chars().count();

    if len < min_len {
        return 0.0;
    }

    // Length score: too short is bad, very long is slightly penalised
    let len_score = if len > max_len { 0.7 } else { 1.0 };

    // Specificity markers improve clarity
    let mut specificity = 0.0;
    if lower.contains("file") || lower.contains("文件") {
        specificity += 0.2;
    }
    if lower.contains("function") || lower.contains("函数") || lower.contains("method") {
        specificity += 0.2;
    }
    if lower.contains("test") || lower.contains("验证") {
        specificity += 0.1;
    }
    if lower.contains(" because ")
        || lower.contains("so that")
        || lower.contains("因为")
        || lower.contains("以便")
    {
        specificity += 0.1;
    }

    let vague_words = ["something", "anything", "whatever", "随便", "之类", "等等"];
    let vague_penalty = vague_words.iter().filter(|w| lower.contains(*w)).count() as f64 * 0.15;

    (0.5 + specificity - vague_penalty).clamp(0.0, 1.0) * len_score
}

fn intent_complexity_score(lower: &str) -> u32 {
    let complex_keywords = [
        ("方案", 10),
        ("重构", 15),
        ("评估", 10),
        ("迁移", 15),
        ("设计", 10),
        ("支持", 5),
        ("实现", 5),
        ("添加功能", 10),
        ("新功能", 10),
        ("架构调整", 15),
        ("redesign", 15),
        ("architecture", 10),
        ("implement", 5),
        ("feature", 5),
    ];

    let mut boost = 0;
    for (kw, points) in &complex_keywords {
        if lower.contains(kw) {
            boost += points;
        }
    }
    boost.min(30)
}

fn compute_confidence(reasons: &[ReasonCode], hard_triggers: &[String], clarity: f64) -> f64 {
    let base = 0.5;
    let signal_count = reasons.len() as f64 * 0.05;
    let hard_bonus = if hard_triggers.is_empty() { 0.0 } else { 0.2 };
    let clarity_bonus = clarity * 0.2;
    (base + signal_count + hard_bonus + clarity_bonus).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clarity_counts_user_visible_chars_not_utf8_bytes() {
        let cjk = "界".repeat(120);

        assert!(assess_clarity(&cjk, &cjk) >= 0.5);
        assert_eq!(assess_clarity("你好", "你好"), 0.0);
    }

    #[test]
    fn short_cjk_greeting_stays_simple_and_confident() {
        let result = assess_with_rules("你好", None);

        assert!(result.score < 15);
        assert!(result.confidence >= 0.75);
        assert!(!result.reason_codes.contains(&ReasonCode::AmbiguousTask));
    }
}
