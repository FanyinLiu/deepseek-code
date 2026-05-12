use std::path::Path;

use tokio::sync::mpsc;

use crate::deepseek::{SessionId, TurnId};
use crate::storage::{EventLogStore, SessionEvent, SessionEventKind};

use super::orchestrator::{AgentEvent, PlanStepStatus};

pub struct EventSink<'a> {
    tx: &'a mpsc::UnboundedSender<AgentEvent>,
    store: Option<EventLogStore>,
    project_root: &'a Path,
    session_id: SessionId,
    turn_id: Option<TurnId>,
}

impl<'a> EventSink<'a> {
    #[must_use]
    pub fn new(
        tx: &'a mpsc::UnboundedSender<AgentEvent>,
        store: Option<EventLogStore>,
        project_root: &'a Path,
        session_id: SessionId,
        turn_id: Option<TurnId>,
    ) -> Self {
        Self {
            tx,
            store,
            project_root,
            session_id,
            turn_id,
        }
    }

    pub fn record(&self, kind: SessionEventKind) {
        if let Some(store) = &self.store {
            let event = SessionEvent::new(self.session_id, self.turn_id, kind);
            if let Err(err) = store.append(self.project_root, &event) {
                tracing::warn!("failed to append session event: {err}");
            }
        }
    }

    pub fn emit(&self, event: AgentEvent) {
        self.record_agent_event(&event);
        if self.tx.send(event).is_err() {
            tracing::warn!("Agent event channel closed; event dropped");
        }
    }

    fn record_agent_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::ContentDelta(content) if !content.trim().is_empty() => {
                self.record(SessionEventKind::AssistantVisible {
                    content: content.clone(),
                });
            }
            AgentEvent::ReasoningDelta(content) if !content.trim().is_empty() => {
                self.record(SessionEventKind::ReasoningInternal {
                    content: content.clone(),
                });
            }
            AgentEvent::ToolApprovalNeeded {
                tool_name, display, ..
            } => {
                self.record(SessionEventKind::ApprovalRequested {
                    tool_name: tool_name.clone(),
                    risk_level: display.risk_level.to_string(),
                    details: display.details.clone(),
                });
            }
            AgentEvent::ToolExecuted {
                tool_name,
                success,
                summary,
            } => {
                self.record(SessionEventKind::ToolCallFinished {
                    tool_call_id: String::new(),
                    name: tool_name.clone(),
                    success: *success,
                    summary: summary.clone(),
                    duration_ms: 0,
                    changed_files: Vec::new(),
                });
            }
            AgentEvent::PlanStarted { summary, total } => {
                self.record(SessionEventKind::PlanStarted {
                    summary: summary.clone(),
                    total: *total,
                });
            }
            AgentEvent::PlanStepUpdate {
                index,
                total,
                description,
                status,
            } => {
                self.record(SessionEventKind::PlanStepUpdated {
                    index: *index,
                    total: *total,
                    description: description.clone(),
                    status: plan_status_label(*status).to_string(),
                });
            }
            AgentEvent::SwarmStarted {
                run_id,
                summary,
                total,
            } => {
                self.record(SessionEventKind::SwarmStarted {
                    run_id: run_id.clone(),
                    summary: summary.clone(),
                    total: *total,
                });
            }
            AgentEvent::SwarmTaskUpdated {
                run_id,
                task_id,
                role,
                status,
                description,
            } => {
                self.record(SessionEventKind::SwarmTaskUpdated {
                    run_id: run_id.clone(),
                    task_id: task_id.clone(),
                    role: role.clone(),
                    status: status.clone(),
                    description: description.clone(),
                });
            }
            AgentEvent::SwarmFinished {
                run_id,
                success,
                summary,
            } => {
                self.record(SessionEventKind::SwarmFinished {
                    run_id: run_id.clone(),
                    success: *success,
                    summary: summary.clone(),
                });
            }
            AgentEvent::FileDiff { path, stats, .. } => {
                self.record(SessionEventKind::FileChanged {
                    path: path.clone(),
                    stats: stats.clone(),
                });
            }
            AgentEvent::TurnComplete { total_tokens, .. } => {
                self.record(SessionEventKind::TurnFinished {
                    total_tokens: *total_tokens,
                });
            }
            AgentEvent::Error(message) => {
                self.record(SessionEventKind::Error {
                    message: message.clone(),
                });
            }
            _ => {}
        }
    }
}

fn plan_status_label(status: PlanStepStatus) -> &'static str {
    match status {
        PlanStepStatus::Pending => "pending",
        PlanStepStatus::Running => "running",
        PlanStepStatus::Done => "done",
        PlanStepStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ApprovalDisplay, RiskLevel};

    #[test]
    fn event_sink_records_and_emits() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();
        let turn_id = TurnId::new_v4();
        let store = EventLogStore::new(temp.path().to_path_buf());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let sink = EventSink::new(&tx, Some(store), &root, session_id, Some(turn_id));

        sink.emit(AgentEvent::PlanStarted {
            summary: "测试".into(),
            total: 2,
        });

        assert!(matches!(rx.try_recv(), Ok(AgentEvent::PlanStarted { .. })));
        let store = EventLogStore::new(temp.path().to_path_buf());
        let events = store.load(&root, &session_id).expect("events");
        assert!(matches!(
            events[0].kind,
            SessionEventKind::PlanStarted { .. }
        ));
    }

    #[test]
    fn approval_event_records_details_without_consuming_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();
        let store = EventLogStore::new(temp.path().to_path_buf());
        let (tx, _rx) = mpsc::unbounded_channel();
        let (approval_tx, _approval_rx) = tokio::sync::oneshot::channel();
        let sink = EventSink::new(&tx, Some(store), &root, session_id, None);

        sink.emit(AgentEvent::ToolApprovalNeeded {
            tool_name: "run_command".into(),
            display: ApprovalDisplay {
                title: "run_command".into(),
                description: "cargo test".into(),
                risk_level: RiskLevel::CommandExecution,
                details: "cargo test".into(),
            },
            respond: approval_tx,
        });

        let store = EventLogStore::new(temp.path().to_path_buf());
        let events = store.load(&root, &session_id).expect("events");
        assert!(matches!(
            &events[0].kind,
            SessionEventKind::ApprovalRequested { tool_name, .. } if tool_name == "run_command"
        ));
    }
}
