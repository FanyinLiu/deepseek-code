use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledTaskKind {
    Heartbeat,
    Standalone,
}

impl ScheduledTaskKind {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Heartbeat => "heartbeat",
            Self::Standalone => "standalone",
        }
    }
}

impl std::str::FromStr for ScheduledTaskKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "heartbeat" | "thread" | "followup" | "follow-up" => Ok(Self::Heartbeat),
            "standalone" | "run" | "job" | "task" => Ok(Self::Standalone),
            other => Err(format!("unknown task kind: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledTaskStatus {
    Active,
    Paused,
}

impl ScheduledTaskStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledTask {
    pub id: String,
    pub kind: ScheduledTaskKind,
    pub status: ScheduledTaskStatus,
    pub prompt: String,
    pub project_root: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_finished_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_log_path: Option<PathBuf>,
}

impl ScheduledTask {
    #[must_use]
    pub fn new(kind: ScheduledTaskKind, prompt: String, project_root: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: format!("task-{}", Uuid::new_v4()),
            kind,
            status: ScheduledTaskStatus::Active,
            prompt,
            project_root,
            created_at: now,
            updated_at: now,
            last_status: None,
            last_started_at: None,
            last_finished_at: None,
            last_log_path: None,
        }
    }

    #[must_use]
    pub fn format_row(&self) -> String {
        format!(
            "{}  {}  {}  {}",
            self.id,
            self.kind.as_str(),
            self.status.as_str(),
            truncate(&self.prompt, 80)
        )
    }
}

#[derive(Debug, Clone)]
pub struct ScheduledTaskStore {
    root: PathBuf,
}

impl ScheduledTaskStore {
    #[must_use]
    pub fn default_user() -> Self {
        let root = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".octocode")
            .join("tasks");
        Self { root }
    }

    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn save(&self, task: &ScheduledTask) -> Result<PathBuf, anyhow::Error> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.path_for(&task.id)?;
        let rendered = toml::to_string_pretty(task)?;
        crate::storage::atomic::write_text_atomic(&path, &rendered)?;
        Ok(path)
    }

    pub fn create(
        &self,
        kind: ScheduledTaskKind,
        prompt: String,
        project_root: PathBuf,
    ) -> Result<ScheduledTask, anyhow::Error> {
        let task = ScheduledTask::new(kind, prompt, project_root);
        self.save(&task)?;
        Ok(task)
    }

    pub fn list(&self) -> Result<Vec<ScheduledTask>, anyhow::Error> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut tasks = Vec::new();
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let content = match crate::storage::read_text_file_capped(&path) {
                Ok(content) => content,
                Err(error) => {
                    tracing::warn!(
                        "skipping unreadable scheduled task {}: {error}",
                        path.display()
                    );
                    continue;
                }
            };
            let task = match toml::from_str::<ScheduledTask>(&content) {
                Ok(task) => task,
                Err(error) => {
                    tracing::warn!(
                        "skipping malformed scheduled task {}: {error}",
                        path.display()
                    );
                    continue;
                }
            };
            if let Err(error) = validate_task_id(&task.id) {
                tracing::warn!(
                    "skipping invalid scheduled task {}: {error}",
                    path.display()
                );
                continue;
            }
            tasks.push(task);
        }
        tasks.sort_by_key(|a| a.created_at);
        Ok(tasks)
    }

    pub fn load(&self, id_or_prefix: &str) -> Result<ScheduledTask, anyhow::Error> {
        let matches: Vec<_> = self
            .list()?
            .into_iter()
            .filter(|task| task.id.starts_with(id_or_prefix))
            .collect();
        match matches.as_slice() {
            [task] => Ok(task.clone()),
            [] => anyhow::bail!("scheduled task not found: {id_or_prefix}"),
            _ => anyhow::bail!("scheduled task prefix is ambiguous: {id_or_prefix}"),
        }
    }

    pub fn set_status(
        &self,
        id_or_prefix: &str,
        status: ScheduledTaskStatus,
    ) -> Result<ScheduledTask, anyhow::Error> {
        let mut task = self.load(id_or_prefix)?;
        task.status = status;
        task.updated_at = Utc::now();
        self.save(&task)?;
        Ok(task)
    }

    pub fn record_run_start(
        &self,
        id_or_prefix: &str,
        log_path: Option<PathBuf>,
    ) -> Result<ScheduledTask, anyhow::Error> {
        let mut task = self.load(id_or_prefix)?;
        task.last_status = Some("running".to_string());
        task.last_started_at = Some(Utc::now());
        task.last_finished_at = None;
        task.last_log_path = log_path;
        task.updated_at = Utc::now();
        self.save(&task)?;
        Ok(task)
    }

    pub fn record_run_finish(
        &self,
        id_or_prefix: &str,
        status: &str,
    ) -> Result<ScheduledTask, anyhow::Error> {
        let mut task = self.load(id_or_prefix)?;
        task.last_status = Some(status.to_string());
        task.last_finished_at = Some(Utc::now());
        task.updated_at = Utc::now();
        self.save(&task)?;
        Ok(task)
    }

    pub fn remove(&self, id_or_prefix: &str) -> Result<ScheduledTask, anyhow::Error> {
        let task = self.load(id_or_prefix)?;
        std::fs::remove_file(self.path_for(&task.id)?)?;
        Ok(task)
    }

    fn path_for(&self, id: &str) -> Result<PathBuf, anyhow::Error> {
        validate_task_id(id)?;
        Ok(self.root.join(format!("{id}.toml")))
    }
}

fn validate_task_id(id: &str) -> Result<(), anyhow::Error> {
    let Some(uuid) = id.strip_prefix("task-") else {
        anyhow::bail!("invalid scheduled task id");
    };
    uuid::Uuid::parse_str(uuid)
        .map(|_| ())
        .map_err(|_| anyhow::anyhow!("invalid scheduled task id"))
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduled_task_roundtrips_and_status_changes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = ScheduledTaskStore::new(dir.path().join("tasks"));
        let task = store
            .create(
                ScheduledTaskKind::Heartbeat,
                "继续检查 CLI".to_string(),
                dir.path().to_path_buf(),
            )
            .expect("create task");

        let loaded = store.load(&task.id[..12]).expect("load by prefix");
        assert_eq!(loaded.kind, ScheduledTaskKind::Heartbeat);
        assert_eq!(loaded.status, ScheduledTaskStatus::Active);

        let paused = store
            .set_status(&task.id, ScheduledTaskStatus::Paused)
            .expect("pause");
        assert_eq!(paused.status, ScheduledTaskStatus::Paused);
    }

    #[test]
    fn scheduled_task_kind_aliases_parse() {
        assert_eq!(
            "thread".parse::<ScheduledTaskKind>().expect("thread"),
            ScheduledTaskKind::Heartbeat
        );
        assert_eq!(
            "job".parse::<ScheduledTaskKind>().expect("job"),
            ScheduledTaskKind::Standalone
        );
    }

    #[test]
    fn scheduled_task_run_status_transitions_are_persisted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = ScheduledTaskStore::new(dir.path().join("tasks"));
        let task = store
            .create(
                ScheduledTaskKind::Standalone,
                "检查 CLI".to_string(),
                dir.path().to_path_buf(),
            )
            .expect("create task");
        let log = dir.path().join("events.jsonl");

        let running = store
            .record_run_start(&task.id, Some(log.clone()))
            .expect("start");
        assert_eq!(running.last_status.as_deref(), Some("running"));
        assert!(running.last_started_at.is_some());
        assert!(running.last_finished_at.is_none());
        assert_eq!(running.last_log_path.as_deref(), Some(log.as_path()));

        let done = store.record_run_finish(&task.id, "done").expect("finish");
        assert_eq!(done.last_status.as_deref(), Some("done"));
        assert!(done.last_finished_at.is_some());

        let failed = store.record_run_finish(&task.id, "failed").expect("fail");
        assert_eq!(failed.last_status.as_deref(), Some("failed"));
    }

    #[test]
    fn scheduled_task_store_rejects_path_traversal_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tasks_dir = dir.path().join("tasks");
        std::fs::create_dir_all(&tasks_dir).expect("create tasks dir");
        let outside = dir.path().join("outside.toml");
        std::fs::write(&outside, "keep").expect("write outside");
        let mut task = ScheduledTask::new(
            ScheduledTaskKind::Heartbeat,
            "继续检查".to_string(),
            dir.path().to_path_buf(),
        );
        task.id = "../outside".to_string();
        std::fs::write(
            tasks_dir.join("malicious.toml"),
            toml::to_string_pretty(&task).expect("serialize malicious task"),
        )
        .expect("write malicious task");
        let store = ScheduledTaskStore::new(tasks_dir);

        let tasks = store.list().expect("list should skip invalid task ids");
        assert!(tasks.is_empty());
        assert!(store.remove("../outside").is_err());
        assert_eq!(
            std::fs::read_to_string(outside).expect("outside remains"),
            "keep"
        );
    }

    #[test]
    fn scheduled_task_save_rejects_invalid_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = ScheduledTaskStore::new(dir.path().join("tasks"));
        let mut task = ScheduledTask::new(
            ScheduledTaskKind::Standalone,
            "检查 CLI".to_string(),
            dir.path().to_path_buf(),
        );
        task.id = "task-not-a-uuid".to_string();

        let err = store
            .save(&task)
            .expect_err("invalid ids should not be saved");
        assert!(err.to_string().contains("invalid scheduled task id"));
    }

    #[test]
    fn scheduled_task_list_skips_malformed_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let tasks_dir = dir.path().join("tasks");
        let store = ScheduledTaskStore::new(tasks_dir.clone());
        let valid = store
            .create(
                ScheduledTaskKind::Standalone,
                "检查 CLI".to_string(),
                dir.path().to_path_buf(),
            )
            .expect("create task");
        std::fs::write(tasks_dir.join("broken.toml"), "id =").expect("write malformed task");

        let tasks = store.list().expect("list should skip malformed task files");

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, valid.id);
    }
}
