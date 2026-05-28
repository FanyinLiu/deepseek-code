//! Render `.ipynb` notebook files as plain text for `read_file`.
//!
//! Notebooks are JSON. Reading them via `read_to_string` produces dense
//! escape-soup that's useless to the model. This extractor pulls out:
//!
//! - one block per cell (numbered, with cell_type)
//! - the source (joined if it's the JSON list form)
//! - text outputs (stdout / stderr / `text/plain`)
//!
//! Image outputs are summarised as `[image/<mime>: <N> bytes (omitted)]`
//! rather than dumped as base64.

use std::path::Path;

use serde::Deserialize;

const TEXT_MIME_PRIORITY: &[&str] = &["text/plain", "text/markdown"];

pub fn render(
    path: &Path,
    display_path: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<String, anyhow::Error> {
    let raw = crate::storage::read_text_file_capped(path)?;
    let notebook: Notebook =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("invalid notebook JSON: {e}"))?;

    let total_cells = notebook.cells.len();
    let start = offset.unwrap_or(0).min(total_cells);
    let end = limit.map_or(total_cells, |l| (start + l).min(total_cells));

    let mut out = format!(
        "Notebook: {display_path}\nKernel: {}\n{} cells total\n\n",
        notebook
            .metadata
            .kernelspec
            .as_ref()
            .map(|k| k.name.as_str())
            .unwrap_or("<unknown>"),
        total_cells
    );

    for (i, cell) in notebook
        .cells
        .iter()
        .enumerate()
        .skip(start)
        .take(end - start)
    {
        let kind = cell.cell_type.as_deref().unwrap_or("unknown");
        let exec_count = cell
            .execution_count
            .map(|n| format!(" [{n}]"))
            .unwrap_or_default();
        out.push_str(&format!("─── cell {i} ({kind}){exec_count} ───\n"));

        let source = join_source(&cell.source);
        if source.trim().is_empty() {
            out.push_str("(empty)\n");
        } else {
            out.push_str(&source);
            if !source.ends_with('\n') {
                out.push('\n');
            }
        }

        for output in &cell.outputs {
            render_output(output, &mut out);
        }
        out.push('\n');
    }

    if end < total_cells {
        out.push_str(&format!(
            "\n... ({} more cells, {} total). Use offset={} limit=... to continue.\n",
            total_cells - end,
            total_cells,
            end
        ));
    }

    Ok(out)
}

fn join_source(source: &SourceField) -> String {
    match source {
        SourceField::String(s) => s.clone(),
        SourceField::Lines(v) => v.join(""),
    }
}

fn render_output(output: &CellOutput, out: &mut String) {
    match output.output_type.as_deref() {
        Some("stream") => {
            let name = output.name.as_deref().unwrap_or("stream");
            let text = output.text.as_ref().map(join_source).unwrap_or_default();
            out.push_str(&format!("[{name}]\n{text}"));
            if !text.ends_with('\n') {
                out.push('\n');
            }
        }
        Some("error") => {
            let ename = output.ename.as_deref().unwrap_or("error");
            let evalue = output.evalue.as_deref().unwrap_or("");
            out.push_str(&format!("[error] {ename}: {evalue}\n"));
            for line in &output.traceback {
                out.push_str(&format!("  {line}\n"));
            }
        }
        Some("execute_result") | Some("display_data") => {
            if let Some(data) = output.data.as_ref() {
                // Prefer plain text; fall back to any text/* mime; summarise images.
                let text = TEXT_MIME_PRIORITY
                    .iter()
                    .find_map(|mime| {
                        data.get(*mime)
                            .and_then(|v| serde_json::from_value::<SourceField>(v.clone()).ok())
                            .map(|s| join_source(&s))
                    })
                    .or_else(|| {
                        data.iter().find_map(|(mime, value)| {
                            mime.starts_with("text/").then(|| {
                                serde_json::from_value::<SourceField>(value.clone())
                                    .map(|s| join_source(&s))
                                    .unwrap_or_default()
                            })
                        })
                    });
                if let Some(text) = text {
                    out.push_str("[result]\n");
                    out.push_str(&text);
                    if !text.ends_with('\n') {
                        out.push('\n');
                    }
                }
                for (mime, value) in data {
                    if mime.starts_with("image/") {
                        let approx_bytes = value.as_str().map(|s| s.len() * 3 / 4).unwrap_or(0);
                        out.push_str(&format!(
                            "[{mime}: ~{approx_bytes} bytes (omitted; use a viewer)]\n"
                        ));
                    }
                }
            }
        }
        _ => {}
    }
}

#[derive(Debug, Deserialize)]
struct Notebook {
    #[serde(default)]
    cells: Vec<NotebookCell>,
    #[serde(default)]
    metadata: NotebookMetadata,
}

#[derive(Debug, Default, Deserialize)]
struct NotebookMetadata {
    #[serde(default)]
    kernelspec: Option<KernelSpec>,
}

#[derive(Debug, Deserialize)]
struct KernelSpec {
    name: String,
}

#[derive(Debug, Deserialize)]
struct NotebookCell {
    cell_type: Option<String>,
    #[serde(default)]
    execution_count: Option<u64>,
    #[serde(default = "default_source")]
    source: SourceField,
    #[serde(default)]
    outputs: Vec<CellOutput>,
}

#[derive(Debug, Deserialize)]
struct CellOutput {
    output_type: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    text: Option<SourceField>,
    #[serde(default)]
    data: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    ename: Option<String>,
    #[serde(default)]
    evalue: Option<String>,
    #[serde(default)]
    traceback: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SourceField {
    String(String),
    Lines(Vec<String>),
}

fn default_source() -> SourceField {
    SourceField::String(String::new())
}

/// Apply an in-place edit to a notebook cell. Used by the
/// `notebook_edit` tool.
pub fn edit_cell(
    path: &Path,
    cell_id_or_index: &str,
    new_source: &str,
    edit_mode: EditMode,
) -> Result<String, anyhow::Error> {
    let raw = crate::storage::read_text_file_capped(path)?;
    let mut notebook: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("invalid notebook JSON: {e}"))?;

    let cells = notebook
        .get_mut("cells")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| anyhow::anyhow!("notebook has no cells array"))?;

    // Try as index first, then as cell_id (Jupyter format 4.5+).
    let idx = if let Ok(n) = cell_id_or_index.parse::<usize>() {
        if n >= cells.len() {
            anyhow::bail!(
                "cell index out of range: {n} (notebook has {} cells)",
                cells.len()
            );
        }
        n
    } else {
        cells
            .iter()
            .position(|cell| {
                cell.get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s == cell_id_or_index)
                    .unwrap_or(false)
            })
            .ok_or_else(|| anyhow::anyhow!("no cell with id `{cell_id_or_index}`"))?
    };

    match edit_mode {
        EditMode::Replace => {
            // Always store as a single string for portability.
            cells[idx]["source"] = serde_json::Value::String(new_source.to_string());
            // Reset outputs since the source changed.
            if cells[idx].get("cell_type").and_then(|v| v.as_str()) == Some("code") {
                cells[idx]["outputs"] = serde_json::Value::Array(Vec::new());
                cells[idx]["execution_count"] = serde_json::Value::Null;
            }
        }
        EditMode::Insert => {
            let mut new_cell = serde_json::json!({
                "cell_type": "code",
                "source": new_source,
                "metadata": {},
                "outputs": [],
                "execution_count": null,
            });
            new_cell["id"] =
                serde_json::Value::String(format!("octo-insert-{}", uuid::Uuid::new_v4()));
            cells.insert(idx + 1, new_cell);
        }
        EditMode::Delete => {
            cells.remove(idx);
        }
    }

    let pretty = serde_json::to_string_pretty(&notebook)?;
    std::fs::write(path, &pretty)?;
    Ok(format!(
        "Notebook updated: {} (mode={:?}, target cell={cell_id_or_index})",
        path.display(),
        edit_mode
    ))
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditMode {
    Replace,
    Insert,
    Delete,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r##"{
        "cells": [
            {
                "cell_type": "markdown",
                "id": "md-1",
                "source": ["# Title\n", "Some prose."],
                "metadata": {}
            },
            {
                "cell_type": "code",
                "id": "code-1",
                "execution_count": 3,
                "source": "print('hi')",
                "outputs": [
                    {"output_type": "stream", "name": "stdout", "text": "hi\n"}
                ],
                "metadata": {}
            }
        ],
        "metadata": {"kernelspec": {"name": "python3"}},
        "nbformat": 4,
        "nbformat_minor": 5
    }"##;

    #[test]
    fn renders_cells_and_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nb.ipynb");
        std::fs::write(&path, SAMPLE).unwrap();
        let out = render(&path, "nb.ipynb", None, None).unwrap();
        assert!(out.contains("2 cells total"));
        assert!(out.contains("Kernel: python3"));
        assert!(out.contains("cell 0 (markdown)"));
        assert!(out.contains("# Title"));
        assert!(out.contains("cell 1 (code) [3]"));
        assert!(out.contains("print('hi')"));
        assert!(out.contains("[stdout]"));
        assert!(out.contains("hi"));
    }

    #[test]
    fn offset_limit_skips_cells() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nb.ipynb");
        std::fs::write(&path, SAMPLE).unwrap();
        let out = render(&path, "nb.ipynb", Some(1), Some(1)).unwrap();
        assert!(!out.contains("# Title"));
        assert!(out.contains("print('hi')"));
    }

    #[test]
    fn edit_replace_clears_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nb.ipynb");
        std::fs::write(&path, SAMPLE).unwrap();
        edit_cell(&path, "code-1", "print('bye')", EditMode::Replace).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("print('bye')"));
        // execution_count reset
        assert!(after.contains("\"execution_count\": null"));
    }

    #[test]
    fn edit_delete_removes_cell() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nb.ipynb");
        std::fs::write(&path, SAMPLE).unwrap();
        edit_cell(&path, "md-1", "", EditMode::Delete).unwrap();
        let out = render(&path, "nb.ipynb", None, None).unwrap();
        assert!(out.contains("1 cells total"));
        assert!(!out.contains("# Title"));
    }

    #[test]
    fn edit_rejects_large_notebook_before_reading() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.ipynb");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(crate::storage::TEXT_FILE_SIZE_CAP + 1)
            .unwrap();

        let error = edit_cell(&path, "0", "print('nope')", EditMode::Replace)
            .unwrap_err()
            .to_string();

        assert_eq!(
            error,
            format!(
                "file too large: {} bytes, cap {}",
                crate::storage::TEXT_FILE_SIZE_CAP + 1,
                crate::storage::TEXT_FILE_SIZE_CAP
            )
        );
    }
}
