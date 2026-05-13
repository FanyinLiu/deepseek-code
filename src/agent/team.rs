//! Canonical team-agent planning models.
//!
//! These types describe the product-level team runtime independently from the
//! existing swarm executor. The swarm layer can map to them while older
//! `AgentEvent` and TUI paths remain compatible.

use serde::{Deserialize, Serialize};

/// Specialized role assigned by the team coordinator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Coordinator,
    Explorer,
    Reviewer,
    Planner,
    Worker,
    Verifier,
    Tester,
}

impl AgentRole {
    /// Stable role label for JSON, logs, and UI.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coordinator => "coordinator",
            Self::Explorer => "explorer",
            Self::Reviewer => "reviewer",
            Self::Planner => "planner",
            Self::Worker => "worker",
            Self::Verifier => "verifier",
            Self::Tester => "tester",
        }
    }

    /// Whether the role may propose file changes.
    #[must_use]
    pub fn can_propose_patch(self) -> bool {
        matches!(self, Self::Worker)
    }

    /// Whether the role should stay read-only.
    #[must_use]
    pub fn is_read_only(self) -> bool {
        !self.can_propose_patch()
    }
}

/// Draft produced by a model or local planner before validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamPlanDraft {
    pub goal: String,
    #[serde(default)]
    pub tasks: Vec<TeamTask>,
    #[serde(default)]
    pub validation_commands: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

/// Validated team execution plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamPlan {
    pub goal: String,
    #[serde(default)]
    pub milestones: Vec<TeamMilestone>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub agent_roles: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<TeamTask>,
    #[serde(default)]
    pub validation_commands: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
}

impl TeamPlan {
    /// Return true when the plan contains duplicate role+title assignments.
    #[must_use]
    pub fn has_duplicate_task_targets(&self) -> bool {
        let mut seen = std::collections::BTreeSet::new();
        self.tasks.iter().any(|task| {
            let key = format!("{}:{}", task.role.as_str(), task.title.trim());
            !seen.insert(key)
        })
    }

    /// Return true when a read-only plan accidentally includes a worker task.
    #[must_use]
    pub fn violates_read_only(&self) -> bool {
        self.tasks
            .iter()
            .any(|task| task.mode == TeamTaskMode::ReadOnly && task.role.can_propose_patch())
    }
}

/// Milestone shown in artifacts and plan summaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMilestone {
    pub title: String,
    #[serde(default)]
    pub acceptance: Vec<String>,
}

/// One task assigned to one role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamTask {
    pub id: String,
    pub role: AgentRole,
    pub title: String,
    pub prompt: String,
    pub mode: TeamTaskMode,
    #[serde(default)]
    pub focus_files: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// Write boundary for a team task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamTaskMode {
    ReadOnly,
    PendingPatch,
    VerifyOnly,
}

/// Durable team run state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamRun {
    pub id: String,
    pub plan: TeamPlan,
    pub status: AgentRunState,
}

/// Shared state for team tasks and individual agent runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunState {
    Pending,
    Running,
    Succeeded,
    Failed,
    Blocked,
    Cancelled,
}

impl AgentRunState {
    /// Stable label for UI and event logs.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, role: AgentRole, title: &str, mode: TeamTaskMode) -> TeamTask {
        TeamTask {
            id: id.to_string(),
            role,
            title: title.to_string(),
            prompt: title.to_string(),
            mode,
            focus_files: Vec::new(),
            depends_on: Vec::new(),
        }
    }

    #[test]
    fn detects_duplicate_role_targets() {
        let plan = TeamPlan {
            goal: "审查蜂群".into(),
            milestones: Vec::new(),
            acceptance_criteria: Vec::new(),
            agent_roles: Vec::new(),
            tasks: vec![
                task("a", AgentRole::Explorer, "定位入口", TeamTaskMode::ReadOnly),
                task("b", AgentRole::Explorer, "定位入口", TeamTaskMode::ReadOnly),
            ],
            validation_commands: Vec::new(),
            risks: Vec::new(),
        };

        assert!(plan.has_duplicate_task_targets());
    }

    #[test]
    fn read_only_plan_rejects_worker() {
        let plan = TeamPlan {
            goal: "只读审查".into(),
            milestones: Vec::new(),
            acceptance_criteria: Vec::new(),
            agent_roles: Vec::new(),
            tasks: vec![task(
                "worker",
                AgentRole::Worker,
                "写补丁",
                TeamTaskMode::ReadOnly,
            )],
            validation_commands: Vec::new(),
            risks: Vec::new(),
        };

        assert!(plan.violates_read_only());
    }
}
