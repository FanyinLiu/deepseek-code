use std::path::Path;

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::tools::todo_state::{read_todo_items, write_todo_items};

#[derive(Debug, Deserialize)]
pub struct TaskCreateArgs {
    pub id: Option<String>,
    #[serde(alias = "prompt")]
    pub content: String,
    pub active_form: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TaskUpdateArgs {
    pub id: String,
    pub content: Option<String>,
    pub active_form: Option<String>,
    pub status: Option<String>,
}

pub fn task_create(project_root: &Path, args: TaskCreateArgs) -> Result<String> {
    let content = non_empty(args.content, "content")?;
    let mut todos = read_todo_items(project_root)?;
    let id = args
        .id
        .map(|id| non_empty(id, "id"))
        .transpose()?
        .unwrap_or_else(|| next_task_id(&todos));
    if find_task_index(&todos, &id).is_ok() {
        bail!("task id already exists: {id}");
    }

    let status = normalize_task_status(args.status.as_deref().unwrap_or("pending"))?;
    let mut object = Map::new();
    object.insert("id".to_string(), Value::String(id.clone()));
    object.insert("content".to_string(), Value::String(content));
    object.insert("status".to_string(), Value::String(status.to_string()));
    if let Some(active_form) = args.active_form {
        object.insert(
            "active_form".to_string(),
            Value::String(non_empty(active_form, "active_form")?),
        );
    }
    todos.push(Value::Object(object));
    write_todo_items(project_root, todos)?;
    Ok(format!(
        "Task created:\n{}",
        format_task(project_root, &id)?
    ))
}

pub fn task_get(project_root: &Path, id: &str) -> Result<String> {
    let id = non_empty(id.to_string(), "id")?;
    format_task(project_root, &id)
}

pub fn task_list(project_root: &Path) -> Result<String> {
    let todos = read_todo_items(project_root)?;
    if todos.is_empty() {
        return Ok("No local tasks.".to_string());
    }
    let mut lines = vec![format!("Tasks: {}", todos.len())];
    for (idx, todo) in todos.iter().enumerate() {
        lines.push(format_task_value(idx, todo));
    }
    Ok(lines.join("\n"))
}

pub fn task_update(project_root: &Path, args: TaskUpdateArgs) -> Result<String> {
    let id = non_empty(args.id, "id")?;
    if args.content.is_none() && args.active_form.is_none() && args.status.is_none() {
        bail!("task_update requires content, active_form, or status");
    }
    let mut todos = read_todo_items(project_root)?;
    let index = find_task_index(&todos, &id)?;
    let Some(object) = todos[index].as_object_mut() else {
        bail!("task is not an object: {id}");
    };
    if let Some(content) = args.content {
        object.insert(
            "content".to_string(),
            Value::String(non_empty(content, "content")?),
        );
    }
    if let Some(active_form) = args.active_form {
        object.insert(
            "active_form".to_string(),
            Value::String(non_empty(active_form, "active_form")?),
        );
    }
    if let Some(status) = args.status {
        object.insert(
            "status".to_string(),
            Value::String(normalize_task_status(&status)?.to_string()),
        );
    }
    write_todo_items(project_root, todos)?;
    Ok(format!(
        "Task updated:\n{}",
        format_task(project_root, &id)?
    ))
}

pub fn task_stop(project_root: &Path, id: &str) -> Result<String> {
    task_update(
        project_root,
        TaskUpdateArgs {
            id: id.to_string(),
            content: None,
            active_form: None,
            status: Some("cancelled".to_string()),
        },
    )
    .map(|task| task.replacen("Task updated:", "Task stopped:", 1))
}

fn format_task(project_root: &Path, id: &str) -> Result<String> {
    let todos = read_todo_items(project_root)?;
    let index = find_task_index(&todos, id)?;
    Ok(format_task_value(index, &todos[index]))
}

fn format_task_value(index: usize, todo: &Value) -> String {
    let id = todo
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("#{}", index + 1));
    let status = todo
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("pending");
    let content = todo
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("(empty)");
    let active = todo
        .get("active_form")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty());
    if status == "in_progress" {
        format!("{id}  {status}  {}", active.unwrap_or(content))
    } else {
        format!("{id}  {status}  {content}")
    }
}

fn find_task_index(todos: &[Value], id_or_prefix: &str) -> Result<usize> {
    let id_or_prefix = id_or_prefix.trim();
    if id_or_prefix.is_empty() {
        bail!("id is required");
    }
    if let Ok(number) = id_or_prefix.parse::<usize>() {
        if (1..=todos.len()).contains(&number) {
            return Ok(number - 1);
        }
    }

    let exact = todos.iter().position(|todo| {
        todo.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == id_or_prefix)
    });
    if let Some(index) = exact {
        return Ok(index);
    }

    let matches = todos
        .iter()
        .enumerate()
        .filter_map(|(index, todo)| {
            todo.get("id")
                .and_then(Value::as_str)
                .filter(|id| id.starts_with(id_or_prefix))
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [index] => Ok(*index),
        [] => Err(anyhow!("task not found: {id_or_prefix}")),
        _ => Err(anyhow!("task id prefix is ambiguous: {id_or_prefix}")),
    }
}

fn next_task_id(todos: &[Value]) -> String {
    let mut next = todos.len() + 1;
    loop {
        let candidate = format!("task-{next}");
        if todos.iter().all(|todo| {
            todo.get("id")
                .and_then(Value::as_str)
                .is_none_or(|id| id != candidate)
        }) {
            return candidate;
        }
        next += 1;
    }
}

fn normalize_task_status(status: &str) -> Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "pending" | "todo" => Ok("pending"),
        "in_progress" | "in-progress" | "active" | "running" | "start" | "resume" => {
            Ok("in_progress")
        }
        "completed" | "complete" | "done" => Ok("completed"),
        "cancelled" | "canceled" | "cancel" | "stop" | "stopped" | "pause" | "paused" => {
            Ok("cancelled")
        }
        other => Err(anyhow!("unsupported task status: {other}")),
    }
}

fn non_empty(value: String, field: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{field} is required");
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::todo_state::{read_todo_items, todo_state_path};

    #[test]
    fn task_lifecycle_uses_project_todo_file() {
        let temp = tempfile::tempdir().expect("tempdir");

        let created = task_create(
            temp.path(),
            TaskCreateArgs {
                id: Some("build-ui".to_string()),
                content: "Build UI".to_string(),
                active_form: Some("Building UI".to_string()),
                status: None,
            },
        )
        .expect("create task");

        assert!(created.contains("build-ui"));
        assert!(todo_state_path(temp.path()).exists());

        let updated = task_update(
            temp.path(),
            TaskUpdateArgs {
                id: "build".to_string(),
                content: None,
                active_form: None,
                status: Some("in_progress".to_string()),
            },
        )
        .expect("update task");

        assert!(updated.contains("Building UI"));
        assert!(task_get(temp.path(), "build-ui")
            .expect("get task")
            .contains("in_progress"));
        assert!(task_list(temp.path())
            .expect("list tasks")
            .contains("Tasks: 1"));

        let stopped = task_stop(temp.path(), "1").expect("stop task");
        assert!(stopped.contains("cancelled"));
        let todos = read_todo_items(temp.path()).expect("read todos");
        assert_eq!(todos[0]["status"], "cancelled");
    }

    #[test]
    fn task_update_rejects_second_in_progress() {
        let temp = tempfile::tempdir().expect("tempdir");
        task_create(
            temp.path(),
            TaskCreateArgs {
                id: Some("a".to_string()),
                content: "A".to_string(),
                active_form: None,
                status: Some("in_progress".to_string()),
            },
        )
        .expect("create first");
        task_create(
            temp.path(),
            TaskCreateArgs {
                id: Some("b".to_string()),
                content: "B".to_string(),
                active_form: None,
                status: None,
            },
        )
        .expect("create second");

        let err = task_update(
            temp.path(),
            TaskUpdateArgs {
                id: "b".to_string(),
                content: None,
                active_form: None,
                status: Some("in_progress".to_string()),
            },
        )
        .expect_err("second in_progress should fail");

        assert!(err.to_string().contains("at most one"));
    }
}
