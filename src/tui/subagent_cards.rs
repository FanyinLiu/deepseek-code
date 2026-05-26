use std::collections::VecDeque;

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::agent::subagent::SubagentResult;

use super::{theme, view_blocks};

const RECENT_LINES_MAX: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentCardStatus {
    Running,
    WaitingApproval,
    Retrying,
    Done,
    Failed,
    Blocked,
    Cancelled,
    Skipped,
}

impl SubagentCardStatus {
    #[must_use]
    pub fn view_status(&self) -> view_blocks::ViewStatus {
        match self {
            Self::Running => view_blocks::ViewStatus::Running,
            Self::WaitingApproval => view_blocks::ViewStatus::Waiting,
            Self::Retrying => view_blocks::ViewStatus::Retrying,
            Self::Done => view_blocks::ViewStatus::Done,
            Self::Failed => view_blocks::ViewStatus::Failed,
            Self::Blocked => view_blocks::ViewStatus::Blocked,
            Self::Cancelled => view_blocks::ViewStatus::Cancelled,
            Self::Skipped => view_blocks::ViewStatus::Skipped,
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::WaitingApproval | Self::Retrying)
    }
}

#[derive(Debug, Clone)]
pub struct SubagentCard {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub status: SubagentCardStatus,
    pub start_time: std::time::Instant,
    pub last_update: Option<String>,
    pub recent_lines: VecDeque<String>,
    pub summary: Option<String>,
    pub duration_ms: Option<u64>,
    pub files_read: usize,
    pub files_written: usize,
    pub token_usage: u64,
    pub is_background: bool,
}

impl SubagentCard {
    #[must_use]
    pub fn new(
        agent_id: impl Into<String>,
        agent_type: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            agent_type: agent_type.into(),
            description: description.into(),
            status: SubagentCardStatus::Running,
            start_time: std::time::Instant::now(),
            last_update: None,
            recent_lines: VecDeque::new(),
            summary: None,
            duration_ms: None,
            files_read: 0,
            files_written: 0,
            token_usage: 0,
            is_background: false,
        }
    }

    pub fn apply_delta(&mut self, content: impl Into<String>) {
        if matches!(
            self.status,
            SubagentCardStatus::WaitingApproval | SubagentCardStatus::Retrying
        ) {
            self.status = SubagentCardStatus::Running;
        }
        let content = content.into();
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                if self.recent_lines.len() >= RECENT_LINES_MAX {
                    self.recent_lines.pop_front();
                }
                self.recent_lines.push_back(trimmed.to_string());
                self.last_update = Some(trimmed.to_string());
            }
        }
    }

    pub fn complete(&mut self, result: &SubagentResult) {
        self.status = if result.success {
            SubagentCardStatus::Done
        } else {
            SubagentCardStatus::Failed
        };
        self.summary = Some(result.summary.clone());
        self.duration_ms = Some(result.duration_ms);
        self.files_read = result.files_read.len();
        self.files_written = result.files_written.len();
        self.token_usage = result.token_usage;
    }

    pub fn mark_waiting_approval(&mut self, tool_name: &str) {
        self.status = SubagentCardStatus::WaitingApproval;
        self.last_update = Some(format!("waiting approval for {tool_name}"));
    }

    pub fn mark_blocked(&mut self, reason: &str) {
        self.status = SubagentCardStatus::Blocked;
        self.last_update = Some(format!("blocked: {}", truncate(reason, 96)));
    }
}

/// Render a quiet Droid-style worker report list.
pub fn render_subagent_cards(
    f: &mut Frame,
    area: Rect,
    cards: &[SubagentCard],
    _global_elapsed_ms: u64,
) {
    if cards.is_empty() || area.width < 10 || area.height < 2 {
        return;
    }

    let active = cards.iter().filter(|card| card.status.is_active()).count();

    let mut lines = Vec::new();
    let p = theme::palette();

    let title = if active > 0 {
        format!("agents {active}/{}", cards.len())
    } else {
        format!("agents {}", cards.len())
    };
    lines.push(Line::from(vec![Span::styled(
        title,
        Style::default().fg(p.dim).add_modifier(Modifier::BOLD),
    )]));

    lines.push(divider_line(area.width));

    for card in cards.iter().rev() {
        if lines.len() >= area.height as usize {
            break;
        }
        let icon = agent_icon(&card.agent_type);
        let bg_tag = if card.is_background { " [bg]" } else { "" };
        let view = view_blocks::WorkerReportView {
            name: format!(
                "{icon} {}{bg_tag} {}",
                card.agent_type,
                short_id(&card.agent_id)
            ),
            status: card.status.view_status(),
            task: card.description.clone(),
            metadata: vec![
                ("time".to_string(), card_duration(card)),
                (
                    "files".to_string(),
                    format!("R{} W{}", card.files_read, card.files_written),
                ),
                ("tokens".to_string(), card.token_usage.to_string()),
            ],
            summary: card.summary.clone().or_else(|| card.last_update.clone()),
        };
        for line in view_blocks::render_worker_lines(&view, area.width.saturating_sub(11) as usize)
        {
            if lines.len() >= area.height as usize {
                break;
            }
            lines.push(line);
        }
        // Show last N output lines for running agents
        if card.status == SubagentCardStatus::Running && !card.recent_lines.is_empty() {
            let desc_width = area.width.saturating_sub(6) as usize;
            for recent in &card.recent_lines {
                if lines.len() >= area.height as usize {
                    break;
                }
                lines.push(Line::from(vec![
                    Span::styled("  ╎ ", Style::default().fg(p.divider)),
                    Span::styled(truncate(recent, desc_width), Style::default().fg(p.muted)),
                ]));
            }
        }
        if lines.len() < area.height as usize {
            lines.push(Line::from(""));
        }
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(p.canvas))
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn divider_line(width: u16) -> Line<'static> {
    let w = width as usize;
    Line::from(vec![Span::styled(
        "─".repeat(w.saturating_sub(1)),
        Style::default().fg(theme::palette().divider),
    )])
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn agent_icon(agent_type: &str) -> &'static str {
    match agent_type {
        "code-explorer" => "◈",
        "code-reviewer" => "◇",
        "planner" => "▷",
        "test-runner" => "▶",
        _ => "●",
    }
}

fn short_id(agent_id: &str) -> String {
    agent_id.chars().take(6).collect()
}

fn card_duration(card: &SubagentCard) -> String {
    card.duration_ms
        .map(format_duration)
        .unwrap_or_else(|| format_duration(card.start_time.elapsed().as_millis() as u64))
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
    fn short_id_keeps_cards_scan_friendly() {
        assert_eq!(short_id("019e0b16-bbce-7433"), "019e0b");
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("多智能体任务", 5), "多智能体…");
    }

    #[test]
    fn completed_card_keeps_result_metadata() {
        let mut card = SubagentCard::new("agent-1", "worker", "Implement UI");
        let result = SubagentResult {
            success: true,
            summary: "Updated subagent card rendering".to_string(),
            output: "done".to_string(),
            tool_calls_used: vec!["edit_file".to_string()],
            files_read: vec!["src/tui/app.rs".to_string()],
            files_written: vec!["src/tui/subagent_cards.rs".to_string()],
            duration_ms: 1_250,
            token_usage: 42,
            error: None,
            started_at: chrono::Utc::now(),
            completed_at: chrono::Utc::now(),
            worktree: None,
        };

        card.complete(&result);

        assert_eq!(card.status, SubagentCardStatus::Done);
        assert_eq!(
            card.summary.as_deref(),
            Some("Updated subagent card rendering")
        );
        assert_eq!(card.files_read, 1);
        assert_eq!(card.files_written, 1);
        assert_eq!(card.token_usage, 42);
        assert_eq!(card.duration_ms, Some(1_250));
    }

    #[test]
    fn apply_delta_keeps_last_n_lines() {
        let mut card = SubagentCard::new("a", "worker", "task");
        card.apply_delta("line1\nline2\nline3\nline4");
        assert_eq!(card.recent_lines.len(), 3);
        assert_eq!(card.recent_lines.back().map(String::as_str), Some("line4"));
        assert_eq!(card.recent_lines.front().map(String::as_str), Some("line2"));
    }

    #[test]
    fn agent_icon_maps_known_types() {
        assert_eq!(agent_icon("code-explorer"), "◈");
        assert_eq!(agent_icon("code-reviewer"), "◇");
        assert_eq!(agent_icon("planner"), "▷");
        assert_eq!(agent_icon("test-runner"), "▶");
        assert_eq!(agent_icon("general-purpose"), "●");
    }
}
