use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;

use chrono::{DateTime, Utc};

const APPLY_PATCH_STDOUT_BYTES: usize = 256 * 1024;
const APPLY_PATCH_STDERR_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
struct EditRecord {
    turn_id: String,
    path: PathBuf,
    original: Option<String>,
    at: DateTime<Utc>,
}

/// In-memory edit history for undo and checkpoint support.
static EDIT_HISTORY: Mutex<Vec<EditRecord>> = Mutex::new(Vec::new());
static CURRENT_TURN_ID: Mutex<String> = Mutex::new(String::new());

/// Set the current turn ID for subsequent edit tracking.
pub fn set_current_turn_id(id: &str) {
    if let Ok(mut tid) = CURRENT_TURN_ID.lock() {
        *tid = id.to_string();
    }
}

fn push_history(path: PathBuf, original: Option<String>) {
    let turn_id = CURRENT_TURN_ID
        .lock()
        .map(|t| t.clone())
        .unwrap_or_default();
    if let Ok(mut history) = EDIT_HISTORY.lock() {
        history.push(EditRecord {
            turn_id,
            path,
            original,
            at: Utc::now(),
        });
        if history.len() > 100 {
            history.remove(0);
        }
    }
}

/// Undo the most recent file change (edit or write).
/// Returns the relative path of the restored file.
pub fn undo_last_change(project_root: &Path) -> Result<String, anyhow::Error> {
    let mut history = EDIT_HISTORY
        .lock()
        .map_err(|e| anyhow::anyhow!("history lock poisoned: {e}"))?;
    let record = history
        .pop()
        .ok_or_else(|| anyhow::anyhow!("no changes to undo"))?;
    restore_record(project_root, record)
}

/// Undo the last `n` changes. Returns restored relative paths.
pub fn undo_n_changes(project_root: &Path, n: usize) -> Result<Vec<String>, anyhow::Error> {
    let mut history = EDIT_HISTORY
        .lock()
        .map_err(|e| anyhow::anyhow!("history lock poisoned: {e}"))?;
    let mut restored = Vec::new();
    for _ in 0..n {
        let Some(record) = history.pop() else { break };
        restored.push(restore_record(project_root, record)?);
    }
    Ok(restored)
}

fn restore_record(project_root: &Path, record: EditRecord) -> Result<String, anyhow::Error> {
    match record.original {
        Some(original) => std::fs::write(&record.path, original)?,
        None => match std::fs::remove_file(&record.path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        },
    }
    let relative = record
        .path
        .strip_prefix(project_root)
        .unwrap_or(&record.path)
        .to_string_lossy()
        .to_string();
    Ok(relative)
}

/// Get edit history grouped by turn.
pub type TurnHistory = Vec<(PathBuf, DateTime<Utc>)>;

pub fn get_history_by_turn() -> Vec<(String, TurnHistory)> {
    let history = EDIT_HISTORY.lock().map(|h| h.clone()).unwrap_or_default();
    let mut map: std::collections::BTreeMap<String, Vec<(PathBuf, DateTime<Utc>)>> =
        std::collections::BTreeMap::new();
    for record in history {
        map.entry(record.turn_id)
            .or_default()
            .push((record.path, record.at));
    }
    map.into_iter().collect()
}

/// Clear the in-memory edit history.
pub fn clear_history() {
    if let Ok(mut history) = EDIT_HISTORY.lock() {
        history.clear();
    }
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, anyhow::Error> {
    if path.exists() {
        return Ok(std::fs::canonicalize(path)?);
    }

    let mut ancestor = path;
    let mut missing = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("cannot canonicalize path: {}", path.display()))?;
        missing.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| anyhow::anyhow!("cannot canonicalize path: {}", path.display()))?;
    }

    let mut canonical = std::fs::canonicalize(ancestor)?;
    for component in missing.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn resolve_for_write(project_root: &Path, relative_path: &str) -> Result<PathBuf, anyhow::Error> {
    if relative_path.trim().is_empty() {
        anyhow::bail!("path is required");
    }

    let path = Path::new(relative_path);
    if path.is_absolute() {
        anyhow::bail!("absolute paths are not allowed: {relative_path}");
    }
    let mut resolved = project_root.to_path_buf();
    for component in path.components() {
        match component {
            std::path::Component::Normal(c) => resolved.push(c),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !resolved.pop() {
                    anyhow::bail!("path escapes project root: {relative_path}");
                }
            }
            _ => anyhow::bail!("invalid path component in: {relative_path}"),
        }
    }
    let canonical_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let canonical_target = canonicalize_existing_prefix(&resolved)
        .map_err(|e| anyhow::anyhow!("cannot validate path: {e}"))?;
    if !canonical_target.starts_with(&canonical_root) {
        anyhow::bail!("path escapes project root: {relative_path}");
    }
    if crate::policy::paths::is_blocked_path(&resolved, project_root)
        || crate::policy::paths::is_blocked_path(&canonical_target, project_root)
    {
        anyhow::bail!("path is protected: {relative_path}");
    }
    Ok(resolved)
}

/// Apply a string edit to a file: find `old_string` and replace with `new_string`.
/// Returns an error if `old_string` is not unique in the file.
pub fn apply_edit(
    project_root: &Path,
    relative_path: &str,
    old_string: &str,
    new_string: &str,
) -> Result<DiffResult, anyhow::Error> {
    let file_path = resolve_for_write(project_root, relative_path)?;

    // Read original
    let file_existed = file_path.exists();
    let original = if file_existed {
        crate::storage::read_text_file_capped(&file_path)?
    } else {
        String::new()
    };

    // Check uniqueness
    let occurrences = original.matches(old_string).count();
    if occurrences == 0 {
        anyhow::bail!(
            "old_string not found in {relative_path}. Read the file with read_file and copy the \
             exact current text (including whitespace and indentation) instead of reconstructing \
             it from memory."
        );
    }
    if occurrences > 1 {
        anyhow::bail!(
            "old_string found {occurrences} times in {relative_path} — must be unique. Use more surrounding context."
        );
    }

    // Apply
    let modified = original.replacen(old_string, new_string, 1);
    std::fs::create_dir_all(
        file_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?,
    )?;
    std::fs::write(&file_path, &modified)?;
    push_history(file_path.clone(), file_existed.then_some(original.clone()));

    let diff = super::diff::unified_diff(&original, &modified, relative_path);
    let stats = super::diff::diff_stats(&diff);

    Ok(DiffResult {
        path: file_path,
        diff,
        stats,
    })
}

/// Write a full file (create or overwrite).
pub fn apply_write(
    project_root: &Path,
    relative_path: &str,
    content: &str,
) -> Result<DiffResult, anyhow::Error> {
    let file_path = resolve_for_write(project_root, relative_path)?;

    let original = if file_path.exists() {
        Some(crate::storage::read_text_file_capped(&file_path)?)
    } else {
        None
    };
    std::fs::create_dir_all(
        file_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid path: no parent directory"))?,
    )?;
    std::fs::write(&file_path, content)?;
    push_history(file_path.clone(), original.clone());

    let diff = super::diff::unified_diff(original.as_deref().unwrap_or(""), content, relative_path);
    let stats = super::diff::diff_stats(&diff);

    Ok(DiffResult {
        path: file_path,
        diff,
        stats,
    })
}

fn apply_patch_with_git(
    project_root: &Path,
    patch: &str,
    git_program: &str,
) -> Result<(), anyhow::Error> {
    let mut child = Command::new(git_program)
        .args(["apply", "--whitespace=fix"])
        .current_dir(project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "git executable not found in PATH; apply_patch requires git to apply unified diffs"
                )
            } else {
                anyhow::anyhow!("failed to start git apply: {e}")
            }
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(patch.as_bytes())?;
    }

    let output = wait_with_limited_output(child)?;
    if !output.status.success() {
        let stderr = output_text(&output.stderr, output.stderr_truncated, "stderr");
        anyhow::bail!("patch apply failed: {stderr}");
    }

    Ok(())
}

struct LimitedApplyOutput {
    status: ExitStatus,
    stderr: Vec<u8>,
    stderr_truncated: bool,
}

fn wait_with_limited_output(
    mut child: std::process::Child,
) -> Result<LimitedApplyOutput, std::io::Error> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture git apply stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture git apply stderr"))?;

    let stdout_handle =
        std::thread::spawn(move || read_limited_stream(stdout, APPLY_PATCH_STDOUT_BYTES));
    let stderr_handle =
        std::thread::spawn(move || read_limited_stream(stderr, APPLY_PATCH_STDERR_BYTES));
    let status = child.wait()?;
    let (_stdout, _stdout_truncated) = stdout_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("git apply stdout reader panicked")))?;
    let (stderr, stderr_truncated) = stderr_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("git apply stderr reader panicked")))?;

    Ok(LimitedApplyOutput {
        status,
        stderr,
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
        text.push_str(&format!("[git apply {stream} truncated]"));
    }
    text
}

/// Extract affected workspace paths from a patch before it is approved.
///
/// Supports ordinary unified diffs (`diff --git`, `---`, `+++`) and the
/// apply-patch style headers used by coding agents. The returned paths are
/// relative to the workspace when the patch uses `a/` or `b/` prefixes.
#[must_use]
pub fn parse_patch_paths(patch: &str) -> Vec<String> {
    fn normalize_path(raw: &str) -> Option<String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "/dev/null" {
            return None;
        }

        let path = trimmed.split('\t').next().unwrap_or(trimmed).trim();
        if path == "/dev/null" {
            return None;
        }

        let path = path
            .strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path);

        (!path.is_empty()).then(|| path.to_string())
    }

    fn push_path(paths: &mut Vec<String>, raw: &str) {
        if let Some(path) = normalize_path(raw) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    let mut paths = Vec::new();
    for line in patch.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let mut parts = rest.split_whitespace();
            if let Some(old_path) = parts.next() {
                push_path(&mut paths, old_path);
            }
            if let Some(new_path) = parts.next() {
                push_path(&mut paths, new_path);
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("rename from ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("copy from ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("copy to ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("*** Add File: ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("*** Update File: ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("*** Delete File: ") {
            push_path(&mut paths, rest);
        } else if let Some(rest) = line.strip_prefix("*** Move to: ") {
            push_path(&mut paths, rest);
        }
    }

    paths
}

/// Apply a unified diff patch using `git apply`.
/// This is safer than manual parsing.
pub fn apply_patch(project_root: &Path, patch: &str) -> Result<(), anyhow::Error> {
    // Enforce the workspace boundary ourselves instead of relying solely on
    // git apply's path handling: reject any patch that targets an absolute
    // path, escapes the project via `..`, or hits a protected path — the same
    // guard write_file/edit_file already apply.
    for path in parse_patch_paths(patch) {
        resolve_for_write(project_root, &path)
            .map_err(|error| anyhow::anyhow!("patch rejected: {error}"))?;
    }
    apply_patch_with_git(project_root, patch, "git")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static HISTORY_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn history_test_guard() -> MutexGuard<'static, ()> {
        HISTORY_TEST_LOCK.lock().expect("history test lock")
    }

    #[test]
    fn apply_patch_reports_missing_git_clearly() {
        let root = tempfile::tempdir().expect("tempdir");
        let err = apply_patch_with_git(root.path(), "", "definitely-missing-git-binary")
            .expect_err("missing git should fail");
        assert!(
            err.to_string().contains("git executable not found in PATH"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_patch_rejects_paths_escaping_the_workspace() {
        let root = tempfile::tempdir().expect("tempdir");
        let escaping = "\
diff --git a/../../etc/evil b/../../etc/evil
--- a/../../etc/evil
+++ b/../../etc/evil
@@ -0,0 +1 @@
+pwned
";
        let err = apply_patch(root.path(), escaping).expect_err("escape must be rejected");
        assert!(
            err.to_string().contains("patch rejected"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_patch_rejects_absolute_paths() {
        let root = tempfile::tempdir().expect("tempdir");
        let absolute = "\
--- /dev/null
+++ /etc/cron.d/evil
@@ -0,0 +1 @@
+pwned
";
        let err = apply_patch(root.path(), absolute).expect_err("absolute path must be rejected");
        assert!(
            err.to_string().contains("patch rejected"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_patch_paths_extracts_unified_diff_paths() {
        let patch = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1 +1 @@
-old
+new
diff --git a/old.txt b/new.txt
rename from old.txt
rename to new.txt
";

        assert_eq!(
            parse_patch_paths(patch),
            vec![
                "src/main.rs".to_string(),
                "old.txt".to_string(),
                "new.txt".to_string(),
            ]
        );
    }

    #[test]
    fn parse_patch_paths_extracts_apply_patch_headers() {
        let patch = "\
*** Begin Patch
*** Update File: src/lib.rs
*** Add File: tests/new_test.rs
*** Delete File: old.rs
*** End Patch
";

        assert_eq!(
            parse_patch_paths(patch),
            vec![
                "src/lib.rs".to_string(),
                "tests/new_test.rs".to_string(),
                "old.rs".to_string(),
            ]
        );
    }

    #[test]
    fn undo_write_removes_new_file() {
        let _guard = history_test_guard();
        clear_history();
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("new.txt");

        apply_write(root.path(), "new.txt", "created").expect("write file");
        assert!(path.exists());

        let restored = undo_last_change(root.path()).expect("undo write");

        assert_eq!(restored, "new.txt");
        assert!(!path.exists());
        clear_history();
    }

    #[test]
    fn failed_edit_does_not_add_undo_history() {
        let _guard = history_test_guard();
        clear_history();
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("tracked.txt");
        std::fs::write(&path, "original").expect("write original");

        let error = apply_edit(root.path(), "tracked.txt", "missing", "updated")
            .expect_err("edit should fail");

        assert!(error.to_string().contains("old_string not found"));
        let undo_error = undo_last_change(root.path()).expect_err("history should be empty");
        assert_eq!(undo_error.to_string(), "no changes to undo");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read original"),
            "original"
        );
        clear_history();
    }

    #[test]
    fn failed_write_does_not_add_undo_history() {
        let _guard = history_test_guard();
        clear_history();
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(root.path().join("parent"), "not a directory").expect("write parent file");

        let error = apply_write(root.path(), "parent/child.txt", "content")
            .expect_err("write should fail because parent is a file");

        assert!(
            error.to_string().contains("Not a directory")
                || error.to_string().contains("File exists")
                || error.to_string().contains("not a directory"),
            "unexpected error: {error}"
        );
        let undo_error = undo_last_change(root.path()).expect_err("history should be empty");
        assert_eq!(undo_error.to_string(), "no changes to undo");
        assert_eq!(
            std::fs::read_to_string(root.path().join("parent")).expect("read parent file"),
            "not a directory"
        );
        clear_history();
    }

    #[test]
    fn limited_stream_reader_caps_git_apply_output() {
        let input = std::io::Cursor::new("x".repeat(8192));
        let (bytes, truncated) = read_limited_stream(input, 1024).expect("limited stream");

        assert_eq!(bytes.len(), 1024);
        assert!(truncated);
    }

    #[test]
    fn output_text_marks_git_apply_truncation() {
        let output = output_text(b"error", true, "stderr");

        assert!(output.contains("error"));
        assert!(output.contains("[git apply stderr truncated]"));
    }
}

#[derive(Debug, Clone)]
pub struct DiffResult {
    pub path: PathBuf,
    pub diff: String,
    pub stats: super::diff::DiffStats,
}
