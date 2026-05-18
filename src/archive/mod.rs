use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const ARCHIVE_DIR: &str = ".octocode/evolution/archive";

#[derive(Debug, Clone)]
pub struct ArchiveStore {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveCandidate {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub source: String,
    pub parent_id: Option<String>,
    pub proposal_id: String,
    pub run_id: String,
    pub patch_hash: Option<String>,
    #[serde(default)]
    pub patch_hash_sha256: Option<String>,
    #[serde(default)]
    pub sandbox_status: Option<String>,
    #[serde(default)]
    pub sandbox_run_id: Option<String>,
    #[serde(default)]
    pub sandbox_backend: Option<String>,
    #[serde(default)]
    pub sandbox_artifact_url: Option<String>,
    pub touched_files: Vec<String>,
    pub risk_level: String,
    pub status: String,
    pub gate_summary: Vec<String>,
    pub judge_summary: Vec<String>,
    pub cost_summary: Vec<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveCandidateView {
    pub candidate: ArchiveCandidate,
    pub utility_score: i32,
    pub score_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchiveStatus {
    pub candidates: usize,
    pub lineage_events: usize,
    pub passed: usize,
    pub failed: usize,
    pub blocked: usize,
    pub rejected: usize,
    pub rolled_back: usize,
    pub disputed: usize,
}

#[derive(Debug, Clone)]
pub struct NewArchiveCandidate {
    pub source: String,
    pub parent_id: Option<String>,
    pub proposal_id: String,
    pub run_id: String,
    pub patch_hash: Option<String>,
    pub patch_hash_sha256: Option<String>,
    pub sandbox_status: Option<String>,
    pub sandbox_run_id: Option<String>,
    pub sandbox_backend: Option<String>,
    pub sandbox_artifact_url: Option<String>,
    pub touched_files: Vec<String>,
    pub risk_level: String,
    pub status: String,
    pub gate_summary: Vec<String>,
    pub judge_summary: Vec<String>,
    pub cost_summary: Vec<String>,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LineageEvent {
    pub event: String,
    pub time: DateTime<Utc>,
    pub candidate_id: String,
    pub parent_id: Option<String>,
    pub proposal_id: String,
    pub run_id: String,
    pub summary: String,
}

impl ArchiveStore {
    pub fn for_project(project_root: impl AsRef<Path>) -> Self {
        Self {
            root: project_root.as_ref().join(ARCHIVE_DIR),
        }
    }

    pub fn record_candidate(&self, input: NewArchiveCandidate) -> Result<ArchiveCandidate> {
        self.ensure_dirs()?;
        let candidate = ArchiveCandidate {
            id: format!("candidate-{}", Uuid::new_v4()),
            created_at: Utc::now(),
            source: input.source,
            parent_id: input.parent_id,
            proposal_id: input.proposal_id,
            run_id: input.run_id,
            patch_hash: input.patch_hash,
            patch_hash_sha256: input.patch_hash_sha256,
            sandbox_status: input.sandbox_status,
            sandbox_run_id: input.sandbox_run_id,
            sandbox_backend: input.sandbox_backend,
            sandbox_artifact_url: input.sandbox_artifact_url,
            touched_files: input.touched_files,
            risk_level: input.risk_level,
            status: input.status,
            gate_summary: input.gate_summary,
            judge_summary: input.judge_summary,
            cost_summary: input.cost_summary,
            failure_reason: input.failure_reason,
        };
        let path = self.candidates_dir().join(format!("{}.json", candidate.id));
        fs::write(&path, serde_json::to_string_pretty(&candidate)?)
            .with_context(|| format!("write {}", path.display()))?;
        self.append_lineage(LineageEvent {
            event: "candidate_created".to_string(),
            time: candidate.created_at,
            candidate_id: candidate.id.clone(),
            parent_id: candidate.parent_id.clone(),
            proposal_id: candidate.proposal_id.clone(),
            run_id: candidate.run_id.clone(),
            summary: format!(
                "{} candidate recorded with status {}",
                candidate.source, candidate.status
            ),
        })?;
        Ok(candidate)
    }

    pub fn status(&self) -> Result<ArchiveStatus> {
        let candidates = self.list_candidates()?;
        let lineage_events = self.list_lineage()?.len();
        Ok(ArchiveStatus {
            candidates: candidates.len(),
            lineage_events,
            passed: candidates
                .iter()
                .filter(|candidate| candidate.status.eq_ignore_ascii_case("passed"))
                .count(),
            failed: candidates
                .iter()
                .filter(|candidate| candidate.status.eq_ignore_ascii_case("failed"))
                .count(),
            blocked: candidates
                .iter()
                .filter(|candidate| candidate.status.eq_ignore_ascii_case("blocked"))
                .count(),
            rejected: candidates
                .iter()
                .filter(|candidate| candidate.status.eq_ignore_ascii_case("rejected"))
                .count(),
            rolled_back: candidates
                .iter()
                .filter(|candidate| candidate.status.eq_ignore_ascii_case("rolled_back"))
                .count(),
            disputed: candidates
                .iter()
                .filter(|candidate| candidate.status.eq_ignore_ascii_case("disputed"))
                .count(),
        })
    }

    pub fn list_candidate_views(&self) -> Result<Vec<ArchiveCandidateView>> {
        Ok(self
            .list_candidates()?
            .into_iter()
            .map(candidate_view)
            .collect())
    }

    pub fn load_candidate_view(&self, candidate_id: &str) -> Result<ArchiveCandidateView> {
        validate_candidate_id(candidate_id)?;
        let path = self.candidates_dir().join(format!("{candidate_id}.json"));
        let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let candidate =
            serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))?;
        Ok(candidate_view(candidate))
    }

    pub fn list_lineage(&self) -> Result<Vec<LineageEvent>> {
        self.ensure_dirs()?;
        let path = self.root.join("lineage.jsonl");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut events = Vec::new();
        for line in data.lines().filter(|line| !line.trim().is_empty()) {
            events.push(serde_json::from_str(line).context("parse lineage event")?);
        }
        Ok(events)
    }

    fn list_candidates(&self) -> Result<Vec<ArchiveCandidate>> {
        self.ensure_dirs()?;
        let dir = self.candidates_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut candidates: Vec<ArchiveCandidate> = Vec::new();
        for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                let data = fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                candidates.push(
                    serde_json::from_str(&data)
                        .with_context(|| format!("parse {}", path.display()))?,
                );
            }
        }
        candidates.sort_by_key(|candidate| candidate.created_at);
        Ok(candidates)
    }

    fn append_lineage(&self, event: LineageEvent) -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.join("lineage.jsonl"))
            .with_context(|| format!("open {}", self.root.join("lineage.jsonl").display()))?;
        writeln!(file, "{}", serde_json::to_string(&event)?)
            .with_context(|| format!("append {}", self.root.join("lineage.jsonl").display()))?;
        Ok(())
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(self.candidates_dir())
            .with_context(|| format!("create {}", self.candidates_dir().display()))
    }

    fn candidates_dir(&self) -> PathBuf {
        self.root.join("candidates")
    }
}

fn candidate_view(candidate: ArchiveCandidate) -> ArchiveCandidateView {
    let (utility_score, score_reasons) = score_candidate(&candidate);
    ArchiveCandidateView {
        candidate,
        utility_score,
        score_reasons,
    }
}

fn score_candidate(candidate: &ArchiveCandidate) -> (i32, Vec<String>) {
    let mut score = 0;
    let mut reasons = Vec::new();
    match candidate.status.to_ascii_lowercase().as_str() {
        "passed" => {
            if candidate
                .sandbox_status
                .as_deref()
                .is_some_and(|status| status.eq_ignore_ascii_case("passed"))
            {
                score += 20;
                reasons.push("status:passed+sandbox:passed +20".to_string());
            } else {
                score += 2;
                reasons.push("status:passed_without_sandbox +2".to_string());
            }
        }
        "failed" => {
            score -= 20;
            reasons.push("status:failed -20".to_string());
        }
        "blocked" => {
            score -= 40;
            reasons.push("status:blocked -40".to_string());
        }
        "rejected" => {
            score -= 30;
            reasons.push("status:rejected -30".to_string());
        }
        "rolled_back" => {
            score -= 35;
            reasons.push("status:rolled_back -35".to_string());
        }
        _ => {
            reasons.push("status:neutral +0".to_string());
        }
    }
    match candidate.risk_level.to_ascii_lowercase().as_str() {
        "high" => {
            score -= 10;
            reasons.push("risk:high -10".to_string());
        }
        "blocked" => {
            score -= 30;
            reasons.push("risk:blocked -30".to_string());
        }
        "medium" => {
            score -= 3;
            reasons.push("risk:medium -3".to_string());
        }
        _ => {}
    }
    for gate in &candidate.gate_summary {
        if gate.to_ascii_lowercase().contains("pass") {
            score += 2;
        } else if gate.to_ascii_lowercase().contains("fail") {
            score -= 5;
        }
    }
    match candidate.sandbox_status.as_deref() {
        Some("passed") => {
            score += 8;
            reasons.push("sandbox:passed +8".to_string());
        }
        Some("failed") | Some("blocked") | Some("error") => {
            score -= 25;
            reasons.push("sandbox:not_passed -25".to_string());
        }
        _ => {}
    }
    if !candidate.touched_files.is_empty() {
        score += 1;
        reasons.push("scoped_files +1".to_string());
    }
    (score, reasons)
}

fn validate_candidate_id(candidate_id: &str) -> Result<()> {
    if !candidate_id.starts_with("candidate-")
        || candidate_id.contains('/')
        || candidate_id.contains('\\')
        || candidate_id.contains("..")
    {
        bail!("invalid candidate id: {candidate_id}");
    }
    Ok(())
}
