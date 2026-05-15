use std::collections::VecDeque;

use crate::agent::subagent::SubagentResult;

const RECENT_LINES_MAX: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubagentCardStatus {
    Running,
    Done,
    Failed,
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
