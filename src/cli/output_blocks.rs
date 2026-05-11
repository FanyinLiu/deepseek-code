use crate::agent::orchestrator::PlanStepStatus;
use crate::agent::subagent::SubagentResult;
use crate::policy::ApprovalDisplay;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    Queued,
    Running,
    Done,
    Failed,
    Denied,
    Cancelled,
}

impl BlockStatus {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Denied => "denied",
            Self::Cancelled => "cancelled",
        }
    }

    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Queued => "○",
            Self::Running => "◈",
            Self::Done => "◆",
            Self::Failed => "✗",
            Self::Denied => "!",
            Self::Cancelled => "×",
        }
    }
}

#[must_use]
pub fn status_from_plan(status: PlanStepStatus) -> BlockStatus {
    match status {
        PlanStepStatus::Pending => BlockStatus::Queued,
        PlanStepStatus::Running => BlockStatus::Running,
        PlanStepStatus::Done => BlockStatus::Done,
        PlanStepStatus::Failed => BlockStatus::Failed,
    }
}

pub fn print_header(title: &str, status: BlockStatus) {
    println!("\n{} {}  {}", status.icon(), status.label(), title);
}

pub fn print_kv(label: &str, value: impl AsRef<str>) {
    println!("  {label:<9} {}", value.as_ref());
}

pub fn print_approval(tool_name: &str, display: &ApprovalDisplay, auto_approved: bool) {
    print_header(
        "approve tool call",
        if auto_approved {
            BlockStatus::Done
        } else {
            BlockStatus::Running
        },
    );
    print_kv("tool", tool_name);
    print_kv("intent", &display.description);
    print_kv("risk", display.risk_level.to_string());
    if !display.details.trim().is_empty() {
        for line in display
            .details
            .lines()
            .filter(|line| !line.trim().is_empty())
        {
            if let Some((label, value)) = line.split_once(':') {
                print_kv(label.trim(), value.trim());
            } else {
                print_kv("detail", line.trim());
            }
        }
    }
    if auto_approved {
        print_kv("action", "approved by session trust");
    } else {
        print_kv("actions", "[a] once  [s] session  [d] deny");
    }
}

pub fn print_tool_result(tool_name: &str, success: bool, summary: &str) {
    print_header(
        &format!("tool {tool_name}"),
        if success {
            BlockStatus::Done
        } else {
            BlockStatus::Failed
        },
    );
    print_kv("summary", truncate(summary, 120));
}

pub fn print_task_complete(session_id: &str, total_tokens: u64) {
    print_header("task complete", BlockStatus::Done);
    print_kv("summary", "agent turn finished");
    print_kv("session", session_id);
    print_kv("tokens", total_tokens.to_string());
    print_kv("next", "review output or continue with another task");
}

pub fn print_worker_started(agent_id: &str, agent_type: &str, description: &str) {
    print_header(
        &format!("agent {}", short_id(agent_id)),
        BlockStatus::Running,
    );
    print_kv("type", agent_type);
    print_kv("task", truncate(description, 120));
}

pub fn print_worker_completed(agent_id: &str, result: &SubagentResult) {
    print_header(
        &format!("agent {}", short_id(agent_id)),
        if result.success {
            BlockStatus::Done
        } else {
            BlockStatus::Failed
        },
    );
    print_kv("summary", truncate(&result.summary, 120));
    print_kv("duration", format_duration(result.duration_ms));
    print_kv(
        "files",
        format!(
            "R{} W{}",
            result.files_read.len(),
            result.files_written.len()
        ),
    );
    print_kv("tokens", result.token_usage.to_string());
    if let Some(error) = &result.error {
        print_kv("error", truncate(error, 120));
    }
}

pub fn print_plan_started(summary: &str, total: usize) {
    print_header("plan runner", BlockStatus::Running);
    print_kv("summary", truncate(summary, 120));
    print_kv("steps", total.to_string());
}

pub fn print_plan_step(index: usize, total: usize, description: &str, status: PlanStepStatus) {
    let status = status_from_plan(status);
    print_header(
        &format!("step {}/{}", index.saturating_add(1), total),
        status,
    );
    print_kv("task", truncate(description, 120));
}

pub fn print_plan_complete() {
    print_header("plan complete", BlockStatus::Done);
}

pub fn print_option_block(title: &str, options: &[String]) {
    print_header(title, BlockStatus::Running);
    for (index, option) in options.iter().enumerate() {
        print_kv(&(index + 1).to_string(), option);
    }
}

#[must_use]
pub fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn short_id(agent_id: &str) -> String {
    agent_id.chars().take(6).collect()
}

fn format_duration(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms}ms")
    } else if duration_ms < 60_000 {
        format!("{:.1}s", duration_ms as f64 / 1_000.0)
    } else {
        format!("{:.1}m", duration_ms as f64 / 60_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("计划任务界面", 4), "计划任…");
    }

    #[test]
    fn maps_plan_status_to_shared_words() {
        assert_eq!(status_from_plan(PlanStepStatus::Pending).label(), "queued");
        assert_eq!(status_from_plan(PlanStepStatus::Running).label(), "running");
        assert_eq!(status_from_plan(PlanStepStatus::Done).label(), "done");
        assert_eq!(status_from_plan(PlanStepStatus::Failed).label(), "failed");
    }
}
