use std::path::{Path, PathBuf};

use crate::deepseek::{Session, SessionId};

/// Manages session persistence: save, load, list, delete.
pub struct SessionStore {
    base_path: PathBuf,
}

impl SessionStore {
    #[must_use]
    pub fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }

    /// Path: ~/.deepseek-code/sessions/{project_hash}/{session_id}/
    #[must_use]
    pub fn session_dir(&self, project_root: &Path, session_id: &SessionId) -> PathBuf {
        self.base_path
            .join(project_hash(project_root))
            .join(session_id.to_string())
    }

    #[must_use]
    pub fn project_dir(&self, project_root: &Path) -> PathBuf {
        self.base_path.join(project_hash(project_root))
    }

    /// Save a session to disk.
    pub fn save(&self, session: &Session) -> Result<(), anyhow::Error> {
        let dir = self.session_dir(&session.project_root, &session.id);
        std::fs::create_dir_all(&dir)?;

        let session_json = serde_json::to_string_pretty(session)?;
        std::fs::write(dir.join("session.json"), session_json)?;

        // Also write a human-readable transcript
        let transcript = transcript_to_markdown(session);
        std::fs::write(dir.join("transcript.md"), transcript)?;

        // Update index
        self.update_index(session)?;

        Ok(())
    }

    /// Load a session from disk.
    pub fn load(
        &self,
        project_root: &Path,
        session_id: &SessionId,
    ) -> Result<Session, anyhow::Error> {
        let path = self
            .session_dir(project_root, session_id)
            .join("session.json");
        let content = std::fs::read_to_string(&path)?;
        let session: Session = serde_json::from_str(&content)?;
        Ok(session)
    }

    /// List all sessions for a project.
    pub fn list(&self, project_root: &Path) -> Result<Vec<SessionSummary>, anyhow::Error> {
        let index_path = self.project_dir(project_root).join("index.json");
        if !index_path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&index_path)?;
        let summaries: Vec<SessionSummary> = serde_json::from_str(&content)?;
        Ok(summaries)
    }

    /// Delete a session.
    pub fn delete(&self, project_root: &Path, session_id: &SessionId) -> Result<(), anyhow::Error> {
        let dir = self.session_dir(project_root, session_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        self.remove_from_index(project_root, session_id)?;
        Ok(())
    }

    fn update_index(&self, session: &Session) -> Result<(), anyhow::Error> {
        let project_dir = self.project_dir(&session.project_root);
        let lock_path = project_dir.join(".index.lock");
        let lock_file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lock_path)?;
        fs2::FileExt::lock_exclusive(&lock_file)?;

        let mut summaries = self.list(&session.project_root).unwrap_or_default();
        let summary = SessionSummary {
            id: session.id,
            name: session.name.clone(),
            created_at: session.created_at,
            updated_at: session.updated_at,
            message_count: session.messages.len(),
            tool_call_count: session.tool_call_history.len(),
        };

        // Upsert
        if let Some(existing) = summaries.iter_mut().find(|s| s.id == session.id) {
            *existing = summary;
        } else {
            summaries.push(summary);
        }

        // Sort by updated_at descending
        summaries.sort_by_key(|b| std::cmp::Reverse(b.updated_at));

        let index_path = project_dir.join("index.json");
        std::fs::create_dir_all(
            index_path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?,
        )?;
        let tmp_path = index_path.with_extension("tmp");
        std::fs::write(&tmp_path, serde_json::to_string_pretty(&summaries)?)?;
        std::fs::rename(&tmp_path, &index_path)?;

        // lock released when lock_file drops
        Ok(())
    }

    fn remove_from_index(
        &self,
        project_root: &Path,
        session_id: &SessionId,
    ) -> Result<(), anyhow::Error> {
        let project_dir = self.project_dir(project_root);
        let lock_path = project_dir.join(".index.lock");
        let lock_file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&lock_path)?;
        fs2::FileExt::lock_exclusive(&lock_file)?;

        let mut summaries = self.list(project_root).unwrap_or_default();
        summaries.retain(|s| s.id != *session_id);

        let index_path = project_dir.join("index.json");
        let tmp_path = index_path.with_extension("tmp");
        std::fs::write(&tmp_path, serde_json::to_string_pretty(&summaries)?)?;
        std::fs::rename(&tmp_path, &index_path)?;
        // lock released when lock_file drops
        Ok(())
    }
}

fn project_hash(project_root: &Path) -> String {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&project_root.to_string_lossy().to_string(), &mut hasher);
    format!("{:016x}", hasher.finish())
}

fn transcript_to_markdown(session: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Session: {}\n\n",
        session.name.as_deref().unwrap_or("unnamed")
    ));
    out.push_str(&format!("- ID: `{}`\n", session.id));
    out.push_str(&format!(
        "- Project: `{}`\n",
        session.project_root.display()
    ));
    out.push_str(&format!(
        "- Created: {}\n",
        session.created_at.format("%Y-%m-%d %H:%M:%S")
    ));
    out.push_str(&format!(
        "- Updated: {}\n\n",
        session.updated_at.format("%Y-%m-%d %H:%M:%S")
    ));

    for msg in &session.messages {
        if msg.visibility == crate::deepseek::MessageVisibility::AuditOnly {
            continue;
        }
        out.push_str(&format!("## {}\n\n", msg.role));
        out.push_str(&msg.content.to_string_lossy());
        out.push_str("\n\n");
        if !msg.tool_calls.is_empty() {
            for tc in &msg.tool_calls {
                out.push_str(&format!("- Tool: `{}` (`{}`)\n", tc.function.name, tc.id));
            }
            out.push('\n');
        }
        if !msg.tool_results.is_empty() {
            for tr in &msg.tool_results {
                out.push_str(&format!("- Result: {} (error: {})\n", tr.name, tr.is_error));
            }
            out.push('\n');
        }
    }

    // Append tool call history summary
    if !session.tool_call_history.is_empty() {
        out.push_str("---\n## Tool Call History\n\n");
        for tc in &session.tool_call_history {
            out.push_str(&format!(
                "- `{}` at {} — {}\n",
                tc.name,
                tc.at.format("%H:%M:%S"),
                tc.result_summary
            ));
        }
    }

    out
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub message_count: usize,
    pub tool_call_count: usize,
}
