use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const KNOWLEDGE_DIR: &str = ".octocode/knowledge";

#[derive(Debug, Clone)]
pub struct KnowledgeStore {
    project_root: PathBuf,
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectKnowledgeUpdate {
    pub project_root: String,
    pub project_path: String,
    pub risk_map_path: String,
    pub failure_memory_path: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskMap {
    pub version: u32,
    pub generated_at: DateTime<Utc>,
    pub paths: Vec<RiskPathRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskPathRule {
    pub pattern: String,
    pub risk_level: KnowledgeRiskLevel,
    pub reason: String,
    pub required_gates: Vec<String>,
    pub auto_apply_allowed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeRiskLevel {
    Low,
    Medium,
    High,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RiskFinding {
    pub path: String,
    pub risk_level: KnowledgeRiskLevel,
    pub reason: String,
    pub required_gates: Vec<String>,
    pub auto_apply_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FailureMemoryEntry {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub source: String,
    pub proposal_id: Option<String>,
    pub run_id: Option<String>,
    pub symptom: String,
    pub root_cause: Option<String>,
    pub touched_files: Vec<String>,
    pub failed_gate: Option<String>,
    pub validation_excerpt: Option<String>,
    pub failed_patch_hash: Option<String>,
    pub avoid_pattern: Option<String>,
    pub lesson: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewFailureMemoryEntry {
    pub source: String,
    pub proposal_id: Option<String>,
    pub run_id: Option<String>,
    pub symptom: String,
    pub root_cause: Option<String>,
    pub touched_files: Vec<String>,
    pub failed_gate: Option<String>,
    pub validation_excerpt: Option<String>,
    pub failed_patch_hash: Option<String>,
    pub avoid_pattern: Option<String>,
    pub lesson: Option<String>,
    pub tags: Vec<String>,
}

impl KnowledgeStore {
    pub fn for_project(project_root: impl AsRef<Path>) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let root = project_root.join(KNOWLEDGE_DIR);
        Self { project_root, root }
    }

    pub fn update_project_knowledge(&self) -> Result<ProjectKnowledgeUpdate> {
        self.ensure_dirs()?;
        let risk_map = self.load_or_create_risk_map()?;
        let updated_at = Utc::now();
        let project_md = render_project_knowledge(&self.project_root, &risk_map, updated_at);
        fs::write(self.project_path(), project_md)
            .with_context(|| format!("write {}", self.project_path().display()))?;
        Ok(ProjectKnowledgeUpdate {
            project_root: self.project_root.display().to_string(),
            project_path: self.project_path().display().to_string(),
            risk_map_path: self.risk_map_path().display().to_string(),
            failure_memory_path: self.failure_memory_path().display().to_string(),
            updated_at,
        })
    }

    pub fn load_or_create_risk_map(&self) -> Result<RiskMap> {
        self.ensure_dirs()?;
        let path = self.risk_map_path();
        if path.exists() {
            let data =
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
            return serde_json::from_str(&data)
                .with_context(|| format!("parse {}", path.display()));
        }
        let risk_map = default_risk_map();
        fs::write(&path, serde_json::to_string_pretty(&risk_map)?)
            .with_context(|| format!("write {}", path.display()))?;
        Ok(risk_map)
    }

    pub fn classify_paths(&self, paths: &[String]) -> Result<Vec<RiskFinding>> {
        let risk_map = self.load_or_create_risk_map()?;
        let mut findings = Vec::new();
        for path in paths {
            for rule in &risk_map.paths {
                if rule_matches(&rule.pattern, path) {
                    findings.push(RiskFinding {
                        path: path.clone(),
                        risk_level: rule.risk_level,
                        reason: rule.reason.clone(),
                        required_gates: rule.required_gates.clone(),
                        auto_apply_allowed: rule.auto_apply_allowed,
                    });
                }
            }
        }
        Ok(findings)
    }

    pub fn append_failure(&self, input: NewFailureMemoryEntry) -> Result<FailureMemoryEntry> {
        self.ensure_dirs()?;
        let entry = FailureMemoryEntry {
            id: format!("failure-{}", Uuid::new_v4()),
            created_at: Utc::now(),
            source: input.source,
            proposal_id: input.proposal_id,
            run_id: input.run_id,
            symptom: input.symptom,
            root_cause: input.root_cause,
            touched_files: input.touched_files,
            failed_gate: input.failed_gate,
            validation_excerpt: input.validation_excerpt,
            failed_patch_hash: input.failed_patch_hash,
            avoid_pattern: input.avoid_pattern,
            lesson: input.lesson,
            tags: input.tags,
        };
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.failure_memory_path())
            .with_context(|| format!("open {}", self.failure_memory_path().display()))?;
        writeln!(file, "{}", serde_json::to_string(&entry)?)
            .with_context(|| format!("append {}", self.failure_memory_path().display()))?;
        Ok(entry)
    }

    pub fn list_failures(&self) -> Result<Vec<FailureMemoryEntry>> {
        self.ensure_dirs()?;
        let path = self.failure_memory_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let mut entries = Vec::new();
        for line in data.lines().filter(|line| !line.trim().is_empty()) {
            entries.push(serde_json::from_str(line).context("parse failure-memory entry")?);
        }
        Ok(entries)
    }

    pub fn find_similar_failures(
        &self,
        query: &str,
        paths: &[String],
        limit: usize,
    ) -> Result<Vec<FailureMemoryEntry>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let terms = search_terms(query, paths);
        let mut scored = self
            .list_failures()?
            .into_iter()
            .filter_map(|entry| {
                let score = failure_score(&entry, &terms);
                (score > 0).then_some((score, entry))
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, entry)| entry)
            .collect())
    }

    pub fn project_path(&self) -> PathBuf {
        self.root.join("project.md")
    }

    pub fn risk_map_path(&self) -> PathBuf {
        self.root.join("risk-map.json")
    }

    pub fn failure_memory_path(&self) -> PathBuf {
        self.root.join("failure-memory.jsonl")
    }

    fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.root).with_context(|| format!("create {}", self.root.display()))
    }
}

fn search_terms(query: &str, paths: &[String]) -> Vec<String> {
    query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .chain(
            paths
                .iter()
                .flat_map(|path| path.split(|ch: char| !ch.is_ascii_alphanumeric())),
        )
        .map(str::to_ascii_lowercase)
        .filter(|term| term.len() >= 3)
        .collect()
}

fn failure_score(entry: &FailureMemoryEntry, terms: &[String]) -> usize {
    let haystack = format!(
        "{} {} {} {} {}",
        entry.symptom,
        entry.root_cause.as_deref().unwrap_or_default(),
        entry.touched_files.join(" "),
        entry.avoid_pattern.as_deref().unwrap_or_default(),
        entry.tags.join(" ")
    )
    .to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
}

pub fn max_risk_level(findings: &[RiskFinding]) -> KnowledgeRiskLevel {
    findings
        .iter()
        .map(|finding| finding.risk_level)
        .max_by_key(|risk| risk_rank(*risk))
        .unwrap_or(KnowledgeRiskLevel::Low)
}

pub fn risk_rank(risk: KnowledgeRiskLevel) -> u8 {
    match risk {
        KnowledgeRiskLevel::Low => 0,
        KnowledgeRiskLevel::Medium => 1,
        KnowledgeRiskLevel::High => 2,
        KnowledgeRiskLevel::Blocked => 3,
    }
}

fn default_risk_map() -> RiskMap {
    RiskMap {
        version: 1,
        generated_at: Utc::now(),
        paths: vec![
            high("src/policy/**", "approval and safety policy"),
            high("src/defense/**", "defense and adversarial safety checks"),
            high("src/storage/keyring.rs", "provider credential storage"),
            high(
                "src/tools/run_command.rs",
                "shell command execution boundary",
            ),
            high("src/tools/dispatch.rs", "tool dispatch boundary"),
            high("src/cli/login.rs", "API key ingestion"),
            high("src/evolution/**", "self-evolution controller"),
            blocked(
                "**/risk-map.json",
                "risk policy cannot lower itself silently",
            ),
        ],
    }
}

fn high(pattern: &str, reason: &str) -> RiskPathRule {
    RiskPathRule {
        pattern: pattern.to_string(),
        risk_level: KnowledgeRiskLevel::High,
        reason: reason.to_string(),
        required_gates: vec![
            "cargo_check".to_string(),
            "targeted_tests".to_string(),
            "manual_review".to_string(),
        ],
        auto_apply_allowed: false,
    }
}

fn blocked(pattern: &str, reason: &str) -> RiskPathRule {
    RiskPathRule {
        pattern: pattern.to_string(),
        risk_level: KnowledgeRiskLevel::Blocked,
        reason: reason.to_string(),
        required_gates: vec!["blocked".to_string()],
        auto_apply_allowed: false,
    }
}

fn rule_matches(pattern: &str, path: &str) -> bool {
    glob::Pattern::new(pattern)
        .map(|pattern| pattern.matches(path))
        .unwrap_or(false)
}

fn render_project_knowledge(
    project_root: &Path,
    risk_map: &RiskMap,
    updated_at: DateTime<Utc>,
) -> String {
    let cargo = project_root.join("Cargo.toml").exists();
    let src = project_root.join("src").exists();
    format!(
        r#"# Project Knowledge

## Product Identity

Octocode is a multi-model, multi-agent coding CLI.

## Project Root

`{}`

## Architecture Map

- Rust CLI project: {}
- Source directory present: {}
- Runtime directory: `.octocode`

## Core Modules

- `src/cli*`: command-line entrypoints and command handlers.
- `src/provider*`: model/provider routing.
- `src/repair*`: repair proposals, runs, reports, and failure memory integration.
- `src/knowledge*`: project knowledge, risk map, and failure memory.
- `src/evolution*`: self-evolution proposals, gates, apply, and rollback.
- `src/tui*`: terminal UI.

## Validation Commands

- `cargo check --all-targets --all-features`
- `cargo test --all-targets --all-features`

## High-Risk Areas

{}

## Known Constraints

- Do not auto-apply high-risk core changes.
- Do not weaken tests, policy, sandboxing, approval, or credential handling.
- Prefer diff-level and deterministic gates before model judgment.

## Last Updated

{}
"#,
        project_root.display(),
        cargo,
        src,
        risk_map
            .paths
            .iter()
            .map(|rule| format!(
                "- `{}`: {:?} - {}",
                rule.pattern, rule.risk_level, rule.reason
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        updated_at
    )
}
