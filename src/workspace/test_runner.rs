//! Automatic test detection and execution after file changes.
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

const TEST_RUNNER_STDOUT_BYTES: usize = 2 * 1024 * 1024;
const TEST_RUNNER_STDERR_BYTES: usize = 512 * 1024;

/// Detect the test framework used in the project and run relevant tests.
/// Returns a human-readable summary of test results.
pub fn detect_and_run_tests(project_root: &Path) -> Result<String, anyhow::Error> {
    let framework = detect_test_framework(project_root);
    match framework {
        Some(TestFramework::Cargo) => run_cargo_tests(project_root),
        Some(TestFramework::Pytest) => run_pytest(project_root),
        Some(TestFramework::Npm) => run_npm_tests(project_root),
        Some(TestFramework::Go) => run_go_tests(project_root),
        None => Ok("No test framework detected".to_string()),
    }
}

#[derive(Debug, Clone, Copy)]
enum TestFramework {
    Cargo,
    Pytest,
    Npm,
    Go,
}

fn detect_test_framework(project_root: &Path) -> Option<TestFramework> {
    if project_root.join("Cargo.toml").exists() {
        return Some(TestFramework::Cargo);
    }
    if project_root.join("pytest.ini").exists()
        || project_root.join("setup.py").exists()
        || project_root.join("pyproject.toml").exists()
    {
        return Some(TestFramework::Pytest);
    }
    if project_root.join("package.json").exists() {
        return Some(TestFramework::Npm);
    }
    if project_root.join("go.mod").exists() {
        return Some(TestFramework::Go);
    }
    None
}

fn run_cargo_tests(project_root: &Path) -> Result<String, anyhow::Error> {
    let output = run_limited_command(project_root, "cargo", &["test", "--color=never"])?;

    let stdout = stream_text(&output.stdout, output.stdout_truncated, "stdout");
    let stderr = stream_text(&output.stderr, output.stderr_truncated, "stderr");
    let mut result = String::new();

    // Extract test summary lines
    let summary_lines: Vec<&str> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|l| l.contains("test result:") || l.contains("running ") || l.contains("test "))
        .collect();

    if summary_lines.is_empty() {
        result.push_str("cargo test output:\n");
        result.push_str(&stdout);
        if !stderr.is_empty() {
            result.push_str("stderr:\n");
            result.push_str(&stderr);
        }
    } else {
        result.push_str("Test results:\n");
        for line in &summary_lines {
            result.push_str(line);
            result.push('\n');
        }
    }

    if output.status.success() {
        result.push_str("\n✓ All tests passed");
    } else {
        result.push_str("\n✗ Some tests failed");
    }

    Ok(result)
}

/// Run a quick self-verification check (cargo check, pytest, npm test, go test).
/// Returns a concise summary. This is lighter than full test detection — it runs
/// the fastest appropriate validation for the project type.
pub fn run_self_verification(project_root: &Path) -> Result<String, anyhow::Error> {
    if project_root.join("Cargo.toml").exists() {
        let output = run_limited_command(project_root, "cargo", &["check", "--color=never"])?;
        let stderr = stream_text(&output.stderr, output.stderr_truncated, "stderr");
        if output.status.success() {
            Ok("cargo check: ✓ clean".to_string())
        } else {
            let msg = truncate_chars(&stderr, 300, "");
            Ok(format!("cargo check: ✗ errors\n{msg}"))
        }
    } else if project_root.join("package.json").exists() {
        let output = run_limited_command(project_root, "npm", &["test", "--color=false"])?;
        if output.status.success() {
            Ok("npm test: ✓ passed".to_string())
        } else {
            Ok("npm test: ✗ failed".to_string())
        }
    } else if project_root.join("go.mod").exists() {
        let output = run_limited_command(project_root, "go", &["test", "./..."])?;
        if output.status.success() {
            Ok("go test: ✓ passed".to_string())
        } else {
            Ok("go test: ✗ failed".to_string())
        }
    } else if project_root.join("pyproject.toml").exists() || project_root.join("setup.py").exists()
    {
        let output = run_limited_command(
            project_root,
            "python",
            &["-m", "pytest", "-q", "--color=no"],
        )?;
        if output.status.success() {
            Ok("pytest: ✓ passed".to_string())
        } else {
            Ok("pytest: ✗ failed".to_string())
        }
    } else {
        Ok("No verification available for this project type".to_string())
    }
}

fn run_pytest(project_root: &Path) -> Result<String, anyhow::Error> {
    let output = run_limited_command(
        project_root,
        "python",
        &["-m", "pytest", "-q", "--color=no"],
    )?;

    let stdout = stream_text(&output.stdout, output.stdout_truncated, "stdout");
    let mut result = String::new();

    // Extract summary
    let summary_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("passed") || l.contains("failed") || l.contains("error"))
        .collect();

    if summary_lines.is_empty() {
        result.push_str(&stdout);
    } else {
        result.push_str("Test results:\n");
        for line in &summary_lines {
            result.push_str(line);
            result.push('\n');
        }
    }

    if output.status.success() {
        result.push_str("\n✓ All tests passed");
    } else {
        result.push_str("\n✗ Some tests failed");
    }

    Ok(result)
}

fn run_npm_tests(project_root: &Path) -> Result<String, anyhow::Error> {
    let output = run_limited_command(project_root, "npm", &["test", "--color=false"])?;

    let stdout = stream_text(&output.stdout, output.stdout_truncated, "stdout");
    let mut result = format!("npm test output:\n{stdout}");

    if output.status.success() {
        result.push_str("\n✓ Tests passed");
    } else {
        result.push_str("\n✗ Tests failed");
    }

    Ok(result)
}

fn run_go_tests(project_root: &Path) -> Result<String, anyhow::Error> {
    let output = run_limited_command(project_root, "go", &["test", "./..."])?;

    let stdout = stream_text(&output.stdout, output.stdout_truncated, "stdout");
    let mut result = format!("go test output:\n{stdout}");

    if output.status.success() {
        result.push_str("\n✓ All tests passed");
    } else {
        result.push_str("\n✗ Some tests failed");
    }

    Ok(result)
}

struct LimitedCommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

fn run_limited_command(
    project_root: &Path,
    program: &str,
    args: &[&str],
) -> Result<LimitedCommandOutput, std::io::Error> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(project_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture test stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("failed to capture test stderr"))?;

    let stdout_handle =
        std::thread::spawn(move || read_limited_stream(stdout, TEST_RUNNER_STDOUT_BYTES));
    let stderr_handle =
        std::thread::spawn(move || read_limited_stream(stderr, TEST_RUNNER_STDERR_BYTES));
    let status = child.wait()?;
    let (stdout, stdout_truncated) = stdout_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("test stdout reader panicked")))?;
    let (stderr, stderr_truncated) = stderr_handle
        .join()
        .unwrap_or_else(|_| Err(std::io::Error::other("test stderr reader panicked")))?;

    Ok(LimitedCommandOutput {
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

fn stream_text(bytes: &[u8], truncated: bool, stream: &str) -> String {
    let mut text = String::from_utf8_lossy(bytes).to_string();
    if truncated {
        if !text.ends_with('\n') && !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("[test {stream} truncated]"));
    }
    text
}

fn truncate_chars(text: &str, limit: usize, suffix: &str) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let keep = limit.saturating_sub(suffix.chars().count());
    let mut output = text.chars().take(keep).collect::<String>();
    output.push_str(suffix);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_stream_reader_caps_test_output() {
        let input = std::io::Cursor::new("x".repeat(8192));
        let (bytes, truncated) = read_limited_stream(input, 1024).expect("limited stream");

        assert_eq!(bytes.len(), 1024);
        assert!(truncated);
    }

    #[test]
    fn self_verification_excerpt_is_unicode_safe() {
        let text = "界".repeat(400);
        let output = truncate_chars(&text, 300, "");

        assert_eq!(output.chars().count(), 300);
        assert!(output.chars().all(|ch| ch == '界'));
    }

    #[test]
    fn stream_text_marks_truncated_output() {
        let output = stream_text(b"hello", true, "stdout");

        assert!(output.contains("hello"));
        assert!(output.contains("[test stdout truncated]"));
    }
}
