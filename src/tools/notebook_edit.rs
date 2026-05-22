//! `notebook_edit` — modify a single cell in a `.ipynb` notebook.
//!
//! Three modes:
//! - `replace` (default): rewrite the cell's source. Outputs and execution_count
//!   are cleared since the source changed.
//! - `insert`: add a new code cell *after* the target cell.
//! - `delete`: drop the target cell entirely.
//!
//! The target is specified by either the numeric cell index (0-based) or the
//! cell's `id` field (Jupyter 4.5+ format).

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::path::Path;

use crate::tools::notebook_render::{edit_cell, EditMode};

#[derive(Debug, Deserialize)]
pub struct NotebookEditArgs {
    pub path: String,
    /// Either a numeric cell index ("0", "3") or the cell's id ("md-1").
    pub cell: String,
    /// New source. Required for `replace` and `insert`; ignored for `delete`.
    #[serde(default)]
    pub new_source: Option<String>,
    /// One of `replace` (default), `insert`, `delete`.
    #[serde(default)]
    pub edit_mode: Option<String>,
}

pub fn notebook_edit(project_root: &Path, args: NotebookEditArgs) -> Result<String> {
    let resolved = crate::workspace::paths::resolve_workspace_path(project_root, &args.path)
        .ok_or_else(|| anyhow!("path not in workspace: {}", args.path))?;
    if resolved
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        != Some("ipynb".to_string())
    {
        anyhow::bail!(
            "notebook_edit only works on .ipynb files; got {}",
            args.path
        );
    }

    let mode = match args.edit_mode.as_deref().unwrap_or("replace") {
        "replace" => EditMode::Replace,
        "insert" => EditMode::Insert,
        "delete" => EditMode::Delete,
        other => anyhow::bail!("unknown edit_mode `{other}`; expected replace|insert|delete"),
    };

    let new_source = match mode {
        EditMode::Delete => "",
        _ => args
            .new_source
            .as_deref()
            .ok_or_else(|| anyhow!("new_source is required for replace/insert mode"))?,
    };

    edit_cell(&resolved, &args.cell, new_source, mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "cells": [{
            "cell_type": "code",
            "id": "code-1",
            "source": "x = 1",
            "metadata": {},
            "outputs": [],
            "execution_count": null
        }],
        "metadata": {},
        "nbformat": 4,
        "nbformat_minor": 5
    }"#;

    #[test]
    fn rejects_non_ipynb_paths() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("not_a_notebook.txt");
        std::fs::write(&path, "hi").unwrap();
        let r = notebook_edit(
            temp.path(),
            NotebookEditArgs {
                path: "not_a_notebook.txt".to_string(),
                cell: "0".to_string(),
                new_source: Some("x = 2".to_string()),
                edit_mode: None,
            },
        );
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains(".ipynb"));
    }

    #[test]
    fn replace_updates_cell_source() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nb.ipynb");
        std::fs::write(&path, SAMPLE).unwrap();
        notebook_edit(
            temp.path(),
            NotebookEditArgs {
                path: "nb.ipynb".to_string(),
                cell: "code-1".to_string(),
                new_source: Some("x = 2".to_string()),
                edit_mode: Some("replace".to_string()),
            },
        )
        .unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.contains("x = 2"));
    }

    #[test]
    fn delete_mode_does_not_require_new_source() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nb.ipynb");
        std::fs::write(&path, SAMPLE).unwrap();
        notebook_edit(
            temp.path(),
            NotebookEditArgs {
                path: "nb.ipynb".to_string(),
                cell: "0".to_string(),
                new_source: None,
                edit_mode: Some("delete".to_string()),
            },
        )
        .unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        // The cells array should be empty
        let v: serde_json::Value = serde_json::from_str(&after).unwrap();
        assert_eq!(v["cells"].as_array().unwrap().len(), 0);
    }
}
