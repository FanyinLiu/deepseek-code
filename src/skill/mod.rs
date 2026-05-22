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

    /// Find skills whose SKILL.md frontmatter declares a `keywords:` or
    /// `trigger:` term that appears in the user's input. Returns up to
    /// `limit` (skill_id, body) pairs; the body is the SKILL.md text minus
    /// frontmatter, ready to drop into system prompt augmentation.
    ///
    /// Used by the orchestrator at turn start to auto-inject skill bodies
    /// when the user's question matches a saved workflow.
    pub fn triggered_for_input(
        &self,
        user_input: &str,
        limit: usize,
    ) -> Result<Vec<(String, String)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let lower = user_input.to_lowercase();
        let mut hits: Vec<(String, String)> = Vec::new();
        let entries =
            fs::read_dir(&self.root).with_context(|| format!("read {}", self.root.display()))?;
        for entry in entries.flatten() {
            let skill_dir = entry.path();
            let md_path = skill_dir.join("SKILL.md");
            if !md_path.is_file() {
                continue;
            }
            let id = match skill_dir.file_name().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };
            let raw = match fs::read_to_string(&md_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let triggers = parse_skill_triggers(&raw);
            if triggers.is_empty() {
                continue;
            }
            let matched = triggers
                .iter()
                .any(|kw| !kw.is_empty() && lower.contains(&kw.to_lowercase()));
            if matched {
                let body = strip_frontmatter(&raw);
                hits.push((id, body));
                if hits.len() >= limit {
                    break;
                }
            }
        }
        Ok(hits)
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

/// Pull `keywords:` / `trigger:` fields out of a SKILL.md YAML frontmatter.
///
/// Accepts either an inline list `keywords: [a, b]`, a JSON-style array,
/// or comma/whitespace-separated tokens after the colon. Multiple keys are
/// merged. Missing frontmatter returns empty.
pub(crate) fn parse_skill_triggers(markdown: &str) -> Vec<String> {
    let Some(frontmatter) = extract_frontmatter(markdown) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in frontmatter.lines() {
        let line = line.trim();
        let lower = line.to_lowercase();
        if let Some(rest) = lower
            .strip_prefix("keywords:")
            .or_else(|| lower.strip_prefix("trigger:"))
            .or_else(|| lower.strip_prefix("triggers:"))
        {
            let raw = &line[line.len() - rest.len()..];
            for token in raw
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(|c: char| c == ',' || c.is_whitespace())
            {
                let token = token.trim().trim_matches(|c: char| c == '"' || c == '\'');
                if !token.is_empty() {
                    out.push(token.to_string());
                }
            }
        }
    }
    out
}

fn extract_frontmatter(markdown: &str) -> Option<&str> {
    let text = markdown.trim_start();
    let stripped = text.strip_prefix("---")?.trim_start_matches('\n');
    let end = stripped.find("\n---")?;
    Some(&stripped[..end])
}

/// Return SKILL.md content with the YAML frontmatter (if any) removed.
pub(crate) fn strip_frontmatter(markdown: &str) -> String {
    let text = markdown.trim_start();
    if let Some(stripped) = text.strip_prefix("---") {
        if let Some(end) = stripped.find("\n---") {
            return stripped[end + 4..].trim_start_matches('\n').to_string();
        }
    }
    markdown.to_string()
}

#[cfg(test)]
mod skill_trigger_tests {
    use super::*;

    #[test]
    fn parses_keywords_inline_list() {
        let md = "---\nkeywords: [refactor, lint]\n---\nbody";
        let kws = parse_skill_triggers(md);
        assert!(kws.contains(&"refactor".to_string()));
        assert!(kws.contains(&"lint".to_string()));
    }

    #[test]
    fn parses_trigger_alias() {
        let md = "---\ntrigger: review\n---\nbody";
        let kws = parse_skill_triggers(md);
        assert_eq!(kws, vec!["review".to_string()]);
    }

    #[test]
    fn no_frontmatter_returns_empty() {
        assert!(parse_skill_triggers("just body, no frontmatter").is_empty());
    }

    #[test]
    fn strip_frontmatter_removes_yaml_block() {
        let md = "---\nkeywords: [a]\n---\nThe body.\nMore.";
        assert_eq!(strip_frontmatter(md), "The body.\nMore.");
    }

    #[test]
    fn triggered_for_input_matches_keyword_in_input() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().to_path_buf();
        let skills = project.join(".octocode").join("skills");
        let skill_dir = skills.join("refactor-large-function");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nkeywords: [refactor, split]\n---\nUse careful diff splits.",
        )
        .unwrap();
        std::fs::write(
            skill_dir.join("metadata.json"),
            r#"{"id":"refactor-large-function","version":1,"created_at":"2026-05-22T00:00:00Z","created_from_run":null,"status":"active","success_count":0,"failure_count":0,"last_used_at":null,"tags":[]}"#,
        )
        .unwrap();

        let store = SkillStore::for_project(&project);
        let hits = store
            .triggered_for_input("please refactor src/foo.rs", 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("Use careful diff splits"));

        let miss = store
            .triggered_for_input("unrelated question about cats", 5)
            .unwrap();
        assert!(miss.is_empty());
    }
}
