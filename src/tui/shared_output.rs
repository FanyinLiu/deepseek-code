//! Shared message block model for both CLI and TUI rendering.

pub const ASSISTANT_PREFIX: &str = "● ";
pub const USER_PREFIX: &str = "◆ ";
pub const SYSTEM_PREFIX: &str = "! ";
pub const TOOL_LOG_PREFIX: &str = "⎿  ";
pub const APPROVAL_PREFIX: &str = "⚑ ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockRole {
    Header,
    Assistant,
    User,
    System,
    Tool,
    Approval,
    Plan,
    Worker,
    Task,
    Diff,
    Subagent,
    Generic,
}

impl BlockRole {
    #[must_use]
    pub fn from_label(label: &str) -> Self {
        match label {
            "assistant" => Self::Assistant,
            "user" => Self::User,
            "system" => Self::System,
            "tool" => Self::Tool,
            "approval" => Self::Approval,
            "plan" => Self::Plan,
            "worker" => Self::Worker,
            "task" => Self::Task,
            "diff" => Self::Diff,
            "subagent" => Self::Subagent,
            "header" => Self::Header,
            _ => Self::Generic,
        }
    }

    pub fn prefix(self) -> &'static str {
        match self {
            Self::Header => "▸",
            Self::Assistant => "●",
            Self::User => "◆",
            Self::System => "!",
            Self::Tool => "⎿",
            Self::Approval => "⚑",
            Self::Plan => "◉",
            Self::Worker => "▪",
            Self::Task => "▸",
            Self::Diff => "▣",
            Self::Subagent => "◌",
            Self::Generic => "•",
        }
    }

    #[must_use]
    pub fn line_prefix(self) -> &'static str {
        match self {
            Self::Assistant => ASSISTANT_PREFIX,
            Self::User => USER_PREFIX,
            Self::System => SYSTEM_PREFIX,
            Self::Tool => TOOL_LOG_PREFIX,
            Self::Approval => APPROVAL_PREFIX,
            _ => "",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RenderLine {
    text: String,
    indent: usize,
}

impl RenderLine {
    #[must_use]
    pub fn new(text: impl Into<String>, indent: usize) -> Self {
        Self {
            text: text.into(),
            indent,
        }
    }

    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, 0)
    }

    fn indent_text(&self) -> String {
        if self.indent == 0 {
            return self.text.clone();
        }
        format!("{}{}", " ".repeat(self.indent), self.text)
    }
}

#[derive(Debug, Clone)]
pub struct RenderedBlock {
    pub role: BlockRole,
    pub title: String,
    pub status: Option<BlockStatus>,
    lines: Vec<RenderLine>,
}

impl RenderedBlock {
    #[must_use]
    pub fn new(title: impl Into<String>, role: &'static str) -> Self {
        Self {
            role: BlockRole::from_label(role),
            title: title.into(),
            status: None,
            lines: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_status(mut self, status: BlockStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn add_line(&mut self, text: impl Into<String>) {
        self.lines.push(RenderLine::plain(text));
    }

    pub fn add_kv(&mut self, label: &str, value: impl AsRef<str>) {
        self.lines
            .push(RenderLine::new(format!("{label:<9} {}", value.as_ref()), 0));
    }

    #[must_use]
    pub fn lines(&self) -> &[RenderLine] {
        &self.lines
    }

    #[must_use]
    pub fn render_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let status_prefix = self.status.map_or(String::new(), |status| {
            format!("{} {}  ", status.icon(), status.label())
        });
        let role_prefix = self.role.prefix();
        let title = if role_prefix.is_empty() {
            self.title.clone()
        } else {
            format!("{role_prefix} {}", self.title)
        };
        out.push(format!("{status_prefix}{title}"));
        for line in &self.lines {
            out.push(line.indent_text());
        }
        out
    }
}

pub trait MessageRenderer {
    fn render_lines(&self) -> Vec<String>;

    fn print_rendered(&self) {
        for line in self.render_lines() {
            println!("{line}");
        }
    }
}

impl MessageRenderer for RenderedBlock {
    fn render_lines(&self) -> Vec<String> {
        self.render_lines()
    }
}

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
            Self::Failed | Self::Denied => "✗",
            Self::Cancelled => "×",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_maps_to_consistent_labels() {
        assert_eq!(BlockStatus::Queued.label(), "queued");
        assert_eq!(BlockStatus::Running.label(), "running");
        assert_eq!(BlockStatus::Done.label(), "done");
        assert_eq!(BlockStatus::Failed.label(), "failed");
    }

    #[test]
    fn roles_map_from_labels() {
        assert_eq!(BlockRole::from_label("assistant"), BlockRole::Assistant);
        assert_eq!(BlockRole::from_label("tool"), BlockRole::Tool);
        assert_eq!(BlockRole::from_label("header"), BlockRole::Header);
        assert_eq!(BlockRole::from_label("unknown"), BlockRole::Generic);
    }

    #[test]
    fn role_prefix_is_included_in_rendered_lines() {
        let mut block =
            RenderedBlock::new("assistant line", "assistant").with_status(BlockStatus::Done);
        block.add_line("content");

        let lines = block.render_lines();
        assert_eq!(lines[0], "◆ done  ● assistant line");
        assert_eq!(lines[1], "content");
    }

    #[test]
    fn protocol_line_prefixes_match_tui_contract() {
        assert_eq!(BlockRole::Assistant.line_prefix(), ASSISTANT_PREFIX);
        assert_eq!(BlockRole::Tool.line_prefix(), TOOL_LOG_PREFIX);
        assert_eq!(BlockRole::System.line_prefix(), SYSTEM_PREFIX);
        assert_eq!(BlockRole::Approval.line_prefix(), APPROVAL_PREFIX);
    }

    #[test]
    fn block_renders_prefixed_lines() {
        let mut block = RenderedBlock::new("task", "task").with_status(BlockStatus::Done);
        block.add_line("one");
        block.add_line("two");

        let lines = block.render_lines();
        assert_eq!(lines, vec!["◆ done  ▸ task", "one", "two"]);
    }
}
