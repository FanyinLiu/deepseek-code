use crate::cli::resolve_project_root;
use crate::evolution::{EvolutionProofVerdict, EvolutionStore};
use crate::goal::{GoalConfig, GoalStore, GoalTaskGraph};
use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum GoalCommand {
    Init {
        overwrite: bool,
        json: bool,
    },
    Show {
        json: bool,
    },
    Plan {
        json: bool,
    },
    Graph {
        json: bool,
    },
    SetActive {
        id: String,
        title: String,
        success: Vec<String>,
        targets: Vec<PathBuf>,
        json: bool,
    },
    Run {
        model: Option<String>,
        full: bool,
        json: bool,
    },
}

pub async fn goal(command: GoalCommand, project_root: Option<PathBuf>) -> Result<()> {
    let root = resolve_project_root(project_root, "goal")?;
    let store = GoalStore::for_project(&root);
    match command {
        GoalCommand::Init { overwrite, json } => {
            let config = store.init_default(overwrite)?;
            if json {
                print_json(&config)?;
            } else {
                print_goal(&config, store.path().display().to_string().as_str());
            }
        }
        GoalCommand::Show { json } => {
            let config = store.load_or_default()?;
            if json {
                print_json(&config)?;
            } else {
                print_goal(&config, store.path().display().to_string().as_str());
            }
        }
        GoalCommand::Plan { json } => {
            let graph = store.create_task_graph()?;
            if json {
                print_json(&graph)?;
            } else {
                print_task_graph(&graph);
            }
        }
        GoalCommand::Graph { json } => {
            let graphs = store.list_task_graphs()?;
            if json {
                print_json(&graphs)?;
            } else if let Some(graph) = graphs.last() {
                print_task_graph(graph);
            } else {
                println!("No goal task graphs recorded yet");
            }
        }
        GoalCommand::SetActive {
            id,
            title,
            success,
            targets,
            json,
        } => {
            let config = store.set_active(
                id,
                title,
                success,
                targets
                    .into_iter()
                    .map(|path| path.display().to_string())
                    .collect(),
            )?;
            if json {
                print_json(&config)?;
            } else {
                print_goal(&config, store.path().display().to_string().as_str());
            }
        }
        GoalCommand::Run { model, full, json } => {
            let config = store.load_or_default()?;
            let graph = store.create_task_graph()?;
            let alignment = config.alignment();
            let model = model.as_deref().or_else(|| config.execution_model());
            let proof = EvolutionStore::for_project(&root)
                .prove_self(
                    config.evolution_task(),
                    alignment.target_files.clone(),
                    model,
                    full || config.execution.full_validation,
                    Some(alignment),
                )
                .await?;
            let graph = store.complete_task_graph(
                graph,
                proof.verdict == EvolutionProofVerdict::SelfEvolutionProven,
            )?;
            if json {
                print_json(&serde_json::json!({
                    "task_graph": graph,
                    "proof": proof
                }))?;
            } else {
                println!("Goal-driven evolution proof: {}", proof.id);
                println!("Verdict: {:?}", proof.verdict);
                println!("Task graph: {}", graph.id);
                println!("Objective: {}", config.active_objective.id);
                println!("Proof JSON: {}", proof.proof_json_path);
                println!("Proof report: {}", proof.proof_report_path);
                if let Some(reason) = proof.failure_reason {
                    println!("Failure reason: {reason}");
                }
            }
        }
    }
    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_goal(config: &GoalConfig, path: &str) {
    println!("Octocode goals");
    println!("Path: {path}");
    println!("Name: {}", config.identity.name);
    println!("North star: {}", config.identity.north_star);
    println!(
        "Phase: {} ({})",
        config.current_phase.id, config.current_phase.name
    );
    println!(
        "Active objective: {} - {}",
        config.active_objective.id, config.active_objective.title
    );
    println!("Success criteria:");
    for item in &config.active_objective.success {
        println!("- {item}");
    }
    println!("Targets: {}", config.target_files().join(", "));
    println!("Constraints:");
    println!(
        "- no_direct_main_mutation={}",
        config.constraints.no_direct_main_mutation
    );
    println!(
        "- cloud_sandbox_required={}",
        config.constraints.cloud_sandbox_required
    );
    println!(
        "- no_secret_exposure={}",
        config.constraints.no_secret_exposure
    );
    println!(
        "- no_unverified_core_changes={}",
        config.constraints.no_unverified_core_changes
    );
    println!(
        "- one_primary_objective_per_evolution={}",
        config.constraints.one_primary_objective_per_evolution
    );
}

fn print_task_graph(graph: &GoalTaskGraph) {
    println!("Octocode goal task graph: {}", graph.id);
    println!(
        "Objective: {} - {}",
        graph.objective_id, graph.objective_title
    );
    println!("Phase: {}", graph.phase_id);
    println!("Status: {:?}", graph.status);
    for task in &graph.tasks {
        println!("- {} [{:?}]: {}", task.id, task.status, task.title);
    }
}
