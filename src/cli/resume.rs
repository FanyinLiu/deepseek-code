use std::path::{Path, PathBuf};

use crate::cli::resolve_project_root;
use crate::storage::{self, EventLogStore, SessionStore, TranscriptFormat};

/// Return the most recently updated session's ID for the given project, or
/// `None` if no sessions have been saved. Used by `--continue` flags to skip
/// the explicit `--session <id>` lookup. Sessions live under
/// `~/.octocode/<project-key>/sessions/` and `SessionStore::list` already
/// sorts by `updated_at` descending, so the head of the list is the latest.
pub fn resolve_latest_session_id(project_root: &Path) -> Result<Option<String>, anyhow::Error> {
    let home =
        storage::user_home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;
    let store = SessionStore::new(home.join(".octocode"));
    latest_session_id_from_store(&store, project_root)
}

/// Pure helper around `SessionStore::list` — exposed for tests so we don't
/// have to mutate process-global env vars to substitute the home dir.
fn latest_session_id_from_store(
    store: &SessionStore,
    project_root: &Path,
) -> Result<Option<String>, anyhow::Error> {
    let summaries = store.list(project_root)?;
    Ok(summaries.first().map(|summary| summary.id.to_string()))
}

/// Run the resume command: list and restore saved sessions.
pub async fn resume(
    session_name: Option<String>,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "resume")?;

    let home =
        storage::user_home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;

    let store = SessionStore::new(home.join(".octocode"));
    let event_store = EventLogStore::new(home.join(".octocode"));

    match session_name {
        Some(name) => {
            // Try to find and resume a specific session
            let summaries = store.list(&root)?;
            let target = summaries.iter().find(|s| {
                s.name.as_deref().is_some_and(|n| n == name) || s.id.to_string().starts_with(&name)
            });

            if let Some(summary) = target {
                println!("Session details: {}", summary.id);
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
                        if let Ok(Some(turn_id)) = event_store.unfinished_turn(&root, &session.id) {
                            println!();
                            println!(
                                "上次任务未完成: turn {}. 为避免重复执行有副作用的命令，继续前请先查看日志或手动确认下一步。",
                                turn_id
                            );
                        }
                        if let Ok(Some(swarm)) = event_store.latest_swarm_state(&root, &session.id)
                        {
                            println!();
                            println!("Swarm 状态: {}", swarm.status_label());
                            println!("Run ID: {}", swarm.run_id);
                            println!("摘要: {}", swarm.summary);
                            println!(
                                "任务: running {} / done {} / failed {} / cancelled {} / total {}",
                                swarm.running,
                                swarm.done,
                                swarm.failed,
                                swarm.cancelled,
                                swarm.total
                            );
                            if swarm.pending_patches > swarm.applied_patches {
                                println!(
                                    "待确认 patch: {}",
                                    swarm.pending_patches - swarm.applied_patches
                                );
                            }
                            println!(
                                "事件日志: {}",
                                event_store
                                    .events_path(&root, &session.id)
                                    .to_string_lossy()
                            );
                            println!("恢复提示: 只展示状态，不会自动重跑命令或重新写文件。");
                        }
                        println!();
                        println!("To continue this session, run one of:");
                        println!("  octo chat --session {}", session.id);
                        println!("  octo chat --continue   # if this is the most recent session");
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
    let root = resolve_project_root(project_root, "export")?;

    let home =
        storage::user_home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;

    let store = SessionStore::new(home.join(".octocode"));

    if let Some(id) = session_id {
        let sid = uuid::Uuid::parse_str(&id)?;
        let session = store.load(&root, &sid)?;

        let fmt = transcript_format(format.as_deref());
        let output = storage::export_transcript(&session, fmt);
        let filename = format!("session-{sid}.{}", transcript_extension(fmt));
        std::fs::write(&filename, output)?;
        println!("Session exported to: {filename}");
    } else {
        // Export latest session
        let summaries = store.list(&root)?;
        if let Some(latest) = summaries.first() {
            let session = store.load(&root, &latest.id)?;
            let fmt = transcript_format(format.as_deref());
            let output = storage::export_transcript(&session, fmt);
            let filename = format!("session-{}.{}", latest.id, transcript_extension(fmt));
            std::fs::write(&filename, output)?;
            println!("Latest session exported to: {filename}");
        } else {
            println!("No sessions found for this project.");
        }
    }

    Ok(())
}

fn transcript_format(value: Option<&str>) -> TranscriptFormat {
    match value {
        Some("json") => TranscriptFormat::Json,
        Some("text" | "txt") => TranscriptFormat::PlainText,
        _ => TranscriptFormat::Markdown,
    }
}

fn transcript_extension(format: TranscriptFormat) -> &'static str {
    match format {
        TranscriptFormat::Json => "json",
        TranscriptFormat::Markdown => "md",
        TranscriptFormat::PlainText => "txt",
    }
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
    println!("Resume: octo resume <name-or-id-prefix>");
    println!("Export: octo export <id>");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek::{ReasoningState, Session, SessionId, SessionMetadata};

    fn fresh_session(project_root: &Path, updated_at: chrono::DateTime<chrono::Utc>) -> Session {
        let id = SessionId::new_v4();
        Session {
            id,
            name: None,
            project_root: project_root.to_path_buf(),
            messages: Vec::new(),
            reasoning_state: ReasoningState::default(),
            tool_call_history: Vec::new(),
            checkpoints: Vec::new(),
            created_at: updated_at,
            updated_at,
            metadata: SessionMetadata::default(),
        }
    }

    #[test]
    fn latest_session_id_is_none_when_no_sessions() {
        let home = tempfile::tempdir().expect("tempdir");
        let project = tempfile::tempdir().expect("project");
        let store = SessionStore::new(home.path().join(".octocode"));
        let latest =
            latest_session_id_from_store(&store, project.path()).expect("list empty store");
        assert!(latest.is_none());
    }

    #[test]
    fn latest_session_id_picks_most_recent_by_updated_at() {
        let home = tempfile::tempdir().expect("tempdir");
        let project = tempfile::tempdir().expect("project");
        let store = SessionStore::new(home.path().join(".octocode"));

        let now = chrono::Utc::now();
        let older = fresh_session(project.path(), now - chrono::Duration::hours(2));
        let newer = fresh_session(project.path(), now);
        store.save(&older).expect("save older");
        store.save(&newer).expect("save newer");

        let latest = latest_session_id_from_store(&store, project.path())
            .expect("list store")
            .expect("expected a latest session");
        assert_eq!(latest, newer.id.to_string());
    }

    #[test]
    fn transcript_format_and_extension_follow_requested_output() {
        assert!(matches!(
            transcript_format(Some("json")),
            TranscriptFormat::Json
        ));
        assert!(matches!(
            transcript_format(Some("txt")),
            TranscriptFormat::PlainText
        ));
        assert!(matches!(
            transcript_format(Some("text")),
            TranscriptFormat::PlainText
        ));
        assert!(matches!(
            transcript_format(None),
            TranscriptFormat::Markdown
        ));

        assert_eq!(transcript_extension(TranscriptFormat::Json), "json");
        assert_eq!(transcript_extension(TranscriptFormat::PlainText), "txt");
        assert_eq!(transcript_extension(TranscriptFormat::Markdown), "md");
    }
}
