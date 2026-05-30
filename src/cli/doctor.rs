use std::path::PathBuf;

use crate::deepseek::errors::DeepSeekError;
use crate::deepseek::models::FunctionDef;
use crate::deepseek::{ChatMessage, ChatRequest, ResponseFormat, ThinkingConfig, ToolDefinition};
use crate::provider::{build_provider, Provider};
use crate::storage;

#[derive(Debug, Clone, Copy, Default)]
pub struct DoctorOptions {
    pub smoke: bool,
    pub no_network: bool,
}

/// Run the doctor command: check provider auth, connectivity, and configuration.
pub async fn doctor(
    project_root: Option<PathBuf>,
    options: DoctorOptions,
) -> Result<(), anyhow::Error> {
    for line in doctor_no_network_lines(project_root.clone())? {
        println!("{line}");
    }
    let root = project_root.or_else(storage::find_project_root);
    let config = storage::Config::load(root.as_deref())?;
    let provider = build_provider(&config.provider, String::new());
    let kind = provider.kind();
    let test_model = config.model.default.canonical();
    let provider_model = provider.request_model_name(&test_model);
    if options.no_network {
        println!("ℹ️  Network checks skipped by --no-network.");
        println!("\nDoctor check complete.");
        return Ok(());
    }

    let Some(api_key) = storage::get_effective_api_key(root.as_deref()) else {
        println!(
            "❌ API key missing: set {} or run `octo login --api-key <key>`.",
            storage::api_key_env_hint(kind)
        );
        println!("Doctor stopped before network checks.");
        return Ok(());
    };
    println!("✅ API key found");

    let provider = build_provider(&config.provider, api_key);
    let client = provider.create_deepseek_client();

    println!("   Checking model list endpoint...");
    match client.list_models().await {
        Ok(models) => {
            println!("✅ Model list reachable");
            if let Some(models) = models["data"].as_array() {
                let model_ids: Vec<&str> = models
                    .iter()
                    .filter_map(|m| m["id"].as_str())
                    .take(8)
                    .collect();
                if !model_ids.is_empty() {
                    println!("   Available models:");
                    for model in model_ids {
                        println!("     - {model}");
                    }
                }
            }
        }
        Err(error) => {
            println!(
                "⚠️  Model list skipped/failed: {}",
                diagnostic_error_hint(&error)
            );
        }
    }

    run_chat_basic(&client, &provider_model).await;
    if options.smoke {
        run_stream_basic(&client, &provider_model).await;
        run_tool_call_basic(&client, &provider_model).await;
        run_thinking_basic(&client, &provider_model).await;
        run_json_output_basic(&client, &provider_model).await;
    } else {
        println!("ℹ️  Extra smoke checks skipped. Run `octo doctor --smoke` to test stream/tool/thinking/json.");
    }

    println!("\nDoctor check complete.");
    Ok(())
}

pub fn doctor_no_network_lines(
    project_root: Option<PathBuf>,
) -> Result<Vec<String>, anyhow::Error> {
    let root = project_root.or_else(storage::find_project_root);
    let config = storage::Config::load(root.as_deref())?;
    let provider = build_provider(&config.provider, String::new());
    let kind = provider.kind();
    let capabilities = kind.capabilities();
    let test_model = config.model.default.canonical();
    let provider_model = provider.request_model_name(&test_model);
    let mut lines = vec![
        "Octocode Doctor".to_string(),
        String::new(),
        format!("Provider: {} ({})", kind.as_str(), kind.display_name()),
        format!(
            "Base URL: {}",
            provider.base_url().unwrap_or("native provider default")
        ),
        format!("Model: {provider_model}"),
        format!("API key env: {}", storage::api_key_env_hint(kind)),
        format!("Profile verified: {}", capabilities.last_verified),
        format!(
            "Capabilities: tools={} thinking={} cache={} context={}",
            yes_no(capabilities.supports_tool_calls),
            yes_no(capabilities.thinking.supports_thinking),
            yes_no(capabilities.supports_prompt_cache),
            capabilities
                .context_window_tokens
                .map(|tokens| tokens.to_string())
                .unwrap_or_else(|| "local-budget".to_string())
        ),
    ];
    if capabilities.requires_endpoint_override || capabilities.requires_model_override {
        lines.push(format!(
            "⚠️  Provider requires explicit override: endpoint={} model={}",
            yes_no(capabilities.requires_endpoint_override),
            yes_no(capabilities.requires_model_override)
        ));
    }
    if let Some(ref root) = root {
        let status = crate::cli::features::status_payload(root);
        lines.push(String::new());
        lines.push("Local capabilities".to_string());
        lines.push(format!(
            "  Custom slash commands: {}",
            status.custom_slash_commands_count
        ));
        lines.push(format!("  Skills loaded: {}", status.skills_count));
        lines.push(format!(
            "  Worktree isolation: {}",
            yes_no(status.worktree_isolation_available)
        ));
        lines.push(format!(
            "  Configured hook events: {}",
            status.configured_hook_events
        ));
        if status.custom_agents_count > 0 {
            lines.push(format!("  Custom agents: {}", status.custom_agents_count));
        }
    }
    Ok(lines)
}

async fn run_chat_basic(client: &crate::deepseek::client::DeepSeekClient, model: &str) {
    println!("   Smoke chat_basic...");
    let req = base_request(model, "Say ok.", false);
    match client.chat(&req).await {
        Ok(resp) => {
            let content = resp
                .choices
                .first()
                .and_then(|choice| {
                    choice
                        .message
                        .content
                        .as_ref()
                        .and_then(|content| content.as_text())
                })
                .unwrap_or("");
            println!("✅ chat_basic: {content}");
            if let Some(usage) = &resp.usage {
                println!(
                    "   Tokens: {} (prompt: {}, completion: {})",
                    usage.total_tokens, usage.prompt_tokens, usage.completion_tokens
                );
                if let (Some(hit), Some(miss)) = (
                    usage.prompt_cache_hit_tokens,
                    usage.prompt_cache_miss_tokens,
                ) {
                    println!("   Cache: hit={hit} miss={miss}");
                }
            }
        }
        Err(error) => println!("❌ chat_basic failed: {}", diagnostic_error_hint(&error)),
    }
}

async fn run_stream_basic(client: &crate::deepseek::client::DeepSeekClient, model: &str) {
    println!("   Smoke stream_basic...");
    let req = base_request(model, "Say ok.", true);
    match client.chat_stream_accumulated(&req).await {
        Ok(result) => println!("✅ stream_basic: {}", one_line(&result.content)),
        Err(error) => println!("❌ stream_basic failed: {}", diagnostic_error_hint(&error)),
    }
}

async fn run_tool_call_basic(client: &crate::deepseek::client::DeepSeekClient, model: &str) {
    println!("   Smoke tool_call_basic...");
    let mut req = base_request(model, "Use the echo tool with value ok.", false);
    req.tools = Some(vec![ToolDefinition {
        tool_type: "function".to_string(),
        function: FunctionDef {
            name: "echo".to_string(),
            description: "Echo a short value.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                },
                "required": ["value"]
            }),
        },
    }]);
    match client.chat(&req).await {
        Ok(resp) => {
            let tool_calls = resp
                .choices
                .first()
                .and_then(|choice| choice.message.tool_calls.as_ref())
                .map_or(0, Vec::len);
            println!("✅ tool_call_basic: request accepted; tool_calls={tool_calls}");
        }
        Err(error) => println!(
            "❌ tool_call_basic failed: {}",
            diagnostic_error_hint(&error)
        ),
    }
}

async fn run_thinking_basic(client: &crate::deepseek::client::DeepSeekClient, model: &str) {
    println!("   Smoke thinking_basic...");
    let mut req = base_request(model, "Think briefly, then say ok.", false);
    req.thinking = Some(ThinkingConfig::enabled());
    match client.chat(&req).await {
        Ok(resp) => {
            let has_reasoning = resp
                .choices
                .first()
                .and_then(|choice| choice.message.reasoning_content.as_ref())
                .is_some();
            println!("✅ thinking_basic: request accepted; reasoning_content={has_reasoning}");
        }
        Err(error) => println!(
            "❌ thinking_basic failed: {}",
            diagnostic_error_hint(&error)
        ),
    }
}

async fn run_json_output_basic(client: &crate::deepseek::client::DeepSeekClient, model: &str) {
    println!("   Smoke json_output_basic...");
    let mut req = base_request(model, r#"Return {"status":"ok"} as JSON."#, false);
    req.response_format = Some(ResponseFormat::json_object());
    match client.chat(&req).await {
        Ok(_) => println!("✅ json_output_basic: request accepted"),
        Err(error) => println!(
            "❌ json_output_basic failed: {}",
            diagnostic_error_hint(&error)
        ),
    }
}

fn base_request(model: &str, prompt: &str, stream: bool) -> ChatRequest {
    ChatRequest {
        model: model.to_string(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: Some(crate::deepseek::models::ChatMessageContent::Text(
                prompt.into(),
            )),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        tools: None,
        thinking: Some(ThinkingConfig::disabled()),
        response_format: None,
        stream,
        max_tokens: Some(32),
        temperature: None,
    }
}

fn diagnostic_error_hint(error: &DeepSeekError) -> String {
    match error {
        DeepSeekError::Unauthorized => {
            "auth failed; verify the provider key and env var for the active provider".to_string()
        }
        DeepSeekError::PaymentRequired => {
            "quota or balance is insufficient for this provider account".to_string()
        }
        DeepSeekError::BadRequest(body) | DeepSeekError::Unprocessable(body) => {
            format!("request rejected; check model name, base URL, and provider-specific fields: {body}")
        }
        DeepSeekError::RateLimited(_) => {
            "rate limited; retry later or lower concurrency".to_string()
        }
        DeepSeekError::ServerError(status) => {
            format!("provider server error HTTP {status}; retry later")
        }
        DeepSeekError::Network(error) => {
            format!("network failure; check base URL, proxy, DNS, and firewall: {error}")
        }
        other => other.to_string(),
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
