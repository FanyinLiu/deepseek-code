use std::path::Path;

const MAX_LIST_DIR_ENTRIES: usize = 2_000;

/// List files and directories in a readable path.
pub fn list_dir(project_root: &Path, path: &str, recursive: bool) -> Result<String, anyhow::Error> {
    let dir = crate::workspace::paths::resolve_read_path(project_root, path)
        .ok_or_else(|| anyhow::anyhow!("path not found or unreadable: {path}"))?;

    let canonical_root =
        std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let mut out = String::new();

    if recursive {
        let mut entries = 0usize;
        let mut truncated = false;
        for entry in walkdir::WalkDir::new(&dir).max_depth(5) {
            let entry = entry?;
            if entry.path() == dir {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&canonical_root)
                .unwrap_or(entry.path());
            out.push_str(&format!("{}\n", relative.display()));
            entries += 1;
            if entries >= MAX_LIST_DIR_ENTRIES {
                truncated = true;
                break;
            }
        }
        if truncated {
            out.push_str(&format!(
                "... truncated after {MAX_LIST_DIR_ENTRIES} entries\n"
            ));
        }
    } else {
        let mut entries = 0usize;
        let mut truncated = false;
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let entry_path = entry.path().clone();
            let relative = entry_path
                .strip_prefix(&canonical_root)
                .unwrap_or(&entry_path);
            let file_type = if entry.file_type()?.is_dir() { "/" } else { "" };
            out.push_str(&format!("{}{}\n", relative.display(), file_type));
            entries += 1;
            if entries >= MAX_LIST_DIR_ENTRIES {
                truncated = true;
                break;
            }
        }
        if truncated {
            out.push_str(&format!(
                "... truncated after {MAX_LIST_DIR_ENTRIES} entries\n"
            ));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursive_listing_skips_root_entry() {
        let root = tempfile::tempdir().expect("workspace");
        std::fs::create_dir(root.path().join("src")).expect("create src");
        std::fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").expect("write lib");

        let output = list_dir(root.path(), ".", true).expect("list dir");

        assert!(!output.starts_with('\n'), "{output:?}");
        assert!(output.contains("src\n") || output.contains("src/\n"));
        assert!(output.contains("src/lib.rs\n"));
    }

    #[test]
    fn non_recursive_listing_is_entry_capped() {
        let root = tempfile::tempdir().expect("workspace");
        for index in 0..=MAX_LIST_DIR_ENTRIES {
            std::fs::write(root.path().join(format!("{index:04}.txt")), "x").expect("write file");
        }

        let output = list_dir(root.path(), ".", false).expect("list dir");

        assert!(output.contains(&format!(
            "... truncated after {MAX_LIST_DIR_ENTRIES} entries"
        )));
        assert_eq!(output.lines().count(), MAX_LIST_DIR_ENTRIES + 1);
    }
}
