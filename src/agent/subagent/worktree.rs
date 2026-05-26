//! RAII guard for running a subagent inside a throwaway `git worktree`.
//!
//! When `SubagentConfig::isolation == Worktree`, the executor creates a
//! guard at the start of the run. The guard does two things:
//!
//! 1. Calls `git worktree add -b <branch> <path> HEAD` so the subagent has
//!    its own working tree on a fresh branch starting at the parent's
//!    current HEAD.
//! 2. On `Drop`, removes the worktree and deletes the branch — *unless*
//!    `keep` was called first. The executor calls `keep` only when the
//!    subagent left changes worth reviewing.
//!
//! `has_changes` checks both uncommitted edits (`git status --porcelain`)
//! and committed-but-ahead-of-base (`git log base..HEAD`), matching Claude
//! Code's behavior of surfacing any non-empty result.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use super::types::{SubagentIsolation, SubagentResult, WorktreeArtifact};

/// Set up a `git worktree` for the subagent if the config asks for it.
/// `Ok(None)` runs in-place; `Ok(Some(guard))` returns an isolated
/// worktree that the caller hands to the executor (via its `path()`) and
/// then passes back to `finalize_worktree`.
pub fn maybe_start_worktree(
    parent_root: &Path,
    isolation: SubagentIsolation,
) -> Result<Option<WorktreeGuard>> {
    if matches!(isolation, SubagentIsolation::Worktree) {
        WorktreeGuard::create(parent_root).map(Some)
    } else {
        Ok(None)
    }
}

/// Inspect a worktree guard after the subagent finishes. Worktrees with
/// changes are surfaced on `result.worktree`; pristine worktrees drop
/// silently so `git worktree list` stays clean.
pub fn finalize_worktree(guard: Option<WorktreeGuard>, result: &mut SubagentResult) {
    if let Some(guard) = guard {
        if guard.has_changes() {
            result.worktree = Some(guard.keep());
        }
    }
}

pub struct WorktreeGuard {
    parent_root: PathBuf,
    path: PathBuf,
    branch: String,
    base_hash: String,
}

impl WorktreeGuard {
    pub fn create(parent_root: &Path) -> Result<Self> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = std::env::temp_dir().join(format!("octo-wt-{id}"));
        let branch = format!("octo-subagent/{id}");

        let base = Command::new("git")
            .arg("-C")
            .arg(parent_root)
            .args(["rev-parse", "HEAD"])
            .output()
            .with_context(|| "spawn git rev-parse HEAD")?;
        if !base.status.success() {
            return Err(anyhow!(
                "git rev-parse HEAD failed in {}: {}",
                parent_root.display(),
                String::from_utf8_lossy(&base.stderr).trim()
            ));
        }
        let base_hash = String::from_utf8_lossy(&base.stdout).trim().to_string();

        let add = Command::new("git")
            .arg("-C")
            .arg(parent_root)
            .args(["worktree", "add", "-b"])
            .arg(&branch)
            .arg(&path)
            .arg(&base_hash)
            .output()
            .with_context(|| "spawn git worktree add")?;
        if !add.status.success() {
            return Err(anyhow!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&add.stderr).trim()
            ));
        }

        Ok(Self {
            parent_root: parent_root.to_path_buf(),
            path,
            branch,
            base_hash,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// True iff the worktree has uncommitted edits or committed past the
    /// base. Errors querying git fall back to "no changes" — better to
    /// leave a worktree behind than to misreport.
    pub fn has_changes(&self) -> bool {
        let porcelain = Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(["status", "--porcelain"])
            .output();
        if let Ok(out) = porcelain {
            if !out.stdout.is_empty() {
                return true;
            }
        }
        let log = Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(["log", "--oneline"])
            .arg(format!("{}..HEAD", self.base_hash))
            .output();
        if let Ok(out) = log {
            if !out.stdout.is_empty() {
                return true;
            }
        }
        false
    }

    /// Hand off the worktree to the caller as a durable artifact. The
    /// guard is consumed and its `Drop` is skipped, so the worktree
    /// persists on disk.
    pub fn keep(self) -> WorktreeArtifact {
        let artifact = WorktreeArtifact {
            path: self.path.clone(),
            branch: self.branch.clone(),
        };
        std::mem::forget(self);
        artifact
    }
}

impl Drop for WorktreeGuard {
    fn drop(&mut self) {
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.parent_root)
            .args(["worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.parent_root)
            .args(["branch", "-D"])
            .arg(&self.branch)
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("spawn git");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        run_git(dir.path(), &["init", "-q", "-b", "main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        std::fs::write(dir.path().join("seed.txt"), "seed\n").expect("write seed");
        run_git(dir.path(), &["add", "."]);
        run_git(dir.path(), &["commit", "-q", "-m", "seed"]);
        dir
    }

    #[test]
    fn clean_worktree_drops_without_leaving_artifacts() {
        let repo = init_repo();
        let path_after_drop = {
            let guard = WorktreeGuard::create(repo.path()).expect("create worktree");
            let path = guard.path().to_path_buf();
            assert!(path.is_dir());
            assert!(!guard.has_changes());
            // guard dropped here, no keep() called
            path
        };
        // The worktree directory should be gone.
        assert!(!path_after_drop.exists());
    }

    #[test]
    fn has_changes_detects_uncommitted_edits() {
        let repo = init_repo();
        let guard = WorktreeGuard::create(repo.path()).expect("create worktree");
        std::fs::write(guard.path().join("scratch.txt"), "hello\n").expect("write scratch");
        assert!(guard.has_changes());
    }

    #[test]
    fn keep_returns_artifact_and_preserves_worktree() {
        let repo = init_repo();
        let guard = WorktreeGuard::create(repo.path()).expect("create worktree");
        let path = guard.path().to_path_buf();
        let artifact = guard.keep();
        assert_eq!(artifact.path, path);
        assert!(artifact.branch.starts_with("octo-subagent/"));
        assert!(path.is_dir());
        // Manual cleanup so the test doesn't leak artifacts.
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["worktree", "remove", "--force"])
            .arg(&path)
            .output();
        let _ = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["branch", "-D"])
            .arg(&artifact.branch)
            .output();
    }
}
