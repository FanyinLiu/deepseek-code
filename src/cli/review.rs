use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::agent::orchestrator::AgentEvent;
use crate::agent::subagent::{SubagentConfig, SubagentTask, SubagentType};
use crate::agent::supervisor::Supervisor;
use crate::provider::{build_provider, Provider};
use crate::storage;

/// Run a full multi-agent code review over the project.
///
/// Scans all `.rs` files, groups them by top-level module, and spawns
/// parallel `code-reviewer` subagents — one per module. Each reviewer
/// reads the files in its group and produces a detailed line-by-line
/// audit.
pub async fn review(
    project_root: Option<PathBuf>,
    max_parallel: usize,
    max_turns: u32,
    output: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));
    let config = storage::Config::load(Some(&root))?;
    let api_key = super::login::resolve_or_prompt_api_key(Some(&root))?;
    let provider = build_provider(&config.provider, api_key);
    let client = Arc::new(provider.create_deepseek_client());

    // 1. Scan all .rs files grouped by top-level module
    let groups = scan_modules(&root)?;
    let total_files: usize = groups.values().map(std::vec::Vec::len).sum();

    println!(
        "🔍 Launching multi-agent code review: {} modules, {} files, parallelism={}",
        groups.len(),
        total_files,
        max_parallel
    );

    // 2. Build review tasks — one per module
    let mut tasks = Vec::with_capacity(groups.len());
    for (module_name, files) in groups {
        let file_list = files.join("\n");
        let config = {
            let mut c = SubagentConfig::for_type(SubagentType::CodeReviewer);
            c.max_turns = max_turns;
            c
        };
        let task = SubagentTask {
            description: format!("Review module: {module_name}"),
            prompt: build_review_prompt(&module_name, &file_list),
            context: None,
            focus_files: files,
            expected_output: Some("Detailed code review report".to_string()),
        };
        tasks.push((config, task));
    }

    // 3. Run reviews in parallel batches (respect max_parallel)
    let supervisor = Supervisor::new(client.clone(), root.clone());
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    let mut all_results = Vec::new();
    for chunk in tasks.chunks(max_parallel) {
        let batch = crate::agent::subagent::ParallelBatch {
            tasks: chunk.to_vec(),
            independent: true,
            merge_strategy: crate::agent::subagent::MergeStrategy::Concatenate,
            shared_context: None,
        };

        println!("▶️  Starting {} parallel reviewers...", chunk.len());
        let results = supervisor.run_parallel(batch, &event_tx).await;
        all_results.extend(results);

        // Drain progress events
        while let Ok(event) = event_rx.try_recv() {
            match event {
                AgentEvent::SubagentStarted {
                    agent_id,
                    agent_type,
                    description,
                    ..
                } => {
                    println!("  [{agent_id}] {agent_type} — {description}");
                }
                AgentEvent::SubagentCompleted { agent_id, result } => {
                    let icon = if result.success { "✅" } else { "❌" };
                    println!(
                        "  {} [{}] completed ({}ms, {} tokens)",
                        icon, agent_id, result.duration_ms, result.token_usage
                    );
                }
                _ => {}
            }
        }
    }

    // 4. Synthesize final report
    let report = synthesize_report(&all_results, &root);

    if let Some(out_path) = output {
        std::fs::write(&out_path, &report)?;
        println!("\n📄 Review report saved to: {}", out_path.display());
    } else {
        println!("\n{report}");
    }

    Ok(())
}

// ------------------------------------------------------------------
// File scanning
// ------------------------------------------------------------------

fn scan_modules(root: &Path) -> Result<BTreeMap<String, Vec<String>>, anyhow::Error> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for entry in walkdir::WalkDir::new(root.join("src"))
        .into_iter()
        .filter_map(std::result::Result::ok)
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }

        // Get relative path from project root
        let relative = path.strip_prefix(root).unwrap_or(path);
        let relative_str = relative.to_string_lossy().replace('\\', "/");

        // Determine top-level module from path like src/agent/orchestrator.rs -> "agent"
        let parts: Vec<_> = relative_str.split('/').collect();
        let module = if parts.len() >= 2 {
            parts[1].to_string() // e.g. "agent" from "src/agent/..."
        } else {
            "root".to_string()
        };

        groups.entry(module).or_default().push(relative_str);
    }

    Ok(groups)
}

// ------------------------------------------------------------------
// Prompt construction
// ------------------------------------------------------------------

fn build_review_prompt(module_name: &str, file_list: &str) -> String {
    format!(
        r"You are a senior Rust code reviewer performing a **line-by-line audit** of the `{module_name}` module.

## Files to review
{file_list}

## Instructions
1. Read EACH file using `read_file`.
2. Perform a **rigorous line-by-line review** for every file.
3. Check for:
   - 🐛 **Bugs & Logic Errors** — incorrect assumptions, off-by-one, race conditions
   - 🔒 **Security Issues** — unsafe blocks, panic paths, credential leaks, injection risks
   - ⚡ **Performance** — unnecessary allocations, inefficient loops, blocking in async
   - 🦀 **Rust Idioms** — proper error handling (? vs unwrap), ownership, lifetimes, traits
   - 📐 **Code Quality** — readability, naming, duplication, module boundaries
   - 🧪 **Test Coverage** — untested branches, missing edge cases
4. For **each issue found**, provide:
   - File path and line number (if possible)
   - Severity: Critical / Warning / Nitpick
   - Description of the problem
   - Suggested fix (with code if applicable)
5. If a file looks good, say so explicitly.
6. Produce a structured markdown report.

## Output Format
```markdown
# Review Report: {module_name}

## Summary
- Files reviewed: N
- Issues found: N critical, N warnings, N nits
- Overall verdict: ✅ Clean / ⚠️ Needs work / ❌ Significant issues

## Per-File Findings

### `src/.../file.rs`
#### Line ~X — [Severity] Title
Description...
```suggestion
// suggested code
```

## Cross-Cutting Concerns
(architecture, consistency across files)
```
"
    )
}

// ------------------------------------------------------------------
// Report synthesis
// ------------------------------------------------------------------

fn synthesize_report(results: &[crate::agent::subagent::SubagentResult], root: &Path) -> String {
    let mut report = String::new();
    report.push_str("# 🔍 Multi-Agent Code Review Report\n\n");
    report.push_str(&format!("**Project:** `{}`\n\n", root.display()));

    let total = results.len();
    let success = results.iter().filter(|r| r.success).count();
    let failed = total - success;

    report.push_str(&format!("**Modules reviewed:** {total}\n"));
    report.push_str(&format!(
        "**Successful:** {success} | **Failed:** {failed}\n\n"
    ));
    report.push_str("---\n\n");

    for (i, result) in results.iter().enumerate() {
        report.push_str(&format!("## Reviewer {}\n\n", i + 1));
        if result.success {
            report.push_str(&result.output);
        } else {
            report.push_str(&format!(
                "❌ **Review failed:** {}\n\n{}",
                result.error.as_deref().unwrap_or("unknown error"),
                result.output
            ));
        }
        report.push_str("\n\n---\n\n");
    }

    report
}
