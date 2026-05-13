use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::events::MissionEvent;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Mission {
    pub id: String,
    pub goal: String,
    pub dry_run: bool,
    pub recommended_mode: MissionMode,
    pub project_root: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Mission {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MissionMode {
    Direct,
    Plan,
    AgentRun,
    Swarm,
    MissionDryRun,
}

impl MissionMode {
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
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionPlan {
    pub goal: String,
    pub recommended_mode: MissionMode,
    pub steps: Vec<MissionStep>,
    pub suggested_agents: Vec<String>,
    pub expected_validation_commands: Vec<String>,
    pub risk_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionStep {
    pub id: String,
    pub title: String,
    pub depends_on: Vec<String>,
    pub suggested_agent: Option<String>,
    pub validation_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionState {
    pub mission_id: String,
    pub status: MissionStatus,
    pub current_step: Option<usize>,
    pub total_steps: usize,
    pub summary: String,
    pub updated_at: DateTime<Utc>,
}

impl MissionState {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Created,
    Planned,
    Completed,
    Failed,
}

impl MissionStatus {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Planned => "planned",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionSummary {
    pub id: String,
    pub goal: String,
    pub dry_run: bool,
    pub status: MissionStatus,
    pub recommended_mode: MissionMode,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionBundle {
    pub mission: Mission,
    pub state: MissionState,
    pub plan: MissionPlan,
    #[serde(default)]
    pub events: Vec<MissionEvent>,
}
