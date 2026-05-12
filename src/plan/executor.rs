use super::schema::Plan;

/// Execute a plan: convert plan steps into tool calls for the agent loop.
/// The Plan Executor does NOT directly modify files — it instructs the
/// agent orchestrator on what to do next.

#[derive(Debug, Clone)]
pub enum PlanStep {
    ReadFile { path: String, reason: String },
    SearchCode { pattern: String, reason: String },
    EditFile { path: String, description: String },
    RunCommand { command: String, reason: String },
    Verify { description: String },
}

impl PlanStep {
    /// Human-readable one-line description for UI display.
    #[must_use]
    pub fn display(&self) -> String {
        self.display_with_language(false)
    }

    /// Human-readable one-line description localized for UI display.
    #[must_use]
    pub fn display_with_language(&self, use_chinese: bool) -> String {
        match self {
            Self::ReadFile { path, reason } if use_chinese => format!("读取 `{path}` - {reason}"),
            Self::SearchCode { pattern, reason } if use_chinese => {
                format!("搜索 `{pattern}` - {reason}")
            }
            Self::EditFile { path, description } if use_chinese => {
                format!("修改 `{path}` - {description}")
            }
            Self::RunCommand { command, reason } if use_chinese => {
                format!("运行 `{command}` - {reason}")
            }
            Self::Verify { description } if use_chinese => format!("验证 - {description}"),
            Self::ReadFile { path, reason } => format!("Read `{path}` — {reason}"),
            Self::SearchCode { pattern, reason } => format!("Search `{pattern}` — {reason}"),
            Self::EditFile { path, description } => format!("Edit `{path}` — {description}"),
            Self::RunCommand { command, reason } => format!("Run `{command}` — {reason}"),
            Self::Verify { description } => format!("Verify — {description}"),
        }
    }
}

/// Translate a plan into concrete execution steps.
#[must_use]
pub fn plan_to_steps(plan: &Plan) -> Vec<PlanStep> {
    let mut steps = Vec::new();

    // Phase 1: Read all target files first
    for file in &plan.target_files {
        steps.push(PlanStep::ReadFile {
            path: file.path.clone(),
            reason: file.reason.clone(),
        });
    }

    // Phase 2: Execute each plan step
    for step in &plan.steps {
        steps.push(classify_step(step, &plan.target_files));
    }

    // Phase 3: Verification
    for verify in &plan.verification {
        steps.push(PlanStep::Verify {
            description: verify.clone(),
        });
    }

    steps
}

/// Classify a plan step into a concrete PlanStep.
///
/// Priority order (highest to lowest):
/// 1. Command  — backtick-quoted command or leading "run"/"test"/"build"/"execute"
/// 2. Verify   — leading "verify"/"check"/"assert"/"confirm"/"ensure"/"validate"
/// 3. Search   — leading "search"/"find"/"look"/"explore"/"grep"/"scan"
/// 4. Edit     — mentions a known target file path AND contains a write verb
/// 5. Search   — default (safer than defaulting to edit)
fn classify_step(step: &str, target_files: &[super::schema::TargetFile]) -> PlanStep {
    let lower = step.to_lowercase();

    // 1. Command: has backtick-quoted shell command or starts with a run verb
    if has_backtick_command(&lower) || starts_with_run_verb(&lower) {
        return PlanStep::RunCommand {
            command: extract_command(step),
            reason: step.to_string(),
        };
    }

    // 2. Verify
    if starts_with_any(
        &lower,
        &[
            "verify",
            "check",
            "assert",
            "confirm",
            "ensure",
            "validate",
            "test that",
            "验证",
            "检查",
            "确认",
            "确保",
            "测试",
        ],
    ) {
        return PlanStep::Verify {
            description: step.to_string(),
        };
    }

    // 3. Explicit search
    if starts_with_any(
        &lower,
        &[
            "search", "find", "look", "explore", "grep", "scan", "locate", "搜索", "查找", "查看",
            "扫描", "定位", "阅读",
        ],
    ) {
        return PlanStep::SearchCode {
            pattern: step.to_string(),
            reason: "from plan step".into(),
        };
    }

    // 4. Edit: step references a known target file AND uses a write verb
    let has_write_verb = lower.contains("modify")
        || lower.contains("update")
        || lower.contains("implement")
        || lower.contains("write")
        || lower.contains("refactor")
        || lower.contains("rename")
        || lower.contains("delete")
        || lower.contains("修改")
        || lower.contains("更新")
        || lower.contains("实现")
        || lower.contains("写入")
        || lower.contains("重构")
        || lower.contains("删除")
        || lower.contains("新增");
    if has_write_verb {
        if let Some(path) = find_file_in_step(step, target_files) {
            return PlanStep::EditFile {
                path,
                description: step.to_string(),
            };
        }
    }

    // 5. Default: search (safer than misclassifying as edit)
    PlanStep::SearchCode {
        pattern: step.to_string(),
        reason: "from plan step".into(),
    }
}

fn has_backtick_command(lower: &str) -> bool {
    lower.contains("`cargo ")
        || lower.contains("`npm ")
        || lower.contains("`go ")
        || lower.contains("`git ")
        || lower.contains("`make")
        || lower.contains("`python")
        || lower.contains("`rustc")
}

fn starts_with_run_verb(lower: &str) -> bool {
    starts_with_any(
        lower,
        &[
            "run ", "execute ", "build ", "compile ", "运行", "执行", "构建", "编译",
        ],
    )
}

fn starts_with_any(text: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| text.starts_with(p))
}

/// Find the first target file whose path appears verbatim in the step text.
fn find_file_in_step(step: &str, target_files: &[super::schema::TargetFile]) -> Option<String> {
    let lower = step.to_lowercase();
    target_files
        .iter()
        .find(|f| lower.contains(&f.path.to_lowercase()))
        .map(|f| f.path.clone())
}

fn extract_command(step: &str) -> String {
    // Try to extract a command from backticks
    for part in step.split('`') {
        if part.contains("cargo")
            || part.contains("npm")
            || part.contains("pip")
            || part.contains("go ")
            || part.contains("git ")
            || part.contains("make")
        {
            return part.trim().to_string();
        }
    }
    let backticked = step.split('`').collect::<Vec<_>>();
    if backticked.len() >= 3 {
        return backticked[1].trim().to_string();
    }
    step.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::schema::{Plan, TargetFile};

    fn plan_with_step(step: &str) -> Plan {
        Plan {
            summary: "Harden plan execution".to_string(),
            target_files: vec![TargetFile {
                path: "src/agent/orchestrator.rs".to_string(),
                reason: "plan execution state".to_string(),
            }],
            steps: vec![step.to_string()],
            risks: Vec::new(),
            verification: Vec::new(),
            requires_write: true,
            requires_command: false,
            recommended_model: None,
            thinking: None,
        }
    }

    #[test]
    fn ambiguous_add_step_defaults_to_search() {
        // "Add" without a file path → search for context first, safer than blindly editing.
        let steps = plan_to_steps(&plan_with_step("Add a runtime guard for failed plan steps"));
        assert!(matches!(steps[1], PlanStep::SearchCode { .. }));
    }

    #[test]
    fn backticked_commands_are_extracted() {
        let steps = plan_to_steps(&plan_with_step("Run `cargo test plan::executor`"));
        assert!(matches!(
            &steps[1],
            PlanStep::RunCommand { command, .. } if command == "cargo test plan::executor"
        ));
    }

    #[test]
    fn verify_prefix_yields_verify_step() {
        let steps = plan_to_steps(&plan_with_step("Verify the fix compiles without errors"));
        assert!(matches!(steps[1], PlanStep::Verify { .. }));
    }

    #[test]
    fn update_with_known_file_yields_edit() {
        let steps = plan_to_steps(&plan_with_step(
            "Update src/agent/orchestrator.rs to handle the new error variant",
        ));
        assert!(matches!(
            &steps[1],
            PlanStep::EditFile { path, .. } if path == "src/agent/orchestrator.rs"
        ));
    }

    #[test]
    fn update_without_file_path_yields_search() {
        let steps = plan_to_steps(&plan_with_step("Update the error handling logic"));
        assert!(matches!(steps[1], PlanStep::SearchCode { .. }));
    }

    #[test]
    fn run_prefix_yields_command() {
        let steps = plan_to_steps(&plan_with_step(
            "run the test suite to confirm no regressions",
        ));
        assert!(matches!(steps[1], PlanStep::RunCommand { .. }));
    }

    #[test]
    fn chinese_prefixes_are_classified() {
        let steps = plan_to_steps(&plan_with_step("运行 `echo hello` 确认 CLI 可用"));
        assert!(matches!(
            &steps[1],
            PlanStep::RunCommand { command, .. } if command == "echo hello"
        ));

        let steps = plan_to_steps(&plan_with_step("验证输出没有错误"));
        assert!(matches!(&steps[1], PlanStep::Verify { .. }));

        let steps = plan_to_steps(&plan_with_step("搜索 CLI 入口文件"));
        assert!(matches!(&steps[1], PlanStep::SearchCode { .. }));
    }

    #[test]
    fn display_can_follow_chinese_ui_language() {
        let step = PlanStep::RunCommand {
            command: "cargo test".to_string(),
            reason: "确认没有回归".to_string(),
        };

        assert_eq!(
            step.display_with_language(true),
            "运行 `cargo test` - 确认没有回归"
        );
        assert_eq!(
            step.display_with_language(false),
            "Run `cargo test` — 确认没有回归"
        );
    }
}
