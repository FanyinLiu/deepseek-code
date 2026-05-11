//! Automatic test detection and execution after file changes.
use std::path::Path;

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
    let output = std::process::Command::new("cargo")
        .args(["test", "--color=never"])
        .current_dir(project_root)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
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
        let output = std::process::Command::new("cargo")
            .args(["check", "--color=never"])
            .current_dir(project_root)
            .output()?;
        let _stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if output.status.success() {
            Ok("cargo check: ✓ clean".to_string())
        } else {
            let msg = if stderr.len() > 300 {
                &stderr[..300]
            } else {
                &stderr
            };
            Ok(format!("cargo check: ✗ errors\n{msg}"))
        }
    } else if project_root.join("package.json").exists() {
        let output = std::process::Command::new("npm")
            .args(["test", "--color=false"])
            .current_dir(project_root)
            .output()?;
        if output.status.success() {
            Ok("npm test: ✓ passed".to_string())
        } else {
            Ok("npm test: ✗ failed".to_string())
        }
    } else if project_root.join("go.mod").exists() {
        let output = std::process::Command::new("go")
            .args(["test", "./..."])
            .current_dir(project_root)
            .output()?;
        if output.status.success() {
            Ok("go test: ✓ passed".to_string())
        } else {
            Ok("go test: ✗ failed".to_string())
        }
    } else if project_root.join("pyproject.toml").exists() || project_root.join("setup.py").exists()
    {
        let output = std::process::Command::new("python")
            .args(["-m", "pytest", "-q", "--color=no"])
            .current_dir(project_root)
            .output()?;
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
    let output = std::process::Command::new("python")
        .args(["-m", "pytest", "-q", "--color=no"])
        .current_dir(project_root)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
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
    let output = std::process::Command::new("npm")
        .args(["test", "--color=false"])
        .current_dir(project_root)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = format!("npm test output:\n{stdout}");

    if output.status.success() {
        result.push_str("\n✓ Tests passed");
    } else {
        result.push_str("\n✗ Tests failed");
    }

    Ok(result)
}

fn run_go_tests(project_root: &Path) -> Result<String, anyhow::Error> {
    let output = std::process::Command::new("go")
        .args(["test", "./..."])
        .current_dir(project_root)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = format!("go test output:\n{stdout}");

    if output.status.success() {
        result.push_str("\n✓ All tests passed");
    } else {
        result.push_str("\n✗ Some tests failed");
    }

    Ok(result)
}
