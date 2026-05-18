use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::events::MissionEvent;

/// Persisted metadata for a mission run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mission {
    /// Stable mission identifier used for its on-disk directory.
    pub id: String,
    /// User-provided objective that the mission plan was generated from.
    pub goal: String,
    /// Whether the mission was planned without executing edits or commands.
    pub dry_run: bool,
    /// Execution mode recommended by the planner.
    pub recommended_mode: MissionMode,
    /// Project root the mission belongs to.
    pub project_root: PathBuf,
    /// Timestamp for when the mission record was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp for the most recent persisted mission update.
    pub updated_at: DateTime<Utc>,
}

impl Mission {
    /// Create a new dry-run mission record for a project.
    #[must_use]
    pub fn new_dry_run(goal: String, recommended_mode: MissionMode, project_root: PathBuf) -> Self {
        let now = Utc::now();
        Self {
            id: format!("mission-{}", Uuid::new_v4()),
            goal,
            dry_run: true,
            recommended_mode,
            project_root,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Planner-selected execution mode for a mission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MissionMode {
    /// A small change that can be handled directly.
    Direct,
    /// A task that should be planned before execution.
    Plan,
    /// A focused task for one specialized agent.
    AgentRun,
    /// Parallel work across multiple specialized agents.
    Swarm,
    /// A dry-run mission that records a replayable plan.
    MissionDryRun,
}

impl MissionMode {
    /// Return the stable CLI/display label for this mode.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Plan => "plan",
            Self::AgentRun => "agent-run",
            Self::Swarm => "swarm",
            Self::MissionDryRun => "mission-dry-run",
        }
    }
}

impl std::fmt::Display for MissionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Generated plan for a dry-run mission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionPlan {
    /// User-provided objective being planned.
    pub goal: String,
    /// Recommended execution mode for the mission.
    pub recommended_mode: MissionMode,
    /// Ordered plan steps.
    pub steps: Vec<MissionStep>,
    /// Agent names suggested for this plan.
    pub suggested_agents: Vec<String>,
    /// Validation commands expected after implementation.
    pub expected_validation_commands: Vec<String>,
    /// Risks or constraints the executor should keep in mind.
    pub risk_notes: Vec<String>,
}

/// One ordered step in a generated mission plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionStep {
    /// Stable step identifier within the plan.
    pub id: String,
    /// Human-readable step description.
    pub title: String,
    /// Step ids that must be completed before this step.
    pub depends_on: Vec<String>,
    /// Suggested agent for this step, if any.
    pub suggested_agent: Option<String>,
    /// Validation commands attached to this step.
    pub validation_commands: Vec<String>,
}

/// Current replayable state of a mission.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionState {
    /// Mission identifier this state belongs to.
    pub mission_id: String,
    /// Current mission lifecycle status.
    pub status: MissionStatus,
    /// Zero-based active step index, when a mission is in progress.
    pub current_step: Option<usize>,
    /// Total number of planned steps.
    pub total_steps: usize,
    /// Human-readable state summary.
    pub summary: String,
    /// Timestamp for when this state was produced.
    pub updated_at: DateTime<Utc>,
}

impl MissionState {
    /// Build the initial state for a newly created mission.
    #[must_use]
    pub fn created(mission: &Mission) -> Self {
        Self {
            mission_id: mission.id.clone(),
            status: MissionStatus::Created,
            current_step: None,
            total_steps: 0,
            summary: "mission created".to_string(),
            updated_at: mission.created_at,
        }
    }
}

/// Lifecycle status for mission state and summaries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    /// The mission record exists but no plan has been generated yet.
    Created,
    /// A plan has been generated.
    Planned,
    /// The mission has been started.
    Running,
    /// The mission is paused and can be resumed.
    Paused,
    /// The mission finished successfully.
    Completed,
    /// The mission failed before completion.
    Failed,
    /// The mission was cancelled before completion.
    Cancelled,
}

impl MissionStatus {
    /// Return the stable CLI/display label for this status.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Compact mission entry used by mission listings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionSummary {
    /// Stable mission identifier.
    pub id: String,
    /// User-provided objective that the mission plan was generated from.
    pub goal: String,
    /// Whether the mission was planned without executing edits or commands.
    pub dry_run: bool,
    /// Current mission lifecycle status.
    pub status: MissionStatus,
    /// Execution mode recommended by the planner.
    pub recommended_mode: MissionMode,
    /// Timestamp for when the mission record was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp for the most recent persisted mission update.
    pub updated_at: DateTime<Utc>,
}

/// Full mission payload loaded from mission storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionBundle {
    /// Persisted mission metadata.
    pub mission: Mission,
    /// Latest persisted mission state.
    pub state: MissionState,
    /// Generated dry-run plan.
    pub plan: MissionPlan,
    /// Replayable event stream, present when explicitly requested.
    #[serde(default)]
    pub events: Vec<MissionEvent>,
}
