use std::path::{Path, PathBuf};
use std::process::Command;

use super::files::{MatchType, SearchMatch};

/// Search code content using ripgrep (rg) as a subprocess.
/// Falls back to basic line-by-line search if rg is unavailable.
pub fn search_code(
    project_root: &Path,
    pattern: &str,
    glob: Option<&str>,
    case_sensitive: bool,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    // Try ripgrep first — trust its results even when empty
    if let Ok(results) = try_ripgrep(project_root, pattern, glob, case_sensitive, limit) {
        return Ok(results);
    }

    // Fall back to simple search
    fallback_search(project_root, pattern, glob, case_sensitive, limit)
}

fn try_ripgrep(
    project_root: &Path,
    pattern: &str,
    glob: Option<&str>,
    case_sensitive: bool,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    let mut cmd = Command::new("rg");
    cmd.args(["--no-heading", "--with-filename", "--line-number"])
        .arg("--max-count")
        .arg(limit.to_string());

    if !case_sensitive {
        cmd.arg("--ignore-case");
    }

    if let Some(g) = glob {
        cmd.arg("--glob").arg(g);
    }

    cmd.arg("--").arg(pattern).current_dir(project_root);

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut results = Vec::new();
    for line in stdout.lines() {
        if results.len() >= limit {
            break;
        }
        // rg output format: path:line_number:content
        if let Some((path_part, rest)) = line.split_once(':') {
            if let Some((line_num_str, content)) = rest.split_once(':') {
                if let Ok(line_num) = line_num_str.parse::<usize>() {
                    results.push(SearchMatch {
                        path: PathBuf::from(path_part),
                        line_number: Some(line_num),
                        matched_text: content.to_string(),
                        match_type: MatchType::CodeLine,
                    });
                }
            }
        }
    }

    Ok(results)
}

fn fallback_search(
    project_root: &Path,
    pattern: &str,
    glob: Option<&str>,
    case_sensitive: bool,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    let mut results = Vec::new();

    let pattern_lower = if case_sensitive {
        None
    } else {
        Some(pattern.to_lowercase())
    };

    for entry in ignore::Walk::new(project_root) {
        let entry = entry?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }

        let path = entry.path();
        let relative = path.strip_prefix(project_root).unwrap_or(path);

        // Apply glob filter
        if let Some(g) = glob {
            let path_str = relative.to_string_lossy();
            if !simple_glob(g, &path_str, case_sensitive) {
                continue;
            }
        }

        // Skip binary and large files
        if is_binary_hint(path) {
            continue;
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB
            if metadata.len() > MAX_FILE_SIZE {
                continue;
            }
        }

        if let Ok(content) = std::fs::read_to_string(path) {
            for (line_num, line) in content.lines().enumerate() {
                let matches = match &pattern_lower {
                    Some(pl) => line.to_lowercase().contains(pl.as_str()),
                    None => line.contains(pattern),
                };

                if matches {
                    results.push(SearchMatch {
                        path: relative.to_path_buf(),
                        line_number: Some(line_num + 1),
                        matched_text: line.to_string(),
                        match_type: MatchType::CodeLine,
                    });

                    if results.len() >= limit {
                        return Ok(results);
                    }
                }
            }
        }
    }

    Ok(results)
}

fn simple_glob(pattern: &str, path: &str, case_sensitive: bool) -> bool {
    if !case_sensitive {
        let path_lower = path.to_lowercase();
        let pattern_lower = pattern.to_lowercase();
        return simple_glob_impl(&pattern_lower, &path_lower);
    }
    simple_glob_impl(pattern, path)
}

fn simple_glob_impl(pattern: &str, path: &str) -> bool {
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut remaining = path;
        for part in parts {
            match remaining.find(part) {
                Some(pos) => remaining = &remaining[pos + part.len()..],
                None => return false,
            }
        }
        true
    } else {
        path.contains(pattern)
    }
}

const BINARY_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "svg", "woff", "woff2", "ttf", "eot", "pdf", "zip", "gz",
    "tar", "o", "so", "dylib", "dll", "exe", "bin", "class", "pyc", "wasm",
];

fn is_binary_hint(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        return BINARY_EXTS.iter().any(|&b| ext.eq_ignore_ascii_case(b));
    }
    false
}
