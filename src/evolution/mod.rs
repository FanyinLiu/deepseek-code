use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use uuid::Uuid;

use crate::archive::ArchiveStore;
use crate::deepseek::{
    ChatMessage, ChatMessageContent, ChatRequest, ResponseFormat, ThinkingConfig,
};
use crate::goal::GoalAlignment;
use crate::provider::{build_provider, parse_model, Provider};
use crate::remote_sandbox::SandboxStatus;
use crate::repair::{NewRepairProposal, RepairRun, RepairRunStatus, RepairStore};
use crate::storage;
use crate::workspace::apply::{apply_patch, parse_patch_paths};
use crate::workspace::diff::unified_diff;

pub const EVOLUTION_DIR: &str = ".octocode/evolution";

const HIGH_RISK_PREFIXES: &[&str] = &["src/policy/", "src/defense/"];
const HIGH_RISK_FILES: &[&str] = &[
    "src/storage/keyring.rs",
    "src/tools/run_command.rs",
    "src/tools/dispatch.rs",
    "src/cli/login.rs",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionProposal {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub title: String,
    pub problem: String,
    pub target_area: String,
    pub target_files: Vec<String>,
    pub risk_level: RiskLevel,
    pub required_gates: Vec<EvolutionGateKind>,
    pub manual_review_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionRun {
    pub id: String,
    pub proposal_id: String,
    pub created_at: DateTime<Utc>,
    pub status: EvolutionRunStatus,
    pub manual_review_required: bool,
    pub touched_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionApply {
    pub id: String,
    pub run_id: String,
    pub proposal_id: String,
    pub created_at: DateTime<Utc>,
    pub status: EvolutionApplyStatus,
    pub allow_high_risk: bool,
    pub manual_review_required: bool,
    pub touched_files: Vec<String>,
    pub checkpoint_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionRollback {
    pub id: String,
    pub apply_id: String,
    pub created_at: DateTime<Utc>,
    pub status: EvolutionRollbackStatus,
    pub restored_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionGateReport {
    pub run_id: String,
    pub checked_at: DateTime<Utc>,
    pub status: EvolutionRunStatus,
    pub gates: Vec<EvolutionGateResult>,
    pub commands: Vec<EvolutionCommandCheck>,
    pub touched_files: Vec<String>,
    pub high_risk_files: Vec<String>,
    pub manual_review_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionCommandCheck {
    pub command: Vec<String>,
    pub status: GateStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionGateResult {
    pub name: EvolutionGateKind,
    pub status: GateStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionInspection {
    pub project_root: String,
    pub proposals: usize,
    pub runs: usize,
    pub applies: usize,
    pub rollbacks: usize,
    pub recommendations: Vec<EvolutionRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionProof {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_alignment: Option<GoalAlignment>,
    #[serde(default)]
    pub meaningfulness: EvolutionMeaningfulness,
    pub verdict: EvolutionProofVerdict,
    pub evolution_proposal_id: String,
    pub repair_proposal_id: String,
    pub repair_run_id: String,
    pub archive_candidate_id: Option<String>,
    pub patch_hash: Option<String>,
    pub sandbox_backend: Option<String>,
    pub sandbox_status: Option<String>,
    pub sandbox_run_id: Option<String>,
    pub sandbox_artifact_url: Option<String>,
    pub evidence: Vec<String>,
    pub failure_reason: Option<String>,
    pub proof_json_path: String,
    pub proof_report_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionProofVerdict {
    SelfEvolutionProven,
    NotProven,
    NotMeaningfulEvolution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionMeaningfulness {
    pub status: MeaningfulnessStatus,
    pub reasons: Vec<String>,
    pub matched_targets: Vec<String>,
}

impl Default for EvolutionMeaningfulness {
    fn default() -> Self {
        Self {
            status: MeaningfulnessStatus::Pass,
            reasons: vec!["legacy proof without recorded meaningfulness".to_string()],
            matched_targets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeaningfulnessStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionBenchmark {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub rounds_requested: usize,
    pub rounds: Vec<EvolutionProof>,
    pub successes: usize,
    pub failures: usize,
    pub success_rate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionRecommendation {
    pub area: String,
    pub title: String,
    pub target_files: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionCandidate {
    pub proposal_id: String,
    pub run_id: String,
    pub generated_at: DateTime<Utc>,
    pub summary: String,
    pub agents: Vec<EvolutionAgentDraft>,
    pub model_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvolutionAgentDraft {
    pub role: EvolutionAgentRole,
    pub source: EvolutionAgentSource,
    pub title: String,
    pub findings: Vec<String>,
    pub target_files: Vec<String>,
    pub recommendation: String,
    pub patch: Option<String>,
    pub veto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionAgentRole {
    Planner,
    Implementer,
    SafetyReviewer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionAgentSource {
    Local,
    Model,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionGateKind {
    PatchPresent,
    CurrentTreeUnchanged,
    HighRiskReview,
    SafetyRegressionScan,
    TestDeletionScan,
    ValidationCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionRunStatus {
    Proposed,
    PatchGenerated,
    Passed,
    NeedsManualReview,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionApplyStatus {
    Applied,
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionRollbackStatus {
    RolledBack,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CheckpointManifest {
    entries: Vec<CheckpointEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CheckpointEntry {
    path: String,
    existed: bool,
    checkpoint_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewEvolutionProposal {
    pub title: Option<String>,
    pub problem: Option<String>,
    pub target_area: String,
    pub target_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EvolutionStore {
    project_root: PathBuf,
    root: PathBuf,
}

impl EvolutionStore {
    pub fn for_project(project_root: impl AsRef<Path>) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let root = project_root.join(EVOLUTION_DIR);
        Self { project_root, root }
    }

    pub fn inspect(&self) -> Result<EvolutionInspection> {
        self.ensure_dirs()?;
        Ok(EvolutionInspection {
            project_root: self.project_root.display().to_string(),
            proposals: self.list_proposals()?.len(),
            runs: self.list_runs()?.len(),
            applies: self.list_applies()?.len(),
            rollbacks: self.list_rollbacks()?.len(),
            recommendations: default_recommendations(),
        })
    }

    pub fn create_proposal(&self, input: NewEvolutionProposal) -> Result<EvolutionProposal> {
        self.ensure_dirs()?;
        let id = format!("proposal-{}", Uuid::new_v4());
        let target_files = normalize_targets(input.target_files);
        let manual_review_required = target_files.iter().any(|path| is_high_risk_path(path));
        let risk_level = if manual_review_required {
            RiskLevel::High
        } else if target_files.is_empty() {
            RiskLevel::Low
        } else {
            RiskLevel::Medium
        };
        let title = input
            .title
            .unwrap_or_else(|| format!("Improve Octocode {} capability", input.target_area));
        let problem = input.problem.unwrap_or_else(|| {
            format!(
                "Octocode should evolve its {} implementation through an isolated proposal, patch, and gate loop before any source change is applied.",
                input.target_area
            )
        });
        let proposal = EvolutionProposal {
            id,
            created_at: Utc::now(),
            title,
            problem,
            target_area: input.target_area,
            target_files,
            risk_level,
            required_gates: default_required_gates(),
            manual_review_required,
        };
        self.write_proposal(&proposal)?;
        Ok(proposal)
    }

    pub async fn prove_self(
        &self,
        task: String,
        target_files: Vec<String>,
        model_override: Option<&str>,
        full_validation: bool,
        goal_alignment: Option<GoalAlignment>,
    ) -> Result<EvolutionProof> {
        self.ensure_dirs()?;
        let proof_id = format!("proof-{}", Uuid::new_v4());
        let targets = if target_files.is_empty() {
            vec!["src/cli/archive.rs".to_string()]
        } else {
            target_files
        };
        let proposal = self.create_proposal(NewEvolutionProposal {
            title: Some(format!("Self-evolution proof: {}", concise_task(&task))),
            problem: Some(format!(
                "Prove Octocode can improve its own code through repair, remote sandbox validation, archive evidence, and a durable proof report. Task: {task}. Goal alignment: {}",
                goal_alignment
                    .as_ref()
                    .map(|alignment| alignment.summary.as_str())
                    .unwrap_or("not attached")
            )),
            target_area: goal_alignment
                .as_ref()
                .map(|alignment| format!("goal:{}", alignment.objective_id))
                .unwrap_or_else(|| "self-evolution-proof".to_string()),
            target_files: targets,
        })?;
        let repair_store = RepairStore::for_project(&self.project_root);
        let repair_proposal = repair_store.create_proposal(NewRepairProposal {
            title: Some(format!("Self-evolution proof repair: {}", proposal.title)),
            problem: proposal.problem.clone(),
            targets: proposal.target_files.clone(),
        })?;
        let repair_run = repair_store
            .run_proposal(&repair_proposal.id, full_validation, true, model_override)
            .await?;
        let archive_candidate_id = ArchiveStore::for_project(&self.project_root)
            .list_candidate_views()?
            .into_iter()
            .find(|view| view.candidate.run_id == repair_run.id)
            .map(|view| view.candidate.id);
        let mut proof = build_proof(
            proof_id,
            task,
            &proposal.id,
            &repair_proposal.id,
            &repair_run,
            archive_candidate_id,
            goal_alignment,
        );
        proof.proof_json_path = self.proof_json_path(&proof.id).display().to_string();
        proof.proof_report_path = self.proof_report_path(&proof.id).display().to_string();
        self.write_proof(&proof)?;
        Ok(proof)
    }

    pub async fn benchmark_from_goal(
        &self,
        goals: &crate::goal::GoalConfig,
        rounds: usize,
        model_override: Option<&str>,
        full_validation: bool,
    ) -> Result<EvolutionBenchmark> {
        self.ensure_dirs()?;
        let rounds = rounds.max(1);
        let mut proofs = Vec::with_capacity(rounds);
        for _ in 0..rounds {
            let alignment = goals.alignment();
            let proof = self
                .prove_self(
                    goals.evolution_task(),
                    alignment.target_files.clone(),
                    model_override.or_else(|| goals.execution_model()),
                    full_validation || goals.execution.full_validation,
                    Some(alignment),
                )
                .await?;
            proofs.push(proof);
        }
        let successes = proofs
            .iter()
            .filter(|proof| proof.verdict == EvolutionProofVerdict::SelfEvolutionProven)
            .count();
        let failures = proofs.len().saturating_sub(successes);
        let benchmark = EvolutionBenchmark {
            id: format!("benchmark-{}", Uuid::new_v4()),
            created_at: Utc::now(),
            rounds_requested: rounds,
            success_rate: format!("{successes}/{rounds}"),
            rounds: proofs,
            successes,
            failures,
        };
        let path = self
            .root
            .join("benchmarks")
            .join(format!("{}.json", benchmark.id));
        write_json(&path, &benchmark)?;
        Ok(benchmark)
    }

    pub fn list_memory_events(&self) -> Result<Vec<serde_json::Value>> {
        let path = self.memory_dir().join("events.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = crate::storage::read_text_file_capped(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        data.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).context("invalid evolution memory event"))
            .collect()
    }

    pub fn load_proposal(&self, proposal_id: &str) -> Result<EvolutionProposal> {
        let path = self.proposals_dir().join(format!("{proposal_id}.json"));
        let data = crate::storage::read_text_file_capped(&path)
            .with_context(|| format!("failed to read proposal {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("invalid proposal {}", path.display()))
    }

    pub fn list_proposals(&self) -> Result<Vec<EvolutionProposal>> {
        let mut proposals = Vec::new();
        let dir = self.proposals_dir();
        if !dir.exists() {
            return Ok(proposals);
        }
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let data = crate::storage::read_text_file_capped(&path)
                .with_context(|| format!("failed to read proposal {}", path.display()))?;
            proposals.push(
                serde_json::from_str(&data)
                    .with_context(|| format!("invalid proposal {}", path.display()))?,
            );
        }
        proposals.sort_by_key(|proposal| proposal.created_at);
        Ok(proposals)
    }

    pub fn create_patch_run(&self, proposal_id: &str) -> Result<EvolutionRun> {
        self.ensure_dirs()?;
        let proposal = self.load_proposal(proposal_id)?;
        let run_id = format!("run-{}", Uuid::new_v4());
        let candidate = self.build_candidate(&proposal, &run_id)?;
        self.create_patch_run_from_candidate(proposal, run_id, candidate)
    }

    pub async fn create_model_patch_run(
        &self,
        proposal_id: &str,
        model_override: Option<&str>,
    ) -> Result<EvolutionRun> {
        self.ensure_dirs()?;
        let proposal = self.load_proposal(proposal_id)?;
        let run_id = format!("run-{}", Uuid::new_v4());
        let local_candidate = self.build_candidate(&proposal, &run_id)?;
        let candidate = self
            .try_build_model_candidate(&proposal, &run_id, local_candidate, model_override)
            .await;
        self.create_patch_run_from_candidate(proposal, run_id, candidate)
    }

    fn create_patch_run_from_candidate(
        &self,
        proposal: EvolutionProposal,
        run_id: String,
        candidate: EvolutionCandidate,
    ) -> Result<EvolutionRun> {
        let run_dir = self.run_dir(&run_id);
        fs::create_dir_all(run_dir.join("worktree"))
            .with_context(|| format!("failed to create {}", run_dir.display()))?;
        write_json(&run_dir.join("proposal.json"), &proposal)?;
        write_json(&run_dir.join("agents.json"), &candidate.agents)?;
        write_json(&run_dir.join("candidate.json"), &candidate)?;
        fs::write(run_dir.join("candidate.md"), candidate_markdown(&candidate))
            .with_context(|| format!("failed to write candidate for {run_id}"))?;
        fs::write(
            run_dir.join("worktree/README.md"),
            isolated_worktree_readme(&proposal),
        )
        .with_context(|| format!("failed to write isolated worktree for {run_id}"))?;
        fs::write(
            run_dir.join("patch.diff"),
            self.build_proposed_patch(&proposal, &candidate)?,
        )
        .with_context(|| format!("failed to write patch for {run_id}"))?;
        let touched_files = proposal_target_files(&proposal);
        let manual_review_required = proposal.manual_review_required
            || touched_files.iter().any(|path| is_high_risk_path(path));
        let run = EvolutionRun {
            id: run_id.clone(),
            proposal_id: proposal.id,
            created_at: Utc::now(),
            status: EvolutionRunStatus::PatchGenerated,
            manual_review_required,
            touched_files,
        };
        write_json(&run_dir.join("run.json"), &run)?;
        Ok(run)
    }

    pub fn load_run(&self, run_id: &str) -> Result<EvolutionRun> {
        let path = self.run_dir(run_id).join("run.json");
        let data = crate::storage::read_text_file_capped(&path)
            .with_context(|| format!("failed to read run {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("invalid run {}", path.display()))
    }

    pub fn list_runs(&self) -> Result<Vec<EvolutionRun>> {
        let mut runs = Vec::new();
        let dir = self.runs_dir();
        if !dir.exists() {
            return Ok(runs);
        }
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            let run_path = path.join("run.json");
            if !run_path.exists() {
                continue;
            }
            let data = crate::storage::read_text_file_capped(&run_path)
                .with_context(|| format!("failed to read run {}", run_path.display()))?;
            runs.push(
                serde_json::from_str(&data)
                    .with_context(|| format!("invalid run {}", run_path.display()))?,
            );
        }
        runs.sort_by_key(|run| run.created_at);
        Ok(runs)
    }

    pub fn list_applies(&self) -> Result<Vec<EvolutionApply>> {
        let mut applies = Vec::new();
        let dir = self.applies_dir();
        if !dir.exists() {
            return Ok(applies);
        }
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path().join("apply.json");
            if !path.exists() {
                continue;
            }
            let data = crate::storage::read_text_file_capped(&path)
                .with_context(|| format!("failed to read apply {}", path.display()))?;
            applies.push(
                serde_json::from_str(&data)
                    .with_context(|| format!("invalid apply {}", path.display()))?,
            );
        }
        applies.sort_by_key(|apply| apply.created_at);
        Ok(applies)
    }

    pub fn list_rollbacks(&self) -> Result<Vec<EvolutionRollback>> {
        let mut rollbacks = Vec::new();
        let dir = self.rollbacks_dir();
        if !dir.exists() {
            return Ok(rollbacks);
        }
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let data = crate::storage::read_text_file_capped(&path)
                .with_context(|| format!("failed to read rollback {}", path.display()))?;
            rollbacks.push(
                serde_json::from_str(&data)
                    .with_context(|| format!("invalid rollback {}", path.display()))?,
            );
        }
        rollbacks.sort_by_key(|rollback| rollback.created_at);
        Ok(rollbacks)
    }

    pub fn load_apply(&self, apply_id: &str) -> Result<EvolutionApply> {
        let path = self.apply_dir(apply_id).join("apply.json");
        let data = crate::storage::read_text_file_capped(&path)
            .with_context(|| format!("failed to read apply {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("invalid apply {}", path.display()))
    }

    pub fn run_gates(&self, run_id: &str, full_validation: bool) -> Result<EvolutionGateReport> {
        let mut run = self.load_run(run_id)?;
        let run_dir = self.run_dir(run_id);
        let patch_path = run_dir.join("patch.diff");
        let patch = crate::storage::read_text_file_capped(&patch_path)
            .with_context(|| format!("failed to read patch {}", patch_path.display()))?;
        let touched_files = changed_files_from_patch(&patch);
        let high_risk_files: Vec<String> = touched_files
            .iter()
            .filter(|path| is_high_risk_path(path))
            .cloned()
            .collect();
        let mut gates = base_gates(&patch, &touched_files, &high_risk_files);
        let commands = self.run_validation_commands(full_validation);
        if !commands.is_empty() {
            let command_failed = commands
                .iter()
                .any(|command| command.status == GateStatus::Fail);
            gates.push(EvolutionGateResult {
                name: EvolutionGateKind::ValidationCommand,
                status: if command_failed {
                    GateStatus::Fail
                } else {
                    GateStatus::Pass
                },
                message: if command_failed {
                    "one or more validation commands failed".to_string()
                } else {
                    "validation commands completed successfully".to_string()
                },
            });
        }
        let failed = gates.iter().any(|gate| gate.status == GateStatus::Fail);
        let manual_review_required = run.manual_review_required || !high_risk_files.is_empty();
        let status = if failed {
            EvolutionRunStatus::Failed
        } else if manual_review_required {
            EvolutionRunStatus::NeedsManualReview
        } else {
            EvolutionRunStatus::Passed
        };
        run.status = status.clone();
        run.manual_review_required = manual_review_required;
        run.touched_files = touched_files.clone();
        write_json(&run_dir.join("run.json"), &run)?;
        let report = EvolutionGateReport {
            run_id: run_id.to_string(),
            checked_at: Utc::now(),
            status,
            gates,
            commands,
            touched_files,
            high_risk_files,
            manual_review_required,
        };
        write_json(&run_dir.join("gates.json"), &report)?;
        fs::write(run_dir.join("stdout.log"), gate_stdout(&report))
            .with_context(|| format!("failed to write stdout log for {run_id}"))?;
        fs::write(run_dir.join("stderr.log"), gate_stderr(&report))
            .with_context(|| format!("failed to write stderr log for {run_id}"))?;
        fs::write(run_dir.join("report.md"), gate_report_markdown(&report))
            .with_context(|| format!("failed to write report for {run_id}"))?;
        Ok(report)
    }

    pub fn apply_run(&self, run_id: &str, allow_high_risk: bool) -> Result<EvolutionApply> {
        self.ensure_dirs()?;
        let run = self.load_run(run_id)?;
        let report = self.load_gate_report(run_id).with_context(|| {
            format!("run {run_id} has no gates.json; run `octocode evolve test {run_id}` first")
        })?;
        if report.status == EvolutionRunStatus::Failed {
            bail!("run {run_id} failed gates and cannot be applied");
        }
        if report.manual_review_required && !allow_high_risk {
            bail!(
                "run {run_id} requires manual review; pass --allow-high-risk to apply after review"
            );
        }
        let patch_path = self.run_dir(run_id).join("patch.diff");
        let patch = crate::storage::read_text_file_capped(&patch_path)
            .with_context(|| format!("failed to read patch {}", patch_path.display()))?;
        let touched_files = if report.touched_files.is_empty() {
            changed_files_from_patch(&patch)
        } else {
            report.touched_files.clone()
        };
        if touched_files.is_empty() {
            bail!("run {run_id} does not touch any files");
        }
        let high_risk_files: Vec<String> = touched_files
            .iter()
            .filter(|path| is_high_risk_path(path))
            .cloned()
            .collect();
        if !high_risk_files.is_empty() && !allow_high_risk {
            bail!(
                "high-risk files require --allow-high-risk: {}",
                high_risk_files.join(", ")
            );
        }
        let apply_id = format!("apply-{}", Uuid::new_v4());
        let apply_dir = self.apply_dir(&apply_id);
        let checkpoint_dir = apply_dir.join("checkpoint");
        fs::create_dir_all(&checkpoint_dir)
            .with_context(|| format!("failed to create {}", checkpoint_dir.display()))?;
        let manifest = self.create_checkpoint(&checkpoint_dir, &touched_files)?;
        write_json(&checkpoint_dir.join("manifest.json"), &manifest)?;
        let mut apply = EvolutionApply {
            id: apply_id.clone(),
            run_id: run_id.to_string(),
            proposal_id: run.proposal_id,
            created_at: Utc::now(),
            status: EvolutionApplyStatus::Applied,
            allow_high_risk,
            manual_review_required: report.manual_review_required,
            touched_files: touched_files.clone(),
            checkpoint_dir: checkpoint_dir.display().to_string(),
        };
        if let Err(err) = apply_patch(&self.project_root, &patch) {
            let _ = self.restore_checkpoint(&manifest);
            apply.status = EvolutionApplyStatus::Failed;
            write_json(&apply_dir.join("apply.json"), &apply)?;
            bail!("failed to apply evolution patch: {err}");
        }
        write_json(&apply_dir.join("apply.json"), &apply)?;
        Ok(apply)
    }

    pub fn rollback_apply(&self, apply_id: &str) -> Result<EvolutionRollback> {
        self.ensure_dirs()?;
        let mut apply = self.load_apply(apply_id)?;
        if apply.status == EvolutionApplyStatus::RolledBack {
            bail!("apply {apply_id} has already been rolled back");
        }
        if apply.status == EvolutionApplyStatus::Failed {
            bail!("apply {apply_id} failed and has no successful source change to roll back");
        }
        let manifest_path = self
            .apply_dir(apply_id)
            .join("checkpoint")
            .join("manifest.json");
        let data = crate::storage::read_text_file_capped(&manifest_path)
            .with_context(|| format!("failed to read checkpoint {}", manifest_path.display()))?;
        let manifest: CheckpointManifest = serde_json::from_str(&data)
            .with_context(|| format!("invalid checkpoint {}", manifest_path.display()))?;
        self.restore_checkpoint(&manifest)?;
        apply.status = EvolutionApplyStatus::RolledBack;
        write_json(&self.apply_dir(apply_id).join("apply.json"), &apply)?;
        let rollback = EvolutionRollback {
            id: format!("rollback-{}", Uuid::new_v4()),
            apply_id: apply_id.to_string(),
            created_at: Utc::now(),
            status: EvolutionRollbackStatus::RolledBack,
            restored_files: manifest
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect(),
        };
        write_json(
            &self.rollbacks_dir().join(format!("{}.json", rollback.id)),
            &rollback,
        )?;
        Ok(rollback)
    }

    fn build_candidate(
        &self,
        proposal: &EvolutionProposal,
        run_id: &str,
    ) -> Result<EvolutionCandidate> {
        let targets = proposal_target_files(proposal);
        let file_signals = targets
            .iter()
            .map(|target| self.target_signal(target))
            .collect::<Result<Vec<_>>>()?;
        let high_risk_targets: Vec<String> = targets
            .iter()
            .filter(|target| is_high_risk_path(target))
            .cloned()
            .collect();
        let agents = vec![
            EvolutionAgentDraft {
                role: EvolutionAgentRole::Planner,
                source: EvolutionAgentSource::Local,
                title: format!("Plan evolution for {}", proposal.target_area),
                findings: vec![
                    format!("proposal {} targets {}", proposal.id, targets.join(", ")),
                    format!("observed targets: {}", file_signals.join("; ")),
                ],
                target_files: targets.clone(),
                recommendation:
                    "Generate a narrow candidate patch first, then require gates before apply."
                        .to_string(),
                patch: None,
                veto: false,
            },
            EvolutionAgentDraft {
                role: EvolutionAgentRole::Implementer,
                source: EvolutionAgentSource::Local,
                title: "Generate candidate diff".to_string(),
                findings: vec![
                    "candidate patch is generated inside the evolution run directory".to_string(),
                    "active source tree is not modified during patch generation".to_string(),
                ],
                target_files: targets.clone(),
                recommendation: if targets
                    .iter()
                    .any(|target| self.project_root.join(target).exists())
                {
                    "Append a compact evolution marker to an existing target so the diff is reviewable and reversible."
                        .to_string()
                } else {
                    "Create a new evolution candidate artifact for review.".to_string()
                },
                patch: None,
                veto: false,
            },
            EvolutionAgentDraft {
                role: EvolutionAgentRole::SafetyReviewer,
                source: EvolutionAgentSource::Local,
                title: "Review risk boundaries".to_string(),
                findings: if high_risk_targets.is_empty() {
                    vec!["no high-risk target paths detected".to_string()]
                } else {
                    vec![format!(
                        "manual review required for high-risk targets: {}",
                        high_risk_targets.join(", ")
                    )]
                },
                target_files: high_risk_targets.clone(),
                recommendation: if high_risk_targets.is_empty() {
                    "Allow normal gates to decide whether this run can be applied.".to_string()
                } else {
                    "Block automatic apply; require explicit high-risk override after manual review."
                        .to_string()
                },
                patch: None,
                veto: !high_risk_targets.is_empty(),
            },
        ];
        Ok(EvolutionCandidate {
            proposal_id: proposal.id.clone(),
            run_id: run_id.to_string(),
            generated_at: Utc::now(),
            summary: format!(
                "Local multi-agent candidate for {} with {} target(s)",
                proposal.target_area,
                targets.len()
            ),
            agents,
            model_errors: Vec::new(),
        })
    }

    async fn try_build_model_candidate(
        &self,
        proposal: &EvolutionProposal,
        run_id: &str,
        mut candidate: EvolutionCandidate,
        model_override: Option<&str>,
    ) -> EvolutionCandidate {
        let config = match storage::Config::load(Some(&self.project_root)) {
            Ok(config) => config,
            Err(err) => {
                candidate
                    .model_errors
                    .push(format!("failed to load config: {err}"));
                return candidate;
            }
        };
        let Some(api_key) = storage::get_effective_api_key(Some(&self.project_root)) else {
            candidate
                .model_errors
                .push("no API key configured; used local fallback agents".to_string());
            return candidate;
        };
        let model = match model_override {
            Some(value) => match parse_model(value) {
                Ok(model) => model.canonical(),
                Err(err) => {
                    candidate
                        .model_errors
                        .push(format!("invalid model override '{value}': {err}"));
                    return candidate;
                }
            },
            None => config.model.default.canonical(),
        };
        let provider = build_provider(&config.provider, api_key);
        let client = provider.create_deepseek_client();
        let target_context = self.target_context_for_prompt(proposal);
        let mut model_agents = Vec::new();
        for role in [
            EvolutionAgentRole::Planner,
            EvolutionAgentRole::Implementer,
            EvolutionAgentRole::SafetyReviewer,
        ] {
            match self
                .run_model_evolution_agent(
                    &client,
                    &model.to_string(),
                    proposal,
                    run_id,
                    &role,
                    &target_context,
                )
                .await
            {
                Ok(agent) => model_agents.push(agent),
                Err(err) => candidate
                    .model_errors
                    .push(format!("{role:?} model agent failed: {err}")),
            }
        }
        if !model_agents.is_empty() {
            candidate.summary = format!(
                "Model-assisted evolution candidate for {} with {} model agent(s)",
                proposal.target_area,
                model_agents.len()
            );
            candidate.agents = merge_model_agents(candidate.agents, model_agents);
        }
        candidate
    }

    async fn run_model_evolution_agent(
        &self,
        client: &crate::deepseek::client::DeepSeekClient,
        model: &str,
        proposal: &EvolutionProposal,
        run_id: &str,
        role: &EvolutionAgentRole,
        target_context: &str,
    ) -> Result<EvolutionAgentDraft> {
        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: Some(ChatMessageContent::Text(model_agent_system_prompt(role))),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: Some(ChatMessageContent::Text(model_agent_user_prompt(
                        proposal,
                        run_id,
                        role,
                        target_context,
                    ))),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: None,
                    name: None,
                },
            ],
            tools: None,
            thinking: Some(ThinkingConfig::disabled()),
            response_format: Some(ResponseFormat::json_object()),
            stream: false,
            max_tokens: Some(1800),
        };
        let response = client.chat_with_retry(&request).await?;
        let content = response
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .map(ChatMessageContent::to_string_lossy)
            .unwrap_or_default();
        Ok(parse_model_agent_response(role.clone(), &content, proposal))
    }

    fn target_context_for_prompt(&self, proposal: &EvolutionProposal) -> String {
        proposal_target_files(proposal)
            .iter()
            .map(|target| self.target_excerpt(target))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn target_excerpt(&self, target: &str) -> String {
        let Ok(relative) = checked_relative_path(target) else {
            return format!("## {target}\ninvalid path");
        };
        let path = self.project_root.join(relative);
        if path.is_dir() {
            return format!("## {target}\ndirectory target");
        }
        match crate::storage::read_text_file_capped(&path) {
            Ok(content) => {
                let excerpt = truncate_for_prompt(&content, 6000);
                format!("## {target}\n```text\n{excerpt}\n```")
            }
            Err(_) => format!("## {target}\nmissing or unreadable target"),
        }
    }

    fn build_proposed_patch(
        &self,
        proposal: &EvolutionProposal,
        candidate: &EvolutionCandidate,
    ) -> Result<String> {
        if let Some(patch) = model_patch_from_candidate(candidate, &proposal_target_files(proposal))
        {
            return Ok(patch);
        }
        let target = proposal_target_files(proposal)
            .first()
            .cloned()
            .unwrap_or_else(|| format!("docs/evolution/{}.md", proposal.id));
        let relative = checked_relative_path(&target)?;
        let active_path = self.project_root.join(&relative);
        let content = candidate_patch_content(proposal, candidate);
        if active_path.exists() && active_path.is_file() {
            let original = crate::storage::read_text_file_capped(&active_path)
                .with_context(|| format!("failed to read target {}", active_path.display()))?;
            let modified = format!(
                "{}{}",
                original,
                appendable_candidate_block(&target, &content)
            );
            Ok(unified_diff(&original, &modified, &target))
        } else {
            Ok(new_file_patch(&target, &content))
        }
    }

    fn target_signal(&self, target: &str) -> Result<String> {
        let relative = checked_relative_path(target)?;
        let path = self.project_root.join(relative);
        if path.is_dir() {
            return Ok(format!("{target}: directory"));
        }
        if !path.exists() {
            return Ok(format!("{target}: missing"));
        }
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to inspect target {}", path.display()))?;
        let line_count = crate::storage::read_text_file_capped(&path)
            .map(|content| content.lines().count())
            .unwrap_or(0);
        Ok(format!(
            "{target}: file, {} bytes, {} lines",
            metadata.len(),
            line_count
        ))
    }

    fn load_gate_report(&self, run_id: &str) -> Result<EvolutionGateReport> {
        let path = self.run_dir(run_id).join("gates.json");
        let data = crate::storage::read_text_file_capped(&path)
            .with_context(|| format!("failed to read gates {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("invalid gates {}", path.display()))
    }

    fn run_validation_commands(&self, full_validation: bool) -> Vec<EvolutionCommandCheck> {
        if !self.project_root.join("Cargo.toml").exists() {
            return Vec::new();
        }
        let mut commands = vec![vec![
            "cargo".to_string(),
            "check".to_string(),
            "--all-targets".to_string(),
            "--all-features".to_string(),
        ]];
        if full_validation {
            commands.push(vec![
                "cargo".to_string(),
                "test".to_string(),
                "--all-targets".to_string(),
                "--all-features".to_string(),
            ]);
        }
        commands
            .into_iter()
            .map(|command| self.run_validation_command(command))
            .collect()
    }

    fn run_validation_command(&self, command: Vec<String>) -> EvolutionCommandCheck {
        let started = Instant::now();
        let output = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(&self.project_root)
            .output();
        let duration_ms = started.elapsed().as_millis();
        match output {
            Ok(output) => EvolutionCommandCheck {
                command,
                status: if output.status.success() {
                    GateStatus::Pass
                } else {
                    GateStatus::Fail
                },
                exit_code: output.status.code(),
                duration_ms,
                stdout: truncate_log(&String::from_utf8_lossy(&output.stdout)),
                stderr: truncate_log(&String::from_utf8_lossy(&output.stderr)),
            },
            Err(_) => EvolutionCommandCheck {
                command,
                status: GateStatus::Fail,
                exit_code: None,
                duration_ms,
                stdout: String::new(),
                stderr: "failed to start validation command".to_string(),
            },
        }
    }

    fn create_checkpoint(
        &self,
        checkpoint_dir: &Path,
        touched_files: &[String],
    ) -> Result<CheckpointManifest> {
        let mut entries = Vec::new();
        for path in touched_files {
            let relative = checked_relative_path(path)?;
            let active_path = self.project_root.join(&relative);
            if active_path.is_dir() {
                bail!("cannot checkpoint directory target: {path}");
            }
            if active_path.exists() {
                let checkpoint_path = checkpoint_dir.join("files").join(&relative);
                if let Some(parent) = checkpoint_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create checkpoint dir {}", parent.display())
                    })?;
                }
                fs::copy(&active_path, &checkpoint_path).with_context(|| {
                    format!(
                        "failed to checkpoint {} to {}",
                        active_path.display(),
                        checkpoint_path.display()
                    )
                })?;
                entries.push(CheckpointEntry {
                    path: path.to_string(),
                    existed: true,
                    checkpoint_path: Some(checkpoint_path.display().to_string()),
                });
            } else {
                entries.push(CheckpointEntry {
                    path: path.to_string(),
                    existed: false,
                    checkpoint_path: None,
                });
            }
        }
        Ok(CheckpointManifest { entries })
    }

    fn restore_checkpoint(&self, manifest: &CheckpointManifest) -> Result<()> {
        for entry in &manifest.entries {
            let relative = checked_relative_path(&entry.path)?;
            let active_path = self.project_root.join(relative);
            if entry.existed {
                let checkpoint_path = entry
                    .checkpoint_path
                    .as_ref()
                    .map(PathBuf::from)
                    .context("checkpoint entry missing checkpoint_path")?;
                if let Some(parent) = active_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create restore dir {}", parent.display())
                    })?;
                }
                fs::copy(&checkpoint_path, &active_path).with_context(|| {
                    format!(
                        "failed to restore {} from {}",
                        active_path.display(),
                        checkpoint_path.display()
                    )
                })?;
            } else if active_path.exists() {
                if active_path.is_dir() {
                    fs::remove_dir_all(&active_path)
                        .with_context(|| format!("failed to remove {}", active_path.display()))?;
                } else {
                    fs::remove_file(&active_path)
                        .with_context(|| format!("failed to remove {}", active_path.display()))?;
                }
            }
        }
        Ok(())
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.proposals_dir())
            .with_context(|| format!("failed to create {}", self.proposals_dir().display()))?;
        fs::create_dir_all(self.runs_dir())
            .with_context(|| format!("failed to create {}", self.runs_dir().display()))?;
        fs::create_dir_all(self.applies_dir())
            .with_context(|| format!("failed to create {}", self.applies_dir().display()))?;
        fs::create_dir_all(self.rollbacks_dir())
            .with_context(|| format!("failed to create {}", self.rollbacks_dir().display()))?;
        fs::create_dir_all(self.proofs_dir())
            .with_context(|| format!("failed to create {}", self.proofs_dir().display()))?;
        fs::create_dir_all(self.memory_dir())
            .with_context(|| format!("failed to create {}", self.memory_dir().display()))?;
        fs::create_dir_all(self.root.join("benchmarks")).with_context(|| {
            format!(
                "failed to create {}",
                self.root.join("benchmarks").display()
            )
        })?;
        Ok(())
    }

    fn write_proposal(&self, proposal: &EvolutionProposal) -> Result<()> {
        write_json(
            &self.proposals_dir().join(format!("{}.json", proposal.id)),
            proposal,
        )?;
        fs::write(
            self.proposals_dir().join(format!("{}.md", proposal.id)),
            proposal_markdown(proposal),
        )
        .with_context(|| format!("failed to write proposal markdown for {}", proposal.id))?;
        Ok(())
    }

    fn proposals_dir(&self) -> PathBuf {
        self.root.join("proposals")
    }

    fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    fn applies_dir(&self) -> PathBuf {
        self.root.join("applies")
    }

    fn rollbacks_dir(&self) -> PathBuf {
        self.root.join("rollbacks")
    }

    fn proofs_dir(&self) -> PathBuf {
        self.root.join("proofs")
    }

    fn memory_dir(&self) -> PathBuf {
        self.root.join("memory")
    }

    fn proof_json_path(&self, proof_id: &str) -> PathBuf {
        self.proofs_dir().join(format!("{proof_id}.json"))
    }

    fn proof_report_path(&self, proof_id: &str) -> PathBuf {
        self.proofs_dir().join(format!("{proof_id}.md"))
    }

    fn write_proof(&self, proof: &EvolutionProof) -> Result<()> {
        let json_path = self.proof_json_path(&proof.id);
        let report_path = self.proof_report_path(&proof.id);
        write_json(&json_path, proof)?;
        fs::write(&report_path, proof_markdown(proof))
            .with_context(|| format!("failed to write proof report {}", report_path.display()))?;
        append_proof_lineage(&self.proofs_dir().join("proofs.jsonl"), proof)?;
        append_evolution_memory(&self.memory_dir().join("events.jsonl"), proof)?;
        Ok(())
    }

    fn run_dir(&self, run_id: &str) -> PathBuf {
        self.runs_dir().join(run_id)
    }

    fn apply_dir(&self, apply_id: &str) -> PathBuf {
        self.applies_dir().join(apply_id)
    }
}

pub fn is_high_risk_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    HIGH_RISK_FILES.contains(&normalized)
        || HIGH_RISK_PREFIXES
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
}

pub fn validate_proposal_id(id: &str) -> Result<()> {
    bail_if_empty_id(id, "proposal")?;
    if !id.starts_with("proposal-") {
        bail!("proposal id must start with proposal-");
    }
    Ok(())
}

pub fn validate_run_id(id: &str) -> Result<()> {
    bail_if_empty_id(id, "run")?;
    if !id.starts_with("run-") {
        bail!("run id must start with run-");
    }
    Ok(())
}

pub fn validate_apply_id(id: &str) -> Result<()> {
    bail_if_empty_id(id, "apply")?;
    if !id.starts_with("apply-") {
        bail!("apply id must start with apply-");
    }
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let data = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{data}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn build_proof(
    proof_id: String,
    task: String,
    evolution_proposal_id: &str,
    repair_proposal_id: &str,
    repair_run: &RepairRun,
    archive_candidate_id: Option<String>,
    goal_alignment: Option<GoalAlignment>,
) -> EvolutionProof {
    let sandbox_result = repair_run.sandbox_result.as_ref();
    let sandbox_passed =
        sandbox_result.is_some_and(|result| result.status == SandboxStatus::Passed);
    let archive_recorded = archive_candidate_id.is_some();
    let patch_present = repair_run.patch_hash.is_some();
    let repair_passed = repair_run.status == RepairRunStatus::Passed;
    let meaningfulness = assess_meaningfulness(
        &task,
        &repair_run.patch_paths,
        patch_present,
        goal_alignment.as_ref(),
    );
    let meaningful = meaningfulness.status != MeaningfulnessStatus::Fail;
    let proven = patch_present && sandbox_passed && archive_recorded && repair_passed && meaningful;
    let mut evidence = vec![
        format!("repair_run={}", repair_run.id),
        format!("repair_status={:?}", repair_run.status),
        format!(
            "patch_hash={}",
            repair_run.patch_hash.as_deref().unwrap_or("none")
        ),
        format!(
            "sandbox_status={}",
            sandbox_result
                .map(|result| result.status.as_str())
                .unwrap_or("not_requested")
        ),
        format!(
            "archive_candidate={}",
            archive_candidate_id.as_deref().unwrap_or("none")
        ),
    ];
    if let Some(result) = sandbox_result {
        evidence.push(format!("sandbox_run={}", result.run_id));
        if let Some(url) = &result.artifact_url {
            evidence.push(format!("sandbox_artifact={url}"));
        }
    }
    if let Some(alignment) = &goal_alignment {
        evidence.push(format!("goal_objective={}", alignment.objective_id));
        evidence.push(format!("goal_phase={}", alignment.phase_id));
        evidence.push(format!("goal_alignment={}", alignment.summary));
    }
    EvolutionProof {
        id: proof_id,
        created_at: Utc::now(),
        task,
        goal_alignment,
        meaningfulness,
        verdict: if proven {
            EvolutionProofVerdict::SelfEvolutionProven
        } else if !meaningful {
            EvolutionProofVerdict::NotMeaningfulEvolution
        } else {
            EvolutionProofVerdict::NotProven
        },
        evolution_proposal_id: evolution_proposal_id.to_string(),
        repair_proposal_id: repair_proposal_id.to_string(),
        repair_run_id: repair_run.id.clone(),
        archive_candidate_id,
        patch_hash: repair_run.patch_hash.clone(),
        sandbox_backend: sandbox_result.and_then(|result| result.backend.clone()),
        sandbox_status: sandbox_result.map(|result| result.status.as_str().to_string()),
        sandbox_run_id: sandbox_result.map(|result| result.run_id.clone()),
        sandbox_artifact_url: sandbox_result.and_then(|result| result.artifact_url.clone()),
        evidence,
        failure_reason: if proven {
            None
        } else {
            Some(proof_failure_reason(
                patch_present,
                sandbox_passed,
                archive_recorded,
                repair_passed,
                sandbox_result.map(|result| result.summary.as_str()),
                meaningful,
            ))
        },
        proof_json_path: String::new(),
        proof_report_path: String::new(),
    }
}

fn proof_failure_reason(
    patch_present: bool,
    sandbox_passed: bool,
    archive_recorded: bool,
    repair_passed: bool,
    sandbox_summary: Option<&str>,
    meaningful: bool,
) -> String {
    if !patch_present {
        "model did not generate a candidate patch".to_string()
    } else if !meaningful {
        "candidate patch did not meaningfully align with the active goal".to_string()
    } else if !sandbox_passed {
        format!(
            "remote sandbox did not pass: {}",
            sandbox_summary.unwrap_or("no sandbox result")
        )
    } else if !archive_recorded {
        "archive candidate was not recorded".to_string()
    } else if !repair_passed {
        "repair run was not marked Passed".to_string()
    } else {
        "proof criteria were not met".to_string()
    }
}

fn assess_meaningfulness(
    task: &str,
    patch_paths: &[String],
    patch_present: bool,
    goal_alignment: Option<&GoalAlignment>,
) -> EvolutionMeaningfulness {
    let Some(goal) = goal_alignment else {
        return EvolutionMeaningfulness {
            status: MeaningfulnessStatus::Pass,
            reasons: vec!["no goal alignment attached; legacy proof path".to_string()],
            matched_targets: Vec::new(),
        };
    };
    let mut reasons = vec![
        format!("objective={}", goal.objective_id),
        "proof is bound to an active goal".to_string(),
    ];
    if !patch_present {
        reasons.push("no candidate patch was generated".to_string());
        return EvolutionMeaningfulness {
            status: MeaningfulnessStatus::Fail,
            reasons,
            matched_targets: Vec::new(),
        };
    }
    if goal.success_criteria.is_empty() {
        reasons.push("active objective has no success criteria".to_string());
        return EvolutionMeaningfulness {
            status: MeaningfulnessStatus::Fail,
            reasons,
            matched_targets: Vec::new(),
        };
    }
    let matched_targets = patch_paths
        .iter()
        .filter(|path| goal.target_files.iter().any(|target| target == *path))
        .cloned()
        .collect::<Vec<_>>();
    if matched_targets.is_empty() {
        reasons.push(format!(
            "patch paths did not match active objective targets: {}",
            goal.target_files.join(", ")
        ));
        return EvolutionMeaningfulness {
            status: MeaningfulnessStatus::Fail,
            reasons,
            matched_targets,
        };
    }
    if !task.contains(&goal.objective_id) && !task.contains(&goal.objective_title) {
        reasons.push("task text does not mention the active objective directly".to_string());
        return EvolutionMeaningfulness {
            status: MeaningfulnessStatus::Warn,
            reasons,
            matched_targets,
        };
    }
    reasons.push(format!(
        "patch touched active objective target(s): {}",
        matched_targets.join(", ")
    ));
    EvolutionMeaningfulness {
        status: MeaningfulnessStatus::Pass,
        reasons,
        matched_targets,
    }
}

fn proof_markdown(proof: &EvolutionProof) -> String {
    let goal_objective = proof
        .goal_alignment
        .as_ref()
        .map(|alignment| alignment.objective_id.as_str())
        .unwrap_or("none");
    let goal_phase = proof
        .goal_alignment
        .as_ref()
        .map(|alignment| alignment.phase_id.as_str())
        .unwrap_or("none");
    let goal_summary = proof
        .goal_alignment
        .as_ref()
        .map(|alignment| alignment.summary.as_str())
        .unwrap_or("none");
    let goal_success = proof
        .goal_alignment
        .as_ref()
        .map(|alignment| {
            alignment
                .success_criteria
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "none".to_string());
    format!(
        r#"# Octocode Self-Evolution Proof

## Verdict

`{:?}`

## Task

{}

## Goal Alignment

- Objective: `{}`
- Phase: `{}`
- Summary: {}

### Success Criteria

{}

## Meaningfulness

- Status: `{:?}`
- Matched targets: `{}`

{}

## IDs

- Proof: `{}`
- Evolution proposal: `{}`
- Repair proposal: `{}`
- Repair run: `{}`
- Archive candidate: `{}`

## Patch

- Patch hash: `{}`

## Remote Sandbox

- Backend: `{}`
- Status: `{}`
- Run: `{}`
- Artifact: `{}`

## Evidence

{}

## Failure Reason

{}
"#,
        proof.verdict,
        proof.task,
        goal_objective,
        goal_phase,
        goal_summary,
        goal_success,
        proof.meaningfulness.status,
        proof.meaningfulness.matched_targets.join(", "),
        proof
            .meaningfulness
            .reasons
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n"),
        proof.id,
        proof.evolution_proposal_id,
        proof.repair_proposal_id,
        proof.repair_run_id,
        proof.archive_candidate_id.as_deref().unwrap_or("none"),
        proof.patch_hash.as_deref().unwrap_or("none"),
        proof.sandbox_backend.as_deref().unwrap_or("none"),
        proof.sandbox_status.as_deref().unwrap_or("none"),
        proof.sandbox_run_id.as_deref().unwrap_or("none"),
        proof.sandbox_artifact_url.as_deref().unwrap_or("none"),
        proof
            .evidence
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n"),
        proof.failure_reason.as_deref().unwrap_or("none")
    )
}

fn append_proof_lineage(path: &Path, proof: &EvolutionProof) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open proof lineage {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(proof)?)
        .with_context(|| format!("failed to append proof lineage {}", path.display()))?;
    Ok(())
}

fn append_evolution_memory(path: &Path, proof: &EvolutionProof) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let event = serde_json::json!({
        "kind": "evolution_proof",
        "proof_id": proof.id.clone(),
        "created_at": proof.created_at,
        "task": proof.task.clone(),
        "verdict": proof.verdict.clone(),
        "goal_objective": proof.goal_alignment.as_ref().map(|goal| goal.objective_id.clone()),
        "meaningfulness": proof.meaningfulness.clone(),
        "patch_hash": proof.patch_hash.clone(),
        "sandbox_backend": proof.sandbox_backend.clone(),
        "sandbox_status": proof.sandbox_status.clone(),
        "failure_reason": proof.failure_reason.clone(),
    });
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open evolution memory {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(&event)?)
        .with_context(|| format!("failed to append evolution memory {}", path.display()))?;
    Ok(())
}

fn concise_task(task: &str) -> String {
    let task = task.trim();
    if task.chars().count() > 64 {
        format!("{}...", task.chars().take(61).collect::<String>())
    } else if task.is_empty() {
        "prove self-evolution".to_string()
    } else {
        task.to_string()
    }
}

fn normalize_targets(targets: Vec<String>) -> Vec<String> {
    let mut targets: Vec<String> = targets
        .into_iter()
        .map(|path| path.trim().trim_start_matches("./").to_string())
        .filter(|path| !path.is_empty())
        .collect();
    targets.sort();
    targets.dedup();
    targets
}

fn checked_relative_path(path: &str) -> Result<PathBuf> {
    let relative = Path::new(path);
    if relative.is_absolute() {
        bail!("absolute paths are not allowed in evolution patches: {path}");
    }
    for component in relative.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => bail!("path escapes project root in evolution patch: {path}"),
        }
    }
    Ok(relative.to_path_buf())
}

fn default_required_gates() -> Vec<EvolutionGateKind> {
    vec![
        EvolutionGateKind::PatchPresent,
        EvolutionGateKind::CurrentTreeUnchanged,
        EvolutionGateKind::HighRiskReview,
        EvolutionGateKind::SafetyRegressionScan,
        EvolutionGateKind::TestDeletionScan,
        EvolutionGateKind::ValidationCommand,
    ]
}

fn default_recommendations() -> Vec<EvolutionRecommendation> {
    vec![
        EvolutionRecommendation {
            area: "model-routing".to_string(),
            title: "Strengthen provider-specific model capability routing".to_string(),
            target_files: vec!["src/provider/mod.rs".to_string(), "src/deepseek/client.rs".to_string()],
            rationale: "Octocode targets many model providers, so routing and capability declarations should evolve with provider behavior.".to_string(),
        },
        EvolutionRecommendation {
            area: "team-agents".to_string(),
            title: "Improve multi-agent task decomposition and status surfaces".to_string(),
            target_files: vec!["src/agent".to_string(), "src/tui".to_string()],
            rationale: "The product direction depends on multi-capability and multi-agent execution being visible and controllable.".to_string(),
        },
        EvolutionRecommendation {
            area: "core-safety".to_string(),
            title: "Harden approval, command, and policy boundaries".to_string(),
            target_files: vec!["src/policy".to_string(), "src/tools".to_string()],
            rationale: "Self-evolution must make risky source changes auditable before the active CLI can apply them.".to_string(),
        },
    ]
}

fn proposal_target_files(proposal: &EvolutionProposal) -> Vec<String> {
    if proposal.target_files.is_empty() {
        vec![format!("docs/evolution/{}.md", proposal.id)]
    } else {
        proposal.target_files.clone()
    }
}

fn candidate_patch_content(proposal: &EvolutionProposal, candidate: &EvolutionCandidate) -> String {
    let agent_lines = candidate
        .agents
        .iter()
        .map(|agent| {
            format!(
                "- {:?}: {} | veto={} | {}",
                agent.role, agent.title, agent.veto, agent.recommendation
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# Octocode Core Evolution Candidate\n\n- evolution proposal: {id}\n- run: {run_id}\n- area: {area}\n- title: {title}\n- problem: {problem}\n- manual_review_required: {manual}\n- generated_by: local_multi_agent_pipeline\n\n## Agent Drafts\n\n{agent_lines}\n",
        id = proposal.id,
        run_id = candidate.run_id,
        area = proposal.target_area,
        title = proposal.title,
        problem = proposal.problem.replace('\n', " "),
        manual = proposal.manual_review_required
    )
}

fn appendable_candidate_block(target: &str, content: &str) -> String {
    if target.ends_with(".rs") {
        format!("\n/*\n{}\n*/\n", content)
    } else {
        format!("\n{}\n", content)
    }
}

fn new_file_patch(target: &str, content: &str) -> String {
    let added = content
        .lines()
        .map(|line| format!("+{line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "diff --git a/{target} b/{target}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{target}\n@@ -0,0 +1,{} @@\n{}\n",
        content.lines().count(),
        added
    )
}

fn model_patch_from_candidate(
    candidate: &EvolutionCandidate,
    allowed_targets: &[String],
) -> Option<String> {
    candidate
        .agents
        .iter()
        .find(|agent| {
            agent.role == EvolutionAgentRole::Implementer
                && agent.source == EvolutionAgentSource::Model
        })
        .and_then(|agent| agent.patch.as_deref())
        .and_then(|patch| validate_model_patch(patch, allowed_targets).ok())
}

fn validate_model_patch(patch: &str, allowed_targets: &[String]) -> Result<String> {
    let patch = patch.trim();
    if patch.is_empty() {
        bail!("empty model patch");
    }
    if !patch.contains("--- ") || !patch.contains("+++ ") {
        bail!("model patch is not a unified diff");
    }
    let changed = changed_files_from_patch(patch);
    if changed.is_empty() {
        bail!("model patch has no changed files");
    }
    for path in &changed {
        checked_relative_path(path)?;
        if !allowed_targets.contains(path) {
            bail!(
                "model patch touched {path}, outside proposal targets: {}",
                allowed_targets.join(", ")
            );
        }
    }
    Ok(format!("{patch}\n"))
}

fn merge_model_agents(
    local_agents: Vec<EvolutionAgentDraft>,
    model_agents: Vec<EvolutionAgentDraft>,
) -> Vec<EvolutionAgentDraft> {
    let mut merged = Vec::new();
    for local in local_agents {
        if let Some(model) = model_agents
            .iter()
            .find(|agent| agent.role == local.role)
            .cloned()
        {
            merged.push(model);
        } else {
            merged.push(local);
        }
    }
    merged
}

#[derive(Debug, Deserialize)]
struct ModelAgentJson {
    title: Option<String>,
    findings: Option<Vec<String>>,
    recommendation: Option<String>,
    patch: Option<String>,
    veto: Option<bool>,
}

fn parse_model_agent_response(
    role: EvolutionAgentRole,
    content: &str,
    proposal: &EvolutionProposal,
) -> EvolutionAgentDraft {
    match serde_json::from_str::<ModelAgentJson>(content) {
        Ok(payload) => EvolutionAgentDraft {
            role,
            source: EvolutionAgentSource::Model,
            title: payload
                .title
                .unwrap_or_else(|| "Model agent draft".to_string()),
            findings: payload
                .findings
                .filter(|items| !items.is_empty())
                .unwrap_or_else(|| vec!["model returned no findings".to_string()]),
            target_files: proposal_target_files(proposal),
            recommendation: payload
                .recommendation
                .unwrap_or_else(|| "model returned no recommendation".to_string()),
            patch: payload.patch.filter(|patch| !patch.trim().is_empty()),
            veto: payload.veto.unwrap_or(false),
        },
        Err(_) => EvolutionAgentDraft {
            role,
            source: EvolutionAgentSource::Model,
            title: "Unstructured model response".to_string(),
            findings: vec!["model returned non-JSON content".to_string()],
            target_files: proposal_target_files(proposal),
            recommendation: truncate_for_prompt(content, 2000),
            patch: None,
            veto: false,
        },
    }
}

fn model_agent_system_prompt(role: &EvolutionAgentRole) -> String {
    let role_text = match role {
        EvolutionAgentRole::Planner => {
            "You are the planner for Octocode self-evolution. Identify a minimal safe change plan."
        }
        EvolutionAgentRole::Implementer => {
            "You are the implementer for Octocode self-evolution. Produce a narrow unified diff when safe."
        }
        EvolutionAgentRole::SafetyReviewer => {
            "You are the safety reviewer for Octocode self-evolution. Look for policy, approval, command, token, and test-regression risks."
        }
    };
    format!(
        "{role_text}\nReturn only a JSON object with keys: title, findings, recommendation, patch, veto. findings must be an array of strings. patch must be a unified diff string or null. Do not include markdown outside JSON."
    )
}

fn model_agent_user_prompt(
    proposal: &EvolutionProposal,
    run_id: &str,
    role: &EvolutionAgentRole,
    target_context: &str,
) -> String {
    let target_files = proposal_target_files(proposal).join(", ");
    let implementer_rule = if *role == EvolutionAgentRole::Implementer {
        "If you provide patch, it must only touch the target files and must be valid unified diff. Prefer the smallest useful code or doc change. If unsafe, set patch to null."
    } else {
        "Do not invent code changes outside the target files. Set patch to null unless your role must provide a patch."
    };
    format!(
        "Proposal id: {id}\nRun id: {run_id}\nArea: {area}\nTitle: {title}\nProblem: {problem}\nManual review required: {manual}\nTarget files: {target_files}\n\nRole: {role:?}\nRule: {implementer_rule}\n\nTarget context:\n{target_context}",
        id = proposal.id,
        area = proposal.target_area,
        title = proposal.title,
        problem = proposal.problem,
        manual = proposal.manual_review_required,
    )
}

fn truncate_for_prompt(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[truncated]", &text[..end])
}

fn candidate_markdown(candidate: &EvolutionCandidate) -> String {
    let agents = candidate
        .agents
        .iter()
        .map(|agent| {
            let findings = agent
                .findings
                .iter()
                .map(|finding| format!("  - {finding}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "## {:?}\n\n- title: {}\n- veto: {}\n- targets: {}\n- recommendation: {}\n\n### Findings\n\n{}\n",
                agent.role,
                agent.title,
                agent.veto,
                if agent.target_files.is_empty() {
                    "-".to_string()
                } else {
                    agent.target_files.join(", ")
                },
                agent.recommendation,
                findings
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "# Evolution Candidate\n\n- proposal: {}\n- run: {}\n- summary: {}\n\n{}\n",
        candidate.proposal_id, candidate.run_id, candidate.summary, agents
    )
}

fn changed_files_from_patch(patch: &str) -> Vec<String> {
    let mut files = parse_patch_paths(patch);
    files.retain(|path| path != "/dev/null");
    files.sort();
    files.dedup();
    files
}

fn base_gates(
    patch: &str,
    touched_files: &[String],
    high_risk_files: &[String],
) -> Vec<EvolutionGateResult> {
    vec![
        EvolutionGateResult {
            name: EvolutionGateKind::PatchPresent,
            status: if patch.trim().is_empty() {
                GateStatus::Fail
            } else {
                GateStatus::Pass
            },
            message: if patch.trim().is_empty() {
                "patch.diff is empty".to_string()
            } else {
                "patch.diff exists and is non-empty".to_string()
            },
        },
        EvolutionGateResult {
            name: EvolutionGateKind::CurrentTreeUnchanged,
            status: GateStatus::Pass,
            message: "patch generation uses an isolated run directory and does not apply changes to the active source tree".to_string(),
        },
        EvolutionGateResult {
            name: EvolutionGateKind::HighRiskReview,
            status: if high_risk_files.is_empty() {
                GateStatus::Pass
            } else {
                GateStatus::Warn
            },
            message: if high_risk_files.is_empty() {
                "no high-risk core files touched".to_string()
            } else {
                format!("manual review required for {}", high_risk_files.join(", "))
            },
        },
        EvolutionGateResult {
            name: EvolutionGateKind::SafetyRegressionScan,
            status: safety_regression_status(patch),
            message: safety_regression_message(patch),
        },
        EvolutionGateResult {
            name: EvolutionGateKind::TestDeletionScan,
            status: test_deletion_status(patch, touched_files),
            message: test_deletion_message(patch, touched_files),
        },
    ]
}

fn safety_regression_status(patch: &str) -> GateStatus {
    let lowered = patch.to_ascii_lowercase();
    if lowered.contains("bypass approval")
        || lowered.contains("disable approval")
        || lowered.contains("allow_all")
        || lowered.contains("dangerously_allow")
    {
        GateStatus::Fail
    } else {
        GateStatus::Pass
    }
}

fn safety_regression_message(patch: &str) -> String {
    if safety_regression_status(patch) == GateStatus::Fail {
        "patch appears to loosen approval or safety boundaries".to_string()
    } else {
        "no obvious approval or safety loosening detected".to_string()
    }
}

fn test_deletion_status(patch: &str, touched_files: &[String]) -> GateStatus {
    let deletes_test_file = touched_files
        .iter()
        .any(|path| path.starts_with("tests/") && patch.contains("deleted file mode"));
    let removes_many_test_lines = patch
        .lines()
        .filter(|line| line.starts_with('-') && !line.starts_with("---"))
        .filter(|line| line.contains("#[test]") || line.contains("assert"))
        .count()
        > 3;
    if deletes_test_file || removes_many_test_lines {
        GateStatus::Fail
    } else {
        GateStatus::Pass
    }
}

fn test_deletion_message(patch: &str, touched_files: &[String]) -> String {
    if test_deletion_status(patch, touched_files) == GateStatus::Fail {
        "patch deletes tests or removes multiple test assertions".to_string()
    } else {
        "no test deletion pattern detected".to_string()
    }
}

fn proposal_markdown(proposal: &EvolutionProposal) -> String {
    let targets = if proposal.target_files.is_empty() {
        format!("- docs/evolution/{}.md", proposal.id)
    } else {
        proposal
            .target_files
            .iter()
            .map(|target| format!("- {target}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "# {title}\n\n- id: {id}\n- area: {area}\n- risk: {risk:?}\n- manual_review_required: {manual}\n\n## Problem\n\n{problem}\n\n## Targets\n\n{targets}\n",
        title = proposal.title,
        id = proposal.id,
        area = proposal.target_area,
        risk = proposal.risk_level,
        manual = proposal.manual_review_required,
        problem = proposal.problem,
    )
}

fn isolated_worktree_readme(proposal: &EvolutionProposal) -> String {
    format!(
        "# Isolated Octocode evolution worktree\n\nProposal: {}\n\nThis directory is reserved for generated artifacts. The active source tree is not modified by `octocode evolve patch`.\n",
        proposal.id
    )
}

fn gate_stdout(report: &EvolutionGateReport) -> String {
    let commands = report
        .commands
        .iter()
        .map(|command| {
            format!(
                "command={} status={:?} exit_code={:?} duration_ms={}\n{}",
                command.command.join(" "),
                command.status,
                command.exit_code,
                command.duration_ms,
                command.stdout
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "run_id={}\nstatus={:?}\nmanual_review_required={}\n{}\n",
        report.run_id, report.status, report.manual_review_required, commands
    )
}

fn gate_stderr(report: &EvolutionGateReport) -> String {
    let command_stderr = report
        .commands
        .iter()
        .filter(|command| !command.stderr.trim().is_empty())
        .map(|command| {
            format!(
                "command={} stderr:\n{}",
                command.command.join(" "),
                command.stderr
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if report.status == EvolutionRunStatus::Failed {
        format!("one or more evolution gates failed\n{command_stderr}\n")
    } else if report.manual_review_required {
        format!("manual review required before applying this patch\n{command_stderr}\n")
    } else {
        command_stderr
    }
}

fn gate_report_markdown(report: &EvolutionGateReport) -> String {
    let gates = report
        .gates
        .iter()
        .map(|gate| format!("- {:?}: {:?} - {}", gate.name, gate.status, gate.message))
        .collect::<Vec<_>>()
        .join("\n");
    let commands = if report.commands.is_empty() {
        "- no validation commands were run".to_string()
    } else {
        report
            .commands
            .iter()
            .map(|command| {
                format!(
                    "- `{}`: {:?} ({:?}, {}ms)",
                    command.command.join(" "),
                    command.status,
                    command.exit_code,
                    command.duration_ms
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "# Evolution Gate Report\n\n- run_id: {run_id}\n- status: {status:?}\n- manual_review_required: {manual}\n\n## Gates\n\n{gates}\n\n## Validation Commands\n\n{commands}\n",
        run_id = report.run_id,
        status = report.status,
        manual = report.manual_review_required,
    )
}

fn bail_if_empty_id(id: &str, kind: &str) -> Result<()> {
    if id.trim().is_empty() {
        bail!("{kind} id cannot be empty");
    }
    Ok(())
}

fn truncate_log(text: &str) -> String {
    const LIMIT: usize = 16_000;
    if text.len() <= LIMIT {
        return text.to_string();
    }
    let mut end = LIMIT;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\\n[truncated]", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn flags_high_risk_paths() {
        assert!(is_high_risk_path("src/policy/mod.rs"));
        assert!(is_high_risk_path("src/storage/keyring.rs"));
        assert!(!is_high_risk_path("src/provider/mod.rs"));
    }

    #[test]
    fn patch_gate_marks_high_risk_manual_review() {
        let temp = tempdir().expect("temp dir");
        let store = EvolutionStore::for_project(temp.path());
        let proposal = store
            .create_proposal(NewEvolutionProposal {
                title: None,
                problem: None,
                target_area: "core-safety".to_string(),
                target_files: vec!["src/policy/mod.rs".to_string()],
            })
            .expect("proposal");
        let run = store.create_patch_run(&proposal.id).expect("run");
        let report = store.run_gates(&run.id, false).expect("gates");
        assert_eq!(report.status, EvolutionRunStatus::NeedsManualReview);
        assert!(report.manual_review_required);
        assert_eq!(report.high_risk_files, vec!["src/policy/mod.rs"]);
    }

    #[test]
    fn apply_and_rollback_restore_checkpoint() {
        let temp = tempdir().expect("temp dir");
        let store = EvolutionStore::for_project(temp.path());
        let proposal = store
            .create_proposal(NewEvolutionProposal {
                title: None,
                problem: None,
                target_area: "core-evolution".to_string(),
                target_files: Vec::new(),
            })
            .expect("proposal");
        let run = store.create_patch_run(&proposal.id).expect("run");
        store.run_gates(&run.id, false).expect("gates");
        let apply = store.apply_run(&run.id, false).expect("apply");
        let touched = apply.touched_files.first().expect("touched file");
        assert!(temp.path().join(touched).exists());
        let rollback = store.rollback_apply(&apply.id).expect("rollback");
        assert_eq!(rollback.status, EvolutionRollbackStatus::RolledBack);
        assert!(!temp.path().join(touched).exists());
    }

    #[test]
    fn proof_requires_patch_sandbox_archive_and_passed_repair() {
        let repair_run = RepairRun {
            id: "repair-run-test".to_string(),
            proposal_id: "repair-proposal-test".to_string(),
            created_at: Utc::now(),
            status: RepairRunStatus::Passed,
            model_patch_requested: true,
            model_errors: Vec::new(),
            sandbox_result: Some(crate::remote_sandbox::SandboxValidationResult {
                run_id: "sandbox-test".to_string(),
                candidate_id: Some("repair-run-test".to_string()),
                status: SandboxStatus::Passed,
                summary: "all validation commands passed".to_string(),
                command_results: Vec::new(),
                risk_flags: Vec::new(),
                started_at_unix_ms: 1,
                finished_at_unix_ms: 2,
                backend: Some("mock".to_string()),
                artifact_url: Some("https://example.invalid/artifact".to_string()),
            }),
            touched_files: vec!["src/cli/archive.rs".to_string()],
            patch_paths: vec!["src/cli/archive.rs".to_string()],
            patch_hash: Some("sha256:test".to_string()),
            risk_findings: Vec::new(),
            validations: Vec::new(),
            diff_judge: crate::repair::DiffJudgeResult {
                verdict: crate::repair::RepairGateStatus::Pass,
                reasons: Vec::new(),
                risk_flags: Vec::new(),
                missing_tests: Vec::new(),
                suggested_next_validation: Vec::new(),
            },
            report_path: ".octocode/repair/runs/repair-run-test/report.md".to_string(),
        };
        let proof = build_proof(
            "proof-test".to_string(),
            "prove self-evolution".to_string(),
            "proposal-test",
            "repair-proposal-test",
            &repair_run,
            Some("candidate-test".to_string()),
            None,
        );
        assert_eq!(proof.verdict, EvolutionProofVerdict::SelfEvolutionProven);
        assert_eq!(proof.sandbox_status.as_deref(), Some("passed"));

        let missing_archive = build_proof(
            "proof-test-2".to_string(),
            "prove self-evolution".to_string(),
            "proposal-test",
            "repair-proposal-test",
            &repair_run,
            None,
            None,
        );
        assert_eq!(missing_archive.verdict, EvolutionProofVerdict::NotProven);
    }
}
