use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use uuid::Uuid;

use crate::archive::{ArchiveStore, NewArchiveCandidate};
use crate::deepseek::{
    ChatMessage, ChatMessageContent, ChatRequest, ResponseFormat, ThinkingConfig,
};
use crate::knowledge::{
    max_risk_level, risk_rank, FailureMemoryEntry, KnowledgeRiskLevel, KnowledgeStore,
    NewFailureMemoryEntry, RiskFinding,
};
use crate::provider::{build_provider, parse_model, Provider};
use crate::remote_sandbox::{self, SandboxStatus, SandboxValidationResult};
use crate::skill::{SkillStore, SkillSummary};
use crate::storage;
use crate::workspace::apply::parse_patch_paths;
use sha2::{Digest, Sha256};

pub const REPAIR_DIR: &str = ".octocode/repair";

#[derive(Debug, Clone)]
pub struct RepairStore {
    project_root: PathBuf,
    root: PathBuf,
    knowledge: KnowledgeStore,
    archive: ArchiveStore,
    skills: SkillStore,
}

#[derive(Debug, Clone)]
pub struct NewRepairProposal {
    pub title: Option<String>,
    pub problem: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepairProposal {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub title: String,
    pub problem_statement: String,
    pub scope_hints: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub blocked_paths: Vec<String>,
    pub risk_level: KnowledgeRiskLevel,
    pub validation_plan: Vec<String>,
    pub context_bundle: RepairContextBundle,
    pub status: RepairProposalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepairContextBundle {
    pub risk_findings: Vec<RiskFinding>,
    pub similar_failures: Vec<FailureMemoryEntry>,
    pub candidate_skills: Vec<SkillSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepairRun {
    pub id: String,
    pub proposal_id: String,
    pub created_at: DateTime<Utc>,
    pub status: RepairRunStatus,
    pub model_patch_requested: bool,
    pub model_errors: Vec<String>,
    #[serde(default)]
    pub sandbox_result: Option<SandboxValidationResult>,
    pub touched_files: Vec<String>,
    pub patch_paths: Vec<String>,
    pub patch_hash: Option<String>,
    pub risk_findings: Vec<RiskFinding>,
    pub validations: Vec<RepairValidationResult>,
    pub diff_judge: DiffJudgeResult,
    pub report_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepairStatus {
    pub proposals: Vec<RepairProposal>,
    pub runs: Vec<RepairRun>,
    pub failure_memory: Vec<FailureMemoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairProposalStatus {
    Proposed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairRunStatus {
    Reported,
    Passed,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepairValidationResult {
    pub name: String,
    pub status: RepairGateStatus,
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub stdout_excerpt: String,
    pub stderr_excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffJudgeResult {
    pub verdict: RepairGateStatus,
    pub reasons: Vec<String>,
    pub risk_flags: Vec<String>,
    pub missing_tests: Vec<String>,
    pub suggested_next_validation: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairGateStatus {
    Pass,
    Warn,
    Fail,
}

impl RepairStore {
    pub fn for_project(project_root: impl AsRef<Path>) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let root = project_root.join(REPAIR_DIR);
        let knowledge = KnowledgeStore::for_project(&project_root);
        let archive = ArchiveStore::for_project(&project_root);
        let skills = SkillStore::for_project(&project_root);
        Self {
            project_root,
            root,
            knowledge,
            archive,
            skills,
        }
    }

    pub fn create_proposal(&self, input: NewRepairProposal) -> Result<RepairProposal> {
        self.ensure_dirs()?;
        self.knowledge.load_or_create_risk_map()?;
        let id = format!("repair-proposal-{}", Uuid::new_v4());
        let targets = normalize_paths(input.targets)?;
        let risk_findings = self.knowledge.classify_paths(&targets)?;
        let risk_level = max_risk_level(&risk_findings);
        let similar_failures = self
            .knowledge
            .find_similar_failures(&input.problem, &targets, 3)?;
        let candidate_skills = self.skills.find_relevant(&input.problem, &targets, 3)?;
        let context_bundle = RepairContextBundle {
            risk_findings: risk_findings.clone(),
            similar_failures,
            candidate_skills,
        };
        let proposal = RepairProposal {
            id: id.clone(),
            created_at: Utc::now(),
            title: input
                .title
                .unwrap_or_else(|| title_from_problem(&input.problem)),
            problem_statement: input.problem,
            scope_hints: targets.clone(),
            allowed_paths: targets,
            blocked_paths: risk_findings
                .iter()
                .filter(|finding| finding.risk_level == KnowledgeRiskLevel::Blocked)
                .map(|finding| finding.path.clone())
                .collect(),
            risk_level,
            validation_plan: validation_plan_for_risk(risk_level),
            context_bundle,
            status: RepairProposalStatus::Proposed,
        };
        fs::write(
            self.proposal_json_path(&id)?,
            serde_json::to_string_pretty(&proposal)?,
        )
        .with_context(|| format!("write proposal {}", proposal.id))?;
        fs::write(self.proposal_md_path(&id)?, render_proposal_md(&proposal))
            .with_context(|| format!("write proposal markdown {}", proposal.id))?;
        fs::write(
            self.proposal_context_path(&id)?,
            serde_json::to_string_pretty(&proposal.context_bundle)?,
        )
        .with_context(|| format!("write proposal context {}", proposal.id))?;
        Ok(proposal)
    }

    pub async fn run_proposal(
        &self,
        proposal_id: &str,
        full: bool,
        model_patch: bool,
        model: Option<&str>,
    ) -> Result<RepairRun> {
        self.ensure_dirs()?;
        let proposal = self.load_proposal(proposal_id)?;
        let run_id = format!("repair-run-{}", Uuid::new_v4());
        let run_dir = self.run_dir(&run_id)?;
        fs::create_dir_all(&run_dir).with_context(|| format!("create {}", run_dir.display()))?;
        let risk_findings = self.knowledge.classify_paths(&proposal.allowed_paths)?;
        let mut model_errors = Vec::new();
        let patch = if model_patch {
            match self.generate_model_patch(&proposal, model).await {
                Ok(patch) => patch,
                Err(err) => {
                    model_errors.push(err.to_string());
                    String::new()
                }
            }
        } else {
            String::new()
        };
        let patch_paths = changed_files_from_patch(&patch);
        let patch_hash = (!patch.trim().is_empty()).then(|| patch_hash(&patch));
        let sandbox_result = if patch_hash.is_some() {
            Some(
                remote_sandbox::validate_candidate(&self.project_root, &run_id, &patch, full)
                    .await
                    .unwrap_or_else(|err| {
                        SandboxValidationResult::blocked(
                            run_id.clone(),
                            Some(proposal.id.clone()),
                            format!("sandbox client error: {err}"),
                            Some("client".to_string()),
                        )
                    }),
            )
        } else {
            None
        };
        let diff_judge = diff_judge_for(
            &proposal,
            &risk_findings,
            &patch,
            &patch_paths,
            &model_errors,
        );
        let validations = if model_patch {
            sandbox_validation_summary(sandbox_result.as_ref())
        } else {
            self.run_validations(full)?
        };
        let failed_validation = validations
            .iter()
            .find(|validation| validation.status == RepairGateStatus::Fail)
            .cloned();
        let failed_validation_name = failed_validation
            .as_ref()
            .map(|validation| validation.name.clone());
        let failed_validation_excerpt = failed_validation
            .as_ref()
            .map(|validation| validation.stderr_excerpt.clone())
            .filter(|excerpt| !excerpt.is_empty());
        let blocked = risk_findings
            .iter()
            .any(|finding| finding.risk_level == KnowledgeRiskLevel::Blocked);
        let model_patch_without_patch = model_patch && patch_hash.is_none();
        let sandbox_passed = sandbox_result
            .as_ref()
            .is_some_and(|result| result.status == SandboxStatus::Passed);
        let sandbox_not_passed = patch_hash.is_some() && !sandbox_passed;
        let status = if blocked {
            RepairRunStatus::Blocked
        } else if model_patch_without_patch || failed_validation.is_some() || sandbox_not_passed {
            RepairRunStatus::Failed
        } else if patch_hash.is_some()
            && sandbox_passed
            && diff_judge.verdict == RepairGateStatus::Pass
        {
            RepairRunStatus::Passed
        } else {
            RepairRunStatus::Reported
        };
        let report_path = run_dir.join("report.md");
        let run = RepairRun {
            id: run_id.clone(),
            proposal_id: proposal.id.clone(),
            created_at: Utc::now(),
            status,
            model_patch_requested: model_patch,
            model_errors,
            sandbox_result,
            touched_files: proposal.allowed_paths.clone(),
            patch_paths,
            patch_hash,
            risk_findings,
            validations,
            diff_judge,
            report_path: report_path.display().to_string(),
        };
        fs::write(
            run_dir.join("run.json"),
            serde_json::to_string_pretty(&run)?,
        )
        .with_context(|| format!("write {}", run_dir.join("run.json").display()))?;
        fs::write(
            run_dir.join("diff-judge.json"),
            serde_json::to_string_pretty(&run.diff_judge)?,
        )
        .with_context(|| format!("write {}", run_dir.join("diff-judge.json").display()))?;
        fs::write(run_dir.join("patch.diff"), &patch)
            .with_context(|| format!("write {}", run_dir.join("patch.diff").display()))?;
        if let Some(sandbox_result) = &run.sandbox_result {
            fs::write(
                run_dir.join("sandbox-result.json"),
                serde_json::to_string_pretty(sandbox_result)?,
            )
            .with_context(|| format!("write {}", run_dir.join("sandbox-result.json").display()))?;
        }
        fs::write(&report_path, render_run_report(&proposal, &run))
            .with_context(|| format!("write {}", report_path.display()))?;
        for skill in &proposal.context_bundle.candidate_skills {
            let success = matches!(run.status, RepairRunStatus::Passed)
                && run.patch_hash.is_some()
                && run
                    .sandbox_result
                    .as_ref()
                    .is_some_and(|result| result.status == SandboxStatus::Passed);
            if let Err(err) = self.skills.record_use(&skill.id, &run.id, success) {
                tracing::warn!("failed to record skill use for {}: {err}", skill.id);
            }
        }
        self.archive.record_candidate(NewArchiveCandidate {
            source: "repair".to_string(),
            parent_id: None,
            proposal_id: proposal.id.clone(),
            run_id: run.id.clone(),
            patch_hash: run.patch_hash.clone(),
            patch_hash_sha256: run.patch_hash.clone(),
            sandbox_status: run
                .sandbox_result
                .as_ref()
                .map(|result| result.status.as_str().to_string()),
            sandbox_run_id: run
                .sandbox_result
                .as_ref()
                .map(|result| result.run_id.clone()),
            sandbox_backend: run
                .sandbox_result
                .as_ref()
                .and_then(|result| result.backend.clone()),
            sandbox_artifact_url: run
                .sandbox_result
                .as_ref()
                .and_then(|result| result.artifact_url.clone()),
            touched_files: run.touched_files.clone(),
            risk_level: format!("{:?}", proposal.risk_level),
            status: format!("{:?}", run.status),
            gate_summary: run
                .validations
                .iter()
                .map(|validation| format!("{}:{:?}", validation.name, validation.status))
                .collect(),
            judge_summary: vec![format!("diff:{:?}", run.diff_judge.verdict)],
            cost_summary: run
                .validations
                .iter()
                .map(|validation| format!("{}:{}ms", validation.name, validation.duration_ms))
                .collect(),
            failure_reason: failure_reason_for(&run, failed_validation_name.as_deref()),
        })?;

        if run.status == RepairRunStatus::Failed || run.status == RepairRunStatus::Blocked {
            self.knowledge.append_failure(NewFailureMemoryEntry {
                source: "repair".to_string(),
                proposal_id: Some(proposal.id.clone()),
                run_id: Some(run.id.clone()),
                symptom: proposal.problem_statement.clone(),
                root_cause: failure_reason_for(&run, failed_validation_name.as_deref()),
                touched_files: run.touched_files.clone(),
                failed_gate: failed_validation_name,
                validation_excerpt: failed_validation_excerpt,
                failed_patch_hash: None,
                avoid_pattern: Some("do not ignore deterministic repair gates".to_string()),
                lesson: Some("repair run failed before any patch was applied".to_string()),
                tags: vec![
                    "repair".to_string(),
                    format!("risk:{:?}", proposal.risk_level),
                ],
            })?;
        }

        Ok(run)
    }

    async fn generate_model_patch(
        &self,
        proposal: &RepairProposal,
        model_override: Option<&str>,
    ) -> Result<String> {
        if proposal.allowed_paths.is_empty() {
            bail!("--model-patch requires at least one --target path");
        }
        let config = storage::Config::load(Some(&self.project_root))
            .context("failed to load provider config")?;
        let Some(api_key) = storage::get_effective_api_key(Some(&self.project_root)) else {
            bail!("no API key configured for model patch generation");
        };
        let model = match model_override {
            Some(value) => parse_model(value)
                .with_context(|| format!("invalid model override '{value}'"))?
                .canonical(),
            None => config.model.default.canonical(),
        };
        let provider = build_provider(&config.provider, api_key);
        let client = provider.create_deepseek_client();
        let prompt = self.model_patch_user_prompt(proposal);
        let mut last_error = None;
        for attempt in 0..3 {
            let user_prompt = if let Some(error) = &last_error {
                format!(
                    "{prompt}\n\nPrevious candidate was rejected before sandbox validation: {error}. Return a complete, directly applicable unified diff. Use only exact context lines copied from target_context. Do not rely on line numbers. Do not use placeholders or pseudo-code."
                )
            } else {
                prompt.clone()
            };
            let request = ChatRequest {
                model: model.to_string(),
                messages: vec![
                    ChatMessage {
                        role: "system".to_string(),
                        content: Some(ChatMessageContent::Text(model_patch_system_prompt())),
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: None,
                        name: None,
                    },
                    ChatMessage {
                        role: "user".to_string(),
                        content: Some(ChatMessageContent::Text(user_prompt)),
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
                max_tokens: Some(4000),
            };
            let response = client.chat_with_retry(&request).await?;
            let content = response
                .choices
                .first()
                .and_then(|choice| choice.message.content.as_ref())
                .map(ChatMessageContent::to_string_lossy)
                .unwrap_or_default();
            match parse_model_patch_response(&content).and_then(|patch| {
                let patch_paths = changed_files_from_patch(&patch);
                validate_patch_scope(&proposal.allowed_paths, &patch_paths)?;
                validate_patch_applies(&self.project_root, &patch)?;
                Ok(patch)
            }) {
                Ok(patch) => return Ok(patch),
                Err(err) if attempt == 0 => last_error = Some(err.to_string()),
                Err(err) => return Err(err),
            }
        }
        bail!(
            "model patch retry failed: {}",
            last_error.unwrap_or_else(|| "unknown model patch error".to_string())
        )
    }

    fn model_patch_user_prompt(&self, proposal: &RepairProposal) -> String {
        let targets = proposal
            .allowed_paths
            .iter()
            .map(|path| self.target_excerpt(path))
            .collect::<Vec<_>>()
            .join("\n\n");
        let similar_failures = proposal
            .context_bundle
            .similar_failures
            .iter()
            .map(|failure| format!("- {}: {}", failure.id, failure.symptom))
            .collect::<Vec<_>>()
            .join("\n");
        let candidate_skills = proposal
            .context_bundle
            .candidate_skills
            .iter()
            .map(|skill| format!("- {} tags={}", skill.id, skill.tags.join(",")))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            r#"Repair proposal:
id: {id}
title: {title}
problem: {problem}
allowed_paths:
{allowed_paths}

similar_failures:
{similar_failures}

candidate_skills:
{candidate_skills}

target_context:
{targets}

Return JSON only with:
{{"summary":"short summary","patch":"unified diff"}}

Patch requirements:
- The diff must apply directly to the exact target_context above using exact copied context lines.
- Do not add placeholder comments, ellipses, or "rest of file unchanged" text.
- Do not invent struct fields, imports, or helper APIs unless the diff also includes the complete valid surrounding code change.
- Prefer a narrow change to existing functions over adding large new blocks.
"#,
            id = proposal.id,
            title = proposal.title,
            problem = proposal.problem_statement,
            allowed_paths = proposal
                .allowed_paths
                .iter()
                .map(|path| format!("- {path}"))
                .collect::<Vec<_>>()
                .join("\n"),
            similar_failures = if similar_failures.is_empty() {
                "none".to_string()
            } else {
                similar_failures
            },
            candidate_skills = if candidate_skills.is_empty() {
                "none".to_string()
            } else {
                candidate_skills
            },
            targets = targets
        )
    }

    fn target_excerpt(&self, target: &str) -> String {
        let relative = match normalize_path(target) {
            Ok(relative) => relative,
            Err(err) => return format!("## {target}\ninvalid target path: {err}"),
        };
        let path = self.project_root.join(&relative);
        match fs::read_to_string(&path) {
            Ok(content) => format!(
                "## {relative}\n```text\n{}\n```",
                excerpt_for_target(&relative, &content)
            ),
            Err(_) => format!("## {relative}\nmissing or unreadable target"),
        }
    }

    pub fn load_report(&self, run_id: &str) -> Result<String> {
        let path = self.run_dir(run_id)?.join("report.md");
        fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
    }

    pub fn status(&self) -> Result<RepairStatus> {
        self.ensure_dirs()?;
        Ok(RepairStatus {
            proposals: self.list_proposals()?,
            runs: self.list_runs()?,
            failure_memory: self.knowledge.list_failures()?,
        })
    }

    pub fn list_proposals(&self) -> Result<Vec<RepairProposal>> {
        self.ensure_dirs()?;
        read_json_dir(self.proposals_dir())
    }

    pub fn list_runs(&self) -> Result<Vec<RepairRun>> {
        self.ensure_dirs()?;
        let runs_dir = self.runs_dir();
        if !runs_dir.exists() {
            return Ok(Vec::new());
        }
        let mut runs: Vec<RepairRun> = Vec::new();
        for entry in
            fs::read_dir(&runs_dir).with_context(|| format!("read {}", runs_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path().join("run.json");
            if path.exists() {
                let data = fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                runs.push(serde_json::from_str(&data)?);
            }
        }
        runs.sort_by_key(|run| run.created_at);
        Ok(runs)
    }

    fn load_proposal(&self, proposal_id: &str) -> Result<RepairProposal> {
        let data = fs::read_to_string(self.proposal_json_path(proposal_id)?)
            .with_context(|| format!("read proposal {proposal_id}"))?;
        Ok(serde_json::from_str(&data)?)
    }

    fn run_validations(&self, full: bool) -> Result<Vec<RepairValidationResult>> {
        if !self.project_root.join("Cargo.toml").exists() {
            return Ok(vec![RepairValidationResult {
                name: "cargo_check".to_string(),
                status: RepairGateStatus::Warn,
                command: vec!["cargo".to_string(), "check".to_string()],
                exit_code: None,
                duration_ms: 0,
                stdout_excerpt: String::new(),
                stderr_excerpt: "Cargo.toml not found; skipped Rust validation".to_string(),
            }]);
        }
        let mut validations = vec![run_command(
            &self.project_root,
            "cargo_check",
            &["cargo", "check", "--all-targets", "--all-features"],
        )?];
        if full {
            validations.push(run_command(
                &self.project_root,
                "cargo_test",
                &["cargo", "test", "--all-targets", "--all-features"],
            )?);
        }
        Ok(validations)
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.proposals_dir())
            .with_context(|| format!("create {}", self.proposals_dir().display()))?;
        fs::create_dir_all(self.runs_dir())
            .with_context(|| format!("create {}", self.runs_dir().display()))?;
        Ok(())
    }

    fn proposals_dir(&self) -> PathBuf {
        self.root.join("proposals")
    }

    fn runs_dir(&self) -> PathBuf {
        self.root.join("runs")
    }

    fn proposal_json_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id, "repair-proposal-")?;
        Ok(self.proposals_dir().join(format!("{id}.json")))
    }

    fn proposal_md_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id, "repair-proposal-")?;
        Ok(self.proposals_dir().join(format!("{id}.md")))
    }

    fn proposal_context_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id, "repair-proposal-")?;
        Ok(self
            .proposals_dir()
            .join(format!("{id}.context-bundle.json")))
    }

    fn run_dir(&self, id: &str) -> Result<PathBuf> {
        validate_id(id, "repair-run-")?;
        Ok(self.runs_dir().join(id))
    }
}

fn validate_id(id: &str, prefix: &str) -> Result<()> {
    if !id.starts_with(prefix) || id.contains('/') || id.contains('\\') || id.contains("..") {
        bail!("invalid id: {id}");
    }
    Ok(())
}

fn normalize_paths(paths: Vec<String>) -> Result<Vec<String>> {
    let mut normalized = Vec::new();
    for path in paths {
        let path = normalize_path(&path)?;
        if !path.is_empty() {
            normalized.push(path);
        }
    }
    Ok(normalized)
}

fn normalize_path(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_start_matches("./");
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        bail!("absolute paths are not allowed: {raw}");
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("paths must stay inside the workspace: {raw}")
            }
        }
    }
    Ok(trimmed.to_string())
}

fn validation_plan_for_risk(risk: KnowledgeRiskLevel) -> Vec<String> {
    let mut plan = vec!["diff_static_scan".to_string(), "cargo_check".to_string()];
    if risk_rank(risk) >= risk_rank(KnowledgeRiskLevel::High) {
        plan.push("targeted_tests".to_string());
        plan.push("manual_review".to_string());
    }
    plan
}

fn sandbox_validation_summary(
    sandbox_result: Option<&SandboxValidationResult>,
) -> Vec<RepairValidationResult> {
    let Some(result) = sandbox_result else {
        return vec![RepairValidationResult {
            name: "remote_sandbox".to_string(),
            status: RepairGateStatus::Warn,
            command: Vec::new(),
            exit_code: None,
            duration_ms: 0,
            stdout_excerpt: String::new(),
            stderr_excerpt: "remote sandbox was not requested".to_string(),
        }];
    };
    let status = match result.status {
        SandboxStatus::Passed => RepairGateStatus::Pass,
        SandboxStatus::Failed | SandboxStatus::Blocked | SandboxStatus::Error => {
            RepairGateStatus::Fail
        }
    };
    vec![RepairValidationResult {
        name: "remote_sandbox".to_string(),
        status,
        command: vec![result.backend.as_deref().unwrap_or("sandbox").to_string()],
        exit_code: None,
        duration_ms: result
            .finished_at_unix_ms
            .saturating_sub(result.started_at_unix_ms)
            .into(),
        stdout_excerpt: result.summary.clone(),
        stderr_excerpt: if result.risk_flags.is_empty() {
            String::new()
        } else {
            result.risk_flags.join("; ")
        },
    }]
}

fn failure_reason_for(run: &RepairRun, failed_validation_name: Option<&str>) -> Option<String> {
    if let Some(name) = failed_validation_name {
        return Some(format!("validation failed: {name}"));
    }
    if run.model_patch_requested && run.patch_hash.is_none() {
        return Some("model patch was requested but no candidate patch was generated".to_string());
    }
    run.sandbox_result.as_ref().and_then(|result| {
        (result.status != SandboxStatus::Passed).then(|| {
            format!(
                "remote sandbox {}: {}",
                result.status.as_str(),
                result.summary
            )
        })
    })
}

fn diff_judge_for(
    proposal: &RepairProposal,
    findings: &[RiskFinding],
    patch: &str,
    patch_paths: &[String],
    model_errors: &[String],
) -> DiffJudgeResult {
    let mut reasons = Vec::new();
    let mut risk_flags = Vec::new();
    let mut suggested_next_validation = proposal.validation_plan.clone();
    if proposal.allowed_paths.is_empty() {
        reasons.push("no target paths were provided; localization remains open".to_string());
        suggested_next_validation.push("localize_target_files".to_string());
    }
    for finding in findings {
        risk_flags.push(format!(
            "{}: {:?} ({})",
            finding.path, finding.risk_level, finding.reason
        ));
    }
    if !model_errors.is_empty() {
        reasons.push(format!(
            "model patch unavailable: {}",
            model_errors.join("; ")
        ));
    }
    if !patch_paths.is_empty() {
        match validate_patch_scope(&proposal.allowed_paths, patch_paths) {
            Ok(()) => reasons.push(format!(
                "patch paths are within proposal scope: {}",
                patch_paths.join(", ")
            )),
            Err(err) => {
                reasons.push(err.to_string());
                risk_flags.push("patch_scope_violation".to_string());
            }
        }
    }
    let safety = scan_patch_safety(patch);
    reasons.extend(safety.reasons);
    risk_flags.extend(safety.risk_flags);
    let mut missing_tests = vec!["no patch-specific targeted test selected yet".to_string()];
    missing_tests.extend(safety.missing_tests);
    let verdict = if findings
        .iter()
        .any(|finding| finding.risk_level == KnowledgeRiskLevel::Blocked)
        || risk_flags
            .iter()
            .any(|flag| flag == "patch_scope_violation")
        || risk_flags.iter().any(|flag| flag == "test_deletion")
        || risk_flags.iter().any(|flag| flag == "safety_regression")
    {
        reasons.push("hard repair safety gate failed".to_string());
        RepairGateStatus::Fail
    } else if !risk_flags.is_empty() || proposal.allowed_paths.is_empty() {
        RepairGateStatus::Warn
    } else {
        reasons.push("proposal scope is low risk and ready for patch generation".to_string());
        RepairGateStatus::Pass
    };
    DiffJudgeResult {
        verdict,
        reasons,
        risk_flags,
        missing_tests,
        suggested_next_validation,
    }
}

#[derive(Debug, Default)]
struct PatchSafetyScan {
    reasons: Vec<String>,
    risk_flags: Vec<String>,
    missing_tests: Vec<String>,
}

fn scan_patch_safety(patch: &str) -> PatchSafetyScan {
    let mut scan = PatchSafetyScan::default();
    if patch.trim().is_empty() {
        return scan;
    }
    let lower = patch.to_ascii_lowercase();
    let deletes_test_markers = patch.lines().any(|line| {
        line.starts_with('-')
            && !line.starts_with("--- ")
            && (line.contains("#[test]")
                || line.contains("assert")
                || line.to_ascii_lowercase().contains("cargo test")
                || line.to_ascii_lowercase().contains("expect(\""))
    });
    if deletes_test_markers {
        scan.risk_flags.push("test_deletion".to_string());
        scan.reasons
            .push("patch deletes test/assertion-like content".to_string());
    }
    let safety_patterns = [
        "allow_high_risk: true",
        "allow_high_risk = true",
        "auto_apply_allowed\": true",
        "auto_apply_allowed = true",
        "skip_approval",
        "bypass",
        "disable_gate",
        "ignore_gate",
        "secret",
        "api_key",
    ];
    if safety_patterns
        .iter()
        .any(|pattern| lower.contains(pattern))
    {
        scan.risk_flags.push("safety_regression".to_string());
        scan.reasons
            .push("patch contains safety/secret/gate-sensitive terms".to_string());
        scan.missing_tests
            .push("manual safety review required for sensitive patch terms".to_string());
    }
    scan
}

fn model_patch_system_prompt() -> String {
    "You are Octocode Repair Implementer. Generate the smallest safe unified diff for the user's repair proposal. Only edit allowed paths. Do not delete or weaken tests. Do not change policy, sandbox, approval, key storage, or risk gates unless explicitly targeted. The patch must be directly applicable by git apply against the provided target_context. Never use placeholders, ellipses, pseudo-code, comments such as 'rest of file unchanged', or invented struct fields. Return strict JSON only.".to_string()
}

fn parse_model_patch_response(content: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(content)
        .or_else(|_| {
            let extracted = extract_json_object(content)
                .context("model did not return valid JSON and no JSON object could be extracted")?;
            serde_json::from_str(extracted).context("extracted model JSON is invalid")
        })
        .context("model did not return valid JSON")?;
    let patch = value
        .get("patch")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .trim()
        .to_string();
    if patch.is_empty() {
        bail!("model JSON did not include a non-empty patch");
    }
    if !patch.contains("--- ") || !patch.contains("+++ ") {
        bail!("model patch is not a unified diff");
    }
    reject_placeholder_patch(&patch)?;
    Ok(normalize_unified_diff_hunks(&patch))
}

fn reject_placeholder_patch(patch: &str) -> Result<()> {
    let lower = patch.to_ascii_lowercase();
    let forbidden = [
        "rest of file unchanged",
        "... rest",
        "rest unchanged",
        "placeholder",
        "pseudo-code",
        "pseudocode",
    ];
    if let Some(pattern) = forbidden.iter().find(|pattern| lower.contains(**pattern)) {
        bail!("model patch contains non-applicable placeholder text: {pattern}");
    }
    Ok(())
}

fn extract_json_object(content: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in content.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(index);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return start.map(|start| &content[start..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn normalize_unified_diff_hunks(patch: &str) -> String {
    let mut lines = Vec::new();
    let mut active_header: Option<usize> = None;
    let mut old_count = 0usize;
    let mut new_count = 0usize;

    for raw_line in patch.lines() {
        let mut line = raw_line.to_string();
        if starts_new_file_header(&line) {
            rewrite_active_hunk(&mut lines, active_header.take(), old_count, new_count);
            old_count = 0;
            new_count = 0;
        }
        if line.starts_with("@@ ") {
            rewrite_active_hunk(&mut lines, active_header.take(), old_count, new_count);
            old_count = 0;
            new_count = 0;
            lines.push(line);
            active_header = Some(lines.len() - 1);
            continue;
        }
        if active_header.is_some() {
            if line.is_empty() {
                line = " ".to_string();
            }
            match line.as_bytes().first().copied() {
                Some(b' ') => {
                    old_count += 1;
                    new_count += 1;
                }
                Some(b'-') => old_count += 1,
                Some(b'+') => new_count += 1,
                Some(b'\\') => {}
                _ => {}
            }
        }
        lines.push(line);
    }

    rewrite_active_hunk(&mut lines, active_header, old_count, new_count);
    format!("{}\n", lines.join("\n"))
}

fn starts_new_file_header(line: &str) -> bool {
    line.starts_with("diff --git ")
        || line.starts_with("--- a/")
        || line == "--- /dev/null"
        || line.starts_with("--- /dev/null\t")
}

fn rewrite_active_hunk(
    lines: &mut [String],
    header_index: Option<usize>,
    old_count: usize,
    new_count: usize,
) {
    let Some(index) = header_index else {
        return;
    };
    if let Some(rewritten) = rewrite_hunk_header(&lines[index], old_count, new_count) {
        lines[index] = rewritten;
    }
}

fn rewrite_hunk_header(header: &str, old_count: usize, new_count: usize) -> Option<String> {
    let after_open = header.strip_prefix("@@")?;
    let close_index = after_open.find("@@")?;
    let body = after_open[..close_index].trim();
    let suffix = after_open[close_index + 2..].trim_start();
    let mut ranges = body.split_whitespace();
    let old_start = parse_hunk_start(ranges.next()?, '-')?;
    let new_start = parse_hunk_start(ranges.next()?, '+')?;
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!(" {suffix}")
    };
    Some(format!(
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@{suffix}"
    ))
}

fn parse_hunk_start(range: &str, prefix: char) -> Option<usize> {
    let range = range.strip_prefix(prefix)?;
    range.split(',').next()?.parse().ok()
}

fn changed_files_from_patch(patch: &str) -> Vec<String> {
    let mut paths = parse_patch_paths(patch);
    paths.sort();
    paths.dedup();
    paths
}

fn validate_patch_scope(allowed_paths: &[String], patch_paths: &[String]) -> Result<()> {
    if patch_paths.is_empty() {
        bail!("patch does not declare touched files");
    }
    let allowed = allowed_paths
        .iter()
        .map(|path| normalize_path(path))
        .collect::<Result<Vec<_>>>()?;
    let outside = patch_paths
        .iter()
        .filter(|path| {
            let normalized = normalize_path(path).unwrap_or_else(|_| path.to_string());
            !allowed.contains(&normalized)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !outside.is_empty() {
        bail!(
            "patch touches files outside proposal scope: {}",
            outside.join(", ")
        );
    }
    Ok(())
}

fn validate_patch_applies(project_root: &Path, patch: &str) -> Result<()> {
    let path = std::env::temp_dir().join(format!("octocode-patch-check-{}.diff", Uuid::new_v4()));
    fs::write(&path, patch).with_context(|| format!("write {}", path.display()))?;
    let output = Command::new("git")
        .args(["apply", "--check", "--whitespace=nowarn"])
        .arg(&path)
        .current_dir(project_root)
        .output()
        .with_context(|| format!("run git apply --check {}", path.display()));
    let _ = fs::remove_file(&path);
    let output = output?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    bail!(
        "model patch does not apply locally: {}{}",
        truncate_patch_check_log(&stderr),
        if stdout.is_empty() {
            String::new()
        } else {
            format!("\n{}", truncate_patch_check_log(&stdout))
        }
    )
}

fn truncate_patch_check_log(text: &str) -> String {
    const LIMIT: usize = 4000;
    if text.len() <= LIMIT {
        text.to_string()
    } else {
        format!("{}...", &text[..LIMIT])
    }
}

fn patch_hash(patch: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(patch.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn run_command(root: &Path, name: &str, command: &[&str]) -> Result<RepairValidationResult> {
    let started = Instant::now();
    let output = Command::new(command[0])
        .args(&command[1..])
        .current_dir(root)
        .output()
        .with_context(|| format!("run {}", command.join(" ")))?;
    let status = if output.status.success() {
        RepairGateStatus::Pass
    } else {
        RepairGateStatus::Fail
    };
    Ok(RepairValidationResult {
        name: name.to_string(),
        status,
        command: command.iter().map(|part| (*part).to_string()).collect(),
        exit_code: output.status.code(),
        duration_ms: started.elapsed().as_millis(),
        stdout_excerpt: excerpt(&String::from_utf8_lossy(&output.stdout)),
        stderr_excerpt: excerpt(&String::from_utf8_lossy(&output.stderr)),
    })
}

fn excerpt(text: &str) -> String {
    const LIMIT: usize = 4000;
    if text.len() <= LIMIT {
        text.to_string()
    } else {
        text[..LIMIT].to_string()
    }
}

fn excerpt_for_target(relative: &str, text: &str) -> String {
    if relative == "src/repair/mod.rs" {
        return repair_patch_excerpt(text);
    }
    excerpt(text)
}

fn repair_patch_excerpt(text: &str) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let first_anchor = lines.iter().position(|line| {
        line.contains("async fn generate_model_patch")
            || line.contains("fn model_patch_user_prompt")
            || line.contains("fn target_excerpt")
    });
    let last_anchor = lines.iter().rposition(|line| {
        line.contains("fn normalize_unified_diff_hunks")
            || line.contains("fn parse_model_patch_response")
            || line.contains("fn model_patch_system_prompt")
    });
    let (Some(first), Some(last)) = (first_anchor, last_anchor) else {
        return excerpt(text);
    };
    let start = first.saturating_sub(20);
    let end = (last + 120).min(lines.len());
    lines[start..end].join("\n")
}

fn read_json_dir<T: for<'de> Deserialize<'de>>(dir: PathBuf) -> Result<Vec<T>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            let data =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            values.push(serde_json::from_str(&data)?);
        }
    }
    Ok(values)
}

fn title_from_problem(problem: &str) -> String {
    let title = problem.trim();
    if title.chars().count() > 72 {
        format!("{}...", title.chars().take(69).collect::<String>())
    } else if title.is_empty() {
        "Repair proposal".to_string()
    } else {
        title.to_string()
    }
}

fn render_proposal_md(proposal: &RepairProposal) -> String {
    format!(
        r#"# Repair Proposal

## ID

`{}`

## Problem

{}

## Scope

{}

## Risk

{:?}

## Validation Plan

{}

## Retrieved Context

- Risk findings: {}
- Similar failures: {}
- Candidate skills: {}
"#,
        proposal.id,
        proposal.problem_statement,
        proposal
            .allowed_paths
            .iter()
            .map(|path| format!("- `{path}`"))
            .collect::<Vec<_>>()
            .join("\n"),
        proposal.risk_level,
        proposal
            .validation_plan
            .iter()
            .map(|gate| format!("- `{gate}`"))
            .collect::<Vec<_>>()
            .join("\n"),
        proposal.context_bundle.risk_findings.len(),
        proposal.context_bundle.similar_failures.len(),
        proposal.context_bundle.candidate_skills.len()
    )
}

fn render_run_report(proposal: &RepairProposal, run: &RepairRun) -> String {
    format!(
        r#"# Repair Run Report

## Run

`{}`

## Proposal

`{}`

## Status

`{:?}`

## Problem

{}

## Touched / Target Files

{}

## Candidate Patch

- Model patch requested: `{}`
- Patch paths: {}
- Patch hash: {}
- Model errors: {}

## Risk Findings

{}

## Diff Judge

- Verdict: `{:?}`
- Reasons: {}
- Risk flags: {}
- Missing tests: {}

## Remote Sandbox

{}

## Retrieved Context

- Similar failures: {}
- Candidate skills: {}

## Validation

{}

## Notes

This first repair-agent implementation records scope, risk, validation, and failure memory. It does not auto-apply patches.
"#,
        run.id,
        proposal.id,
        run.status,
        proposal.problem_statement,
        list_or_none(&run.touched_files),
        run.model_patch_requested,
        list_or_none(&run.patch_paths),
        run.patch_hash.as_deref().unwrap_or("none"),
        list_or_none(&run.model_errors),
        if run.risk_findings.is_empty() {
            "none".to_string()
        } else {
            run.risk_findings
                .iter()
                .map(|finding| {
                    format!(
                        "- `{}`: {:?} - {}",
                        finding.path, finding.risk_level, finding.reason
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        },
        run.diff_judge.verdict,
        list_or_none(&run.diff_judge.reasons),
        list_or_none(&run.diff_judge.risk_flags),
        list_or_none(&run.diff_judge.missing_tests),
        render_sandbox_result(run.sandbox_result.as_ref()),
        proposal.context_bundle.similar_failures.len(),
        proposal.context_bundle.candidate_skills.len(),
        if run.validations.is_empty() {
            "none".to_string()
        } else {
            run.validations
                .iter()
                .map(|validation| {
                    format!(
                        "- `{}`: {:?} exit={:?} duration={}ms",
                        validation.name,
                        validation.status,
                        validation.exit_code,
                        validation.duration_ms
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    )
}

fn render_sandbox_result(result: Option<&SandboxValidationResult>) -> String {
    let Some(result) = result else {
        return "not requested".to_string();
    };
    format!(
        "- Backend: {}\n- Run: `{}`\n- Status: `{}`\n- Summary: {}\n- Artifact: {}",
        result.backend.as_deref().unwrap_or("unknown"),
        result.run_id,
        result.status.as_str(),
        result.summary,
        result.artifact_url.as_deref().unwrap_or("none")
    )
}

fn list_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values
            .iter()
            .map(|value| format!("`{value}`"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
