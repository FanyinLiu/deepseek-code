use super::{manager_header, CommandContext, CommandResult};

pub(super) fn cmd_schedule(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let store = crate::storage::ScheduledTaskStore::default_user();
    let mut parts = args.trim().splitn(3, ' ');
    let action = parts.next().unwrap_or("");
    match action {
        "" | "list" => {
            let tasks = store
                .list()
                .map_err(|e| format!("Failed to list scheduled tasks: {e}"))?;
            let mut lines = vec![manager_header("schedule", "local")];
            if tasks.is_empty() {
                lines.push("status   no planned tasks".to_string());
                lines.push(format!("store    {}", store.root().display()));
            } else {
                lines.push(format!("count    {}", tasks.len()));
                for task in tasks {
                    lines.push(task.format_row());
                }
            }
            lines.push(
                "usage    /schedule add heartbeat|standalone <task> | pause|resume|logs|rm <id>"
                    .to_string(),
            );
            Ok(Some(lines.join("\n")))
        }
        "add" => {
            let Some(kind) = parts.next() else {
                return Err("Usage: /schedule add heartbeat|standalone <task>".to_string());
            };
            let Some(prompt) = parts.next() else {
                return Err("Usage: /schedule add heartbeat|standalone <task>".to_string());
            };
            let kind = kind.parse::<crate::storage::ScheduledTaskKind>()?;
            let task = store
                .create(kind, prompt.trim().to_string(), ctx.project_root.to_path_buf())
                .map_err(|e| format!("Failed to create scheduled task: {e}"))?;
            Ok(Some(format!(
                "{}\nid      {}\nkind    {}\nstatus  {}",
                manager_header("schedule", "created"),
                task.id,
                task.kind.as_str(),
                task.status.as_str()
            )))
        }
        "pause" | "resume" | "logs" | "rm" | "remove" | "run" => {
            let Some(id) = parts.next() else {
                return Err(format!("Usage: /schedule {action} <id>"));
            };
            match action {
                "pause" => {
                    let task = store
                        .set_status(id, crate::storage::ScheduledTaskStatus::Paused)
                        .map_err(|e| format!("Failed to pause task: {e}"))?;
                    Ok(Some(format!(
                        "{}\nid      {}",
                        manager_header("schedule", "paused"),
                        task.id
                    )))
                }
                "resume" => {
                    let task = store
                        .set_status(id, crate::storage::ScheduledTaskStatus::Active)
                        .map_err(|e| format!("Failed to resume task: {e}"))?;
                    Ok(Some(format!(
                        "{}\nid      {}",
                        manager_header("schedule", "resumed"),
                        task.id
                    )))
                }
                "logs" => {
                    let task = store
                        .load(id)
                        .map_err(|e| format!("Failed to load task: {e}"))?;
                    let mut lines = vec![
                        manager_header("schedule", "logs"),
                        format!("id      {}", task.id),
                        format!("kind    {}", task.kind.as_str()),
                        format!("status  {}", task.status.as_str()),
                        format!("root    {}", task.project_root.display()),
                        format!("prompt  {}", task.prompt),
                        format!("last    {}", task.last_status.as_deref().unwrap_or("never run")),
                    ];
                    if let Some(path) = task.last_log_path {
                        lines.push(format!("log     {}", path.display()));
                    } else {
                        lines.push(
                            "log     use `octo resume` to inspect session event logs".to_string(),
                        );
                    }
                    Ok(Some(lines.join("\n")))
                }
                "run" => Ok(Some(
                    [
                        manager_header("schedule", "manual-run"),
                        format!("id      {id}"),
                        "next    run `octo task run <id>` from the shell".to_string(),
                        "note    TUI slash commands stay synchronous to avoid hidden side effects"
                            .to_string(),
                    ]
                    .join("\n"),
                )),
                _ => {
                    let task = store
                        .remove(id)
                        .map_err(|e| format!("Failed to remove task: {e}"))?;
                    Ok(Some(format!(
                        "{}\nid      {}",
                        manager_header("schedule", "removed"),
                        task.id
                    )))
                }
            }
        }
        _ => Err(
            "Usage: /schedule list | add heartbeat|standalone <task> | pause|resume|run|logs|rm <id>"
                .to_string(),
        ),
    }
}
