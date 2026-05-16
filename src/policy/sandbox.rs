use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Legacy sandbox limits kept for the public policy module surface.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub timeout_seconds: u64,
    pub network_access: bool,
    pub max_output_bytes: usize,
    pub allowed_directories: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 120,
            network_access: false,
            max_output_bytes: 1_000_000, // 1MB
            allowed_directories: Vec::new(),
        }
    }
}

/// Check if a command should be sandboxed.
#[must_use]
pub fn should_sandbox(command: &str) -> bool {
    let dangerous = [
        "rm -rf /",
        "mkfs.",
        "dd if=",
        "> /dev/sda",
        "chmod 777 /",
        "chown -R",
        ":(){ :|:& };:", // fork bomb
    ];

    let cmd_lower = command.to_lowercase();
    dangerous.iter().any(|d| cmd_lower.contains(d))
}

/// Add network isolation flags for a command (if supported).
/// On Linux, this could use `unshare` or `bwrap`.
/// For v1, we simply warn and require explicit opt-in.
#[must_use]
pub fn network_isolation_args() -> Vec<&'static str> {
    if cfg!(target_os = "linux") {
        vec!["--network=none"]
    } else {
        vec![]
    }
}

/// Controls whether local shell commands are wrapped in an OS sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CommandSandboxMode {
    /// Do not wrap commands.
    #[default]
    Off,
    /// Use a native sandbox when available; fall back to normal execution.
    Auto,
    /// Require a native sandbox and fail if this platform cannot provide one.
    Strict,
}

impl CommandSandboxMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::Strict => "strict",
        }
    }
}

/// Native command sandbox configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSandboxConfig {
    #[serde(default)]
    pub mode: CommandSandboxMode,
    #[serde(default)]
    pub network_access: bool,
    #[serde(default)]
    pub readable_roots: Vec<PathBuf>,
    #[serde(default)]
    pub writable_roots: Vec<PathBuf>,
}

impl Default for CommandSandboxConfig {
    fn default() -> Self {
        Self {
            mode: CommandSandboxMode::Off,
            network_access: false,
            readable_roots: Vec::new(),
            writable_roots: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxInvocation {
    pub program: String,
    pub args: Vec<String>,
}

/// Build a native sandbox wrapper for the shell command.
///
/// The returned invocation contains the sandbox executable and all arguments,
/// including the original shell command. `None` means sandboxing is disabled or
/// unavailable in auto mode.
pub fn command_invocation(
    shell: &str,
    shell_arg: &str,
    command: &str,
    working_dir: &Path,
    config: &CommandSandboxConfig,
) -> Result<Option<SandboxInvocation>, anyhow::Error> {
    match config.mode {
        CommandSandboxMode::Off => Ok(None),
        CommandSandboxMode::Auto | CommandSandboxMode::Strict => {
            platform_command_invocation(shell, shell_arg, command, working_dir, config)
        }
    }
}

#[cfg(target_os = "macos")]
fn platform_command_invocation(
    shell: &str,
    shell_arg: &str,
    command: &str,
    working_dir: &Path,
    config: &CommandSandboxConfig,
) -> Result<Option<SandboxInvocation>, anyhow::Error> {
    let sandbox_exec = Path::new("/usr/bin/sandbox-exec");
    if !sandbox_exec.exists() {
        return handle_missing_sandbox(config.mode, "macOS sandbox-exec is not available");
    }

    let profile = macos_seatbelt_profile(working_dir, config);
    Ok(Some(SandboxInvocation {
        program: sandbox_exec.display().to_string(),
        args: vec![
            "-p".to_string(),
            profile,
            shell.to_string(),
            shell_arg.to_string(),
            command.to_string(),
        ],
    }))
}

#[cfg(target_os = "linux")]
fn platform_command_invocation(
    shell: &str,
    shell_arg: &str,
    command: &str,
    working_dir: &Path,
    config: &CommandSandboxConfig,
) -> Result<Option<SandboxInvocation>, anyhow::Error> {
    let Some(bwrap) = find_program("bwrap") else {
        return handle_missing_sandbox(config.mode, "Linux bubblewrap is not available");
    };

    let mut args = vec![
        "--die-with-parent".to_string(),
        "--unshare-all".to_string(),
        "--proc".to_string(),
        "/proc".to_string(),
        "--dev".to_string(),
        "/dev".to_string(),
        "--tmpfs".to_string(),
        "/tmp".to_string(),
    ];
    if config.network_access {
        args.push("--share-net".to_string());
    }
    for root in default_linux_readable_roots() {
        if root.exists() {
            args.extend([
                "--ro-bind".to_string(),
                root.display().to_string(),
                root.display().to_string(),
            ]);
        }
    }
    push_bwrap_bind(&mut args, "--bind", working_dir, working_dir);
    for root in &config.readable_roots {
        push_bwrap_bind(&mut args, "--ro-bind", root, root);
    }
    for root in &config.writable_roots {
        push_bwrap_bind(&mut args, "--bind", root, root);
    }
    args.extend([
        "--chdir".to_string(),
        working_dir.display().to_string(),
        shell.to_string(),
        shell_arg.to_string(),
        command.to_string(),
    ]);

    Ok(Some(SandboxInvocation {
        program: bwrap.display().to_string(),
        args,
    }))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_command_invocation(
    _shell: &str,
    _shell_arg: &str,
    _command: &str,
    _working_dir: &Path,
    config: &CommandSandboxConfig,
) -> Result<Option<SandboxInvocation>, anyhow::Error> {
    handle_missing_sandbox(
        config.mode,
        "OS command sandbox is not supported on this platform",
    )
}

fn handle_missing_sandbox(
    mode: CommandSandboxMode,
    message: &str,
) -> Result<Option<SandboxInvocation>, anyhow::Error> {
    if mode == CommandSandboxMode::Strict {
        Err(anyhow::anyhow!("{message}"))
    } else {
        Ok(None)
    }
}

#[cfg(target_os = "macos")]
fn macos_seatbelt_profile(working_dir: &Path, config: &CommandSandboxConfig) -> String {
    let mut profile = String::from(
        r#"(version 1)
(deny default)
(allow process*)
(allow signal (target self))
(allow sysctl-read)
(allow mach-lookup)
(allow file-read-metadata)
(allow file-read* (subpath "/bin") (subpath "/sbin") (subpath "/usr") (subpath "/System") (subpath "/Library") (subpath "/private/etc") (subpath "/dev"))
(allow file-write* (subpath "/tmp") (subpath "/private/tmp") (subpath "/private/var/folders"))
"#,
    );

    push_macos_subpath_rule(&mut profile, "file-read*", working_dir);
    push_macos_subpath_rule(&mut profile, "file-write*", working_dir);
    for root in &config.readable_roots {
        push_macos_subpath_rule(&mut profile, "file-read*", root);
    }
    for root in &config.writable_roots {
        push_macos_subpath_rule(&mut profile, "file-read*", root);
        push_macos_subpath_rule(&mut profile, "file-write*", root);
    }
    if config.network_access {
        profile.push_str("(allow network*)\n");
    }
    profile
}

#[cfg(target_os = "macos")]
fn push_macos_subpath_rule(profile: &mut String, operation: &str, path: &Path) {
    let Some(path) = canonical_or_original(path)
        .to_str()
        .map(escape_seatbelt_string)
    else {
        return;
    };
    profile.push_str(&format!("(allow {operation} (subpath \"{path}\"))\n"));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(target_os = "macos")]
fn escape_seatbelt_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "linux")]
fn default_linux_readable_roots() -> Vec<PathBuf> {
    ["/bin", "/usr", "/lib", "/lib64", "/etc"]
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

#[cfg(target_os = "linux")]
fn push_bwrap_bind(args: &mut Vec<String>, flag: &str, source: &Path, dest: &Path) {
    let source = canonical_or_original(source);
    args.extend([
        flag.to_string(),
        source.display().to_string(),
        dest.display().to_string(),
    ]);
}

#[cfg(target_os = "linux")]
fn find_program(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_mode_labels_are_stable() {
        assert_eq!(CommandSandboxMode::Off.as_str(), "off");
        assert_eq!(CommandSandboxMode::Auto.as_str(), "auto");
        assert_eq!(CommandSandboxMode::Strict.as_str(), "strict");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_profile_includes_workspace_and_network_policy() {
        let root = tempfile::tempdir().expect("tempdir");
        let config = CommandSandboxConfig {
            mode: CommandSandboxMode::Auto,
            network_access: true,
            readable_roots: Vec::new(),
            writable_roots: Vec::new(),
        };

        let profile = macos_seatbelt_profile(root.path(), &config);

        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(allow network*)"));
        let canonical = root.path().canonicalize().expect("canonical tempdir");
        assert!(profile.contains(&format!(
            "(allow file-write* (subpath \"{}\"))",
            canonical.display()
        )));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn strict_mode_fails_when_platform_has_no_sandbox() {
        let config = CommandSandboxConfig {
            mode: CommandSandboxMode::Strict,
            ..CommandSandboxConfig::default()
        };

        let err = command_invocation("sh", "-c", "echo ok", Path::new("."), &config)
            .expect_err("strict unsupported sandbox should fail");

        assert!(err.to_string().contains("not supported"));
    }
}
