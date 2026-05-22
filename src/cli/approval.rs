use std::io::{self, IsTerminal};

use crate::cli::inline_popup::{self, ApprovalChoice};
use crate::cli::ToolApprovalPolicy;
use crate::policy::{ApprovalDisplay, PolicyAction, PolicyDecision};

pub(crate) const DENIAL_REASON_POLICY_DENY: &str = "policy=deny";
pub(crate) const DENIAL_REASON_ASK_NO_TTY: &str = "policy=ask-no-tty";
pub(crate) const DENIAL_REASON_USER_DENY: &str = "user=deny";
pub(crate) const DENIAL_REASON_UI_ERROR: &str = "ui=error";
pub(crate) const DENIAL_REASON_APPROVAL_RESPONSE_CLOSED: &str = "approval=response-closed";
pub(crate) const DENIAL_REASON_APPROVAL_TIMEOUT: &str = "approval=timeout";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolApprovalDecision {
    pub approved: bool,
    pub denial_reason: Option<&'static str>,
}

impl ToolApprovalDecision {
    #[must_use]
    pub(crate) const fn approved() -> Self {
        Self {
            approved: true,
            denial_reason: None,
        }
    }

    #[must_use]
    pub(crate) const fn denied(reason: &'static str) -> Self {
        Self {
            approved: false,
            denial_reason: Some(reason),
        }
    }
}

#[must_use]
pub(crate) fn cli_prompt_tty() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

pub(crate) async fn resolve_tool_approval(
    policy: ToolApprovalPolicy,
    auto_approve_session: &mut bool,
    is_tty: bool,
    display: &ApprovalDisplay,
) -> ToolApprovalDecision {
    if let Some(decision) = resolve_non_interactive_policy(policy, *auto_approve_session, is_tty) {
        return decision;
    }

    match inline_popup::prompt_approval_inline(display).await {
        Ok(ApprovalChoice::ApproveOnce) => ToolApprovalDecision::approved(),
        Ok(ApprovalChoice::ApproveSession) => {
            *auto_approve_session = true;
            ToolApprovalDecision::approved()
        }
        Ok(ApprovalChoice::Deny) => ToolApprovalDecision::denied(DENIAL_REASON_USER_DENY),
        Err(error) => {
            tracing::warn!("tool approval UI failed: {error}");
            ToolApprovalDecision::denied(DENIAL_REASON_UI_ERROR)
        }
    }
}

pub(crate) async fn resolve_subagent_tool_approval(
    policy_decision: &PolicyDecision,
    cli_policy: ToolApprovalPolicy,
    auto_approve_session: &mut bool,
    is_tty: bool,
) -> ToolApprovalDecision {
    match policy_decision.action {
        PolicyAction::Allow => ToolApprovalDecision::approved(),
        PolicyAction::Deny => ToolApprovalDecision::denied(DENIAL_REASON_POLICY_DENY),
        PolicyAction::AskOnce | PolicyAction::AskSession => {
            resolve_tool_approval(
                cli_policy,
                auto_approve_session,
                is_tty,
                &policy_decision.display,
            )
            .await
        }
    }
}

fn resolve_non_interactive_policy(
    policy: ToolApprovalPolicy,
    auto_approve_session: bool,
    is_tty: bool,
) -> Option<ToolApprovalDecision> {
    if let Some(approved) = policy.auto_approved(auto_approve_session) {
        return Some(if approved {
            ToolApprovalDecision::approved()
        } else {
            ToolApprovalDecision::denied(DENIAL_REASON_POLICY_DENY)
        });
    }

    if !is_tty {
        tracing::warn!("tool approval policy ask requested without tty; denying");
        return Some(ToolApprovalDecision::denied(DENIAL_REASON_ASK_NO_TTY));
    }

    None
}

#[derive(Debug, Clone)]
pub enum ApprovalCommand {
    List { json: bool },
    Show { id: String },
    Approve { id: String, reason: Option<String> },
    Deny { id: String, reason: Option<String> },
}

pub async fn approval(
    command: ApprovalCommand,
    project_root: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let root = crate::cli::resolve_project_root(project_root, "approval")?;
    match command {
        ApprovalCommand::List { json } => {
            let pending = crate::mcp::approval_bridge::list_pending(&root).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&pending)?);
            } else if pending.is_empty() {
                println!("No pending approvals.");
            } else {
                for approval in pending {
                    println!(
                        "{}  {}  {}  expires {}",
                        approval.id, approval.tool, approval.risk, approval.expires_at
                    );
                }
            }
        }
        ApprovalCommand::Show { id } => {
            let approval = crate::mcp::approval_bridge::show_pending(&root, &id).await?;
            println!("{}", serde_json::to_string_pretty(&approval)?);
        }
        ApprovalCommand::Approve { id, reason } => {
            crate::mcp::approval_bridge::respond(
                &root,
                &id,
                true,
                reason.unwrap_or_else(|| "approved by operator".to_string()),
            )
            .await?;
            println!("Approved {id}");
        }
        ApprovalCommand::Deny { id, reason } => {
            crate::mcp::approval_bridge::respond(
                &root,
                &id,
                false,
                reason.unwrap_or_else(|| "denied by operator".to_string()),
            )
            .await?;
            println!("Denied {id}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{PolicyDecision, RiskLevel};

    fn sample_approval() -> ApprovalDisplay {
        ApprovalDisplay {
            title: "approve tool".to_string(),
            description: "run command".to_string(),
            risk_level: RiskLevel::CommandExecution,
            details: "echo hello".to_string(),
        }
    }

    #[tokio::test]
    async fn allow_policy_auto_approves() {
        let mut auto_approve_session = false;
        let decision = resolve_tool_approval(
            ToolApprovalPolicy::Allow,
            &mut auto_approve_session,
            false,
            &sample_approval(),
        )
        .await;

        assert_eq!(decision, ToolApprovalDecision::approved());
        assert!(!auto_approve_session);
    }

    #[tokio::test]
    async fn deny_policy_returns_policy_reason() {
        let mut auto_approve_session = false;
        let decision = resolve_tool_approval(
            ToolApprovalPolicy::Deny,
            &mut auto_approve_session,
            true,
            &sample_approval(),
        )
        .await;

        assert_eq!(
            decision,
            ToolApprovalDecision::denied(DENIAL_REASON_POLICY_DENY)
        );
    }

    #[tokio::test]
    async fn ask_without_tty_denies_with_no_tty_reason() {
        let mut auto_approve_session = false;
        let decision = resolve_tool_approval(
            ToolApprovalPolicy::Ask,
            &mut auto_approve_session,
            false,
            &sample_approval(),
        )
        .await;

        assert_eq!(
            decision,
            ToolApprovalDecision::denied(DENIAL_REASON_ASK_NO_TTY)
        );
    }

    #[tokio::test]
    async fn ask_auto_approve_session_continues_to_approve() {
        let mut auto_approve_session = true;
        let decision = resolve_tool_approval(
            ToolApprovalPolicy::Ask,
            &mut auto_approve_session,
            false,
            &sample_approval(),
        )
        .await;

        assert_eq!(decision, ToolApprovalDecision::approved());
        assert!(auto_approve_session);
    }

    #[tokio::test]
    async fn subagent_policy_deny_cannot_be_overridden_by_cli_allow() {
        let policy_decision = PolicyDecision::deny(
            "blocked",
            "blocked tool",
            "policy denied",
            RiskLevel::Blocked,
            "details",
        );
        let mut auto_approve_session = true;
        let decision = resolve_subagent_tool_approval(
            &policy_decision,
            ToolApprovalPolicy::Allow,
            &mut auto_approve_session,
            true,
        )
        .await;

        assert_eq!(
            decision,
            ToolApprovalDecision::denied(DENIAL_REASON_POLICY_DENY)
        );
        assert!(auto_approve_session);
    }
}
