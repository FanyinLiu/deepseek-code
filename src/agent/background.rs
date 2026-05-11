use std::collections::HashMap;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::orchestrator::AgentEvent;
use super::subagent::{SubagentConfig, SubagentResult, SubagentTask};
use super::supervisor::Supervisor;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Background Task System
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

struct BackgroundState {
    entries: HashMap<String, BackgroundTaskEntry>,
    handles: HashMap<String, JoinHandle<()>>,
}

/// A queue for managing background subagent tasks.
///
/// Background tasks run independently of the main agent loop.
/// The user can check their status via `/tasks` or similar commands.
#[derive(Clone)]
pub struct BackgroundQueue {
    state: Arc<Mutex<BackgroundState>>,
}

impl Default for BackgroundQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl BackgroundQueue {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(BackgroundState {
                entries: HashMap::new(),
                handles: HashMap::new(),
            })),
        }
    }

    /// Spawn a background task and return its ID immediately.
    pub fn spawn(
        &self,
        supervisor: Supervisor,
        config: SubagentConfig,
        task: SubagentTask,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> String {
        let task_id = format!("bg-{}", uuid::Uuid::new_v4());

        let entry = BackgroundTaskEntry {
            task_id: task_id.clone(),
            description: task.description.clone(),
            status: TaskStatus::Running,
            result: None,
            started_at: Utc::now(),
            completed_at: None,
        };

        let state = self.state.clone();
        let task_id_clone = task_id.clone();

        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.entries.insert(task_id.clone(), entry);
        }

        let handle: JoinHandle<()> = tokio::spawn(async move {
            let result = supervisor.run_single(config, task, &event_tx).await;

            let mut state = state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(entry) = state.entries.get_mut(&task_id_clone) {
                entry.status = if result.success {
                    TaskStatus::Completed
                } else {
                    TaskStatus::Failed
                };
                entry.result = Some(result);
                entry.completed_at = Some(Utc::now());
            }
            state.handles.remove(&task_id_clone);
        });

        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if state
                .entries
                .get(&task_id)
                .is_some_and(|entry| !entry.status.is_terminal())
            {
                state.handles.insert(task_id.clone(), handle);
            }
        }

        task_id
    }

    /// Get the status of a background task.
    pub fn get_status(&self, task_id: &str) -> Option<TaskStatus> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.entries.get(task_id).map(|e| e.status)
    }

    /// Get a snapshot of a background task.
    pub fn get_task(&self, task_id: &str) -> Option<BackgroundTaskSnapshot> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .entries
            .get(task_id)
            .map(BackgroundTaskEntry::to_snapshot)
    }

    /// List all background tasks.
    pub fn list_tasks(&self) -> Vec<BackgroundTaskSnapshot> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .entries
            .values()
            .map(BackgroundTaskEntry::to_snapshot)
            .collect()
    }

    /// List tasks filtered by status.
    pub fn list_by_status(&self, status: TaskStatus) -> Vec<BackgroundTaskSnapshot> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .entries
            .values()
            .filter(|e| e.status == status)
            .map(BackgroundTaskEntry::to_snapshot)
            .collect()
    }

    /// Cancel a running background task by its ID.
    #[must_use]
    pub fn cancel(&self, task_id: &str) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state
            .entries
            .get(task_id)
            .is_some_and(|entry| entry.status.is_terminal())
        {
            if let Some(handle) = state.handles.remove(task_id) {
                handle.abort();
            }
            return false;
        }
        if let Some(handle) = state.handles.remove(task_id) {
            handle.abort();
            if let Some(entry) = state.entries.get_mut(task_id) {
                entry.status = TaskStatus::Cancelled;
                entry.completed_at = Some(Utc::now());
            }
            true
        } else {
            false
        }
    }

    /// Clean up completed/failed tasks older than the given duration.
    pub fn cleanup_completed(&self, older_than: std::time::Duration) {
        let cutoff = match chrono::Duration::from_std(older_than) {
            Ok(d) => Utc::now() - d,
            Err(e) => {
                tracing::warn!("Invalid duration for cleanup: {}", e);
                return;
            }
        };
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut to_remove = Vec::new();
        state.entries.retain(|id, entry| {
            let keep = match entry.status {
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => {
                    entry.completed_at.is_none_or(|t| t > cutoff)
                }
                _ => true,
            };
            if !keep {
                to_remove.push(id.clone());
            }
            keep
        });
        for id in to_remove {
            state.handles.remove(&id);
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Task Status
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl TaskStatus {
    #[must_use]
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Internal Entry
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

struct BackgroundTaskEntry {
    task_id: String,
    description: String,
    status: TaskStatus,
    result: Option<SubagentResult>,
    started_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

impl BackgroundTaskEntry {
    fn to_snapshot(&self) -> BackgroundTaskSnapshot {
        BackgroundTaskSnapshot {
            task_id: self.task_id.clone(),
            description: self.description.clone(),
            status: self.status,
            success: self.result.as_ref().map(|r| r.success),
            summary: self.result.as_ref().map(|r| r.summary.clone()),
            started_at: self.started_at,
            completed_at: self.completed_at,
            duration_ms: self.result.as_ref().map(|r| r.duration_ms),
        }
    }
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Public Snapshot
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

/// A read-only snapshot of a background task for display.
#[derive(Debug, Clone)]
pub struct BackgroundTaskSnapshot {
    pub task_id: String,
    pub description: String,
    pub status: TaskStatus,
    pub success: Option<bool>,
    pub summary: Option<String>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
}

impl BackgroundTaskSnapshot {
    #[must_use]
    pub fn format_for_display(&self) -> String {
        let status_icon = match self.status {
            TaskStatus::Pending => "⏳",
            TaskStatus::Running => "▶️",
            TaskStatus::Completed => "✅",
            TaskStatus::Failed => "❌",
            TaskStatus::Cancelled => "🚫",
        };
        let mut s = format!(
            "{} [{}] {} (started: {})",
            status_icon,
            self.task_id,
            self.description,
            self.started_at.format("%H:%M:%S")
        );
        if let Some(ref summary) = self.summary {
            let _ = write!(s, "\n   → {summary}");
        }
        if let Some(dur) = self.duration_ms {
            let _ = write!(s, " [{dur}ms]");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Running.to_string(), "running");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn test_snapshot_format() {
        let snap = BackgroundTaskSnapshot {
            task_id: "bg-123".to_string(),
            description: "Review auth.rs".to_string(),
            status: TaskStatus::Completed,
            success: Some(true),
            summary: Some("Found 2 issues".to_string()),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            duration_ms: Some(1500),
        };
        let display = snap.format_for_display();
        assert!(display.contains("Review auth.rs"));
        assert!(display.contains("Found 2 issues"));
        assert!(display.contains("1500ms"));
    }

    #[tokio::test]
    async fn test_cancel_does_not_overwrite_completed_task() {
        let queue = BackgroundQueue::new();
        let task_id = "bg-completed".to_string();
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        });

        {
            let mut state = queue
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.entries.insert(
                task_id.clone(),
                BackgroundTaskEntry {
                    task_id: task_id.clone(),
                    description: "Already completed".to_string(),
                    status: TaskStatus::Completed,
                    result: None,
                    started_at: Utc::now(),
                    completed_at: Some(Utc::now()),
                },
            );
            state.handles.insert(task_id.clone(), handle);
        }

        assert!(!queue.cancel(&task_id));
        assert_eq!(queue.get_status(&task_id), Some(TaskStatus::Completed));
        assert!(!queue
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .handles
            .contains_key(&task_id));
    }
}
