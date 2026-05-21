use crate::agent::orchestrator::PlanStepStatus;
use crate::agent::subagent::SubagentResult;
use crate::policy::ApprovalDisplay;
use crate::tui::shared_output::{MessageRenderer, RenderedBlock};

use crossterm::{
    style::{Color, Stylize},
    terminal,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub use crate::tui::shared_output::{BlockRole, BlockStatus};

#[must_use]
pub fn status_from_plan(status: PlanStepStatus) -> BlockStatus {
    match status {
        PlanStepStatus::Pending => BlockStatus::Queued,
        PlanStepStatus::Running => BlockStatus::Running,
        PlanStepStatus::Done => BlockStatus::Done,
        PlanStepStatus::Failed => BlockStatus::Failed,
    }
}

pub fn print_header(title: &str, status: BlockStatus) -> RenderedBlock {
    let block = RenderedBlock::new(title, "assistant").with_status(status);
    block.print_rendered();
    block
}

fn print_role_block(title: &str, status: BlockStatus, role: &'static str) -> RenderedBlock {
    let block = RenderedBlock::new(title, role).with_status(status);
    block.print_rendered();
    block
}

pub fn print_kv(label: &str, value: impl AsRef<str>) {
    println!("  {label:<9} {}", value.as_ref());
}

pub fn print_approval(tool_name: &str, display: &ApprovalDisplay, auto_approved: bool) {
    let status = if auto_approved {
        BlockStatus::Done
    } else {
        BlockStatus::Running
    };
    let risk_level = display.risk_level.to_string().to_lowercase();
    let mut lines = Vec::new();
    lines.push(CardLine {
        label: "tool".to_string(),
        value: tool_name.to_string(),
        value_color: None,
    });
    lines.push(CardLine {
        label: "intent".to_string(),
        value: display.description.clone(),
        value_color: None,
    });
    lines.push(CardLine {
        label: "risk".to_string(),
        value: risk_level.clone(),
        value_color: Some(risk_level_color(&risk_level)),
    });
    if !display.details.trim().is_empty() {
        for line in display
            .details
            .lines()
            .filter(|line| !line.trim().is_empty())
        {
            if let Some((label, value)) = line.split_once(':') {
                lines.push(CardLine {
                    label: label.trim().to_string(),
                    value: value.trim().to_string(),
                    value_color: None,
                });
            } else {
                lines.push(CardLine {
                    label: "detail".to_string(),
                    value: line.trim().to_string(),
                    value_color: None,
                });
            }
        }
    }
    if auto_approved {
        lines.push(CardLine {
            label: "action".to_string(),
            value: "approved".to_string(),
            value_color: None,
        });
    } else {
        lines.push(CardLine {
            label: "actions".to_string(),
            value: "approve once / approve session / deny".to_string(),
            value_color: None,
        });
    }
    print_box_block("⚠ approve tool", status, &lines);
}

pub fn print_tool_result(tool_name: &str, success: bool, summary: &str) {
    let status = if success {
        BlockStatus::Done
    } else {
        BlockStatus::Failed
    };
    let block = RenderedBlock::new(format!("tool {tool_name}"), "tool").with_status(status);
    block.print_rendered();

    print_kv("summary", truncate(summary, 120));
}

pub fn print_task_complete(session_id: &str, total_tokens: u64) {
    print_role_block("task complete", BlockStatus::Done, "assistant");
    print_kv("summary", "agent turn finished");
    print_kv("session", session_id);
    print_kv("tokens", total_tokens.to_string());
    print_kv("next", "review output or continue with another task");
}

pub fn print_worker_started(agent_id: &str, agent_type: &str, description: &str) {
    print_role_block(
        &format!("agent {}", short_id(agent_id)),
        BlockStatus::Running,
        "worker",
    );
    print_kv("type", agent_type);
    print_kv("task", truncate(description, 120));
}

pub fn print_worker_completed(agent_id: &str, result: &SubagentResult) {
    print_role_block(
        &format!("agent {}", short_id(agent_id)),
        if result.success {
            BlockStatus::Done
        } else {
            BlockStatus::Failed
        },
        "worker",
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
    print_role_block("plan runner", BlockStatus::Running, "plan");
    print_kv("summary", truncate(summary, 120));
    print_kv("steps", total.to_string());
}

pub fn print_plan_step(index: usize, total: usize, description: &str, status: PlanStepStatus) {
    let status = status_from_plan(status);
    print_role_block(
        &format!("step {}/{}", index.saturating_add(1), total),
        status,
        "plan",
    );
    print_kv("task", truncate(description, 120));
}

pub fn print_plan_complete() {
    print_role_block("plan complete", BlockStatus::Done, "plan");
}

pub fn print_option_block(title: &str, options: &[String]) {
    let lines = options
        .iter()
        .enumerate()
        .map(|(index, option)| CardLine {
            label: format!("{}", index + 1),
            value: option.to_string(),
            value_color: None,
        })
        .collect::<Vec<_>>();
    print_box_block(title, BlockStatus::Running, &lines);
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

pub fn build_rendered_message_block(title: impl Into<String>, role: &'static str) -> RenderedBlock {
    RenderedBlock::new(title, role)
}

#[derive(Debug)]
struct CardLine {
    label: String,
    value: String,
    value_color: Option<Color>,
}

fn print_box_block(title: &str, status: BlockStatus, lines: &[CardLine]) {
    let width = terminal_width();
    let inner_width = width.saturating_sub(2);

    let top = bordered_line(inner_width, format!("{} ", title));
    println!("{}", top.with(header_color(status)));

    for line in lines {
        let label = format!(
            "{:<8}{}",
            line.label,
            if line.value.is_empty() { "" } else { ": " }
        );
        let row_content = format!("{}{}", label, line.value);
        let row = row_text(inner_width, &row_content);
        print_box_row(&row, line.value_color);
    }

    let bottom = format!("╰{}╯", "─".repeat(inner_width));
    println!("{}", bottom.with(header_color(status)));
}

fn print_box_row(content: &str, value_color: Option<Color>) {
    let row = format!("│ {} │", content);
    match value_color {
        Some(color) => println!("{}", row.with(color)),
        None => println!("{}", row),
    }
}

fn row_text(inner_width: usize, text: &str) -> String {
    let available = inner_width.saturating_sub(2);
    let mut output = fit_to_width(text, available);
    if display_width(&output) < available {
        output.push_str(&" ".repeat(available - display_width(&output)));
    }
    output
}

fn bordered_line(inner_width: usize, title: String) -> String {
    let title = fit_to_width(&title, inner_width.saturating_sub(5).max(1));
    let body = format!("─ {title} ");
    let fill = inner_width.saturating_sub(display_width(&body));
    format!("╭{}{}╮", body, "─".repeat(fill))
}

fn terminal_width() -> usize {
    match terminal::size() {
        Ok((columns, _)) => columns.clamp(60, 100) as usize,
        Err(_) => 80,
    }
}

fn header_color(status: BlockStatus) -> Color {
    match status {
        BlockStatus::Running => Color::Blue,
        BlockStatus::Done => Color::Green,
        BlockStatus::Failed => Color::Red,
        _ => Color::DarkGrey,
    }
}

fn risk_level_color(level: &str) -> Color {
    match level.to_lowercase().as_str() {
        "high" | "critical" => Color::Red,
        "medium" => Color::Yellow,
        "low" | "safe" => Color::DarkGrey,
        _ => Color::DarkGrey,
    }
}

fn fit_to_width(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(1);
        if used + char_width > width {
            break;
        }
        output.push(ch);
        used += char_width;
    }
    output
}

fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
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
