use anyhow::Result;
use std::path::PathBuf;

use crate::cli::resolve_project_root;
use crate::knowledge::KnowledgeStore;

#[derive(Debug, Clone)]
pub enum KnowledgeCommand {
    Update { json: bool },
    Show { topic: KnowledgeTopic, json: bool },
}

#[derive(Debug, Clone)]
pub enum KnowledgeTopic {
    Project,
    RiskMap,
    Failures,
}

pub async fn knowledge(command: KnowledgeCommand, project_root: Option<PathBuf>) -> Result<()> {
    let root = resolve_project_root(project_root, "knowledge")?;
    let store = KnowledgeStore::for_project(root);
    match command {
        KnowledgeCommand::Update { json } => {
            let update = store.update_project_knowledge()?;
            if json {
                print_json(&update)?;
            } else {
                println!("Updated project knowledge");
                println!("Project: {}", update.project_root);
                println!("Knowledge: {}", update.project_path);
                println!("Risk map: {}", update.risk_map_path);
                println!("Failure memory: {}", update.failure_memory_path);
            }
        }
        KnowledgeCommand::Show { topic, json } => match topic {
            KnowledgeTopic::Project => {
                let update = store.update_project_knowledge()?;
                let content = crate::storage::read_text_file_capped(&update.project_path)?;
                if json {
                    print_json(&serde_json::json!({
                        "path": update.project_path,
                        "content": content
                    }))?;
                } else {
                    println!("{content}");
                }
            }
            KnowledgeTopic::RiskMap => {
                let risk_map = store.load_or_create_risk_map()?;
                if json {
                    print_json(&risk_map)?;
                } else {
                    println!("Risk map v{}", risk_map.version);
                    for rule in risk_map.paths {
                        println!(
                            "- {}: {:?} auto_apply_allowed={}",
                            rule.pattern, rule.risk_level, rule.auto_apply_allowed
                        );
                    }
                }
            }
            KnowledgeTopic::Failures => {
                let failures = store.list_failures()?;
                if json {
                    print_json(&failures)?;
                } else if failures.is_empty() {
                    println!("no failure memory entries");
                } else {
                    println!("failure memory ({})", failures.len());
                    for failure in failures {
                        println!("- {} [{}] {}", failure.id, failure.source, failure.symptom);
                    }
                }
            }
        },
    }
    Ok(())
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
