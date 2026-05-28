use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use crate::deepseek::ToolCall;

use super::dispatch::ToolDispatchConfig;

#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    pub project_root: PathBuf,
    pub dispatch_config: ToolDispatchConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolExecutionResult {
    pub success: bool,
    pub summary: String,
    pub content: String,
    pub changed_files: Vec<String>,
    pub duration_ms: u64,
}

pub trait ToolBackend {
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a ToolExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ToolExecutionResult> + Send + 'a>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalToolBackend;

impl ToolBackend for LocalToolBackend {
    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
        context: &'a ToolExecutionContext,
    ) -> Pin<Box<dyn Future<Output = ToolExecutionResult> + Send + 'a>> {
        Box::pin(async move {
            let start = std::time::Instant::now();
            let (content, is_error) = crate::tools::dispatch::execute_single_tool_with_config(
                call,
                &context.project_root,
                context.dispatch_config.clone(),
            )
            .await;
            let changed_files = changed_files_for_call(call);
            ToolExecutionResult {
                success: !is_error,
                summary: crate::agent::utils::truncate_for_summary(&content, 200),
                content,
                changed_files,
                duration_ms: start.elapsed().as_millis() as u64,
            }
        })
    }
}

pub(crate) fn changed_files_for_call(call: &ToolCall) -> Vec<String> {
    if !matches!(
        call.function.name.as_str(),
        "edit_file" | "write_file" | "notebook_edit" | "apply_patch"
    ) {
        return Vec::new();
    }

    let Ok(args) = serde_json::from_str::<serde_json::Value>(&call.function.arguments) else {
        return Vec::new();
    };
    if call.function.name == "apply_patch" {
        return args
            .get("patch")
            .and_then(serde_json::Value::as_str)
            .map(crate::workspace::apply::parse_patch_paths)
            .unwrap_or_default();
    }

    args.get("path")
        .and_then(serde_json::Value::as_str)
        .map(|path| vec![path.to_string()])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deepseek::{ToolCall, ToolCallFunction};

    #[tokio::test]
    async fn local_tool_backend_wraps_dispatch_result() {
        let temp = tempfile::tempdir().expect("tempdir");
        let call = ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "read_file".into(),
                arguments: r#"{"path":"missing.txt"}"#.into(),
            },
        };
        let context = ToolExecutionContext {
            project_root: temp.path().to_path_buf(),
            dispatch_config: ToolDispatchConfig::default(),
        };

        let result = LocalToolBackend.execute(&call, &context).await;

        assert!(!result.success);
        assert!(result.content.contains("not found") || result.content.contains("No such"));
    }

    #[test]
    fn changed_files_extracts_apply_patch_paths() {
        let call = ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "apply_patch".into(),
                arguments: serde_json::json!({
                    "patch": "*** Begin Patch\n*** Update File: src/lib.rs\n*** Add File: tests/new.rs\n*** End Patch\n"
                })
                .to_string(),
            },
        };

        assert_eq!(
            changed_files_for_call(&call),
            vec!["src/lib.rs".to_string(), "tests/new.rs".to_string()]
        );
    }

    #[test]
    fn changed_files_extracts_notebook_edit_path() {
        let call = ToolCall {
            id: "call-1".into(),
            call_type: "function".into(),
            function: ToolCallFunction {
                name: "notebook_edit".into(),
                arguments: serde_json::json!({
                    "path": "notebooks/analysis.ipynb",
                    "cell": "0",
                    "new_source": "print(1)"
                })
                .to_string(),
            },
        };

        assert_eq!(
            changed_files_for_call(&call),
            vec!["notebooks/analysis.ipynb".to_string()]
        );
    }
}
