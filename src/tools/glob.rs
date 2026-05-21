//! Glob — file path matching across the workspace.
//!
//! Use this when the model needs to find files by path/name pattern.
//! For content search see `grep.rs`.

use anyhow::{anyhow, Context, Result};
use glob::Pattern;
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, serde::Deserialize)]
pub struct GlobArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub limit: Option<usize>,
}

pub fn glob(project_root: &Path, args: GlobArgs) -> Result<String> {
    let root: PathBuf = match args.path.as_deref() {
        Some(p) => crate::workspace::paths::resolve_read_path(project_root, p)
            .ok_or_else(|| anyhow!("search path not found or unreadable: {p}"))?,
        None => project_root.to_path_buf(),
    };

    let pattern =
        Pattern::new(&args.pattern).with_context(|| format!("invalid glob: {}", args.pattern))?;
    let limit = args.limit.unwrap_or(200).min(2000);

    let mut matches: Vec<(PathBuf, SystemTime)> = Vec::new();

    for entry in WalkBuilder::new(&root).hidden(false).build().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = path.strip_prefix(&root).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if pattern.matches(&rel_str) {
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            matches.push((path.to_path_buf(), mtime));
        }
    }

    matches.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    let total = matches.len();
    let truncated = total > limit;
    matches.truncate(limit);

    if matches.is_empty() {
        return Ok(format!("No files match pattern: {}", args.pattern));
    }

    let mut out = format!(
        "Glob: {} (root: {})\n{} match{}{}\n\n",
        args.pattern,
        crate::workspace::paths::display_path(project_root, &root),
        total,
        if total == 1 { "" } else { "es" },
        if truncated {
            format!(", showing first {limit}")
        } else {
            String::new()
        },
    );
    for (p, _) in &matches {
        out.push_str(&crate::workspace::paths::display_path(project_root, p));
        out.push('\n');
    }
    Ok(out)
}
