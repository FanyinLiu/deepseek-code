use crate::deepseek::{MessageVisibility, Role, Session};

/// Export a session transcript in the requested format.
#[must_use]
pub fn export_transcript(session: &Session, format: TranscriptFormat) -> String {
    match format {
        TranscriptFormat::Markdown => to_markdown(session),
        TranscriptFormat::Json => serde_json::to_string_pretty(session)
            .unwrap_or_else(|e| format!("serialization error: {e}")),
        TranscriptFormat::PlainText => to_plain_text(session),
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TranscriptFormat {
    Markdown,
    Json,
    PlainText,
}

fn to_markdown(session: &Session) -> String {
    let mut out = String::new();
    out.push_str("# DeepSeek-Code Session\n\n");
    out.push_str(&format!("- **ID**: `{}`\n", session.id));
    out.push_str(&format!(
        "- **Name**: {}\n",
        session.name.as_deref().unwrap_or("unnamed")
    ));
    out.push_str(&format!(
        "- **Project**: {}\n",
        session.project_root.display()
    ));
    out.push_str(&format!("- **Messages**: {}\n", session.messages.len()));
    out.push_str(&format!(
        "- **Tool calls**: {}\n\n",
        session.tool_call_history.len()
    ));

    for (i, msg) in session.messages.iter().enumerate() {
        if msg.visibility == MessageVisibility::AuditOnly {
            continue;
        }

        let role_label = match msg.role {
            Role::User => "You",
            Role::Assistant => "DeepSeek",
            Role::System => "System",
            Role::Tool => "Tool",
        };

        out.push_str(&format!("### {} [{}]\n\n", role_label, i + 1));

        match &msg.content {
            crate::deepseek::MessageContent::Text(text) if !text.is_empty() => {
                out.push_str(text);
                out.push('\n');
            }
            crate::deepseek::MessageContent::MultiPart(parts) => {
                for part in parts {
                    if let Some(ref text) = part.text {
                        out.push_str(text);
                    }
                }
                out.push('\n');
            }
            _ => {}
        }

        // Show reasoning summary, not raw content
        if msg.reasoning_content.is_some() {
            out.push_str("*[thinking — content not exported by default]*\n");
        }

        if !msg.tool_calls.is_empty() {
            out.push_str("\n**Tool calls:**\n\n");
            for tc in &msg.tool_calls {
                out.push_str(&format!("- `{}` (`{}`)\n", tc.function.name, tc.id));
            }
        }

        if !msg.tool_results.is_empty() {
            out.push_str("\n**Results:**\n\n");
            for tr in &msg.tool_results {
                let status = if tr.is_error { "❌" } else { "✅" };
                out.push_str(&format!("- {} `{}`\n", status, tr.name));
            }
        }

        out.push_str("\n---\n\n");
    }

    out
}

fn to_plain_text(session: &Session) -> String {
    let mut out = String::new();
    out.push_str(&format!("DeepSeek-Code Session: {}\n", session.id));
    out.push_str(&format!("Project: {}\n\n", session.project_root.display()));

    for msg in &session.messages {
        if msg.visibility == MessageVisibility::AuditOnly {
            continue;
        }
        let role = match msg.role {
            Role::User => "> ",
            Role::Assistant => "",
            Role::System => "[System] ",
            Role::Tool => "[Tool] ",
        };
        let content = msg.content.to_string_lossy();
        if !content.is_empty() {
            for line in content.lines() {
                out.push_str(&format!("{role}{line}\n"));
            }
        }
        if !msg.tool_calls.is_empty() {
            for tc in &msg.tool_calls {
                out.push_str(&format!("[Tool call: {} ({})]\n", tc.function.name, tc.id));
            }
        }
    }
    out
}
