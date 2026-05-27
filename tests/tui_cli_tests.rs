use std::process::{Command, Stdio};

fn command_with_dumb_stdio(bin: &str) -> Command {
    let mut command = Command::new(bin);
    command.env("TERM", "dumb");
    command.stdin(Stdio::null());
    command
}

#[test]
fn preview_tui_works_with_dumb_non_tty_stdio() {
    let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_octo"))
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
    assert!(
        stdout.contains("今天想从哪开始？") || stdout.contains("Where do you want to start")
    );
    assert!(stdout.contains("Context") || stdout.contains("上下文"));
    assert!(stdout.contains("ready"));
    assert!(stdout.contains("confirm"));
}

#[test]
fn preview_tui_exposes_core_product_surfaces_from_octo() {
    let scenarios: &[(&str, &[&[&str]])] = &[
        (
            "slash",
            &[&["Commands", "命令"], &["/agents"], &["/commands"]],
        ),
        (
            "approval",
            &[
                &["approve tool call", "审批工具调用"],
                &["run_command"],
                &["cargo test --all-features"],
            ],
        ),
        (
            "settings",
            &[
                &["Settings", "设置"],
                &["Safety", "安全"],
                &["Command approval", "命令审批"],
            ],
        ),
        (
            "diff",
            &[&["Diff focus", "Diff 焦点"], &["src/tui/app.rs"], &["@@"]],
        ),
        (
            "history",
            &[&["History search"], &["cargo test --all-features"]],
        ),
        ("file-mention", &[&["Files", "文件"], &["src/tui/app.rs"]]),
    ];

    for (scenario, marker_groups) in scenarios {
        let output = command_with_dumb_stdio(env!("CARGO_BIN_EXE_octo"))
            .args([
                "preview-tui",
                "--width",
                "100",
                "--height",
                "28",
                "--api",
                "ready",
                "--scenario",
                scenario,
                "--theme",
                "high-contrast",
            ])
            .output()
            .expect("run preview-tui scenario");

        assert!(
            output.status.success(),
            "scenario={scenario} stderr={}",
            stderr(&output)
        );
        let stdout = stdout(&output);
        for markers in *marker_groups {
            assert!(
                markers.iter().any(|marker| stdout.contains(marker)),
                "scenario={scenario} missing one of {markers:?}\n{stdout}"
            );
        }
    }
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
