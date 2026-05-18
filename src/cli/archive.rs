use anyhow::Result;
use std::path::PathBuf;

use crate::archive::{ArchiveCandidateView, ArchiveStatus, ArchiveStore, LineageEvent};
use crate::cli::resolve_project_root;

#[derive(Debug, Clone)]
pub enum ArchiveCommand {
    Status { json: bool },
    List { json: bool },
    Show { candidate_id: String, json: bool },
    Lineage { json: bool },
}

pub async fn archive(command: ArchiveCommand, project_root: Option<PathBuf>) -> Result<()> {
    let root = resolve_project_root(project_root, "archive")?;
    let store = ArchiveStore::for_project(root);
    match command {
        ArchiveCommand::Status { json } => {
            let status = store.status()?;
            if json {
                print_json(&status)?;
            } else {
                print_status(&status);
            }
        }
        ArchiveCommand::List { json } => {
            let candidates = store.list_candidate_views()?;
            if json {
                print_json(&candidates)?;
            } else if candidates.is_empty() {
                println!("no archive candidates");
            } else {
                println!("archive candidates ({})", candidates.len());
                for candidate in candidates {
                    print_candidate_row(&candidate);
                }
            }
        }
        ArchiveCommand::Show { candidate_id, json } => {
            let candidate = store.load_candidate_view(&candidate_id)?;
            if json {
                print_json(&candidate)?;
            } else {
                print_candidate_detail(&candidate);
            }
        }
        ArchiveCommand::Lineage { json } => {
            let events = store.list_lineage()?;
            if json {
                print_json(&events)?;
            } else if events.is_empty() {
                println!("no lineage events");
            } else {
                println!("lineage events ({})", events.len());
                for event in events {
                    print_lineage_event(&event);
                }
            }
        }
    }
    Ok(())
}

fn print_status(status: &ArchiveStatus) {
    println!("Octocode archive status");
    println!("Candidates: {}", status.candidates);
    println!("Lineage events: {}", status.lineage_events);
    println!("Passed: {}", status.passed);
    println!("Failed: {}", status.failed);
    println!("Blocked: {}", status.blocked);
    println!("Rejected: {}", status.rejected);
    println!("Rolled back: {}", status.rolled_back);
    println!("Disputed: {}", status.disputed);
}

fn print_candidate_row(candidate: &ArchiveCandidateView) {
    println!(
        "- {} [{}] score={} risk={} source={}",
        candidate.candidate.id,
        candidate.candidate.status,
        candidate.utility_score,
        candidate.candidate.risk_level,
        candidate.candidate.source
    );
}

fn print_candidate_detail(candidate: &ArchiveCandidateView) {
    println!("Candidate: {}", candidate.candidate.id);
    println!("Source: {}", candidate.candidate.source);
    println!("Proposal: {}", candidate.candidate.proposal_id);
    println!("Run: {}", candidate.candidate.run_id);
    println!("Status: {}", candidate.candidate.status);
    if let Some(status) = &candidate.candidate.sandbox_status {
        println!(
            "Sandbox: {} ({})",
            status,
            candidate
                .candidate
                .sandbox_backend
                .as_deref()
                .unwrap_or("unknown")
        );
    }
    println!("Risk: {}", candidate.candidate.risk_level);
    println!("Utility score: {}", candidate.utility_score);
    println!("Score reasons: {}", candidate.score_reasons.join(", "));
    println!("Touched: {}", candidate.candidate.touched_files.join(", "));
}

fn print_lineage_event(event: &LineageEvent) {
    println!(
        "- {} candidate={} parent={} run={} {}",
        event.event,
        event.candidate_id,
        event.parent_id.as_deref().unwrap_or("-"),
        event.run_id,
        event.summary
    );
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
