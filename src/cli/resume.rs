use std::path::PathBuf;

use crate::storage::{self, SessionStore, TranscriptFormat};

/// Run the resume command: list and restore saved sessions.
pub async fn resume(
    session_name: Option<String>,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;

    let store = SessionStore::new(home.join(".deepseek-code"));

    match session_name {
        Some(name) => {
            // Try to find and resume a specific session
            let summaries = store.list(&root)?;
            let target = summaries.iter().find(|s| {
                s.name.as_deref().is_some_and(|n| n == name) || s.id.to_string().starts_with(&name)
            });

            if let Some(summary) = target {
                println!("Resuming session: {}", summary.id);
                match store.load(&root, &summary.id) {
                    Ok(session) => {
                        println!(
                            "Session loaded: {} messages, {} tool calls",
                            session.messages.len(),
                            session.tool_call_history.len()
                        );
                        println!();
                        println!("Session ID: {}", session.id);
                        println!("Name: {}", session.name.as_deref().unwrap_or("unnamed"));
                        println!("Created: {}", session.created_at.format("%Y-%m-%d %H:%M"));
                        println!("Updated: {}", session.updated_at.format("%Y-%m-%d %H:%M"));
                        println!();
                        println!("To continue this session, run:");
                        println!("  deepseek-code chat --session {}", session.id);
                    }
                    Err(e) => {
                        eprintln!("Failed to load session: {e}");
                    }
                }
            } else {
                println!("Session not found: {name}");
                println!("\nAvailable sessions:");
                list_and_print(&store, &root)?;
            }
        }
        None => {
            // List all sessions
            list_and_print(&store, &root)?;
        }
    }

    Ok(())
}

/// Export a session to a file.
pub async fn export(
    session_id: Option<String>,
    format: Option<String>,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = project_root
        .unwrap_or_else(|| storage::find_project_root().unwrap_or_else(|| PathBuf::from(".")));

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;

    let store = SessionStore::new(home.join(".deepseek-code"));

    if let Some(id) = session_id {
        let sid = uuid::Uuid::parse_str(&id)?;
        let session = store.load(&root, &sid)?;

        let fmt = match format.as_deref() {
            Some("json") => TranscriptFormat::Json,
            Some("text" | "txt") => TranscriptFormat::PlainText,
            _ => TranscriptFormat::Markdown,
        };

        let output = storage::export_transcript(&session, fmt);

        let ext = match fmt {
            TranscriptFormat::Json => "json",
            TranscriptFormat::Markdown => "md",
            TranscriptFormat::PlainText => "txt",
        };

        let filename = format!("session-{sid}.{ext}");
        std::fs::write(&filename, output)?;
        println!("Session exported to: {filename}");
    } else {
        // Export latest session
        let summaries = store.list(&root)?;
        if let Some(latest) = summaries.first() {
            let session = store.load(&root, &latest.id)?;
            let output = storage::export_transcript(&session, TranscriptFormat::Markdown);
            let filename = format!("session-{}.md", latest.id);
            std::fs::write(&filename, output)?;
            println!("Latest session exported to: {filename}");
        } else {
            println!("No sessions found for this project.");
        }
    }

    Ok(())
}

fn list_and_print(
    store: &SessionStore,
    project_root: &std::path::Path,
) -> Result<(), anyhow::Error> {
    let summaries = store.list(project_root)?;

    if summaries.is_empty() {
        println!("No saved sessions for this project.");
        return Ok(());
    }

    println!("Saved sessions ({})\n", summaries.len());
    for s in &summaries {
        println!(
            "  {} | {} | {} msgs | {} tools | {}",
            s.id.to_string().chars().take(8).collect::<String>(),
            s.name.as_deref().unwrap_or("unnamed"),
            s.message_count,
            s.tool_call_count,
            s.updated_at.format("%Y-%m-%d %H:%M")
        );
    }
    println!();
    println!("Resume: deepseek-code resume <name-or-id-prefix>");
    println!("Export: deepseek-code export <id>");

    Ok(())
}
