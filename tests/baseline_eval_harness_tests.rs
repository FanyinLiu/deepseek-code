use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct EvalCase {
    input: String,
    command_args: Vec<String>,
    expected_events: Vec<String>,
    forbidden_events: Vec<String>,
    expected_exit: ExpectedExit,
    notes: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ExpectedExit {
    Success,
    Failure,
}

#[test]
fn baseline_eval_cases_exercise_real_cli_machine_output_contracts() {
    let cases = load_cases();
    assert!(
        cases.len() >= 10,
        "baseline eval should cover at least ten cases"
    );

    for case in cases {
        run_case(&case);
    }
}

fn load_cases() -> Vec<EvalCase> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/evals/baseline_cases.jsonl");
    let data = std::fs::read_to_string(path).expect("read baseline eval cases");
    data.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("parse eval case"))
        .collect()
}

fn run_case(case: &EvalCase) {
    let root = tempfile::tempdir().expect("tempdir");
    if case
        .command_args
        .first()
        .is_some_and(|command| matches!(command.as_str(), "models" | "settings" | "doctor"))
    {
        std::fs::create_dir_all(root.path().join(".octocode")).expect("create eval project root");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_octo"))
        .current_dir(root.path())
        .args(&case.command_args)
        .output()
        .unwrap_or_else(|error| panic!("run eval case '{}': {error}", case.notes));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    match case.expected_exit {
        ExpectedExit::Success => assert!(
            output.status.success(),
            "{} should succeed; stderr={stderr}",
            case.notes
        ),
        ExpectedExit::Failure => assert!(
            !output.status.success(),
            "{} should fail; stdout={stdout}",
            case.notes
        ),
    }

    assert!(
        !case.input.trim().is_empty(),
        "{} should declare the motivating input",
        case.notes
    );
    for expected in &case.expected_events {
        assert_expected_event(expected, &stdout, &stderr, &case.notes);
    }
    for forbidden in &case.forbidden_events {
        assert_forbidden_event(forbidden, &stdout, &stderr, &case.notes);
    }
}

fn assert_expected_event(expected: &str, stdout: &str, stderr: &str, notes: &str) {
    match expected {
        "final:error" => {
            let lines = stdout.lines().collect::<Vec<_>>();
            assert_eq!(lines.len(), 1, "{notes}: final JSON must be one line");
            let value: serde_json::Value =
                serde_json::from_str(lines[0]).expect("final stdout line is JSON");
            assert_eq!(value["status"], "error", "{notes}: {value}");
            assert!(value["events"].is_array(), "{notes}: {value}");
        }
        "stream:error" => {
            let found = stdout.lines().any(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .is_some_and(|value| value["type"] == "error" && value["error"].is_string())
            });
            assert!(found, "{notes}: stream-json should include error event");
        }
        expected if expected.starts_with("stdout:") || expected.starts_with("stderr:") => {
            assert_expected_text(expected, stdout, stderr, notes);
        }
        other => panic!("unknown expected eval event '{other}' in {notes}"),
    }
}

fn assert_expected_text(expected: &str, stdout: &str, stderr: &str, notes: &str) {
    if let Some(needle) = expected.strip_prefix("stdout:") {
        assert!(
            stdout.contains(needle),
            "{notes}: stdout did not contain expected text {needle:?}; stdout={stdout}"
        );
    } else if let Some(needle) = expected.strip_prefix("stderr:") {
        assert!(
            stderr.contains(needle),
            "{notes}: stderr did not contain expected text {needle:?}; stderr={stderr}"
        );
    }
}

fn assert_forbidden_event(forbidden: &str, stdout: &str, stderr: &str, notes: &str) {
    if let Some(needle) = forbidden.strip_prefix("stdout:") {
        assert!(
            !stdout.contains(needle),
            "{notes}: stdout contained forbidden text {needle:?}"
        );
    } else if let Some(needle) = forbidden.strip_prefix("stderr:") {
        assert!(
            !stderr.contains(needle),
            "{notes}: stderr contained forbidden text {needle:?}"
        );
    } else {
        panic!("unknown forbidden eval event '{forbidden}' in {notes}");
    }
}
