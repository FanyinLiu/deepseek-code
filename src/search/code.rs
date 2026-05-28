use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::{cmp, io::Read};

use super::files::{MatchType, SearchMatch};

const MAX_SEARCH_RESULTS: usize = 2000;
const MAX_SEARCH_FILE_BYTES: u64 = 10 * 1024 * 1024;
const MAX_MATCH_TEXT_CHARS: usize = 4096;
const MAX_RIPGREP_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const TRUNCATED_MATCH_MARKER: &str = " ... [line truncated]";

/// Search code content using ripgrep (rg) as a subprocess.
/// Falls back to basic line-by-line search if rg is unavailable.
pub fn search_code(
    project_root: &Path,
    pattern: &str,
    glob: Option<&str>,
    case_sensitive: bool,
    limit: usize,
) -> Result<Vec<SearchMatch>, anyhow::Error> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let limit = limit.min(MAX_SEARCH_RESULTS);

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
        .args([
            "--max-filesize",
            "10M",
            "--max-columns",
            "4096",
            "--max-columns-preview",
        ])
        .arg("--max-count")
        .arg(limit.to_string());

    if !case_sensitive {
        cmd.arg("--ignore-case");
    }

    if let Some(g) = glob {
        cmd.arg("--glob").arg(g);
    }

    cmd.arg("--").arg(pattern).current_dir(project_root);

    let output = run_limited_command(cmd, MAX_RIPGREP_OUTPUT_BYTES)?;
    if output.stdout.is_empty() && !output.truncated && !is_ripgrep_search_status(output.status) {
        return Err(anyhow::anyhow!("ripgrep failed"));
    }
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
                        matched_text: truncate_match_text(content),
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
            if metadata.len() > MAX_SEARCH_FILE_BYTES {
                continue;
            }
        }

        if let Ok(content) = crate::storage::read_text_file_capped(path) {
            for (line_num, line) in content.lines().enumerate() {
                let matches = match &pattern_lower {
                    Some(pl) => line.to_lowercase().contains(pl.as_str()),
                    None => line.contains(pattern),
                };

                if matches {
                    results.push(SearchMatch {
                        path: relative.to_path_buf(),
                        line_number: Some(line_num + 1),
                        matched_text: truncate_match_text(line),
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

struct LimitedCommandOutput {
    stdout: Vec<u8>,
    status: ExitStatus,
    truncated: bool,
}

fn run_limited_command(
    mut command: Command,
    max_stdout_bytes: usize,
) -> Result<LimitedCommandOutput, anyhow::Error> {
    command.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to capture ripgrep stdout"))?;
    let mut collected = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = stdout.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        let remaining = max_stdout_bytes.saturating_sub(collected.len());
        let keep = cmp::min(bytes_read, remaining);
        if keep > 0 {
            collected.extend_from_slice(&buffer[..keep]);
        }
        if keep < bytes_read || collected.len() >= max_stdout_bytes {
            truncated = true;
            let _ = child.kill();
            break;
        }
    }

    let status = child.wait()?;
    Ok(LimitedCommandOutput {
        stdout: collected,
        status,
        truncated,
    })
}

fn is_ripgrep_search_status(status: ExitStatus) -> bool {
    matches!(status.code(), Some(0 | 1))
}

fn truncate_match_text(text: &str) -> String {
    let mut chars = text.chars();
    let mut out = chars
        .by_ref()
        .take(MAX_MATCH_TEXT_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        out.push_str(TRUNCATED_MATCH_MARKER);
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_match_text_is_char_safe() {
        let text = format!("{}{}", "界".repeat(MAX_MATCH_TEXT_CHARS), "尾");
        let truncated = truncate_match_text(&text);

        assert!(truncated.contains(TRUNCATED_MATCH_MARKER));
        assert!(truncated.starts_with('界'));
    }

    #[test]
    fn fallback_search_truncates_long_match_text() {
        let temp = tempfile::tempdir().expect("tempdir");
        let line = format!("needle {}", "x".repeat(MAX_MATCH_TEXT_CHARS + 100));
        std::fs::write(temp.path().join("main.rs"), line).expect("write main");

        let matches =
            fallback_search(temp.path(), "needle", None, true, 1).expect("fallback search");

        assert_eq!(matches.len(), 1);
        assert!(matches[0].matched_text.contains(TRUNCATED_MATCH_MARKER));
        assert!(matches[0].matched_text.len() < MAX_MATCH_TEXT_CHARS + 128);
    }

    #[test]
    fn run_limited_command_caps_stdout() {
        #[cfg(windows)]
        let command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "for /L %i in (1,1,4000) do @echo xxxxxxxxxx"]);
            command
        };
        #[cfg(not(windows))]
        let command = {
            let mut command = Command::new("sh");
            command.args(["-c", "yes xxxxxxxxxx | head -c 20000"]);
            command
        };

        let output = run_limited_command(command, 1024).expect("limited command");

        assert_eq!(output.stdout.len(), 1024);
        assert!(output.truncated);
    }
}
