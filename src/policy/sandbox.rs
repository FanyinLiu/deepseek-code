/// Sandbox configuration for command execution.
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
