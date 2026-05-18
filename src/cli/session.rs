use std::path::{Path, PathBuf};

use crate::cli::resolve_project_root;
use crate::storage::{self, EventLogStore, SessionStore};

#[derive(Debug, Clone)]
pub enum SessionCommand {
    List {
        json: bool,
    },
    Replay {
        session: Option<String>,
        json: bool,
    },
    Export {
        session: Option<String>,
        format: String,
    },
}

pub async fn session(
    command: SessionCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "session")?;
    let home = user_home_dir().ok_or_else(|| anyhow::anyhow!("cannot find home directory"))?;
    let store = SessionStore::new(home.join(".octocode"));
    let events = EventLogStore::new(home.join(".octocode"));

    match command {
        SessionCommand::List { json } => list(&store, &root, json),
        SessionCommand::Replay { session, json } => replay(&store, &events, &root, session, json),
        SessionCommand::Export { session, format } => export(&store, &root, session, &format),
    }
}

fn user_home_dir() -> Option<PathBuf> {
    std::env::var_os("DEEPSEEK_CODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

fn list(store: &SessionStore, root: &Path, json: bool) -> Result<(), anyhow::Error> {
    let sessions = store.list(root)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
    } else if sessions.is_empty() {
        println!("No saved sessions for this project.");
    } else {
        println!("Saved sessions ({})", sessions.len());
        for session in &sessions {
            println!(
                "  {} | {} | {} msgs | {} tools | {}",
                short_id(&session.id.to_string()),
                session.name.as_deref().unwrap_or("unnamed"),
                session.message_count,
                session.tool_call_count,
                session.updated_at.format("%Y-%m-%d %H:%M")
            );
        }
    }
    Ok(())
}

fn replay(
    store: &SessionStore,
    events: &EventLogStore,
    root: &Path,
    target: Option<String>,
    json: bool,
) -> Result<(), anyhow::Error> {
    let session_id = resolve_session_id(store, root, target)?;
    let loaded = events.load(root, &session_id)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&loaded)?);
    } else {
        println!("Replay session {session_id}");
        if loaded.is_empty() {
            println!("No event log found. Nothing was re-executed.");
            return Ok(());
        }
        for line in loaded.iter().map(crate::cli::replay::session_line) {
            println!("{} {}", line.at.format("%H:%M:%S"), line.text);
        }
        println!("Replay complete. No tools or commands were re-run.");
    }
    Ok(())
}

fn export(
    store: &SessionStore,
    root: &Path,
    target: Option<String>,
    format: &str,
) -> Result<(), anyhow::Error> {
    let session_id = resolve_session_id(store, root, target)?;
    let session = store.load(root, &session_id)?;
    let format = transcript_format(format);
    let output = storage::export_transcript(&session, format);
    let filename = format!("session-{session_id}.{}", transcript_extension(format));
    std::fs::write(&filename, output)?;
    println!("Session exported to: {filename}");
    Ok(())
}

fn resolve_session_id(
    store: &SessionStore,
    root: &Path,
    target: Option<String>,
) -> Result<crate::deepseek::SessionId, anyhow::Error> {
    let sessions = store.list(root)?;
    let Some(target) = target else {
        return sessions
            .first()
            .map(|session| session.id)
            .ok_or_else(|| anyhow::anyhow!("no saved sessions for this project"));
    };
    if let Ok(id) = uuid::Uuid::parse_str(&target) {
        return Ok(id);
    }
    sessions
        .iter()
        .find(|session| {
            session.id.to_string().starts_with(&target)
                || session.name.as_deref().is_some_and(|name| name == target)
        })
        .map(|session| session.id)
        .ok_or_else(|| anyhow::anyhow!("session not found: {target}"))
}

fn transcript_format(value: &str) -> storage::TranscriptFormat {
    match value {
        "json" => storage::TranscriptFormat::Json,
        "text" | "txt" => storage::TranscriptFormat::PlainText,
        _ => storage::TranscriptFormat::Markdown,
    }
}

fn transcript_extension(format: storage::TranscriptFormat) -> &'static str {
    match format {
        storage::TranscriptFormat::Json => "json",
        storage::TranscriptFormat::Markdown => "md",
        storage::TranscriptFormat::PlainText => "txt",
    }
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}
