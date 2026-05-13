use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{MissionPlan, MissionState};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionEvent {
    pub id: Uuid,
    pub mission_id: String,
    pub at: DateTime<Utc>,
    #[serde(flatten)]
    pub kind: MissionEventKind,
}

impl MissionEvent {
    #[must_use]
    pub fn new(mission_id: impl Into<String>, kind: MissionEventKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            mission_id: mission_id.into(),
            at: Utc::now(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MissionEventKind {
    MissionCreated {
        state: MissionState,
    },
    PlanGenerated {
        plan: MissionPlan,
    },
    MissionCompleted {
        state: MissionState,
    },
    MissionFailed {
        message: String,
        state: MissionState,
    },
}
