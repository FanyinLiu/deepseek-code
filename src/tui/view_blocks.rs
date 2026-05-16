use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::{motion, theme};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewStatus {
    Queued,
    Running,
    Done,
    Failed,
    Denied,
    Cancelled,
}

impl ViewStatus {
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

    #[must_use]
    pub fn color(self) -> Color {
        let p = theme::palette();
        match self {
            Self::Queued => p.muted,
            Self::Running => p.accent,
            Self::Done => p.success,
            Self::Failed | Self::Denied => p.danger,
            Self::Cancelled => p.dim,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DialogView {
    pub title: String,
    pub status: ViewStatus,
    pub rows: Vec<(String, String)>,
    pub actions: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ToolCallView {
    pub name: String,
    pub status: ViewStatus,
    pub intent: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct TaskReturnView {
    pub status: ViewStatus,
    pub title: String,
    pub summary: String,
    pub metadata: Vec<(String, String)>,
    pub next_action: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkerReportView {
    pub name: String,
    pub status: ViewStatus,
    pub task: String,
    pub metadata: Vec<(String, String)>,
    pub summary: Option<String>,
}

#[must_use]
pub fn status_word(status: ViewStatus) -> Span<'static> {
    status_word_with_motion(status, motion::MotionFrame::disabled())
}

#[must_use]
pub fn status_word_with_motion(status: ViewStatus, frame: motion::MotionFrame) -> Span<'static> {
    let icon = if status == ViewStatus::Running && frame.level.is_enabled() {
        frame.running_icon()
    } else {
        status.icon()
    };
    Span::styled(
        format!("{icon} {}", status.label()),
        Style::default()
            .fg(status.color())
            .add_modifier(Modifier::BOLD),
    )
}

#[must_use]
pub fn kv_line(label: impl Into<String>, value: impl Into<String>) -> Line<'static> {
    let p = theme::palette();
    Line::from(vec![
        Span::styled(format!("{:<9}", label.into()), Style::default().fg(p.muted)),
        Span::styled(value.into(), Style::default().fg(p.text)),
    ])
}

#[must_use]
pub fn header_line(title: impl Into<String>, status: ViewStatus) -> Line<'static> {
    header_line_with_motion(title, status, motion::MotionFrame::disabled())
}

#[must_use]
pub fn header_line_with_motion(
    title: impl Into<String>,
    status: ViewStatus,
    frame: motion::MotionFrame,
) -> Line<'static> {
    let p = theme::palette();
    Line::from(vec![
        status_word_with_motion(status, frame),
        Span::styled("  ", Style::default()),
        Span::styled(
            title.into(),
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        ),
    ])
}

#[must_use]
pub fn divider_line(width: u16) -> Line<'static> {
    Line::from(vec![Span::styled(
        "─".repeat((width as usize).saturating_sub(1)),
        Style::default().fg(theme::palette().divider),
    )])
}

#[must_use]
pub fn render_tool_lines(view: &ToolCallView, max_detail: usize) -> Vec<Line<'static>> {
    vec![compact_tool_line(view, max_detail)]
}

#[must_use]
pub fn render_tool_card_lines(view: &ToolCallView, max_detail: usize) -> Vec<Line<'static>> {
    render_tool_card_lines_with_motion(view, max_detail, motion::MotionFrame::disabled())
}

#[must_use]
pub fn render_tool_card_lines_with_motion(
    view: &ToolCallView,
    max_detail: usize,
    frame: motion::MotionFrame,
) -> Vec<Line<'static>> {
    let p = theme::palette();
    let detail = truncate(&summarize_tool_detail(&view.detail), max_detail);
    let mut lines = vec![Line::from(vec![
        status_word_with_motion(view.status, frame),
        Span::styled("  ", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(
            format!("tool {}", view.name),
            Style::default()
                .fg(p.text)
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        ),
    ])];
    if !detail.trim().is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  └ ", Style::default().fg(p.divider).bg(p.canvas)),
            Span::styled(detail, Style::default().fg(p.dim).bg(p.canvas)),
        ]));
    }
    lines
}

#[must_use]
pub fn compact_tool_line(view: &ToolCallView, max_detail: usize) -> Line<'static> {
    compact_tool_line_with_motion(view, max_detail, motion::MotionFrame::disabled())
}

#[must_use]
pub fn compact_tool_line_with_motion(
    view: &ToolCallView,
    max_detail: usize,
    frame: motion::MotionFrame,
) -> Line<'static> {
    let p = theme::palette();
    let detail = summarize_tool_detail(&view.detail);
    let detail = truncate(&detail, max_detail);
    let title = if detail.trim().is_empty() {
        format!("tool {}", view.name)
    } else {
        format!("tool {}  {}", view.name, detail)
    };
    Line::from(vec![
        status_word_with_motion(view.status, frame),
        Span::styled("  ", Style::default().fg(p.text).bg(p.canvas)),
        Span::styled(
            title,
            Style::default()
                .fg(p.text)
                .bg(p.canvas)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

#[must_use]
pub fn render_task_lines(view: &TaskReturnView, max_detail: usize) -> Vec<Line<'static>> {
    let mut lines = vec![
        header_line(&view.title, view.status),
        kv_line("summary", truncate(&view.summary, max_detail)),
    ];
    for (label, value) in &view.metadata {
        lines.push(kv_line(label, truncate(value, max_detail)));
    }
    if let Some(next) = &view.next_action {
        lines.push(kv_line("next", truncate(next, max_detail)));
    }
    lines
}

#[must_use]
pub fn render_worker_lines(view: &WorkerReportView, max_detail: usize) -> Vec<Line<'static>> {
    render_worker_lines_with_motion(view, max_detail, motion::MotionFrame::disabled())
}

#[must_use]
pub fn render_worker_lines_with_motion(
    view: &WorkerReportView,
    max_detail: usize,
    frame: motion::MotionFrame,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        header_line_with_motion(format!("agent {}", view.name), view.status, frame),
        kv_line("task", truncate(&view.task, max_detail)),
    ];
    for (label, value) in &view.metadata {
        lines.push(kv_line(label, truncate(value, max_detail)));
    }
    if let Some(summary) = &view.summary {
        lines.push(kv_line("summary", truncate(summary, max_detail)));
    }
    lines
}

#[must_use]
pub fn classify_tool(name: &str) -> &'static str {
    match name {
        "read_file" | "list_dir" => "read",
        "search_files" | "search_code" | "semantic_search" => "search",
        "edit_file" | "write_file" | "apply_patch" => "edit",
        "run_command" => "run",
        "git_status" | "git_diff" | "git_log" | "git_add" | "git_commit" => "git",
        "fetch_url" | "web_search" => "fetch",
        name if is_mcp_tool_name(name) => "mcp",
        _ => "tool",
    }
}

fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with("mcp__")
        || name.starts_with("mcp:")
        || name
            .split_once('.')
            .is_some_and(|(server, tool)| !server.is_empty() && !tool.is_empty())
}

#[must_use]
pub fn summarize_tool_arguments(name: &str, arguments: &str) -> String {
    let value = serde_json::from_str::<serde_json::Value>(arguments).ok();
    let Some(value) = value else {
        return truncate(arguments, 120);
    };

    match name {
        "read_file" | "list_dir" | "edit_file" | "write_file" => value["path"]
            .as_str()
            .map_or_else(|| truncate(arguments, 120), ToOwned::to_owned),
        "run_command" => value["command"]
            .as_str()
            .map_or_else(|| truncate(arguments, 120), ToOwned::to_owned),
        "search_files" | "search_code" | "semantic_search" => value["query"]
            .as_str()
            .map_or_else(|| truncate(arguments, 120), ToOwned::to_owned),
        "fetch_url" => value["url"]
            .as_str()
            .map_or_else(|| truncate(arguments, 120), ToOwned::to_owned),
        "git_commit" => value["message"]
            .as_str()
            .map_or_else(|| truncate(arguments, 120), ToOwned::to_owned),
        _ => truncate(arguments, 120),
    }
}

#[must_use]
pub fn summarize_tool_result(result: &str) -> String {
    result
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| summarize_tool_detail(line.trim()))
        .unwrap_or_else(|| "completed with empty output".to_string())
}

#[must_use]
pub fn summarize_tool_detail(detail: &str) -> String {
    let trimmed = detail.trim();
    if let Some(path) = trimmed
        .strip_prefix("--- a/")
        .or_else(|| trimmed.strip_prefix("+++ b/"))
    {
        return format!("changed {path}");
    }
    if trimmed.starts_with("@@") {
        return "diff updated".to_string();
    }
    truncate(trimmed, 120)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("计划任务界面", 4), "计划任…");
    }

    #[test]
    fn summarize_tool_arguments_extracts_command() {
        assert_eq!(
            summarize_tool_arguments("run_command", r#"{"command":"cargo test"}"#),
            "cargo test"
        );
    }

    #[test]
    fn classify_dotted_tool_names_as_mcp() {
        assert_eq!(classify_tool("filesystem.read_file"), "mcp");
        assert_eq!(classify_tool("mcp:filesystem.read_file"), "mcp");
    }

    #[test]
    fn tool_lines_are_compact_without_intent_detail_rows() {
        let lines = render_tool_lines(
            &ToolCallView {
                name: "write_file".to_string(),
                status: ViewStatus::Done,
                intent: "edit result".to_string(),
                detail: "--- a/todo.md".to_string(),
            },
            80,
        );
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(lines.len(), 1);
        assert!(text.contains("tool write_file"));
        assert!(text.contains("changed todo.md"));
        assert!(!text.contains("intent"));
        assert!(!text.contains("detail"));
    }

    #[test]
    fn task_lines_include_metadata_and_next_action() {
        let lines = render_task_lines(
            &TaskReturnView {
                status: ViewStatus::Done,
                title: "task complete".to_string(),
                summary: "fixed input".to_string(),
                metadata: vec![("checks".to_string(), "cargo test passed".to_string())],
                next_action: Some("run clippy".to_string()),
            },
            80,
        );
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.contains("checks"));
        assert!(text.contains("run clippy"));
    }

    #[test]
    fn tool_card_lines_use_status_header_and_detail_tail() {
        let lines = render_tool_card_lines(
            &ToolCallView {
                name: "run_command".to_string(),
                status: ViewStatus::Running,
                intent: "run request".to_string(),
                detail: "cargo test --test tui_cli_tests".to_string(),
            },
            80,
        );
        let text = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert_eq!(lines.len(), 2);
        assert!(text.contains("running"));
        assert!(text.contains("tool run_command"));
        assert!(text.contains("cargo test --test tui_cli_tests"));
        assert!(!text.contains("intent"));
    }
}
