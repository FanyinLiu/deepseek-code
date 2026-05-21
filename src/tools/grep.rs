//! Grep — regex content search across the workspace.
//!
//! Three output modes:
//!   - FilesWithMatches: just paths (cheapest, default)
//!   - Content:          matching lines with optional before/after context
//!   - Count:            per-file match counts

use anyhow::{anyhow, Context, Result};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use serde::Deserialize;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Copy, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    #[default]
    FilesWithMatches,
    Content,
    Count,
}

#[derive(Debug, Deserialize)]
pub struct GrepArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub glob: Option<String>,
    #[serde(default)]
    pub output_mode: OutputMode,
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default)]
    pub multiline: bool,
    pub before: Option<usize>,
    pub after: Option<usize>,
    pub head_limit: Option<usize>,
    #[serde(default)]
    pub show_line_numbers: bool,
}

pub fn grep(project_root: &Path, args: GrepArgs) -> Result<String> {
    let root = match args.path.as_deref() {
        Some(p) => crate::workspace::paths::resolve_read_path(project_root, p)
            .ok_or_else(|| anyhow!("search path not found or unreadable: {p}"))?,
        None => project_root.to_path_buf(),
    };

    let regex = RegexBuilder::new(&args.pattern)
        .case_insensitive(args.case_insensitive)
        .multi_line(args.multiline)
        .dot_matches_new_line(args.multiline)
        .build()
        .with_context(|| format!("invalid regex: {}", args.pattern))?;

    let path_filter = args
        .glob
        .as_deref()
        .map(glob::Pattern::new)
        .transpose()
        .with_context(|| "invalid glob filter")?;

    let head_limit = args.head_limit.unwrap_or(250);
    let before = args.before.unwrap_or(0);
    let after = args.after.unwrap_or(0);

    let mut file_hits: Vec<FileHit> = Vec::new();
    let mut emitted = 0usize;
    let mut truncated = false;

    'walk: for entry in WalkBuilder::new(&root).hidden(false).build().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(pat) = &path_filter {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();
            if !pat.matches(&name) {
                continue;
            }
        }

        let Ok(f) = File::open(path) else { continue };
        let reader = BufReader::new(f);
        let mut hit = FileHit {
            path: path.to_path_buf(),
            matches: Vec::new(),
            count: 0,
        };

        let mut prev: VecDeque<(usize, String)> = VecDeque::with_capacity(before);
        let mut pending_after: usize = 0;
        let mut staged_context: Vec<(usize, String, bool)> = Vec::new();

        for (i, line) in reader.lines().enumerate() {
            let Ok(line) = line else { break };
            let lineno = i + 1;
            let is_match = regex.is_match(&line);

            if is_match {
                hit.count += 1;
                match args.output_mode {
                    OutputMode::FilesWithMatches => break,
                    OutputMode::Count => {}
                    OutputMode::Content => {
                        for (n, l) in prev.drain(..) {
                            staged_context.push((n, l, false));
                        }
                        staged_context.push((lineno, line.clone(), true));
                        pending_after = after;
                    }
                }
            } else if pending_after > 0 {
                staged_context.push((lineno, line.clone(), false));
                pending_after -= 1;
            } else if before > 0 {
                if prev.len() == before {
                    prev.pop_front();
                }
                prev.push_back((lineno, line));
            }
        }

        if !staged_context.is_empty() {
            hit.matches = staged_context;
        }

        let counted = match args.output_mode {
            OutputMode::FilesWithMatches => hit.count > 0,
            OutputMode::Count => hit.count > 0,
            OutputMode::Content => !hit.matches.is_empty(),
        };

        if counted {
            file_hits.push(hit);
            emitted += 1;
            if emitted >= head_limit {
                truncated = true;
                break 'walk;
            }
        }
    }

    Ok(format_output(
        project_root,
        &args,
        &file_hits,
        truncated,
        head_limit,
    ))
}

struct FileHit {
    path: std::path::PathBuf,
    matches: Vec<(usize, String, bool)>,
    count: usize,
}

fn format_output(
    project_root: &Path,
    args: &GrepArgs,
    hits: &[FileHit],
    truncated: bool,
    head_limit: usize,
) -> String {
    if hits.is_empty() {
        return format!("No matches for /{}/", args.pattern);
    }
    let mut out = String::new();
    match args.output_mode {
        OutputMode::FilesWithMatches => {
            out.push_str(&format!(
                "{} file(s) matched /{}/:\n",
                hits.len(),
                args.pattern
            ));
            for h in hits {
                out.push_str(&crate::workspace::paths::display_path(
                    project_root,
                    &h.path,
                ));
                out.push('\n');
            }
        }
        OutputMode::Count => {
            out.push_str(&format!("Match counts for /{}/:\n", args.pattern));
            for h in hits {
                out.push_str(&format!(
                    "{}: {}\n",
                    crate::workspace::paths::display_path(project_root, &h.path),
                    h.count
                ));
            }
        }
        OutputMode::Content => {
            out.push_str(&format!(
                "Matches for /{}/ ({} file{}):\n",
                args.pattern,
                hits.len(),
                if hits.len() == 1 { "" } else { "s" }
            ));
            for h in hits {
                out.push_str(&format!(
                    "\n--- {} ---\n",
                    crate::workspace::paths::display_path(project_root, &h.path)
                ));
                for (lineno, text, is_match) in &h.matches {
                    let marker = if *is_match { ">" } else { " " };
                    if args.show_line_numbers {
                        out.push_str(&format!("{marker} {lineno:>6} | {text}\n"));
                    } else {
                        out.push_str(&format!("{marker} {text}\n"));
                    }
                }
            }
        }
    }
    if truncated {
        out.push_str(&format!(
            "\n... truncated at {head_limit} files; refine pattern or path for more.\n"
        ));
    }
    out
}
