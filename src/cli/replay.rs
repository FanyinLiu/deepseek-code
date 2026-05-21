use chrono::{DateTime, Utc};

use crate::mission::{MissionEvent, MissionEventKind};
use crate::storage::{SessionEvent, SessionEventKind};

#[derive(Debug, Clone)]
pub struct ReplayLine {
    pub at: DateTime<Utc>,
    pub text: String,
}

pub fn print_replay_lines(title: &str, lines: &[ReplayLine], skipped: usize, footer: &[String]) {
    println!("{title}");
    for line in lines {
        println!("  {} {}", line.at, line.text);
    }
    if skipped > 0 {
        println!("  skipped malformed event lines: {skipped}");
    }
    if !footer.is_empty() {
        println!();
        for line in footer {
            println!("{line}");
        }
    }
}

pub fn session_line(event: &SessionEvent) -> ReplayLine {
    ReplayLine {
        at: event.at,
        text: match &event.kind {
            SessionEventKind::UserMessage { content } => format!("user: {}", one_line(content)),
            SessionEventKind::AssistantVisible { content } => {
                format!("assistant: {}", one_line(content))
            }
            SessionEventKind::AssistantInternal { .. } => "assistant internal".to_string(),
            SessionEventKind::ReasoningInternal { .. } => "reasoning internal".to_string(),
            SessionEventKind::ToolCallStarted { name, .. } => format!("tool started: {name}"),
            SessionEventKind::ToolCallFinished {
                name,
                success,
                summary,
                ..
            } => format!(
                "tool finished: {name} success={success} {}",
                one_line(summary)
            ),
            SessionEventKind::UserQuestionRequested {
                title,
                options,
                summary,
                ..
            } => format!(
                "question requested: {} ({} options) {}",
                one_line(title),
                options.len(),
                one_line(summary)
            ),
            SessionEventKind::ContextCompacted {
                before_tokens,
                after_tokens,
                before_messages,
                after_messages,
                reason,
                summary,
                ..
            } => format!(
                "context compacted {reason}: tokens {before_tokens}->{after_tokens} messages {before_messages}->{after_messages} {}",
                one_line(summary)
            ),
            SessionEventKind::HookExecuted {
                event,
                success,
                summary,
                command_count,
            } => format!("hook {event} success={success} commands={command_count}: {summary}"),
            SessionEventKind::PlanStarted { summary, total } => {
                format!("plan started: {summary} ({total})")
            }
            SessionEventKind::PlanStepUpdated {
                index,
                total,
                description,
                status,
            } => format!(
                "plan step {}/{} {status}: {}",
                index + 1,
                total,
                description
            ),
            SessionEventKind::SwarmStarted {
                run_id,
                summary,
                total,
            } => format!("swarm {run_id}: {summary} ({total})"),
            SessionEventKind::SwarmTaskUpdated {
                role,
                status,
                description,
                ..
            } => format!("swarm {role} {status}: {description}"),
            SessionEventKind::SwarmFinished {
                success, summary, ..
            } => format!("swarm finished success={success}: {summary}"),
            SessionEventKind::ScheduledTaskUpdated {
                task_id,
                status,
                summary,
            } => format!("task {task_id} {status}: {summary}"),
            SessionEventKind::SwarmPatchPending {
                summary,
                changed_files,
                ..
            } => format!("patch pending: {summary} [{}]", changed_files.join(", ")),
            SessionEventKind::SwarmPatchApplied {
                success, summary, ..
            } => format!("patch applied success={success}: {summary}"),
            SessionEventKind::ApprovalRequested {
                tool_name,
                risk_level,
                details,
            } => format!(
                "approval {tool_name} risk={risk_level}: {}",
                one_line(details)
            ),
            SessionEventKind::FileChanged { path, stats } => {
                format!("file changed {path}: {stats}")
            }
            SessionEventKind::TurnFinished { total_tokens } => {
                format!("turn finished tokens={total_tokens}")
            }
            SessionEventKind::Error { message } => format!("error: {message}"),
        },
    }
}

pub fn mission_line(event: &MissionEvent) -> ReplayLine {
    ReplayLine {
        at: event.at,
        text: mission_event_label(&event.kind),
    }
}

fn mission_event_label(kind: &MissionEventKind) -> String {
    match kind {
        MissionEventKind::MissionCreated { .. } => "mission_created".to_string(),
        MissionEventKind::PlanGenerated { .. } => "plan_generated".to_string(),
        MissionEventKind::MissionStarted { .. } => "mission_started".to_string(),
        MissionEventKind::MissionPaused { .. } => "mission_paused".to_string(),
        MissionEventKind::MissionResumed { .. } => "mission_resumed".to_string(),
        MissionEventKind::MissionNote { message } => format!("mission_note: {}", one_line(message)),
        MissionEventKind::MissionCompleted { .. } => "mission_completed".to_string(),
        MissionEventKind::MissionFailed { message, .. } => {
            format!("mission_failed: {}", one_line(message))
        }
        MissionEventKind::MissionCancelled { message, .. } => {
            format!("mission_cancelled: {}", one_line(message))
        }
    }
}

fn one_line(value: &str) -> String {
    value.lines().next().unwrap_or("").trim().to_string()
}
