use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::deepseek::{SessionId, TurnId};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionEvent {
    pub id: Uuid,
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<TurnId>,
    pub at: DateTime<Utc>,
    #[serde(flatten)]
    pub kind: SessionEventKind,
}

impl SessionEvent {
    #[must_use]
    pub fn new(session_id: SessionId, turn_id: Option<TurnId>, kind: SessionEventKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            turn_id,
            at: Utc::now(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEventKind {
    UserMessage {
        content: String,
    },
    AssistantVisible {
        content: String,
    },
    AssistantInternal {
        content: String,
    },
    ReasoningInternal {
        content: String,
    },
    ToolCallStarted {
        tool_call_id: String,
        name: String,
        arguments: String,
    },
    ToolCallFinished {
        tool_call_id: String,
        name: String,
        success: bool,
        summary: String,
        duration_ms: u64,
        changed_files: Vec<String>,
    },
    UserQuestionRequested {
        tool_call_id: String,
        name: String,
        title: String,
        options: Vec<String>,
        summary: String,
        #[serde(default)]
        descriptions: Vec<String>,
        #[serde(default)]
        previews: Vec<Option<String>>,
        #[serde(default)]
        multi_select: bool,
    },
    ContextCompacted {
        before_tokens: u64,
        after_tokens: u64,
        before_messages: usize,
        after_messages: usize,
        retained_start: usize,
        retained_count: usize,
        summary: String,
        reason: String,
    },
    HookExecuted {
        event: String,
        success: bool,
        summary: String,
        command_count: usize,
    },
    PlanStarted {
        summary: String,
        total: usize,
    },
    PlanStepUpdated {
        index: usize,
        total: usize,
        description: String,
        status: String,
    },
    SwarmStarted {
        run_id: String,
        summary: String,
        total: usize,
    },
    SwarmTaskUpdated {
        run_id: String,
        task_id: String,
        role: String,
        status: String,
        description: String,
    },
    SwarmFinished {
        run_id: String,
        success: bool,
        summary: String,
    },
    ScheduledTaskUpdated {
        task_id: String,
        status: String,
        summary: String,
    },
    SwarmPatchPending {
        run_id: String,
        task_id: String,
        summary: String,
        changed_files: Vec<String>,
        conflict: bool,
    },
    SwarmPatchApplied {
        run_id: String,
        success: bool,
        summary: String,
        changed_files: Vec<String>,
    },
    ApprovalRequested {
        tool_name: String,
        risk_level: String,
        details: String,
    },
    FileChanged {
        path: String,
        stats: String,
    },
    TurnFinished {
        total_tokens: u64,
    },
    Error {
        message: String,
    },
}

#[derive(Clone)]
pub struct EventLogStore {
    base_path: PathBuf,
}

impl EventLogStore {
    #[must_use]
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    #[must_use]
    pub fn events_path(&self, project_root: &Path, session_id: &SessionId) -> PathBuf {
        self.base_path
            .join(crate::storage::project_storage_key(project_root))
            .join(session_id.to_string())
            .join("events.jsonl")
    }

    #[must_use]
    pub fn artifacts_dir(&self, project_root: &Path, session_id: &SessionId) -> PathBuf {
        self.base_path
            .join(crate::storage::project_storage_key(project_root))
            .join(session_id.to_string())
            .join("artifacts")
    }

    pub fn write_artifact(
        &self,
        project_root: &Path,
        session_id: &SessionId,
        slug: &str,
        content: &str,
    ) -> Result<PathBuf, anyhow::Error> {
        let dir = self.artifacts_dir(project_root, session_id);
        std::fs::create_dir_all(&dir)?;
        let filename = format!(
            "{}-{}-{}.md",
            Utc::now().format("%Y%m%dT%H%M%SZ"),
            Uuid::new_v4().simple(),
            artifact_slug(slug)
        );
        let path = dir.join(filename);
        crate::storage::atomic::write_text_atomic(&path, content)?;
        Ok(path)
    }

    pub fn append(&self, project_root: &Path, event: &SessionEvent) -> Result<(), anyhow::Error> {
        let path = self.events_path(project_root, &event.session_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(event)?;
        crate::storage::atomic::append_jsonl_locked(&path, &line)?;
        Ok(())
    }

    pub fn load(
        &self,
        project_root: &Path,
        session_id: &SessionId,
    ) -> Result<Vec<SessionEvent>, anyhow::Error> {
        let path = self.events_path(project_root, session_id);
        let path = if path.exists() {
            path
        } else {
            self.legacy_events_path(project_root, session_id)
        };
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = crate::storage::read_text_file_capped(path)?;
        let non_empty_lines: Vec<_> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let mut events = Vec::new();
        for (index, line) in non_empty_lines.iter().enumerate() {
            match serde_json::from_str(line) {
                Ok(event) => events.push(event),
                Err(error) if index + 1 == non_empty_lines.len() => {
                    tracing::warn!("ignored malformed final session event line: {error}");
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(events)
    }

    pub fn unfinished_turn(
        &self,
        project_root: &Path,
        session_id: &SessionId,
    ) -> Result<Option<TurnId>, anyhow::Error> {
        let events = self.load(project_root, session_id)?;
        Ok(unfinished_turn_from_events(&events))
    }

    pub fn latest_swarm_state(
        &self,
        project_root: &Path,
        session_id: &SessionId,
    ) -> Result<Option<SwarmResumeState>, anyhow::Error> {
        let events = self.load(project_root, session_id)?;
        Ok(latest_swarm_state_from_events(&events))
    }

    fn legacy_events_path(&self, project_root: &Path, session_id: &SessionId) -> PathBuf {
        self.base_path
            .join(crate::storage::legacy_project_storage_key(project_root))
            .join(session_id.to_string())
            .join("events.jsonl")
    }
}

fn artifact_slug(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if (ch.is_whitespace() || matches!(ch, '-' | '_' | '/' | ':' | '.'))
            && !slug.ends_with('-')
        {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "plan".to_string()
    } else {
        slug.to_string()
    }
}

#[must_use]
pub fn unfinished_turn_from_events(events: &[SessionEvent]) -> Option<TurnId> {
    let mut current = None;
    for event in events {
        match event.kind {
            SessionEventKind::UserMessage { .. } => current = event.turn_id,
            SessionEventKind::TurnFinished { .. } if event.turn_id == current => current = None,
            _ => {}
        }
    }
    current
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwarmResumeState {
    pub run_id: String,
    pub summary: String,
    pub total: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub finished: bool,
    pub success: bool,
    pub pending_patches: usize,
    pub applied_patches: usize,
}

impl SwarmResumeState {
    #[must_use]
    pub fn status_label(&self) -> &'static str {
        if self.pending_patches > self.applied_patches {
            "待确认 patch"
        } else if self.cancelled > 0 && !self.success {
            "已取消"
        } else if !self.finished {
            "未完成"
        } else if self.success {
            "已完成"
        } else {
            "失败"
        }
    }
}

#[must_use]
pub fn latest_swarm_state_from_events(events: &[SessionEvent]) -> Option<SwarmResumeState> {
    let last_run_id = events.iter().rev().find_map(|event| match &event.kind {
        SessionEventKind::SwarmStarted { run_id, .. }
        | SessionEventKind::SwarmTaskUpdated { run_id, .. }
        | SessionEventKind::SwarmFinished { run_id, .. }
        | SessionEventKind::SwarmPatchPending { run_id, .. }
        | SessionEventKind::SwarmPatchApplied { run_id, .. } => Some(run_id.clone()),
        _ => None,
    })?;

    let mut state = None;
    for event in events {
        match &event.kind {
            SessionEventKind::SwarmStarted {
                run_id,
                summary,
                total,
            } if *run_id == last_run_id => {
                state = Some(SwarmResumeState {
                    run_id: run_id.clone(),
                    summary: summary.clone(),
                    total: *total,
                    running: 0,
                    done: 0,
                    failed: 0,
                    cancelled: 0,
                    finished: false,
                    success: false,
                    pending_patches: 0,
                    applied_patches: 0,
                });
            }
            SessionEventKind::SwarmTaskUpdated { run_id, status, .. } if *run_id == last_run_id => {
                if let Some(state) = state.as_mut() {
                    match status.as_str() {
                        "running" => state.running += 1,
                        "done" | "completed" => {
                            state.running = state.running.saturating_sub(1);
                            state.done += 1;
                        }
                        "failed" => {
                            state.running = state.running.saturating_sub(1);
                            state.failed += 1;
                        }
                        "cancelled" => {
                            state.running = state.running.saturating_sub(1);
                            state.cancelled += 1;
                        }
                        _ => {}
                    }
                }
            }
            SessionEventKind::SwarmFinished {
                run_id,
                success,
                summary,
            } if *run_id == last_run_id => {
                if let Some(state) = state.as_mut() {
                    state.finished = true;
                    state.success = *success;
                    state.summary = summary.clone();
                    state.running = 0;
                }
            }
            SessionEventKind::SwarmPatchPending { run_id, .. } if *run_id == last_run_id => {
                if let Some(state) = state.as_mut() {
                    state.pending_patches += 1;
                }
            }
            SessionEventKind::SwarmPatchApplied { run_id, .. } if *run_id == last_run_id => {
                if let Some(state) = state.as_mut() {
                    state.applied_patches += 1;
                }
            }
            _ => {}
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_event_jsonl_roundtrips() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = EventLogStore::new(temp.path().to_path_buf());
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();
        let turn_id = TurnId::new_v4();
        let event = SessionEvent::new(
            session_id,
            Some(turn_id),
            SessionEventKind::PlanStarted {
                summary: "测试 CLI".into(),
                total: 3,
            },
        );

        store.append(&root, &event).expect("append event");
        let events = store.load(&root, &session_id).expect("load events");

        assert_eq!(events, vec![event]);
    }

    #[test]
    fn swarm_event_jsonl_roundtrips() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = EventLogStore::new(temp.path().to_path_buf());
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();
        let event = SessionEvent::new(
            session_id,
            None,
            SessionEventKind::SwarmTaskUpdated {
                run_id: "swarm-1".into(),
                task_id: "task-1".into(),
                role: "explorer".into(),
                status: "running".into(),
                description: "读取代码".into(),
            },
        );

        store.append(&root, &event).expect("append event");
        let events = store.load(&root, &session_id).expect("load events");

        assert_eq!(events, vec![event]);
    }

    #[test]
    fn context_compacted_event_jsonl_roundtrips() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = EventLogStore::new(temp.path().to_path_buf());
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();
        let event = SessionEvent::new(
            session_id,
            None,
            SessionEventKind::ContextCompacted {
                before_tokens: 10_000,
                after_tokens: 2_000,
                before_messages: 80,
                after_messages: 20,
                retained_start: 60,
                retained_count: 20,
                summary: "deterministic compact summary".into(),
                reason: "manual /compact".into(),
            },
        );

        store.append(&root, &event).expect("append compact event");
        let events = store.load(&root, &session_id).expect("load events");

        assert_eq!(events, vec![event]);
    }

    #[test]
    fn session_event_concurrent_appends_preserve_all_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = std::sync::Arc::new(EventLogStore::new(temp.path().to_path_buf()));
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();

        let mut handles = Vec::new();
        for index in 0..32 {
            let store = std::sync::Arc::clone(&store);
            let root = root.clone();
            handles.push(std::thread::spawn(move || {
                let event = SessionEvent::new(
                    session_id,
                    None,
                    SessionEventKind::UserMessage {
                        content: format!("message-{index}"),
                    },
                );
                store.append(&root, &event).expect("append event");
            }));
        }
        for handle in handles {
            handle.join().expect("thread joins");
        }

        let events = store.load(&root, &session_id).expect("load events");
        let mut messages = events
            .iter()
            .filter_map(|event| match &event.kind {
                SessionEventKind::UserMessage { content } => Some(content.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        messages.sort();

        assert_eq!(messages.len(), 32);
        assert_eq!(messages.first().map(String::as_str), Some("message-0"));
        assert!(messages.contains(&"message-31".to_string()));
    }

    #[test]
    fn corrupt_final_jsonl_line_keeps_prior_session_events() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = EventLogStore::new(temp.path().to_path_buf());
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();
        let event = SessionEvent::new(
            session_id,
            None,
            SessionEventKind::UserMessage {
                content: "hello".to_string(),
            },
        );
        store.append(&root, &event).expect("append event");
        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(store.events_path(&root, &session_id))
                .expect("open events");
            use std::io::Write;
            writeln!(file, "{{not-json").expect("append broken final line");
        }

        let events = store
            .load(&root, &session_id)
            .expect("load lossy final line");

        assert_eq!(events, vec![event]);
    }

    #[test]
    fn missing_event_log_loads_as_empty() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = EventLogStore::new(temp.path().to_path_buf());

        let events = store
            .load(temp.path(), &SessionId::new_v4())
            .expect("missing log is ok");

        assert!(events.is_empty());
    }

    #[test]
    fn artifact_write_creates_readable_markdown_in_session_dir() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = EventLogStore::new(temp.path().to_path_buf());
        let root = temp.path().join("project");
        let session_id = SessionId::new_v4();

        let path = store
            .write_artifact(&root, &session_id, "执行计划: swarm.rs", "# 执行计划\n")
            .expect("write artifact");

        assert!(path.exists());
        assert!(path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with("swarm-rs.md")));
        assert_eq!(
            std::fs::read_to_string(path).expect("read artifact"),
            "# 执行计划\n"
        );
    }

    #[test]
    fn unfinished_turn_detects_open_turn_without_rerun() {
        let session_id = SessionId::new_v4();
        let turn_id = TurnId::new_v4();
        let events = vec![SessionEvent::new(
            session_id,
            Some(turn_id),
            SessionEventKind::UserMessage {
                content: "改 CLI".into(),
            },
        )];

        assert_eq!(unfinished_turn_from_events(&events), Some(turn_id));

        let mut closed = events;
        closed.push(SessionEvent::new(
            session_id,
            Some(turn_id),
            SessionEventKind::TurnFinished { total_tokens: 42 },
        ));
        assert_eq!(unfinished_turn_from_events(&closed), None);
    }

    #[test]
    fn latest_swarm_state_rebuilds_pending_patch_resume() {
        let session_id = SessionId::new_v4();
        let events = vec![
            SessionEvent::new(
                session_id,
                None,
                SessionEventKind::SwarmStarted {
                    run_id: "swarm-1".into(),
                    summary: "审查 CLI".into(),
                    total: 2,
                },
            ),
            SessionEvent::new(
                session_id,
                None,
                SessionEventKind::SwarmTaskUpdated {
                    run_id: "swarm-1".into(),
                    task_id: "task-1".into(),
                    role: "worker".into(),
                    status: "done".into(),
                    description: "生成补丁".into(),
                },
            ),
            SessionEvent::new(
                session_id,
                None,
                SessionEventKind::SwarmPatchPending {
                    run_id: "swarm-1".into(),
                    task_id: "task-1".into(),
                    summary: "修改 swarm.rs".into(),
                    changed_files: vec!["src/agent/swarm.rs".into()],
                    conflict: false,
                },
            ),
        ];

        let state = latest_swarm_state_from_events(&events).expect("swarm state");

        assert_eq!(state.status_label(), "待确认 patch");
        assert_eq!(state.pending_patches, 1);
        assert_eq!(state.applied_patches, 0);
        assert_eq!(state.done, 1);
    }
}
