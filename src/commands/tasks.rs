use serde::Serialize;

use super::{manager_header, truncate_display_width, CommandContext, CommandResult};

#[derive(Debug, Clone, Serialize)]
pub(super) struct UnifiedBackgroundTaskView {
    kind: &'static str,
    id: String,
    status: String,
    started: String,
    duration: String,
    summary: String,
    latest_output: String,
}

impl UnifiedBackgroundTaskView {
    fn from_subagent(
        task: &crate::agent::BackgroundTaskSnapshot,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let duration_ms = task
            .duration_ms
            .unwrap_or_else(|| duration_between_ms(task.started_at, task.completed_at, now));
        Self {
            kind: "subagent",
            id: task.task_id.clone(),
            status: task.status.to_string(),
            started: format_started_at(task.started_at),
            duration: format_duration_ms(duration_ms),
            summary: truncate_display_width(&task.description, 120),
            latest_output: task
                .summary
                .as_deref()
                .map(|summary| truncate_display_width(summary, 160))
                .unwrap_or_default(),
        }
    }

    fn from_shell(
        shell: &crate::tools::background_shells::ShellSnapshot,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        let status = if shell.is_running() {
            "running"
        } else if shell.exit_code == Some(0) {
            "completed"
        } else {
            "failed"
        };
        let latest_output = latest_shell_output(shell);
        Self {
            kind: "shell",
            id: shell.shell_id.clone(),
            status: status.to_string(),
            started: format_started_at(shell.started_at),
            duration: format_duration_ms(duration_between_ms(
                shell.started_at,
                shell.finished_at,
                now,
            )),
            summary: truncate_display_width(&shell.command, 120),
            latest_output,
        }
    }
}

pub(super) fn unified_background_task_views(
    subagents: &[crate::agent::BackgroundTaskSnapshot],
    shells: &[crate::tools::background_shells::ShellSnapshot],
    now: chrono::DateTime<chrono::Utc>,
) -> Vec<UnifiedBackgroundTaskView> {
    let mut views = subagents
        .iter()
        .map(|task| UnifiedBackgroundTaskView::from_subagent(task, now))
        .collect::<Vec<_>>();
    views.extend(
        shells
            .iter()
            .map(|shell| UnifiedBackgroundTaskView::from_shell(shell, now)),
    );
    views
}

fn format_started_at(started_at: chrono::DateTime<chrono::Utc>) -> String {
    started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

fn duration_between_ms(
    started_at: chrono::DateTime<chrono::Utc>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
) -> u64 {
    completed_at
        .unwrap_or(now)
        .signed_duration_since(started_at)
        .num_milliseconds()
        .max(0) as u64
}

fn format_duration_ms(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{}m {}s", ms / 60_000, (ms % 60_000) / 1_000)
    } else if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{ms}ms")
    }
}

fn latest_shell_output(shell: &crate::tools::background_shells::ShellSnapshot) -> String {
    let output = if shell.stderr.trim().is_empty() {
        shell.stdout.as_str()
    } else {
        shell.stderr.as_str()
    };
    let line = output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim();
    let mut latest = truncate_display_width(line, 160);
    if !latest.is_empty() && (shell.stdout_truncated || shell.stderr_truncated) {
        latest = format!("[truncated] {latest}");
    }
    latest
}

pub(super) fn cmd_tasks(args: &str, ctx: &mut CommandContext) -> CommandResult {
    let shells = crate::tools::background_shells::registry().list();
    render_background_tasks(args, ctx.background_tasks, &shells, chrono::Utc::now())
}

pub(super) fn render_background_tasks(
    args: &str,
    background_tasks: &[crate::agent::BackgroundTaskSnapshot],
    shells: &[crate::tools::background_shells::ShellSnapshot],
    now: chrono::DateTime<chrono::Utc>,
) -> CommandResult {
    let views = unified_background_task_views(background_tasks, shells, now);
    let json = matches!(args.trim(), "--json" | "json");
    if json {
        return serde_json::to_string_pretty(&serde_json::json!({
            "count": views.len(),
            "tasks": views,
        }))
        .map(Some)
        .map_err(|error| format!("Failed to render tasks JSON: {error}"));
    }
    if views.is_empty() {
        return Ok(Some(format!(
            "{}\nstatus    no background tasks",
            manager_header("tasks", "empty")
        )));
    }

    let status = if views.iter().any(|task| task.status == "running") {
        "running"
    } else {
        "ready"
    };
    let mut lines = vec![
        manager_header("tasks", status),
        format!("count     {}", views.len()),
    ];
    for task in views {
        lines.push(String::new());
        lines.push(format!("kind      {}", task.kind));
        lines.push(format!("id        {}", task.id));
        lines.push(format!("status    {}", task.status));
        lines.push(format!("started   {}", task.started));
        lines.push(format!("duration  {}", task.duration));
        lines.push(format!("summary   {}", task.summary));
        lines.push(format!("latest_output {}", task.latest_output));
    }
    Ok(Some(lines.join("\n").trim_end().to_string()))
}
