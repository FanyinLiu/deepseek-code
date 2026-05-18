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

#[test]
fn knowledge_update_writes_project_and_risk_map() {
    let temp = tempdir().expect("temp dir");

    let output = run_octocode(temp.path(), &["knowledge", "update", "--json"]);
    assert_success(&output);
    let payload: Value = serde_json::from_slice(&output.stdout).expect("knowledge json");

    assert_eq!(payload["project_root"], temp.path().display().to_string());
    assert!(temp.path().join(".octocode/knowledge/project.md").exists());
    assert!(temp
        .path()
        .join(".octocode/knowledge/risk-map.json")
        .exists());

    let risk_output = run_octocode(temp.path(), &["knowledge", "show", "risk-map", "--json"]);
    assert_success(&risk_output);
    let risk_map: Value = serde_json::from_slice(&risk_output.stdout).expect("risk map json");
    assert_eq!(risk_map["version"], 1);
    assert!(risk_map["paths"]
        .as_array()
        .is_some_and(|paths| !paths.is_empty()));
}

#[test]
fn repair_propose_writes_proposal_files() {
    let temp = tempdir().expect("temp dir");

    let output = run_octocode(
        temp.path(),
        &[
            "repair",
            "propose",
            "fix provider model routing",
            "--target",
            "src/provider/mod.rs",
            "--json",
        ],
    );
    assert_success(&output);
    let proposal: Value = serde_json::from_slice(&output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id");

    assert_eq!(proposal["risk_level"], "low");
    assert!(temp
        .path()
        .join(format!(".octocode/repair/proposals/{proposal_id}.json"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/repair/proposals/{proposal_id}.md"))
        .exists());
    assert!(temp
        .path()
        .join(format!(
            ".octocode/repair/proposals/{proposal_id}.context-bundle.json"
        ))
        .exists());
}

#[test]
fn repair_run_writes_report_without_applying_patch() {
    let temp = tempdir().expect("temp dir");

    let proposal_output = run_octocode(
        temp.path(),
        &[
            "repair",
            "propose",
            "fix command wiring",
            "--target",
            "src/cli_entry.rs",
            "--json",
        ],
    );
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id");

    let run_output = run_octocode(temp.path(), &["repair", "run", proposal_id, "--json"]);
    assert_success(&run_output);
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_id = run["id"].as_str().expect("run id");

    assert!(temp
        .path()
        .join(format!(".octocode/repair/runs/{run_id}/run.json"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/repair/runs/{run_id}/report.md"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/repair/runs/{run_id}/patch.diff"))
        .exists());
    let archive_candidates = temp.path().join(".octocode/evolution/archive/candidates");
    assert!(archive_candidates.exists());
    assert!(fs::read_dir(&archive_candidates)
        .expect("archive candidates")
        .next()
        .is_some());
    assert!(temp
        .path()
        .join(".octocode/evolution/archive/lineage.jsonl")
        .exists());
    let archive_status_output = run_octocode(temp.path(), &["archive", "status", "--json"]);
    assert_success(&archive_status_output);
    let archive_status: Value =
        serde_json::from_slice(&archive_status_output.stdout).expect("archive status json");
    assert_eq!(archive_status["candidates"], 1);
    assert_eq!(archive_status["lineage_events"], 1);

    let archive_list_output = run_octocode(temp.path(), &["archive", "list", "--json"]);
    assert_success(&archive_list_output);
    let archive_list: Value =
        serde_json::from_slice(&archive_list_output.stdout).expect("archive list json");
    let candidate_id = archive_list[0]["candidate"]["id"]
        .as_str()
        .expect("candidate id");
    assert!(archive_list[0]["utility_score"].is_number());

    let archive_show_output =
        run_octocode(temp.path(), &["archive", "show", candidate_id, "--json"]);
    assert_success(&archive_show_output);
    let archive_show: Value =
        serde_json::from_slice(&archive_show_output.stdout).expect("archive show json");
    assert_eq!(archive_show["candidate"]["id"], candidate_id);

    let lineage_output = run_octocode(temp.path(), &["archive", "lineage", "--json"]);
    assert_success(&lineage_output);
    let lineage: Value = serde_json::from_slice(&lineage_output.stdout).expect("lineage json");
    assert_eq!(lineage[0]["candidate_id"], candidate_id);
    assert!(!temp.path().join("src/cli_entry.rs").exists());

    let report_output = run_octocode(temp.path(), &["repair", "report", run_id]);
    assert_success(&report_output);
    let report = String::from_utf8_lossy(&report_output.stdout);
    assert!(report.contains("Repair Run Report"));
    assert!(report.contains("does not auto-apply patches"));
}

#[test]
fn repair_model_patch_records_model_error_without_applying() {
    let temp = tempdir().expect("temp dir");

    let proposal_output = run_octocode(
        temp.path(),
        &[
            "repair",
            "propose",
            "fix model patch path",
            "--target",
            "src/provider/mod.rs",
            "--json",
        ],
    );
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id");

    let run_output = run_octocode(
        temp.path(),
        &[
            "repair",
            "run",
            proposal_id,
            "--model-patch",
            "--model",
            "not-a-real-model",
            "--json",
        ],
    );
    assert_success(&run_output);
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_id = run["id"].as_str().expect("run id");
    assert_eq!(run["model_patch_requested"], true);
    assert!(run["model_errors"]
        .as_array()
        .is_some_and(|errors| !errors.is_empty()));
    assert!(run["patch_hash"].is_null());

    let patch = fs::read_to_string(
        temp.path()
            .join(format!(".octocode/repair/runs/{run_id}/patch.diff")),
    )
    .expect("patch file");
    assert!(patch.is_empty());
    assert!(!temp.path().join("src/provider/mod.rs").exists());
}

#[test]
fn evolve_repair_creates_repair_backed_candidate() {
    let temp = tempdir().expect("temp dir");

    let proposal_output = run_octocode(
        temp.path(),
        &[
            "evolve",
            "propose",
            "--area",
            "repair-bridge",
            "--target",
            "src/provider/mod.rs",
            "--json",
        ],
    );
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id");

    let repair_output = run_octocode(temp.path(), &["evolve", "repair", proposal_id, "--json"]);
    assert_success(&repair_output);
    let payload: Value = serde_json::from_slice(&repair_output.stdout).expect("repair json");
    assert_eq!(payload["evolution_proposal_id"], proposal_id);
    assert!(payload["repair_proposal"]["id"]
        .as_str()
        .is_some_and(|id| id.starts_with("repair-proposal-")));
    assert!(payload["repair_run"]["id"]
        .as_str()
        .is_some_and(|id| id.starts_with("repair-run-")));

    let archive_status_output = run_octocode(temp.path(), &["archive", "status", "--json"]);
    assert_success(&archive_status_output);
    let archive_status: Value =
        serde_json::from_slice(&archive_status_output.stdout).expect("archive status json");
    assert_eq!(archive_status["candidates"], 1);
}

#[test]
fn skill_add_from_repair_run_creates_draft_skill() {
    let temp = tempdir().expect("temp dir");

    let proposal_output = run_octocode(
        temp.path(),
        &[
            "repair",
            "propose",
            "fix provider command path",
            "--target",
            "src/provider/mod.rs",
            "--json",
        ],
    );
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id");

    let run_output = run_octocode(temp.path(), &["repair", "run", proposal_id, "--json"]);
    assert_success(&run_output);
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_id = run["id"].as_str().expect("run id");

    let skill_output = run_octocode(
        temp.path(),
        &[
            "skill",
            "add",
            run_id,
            "--name",
            "provider repair",
            "--json",
        ],
    );
    assert_success(&skill_output);
    let skill: Value = serde_json::from_slice(&skill_output.stdout).expect("skill json");
    let skill_id = skill["id"].as_str().expect("skill id");
    assert_eq!(skill["status"], "draft");

    assert!(temp
        .path()
        .join(format!(".octocode/skills/{skill_id}/SKILL.md"))
        .exists());
    assert!(temp
        .path()
        .join(format!(".octocode/skills/{skill_id}/metadata.json"))
        .exists());

    let list_output = run_octocode(temp.path(), &["skill", "list", "--json"]);
    assert_success(&list_output);
    let list: Value = serde_json::from_slice(&list_output.stdout).expect("skill list json");
    assert_eq!(list.as_array().expect("skill list").len(), 1);

    let show_output = run_octocode(temp.path(), &["skill", "show", skill_id]);
    assert_success(&show_output);
    let skill_markdown = String::from_utf8_lossy(&show_output.stdout);
    assert!(skill_markdown.contains("Draft repair skill"));

    let test_output = run_octocode(temp.path(), &["skill", "test", skill_id, "--json"]);
    assert_success(&test_output);
    let test_report: Value = serde_json::from_slice(&test_output.stdout).expect("test report json");
    assert_eq!(test_report["status"], "pass");

    let next_proposal_output = run_octocode(
        temp.path(),
        &[
            "repair",
            "propose",
            "fix provider routing again",
            "--target",
            "src/provider/mod.rs",
            "--json",
        ],
    );
    assert_success(&next_proposal_output);
    let next_proposal: Value =
        serde_json::from_slice(&next_proposal_output.stdout).expect("next proposal json");
    assert_eq!(
        next_proposal["context_bundle"]["candidate_skills"][0]["id"],
        skill_id
    );
}

#[test]
fn blocked_repair_run_appends_failure_memory() {
    let temp = tempdir().expect("temp dir");

    let proposal_output = run_octocode(
        temp.path(),
        &[
            "repair",
            "propose",
            "try to lower risk-map",
            "--target",
            ".octocode/knowledge/risk-map.json",
            "--json",
        ],
    );
    assert_success(&proposal_output);
    let proposal: Value = serde_json::from_slice(&proposal_output.stdout).expect("proposal json");
    let proposal_id = proposal["id"].as_str().expect("proposal id");

    let run_output = run_octocode(temp.path(), &["repair", "run", proposal_id, "--json"]);
    assert_success(&run_output);
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    assert_eq!(run["status"], "blocked");

    let failures_output = run_octocode(temp.path(), &["knowledge", "show", "failures", "--json"]);
    assert_success(&failures_output);
    let failures: Value = serde_json::from_slice(&failures_output.stdout).expect("failures json");
    assert_eq!(failures.as_array().expect("failures array").len(), 1);

    let failure_memory =
        fs::read_to_string(temp.path().join(".octocode/knowledge/failure-memory.jsonl"))
            .expect("failure memory");
    assert!(failure_memory.contains("try to lower risk-map"));
}
