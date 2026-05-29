use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::repair::{RepairRun, REPAIR_DIR};

pub const SKILLS_DIR: &str = ".octocode/skills";
/// Claude Code's skill directory, scanned read-only so `.claude/skills/*`
/// activate without being copied into `.octocode/skills/`.
pub const CLAUDE_SKILLS_DIR: &str = ".claude/skills";

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

    /// Scaffold a user-authored skill from a name, description, and keywords.
    ///
    /// Unlike [`add_from_repair_run`], this produces an empty SKILL.md template
    /// for the user to fill in. The `keywords` become frontmatter triggers, so
    /// the skill auto-activates whenever one appears in the user's input.
    pub fn scaffold(
        &self,
        name: &str,
        description: Option<&str>,
        keywords: &[String],
    ) -> Result<SkillMetadata> {
        self.ensure_dirs()?;
        let id = unique_skill_id(&self.root, name);
        let skill_dir = self.skill_dir(&id)?;
        fs::create_dir_all(skill_dir.join("examples"))
            .with_context(|| format!("create {}", skill_dir.join("examples").display()))?;
        fs::create_dir_all(skill_dir.join("tests"))
            .with_context(|| format!("create {}", skill_dir.join("tests").display()))?;
        let metadata = SkillMetadata {
            id: id.clone(),
            version: 1,
            created_at: Utc::now(),
            created_from_run: None,
            status: SkillStatus::Draft,
            success_count: 0,
            failure_count: 0,
            last_used_at: None,
            tags: keywords.to_vec(),
        };
        crate::storage::atomic::write_json_pretty_atomic(
            &skill_dir.join("metadata.json"),
            &metadata,
        )
        .with_context(|| format!("write {}", skill_dir.join("metadata.json").display()))?;
        crate::storage::atomic::write_text_atomic(
            &skill_dir.join("SKILL.md"),
            &render_authored_skill_markdown(&id, description, keywords),
        )
        .with_context(|| format!("write {}", skill_dir.join("SKILL.md").display()))?;
        append_trace(
            &skill_dir,
            "skill_authored",
            "user-authored draft skill created",
        )?;
        Ok(metadata)
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
        crate::storage::atomic::write_json_pretty_atomic(
            &skill_dir.join("metadata.json"),
            &metadata,
        )
        .with_context(|| format!("write {}", skill_dir.join("metadata.json").display()))?;
        crate::storage::atomic::write_text_atomic(
            &skill_dir.join("SKILL.md"),
            &render_skill_markdown(&metadata, &run),
        )
        .with_context(|| format!("write {}", skill_dir.join("SKILL.md").display()))?;
        append_trace(
            &skill_dir,
            "skill_draft_created",
            &format!("draft skill generated from repair run {}", run.id),
        )?;
        Ok(metadata)
    }

    pub fn list(&self) -> Result<Vec<SkillSummary>> {
        self.ensure_dirs()?;
        let mut summaries: Vec<SkillSummary> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for root in self.read_roots() {
            let entries = match fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let dir = entry.path();
                let metadata_path = dir.join("metadata.json");
                let summary = if metadata_path.exists() {
                    let metadata = self.load_metadata_from_path(&metadata_path)?;
                    SkillSummary {
                        id: metadata.id,
                        status: metadata.status,
                        path: dir.display().to_string(),
                        created_from_run: metadata.created_from_run,
                        tags: metadata.tags,
                    }
                } else if dir.join("SKILL.md").is_file() {
                    // Claude Code skill: no metadata.json, synthesize from the dir.
                    match dir.file_name().and_then(|s| s.to_str()) {
                        Some(id) => SkillSummary {
                            id: id.to_string(),
                            status: SkillStatus::Active,
                            path: dir.display().to_string(),
                            created_from_run: None,
                            tags: Vec::new(),
                        },
                        None => continue,
                    }
                } else {
                    continue;
                };
                // octocode's own root is scanned first and wins on id clashes.
                if seen.insert(summary.id.clone()) {
                    summaries.push(summary);
                }
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
        let path = self.resolve_skill_dir(skill_id)?.join("SKILL.md");
        crate::storage::read_text_file_capped(&path)
            .with_context(|| format!("read {}", path.display()))
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
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for root in self.read_roots() {
            let entries = match fs::read_dir(&root) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
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
                // octocode root wins on id clashes; skip a Claude duplicate.
                if !seen.insert(id.clone()) {
                    continue;
                }
                let raw = match crate::storage::read_text_file_capped(&md_path) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let triggers = effective_triggers(&raw);
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
                        return Ok(hits);
                    }
                }
            }
        }
        Ok(hits)
    }

    pub fn test(&self, skill_id: &str) -> Result<SkillTestReport> {
        let skill_dir = self.resolve_skill_dir(skill_id)?;
        let md_path = skill_dir.join("SKILL.md");
        let mut checks = vec![
            check_exists("SKILL.md", md_path.clone()),
            check_exists("metadata.json", skill_dir.join("metadata.json")),
            check_exists("examples", skill_dir.join("examples")),
            check_exists("tests", skill_dir.join("tests")),
        ];
        if md_path.is_file() {
            let markdown = crate::storage::read_text_file_capped(&md_path).unwrap_or_default();
            checks.extend(frontmatter_checks(&markdown));
        }
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
        crate::storage::atomic::write_json_pretty_atomic(&metadata_path, &metadata)
            .with_context(|| format!("write {}", metadata_path.display()))?;
        append_trace(
            &skill_dir,
            "skill_used",
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
        let data = crate::storage::read_text_file_capped(&path)
            .with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))
    }

    fn load_metadata_from_path(&self, path: &Path) -> Result<SkillMetadata> {
        let data = crate::storage::read_text_file_capped(path)
            .with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&data).with_context(|| format!("parse {}", path.display()))
    }

    fn skill_dir(&self, skill_id: &str) -> Result<PathBuf> {
        validate_skill_id(skill_id)?;
        Ok(self.root.join(skill_id))
    }

    /// Directories scanned when reading/activating skills: octocode's own
    /// (which also receives writes) plus Claude Code's, in that precedence.
    fn read_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.root.clone()];
        let claude = self.project_root.join(CLAUDE_SKILLS_DIR);
        if claude != self.root {
            roots.push(claude);
        }
        roots
    }

    /// Resolve a skill id to its directory across the read roots (octocode
    /// first, then Claude Code). Used by read-only operations.
    fn resolve_skill_dir(&self, skill_id: &str) -> Result<PathBuf> {
        validate_skill_id(skill_id)?;
        for root in self.read_roots() {
            let candidate = root.join(skill_id);
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
        // Fall back to the writable root so error messages point somewhere real.
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
    // Keywords come from the run's tags (dropping `key:value` tags like
    // `status:Passed`), so a repair-generated skill is activatable and passes
    // the same frontmatter checks as an authored one.
    let keyword_list = metadata
        .tags
        .iter()
        .filter(|tag| !tag.contains(':'))
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let frontmatter = format!(
        "---\nname: {}\ndescription: Draft repair skill generated from repair run {}.\nkeywords: [{}]\n---\n\n",
        metadata.id, run.id, keyword_list
    );
    let body = format!(
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
    );
    format!("{frontmatter}{body}")
}

fn render_authored_skill_markdown(
    id: &str,
    description: Option<&str>,
    keywords: &[String],
) -> String {
    let description = description.unwrap_or("Describe what this skill does in one line.");
    let keyword_list = keywords
        .iter()
        .map(|keyword| keyword.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"---
name: {id}
description: {description}
keywords: [{keyword_list}]
---

# {id}

## Purpose

{description}

## When to Use

- Add the situations where this skill should guide the work.

## When Not to Use

- Add the cases where this skill does not apply.

## Steps

1. Outline the first step.
2. Outline the next step.

## Examples

Add a concrete example once you have used this skill.
"#
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

/// Validate that a SKILL.md carries the frontmatter a skill needs to work:
/// a `name`, a `description`, and at least one keyword/trigger (without which
/// the skill never auto-activates on user input).
fn frontmatter_checks(markdown: &str) -> Vec<SkillTestCheck> {
    let frontmatter = extract_frontmatter(markdown);
    let has_name = frontmatter.is_some_and(|block| frontmatter_field(block, "name").is_some());
    let has_description =
        frontmatter.is_some_and(|block| frontmatter_field(block, "description").is_some());
    let has_keywords = !parse_skill_triggers(markdown).is_empty();
    let activates = !effective_triggers(markdown).is_empty();
    vec![
        SkillTestCheck {
            name: "frontmatter".to_string(),
            passed: frontmatter.is_some(),
            message: if frontmatter.is_some() {
                "SKILL.md opens with a --- frontmatter block".to_string()
            } else {
                "add a --- frontmatter block at the top of SKILL.md".to_string()
            },
        },
        SkillTestCheck {
            name: "name".to_string(),
            passed: has_name,
            message: if has_name {
                "name is set".to_string()
            } else {
                "add a `name:` field to the frontmatter".to_string()
            },
        },
        SkillTestCheck {
            name: "description".to_string(),
            passed: has_description,
            message: if has_description {
                "description is set".to_string()
            } else {
                "add a `description:` field to the frontmatter".to_string()
            },
        },
        SkillTestCheck {
            name: "activation".to_string(),
            passed: activates,
            message: if has_keywords {
                "keywords are set, so the skill can auto-activate".to_string()
            } else if activates {
                "no keywords; will auto-activate on the skill name (add `keywords:` for broader matching)".to_string()
            } else {
                "add `keywords:` so the skill auto-activates on matching input".to_string()
            },
        },
    ]
}

/// Triggers used to auto-activate a skill: explicit `keywords:`/`trigger:`
/// frontmatter if present, otherwise terms derived from the skill `name:`.
/// The name fallback lets Claude Code skills (which declare only `name` and
/// `description`) activate without an octocode-specific `keywords:` field.
fn effective_triggers(markdown: &str) -> Vec<String> {
    let explicit = parse_skill_triggers(markdown);
    if !explicit.is_empty() {
        return explicit;
    }
    extract_frontmatter(markdown)
        .and_then(|block| frontmatter_field(block, "name"))
        .map(|name| name_derived_triggers(&name))
        .unwrap_or_default()
}

/// Split a skill name into lowercase trigger terms (length >= 3).
fn name_derived_triggers(name: &str) -> Vec<String> {
    name.split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|term| term.len() >= 3)
        .collect()
}

/// Read a non-empty scalar value for `key` from a frontmatter block.
fn frontmatter_field(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    frontmatter.lines().find_map(|line| {
        let trimmed = line.trim();
        let value = trimmed
            .strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        Some(value.to_string())
    })
}

fn check_exists(name: &str, path: PathBuf) -> SkillTestCheck {
    SkillTestCheck {
        name: name.to_string(),
        passed: path.exists(),
        message: path.display().to_string(),
    }
}

fn append_trace(skill_dir: &Path, event: &str, summary: &str) -> Result<()> {
    let path = skill_dir.join("traces.jsonl");
    crate::storage::atomic::append_jsonl_locked(
        &path,
        &serde_json::to_string(&serde_json::json!({
            "time": Utc::now(),
            "event": event,
            "summary": summary
        }))?,
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
    fn append_trace_uses_given_event_name() {
        let temp = tempfile::tempdir().expect("tempdir");
        append_trace(temp.path(), "skill_used", "used successfully").expect("append trace");

        let data = crate::storage::read_text_file_capped(temp.path().join("traces.jsonl"))
            .expect("read trace");
        let event: serde_json::Value =
            serde_json::from_str(data.trim()).expect("parse trace event");
        assert_eq!(event["event"], "skill_used");
        assert_eq!(event["summary"], "used successfully");
    }

    #[test]
    fn scaffold_creates_user_authored_skill_that_triggers() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().to_path_buf();
        let store = SkillStore::for_project(&project);

        let metadata = store
            .scaffold(
                "Refactor Large Function",
                Some("Split an oversized function into smaller pieces."),
                &["refactor".to_string(), "split".to_string()],
            )
            .expect("scaffold skill");

        assert_eq!(metadata.id, "refactor-large-function");
        assert_eq!(metadata.created_from_run, None);
        assert_eq!(metadata.status, SkillStatus::Draft);

        let hits = store
            .triggered_for_input("please refactor src/foo.rs", 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "refactor-large-function");
    }

    #[test]
    fn test_passes_for_scaffolded_skill_with_keywords() {
        let temp = tempfile::tempdir().unwrap();
        let store = SkillStore::for_project(temp.path());
        let metadata = store
            .scaffold(
                "Tidy Imports",
                Some("Remove unused imports."),
                &["imports".to_string()],
            )
            .expect("scaffold");

        let report = store.test(&metadata.id).expect("test");
        assert_eq!(report.status, SkillTestStatus::Pass);
        assert!(report
            .checks
            .iter()
            .any(|c| c.name == "activation" && c.passed));
    }

    #[test]
    fn claude_style_skill_activates_on_name_without_keywords() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().to_path_buf();
        let skill_dir = project.join(".octocode").join("skills").join("pdf-export");
        std::fs::create_dir_all(skill_dir.join("examples")).unwrap();
        std::fs::create_dir_all(skill_dir.join("tests")).unwrap();
        // A Claude Code skill: name + description, no octocode `keywords:`.
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: pdf-export\ndescription: Export reports to PDF.\n---\nUse the pdf crate.",
        )
        .unwrap();
        std::fs::write(
            skill_dir.join("metadata.json"),
            r#"{"id":"pdf-export","version":1,"created_at":"2026-05-22T00:00:00Z","created_from_run":null,"status":"active","success_count":0,"failure_count":0,"last_used_at":null,"tags":[]}"#,
        )
        .unwrap();

        let store = SkillStore::for_project(&project);
        let hits = store
            .triggered_for_input("can you handle pdf export here", 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "pdf-export");

        let report = store.test("pdf-export").expect("test");
        let activation = report
            .checks
            .iter()
            .find(|c| c.name == "activation")
            .expect("activation check present");
        assert!(activation.passed);
    }

    #[test]
    fn scans_claude_skills_directory() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().to_path_buf();
        // A Claude Code skill living under .claude/skills, not .octocode.
        let claude_skill = project.join(".claude").join("skills").join("changelog");
        std::fs::create_dir_all(&claude_skill).unwrap();
        std::fs::write(
            claude_skill.join("SKILL.md"),
            "---\nname: changelog\ndescription: Maintain the changelog.\n---\nKeep Keep-a-Changelog format.",
        )
        .unwrap();

        let store = SkillStore::for_project(&project);

        // Activates from the Claude directory.
        let hits = store
            .triggered_for_input("update the changelog please", 5)
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "changelog");

        // Appears in list even without an octocode metadata.json.
        let listed = store.list().unwrap();
        assert!(listed.iter().any(|s| s.id == "changelog"));

        // show resolves across roots.
        assert!(store
            .show("changelog")
            .unwrap()
            .contains("Keep-a-Changelog"));
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
