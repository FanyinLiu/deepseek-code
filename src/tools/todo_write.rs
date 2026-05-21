//! TodoWrite — session-level multi-step task tracker.
//!
//! Design (from public tool specs for TodoWrite-style behavior):
//! - The model overwrites the FULL list each call. This forces it to re-evaluate
//!   the whole plan rather than mindlessly appending.
//! - Each item carries both `content` ("Read config.toml", imperative) and
//!   `active_form` ("Reading config.toml", present-continuous) so the UI can
//!   show a nice "what I'm doing right now" line.
//! - At most one `in_progress` at a time. Enforced here.
//!
//! Storage: a process-wide `OnceLock<Mutex<TodoStore>>` so `dispatch.rs` stays
//! stateless. If you later add real session state, swap this out for a
//! `&mut SessionState.todos`.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub content: String,
    pub active_form: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TodoStore {
    pub items: Vec<Todo>,
}

#[derive(Debug, Deserialize)]
pub struct TodoWriteArgs {
    pub todos: Vec<Todo>,
}

static STORE: OnceLock<Mutex<TodoStore>> = OnceLock::new();

fn store() -> &'static Mutex<TodoStore> {
    STORE.get_or_init(|| Mutex::new(TodoStore::default()))
}

/// Apply a new todo list (replaces existing). Returns human-readable rendering
/// for the model to see post-write state.
pub fn todo_write(args: TodoWriteArgs) -> Result<String> {
    validate(&args.todos)?;
    let mut s = store().lock().map_err(|_| anyhow!("todo store poisoned"))?;
    s.items = args.todos;
    Ok(render(&s))
}

/// Read-only snapshot, useful for status bar / UI rendering.
pub fn snapshot() -> Result<TodoStore> {
    let s = store().lock().map_err(|_| anyhow!("todo store poisoned"))?;
    Ok(s.clone())
}

/// Clear the list (call between sessions if you have a notion of "new session").
pub fn reset() {
    if let Some(s) = STORE.get() {
        if let Ok(mut g) = s.lock() {
            g.items.clear();
        }
    }
}

fn validate(todos: &[Todo]) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    let mut in_progress = 0;
    for t in todos {
        if t.id.trim().is_empty() {
            return Err(anyhow!("todo has empty id"));
        }
        if !ids.insert(&t.id) {
            return Err(anyhow!("duplicate todo id: {}", t.id));
        }
        if t.content.trim().is_empty() {
            return Err(anyhow!("todo {} has empty content", t.id));
        }
        if t.active_form.trim().is_empty() {
            return Err(anyhow!("todo {} has empty active_form", t.id));
        }
        if matches!(t.status, TodoStatus::InProgress) {
            in_progress += 1;
        }
    }
    if in_progress > 1 {
        return Err(anyhow!(
            "at most one todo may be in_progress (found {in_progress})"
        ));
    }
    Ok(())
}

fn render(store: &TodoStore) -> String {
    if store.items.is_empty() {
        return "Todo list is empty.".to_string();
    }
    let mut out = String::from("Todo list:\n");
    for t in &store.items {
        let mark = match t.status {
            TodoStatus::Pending => "[ ]",
            TodoStatus::InProgress => "[~]",
            TodoStatus::Completed => "[x]",
        };
        let label = match t.status {
            TodoStatus::InProgress => &t.active_form,
            _ => &t.content,
        };
        out.push_str(&format!("  {} {} ({})\n", mark, label, t.id));
    }
    let done = store
        .items
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    out.push_str(&format!("\n{} / {} completed", done, store.items.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, status: TodoStatus) -> Todo {
        Todo {
            id: id.into(),
            content: format!("Task {id}"),
            active_form: format!("Doing {id}"),
            status,
        }
    }

    #[test]
    fn rejects_two_in_progress() {
        let err = validate(&[
            t("a", TodoStatus::InProgress),
            t("b", TodoStatus::InProgress),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("in_progress"));
    }

    #[test]
    fn rejects_duplicate_id() {
        let err =
            validate(&[t("a", TodoStatus::Pending), t("a", TodoStatus::Pending)]).unwrap_err();
        assert!(err.to_string().contains("duplicate"));
    }

    #[test]
    fn happy_path_renders_progress() {
        let out = todo_write(TodoWriteArgs {
            todos: vec![
                t("a", TodoStatus::Completed),
                t("b", TodoStatus::InProgress),
                t("c", TodoStatus::Pending),
            ],
        })
        .unwrap();
        assert!(out.contains("1 / 3 completed"));
        assert!(out.contains("Doing b"));
        reset();
    }
}
