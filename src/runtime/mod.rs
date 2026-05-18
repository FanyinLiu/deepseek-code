//! Runtime boundary models for the Octocode local-first agent workbench.
//!
//! The runtime kernel is intentionally small in this phase: it names the shared
//! services that CLI, TUI, mission, and task runners must converge on before
//! the orchestrator is split into dedicated controllers.

use serde::{Deserialize, Serialize};

/// Shared services that every execution surface should acquire from runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeKernel {
    /// Configuration and profile resolution.
    pub config: RuntimeComponent,
    /// Session snapshots and transcript access.
    pub session: RuntimeComponent,
    /// Durable event log and human-readable artifacts.
    pub audit: RuntimeComponent,
    /// Tool policy, approvals, and sandbox decisions.
    pub policy: RuntimeComponent,
    /// Tool registry and backend dispatch.
    pub tools: RuntimeComponent,
    /// Model client and stream handling.
    pub model: RuntimeComponent,
}

impl Default for RuntimeKernel {
    fn default() -> Self {
        Self {
            config: RuntimeComponent::new("config", RuntimeBoundary::Core),
            session: RuntimeComponent::new("session", RuntimeBoundary::Storage),
            audit: RuntimeComponent::new("audit", RuntimeBoundary::Storage),
            policy: RuntimeComponent::new("policy", RuntimeBoundary::Policy),
            tools: RuntimeComponent::new("tools", RuntimeBoundary::Tool),
            model: RuntimeComponent::new("model", RuntimeBoundary::Integration),
        }
    }
}

impl RuntimeKernel {
    /// Return the stable component list used by architecture docs and tests.
    #[must_use]
    pub fn components(&self) -> [&RuntimeComponent; 6] {
        [
            &self.config,
            &self.session,
            &self.audit,
            &self.policy,
            &self.tools,
            &self.model,
        ]
    }

    /// Validate that the kernel has one component for each required service.
    pub fn validate(&self) -> Result<(), RuntimeKernelError> {
        let mut names = std::collections::BTreeSet::new();
        for component in self.components() {
            if component.name.trim().is_empty() {
                return Err(RuntimeKernelError::EmptyComponentName);
            }
            if !names.insert(component.name.clone()) {
                return Err(RuntimeKernelError::DuplicateComponent(
                    component.name.clone(),
                ));
            }
        }
        Ok(())
    }
}

/// One named runtime service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeComponent {
    /// Stable component name.
    pub name: String,
    /// Owning subsystem.
    pub boundary: RuntimeBoundary,
}

impl RuntimeComponent {
    /// Create a component descriptor.
    #[must_use]
    pub fn new(name: impl Into<String>, boundary: RuntimeBoundary) -> Self {
        Self {
            name: name.into(),
            boundary,
        }
    }
}

/// Top-level architecture boundary for runtime ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeBoundary {
    Core,
    Agent,
    Tool,
    PlanMissionTask,
    Tui,
    Storage,
    Policy,
    Integration,
}

impl RuntimeBoundary {
    /// Stable label for docs, JSON, and CLI output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::PlanMissionTask => "plan-mission-task",
            Self::Tui => "tui",
            Self::Storage => "storage",
            Self::Policy => "policy",
            Self::Integration => "integration",
        }
    }
}

/// Public surface that consumes the runtime kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeSurface {
    Cli,
    Tui,
    TaskRunner,
    MissionRunner,
}

/// Validation errors for the runtime kernel descriptor.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeKernelError {
    #[error("runtime component name must not be empty")]
    EmptyComponentName,
    #[error("duplicate runtime component '{0}'")]
    DuplicateComponent(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_kernel_is_valid_and_complete() {
        let kernel = RuntimeKernel::default();

        kernel.validate().expect("kernel validates");
        let names: Vec<_> = kernel
            .components()
            .into_iter()
            .map(|component| component.name.as_str())
            .collect();

        assert_eq!(
            names,
            ["config", "session", "audit", "policy", "tools", "model"]
        );
    }
}
