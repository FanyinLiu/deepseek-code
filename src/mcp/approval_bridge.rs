use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::policy::PolicyDecision;
use crate::runtime::tool_runtime::{ApprovalFuture, ApprovalOutcome, ApprovalResolver, ToolCall};

pub const DEFAULT_APPROVAL_TIMEOUT_SECONDS: u64 = 60;
const APPROVAL_FILE_SIZE_CAP: u64 = crate::storage::TEXT_FILE_SIZE_CAP;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    pub source: String,
    pub tool: String,
    pub arguments_summary: String,
    pub risk: String,
    pub reason: String,
    pub title: String,
    pub description: String,
    pub details: String,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalResolution {
    pub id: String,
    pub approved: bool,
    pub reason: String,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct FileApprovalBridge {
    project_root: PathBuf,
    timeout: Duration,
}

impl FileApprovalBridge {
    #[must_use]
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            timeout: Duration::from_secs(DEFAULT_APPROVAL_TIMEOUT_SECONDS),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn create_pending(&self, call: &ToolCall, decision: &PolicyDecision) -> Result<String> {
        let id = format!("approval-{}", uuid::Uuid::new_v4());
        ensure_bridge_dirs(&self.project_root).await?;
        let now = Utc::now();
        let pending = PendingApproval {
            id: id.clone(),
            source: decision.source.as_str().to_string(),
            tool: call.tool.clone(),
            arguments_summary: summarize_arguments(&call.arguments),
            risk: decision.display.risk_level.to_string(),
            reason: decision.reason.clone(),
            title: decision.display.title.clone(),
            description: decision.display.description.clone(),
            details: decision.display.details.clone(),
            requested_at: now,
            expires_at: now
                + chrono::Duration::from_std(self.timeout).unwrap_or_else(|_| {
                    chrono::Duration::seconds(DEFAULT_APPROVAL_TIMEOUT_SECONDS as i64)
                }),
        };
        write_json_atomic(&pending_path(&self.project_root, &id), &pending).await?;
        Ok(id)
    }

    async fn wait_for_resolution(&self, id: &str) -> ApprovalOutcome {
        let deadline = tokio::time::Instant::now() + self.timeout;
        let resolution_path = resolution_path(&self.project_root, id);
        loop {
            match read_approval_file_capped(&resolution_path).await {
                Ok(contents) => match serde_json::from_str::<ApprovalResolution>(&contents) {
                    Ok(resolution) if resolution.id == id => {
                        let _ = fs::remove_file(pending_path(&self.project_root, id)).await;
                        let _ = fs::remove_file(&resolution_path).await;
                        return if resolution.approved {
                            ApprovalOutcome::Approved
                        } else {
                            ApprovalOutcome::denied(resolution.reason)
                        };
                    }
                    Ok(_) => return ApprovalOutcome::denied("Approval id mismatch"),
                    Err(error) => {
                        return ApprovalOutcome::denied(format!(
                            "Invalid approval resolution: {error}"
                        ));
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return ApprovalOutcome::denied(format!(
                        "Unable to read approval resolution: {error}"
                    ));
                }
            }
            if tokio::time::Instant::now() >= deadline {
                let _ = fs::remove_file(pending_path(&self.project_root, id)).await;
                return ApprovalOutcome::denied("Approval timed out");
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

impl ApprovalResolver for FileApprovalBridge {
    fn resolve<'a>(
        &'a mut self,
        call: &'a ToolCall,
        decision: &'a PolicyDecision,
    ) -> ApprovalFuture<'a> {
        Box::pin(async move {
            match self.create_pending(call, decision).await {
                Ok(id) => self.wait_for_resolution(&id).await,
                Err(error) => {
                    ApprovalOutcome::denied(format!("Unable to create approval request: {error}"))
                }
            }
        })
    }
}

pub async fn list_pending(project_root: &Path) -> Result<Vec<PendingApproval>> {
    let dir = approvals_root(project_root).join("pending");
    let mut approvals = Vec::new();
    let Ok(mut entries) = fs::read_dir(&dir).await else {
        return Ok(approvals);
    };
    while let Some(entry) = entries.next_entry().await? {
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let contents = read_approval_file_capped(entry.path()).await?;
        if let Ok(pending) = serde_json::from_str::<PendingApproval>(&contents) {
            approvals.push(pending);
        }
    }
    approvals.sort_by_key(|approval| approval.requested_at);
    Ok(approvals)
}

pub async fn show_pending(project_root: &Path, id: &str) -> Result<PendingApproval> {
    validate_id(id)?;
    let contents = read_approval_file_capped(pending_path(project_root, id))
        .await
        .with_context(|| format!("approval request not found: {id}"))?;
    serde_json::from_str(&contents).context("invalid approval request JSON")
}

pub async fn respond(
    project_root: &Path,
    id: &str,
    approved: bool,
    reason: impl Into<String>,
) -> Result<()> {
    validate_id(id)?;
    ensure_bridge_dirs(project_root).await?;
    let resolution = ApprovalResolution {
        id: id.to_string(),
        approved,
        reason: reason.into(),
        decided_at: Utc::now(),
    };
    write_json_atomic(&resolution_path(project_root, id), &resolution).await
}

fn approvals_root(project_root: &Path) -> PathBuf {
    project_root.join(".octocode").join("approvals")
}

fn pending_path(project_root: &Path, id: &str) -> PathBuf {
    approvals_root(project_root)
        .join("pending")
        .join(format!("{id}.json"))
}

fn resolution_path(project_root: &Path, id: &str) -> PathBuf {
    approvals_root(project_root)
        .join("resolved")
        .join(format!("{id}.json"))
}

async fn ensure_bridge_dirs(project_root: &Path) -> Result<()> {
    fs::create_dir_all(approvals_root(project_root).join("pending")).await?;
    fs::create_dir_all(approvals_root(project_root).join("resolved")).await?;
    Ok(())
}

async fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    crate::storage::atomic::write_json_pretty_atomic(path, value)
}

async fn read_approval_file_capped(path: impl AsRef<Path>) -> std::io::Result<String> {
    let path = path.as_ref();
    let size = fs::metadata(path).await?.len();
    if size > APPROVAL_FILE_SIZE_CAP {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file too large: {size} bytes, cap {APPROVAL_FILE_SIZE_CAP}"),
        ));
    }
    fs::read_to_string(path).await
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        anyhow::bail!("invalid approval id");
    }
    Ok(())
}

fn summarize_arguments(arguments: &str) -> String {
    let redacted = crate::policy::redact_all(arguments);
    if redacted.chars().count() <= 240 {
        return redacted;
    }
    let mut summary: String = redacted.chars().take(239).collect();
    summary.push('…');
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ApprovalDisplay, PolicyAction, RiskLevel, ToolCallSource};

    fn decision() -> PolicyDecision {
        PolicyDecision {
            source: ToolCallSource::Mcp,
            action: PolicyAction::AskOnce,
            reason: "test approval".to_string(),
            display: ApprovalDisplay {
                title: "Run tool".to_string(),
                description: "needs approval".to_string(),
                risk_level: RiskLevel::WriteProject,
                details: "Path: file.txt".to_string(),
            },
        }
    }

    #[tokio::test]
    async fn bridge_lists_and_resolves_pending_approval() {
        let root = tempfile::tempdir().expect("tempdir");
        let mut bridge = FileApprovalBridge::new(root.path()).with_timeout(Duration::from_secs(5));
        let call =
            ToolCall::new("write_file", r#"{"path":"file.txt"}"#).with_source(ToolCallSource::Mcp);
        let policy_decision = decision();
        let wait = tokio::spawn(async move { bridge.resolve(&call, &policy_decision).await });

        let id = loop {
            let pending = list_pending(root.path()).await.expect("list pending");
            if let Some(first) = pending.first() {
                break first.id.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        respond(root.path(), &id, true, "ok")
            .await
            .expect("write response");

        assert_eq!(wait.await.expect("bridge task"), ApprovalOutcome::Approved);
    }

    #[tokio::test]
    async fn approval_file_reads_reject_large_files() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = pending_path(root.path(), "approval-large");
        fs::create_dir_all(path.parent().expect("pending parent"))
            .await
            .expect("create pending dir");
        let file = std::fs::File::create(&path).expect("create approval file");
        file.set_len(APPROVAL_FILE_SIZE_CAP + 1)
            .expect("extend approval file");

        let error = show_pending(root.path(), "approval-large")
            .await
            .expect_err("large approval file should be rejected before read");
        assert!(format!("{error:#}").contains("file too large"));
    }
}
