use std::path::PathBuf;

use crate::deepseek::{ChatMessage, ChatRequest, ThinkingConfig};
use crate::provider::{build_provider, Provider};
use crate::storage;

/// Run the doctor command: check connectivity, auth, model availability.
pub async fn doctor(project_root: Option<PathBuf>) -> Result<(), anyhow::Error> {
    println!("DeepSeek-Code Doctor\n");
    let root = project_root.or_else(storage::find_project_root);
    let config = storage::Config::load(root.as_deref())?;

    // 1. Check API key
    let api_key = if let Some(key) = storage::get_effective_api_key(root.as_deref()) {
        println!("✅ API key found");
        key
    } else {
        println!("No API key configured — let's set it up first.\n");
        super::login::prompt_and_store_api_key(root.as_deref())?
    };

    // 2. Check connectivity
    let provider = build_provider(&config.provider, api_key.clone());
    let client = provider.create_deepseek_client();
    println!("   Testing connectivity to api.deepseek.com...");

    match client.list_models().await {
        Ok(models) => {
            println!("✅ API reachable");
            if let Some(models) = models["data"].as_array() {
                let model_ids: Vec<&str> = models
                    .iter()
                    .filter_map(|m| m["id"].as_str())
                    .filter(|id| id.starts_with("deepseek"))
                    .collect();
                if !model_ids.is_empty() {
                    println!("   Available models:");
                    for m in model_ids {
                        println!("     - {m}");
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ API unreachable: {e}");
            return Ok(());
        }
    }

    // 3. Test chat call
    let test_model = config.model.default.canonical();
    println!("   Testing chat completion ({test_model})...");
    let req = ChatRequest {
        model: provider.request_model_name(&test_model),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: Some(crate::deepseek::models::ChatMessageContent::Text(
                "Say 'ok'".into(),
            )),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }],
        tools: None,
        thinking: Some(ThinkingConfig::disabled()),
        response_format: None,
        stream: false,
        max_tokens: Some(5),
    };

    match client.chat(&req).await {
        Ok(resp) => {
            let content = resp
                .choices
                .first()
                .and_then(|c| c.message.content.as_ref().and_then(|cc| cc.as_text()))
                .unwrap_or("");
            println!("✅ Chat response: {content}");

            if let Some(usage) = &resp.usage {
                println!(
                    "   Tokens: {} (prompt: {}, completion: {})",
                    usage.total_tokens, usage.prompt_tokens, usage.completion_tokens
                );
                if let (Some(hit), Some(miss)) = (
                    usage.prompt_cache_hit_tokens,
                    usage.prompt_cache_miss_tokens,
                ) {
                    let total = hit + miss;
                    let rate = if total > 0 {
                        f64::from(hit) / f64::from(total) * 100.0
                    } else {
                        0.0
                    };
                    println!("   Cache: hit={hit} miss={miss} rate={rate:.0}%");
                }
            }
        }
        Err(e) => {
            println!("❌ Chat test failed: {e}");
        }
    }

    // 4. Check config
    println!("   Checking configuration...");
    match storage::Config::load(root.as_deref()) {
        Ok(config) => {
            println!("✅ Configuration loaded");
            println!(
                "   Model: default={} heavy={}",
                config.model.default, config.model.heavy
            );
            println!("   Thinking: {}", config.model.thinking_mode);
            println!(
                "   Policy: auto_approve_safe_read={} network_access={}",
                config.policy.auto_approve_safe_read, config.policy.network_access
            );
        }
        Err(e) => {
            println!("⚠️  Config not loaded (using defaults): {e}");
        }
    }

    // 5. Check session store
    if let Some(home) = dirs::home_dir() {
        let store_dir = home.join(".deepseek-code").join("sessions");
        if store_dir.exists() {
            let count = std::fs::read_dir(&store_dir).map_or(0, std::iter::Iterator::count);
            println!(
                "✅ Session store: {} project(s) at {}",
                count,
                store_dir.display()
            );
        } else {
            println!("ℹ️  No session store yet (will be created on first use)");
        }
    }

    println!("\nDoctor check complete.");
    Ok(())
}
