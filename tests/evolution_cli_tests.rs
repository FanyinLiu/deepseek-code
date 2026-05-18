use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::tempdir;

fn run_octocode(project_root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_octocode"))
        .arg("-C")
        .arg(project_root)
        .args(args)
        .output()
        .expect("run octocode")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output) {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn create_run(project_root: &Path, args: &[&str]) -> (String, String) {
    let proposal_output = run_octocode(project_root, args);
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id").to_string();
    let run_output = run_octocode(project_root, &["evolve", "patch", &proposal_id, "--json"]);
    assert_success(&run_output);
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_id = run["id"].as_str().expect("run id").to_string();
    (proposal_id, run_id)
}

#[test]
fn evolve_inspect_works_empty() {
    let temp = tempdir().expect("temp dir");

    let output = run_octocode(temp.path(), &["evolve", "inspect"]);
    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Octocode evolution inspect"));
    assert!(stdout.contains("Proposals: 0"));
    assert!(stdout.contains("Runs: 0"));
    assert!(stdout.contains("Applies: 0"));
    assert!(stdout.contains("Rollbacks: 0"));
}

#[test]
fn evolve_propose_writes_json_and_markdown() {
    let temp = tempdir().expect("temp dir");

    let output = run_octocode(
        temp.path(),
        &[
            "evolve",
            "propose",
            "--area",
            "model-routing",
            "--target",
            "src/provider/mod.rs",
            "--json",
        ],
    );
    assert_success(&output);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("json payload");
    let proposal_id = payload["id"].as_str().expect("proposal id");

    assert!(temp
        .path()
        .join(format!(".octocode/evolution/proposals/{proposal_id}.json"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/evolution/proposals/{proposal_id}.md"))
        .exists());
    assert_eq!(payload["manual_review_required"], false);
}

#[test]
fn evolve_patch_does_not_modify_current_source_tree() {
    let temp = tempdir().expect("temp dir");

    let (_proposal_id, run_id) = create_run(temp.path(), &["evolve", "propose", "--json"]);
    let patch_path = temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/patch.diff"));
    let patch = fs::read_to_string(patch_path).expect("patch");
    let target = patch
        .lines()
        .find_map(|line| line.strip_prefix("+++ b/"))
        .expect("target path");

    assert!(!temp.path().join(target).exists());
    assert!(temp
        .path()
        .join(format!(
            ".octocode/evolution/runs/{run_id}/worktree/README.md"
        ))
        .exists());
    let agents_path = temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/agents.json"));
    let candidate_path = temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/candidate.md"));
    assert!(agents_path.exists());
    assert!(candidate_path.exists());
    let agents = fs::read_to_string(agents_path).expect("agents json");
    let candidate = fs::read_to_string(candidate_path).expect("candidate markdown");
    assert!(agents.contains("planner"));
    assert!(agents.contains("implementer"));
    assert!(agents.contains("safety_reviewer"));
    assert!(candidate.contains("Evolution Candidate"));
}

#[test]
fn evolve_patch_model_agents_falls_back_without_network_when_model_invalid() {
    let temp = tempdir().expect("temp dir");
    let proposal_output = run_octocode(temp.path(), &["evolve", "propose", "--json"]);
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id");

    let run_output = run_octocode(
        temp.path(),
        &[
            "evolve",
            "patch",
            proposal_id,
            "--model-agents",
            "--model",
            "not-a-real-model",
            "--json",
        ],
    );
    assert_success(&run_output);
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_id = run["id"].as_str().expect("run id");
    let candidate_path = temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/candidate.json"));
    let candidate: Value =
        serde_json::from_str(&fs::read_to_string(candidate_path).expect("candidate json"))
            .expect("candidate payload");
    assert!(candidate["model_errors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty()));
    assert!(run["manual_review_required"].is_boolean());
}

#[test]
fn evolve_test_records_gate_outputs() {
    let temp = tempdir().expect("temp dir");
    let (_proposal_id, run_id) = create_run(temp.path(), &["evolve", "propose", "--json"]);

    let gate_output = run_octocode(temp.path(), &["evolve", "test", &run_id]);
    assert_success(&gate_output);
    let stdout = String::from_utf8_lossy(&gate_output.stdout);
    assert!(stdout.contains("Evolution gate report"));
    assert!(stdout.contains("PatchPresent"));

    assert!(temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/gates.json"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/report.md"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/stdout.log"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/stderr.log"))
        .exists());
}

#[test]
fn evolve_apply_requires_tested_run() {
    let temp = tempdir().expect("temp dir");
    let (_proposal_id, run_id) = create_run(temp.path(), &["evolve", "propose", "--json"]);

    let output = run_octocode(temp.path(), &["evolve", "apply", &run_id]);
    assert_failure(&output);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("gates.json") || stderr.contains("evolve test"));
}

#[test]
fn evolve_apply_and_rollback_restore_current_tree() {
    let temp = tempdir().expect("temp dir");
    let (_proposal_id, run_id) = create_run(temp.path(), &["evolve", "propose", "--json"]);
    let gate_output = run_octocode(temp.path(), &["evolve", "test", &run_id, "--json"]);
    assert_success(&gate_output);
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("gate json");
    let target = report["touched_files"][0].as_str().expect("touched file");
    assert!(!temp.path().join(target).exists());

    let apply_output = run_octocode(temp.path(), &["evolve", "apply", &run_id, "--json"]);
    assert_success(&apply_output);
    let apply: Value = serde_json::from_slice(&apply_output.stdout).expect("apply json");
    let apply_id = apply["id"].as_str().expect("apply id");
    assert_eq!(apply["status"], "applied");
    assert!(temp.path().join(target).exists());

    let rollback_output = run_octocode(temp.path(), &["evolve", "rollback", apply_id, "--json"]);
    assert_success(&rollback_output);
    let rollback: Value = serde_json::from_slice(&rollback_output.stdout).expect("rollback json");
    assert_eq!(rollback["status"], "rolled_back");
    assert!(!temp.path().join(target).exists());
}

#[test]
fn evolve_test_flags_high_risk_file_touch_and_apply_requires_override() {
    let temp = tempdir().expect("temp dir");

    let proposal_output = run_octocode(
        temp.path(),
        &[
            "evolve",
            "propose",
            "--area",
            "core-safety",
            "--target",
            "src/policy/mod.rs",
            "--json",
        ],
    );
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    assert_eq!(proposal["manual_review_required"], true);
    let proposal_id = proposal["id"].as_str().expect("proposal id");
    let run_output = run_octocode(temp.path(), &["evolve", "patch", proposal_id, "--json"]);
    assert_success(&run_output);
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_id = run["id"].as_str().expect("run id");

    let gate_output = run_octocode(temp.path(), &["evolve", "test", run_id, "--json"]);
    assert_success(&gate_output);
    let report: Value = serde_json::from_slice(&gate_output.stdout).expect("gate report json");

    assert_eq!(report["manual_review_required"], true);
    assert_eq!(report["status"], "needs_manual_review");
    assert_eq!(report["high_risk_files"][0], "src/policy/mod.rs");
    let apply_output = run_octocode(temp.path(), &["evolve", "apply", run_id]);
    assert_failure(&apply_output);
    let gates_path = temp
        .path()
        .join(format!(".octocode/evolution/runs/{run_id}/gates.json"));
    let gates = fs::read_to_string(gates_path).expect("gates json file");
    assert!(gates.contains("high_risk_review"));
}

#[test]
fn evolve_status_lists_applies_and_rollbacks() {
    let temp = tempdir().expect("temp dir");
    let (_proposal_id, run_id) = create_run(temp.path(), &["evolve", "propose", "--json"]);
    let gate_output = run_octocode(temp.path(), &["evolve", "test", &run_id]);
    assert_success(&gate_output);
    let apply_output = run_octocode(temp.path(), &["evolve", "apply", &run_id, "--json"]);
    assert_success(&apply_output);
    let apply: Value = serde_json::from_slice(&apply_output.stdout).expect("apply json");
    let apply_id = apply["id"].as_str().expect("apply id");
    let rollback_output = run_octocode(temp.path(), &["evolve", "rollback", apply_id]);
    assert_success(&rollback_output);

    let status_output = run_octocode(temp.path(), &["evolve", "status"]);
    assert_success(&status_output);
    let stdout = String::from_utf8_lossy(&status_output.stdout);
    assert!(stdout.contains("Applies: 1"));
    assert!(stdout.contains("Rollbacks: 1"));
}
