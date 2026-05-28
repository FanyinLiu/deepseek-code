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

    /// Path: ~/.octocode/{project_storage_key}/{session_id}/
    #[must_use]
    pub fn session_dir(&self, project_root: &Path, session_id: &SessionId) -> PathBuf {
        self.project_dir(project_root).join(session_id.to_string())
    }

    #[must_use]
    pub fn project_dir(&self, project_root: &Path) -> PathBuf {
        self.base_path
            .join(crate::storage::project_storage_key(project_root))
    }

    /// Save a session to disk.
    pub fn save(&self, session: &Session) -> Result<(), anyhow::Error> {
        let dir = self.session_dir(&session.project_root, &session.id);
        std::fs::create_dir_all(&dir)?;

        let session_json = serde_json::to_string_pretty(session)?;
        crate::storage::atomic::write_text_atomic(&dir.join("session.json"), &session_json)?;

        // Also write a human-readable transcript
        let transcript = transcript_to_markdown(session);
        crate::storage::atomic::write_text_atomic(&dir.join("transcript.md"), &transcript)?;

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
        let path = if path.exists() {
            path
        } else {
            self.legacy_session_dir(project_root, session_id)
                .join("session.json")
        };
        let content = crate::storage::read_text_file_capped(&path)?;
        let session: Session = serde_json::from_str(&content)?;
        Ok(session)
    }

    /// List all sessions for a project.
    pub fn list(&self, project_root: &Path) -> Result<Vec<SessionSummary>, anyhow::Error> {
        let mut summaries = Vec::new();
        for project_dir in self.project_dirs(project_root) {
            let loaded = self.load_or_rebuild_project_index(&project_dir)?;
            for summary in loaded {
                if !summaries
                    .iter()
                    .any(|existing: &SessionSummary| existing.id == summary.id)
                {
                    summaries.push(summary);
                }
            }
        }
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
        Ok(summaries)
    }

    /// Delete a session.
    pub fn delete(&self, project_root: &Path, session_id: &SessionId) -> Result<(), anyhow::Error> {
        let dir = self.session_dir(project_root, session_id);
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        let legacy_dir = self.legacy_session_dir(project_root, session_id);
        if legacy_dir.exists() && legacy_dir != dir {
            std::fs::remove_dir_all(&legacy_dir)?;
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
            .truncate(false)
            .open(&lock_path)?;
        fs2::FileExt::lock_exclusive(&lock_file)?;

        let mut summaries = self.list(&session.project_root)?;
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
        crate::storage::atomic::write_json_pretty_atomic(&index_path, &summaries)?;

        // lock released when lock_file drops
        Ok(())
    }

    fn remove_from_index(
        &self,
        project_root: &Path,
        session_id: &SessionId,
    ) -> Result<(), anyhow::Error> {
        for project_dir in self.project_dirs(project_root) {
            if !project_dir.exists() {
                continue;
            }
            let lock_path = project_dir.join(".index.lock");
            let lock_file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)?;
            fs2::FileExt::lock_exclusive(&lock_file)?;

            let index_path = project_dir.join("index.json");
            let mut summaries: Vec<SessionSummary> = if index_path.exists() {
                serde_json::from_str(&crate::storage::read_text_file_capped(&index_path)?)?
            } else {
                Vec::new()
            };
            summaries.retain(|s| s.id != *session_id);
            crate::storage::atomic::write_json_pretty_atomic(&index_path, &summaries)?;
        }
        Ok(())
    }

    fn legacy_session_dir(&self, project_root: &Path, session_id: &SessionId) -> PathBuf {
        self.base_path
            .join(crate::storage::legacy_project_storage_key(project_root))
            .join(session_id.to_string())
    }

    fn project_dirs(&self, project_root: &Path) -> Vec<PathBuf> {
        crate::storage::project_storage_keys(project_root)
            .into_iter()
            .map(|key| self.base_path.join(key))
            .collect()
    }

    fn load_or_rebuild_project_index(
        &self,
        project_dir: &Path,
    ) -> Result<Vec<SessionSummary>, anyhow::Error> {
        let index_path = project_dir.join("index.json");
        if index_path.exists() {
            match serde_json::from_str::<Vec<SessionSummary>>(
                &crate::storage::read_text_file_capped(&index_path)?,
            ) {
                Ok(summaries) => return Ok(summaries),
                Err(error) => {
                    tracing::warn!(
                        "rebuilding session index after parse failure at {}: {error}",
                        index_path.display()
                    );
                }
            }
        }
        self.rebuild_project_index(project_dir)
    }

    fn rebuild_project_index(
        &self,
        project_dir: &Path,
    ) -> Result<Vec<SessionSummary>, anyhow::Error> {
        if !project_dir.exists() {
            return Ok(Vec::new());
        }
        let mut summaries = Vec::new();
        for entry in std::fs::read_dir(project_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let session_path = path.join("session.json");
            if !session_path.exists() {
                continue;
            }
            let content = match crate::storage::read_text_file_capped(&session_path) {
                Ok(content) => content,
                Err(error) => {
                    tracing::warn!(
                        "skipping unreadable session {}: {error}",
                        session_path.display()
                    );
                    continue;
                }
            };
            let session: Session = match serde_json::from_str(&content) {
                Ok(session) => session,
                Err(error) => {
                    tracing::warn!(
                        "skipping malformed session {}: {error}",
                        session_path.display()
                    );
                    continue;
                }
            };
            summaries.push(SessionSummary {
                id: session.id,
                name: session.name,
                created_at: session.created_at,
                updated_at: session.updated_at,
                message_count: session.messages.len(),
                tool_call_count: session.tool_call_history.len(),
            });
        }
        summaries.sort_by_key(|summary| std::cmp::Reverse(summary.updated_at));
        if !summaries.is_empty() {
            crate::storage::atomic::write_json_pretty_atomic(
                &project_dir.join("index.json"),
                &summaries,
            )?;
        }
        Ok(summaries)
    }
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
