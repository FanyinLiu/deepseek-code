use std::process::{Command, Stdio};

fn command_with_dumb_stdio(bin: &str) -> Command {
    let mut command = Command::new(bin);
    command.env("TERM", "dumb");
    command.stdin(Stdio::null());
    command
}

#[test]
fn preview_tui_works_with_dumb_non_tty_stdio() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_ds"))
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
    assert!(stdout.contains("DSCODE"));
    assert!(stdout.contains("DeepSeek V4 Flash"));
}

#[test]
fn ds_without_args_fails_cleanly_when_stdio_is_not_tty() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_ds"))
        .output()
        .expect("run ds without args");

    assert_clean_non_tty_tui_failure(&output);
}

#[test]
fn dscode_without_args_fails_cleanly_when_stdio_is_not_tty() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_dscode"))
        .output()
        .expect("run dscode without args");

    assert_clean_non_tty_tui_failure(&output);
}

#[test]
fn explicit_tui_fails_cleanly_when_stdio_is_not_tty() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_ds"))
        .arg("tui")
        .output()
        .expect("run ds tui");

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
