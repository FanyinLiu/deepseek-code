use anyhow::Result;
use std::path::PathBuf;

use crate::cli::resolve_project_root;
use crate::skill::{SkillStore, SkillTestReport};

#[derive(Debug, Clone)]
pub enum SkillCommand {
    List {
        json: bool,
    },
    Show {
        skill_id: String,
        json: bool,
    },
    Add {
        run_id: String,
        name: Option<String>,
        json: bool,
    },
    Test {
        skill_id: String,
        json: bool,
    },
}

pub async fn skill(command: SkillCommand, project_root: Option<PathBuf>) -> Result<()> {
    let root = resolve_project_root(project_root, "skill")?;
    let store = SkillStore::for_project(root);
    match command {
        SkillCommand::List { json } => {
            let skills = store.list()?;
            if json {
                print_json(&skills)?;
            } else if skills.is_empty() {
                println!("no skills");
            } else {
                println!("skills ({})", skills.len());
                for skill in skills {
                    println!(
                        "- {} [{:?}] from={}",
                        skill.id,
                        skill.status,
                        skill.created_from_run.as_deref().unwrap_or("-")
                    );
                }
            }
        }
        SkillCommand::Show { skill_id, json } => {
            let content = store.show(&skill_id)?;
            if json {
                print_json(&serde_json::json!({
                    "id": skill_id,
                    "content": content
                }))?;
            } else {
                println!("{content}");
            }
        }
        SkillCommand::Add { run_id, name, json } => {
            let metadata = store.add_from_repair_run(&run_id, name)?;
            if json {
                print_json(&metadata)?;
            } else {
                println!("Created draft skill: {}", metadata.id);
                println!("Status: {:?}", metadata.status);
                println!("Path: .octocode/skills/{}/SKILL.md", metadata.id);
            }
        }
        SkillCommand::Test { skill_id, json } => {
            let report = store.test(&skill_id)?;
            if json {
                print_json(&report)?;
            } else {
                print_test_report(&report);
            }
        }
    }
    Ok(())
}

fn print_test_report(report: &SkillTestReport) {
    println!("Skill test: {}", report.id);
    println!("Status: {:?}", report.status);
    for check in &report.checks {
        println!("- {}: {} ({})", check.name, check.passed, check.message);
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
