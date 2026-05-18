use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::repair::{RepairRun, REPAIR_DIR};

pub const SKILLS_DIR: &str = ".octocode/skills";

#[derive(Debug, Clone)]
pub struct SkillStore {
    project_root: PathBuf,
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillMetadata {
    pub id: String,
    pub version: u32,
    pub created_at: DateTime<Utc>,
    pub created_from_run: Option<String>,
    pub status: SkillStatus,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_used_at: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSummary {
    pub id: String,
    pub status: SkillStatus,
    pub path: String,
    pub created_from_run: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillTestReport {
    pub id: String,
    pub status: SkillTestStatus,
    pub checks: Vec<SkillTestCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillTestCheck {
    pub name: String,
    pub passed: bool,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillStatus {
    Draft,
    Active,
    Deprecated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillTestStatus {
    Pass,
    Fail,
}

impl SkillStore {
    pub fn for_project(project_root: impl AsRef<Path>) -> Self {
        let project_root = project_root.as_ref().to_path_buf();
        let root = project_root.join(SKILLS_DIR);
        Self { project_root, root }
    }

    pub fn add_from_repair_run(&self, run_id: &str, name: Option<String>) -> Result<SkillMetadata> {
        self.ensure_dirs()?;
        validate_run_id(run_id)?;
        let run = self.load_repair_run(run_id)?;
        let id = unique_skill_id(
            &self.root,
            name.as_deref().unwrap_or_else(|| default_skill_name(&run)),
        );
        let skill_dir = self.skill_dir(&id)?;
        fs::create_dir_all(skill_dir.join("examples"))
            .with_context(|| format!("create {}", skill_dir.join("examples").display()))?;
        fs::create_dir_all(skill_dir.join("tests"))
            .with_context(|| format!("create {}", skill_dir.join("tests").display()))?;
        let metadata = SkillMetadata {
            id: id.clone(),
            version: 1,
            created_at: Utc::now(),
            created_from_run: Some(run.id.clone()),
            status: SkillStatus::Draft,
            success_count: u32::from(
                matches!(run.status, crate::repair::RepairRunStatus::Passed)
                    && run.patch_hash.is_some()
                    && run.sandbox_result.as_ref().is_some_and(|result| {
                        result.status == crate::remote_sandbox::SandboxStatus::Passed
                    }),
            ),
            failure_count: u32::from(matches!(
                run.status,
                crate::repair::RepairRunStatus::Failed | crate::repair::RepairRunStatus::Blocked
            )),
            last_used_at: None,
            tags: skill_tags(&run),
        };
        fs::write(
            skill_dir.join("metadata.json"),
            serde_json::to_string_pretty(&metadata)?,
        )
        .with_context(|| format!("write {}", skill_dir.join("metadata.json").display()))?;
        fs::write(
            skill_dir.join("SKILL.md"),
            render_skill_markdown(&metadata, &run),
        )
        .with_context(|| format!("write {}", skill_dir.join("SKILL.md").display()))?;
        append_trace(
            &skill_dir,
            &format!("draft skill generated from repair run {}", run.id),
        )?;
        Ok(metadata)
    }

    pub fn list(&self) -> Result<Vec<SkillSummary>> {
        self.ensure_dirs()?;
        let mut summaries = Vec::new();
        for entry in
            fs::read_dir(&self.root).with_context(|| format!("read {}", self.root.display()))?
        {
            let entry = entry?;
            let metadata_path = entry.path().join("metadata.json");
            if metadata_path.exists() {
                let metadata = self.load_metadata_from_path(&metadata_path)?;
                summaries.push(SkillSummary {
                    id: metadata.id,
                    status: metadata.status,
                    path: entry.path().display().to_string(),
                    created_from_run: metadata.created_from_run,
                    tags: metadata.tags,
                });
            }
        }
        summaries.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(summaries)
    }

    pub fn find_relevant(
        &self,
        query: &str,
        paths: &[String],
        limit: usize,
    ) -> Result<Vec<SkillSummary>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let terms = search_terms(query, paths);
        let mut scored = self
            .list()?
            .into_iter()
            .filter_map(|skill| {
                let score = skill_score(&skill, &terms);
                (score > 0).then_some((score, skill))
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, skill)| skill)
            .collect())
    }

    pub fn show(&self, skill_id: &str) -> Result<String> {
        let path = self.skill_dir(skill_id)?.join("SKILL.md");
        fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))
    }

    pub fn test(&self, skill_id: &str) -> Result<SkillTestReport> {
        let skill_dir = self.skill_dir(skill_id)?;
        let checks = vec![
            check_exists("SKILL.md", skill_dir.join("SKILL.md")),
            check_exists("metadata.json", skill_dir.join("metadata.json")),
            check_exists("examples", skill_dir.join("examples")),
            check_exists("tests", skill_dir.join("tests")),
        ];
        let status = if checks.iter().all(|check| check.passed) {
            SkillTestStatus::Pass
        } else {
            SkillTestStatus::Fail
        };
        Ok(SkillTestReport {
            id: skill_id.to_string(),
            status,
            checks,
        })
    }

    pub fn record_use(&self, skill_id: &str, run_id: &str, success: bool) -> Result<()> {
        let skill_dir = self.skill_dir(skill_id)?;
        let metadata_path = skill_dir.join("metadata.json");
        let mut metadata = self.load_metadata_from_path(&metadata_path)?;
        metadata.last_used_at = Some(Utc::now());
        if success {
            metadata.success_count = metadata.success_count.saturating_add(1);
        } else {
            metadata.failure_count = metadata.failure_count.saturating_add(1);
        }
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)
            .with_context(|| format!("write {}", metadata_path.display()))?;
        append_trace(
            &skill_dir,
            &format!("skill used by repair run {run_id}; success={success}"),
        )?;
        Ok(())
    }

    fn load_repair_run(&self, run_id: &str) -> Result<RepairRun> {
        let path = self
            .project_root
            .join(REPAIR_DIR)
            .join("runs")
            .join(run_id)
            .join("run.json");
        let data = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))
    }

    fn load_metadata_from_path(&self, path: &Path) -> Result<SkillMetadata> {
        let data = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))
    }

    fn skill_dir(&self, skill_id: &str) -> Result<PathBuf> {
        validate_skill_id(skill_id)?;
        Ok(self.root.join(skill_id))
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

fn skill_score(skill: &SkillSummary, terms: &[String]) -> usize {
    let haystack = format!("{} {}", skill.id, skill.tags.join(" ")).to_ascii_lowercase();
    terms
        .iter()
        .filter(|term| haystack.contains(term.as_str()))
        .count()
}

fn validate_run_id(run_id: &str) -> Result<()> {
    if !run_id.starts_with("repair-run-")
        || run_id.contains('/')
        || run_id.contains('\\')
        || run_id.contains("..")
    {
        bail!("invalid repair run id: {run_id}");
    }
    Ok(())
}

fn validate_skill_id(skill_id: &str) -> Result<()> {
    if skill_id.is_empty()
        || skill_id.contains('/')
        || skill_id.contains('\\')
        || skill_id.contains("..")
    {
        bail!("invalid skill id: {skill_id}");
    }
    Ok(())
}

fn unique_skill_id(root: &Path, raw: &str) -> String {
    let base = slug(raw);
    if !root.join(&base).exists() {
        return base;
    }
    for index in 2..1000 {
        let candidate = format!("{base}-{index}");
        if !root.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{base}-{}", Utc::now().timestamp())
}

fn slug(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in raw.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "repair-skill".to_string()
    } else {
        trimmed.chars().take(64).collect()
    }
}

fn default_skill_name(run: &RepairRun) -> &str {
    if run
        .touched_files
        .iter()
        .any(|path| path.contains("provider"))
    {
        "provider-repair"
    } else if run.touched_files.iter().any(|path| path.contains("cli")) {
        "cli-repair"
    } else {
        "repair-run-skill"
    }
}

fn skill_tags(run: &RepairRun) -> Vec<String> {
    let mut tags = vec!["repair".to_string(), format!("status:{:?}", run.status)];
    if run
        .touched_files
        .iter()
        .any(|path| path.contains("provider"))
    {
        tags.push("provider".to_string());
    }
    if run.touched_files.iter().any(|path| path.contains("cli")) {
        tags.push("cli".to_string());
    }
    tags
}

fn render_skill_markdown(metadata: &SkillMetadata, run: &RepairRun) -> String {
    format!(
        r#"# {}

## Purpose

Draft repair skill generated from repair run `{}`.

## When to Use

- Use when a future task resembles the touched files or validation pattern from this run.
- Use as a planning aid, not as an automatic patch authority.

## When Not to Use

- Do not use for high-risk policy, sandbox, approval, or credential changes without stronger gates.
- Do not use when validation requirements differ from this run.

## Required Context

- Repair proposal id: `{}`
- Touched files: {}
- Run status: `{:?}`

## Steps

1. Review the current issue and compare it with this run.
2. Check risk-map before selecting target files.
3. Generate the smallest scoped patch.
4. Run deterministic validation before model judgment.
5. Record failures back into failure-memory.

## Validation

{}

## Examples

Add examples after this draft is successfully reused.

## Failure Modes

- Overgeneralizing this draft to unrelated files.
- Reusing it without checking current risk-map.
- Treating this draft as proof that future patches are safe.

## Related Files

{}
"#,
        metadata.id,
        run.id,
        run.proposal_id,
        list_or_none(&run.touched_files),
        run.status,
        run.validations
            .iter()
            .map(|validation| format!(
                "- `{}`: {:?} exit={:?}",
                validation.name, validation.status, validation.exit_code
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        list_or_none(&run.touched_files)
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

fn check_exists(name: &str, path: PathBuf) -> SkillTestCheck {
    SkillTestCheck {
        name: name.to_string(),
        passed: path.exists(),
        message: path.display().to_string(),
    }
}

fn append_trace(skill_dir: &Path, summary: &str) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(skill_dir.join("traces.jsonl"))
        .with_context(|| format!("open {}", skill_dir.join("traces.jsonl").display()))?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(&serde_json::json!({
            "time": Utc::now(),
            "event": "skill_draft_created",
            "summary": summary
        }))?
    )
    .with_context(|| format!("append {}", skill_dir.join("traces.jsonl").display()))?;
    Ok(())
}
