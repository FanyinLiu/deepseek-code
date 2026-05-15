use std::path::Path;
use std::process::Command;

/// Thin wrapper around git CLI for read-only operations.
/// Mutations require explicit approval from the Policy Engine.
pub fn git_status(project_root: &Path) -> Result<String, anyhow::Error> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_root)
        .output()?;
    command_stdout(output, "git status")
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
    let output = Command::new("git")
        .args(&args)
        .current_dir(project_root)
        .output()?;
    command_stdout(output, "git diff")
}

pub fn git_log(project_root: &Path, count: usize) -> Result<String, anyhow::Error> {
    let output = Command::new("git")
        .args(["log", "--oneline", "-n", &count.to_string()])
        .current_dir(project_root)
        .output()?;
    command_stdout(output, "git log")
}

pub fn git_branch(project_root: &Path) -> Result<String, anyhow::Error> {
    let output = Command::new("git")
        .args(["branch", "--list"])
        .current_dir(project_root)
        .output()?;
    command_stdout(output, "git branch")
}

#[must_use]
pub fn is_git_repo(project_root: &Path) -> bool {
    project_root.join(".git").exists()
}

fn command_stdout(output: std::process::Output, command: &str) -> Result<String, anyhow::Error> {
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    if message.is_empty() {
        anyhow::bail!("{command} failed with status {}", output.status);
    }
    anyhow::bail!("{command} failed: {message}");
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
}
