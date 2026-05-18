use std::path::PathBuf;

use serde::Serialize;

use crate::cli::resolve_project_root;
use crate::mission::{MissionBundle, MissionEvent, MissionState, MissionSummary};
use crate::storage::MissionStore;

pub enum MissionCommand {
    New {
        task: String,
        dry_run: bool,
        json: bool,
    },
    Status {
        target: Option<String>,
        json: bool,
    },
    Inspect {
        target: Option<String>,
        json: bool,
        events: bool,
    },
    Replay {
        target: Option<String>,
        json: bool,
    },
    List {
        json: bool,
        status: Option<crate::mission::MissionStatus>,
        limit: Option<usize>,
    },
    Start {
        target: Option<String>,
        json: bool,
    },
    Pause {
        target: Option<String>,
        json: bool,
    },
    Resume {
        target: Option<String>,
        json: bool,
    },
    Complete {
        target: Option<String>,
        json: bool,
    },
    Fail {
        target: Option<String>,
        message: String,
        json: bool,
    },
    Cancel {
        target: Option<String>,
        message: String,
        json: bool,
    },
    Note {
        target: Option<String>,
        message: String,
        json: bool,
    },
}

pub async fn mission(
    command: MissionCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "mission")?;
    let store = MissionStore::for_project(&root);

    match command {
        MissionCommand::New {
            task,
            dry_run,
            json,
        } => {
            if !dry_run {
                anyhow::bail!("only --dry-run missions are supported in this release");
            }
            let bundle = store.create_dry_run(task, root)?;
            let payload = MissionInspectPayload::from_bundle(bundle, 0);
            if json {
                print_json(&payload)?;
            } else {
                print_new_mission(&payload);
            }
        }
        MissionCommand::Status { target, json } => {
            let target = target.unwrap_or_else(|| "latest".to_string());
            let state = store.load_state(&target)?;
            if json {
                print_json(&state)?;
            } else {
                print_status(&state);
            }
        }
        MissionCommand::Inspect {
            target,
            json,
            events,
        } => {
            let target = target.unwrap_or_else(|| "latest".to_string());
            let event_load = events
                .then(|| store.load_events_lossy(&target))
                .transpose()?;
            let bundle = store.load_bundle(&target, false)?;
            let mut payload = MissionInspectPayload::from_bundle(bundle, 0);
            if let Some(event_load) = event_load {
                payload.events = event_load.events;
                payload.skipped_malformed_event_lines = event_load.skipped_malformed_lines;
            }
            if json {
                print_json(&payload)?;
            } else {
                print_inspect(&payload, events);
            }
        }
        MissionCommand::Replay { target, json } => {
            let target = target.unwrap_or_else(|| "latest".to_string());
            let event_load = store.load_events_lossy(&target)?;
            let state = store.replay_state(&target)?;
            let payload = MissionReplayPayload {
                events: event_load.events,
                skipped_malformed_event_lines: event_load.skipped_malformed_lines,
                state,
            };
            if json {
                print_json(&payload)?;
            } else {
                print_replay(&payload);
            }
        }
        MissionCommand::List {
            json,
            status,
            limit,
        } => {
            let mut missions = store.list()?;
            if let Some(status) = status {
                missions.retain(|mission| mission.status == status);
            }
            if let Some(limit) = limit {
                missions.truncate(limit);
            }
            let list = MissionListPayload {
                project_root: root.display().to_string(),
                store: store.root().display().to_string(),
                missions,
            };
            if json {
                print_json(&list)?;
            } else {
                print_list(&list);
            }
        }
        MissionCommand::Start { target, json } => {
            print_bundle_result(
                store.start(&target.unwrap_or_else(|| "latest".to_string()))?,
                json,
            )?;
        }
        MissionCommand::Pause { target, json } => {
            print_bundle_result(
                store.pause(&target.unwrap_or_else(|| "latest".to_string()))?,
                json,
            )?;
        }
        MissionCommand::Resume { target, json } => {
            print_bundle_result(
                store.resume(&target.unwrap_or_else(|| "latest".to_string()))?,
                json,
            )?;
        }
        MissionCommand::Complete { target, json } => {
            print_bundle_result(
                store.complete(&target.unwrap_or_else(|| "latest".to_string()))?,
                json,
            )?;
        }
        MissionCommand::Fail {
            target,
            message,
            json,
        } => {
            print_bundle_result(
                store.fail(&target.unwrap_or_else(|| "latest".to_string()), message)?,
                json,
            )?;
        }
        MissionCommand::Cancel {
            target,
            message,
            json,
        } => {
            print_bundle_result(
                store.cancel(&target.unwrap_or_else(|| "latest".to_string()), message)?,
                json,
            )?;
        }
        MissionCommand::Note {
            target,
            message,
            json,
        } => {
            print_bundle_result(
                store.add_note(&target.unwrap_or_else(|| "latest".to_string()), message)?,
                json,
            )?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionInspectPayload {
    pub mission: crate::mission::Mission,
    pub state: MissionState,
    pub plan: crate::mission::MissionPlan,
    pub events: Vec<MissionEvent>,
    pub skipped_malformed_event_lines: usize,
}

impl MissionInspectPayload {
    fn from_bundle(bundle: MissionBundle, skipped_malformed_event_lines: usize) -> Self {
        Self {
            mission: bundle.mission,
            state: bundle.state,
            plan: bundle.plan,
            events: bundle.events,
            skipped_malformed_event_lines,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionListPayload {
    pub project_root: String,
    pub store: String,
    pub missions: Vec<MissionSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissionReplayPayload {
    pub events: Vec<MissionEvent>,
    pub skipped_malformed_event_lines: usize,
    pub state: MissionState,
}

fn print_bundle_result(bundle: MissionBundle, json: bool) -> Result<(), anyhow::Error> {
    let payload = MissionInspectPayload::from_bundle(bundle, 0);
    if json {
        print_json(&payload)?;
    } else {
        print_status(&payload.state);
    }
    Ok(())
}

fn print_new_mission(payload: &MissionInspectPayload) {
    println!("created mission {}", payload.mission.id);
    println!("goal     {}", payload.mission.goal);
    println!("mode     {}", payload.mission.recommended_mode);
    println!("status   {}", payload.state.status.as_str());
    println!("steps    {}", payload.plan.steps.len());
    println!("store    .octocode/missions/{}", payload.mission.id);
}

fn print_status(state: &MissionState) {
    println!("mission {}", state.mission_id);
    println!("status  {}", state.status.as_str());
    println!("steps   {}", state.total_steps);
    println!("summary {}", state.summary);
    println!("updated {}", state.updated_at);
}

fn print_inspect(payload: &MissionInspectPayload, include_events: bool) {
    println!("mission {}", payload.mission.id);
    println!("goal    {}", payload.mission.goal);
    println!("dry-run {}", payload.mission.dry_run);
    println!("mode    {}", payload.mission.recommended_mode);
    println!("status  {}", payload.state.status.as_str());
    println!();
    println!("Plan");
    for step in &payload.plan.steps {
        let agent = step.suggested_agent.as_deref().unwrap_or("-");
        println!("  {} [{}] {}", step.id, agent, step.title);
        if !step.validation_commands.is_empty() {
            println!("    validate: {}", step.validation_commands.join(" && "));
        }
    }
    if !payload.plan.risk_notes.is_empty() {
        println!();
        println!("Risks");
        for note in &payload.plan.risk_notes {
            println!("  - {note}");
        }
    }
    if include_events {
        println!();
        println!("Events");
        for event in &payload.events {
            println!("  {} {}", event.at, event_kind_label(event));
        }
        if payload.skipped_malformed_event_lines > 0 {
            println!(
                "  skipped malformed event lines: {}",
                payload.skipped_malformed_event_lines
            );
        }
    }
}

fn print_replay(payload: &MissionReplayPayload) {
    let lines = payload
        .events
        .iter()
        .map(crate::cli::replay::mission_line)
        .collect::<Vec<_>>();
    let footer = vec![
        format!("replayed status  {}", payload.state.status.as_str()),
        format!("replayed summary {}", payload.state.summary),
    ];
    crate::cli::replay::print_replay_lines(
        "Mission replay",
        &lines,
        payload.skipped_malformed_event_lines,
        &footer,
    );
}

fn print_list(payload: &MissionListPayload) {
    if payload.missions.is_empty() {
        println!("no missions");
        println!("store {}", payload.store);
        return;
    }
    println!("missions ({})", payload.missions.len());
    for mission in &payload.missions {
        println!(
            "{}  {}  {}  {}",
            mission.id,
            mission.status.as_str(),
            mission.recommended_mode,
            truncate(&mission.goal, 80)
        );
    }
}

fn event_kind_label(event: &MissionEvent) -> String {
    match &event.kind {
        crate::mission::MissionEventKind::MissionCreated { .. } => "mission_created".to_string(),
        crate::mission::MissionEventKind::PlanGenerated { .. } => "plan_generated".to_string(),
        crate::mission::MissionEventKind::MissionStarted { .. } => "mission_started".to_string(),
        crate::mission::MissionEventKind::MissionPaused { .. } => "mission_paused".to_string(),
        crate::mission::MissionEventKind::MissionResumed { .. } => "mission_resumed".to_string(),
        crate::mission::MissionEventKind::MissionNote { message } => {
            format!("mission_note {message}")
        }
        crate::mission::MissionEventKind::MissionCompleted { .. } => {
            "mission_completed".to_string()
        }
        crate::mission::MissionEventKind::MissionFailed { message, .. } => {
            format!("mission_failed {message}")
        }
        crate::mission::MissionEventKind::MissionCancelled { message, .. } => {
            format!("mission_cancelled {message}")
        }
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), anyhow::Error> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let suffix = "...";
    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(suffix.len()))
        .collect::<String>();
    out.push_str(suffix);
    out
}
