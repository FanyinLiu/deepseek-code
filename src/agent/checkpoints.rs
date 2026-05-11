use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use chrono::Utc;

use crate::deepseek::{Checkpoint, FileSnapshot, Session, TurnId};

/// Manages session checkpoints — snapshots of file state before tool execution.
/// Allows rollback of tool-call side effects.
pub struct CheckpointManager;

impl CheckpointManager {
    /// Create a checkpoint before executing potentially destructive tools.
    pub fn create_checkpoint(
        session: &mut Session,
        turn_id: TurnId,
        label: &str,
        project_root: &Path,
    ) -> Result<Checkpoint, anyhow::Error> {
        let checkpoint_id = uuid::Uuid::new_v4();
        let mut file_snapshots = Vec::new();

        // Only snapshot files that exist and are tracked
        for entry in walkdir::WalkDir::new(project_root)
            .max_depth(10)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let relative = path.strip_prefix(project_root).unwrap_or(path);

            // Skip .git and .deepseek-code directories
            if relative.starts_with(".git") || relative.starts_with(".deepseek-code") {
                continue;
            }

            // Skip large files (>1MB)
            if let Ok(meta) = std::fs::metadata(path) {
                if meta.len() > 1_000_000 {
                    continue;
                }
            }

            // Hash the content
            if let Ok(content) = std::fs::read(path) {
                let content_hash = hash_bytes(&content);

                // Store backup for small files (<100KB)
                let backup_path = if content.len() < 100_000 {
                    let backup_dir = project_root
                        .join(".deepseek-code")
                        .join("checkpoints")
                        .join(checkpoint_id.to_string());
                    let backup_file = backup_dir.join(relative);
                    if let Some(parent) = backup_file.parent() {
                        if let Err(e) = std::fs::create_dir_all(parent) {
                            tracing::warn!("failed to create backup dir: {}", e);
                            None
                        } else {
                            match std::fs::write(&backup_file, &content) {
                                Ok(()) => Some(backup_file),
                                Err(e) => {
                                    tracing::warn!(
                                        "failed to backup {}: {}",
                                        relative.display(),
                                        e
                                    );
                                    None
                                }
                            }
                        }
                    } else {
                        tracing::warn!("backup file has no parent directory");
                        None
                    }
                } else {
                    None
                };

                file_snapshots.push(FileSnapshot {
                    path: relative.to_path_buf(),
                    content_hash,
                    backup_path,
                });
            }
        }

        let checkpoint = Checkpoint {
            id: checkpoint_id,
            turn_id,
            label: label.to_string(),
            file_snapshot: file_snapshots,
            created_at: Utc::now(),
        };

        session.checkpoints.push(checkpoint.clone());
        Ok(checkpoint)
    }

    /// Rollback to a checkpoint — restore files from backups.
    pub fn rollback_to(
        session: &Session,
        checkpoint_id: &uuid::Uuid,
        project_root: &Path,
    ) -> Result<(), anyhow::Error> {
        let checkpoint = session
            .checkpoints
            .iter()
            .find(|c| c.id == *checkpoint_id)
            .ok_or_else(|| anyhow::anyhow!("checkpoint not found: {checkpoint_id}"))?;

        for snapshot in &checkpoint.file_snapshot {
            if let Some(ref backup) = snapshot.backup_path {
                let target = project_root.join(&snapshot.path);
                if backup.exists() {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::copy(backup, &target)?;
                }
            }
        }

        Ok(())
    }

    /// Cleanup checkpoints older than a given session.
    pub fn cleanup_checkpoints(session: &Session, project_root: &Path, keep_count: usize) {
        if session.checkpoints.len() <= keep_count {
            return;
        }

        let to_remove = session.checkpoints.len() - keep_count;
        for checkpoint in session.checkpoints.iter().take(to_remove) {
            for snapshot in &checkpoint.file_snapshot {
                if let Some(ref backup) = snapshot.backup_path {
                    let _ = std::fs::remove_file(backup);
                }
            }
            let checkpoint_dir = project_root
                .join(".deepseek-code")
                .join("checkpoints")
                .join(checkpoint.id.to_string());
            let _ = std::fs::remove_dir_all(&checkpoint_dir);
        }
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
