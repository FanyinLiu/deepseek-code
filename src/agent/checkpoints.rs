use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

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
        let file_snapshots = collect_file_snapshots(checkpoint_id, project_root);
        let checkpoint =
            build_checkpoint(checkpoint_id, turn_id, label.to_string(), file_snapshots);

        session.checkpoints.push(checkpoint.clone());
        Ok(checkpoint)
    }

    /// Create a checkpoint without blocking the async runtime on filesystem work.
    pub async fn create_checkpoint_async(
        session: &mut Session,
        turn_id: TurnId,
        label: &str,
        project_root: &Path,
    ) -> Result<Checkpoint, anyhow::Error> {
        let checkpoint_id = uuid::Uuid::new_v4();
        let label = label.to_string();
        let project_root = project_root.to_path_buf();
        let file_snapshots = tokio::task::spawn_blocking(move || {
            collect_file_snapshots(checkpoint_id, &project_root)
        })
        .await
        .map_err(|e| anyhow::anyhow!("checkpoint creation task failed: {e}"))?;
        let checkpoint = build_checkpoint(checkpoint_id, turn_id, label, file_snapshots);

        session.checkpoints.push(checkpoint.clone());
        Ok(checkpoint)
    }

    /// Rollback to a checkpoint — restore files from backups.
    pub fn rollback_to(
        session: &Session,
        checkpoint_id: &uuid::Uuid,
        project_root: &Path,
    ) -> Result<(), anyhow::Error> {
        let checkpoint = find_checkpoint(session, checkpoint_id)?;
        restore_checkpoint(checkpoint, project_root)
    }

    /// Rollback to a checkpoint without blocking the async runtime on filesystem work.
    pub async fn rollback_to_async(
        session: &Session,
        checkpoint_id: &uuid::Uuid,
        project_root: &Path,
    ) -> Result<(), anyhow::Error> {
        let checkpoint = find_checkpoint(session, checkpoint_id)?.clone();
        let project_root = project_root.to_path_buf();
        tokio::task::spawn_blocking(move || restore_checkpoint(&checkpoint, &project_root))
            .await
            .map_err(|e| anyhow::anyhow!("checkpoint rollback task failed: {e}"))?
    }

    /// Cleanup checkpoints older than a given session.
    pub fn cleanup_checkpoints(session: &Session, project_root: &Path, keep_count: usize) {
        let stale_checkpoints = stale_checkpoints(session, keep_count);
        cleanup_checkpoint_backups(&stale_checkpoints, project_root);
    }

    /// Cleanup checkpoints without blocking the async runtime on filesystem work.
    pub async fn cleanup_checkpoints_async(
        session: &Session,
        project_root: &Path,
        keep_count: usize,
    ) -> Result<(), anyhow::Error> {
        let stale_checkpoints = stale_checkpoints(session, keep_count);
        if stale_checkpoints.is_empty() {
            return Ok(());
        }
        let project_root = project_root.to_path_buf();
        tokio::task::spawn_blocking(move || {
            cleanup_checkpoint_backups(&stale_checkpoints, &project_root);
        })
        .await
        .map_err(|e| anyhow::anyhow!("checkpoint cleanup task failed: {e}"))?;
        Ok(())
    }
}

fn build_checkpoint(
    checkpoint_id: uuid::Uuid,
    turn_id: TurnId,
    label: String,
    file_snapshots: Vec<FileSnapshot>,
) -> Checkpoint {
    Checkpoint {
        id: checkpoint_id,
        turn_id,
        label,
        file_snapshot: file_snapshots,
        created_at: Utc::now(),
    }
}

fn collect_file_snapshots(checkpoint_id: uuid::Uuid, project_root: &Path) -> Vec<FileSnapshot> {
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

        // Skip .git and .octocode directories
        if relative.starts_with(".git") || relative.starts_with(".octocode") {
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
            let backup_path = backup_small_file(checkpoint_id, project_root, relative, &content);

            file_snapshots.push(FileSnapshot {
                path: relative.to_path_buf(),
                content_hash,
                backup_path,
            });
        }
    }

    file_snapshots
}

fn backup_small_file(
    checkpoint_id: uuid::Uuid,
    project_root: &Path,
    relative: &Path,
    content: &[u8],
) -> Option<PathBuf> {
    if content.len() >= 100_000 {
        return None;
    }

    let backup_dir = project_root
        .join(".octocode")
        .join("checkpoints")
        .join(checkpoint_id.to_string());
    let backup_file = backup_dir.join(relative);
    if let Some(parent) = backup_file.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("failed to create backup dir: {}", e);
            None
        } else {
            match std::fs::write(&backup_file, content) {
                Ok(()) => Some(backup_file),
                Err(e) => {
                    tracing::warn!("failed to backup {}: {}", relative.display(), e);
                    None
                }
            }
        }
    } else {
        tracing::warn!("backup file has no parent directory");
        None
    }
}

fn find_checkpoint<'a>(
    session: &'a Session,
    checkpoint_id: &uuid::Uuid,
) -> Result<&'a Checkpoint, anyhow::Error> {
    session
        .checkpoints
        .iter()
        .find(|c| c.id == *checkpoint_id)
        .ok_or_else(|| anyhow::anyhow!("checkpoint not found: {checkpoint_id}"))
}

fn restore_checkpoint(checkpoint: &Checkpoint, project_root: &Path) -> Result<(), anyhow::Error> {
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

fn stale_checkpoints(session: &Session, keep_count: usize) -> Vec<Checkpoint> {
    if session.checkpoints.len() <= keep_count {
        return Vec::new();
    }

    let to_remove = session.checkpoints.len() - keep_count;
    session
        .checkpoints
        .iter()
        .take(to_remove)
        .cloned()
        .collect()
}

fn cleanup_checkpoint_backups(checkpoints: &[Checkpoint], project_root: &Path) {
    for checkpoint in checkpoints {
        for snapshot in &checkpoint.file_snapshot {
            if let Some(ref backup) = snapshot.backup_path {
                let _ = std::fs::remove_file(backup);
            }
        }
        let checkpoint_dir = project_root
            .join(".octocode")
            .join("checkpoints")
            .join(checkpoint.id.to_string());
        let _ = std::fs::remove_dir_all(&checkpoint_dir);
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek::{ReasoningState, SessionId, SessionMetadata};

    fn test_session(project_root: &Path) -> Session {
        Session {
            id: SessionId::new_v4(),
            name: None,
            project_root: project_root.to_path_buf(),
            messages: Vec::new(),
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: SessionMetadata::default(),
        }
    }

    #[tokio::test]
    async fn async_checkpoint_create_and_rollback_restores_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let tracked_file = root.path().join("tracked.txt");
        std::fs::write(&tracked_file, "original").expect("write original");
        let mut session = test_session(root.path());

        let checkpoint = CheckpointManager::create_checkpoint_async(
            &mut session,
            TurnId::new_v4(),
            "before edit",
            root.path(),
        )
        .await
        .expect("create checkpoint");

        assert_eq!(session.checkpoints.len(), 1);
        assert!(checkpoint.file_snapshot.iter().any(|snapshot| snapshot.path
            == Path::new("tracked.txt")
            && snapshot
                .backup_path
                .as_ref()
                .is_some_and(|path| path.exists())));

        std::fs::write(&tracked_file, "modified").expect("write modified");
        CheckpointManager::rollback_to_async(&session, &checkpoint.id, root.path())
            .await
            .expect("rollback checkpoint");

        let restored = std::fs::read_to_string(&tracked_file).expect("read restored");
        assert_eq!(restored, "original");
    }

    #[tokio::test]
    async fn async_checkpoint_cleanup_removes_old_backup_dirs() {
        let root = tempfile::tempdir().expect("tempdir");
        let tracked_file = root.path().join("tracked.txt");
        std::fs::write(&tracked_file, "first").expect("write first");
        let mut session = test_session(root.path());

        let first = CheckpointManager::create_checkpoint_async(
            &mut session,
            TurnId::new_v4(),
            "first",
            root.path(),
        )
        .await
        .expect("create first checkpoint");
        std::fs::write(&tracked_file, "second").expect("write second");
        let second = CheckpointManager::create_checkpoint_async(
            &mut session,
            TurnId::new_v4(),
            "second",
            root.path(),
        )
        .await
        .expect("create second checkpoint");

        let first_dir = root
            .path()
            .join(".octocode")
            .join("checkpoints")
            .join(first.id.to_string());
        let second_dir = root
            .path()
            .join(".octocode")
            .join("checkpoints")
            .join(second.id.to_string());
        assert!(first_dir.exists());
        assert!(second_dir.exists());

        CheckpointManager::cleanup_checkpoints_async(&session, root.path(), 1)
            .await
            .expect("cleanup checkpoints");

        assert!(!first_dir.exists());
        assert!(second_dir.exists());
    }
}
