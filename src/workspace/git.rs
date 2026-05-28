use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

const MAX_GIT_STDOUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_GIT_STDERR_BYTES: usize = 64 * 1024;

/// Thin wrapper around git CLI for read-only operations.
/// Mutations require explicit approval from the Policy Engine.
pub fn git_status(project_root: &Path) -> Result<String, anyhow::Error> {
    run_git_read_only(project_root, &["status", "--porcelain"], "git status")
}

pub fn git_diff(
    project_root: &Path,
    staged: bool,
    path: Option<&str>,
) -> Result<String, anyhow::Error> {
    let mut args = vec!["diff"];
    if staged {
        args.push("--staged");
    }
    if let Some(p) = path {
        args.push("--");
        args.push(p);
    }
    run_git_read_only(project_root, &args, "git diff")
}

pub fn git_log(project_root: &Path, count: usize) -> Result<String, anyhow::Error> {
    run_git_read_only(
        project_root,
        &["log", "--oneline", "-n", &count.to_string()],
        "git log",
    )
}

pub fn git_branch(project_root: &Path) -> Result<String, anyhow::Error> {
    run_git_read_only(project_root, &["branch", "--list"], "git branch")
}

#[must_use]
pub fn is_git_repo(project_root: &Path) -> bool {
    project_root.join(".git").exists()
}

struct LimitedGitOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn run_git_read_only(
    project_root: &Path,
    args: &[&str],
    command_name: &str,
) -> Result<String, anyhow::Error> {
    let output = run_limited_git_command(project_root, args)?;
    command_stdout(output, command_name)
}

fn run_limited_git_command(
    project_root: &Path,
    args: &[&str],
) -> Result<LimitedGitOutput, std::io::Error> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture git stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture git stderr"))?;

    let stdout_handle =
        std::thread::spawn(move || read_limited_stream(stdout, MAX_GIT_STDOUT_BYTES));
    let stderr_handle =
        std::thread::spawn(move || read_limited_stream(stderr, MAX_GIT_STDERR_BYTES));
    let status = child.wait()?;
    let (stdout, stdout_truncated) = stdout_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("git stdout reader panicked")))?;
    let (stderr, stderr_truncated) = stderr_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("git stderr reader panicked")))?;

    Ok(LimitedGitOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

fn read_limited_stream<R: std::io::Read>(
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

fn command_stdout(output: LimitedGitOutput, command: &str) -> Result<String, anyhow::Error> {
    if output.status.success() {
        let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
        append_truncation_marker(&mut stdout, output.stdout_truncated, "stdout");
        return Ok(stdout);
    }
    let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
    append_truncation_marker(&mut stderr, output.stderr_truncated, "stderr");
    let message = stderr.trim();
    if message.is_empty() {
        anyhow::bail!("{command} failed with status {}", output.status);
    }
    anyhow::bail!("{command} failed: {message}");
}

fn append_truncation_marker(text: &mut String, truncated: bool, stream: &str) {
    if !truncated {
        return;
    }
    if !text.ends_with('\n') && !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&format!("[git {stream} truncated]"));
}

/// Mutations (require approval):
pub fn git_add(project_root: &Path, paths: &[&str]) -> Result<(), anyhow::Error> {
    let mut args = vec!["add"];
    args.extend(paths);
    let status = Command::new("git")
        .args(&args)
        .current_dir(project_root)
        .status()?;
    if !status.success() {
        anyhow::bail!("git add failed");
    }
    Ok(())
}

pub fn git_commit(project_root: &Path, message: &str) -> Result<(), anyhow::Error> {
    let status = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(project_root)
        .status()?;
    if !status.success() {
        anyhow::bail!("git commit failed");
    }
    Ok(())
}

pub fn git_create_branch(project_root: &Path, name: &str) -> Result<(), anyhow::Error> {
    let status = Command::new("git")
        .args(["checkout", "-b", name])
        .current_dir(project_root)
        .status()?;
    if !status.success() {
        anyhow::bail!("git checkout -b failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_git_commands_surface_failures() {
        let root = tempfile::tempdir().expect("tempdir");

        assert!(git_status(root.path()).is_err());
        assert!(git_diff(root.path(), false, None).is_err());
        assert!(git_log(root.path(), 1).is_err());
        assert!(git_branch(root.path()).is_err());
    }

    #[test]
    fn limited_stream_reader_caps_git_output() {
        let input = std::io::Cursor::new("界".repeat(2000).into_bytes());
        let (bytes, truncated) = read_limited_stream(input, 4096).expect("limited stream");

        assert_eq!(bytes.len(), 4096);
        assert!(truncated);
    }

    #[test]
    fn truncation_marker_is_appended_on_own_line() {
        let mut output = "diff line".to_string();
        append_truncation_marker(&mut output, true, "stdout");

        assert_eq!(output, "diff line\n[git stdout truncated]");
    }
}
