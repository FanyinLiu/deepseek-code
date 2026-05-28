use crate::policy::{PermissionMode, PolicyAction, PolicyDecision};
use crate::runtime::tool_runtime::ApprovalOutcome;

pub(crate) fn permission_mode_approval_outcome(
    tool_name: &str,
    yolo_mode: bool,
    permission_mode: PermissionMode,
    decision: &PolicyDecision,
) -> Option<ApprovalOutcome> {
    if permission_mode.blocks_tool(tool_name) {
        return Some(ApprovalOutcome::denied(format!(
            "Skipped — `{}` mode keeps this kind of tool out of reach for now",
            permission_mode.as_str()
        )));
    }

    if yolo_mode
        || matches!(decision.action, PolicyAction::Allow)
        || permission_mode.auto_approves(tool_name, decision)
    {
        return Some(ApprovalOutcome::Approved);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ApprovalDisplay, PolicyAction, PolicyDecision, RiskLevel, ToolCallSource};

    fn ask_decision(risk_level: RiskLevel) -> PolicyDecision {
        PolicyDecision {
            source: ToolCallSource::Main,
            action: PolicyAction::AskOnce,
            reason: "requires approval".to_string(),
            display: ApprovalDisplay {
                title: "Approval".to_string(),
                description: "approval required".to_string(),
                risk_level,
                details: "details".to_string(),
            },
        }
    }

    #[test]
    fn read_only_blocks_mutating_tools() {
        let decision = ask_decision(RiskLevel::WriteProject);

        let outcome = permission_mode_approval_outcome(
            "write_file",
            false,
            PermissionMode::ReadOnly,
            &decision,
        )
        .expect("read-only should decide locally");

        assert!(!outcome.approved());
        assert!(outcome
            .reason()
            .expect("denied reason")
            .contains("read-only"));
    }

    #[test]
    fn read_only_blocks_subagent_launches() {
        let decision = ask_decision(RiskLevel::WriteProject);

        let outcome = permission_mode_approval_outcome(
            "run_subagent",
            false,
            PermissionMode::ReadOnly,
            &decision,
        )
        .expect("read-only should decide locally");

        assert!(!outcome.approved());
    }

    #[test]
    fn accept_edits_auto_approves_project_edits() {
        let decision = ask_decision(RiskLevel::WriteProject);

        let outcome = permission_mode_approval_outcome(
            "write_file",
            false,
            PermissionMode::AcceptEdits,
            &decision,
        )
        .expect("accept-edits should decide locally");

        assert!(outcome.approved());
    }

    #[test]
    fn accept_edits_keeps_subagent_launches_interactive() {
        let decision = ask_decision(RiskLevel::WriteProject);

        assert!(permission_mode_approval_outcome(
            "run_subagent",
            false,
            PermissionMode::AcceptEdits,
            &decision,
        )
        .is_none());
    }

    #[test]
    fn default_mode_leaves_ask_decisions_to_the_ui() {
        let decision = ask_decision(RiskLevel::CommandExecution);

        assert!(permission_mode_approval_outcome(
            "run_command",
            false,
            PermissionMode::Default,
            &decision,
        )
        .is_none());
    }
}
