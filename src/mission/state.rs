use chrono::Utc;

use super::events::{MissionEvent, MissionEventKind};
use super::types::{MissionMode, MissionPlan, MissionState, MissionStatus, MissionStep};

#[must_use]
pub fn generate_dry_run_plan(goal: &str) -> MissionPlan {
    let mode = recommend_mode(goal);
    let suggested_agents = suggested_agents(goal, &mode);
    let validation = validation_commands(goal);
    let risk_notes = risk_notes(goal, &mode);
    let steps = plan_steps(goal, &mode, &suggested_agents, &validation);

    MissionPlan {
        goal: goal.to_string(),
        recommended_mode: mode,
        steps,
        suggested_agents,
        expected_validation_commands: validation,
        risk_notes,
    }
}

#[must_use]
pub fn replay_state_from_events(events: &[MissionEvent]) -> Option<MissionState> {
    let mut state = None;
    for event in events {
        match &event.kind {
            MissionEventKind::MissionCreated { state: next } => {
                state = Some(next.clone());
            }
            MissionEventKind::PlanGenerated { plan } => {
                state = Some(MissionState {
                    mission_id: event.mission_id.clone(),
                    status: MissionStatus::Planned,
                    current_step: Some(0),
                    total_steps: plan.steps.len(),
                    summary: format!("plan generated with {} steps", plan.steps.len()),
                    updated_at: event.at,
                });
            }
            MissionEventKind::MissionCompleted { state: next }
            | MissionEventKind::MissionFailed { state: next, .. } => {
                state = Some(next.clone());
            }
        }
    }
    state
}

fn recommend_mode(goal: &str) -> MissionMode {
    let lower = goal.to_lowercase();
    let words = lower.split_whitespace().count();
    let asks_review = contains_any(&lower, &["review", "audit", "security", "safety"]);
    let asks_explain = contains_any(
        &lower,
        &["explain", "analyze", "understand", "architecture"],
    );
    let asks_refactor = contains_any(&lower, &["refactor", "runtime", "rewrite", "migration"]);
    let asks_tests = contains_any(&lower, &["test", "tests", "cargo test", "clippy"]);
    let mentions_many_files =
        lower.matches("src/").count() >= 2 || lower.contains("codebase") || lower.contains("all ");
    let small_edit =
        contains_any(&lower, &["typo", "wording", "small"]) && !mentions_many_files && words <= 8;

    if asks_refactor && (asks_tests || mentions_many_files || words > 8) {
        MissionMode::MissionDryRun
    } else if asks_review && mentions_many_files {
        MissionMode::Swarm
    } else if asks_review || asks_explain {
        MissionMode::AgentRun
    } else if small_edit {
        MissionMode::Direct
    } else {
        MissionMode::Plan
    }
}

fn suggested_agents(goal: &str, mode: &MissionMode) -> Vec<String> {
    let lower = goal.to_lowercase();
    match mode {
        MissionMode::Swarm => {
            if lower.contains("security") || lower.contains("safety") {
                vec![
                    "security-auditor".to_string(),
                    "code-reviewer".to_string(),
                    "test-runner".to_string(),
                ]
            } else {
                vec!["code-reviewer".to_string(), "test-runner".to_string()]
            }
        }
        MissionMode::AgentRun => {
            if lower.contains("review") {
                vec!["code-reviewer".to_string()]
            } else if lower.contains("security") || lower.contains("safety") {
                vec!["security-auditor".to_string()]
            } else {
                vec!["code-explorer".to_string()]
            }
        }
        MissionMode::MissionDryRun => vec![
            "planner".to_string(),
            "code-explorer".to_string(),
            "test-runner".to_string(),
        ],
        MissionMode::Plan => vec!["planner".to_string()],
        MissionMode::Direct => Vec::new(),
    }
}

fn validation_commands(goal: &str) -> Vec<String> {
    let lower = goal.to_lowercase();
    let mut commands = vec!["cargo check --all-targets --all-features".to_string()];
    if lower.contains("fmt") || lower.contains("format") || lower.contains("refactor") {
        commands.push("cargo fmt --all --check".to_string());
    }
    if lower.contains("test") || lower.contains("refactor") || lower.contains("agent") {
        commands.push("cargo test".to_string());
    }
    if lower.contains("clippy") || lower.contains("safety") || lower.contains("security") {
        commands.push("cargo clippy --all-targets --all-features".to_string());
    }
    commands
}

fn risk_notes(goal: &str, mode: &MissionMode) -> Vec<String> {
    let mut notes = Vec::new();
    let lower = goal.to_lowercase();
    if matches!(mode, MissionMode::MissionDryRun | MissionMode::Swarm) {
        notes.push("coordinate file ownership before parallel edits".to_string());
    }
    if lower.contains("security") || lower.contains("safety") {
        notes.push("run read-only security audit before any write-capable work".to_string());
    }
    if lower.contains("runtime") || lower.contains("storage") {
        notes.push("preserve backwards-compatible on-disk formats".to_string());
    }
    if notes.is_empty() {
        notes.push("keep changes scoped and rerun targeted validation".to_string());
    }
    notes
}

fn plan_steps(
    goal: &str,
    mode: &MissionMode,
    suggested_agents: &[String],
    validation_commands: &[String],
) -> Vec<MissionStep> {
    let agent = suggested_agents.first().cloned();
    let mut steps = vec![
        MissionStep {
            id: "step-1".to_string(),
            title: format!("Clarify goal and inspect current state: {goal}"),
            depends_on: Vec::new(),
            suggested_agent: Some("code-explorer".to_string()),
            validation_commands: Vec::new(),
        },
        MissionStep {
            id: "step-2".to_string(),
            title: format!("Choose execution mode: {}", mode.as_str()),
            depends_on: vec!["step-1".to_string()],
            suggested_agent: agent,
            validation_commands: Vec::new(),
        },
        MissionStep {
            id: "step-3".to_string(),
            title: "Implement the smallest useful slice".to_string(),
            depends_on: vec!["step-2".to_string()],
            suggested_agent: suggested_agents.first().cloned(),
            validation_commands: Vec::new(),
        },
        MissionStep {
            id: "step-4".to_string(),
            title: "Run targeted validation and record findings".to_string(),
            depends_on: vec!["step-3".to_string()],
            suggested_agent: Some("test-runner".to_string()),
            validation_commands: validation_commands.to_vec(),
        },
    ];

    if matches!(mode, MissionMode::Swarm | MissionMode::MissionDryRun) {
        steps.insert(
            2,
            MissionStep {
                id: "step-2a".to_string(),
                title: "Split work by module and assign non-overlapping ownership".to_string(),
                depends_on: vec!["step-2".to_string()],
                suggested_agent: Some("planner".to_string()),
                validation_commands: Vec::new(),
            },
        );
        if let Some(step) = steps.iter_mut().find(|step| step.id == "step-3") {
            step.depends_on = vec!["step-2a".to_string()];
        }
    }

    for step in &mut steps {
        if step.validation_commands.is_empty() && step.id == "step-4" {
            step.validation_commands = validation_commands.to_vec();
        }
    }

    steps
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[must_use]
pub fn completed_state(mission_id: &str, step_count: usize) -> MissionState {
    MissionState {
        mission_id: mission_id.to_string(),
        status: MissionStatus::Completed,
        current_step: None,
        total_steps: step_count,
        summary: format!("dry-run mission completed with {step_count} planned steps"),
        updated_at: Utc::now(),
    }
}
