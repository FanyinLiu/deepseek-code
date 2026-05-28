use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde_json::Value;

const VALID_STATUSES: &[&str] = &["pending", "in_progress", "completed", "cancelled"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TodoSummary {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub cancelled: usize,
    pub active: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TodoBoardItem {
    pub id: String,
    pub status: String,
    pub content: String,
    pub active_form: Option<String>,
    pub priority: Option<String>,
}

impl TodoBoardItem {
    #[must_use]
    pub fn display_text(&self) -> &str {
        if self.status == "in_progress" {
            self.active_form.as_deref().unwrap_or(&self.content)
        } else {
            &self.content
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TodoBoard {
    pub summary: TodoSummary,
    pub items: Vec<TodoBoardItem>,
}

impl TodoSummary {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    #[must_use]
    pub fn compact_line(&self) -> String {
        if self.total == 0 {
            return "todo 0/0".to_string();
        }
        let mut line = format!("todo {}/{}", self.completed, self.total);
        if let Some(active) = &self.active {
            line.push_str(&format!(" · active: {active}"));
        } else if self.pending > 0 {
            line.push_str(&format!(" · pending {}", self.pending));
        }
        if self.cancelled > 0 {
            line.push_str(&format!(" · cancelled {}", self.cancelled));
        }
        line
    }

    #[must_use]
    pub fn context_line(&self, chinese: bool) -> String {
        let label = if chinese { "待办" } else { "todo" };
        if self.total == 0 {
            return format!("{label}        none");
        }
        format!(
            "{label}        {}/{} done, {} active, {} pending{}",
            self.completed,
            self.total,
            self.in_progress,
            self.pending,
            self.active
                .as_ref()
                .map(|active| format!(" · {active}"))
                .unwrap_or_default()
        )
    }
}

#[must_use]
pub fn todo_state_path(project_root: &Path) -> PathBuf {
    project_root.join(".octocode").join("todos.json")
}

pub fn read_todo_items(project_root: &Path) -> Result<Vec<Value>> {
    let path = todo_state_path(project_root);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = crate::storage::read_text_file_capped(&path)?;
    let value: Value = serde_json::from_str(&content)?;
    Ok(value
        .get("todos")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub fn write_todo_items(project_root: &Path, todos: Vec<Value>) -> Result<TodoSummary> {
    validate_todo_items(&todos)?;
    let dir = project_root.join(".octocode");
    std::fs::create_dir_all(&dir)?;
    let payload = serde_json::json!({
        "updated_at": chrono::Utc::now(),
        "todos": todos,
    });
    std::fs::write(
        todo_state_path(project_root),
        serde_json::to_string_pretty(&payload)?,
    )?;
    Ok(summarize_todo_items(
        payload
            .get("todos")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
    ))
}

pub fn load_todo_summary(project_root: &Path) -> TodoSummary {
    read_todo_items(project_root)
        .map(|items| summarize_todo_items(&items))
        .unwrap_or_default()
}

#[must_use]
pub fn load_todo_board(project_root: &Path) -> TodoBoard {
    let items = read_todo_items(project_root).unwrap_or_default();
    TodoBoard {
        summary: summarize_todo_items(&items),
        items: board_items(&items),
    }
}

pub fn validate_todo_items(todos: &[Value]) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    let mut in_progress = 0usize;
    for todo in todos {
        let id = todo
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let content = todo
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let active_form = todo
            .get("active_form")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let status = todo
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if todo.get("id").is_some() && id.is_empty() {
            return Err(anyhow!("todo id must not be empty when provided"));
        }
        if !id.is_empty() && !ids.insert(id.to_string()) {
            return Err(anyhow!("duplicate todo id: {id}"));
        }
        if content.is_empty() {
            return Err(anyhow!("todo content must not be empty"));
        }
        if todo.get("active_form").is_some() && active_form.is_empty() {
            return Err(anyhow!("todo active_form must not be empty when provided"));
        }
        if !VALID_STATUSES.contains(&status) {
            return Err(anyhow!("unsupported todo status: {status}"));
        }
        if status == "in_progress" {
            in_progress += 1;
        }
    }
    if in_progress > 1 {
        return Err(anyhow!("at most one todo may be in_progress"));
    }
    Ok(())
}

#[must_use]
pub fn summarize_todo_items(todos: &[Value]) -> TodoSummary {
    let mut summary = TodoSummary {
        total: todos.len(),
        ..TodoSummary::default()
    };
    for todo in todos {
        let status = todo
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        match status {
            "pending" => summary.pending += 1,
            "in_progress" => {
                summary.in_progress += 1;
                summary.active = todo
                    .get("active_form")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .or_else(|| todo.get("content").and_then(Value::as_str))
                    .map(str::to_string);
            }
            "completed" => summary.completed += 1,
            "cancelled" => summary.cancelled += 1,
            _ => {}
        }
    }
    summary
}

#[must_use]
pub fn board_items(todos: &[Value]) -> Vec<TodoBoardItem> {
    todos
        .iter()
        .enumerate()
        .filter_map(|(index, todo)| {
            let content = todo
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim();
            if content.is_empty() {
                return None;
            }
            let id = todo
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| (index + 1).to_string());
            let status = todo
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending")
                .to_string();
            let active_form = todo
                .get("active_form")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string);
            let priority = todo
                .get("priority")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string);
            Some(TodoBoardItem {
                id,
                status,
                content: content.to_string(),
                active_form,
                priority,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_two_in_progress_items() {
        let todos = vec![
            serde_json::json!({"content":"a","status":"in_progress"}),
            serde_json::json!({"content":"b","status":"in_progress"}),
        ];

        let err = validate_todo_items(&todos).expect_err("should reject duplicate active todos");

        assert!(err.to_string().contains("at most one"));
    }

    #[test]
    fn accepts_id_and_active_form_and_summarizes_active() {
        let todos = vec![
            serde_json::json!({"id":"a","content":"implement","active_form":"implementing","status":"in_progress"}),
            serde_json::json!({"id":"b","content":"verify","status":"pending"}),
            serde_json::json!({"id":"c","content":"done","status":"completed"}),
        ];

        validate_todo_items(&todos).expect("valid todos");
        let summary = summarize_todo_items(&todos);

        assert_eq!(summary.total, 3);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.in_progress, 1);
        assert_eq!(summary.active.as_deref(), Some("implementing"));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let todos = vec![
            serde_json::json!({"id":"same","content":"a","status":"pending"}),
            serde_json::json!({"id":"same","content":"b","status":"pending"}),
        ];

        let err = validate_todo_items(&todos).expect_err("should reject duplicate ids");

        assert!(err.to_string().contains("duplicate todo id"));
    }

    #[test]
    fn writes_project_todo_state_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let summary = write_todo_items(
            temp.path(),
            vec![serde_json::json!({"content":"ship","status":"completed"})],
        )
        .expect("write todos");

        assert_eq!(summary.completed, 1);
        assert!(todo_state_path(temp.path()).exists());
    }

    #[test]
    fn board_items_prefer_active_form_for_running_task() {
        let items = board_items(&[
            serde_json::json!({
                "id": "ship-ui",
                "content": "Ship UI",
                "active_form": "Shipping UI",
                "status": "in_progress",
                "priority": "high"
            }),
            serde_json::json!({"content": "Verify", "status": "pending"}),
        ]);

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "ship-ui");
        assert_eq!(items[0].display_text(), "Shipping UI");
        assert_eq!(items[0].priority.as_deref(), Some("high"));
        assert_eq!(items[1].id, "2");
    }
}
