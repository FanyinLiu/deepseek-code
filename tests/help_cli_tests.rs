use std::process::Command;

fn octo(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_octo"))
        .args(args)
        .output()
        .expect("run octo help command")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_stdout_contains(output: &std::process::Output, needle: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(needle),
        "expected stdout to contain {needle:?}\nstdout:\n{stdout}"
    );
}

fn assert_stdout_contains_any(output: &std::process::Output, needles: &[&str]) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        needles.iter().any(|needle| stdout.contains(needle)),
        "expected stdout to contain one of {needles:?}\nstdout:\n{stdout}"
    );
}

#[test]
fn root_help_lists_core_discoverability_commands() {
    let output = octo(&["--help"]);
    assert_success(&output);

    assert_stdout_contains(&output, "Multi-model, multi-agent coding CLI");
    assert_stdout_contains(&output, "Usage: octo");
    assert_stdout_contains(
        &output,
        "features   Discover available features and recommended operating modes",
    );
    assert_stdout_contains(
        &output,
        "evolve     Inspect and run self-evolution proposal workflows",
    );
    assert_stdout_contains(
        &output,
        "agent      Manage built-in and project custom agents",
    );
    assert_stdout_contains(
        &output,
        "mission    Record and inspect long-running mission dry-runs",
    );
}

#[test]
fn agent_help_lists_agent_management_commands() {
    let output = octo(&["agent", "--help"]);
    assert_success(&output);

    assert_stdout_contains_any(
        &output,
        &[
            "Usage: octo agent [OPTIONS] <COMMAND>",
            "Usage: octo.exe agent [OPTIONS] <COMMAND>",
        ],
    );
    assert_stdout_contains(&output, "list      List built-in and project custom agents");
    assert_stdout_contains(
        &output,
        "show      Show one agent's configuration and prompt",
    );
    assert_stdout_contains(&output, "run       Run one named agent on a task");
    assert_stdout_contains(
        &output,
        "create    Create a custom markdown agent from a template",
    );
    assert_stdout_contains(
        &output,
        "validate  Validate custom agent files or built-ins",
    );
}

#[test]
fn features_help_lists_discovery_commands() {
    let output = octo(&["features", "--help"]);
    assert_success(&output);

    assert_stdout_contains(
        &output,
        "Discover available features and recommended operating modes",
    );
    assert_stdout_contains(
        &output,
        "matrix     Show the competitive feature matrix summary",
    );
    assert_stdout_contains(
        &output,
        "status     Show local capability and configuration status",
    );
    assert_stdout_contains(&output, "recommend  Recommend an operating mode for a task");
}

#[test]
fn mission_help_lists_dry_run_replay_commands() {
    let output = octo(&["mission", "--help"]);
    assert_success(&output);

    assert_stdout_contains(&output, "Record and inspect long-running mission dry-runs");
    assert_stdout_contains(&output, "new       Create a new dry-run mission record");
    assert_stdout_contains(&output, "status    Show mission state");
    assert_stdout_contains(
        &output,
        "inspect   Inspect mission, plan, and optionally events",
    );
    assert_stdout_contains(
        &output,
        "replay    Replay mission events and reconstruct state",
    );
    assert_stdout_contains(&output, "list      List local mission records");
    assert_stdout_contains(&output, "start     Mark a mission as running");
    assert_stdout_contains(&output, "pause     Mark a mission as paused");
    assert_stdout_contains(&output, "complete  Mark a mission as completed");
}
