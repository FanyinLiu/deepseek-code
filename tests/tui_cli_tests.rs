use std::process::{Command, Stdio};

fn command_with_dumb_stdio(bin: &str) -> Command {
    let mut command = Command::new(bin);
    command.env("TERM", "dumb");
    command.stdin(Stdio::null());
    command
}

#[test]
fn preview_tui_works_with_dumb_non_tty_stdio() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_octocode"))
        .args([
            "preview-tui",
            "--width",
            "80",
            "--height",
            "24",
            "--api",
            "ready",
            "--scenario",
            "welcome",
            "--theme",
            "high-contrast",
        ])
        .output()
        .expect("run preview-tui");

    assert!(output.status.success(), "stderr={}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("Octocode") || stdout.contains("OCTOCODE"));
    assert!(stdout.contains("octo"));
    assert!(stdout.contains("今天要改什么？") || stdout.contains("Describe your next step"));
    assert!(stdout.contains("Context") || stdout.contains("上下文"));
    assert!(stdout.contains("ready"));
    assert!(stdout.contains("confirm"));
}

#[test]
fn octocode_without_args_fails_cleanly_when_stdio_is_not_tty() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_octocode"))
        .output()
        .expect("run octocode without args");

    assert_clean_non_tty_tui_failure(&output);
}

#[test]
fn octo_without_args_fails_cleanly_when_stdio_is_not_tty() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_octo"))
        .output()
        .expect("run octo without args");

    assert_clean_non_tty_tui_failure(&output);
}

#[test]
fn explicit_tui_fails_cleanly_when_stdio_is_not_tty() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_octocode"))
        .arg("tui")
        .output()
        .expect("run octocode tui");

    assert_clean_non_tty_tui_failure(&output);
}

fn assert_clean_non_tty_tui_failure(output: &std::process::Output) {
    assert!(!output.status.success());
    let stderr = stderr(output);
    assert!(stderr.contains("TUI requires an interactive terminal"));
    assert!(!stderr.contains("Operation not permitted"));
    assert!(!stderr.contains("cursor position could not be read"));
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
