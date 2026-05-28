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

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{anyhow, Context, Result};

use super::types::{SubagentIsolation, SubagentResult, WorktreeArtifact};

const WORKTREE_GIT_STDOUT_BYTES: usize = 512 * 1024;
const WORKTREE_GIT_STDERR_BYTES: usize = 64 * 1024;

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

        let base = run_git_limited(parent_root, &["rev-parse", "HEAD"])
            .with_context(|| "spawn git rev-parse HEAD")?;
        if !base.status.success() {
            return Err(anyhow!(
                "git rev-parse HEAD failed in {}: {}",
                parent_root.display(),
                output_text(&base.stderr, base.stderr_truncated, "stderr").trim()
            ));
        }
        let base_hash = output_text(&base.stdout, base.stdout_truncated, "stdout")
            .trim()
            .to_string();

        let path_arg = path.to_string_lossy().to_string();
        let add = run_git_limited(
            parent_root,
            &["worktree", "add", "-b", &branch, &path_arg, &base_hash],
        )
        .with_context(|| "spawn git worktree add")?;
        if !add.status.success() {
            return Err(anyhow!(
                "git worktree add failed: {}",
                output_text(&add.stderr, add.stderr_truncated, "stderr").trim()
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
    /// base. Errors querying git fall back to "has changes" so the caller
    /// leaves the worktree behind for manual inspection instead of deleting it.
    pub fn has_changes(&self) -> bool {
        let Ok(porcelain) = run_git_limited(&self.path, &["status", "--porcelain"]) else {
            return true;
        };
        if !porcelain.status.success() {
            return true;
        }
        if !porcelain.stdout.is_empty() {
            return true;
        }

        let range = format!("{}..HEAD", self.base_hash);
        let Ok(log) = run_git_limited(&self.path, &["log", "--oneline", &range]) else {
            return true;
        };
        if !log.status.success() {
            return true;
        }
        !log.stdout.is_empty()
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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = Command::new("git")
            .arg("-C")
            .arg(&self.parent_root)
            .args(["branch", "-D"])
            .arg(&self.branch)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

struct LimitedGitOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn run_git_limited(repo: &Path, args: &[&str]) -> Result<LimitedGitOutput, std::io::Error> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture worktree git stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture worktree git stderr"))?;

    let stdout_handle =
        std::thread::spawn(move || read_limited_stream(stdout, WORKTREE_GIT_STDOUT_BYTES));
    let stderr_handle =
        std::thread::spawn(move || read_limited_stream(stderr, WORKTREE_GIT_STDERR_BYTES));
    let status = child.wait()?;
    let (stdout, stdout_truncated) = stdout_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("worktree git stdout reader panicked")))?;
    let (stderr, stderr_truncated) = stderr_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("worktree git stderr reader panicked")))?;

    Ok(LimitedGitOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

fn read_limited_stream<R: Read>(
    mut reader: R,
    max_bytes: usize,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut collected = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        let remaining = max_bytes.saturating_sub(collected.len());
        let keep = bytes_read.min(remaining);
        if keep > 0 {
            collected.extend_from_slice(&buffer[..keep]);
        }
        if keep < bytes_read || collected.len() >= max_bytes {
            truncated = true;
        }
    }

    Ok((collected, truncated))
}

fn output_text(bytes: &[u8], truncated: bool, stream: &str) -> String {
    let mut text = String::from_utf8_lossy(bytes).to_string();
    if truncated {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("[worktree git {stream} truncated]"));
    }
    text
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
    fn has_changes_keeps_worktree_when_git_query_fails() {
        let repo = tempfile::tempdir().expect("tempdir");
        let guard = WorktreeGuard {
            parent_root: repo.path().to_path_buf(),
            path: repo.path().join("missing-worktree"),
            branch: "octo-subagent/missing".to_string(),
            base_hash: "HEAD".to_string(),
        };

        assert!(guard.has_changes());
        std::mem::forget(guard);
    }

    #[test]
    fn limited_stream_reader_caps_worktree_git_output() {
        let input = std::io::Cursor::new("x".repeat(8192));
        let (bytes, truncated) = read_limited_stream(input, 1024).expect("limited stream");

        assert_eq!(bytes.len(), 1024);
        assert!(truncated);
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
