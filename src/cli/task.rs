use std::path::PathBuf;

use std::io::IsTerminal;

use crate::cli::{resolve_project_root, ToolApprovalPolicy, TurnOutputFormat};
use crate::deepseek::SessionId;
use crate::storage::{
    EventLogStore, ScheduledTask, ScheduledTaskKind, ScheduledTaskStatus, ScheduledTaskStore,
    SessionEvent, SessionEventKind,
};

pub async fn task(
    command: TaskCommand,
    project_root: Option<PathBuf>,
) -> Result<(), anyhow::Error> {
    let store = ScheduledTaskStore::default_user();
    match command {
        TaskCommand::List => {
            print_task_list(&store)?;
        }
        TaskCommand::Add { kind, prompt } => {
            let root = resolve_project_root(project_root, "task")?;
            let task = store.create(kind, prompt, root)?;
            println!("created {}", task.id);
            println!("kind    {}", task.kind.as_str());
            println!("status  {}", task.status.as_str());
        }
        TaskCommand::Pause { id } => {
            let task = store.set_status(&id, ScheduledTaskStatus::Paused)?;
            println!("paused {}", task.id);
        }
        TaskCommand::Resume { id } => {
            let task = store.set_status(&id, ScheduledTaskStatus::Active)?;
            println!("resumed {}", task.id);
        }
        TaskCommand::Logs { id } => {
            let task = store.load(&id)?;
            println!("task    {}", task.id);
            println!("kind    {}", task.kind.as_str());
            println!("status  {}", task.status.as_str());
            println!("prompt  {}", task.prompt);
            println!("root    {}", task.project_root.display());
            println!(
                "last    {}",
                task.last_status.as_deref().unwrap_or("never run")
            );
            if let Some(path) = task.last_log_path {
                println!("log     {}", path.display());
            } else {
                println!(
                    "log     run history is in session events; use `octo resume` to inspect sessions"
                );
            }
        }
        TaskCommand::Run { id, format } => {
            let task = store.load(&id)?;
            ensure_task_can_run(&task)?;
            let run_session_id = SessionId::new_v4();
            let log_path = task_event_log_path(&task, run_session_id);
            store.record_run_start(&task.id, log_path.clone())?;
            append_task_event(
                &task,
                run_session_id,
                "running",
                "scheduled task run started",
            );
            let output_format = format.unwrap_or_else(|| {
                if std::io::stdin().is_terminal() {
                    TurnOutputFormat::Text
                } else {
                    TurnOutputFormat::Json
                }
            });
            let result = crate::cli::run(
                task.prompt.clone(),
                false,
                Some(task.project_root.clone()),
                output_format,
                ToolApprovalPolicy::Deny,
                None,
                None,
            )
            .await;
            match result {
                Ok(()) => {
                    store.record_run_finish(&task.id, "done")?;
                    append_task_event(&task, run_session_id, "done", "scheduled task run finished");
                    println!("task done {}", task.id);
                }
                Err(err) => {
                    let _ = store.record_run_finish(&task.id, "failed");
                    append_task_event(
                        &task,
                        run_session_id,
                        "failed",
                        &format!("scheduled task run failed: {err}"),
                    );
                    return Err(err);
                }
            }
        }
        TaskCommand::Remove { id } => {
            let task = store.remove(&id)?;
            println!("removed {}", task.id);
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum TaskCommand {
    List,
    Add {
        kind: ScheduledTaskKind,
        prompt: String,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Run {
        id: String,
        format: Option<TurnOutputFormat>,
    },
    Logs {
        id: String,
    },
    Remove {
        id: String,
    },
}

fn print_task_list(store: &ScheduledTaskStore) -> Result<(), anyhow::Error> {
    let tasks = store.list()?;
    if tasks.is_empty() {
        println!("no scheduled tasks");
        println!("store {}", store.root().display());
        return Ok(());
    }
    println!("scheduled tasks ({})", tasks.len());
    for task in tasks {
        println!("{}", task.format_row());
    }
    Ok(())
}

fn ensure_task_can_run(task: &ScheduledTask) -> Result<(), anyhow::Error> {
    if task.status == ScheduledTaskStatus::Paused {
        anyhow::bail!("task is paused: {}", task.id);
    }
    Ok(())
}

fn task_event_store() -> Option<EventLogStore> {
    dirs::home_dir().map(|home| EventLogStore::new(home.join(".octocode")))
}

fn task_event_log_path(task: &ScheduledTask, session_id: SessionId) -> Option<PathBuf> {
    task_event_store().map(|store| store.events_path(&task.project_root, &session_id))
}

fn append_task_event(task: &ScheduledTask, session_id: SessionId, status: &str, summary: &str) {
    let Some(store) = task_event_store() else {
        return;
    };
    let event = SessionEvent::new(
        session_id,
        None,
        SessionEventKind::ScheduledTaskUpdated {
            task_id: task.id.clone(),
            status: status.to_string(),
            summary: summary.to_string(),
        },
    );
    if let Err(err) = store.append(&task.project_root, &event) {
        tracing::warn!("failed to append scheduled task event: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_task(status: ScheduledTaskStatus) -> ScheduledTask {
        let mut task = ScheduledTask::new(
            ScheduledTaskKind::Standalone,
            "检查计划任务".to_string(),
            PathBuf::from("/tmp/project"),
        );
        task.status = status;
        task
    }

    #[test]
    fn paused_task_is_rejected_before_running() {
        let task = test_task(ScheduledTaskStatus::Paused);
        let err = ensure_task_can_run(&task).expect_err("paused task should not run");

        assert!(err.to_string().contains("task is paused"));
    }

    #[test]
    fn active_task_can_run() {
        let task = test_task(ScheduledTaskStatus::Active);

        ensure_task_can_run(&task).expect("active task should run");
    }
}
