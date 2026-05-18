use crate::cli::resolve_project_root;
use crate::evolution::{
    validate_apply_id, validate_proposal_id, validate_run_id, EvolutionApply, EvolutionGateReport,
    EvolutionInspection, EvolutionProposal, EvolutionRollback, EvolutionRun, EvolutionStore,
    NewEvolutionProposal, EVOLUTION_DIR,
};
use crate::goal::GoalStore;
use crate::repair::{NewRepairProposal, RepairStore};
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum EvolutionCommand {
    Inspect {
        json: bool,
    },
    Propose {
        area: Option<String>,
        targets: Vec<PathBuf>,
        title: Option<String>,
        problem: Option<String>,
        json: bool,
    },
    Patch {
        proposal_id: String,
        model_agents: bool,
        model: Option<String>,
        json: bool,
    },
    Repair {
        proposal_id: String,
        model_patch: bool,
        model: Option<String>,
        full: bool,
        json: bool,
    },
    ProveSelf {
        task: String,
        targets: Vec<PathBuf>,
        model: Option<String>,
        full: bool,
        json: bool,
    },
    FromGoal {
        model: Option<String>,
        full: bool,
        json: bool,
    },
    Benchmark {
        rounds: usize,
        model: Option<String>,
        full: bool,
        json: bool,
    },
    Memory {
        json: bool,
    },
    Test {
        run_id: String,
        full: bool,
        json: bool,
    },
    Apply {
        run_id: String,
        allow_high_risk: bool,
        json: bool,
    },
    Rollback {
        apply_id: String,
        json: bool,
    },
    Status {
        json: bool,
    },
}

pub async fn evolution(command: EvolutionCommand, project_root: Option<PathBuf>) -> Result<()> {
    let root = resolve_project_root(project_root, "evolve")?;
    let store = EvolutionStore::for_project(&root);
    match command {
        EvolutionCommand::Inspect { json } => {
            let inspection = store.inspect()?;
            if json {
                print_json(&inspection)?;
            } else {
                print_inspection(&inspection);
            }
        }
        EvolutionCommand::Propose {
            area,
            targets,
            title,
            problem,
            json,
        } => {
            let proposal = store.create_proposal(NewEvolutionProposal {
                title,
                problem,
                target_area: area.unwrap_or_else(|| "core-evolution".to_string()),
                target_files: targets
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect(),
            })?;
            if json {
                print_json(&proposal)?;
            } else {
                print_proposal(&proposal);
            }
        }
        EvolutionCommand::Patch {
            proposal_id,
            model_agents,
            model,
            json,
        } => {
            validate_proposal_id(&proposal_id)?;
            let run = if model_agents {
                store
                    .create_model_patch_run(&proposal_id, model.as_deref())
                    .await?
            } else {
                store.create_patch_run(&proposal_id)?
            };
            if json {
                print_json(&run)?;
            } else {
                print_run(&run);
            }
        }
        EvolutionCommand::Repair {
            proposal_id,
            model_patch,
            model,
            full,
            json,
        } => {
            validate_proposal_id(&proposal_id)?;
            let evolution_proposal = load_evolution_proposal(&root, &proposal_id)?;
            let repair_store = RepairStore::for_project(&root);
            let repair_proposal = repair_store.create_proposal(NewRepairProposal {
                title: Some(format!("Evolve via repair: {}", evolution_proposal.title)),
                problem: evolution_proposal.problem.clone(),
                targets: evolution_proposal.target_files.clone(),
            })?;
            let repair_run = repair_store
                .run_proposal(&repair_proposal.id, full, model_patch, model.as_deref())
                .await?;
            if json {
                print_json(&serde_json::json!({
                    "evolution_proposal_id": proposal_id,
                    "repair_proposal": repair_proposal,
                    "repair_run": repair_run
                }))?;
            } else {
                println!("Created repair-backed evolution candidate");
                println!("Evolution proposal: {}", proposal_id);
                println!("Repair proposal: {}", repair_proposal.id);
                println!("Repair run: {}", repair_run.id);
                println!("Repair status: {:?}", repair_run.status);
                println!("Report: {}", repair_run.report_path);
            }
        }
        EvolutionCommand::ProveSelf {
            task,
            targets,
            model,
            full,
            json,
        } => {
            let proof = store
                .prove_self(
                    task,
                    targets
                        .into_iter()
                        .map(|path| path.display().to_string())
                        .collect(),
                    model.as_deref(),
                    full,
                    None,
                )
                .await?;
            if json {
                print_json(&proof)?;
            } else {
                print_proof(&proof);
            }
        }
        EvolutionCommand::FromGoal { model, full, json } => {
            let goal_store = GoalStore::for_project(&root);
            let goals = goal_store.load_or_default()?;
            let graph = goal_store.create_task_graph()?;
            let alignment = goals.alignment();
            let model = model.as_deref().or_else(|| goals.execution_model());
            let proof = store
                .prove_self(
                    goals.evolution_task(),
                    alignment.target_files.clone(),
                    model,
                    full || goals.execution.full_validation,
                    Some(alignment),
                )
                .await?;
            let graph = goal_store.complete_task_graph(
                graph,
                proof.verdict == crate::evolution::EvolutionProofVerdict::SelfEvolutionProven,
            )?;
            if json {
                print_json(&serde_json::json!({
                    "task_graph": graph,
                    "proof": proof
                }))?;
            } else {
                print_proof(&proof);
                println!("Task graph: {}", graph.id);
            }
        }
        EvolutionCommand::Benchmark {
            rounds,
            model,
            full,
            json,
        } => {
            let goals = GoalStore::for_project(&root).load_or_default()?;
            let benchmark = store
                .benchmark_from_goal(&goals, rounds, model.as_deref(), full)
                .await?;
            if json {
                print_json(&benchmark)?;
            } else {
                println!("Evolution benchmark: {}", benchmark.id);
                println!("Rounds: {}", benchmark.rounds_requested);
                println!("Success rate: {}", benchmark.success_rate);
                for proof in &benchmark.rounds {
                    println!("- {}: {:?}", proof.id, proof.verdict);
                }
            }
        }
        EvolutionCommand::Memory { json } => {
            let events = store.list_memory_events()?;
            if json {
                print_json(&events)?;
            } else {
                println!("Evolution memory events: {}", events.len());
                for event in events.iter().rev().take(10).rev() {
                    let proof_id = event
                        .get("proof_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown");
                    let verdict = event
                        .get("verdict")
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let sandbox = event
                        .get("sandbox_status")
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    println!("- {proof_id}: verdict={verdict} sandbox={sandbox}");
                }
            }
        }
        EvolutionCommand::Test { run_id, full, json } => {
            validate_run_id(&run_id)?;
            let report = store.run_gates(&run_id, full)?;
            if json {
                print_json(&report)?;
            } else {
                print_gate_report(&report);
            }
        }
        EvolutionCommand::Apply {
            run_id,
            allow_high_risk,
            json,
        } => {
            validate_run_id(&run_id)?;
            let apply = store.apply_run(&run_id, allow_high_risk)?;
            if json {
                print_json(&apply)?;
            } else {
                print_apply(&apply);
            }
        }
        EvolutionCommand::Rollback { apply_id, json } => {
            validate_apply_id(&apply_id)?;
            let rollback = store.rollback_apply(&apply_id)?;
            if json {
                print_json(&rollback)?;
            } else {
                print_rollback(&rollback);
            }
        }
        EvolutionCommand::Status { json } => {
            let status = EvolutionStatus {
                proposals: store.list_proposals()?,
                runs: store.list_runs()?,
                applies: store.list_applies()?,
                rollbacks: store.list_rollbacks()?,
            };
            if json {
                print_json(&status)?;
            } else {
                print_status(&status);
            }
        }
    }
    Ok(())
}

fn load_evolution_proposal(root: &std::path::Path, proposal_id: &str) -> Result<EvolutionProposal> {
    let path = root
        .join(EVOLUTION_DIR)
        .join("proposals")
        .join(format!("{proposal_id}.json"));
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

#[derive(Debug, serde::Serialize)]
struct EvolutionStatus {
    proposals: Vec<EvolutionProposal>,
    runs: Vec<EvolutionRun>,
    applies: Vec<EvolutionApply>,
    rollbacks: Vec<EvolutionRollback>,
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_inspection(inspection: &EvolutionInspection) {
    println!("Octocode evolution inspect");
    println!("Project: {}", inspection.project_root);
    println!("Proposals: {}", inspection.proposals);
    println!("Runs: {}", inspection.runs);
    println!("Applies: {}", inspection.applies);
    println!("Rollbacks: {}", inspection.rollbacks);
    println!("Recommendations:");
    for recommendation in &inspection.recommendations {
        println!("- {}: {}", recommendation.area, recommendation.title);
        println!("  targets: {}", recommendation.target_files.join(", "));
    }
}

fn print_proposal(proposal: &EvolutionProposal) {
    println!("Created evolution proposal: {}", proposal.id);
    println!("Area: {}", proposal.target_area);
    println!("Risk: {:?}", proposal.risk_level);
    println!(
        "Manual review required: {}",
        proposal.manual_review_required
    );
    if proposal.target_files.is_empty() {
        println!("Targets: docs/evolution/{}.md", proposal.id);
    } else {
        println!("Targets: {}", proposal.target_files.join(", "));
    }
}

fn print_run(run: &EvolutionRun) {
    println!("Created evolution run: {}", run.id);
    println!("Proposal: {}", run.proposal_id);
    println!("Status: {:?}", run.status);
    println!("Manual review required: {}", run.manual_review_required);
    println!("Patch: .octocode/evolution/runs/{}/patch.diff", run.id);
    println!("Agents: .octocode/evolution/runs/{}/agents.json", run.id);
    println!(
        "Candidate: .octocode/evolution/runs/{}/candidate.md",
        run.id
    );
}

fn print_proof(proof: &crate::evolution::EvolutionProof) {
    println!("Self-evolution proof: {}", proof.id);
    println!("Verdict: {:?}", proof.verdict);
    if let Some(alignment) = &proof.goal_alignment {
        println!(
            "Goal: {} - {}",
            alignment.objective_id, alignment.objective_title
        );
    }
    println!("Meaningfulness: {:?}", proof.meaningfulness.status);
    println!("Evolution proposal: {}", proof.evolution_proposal_id);
    println!("Repair proposal: {}", proof.repair_proposal_id);
    println!("Repair run: {}", proof.repair_run_id);
    println!(
        "Archive candidate: {}",
        proof.archive_candidate_id.as_deref().unwrap_or("none")
    );
    println!(
        "Sandbox: {} ({})",
        proof.sandbox_status.as_deref().unwrap_or("none"),
        proof.sandbox_backend.as_deref().unwrap_or("none")
    );
    println!("Proof JSON: {}", proof.proof_json_path);
    println!("Proof report: {}", proof.proof_report_path);
    if let Some(reason) = &proof.failure_reason {
        println!("Failure reason: {reason}");
    }
}

fn print_gate_report(report: &EvolutionGateReport) {
    println!("Evolution gate report: {}", report.run_id);
    println!("Status: {:?}", report.status);
    println!("Manual review required: {}", report.manual_review_required);
    for gate in &report.gates {
        println!("- {:?}: {:?} - {}", gate.name, gate.status, gate.message);
    }
    for command in &report.commands {
        println!(
            "- command `{}`: {:?} ({:?}, {}ms)",
            command.command.join(" "),
            command.status,
            command.exit_code,
            command.duration_ms
        );
    }
}

fn print_apply(apply: &EvolutionApply) {
    println!("Applied evolution run: {}", apply.run_id);
    println!("Apply: {}", apply.id);
    println!("Status: {:?}", apply.status);
    println!("Manual review required: {}", apply.manual_review_required);
    println!("Touched: {}", apply.touched_files.join(", "));
}

fn print_rollback(rollback: &EvolutionRollback) {
    println!("Rolled back evolution apply: {}", rollback.apply_id);
    println!("Rollback: {}", rollback.id);
    println!("Status: {:?}", rollback.status);
    println!("Restored: {}", rollback.restored_files.join(", "));
}

fn print_status(status: &EvolutionStatus) {
    println!("Octocode evolution status");
    println!("Proposals: {}", status.proposals.len());
    for proposal in &status.proposals {
        println!(
            "- {} [{}] manual_review_required={}",
            proposal.id, proposal.target_area, proposal.manual_review_required
        );
    }
    println!("Runs: {}", status.runs.len());
    for run in &status.runs {
        println!(
            "- {} [{}] manual_review_required={}",
            run.id, run.proposal_id, run.manual_review_required
        );
    }
    println!("Applies: {}", status.applies.len());
    for apply in &status.applies {
        println!(
            "- {} [{}] status={:?}",
            apply.id, apply.run_id, apply.status
        );
    }
    println!("Rollbacks: {}", status.rollbacks.len());
    for rollback in &status.rollbacks {
        println!(
            "- {} [{}] status={:?}",
            rollback.id, rollback.apply_id, rollback.status
        );
    }
}
