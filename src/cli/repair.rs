use anyhow::Result;
use std::path::PathBuf;

use crate::cli::resolve_project_root;
use crate::repair::{NewRepairProposal, RepairRun, RepairStatus, RepairStore};

#[derive(Debug, Clone)]
pub enum RepairCommand {
    Propose {
        problem: String,
        title: Option<String>,
        targets: Vec<PathBuf>,
        json: bool,
    },
    Run {
        proposal_id: String,
        full: bool,
        model_patch: bool,
        model: Option<String>,
        json: bool,
    },
    Report {
        run_id: String,
    },
    Status {
        json: bool,
    },
}

pub async fn repair(command: RepairCommand, project_root: Option<PathBuf>) -> Result<()> {
    let root = resolve_project_root(project_root, "repair")?;
    let store = RepairStore::for_project(root);
    match command {
        RepairCommand::Propose {
            problem,
            title,
            targets,
            json,
        } => {
            let proposal = store.create_proposal(NewRepairProposal {
                title,
                problem,
                targets: targets
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect(),
            })?;
            if json {
                print_json(&proposal)?;
            } else {
                println!("Created repair proposal: {}", proposal.id);
                println!("Title: {}", proposal.title);
                println!("Risk: {:?}", proposal.risk_level);
                println!("Targets: {}", proposal.allowed_paths.join(", "));
                println!("Path: .octocode/repair/proposals/{}.md", proposal.id);
            }
        }
        RepairCommand::Run {
            proposal_id,
            full,
            model_patch,
            model,
            json,
        } => {
            let run = store
                .run_proposal(&proposal_id, full, model_patch, model.as_deref())
                .await?;
            if json {
                print_json(&run)?;
            } else {
                print_run(&run);
            }
        }
        RepairCommand::Report { run_id } => {
            println!("{}", store.load_report(&run_id)?);
        }
        RepairCommand::Status { json } => {
            let status = store.status()?;
            if json {
                print_json(&status)?;
            } else {
                print_status(&status);
            }
        }
    }
    Ok(())
}

fn print_run(run: &RepairRun) {
    println!("Created repair run: {}", run.id);
    println!("Proposal: {}", run.proposal_id);
    println!("Status: {:?}", run.status);
    println!("Model patch requested: {}", run.model_patch_requested);
    if !run.model_errors.is_empty() {
        println!("Model errors: {}", run.model_errors.join("; "));
    }
    if let Some(hash) = &run.patch_hash {
        println!("Patch hash: {hash}");
    }
    if let Some(result) = &run.sandbox_result {
        println!(
            "Sandbox: {} ({})",
            result.status.as_str(),
            result.backend.as_deref().unwrap_or("unknown")
        );
    }
    println!("Report: {}", run.report_path);
    println!("Diff judge: {:?}", run.diff_judge.verdict);
    for validation in &run.validations {
        println!(
            "- validation `{}`: {:?} exit={:?} duration={}ms",
            validation.name, validation.status, validation.exit_code, validation.duration_ms
        );
    }
}

fn print_status(status: &RepairStatus) {
    println!("Octocode repair status");
    println!("Proposals: {}", status.proposals.len());
    for proposal in &status.proposals {
        println!(
            "- {} [{:?}] {}",
            proposal.id, proposal.risk_level, proposal.title
        );
    }
    println!("Runs: {}", status.runs.len());
    for run in &status.runs {
        println!("- {} [{}] {:?}", run.id, run.proposal_id, run.status);
    }
    println!("Failure memory: {}", status.failure_memory.len());
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
