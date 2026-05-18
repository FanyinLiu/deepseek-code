use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::types::{MissionPlan, MissionState};

/// Replayable event persisted in a mission's `events.jsonl` stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionEvent {
    /// Stable identifier for this event entry.
    pub id: Uuid,
    /// Mission identifier this event belongs to.
    pub mission_id: String,
    /// Timestamp for when this event was produced.
    pub at: DateTime<Utc>,
    /// Event payload.
    #[serde(flatten)]
    pub kind: MissionEventKind,
}

impl MissionEvent {
    /// Build a new mission event with a generated event id and current timestamp.
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

/// Payload variants for mission lifecycle events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MissionEventKind {
    /// Mission metadata and initial state were created.
    MissionCreated {
        /// Initial replayable mission state.
        state: MissionState,
    },
    /// A plan was generated for the mission.
    PlanGenerated {
        /// Generated plan payload.
        plan: MissionPlan,
    },
    /// The mission was started.
    MissionStarted {
        /// Replayable mission state.
        state: MissionState,
    },
    /// The mission was paused.
    MissionPaused {
        /// Replayable mission state.
        state: MissionState,
    },
    /// The mission was resumed.
    MissionResumed {
        /// Replayable mission state.
        state: MissionState,
    },
    /// A human note was attached to the mission timeline.
    MissionNote {
        /// Note text.
        message: String,
    },
    /// The mission completed successfully.
    MissionCompleted {
        /// Final replayable mission state.
        state: MissionState,
    },
    /// The mission failed before completion.
    MissionFailed {
        /// Human-readable failure reason.
        message: String,
        /// Final replayable mission state.
        state: MissionState,
    },
    /// The mission was cancelled before completion.
    MissionCancelled {
        /// Human-readable cancellation reason.
        message: String,
        /// Final replayable mission state.
        state: MissionState,
    },
}
