use crate::policy::{PolicyAction, PolicyDecision, RiskLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Default,
    ReadOnly,
    AcceptEdits,
    Plan,
    Auto,
    Bypass,
}

impl PermissionMode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::ReadOnly => "read-only",
            Self::AcceptEdits => "accept-edits",
            Self::Plan => "plan",
            Self::Auto => "auto",
            Self::Bypass => "bypass",
        }
    }

    #[must_use]
    pub fn visible_str(self) -> &'static str {
        match self {
            Self::Default => "ask",
            Self::ReadOnly | Self::Plan => "plan",
            Self::AcceptEdits => "accept-edits",
            Self::Auto | Self::Bypass => "yolo",
        }
    }

    #[must_use]
    pub fn from_visible_or_legacy(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "ask" | "default" => Some(Self::Default),
            "read-only" => Some(Self::ReadOnly),
            "accept-edits" => Some(Self::AcceptEdits),
            "plan" => Some(Self::Plan),
            "yolo" | "auto" | "bypass" => Some(Self::Bypass),
            _ => None,
        }
    }

    #[must_use]
    pub fn blocks_tool(self, tool_name: &str) -> bool {
        match self {
            Self::ReadOnly | Self::Plan => !crate::tools::metadata::is_read_only(tool_name),
            Self::Default | Self::AcceptEdits | Self::Auto | Self::Bypass => false,
        }
    }

    #[must_use]
    pub fn auto_approves(self, tool_name: &str, decision: &PolicyDecision) -> bool {
        if matches!(decision.action, PolicyAction::Deny) {
            return false;
        }
        match self {
            Self::Bypass | Self::Auto => true,
            Self::ReadOnly | Self::Plan => crate::tools::metadata::is_read_only(tool_name),
            Self::AcceptEdits => {
                tool_name != "run_command"
                    && tool_name != "run_subagent"
                    && !matches!(
                        decision.display.risk_level,
                        RiskLevel::SensitiveRead | RiskLevel::NetworkAccess | RiskLevel::Blocked
                    )
            }
            Self::Default => matches!(decision.action, PolicyAction::Allow),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ApprovalDisplay, ToolCallSource};

    fn decision(action: PolicyAction, risk_level: RiskLevel) -> PolicyDecision {
        PolicyDecision {
            source: ToolCallSource::Main,
            action,
            reason: "test".to_string(),
            display: ApprovalDisplay {
                title: "test".to_string(),
                description: "test".to_string(),
                risk_level,
                details: String::new(),
            },
        }
    }

    #[test]
    fn read_only_and_plan_block_mutating_tools() {
        assert!(PermissionMode::ReadOnly.blocks_tool("write_file"));
        assert!(PermissionMode::Plan.blocks_tool("run_command"));
        assert!(!PermissionMode::ReadOnly.blocks_tool("read_file"));
    }

    #[test]
    fn accept_edits_does_not_auto_approve_commands_or_sensitive_reads() {
        assert!(!PermissionMode::AcceptEdits.auto_approves(
            "run_command",
            &decision(PolicyAction::AskOnce, RiskLevel::CommandExecution)
        ));
        assert!(!PermissionMode::AcceptEdits.auto_approves(
            "read_file",
            &decision(PolicyAction::AskOnce, RiskLevel::SensitiveRead)
        ));
        assert!(PermissionMode::AcceptEdits.auto_approves(
            "write_file",
            &decision(PolicyAction::AskOnce, RiskLevel::WriteProject)
        ));
    }

    #[test]
    fn visible_permission_modes_use_public_labels() {
        assert_eq!(PermissionMode::Default.visible_str(), "ask");
        assert_eq!(PermissionMode::AcceptEdits.visible_str(), "accept-edits");
        assert_eq!(PermissionMode::Plan.visible_str(), "plan");
        assert_eq!(PermissionMode::Bypass.visible_str(), "yolo");
        assert_eq!(
            PermissionMode::from_visible_or_legacy("accept_edits"),
            Some(PermissionMode::AcceptEdits)
        );
        assert_eq!(
            PermissionMode::from_visible_or_legacy("bypass"),
            Some(PermissionMode::Bypass)
        );
    }
}
