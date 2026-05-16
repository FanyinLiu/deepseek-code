use std::path::PathBuf;

use crate::agent::router::{ComplexityRouter, Route};
use crate::cli::resolve_project_root;
use crate::provider::{build_provider, Provider};
use crate::storage;

/// Assess a task's complexity and print the routing decision (debug/diagnostics).
pub async fn assess(task: String, project_root: Option<PathBuf>) -> Result<(), anyhow::Error> {
    let root = resolve_project_root(project_root, "assess")?;

    let config = storage::Config::load(Some(&root))?;
    let router_config = config.router.clone();

    let api_key = storage::get_effective_api_key(Some(&root));
    let has_api_key = api_key.is_some();
    let client = api_key.map(|key| {
        let provider = build_provider(&config.provider, key);
        provider.create_deepseek_client()
    });

    let router = if router_config.enabled && has_api_key {
        ComplexityRouter::from(&router_config)
    } else {
        ComplexityRouter::new().rules_only()
    };

    println!("Task: {task}");
    println!(
        "Router config: enabled={}, conservative={}, use_model={}",
        router_config.enabled, router_config.conservative, router_config.use_model_classifier
    );
    if !has_api_key {
        println!("Note: No API key found — running in rules-only mode.");
    }
    println!();

    let project_rules = crate::agent::prompt_builder::load_project_rules(&root);
    let assessment = router
        .assess(&task, project_rules.as_deref(), client.as_ref())
        .await;

    println!("{}", "━".repeat(56));
    println!();
    println!("Route:        {:?}", assessment.route);
    println!("Label:        {:?}", assessment.complexity_label);
    println!("Score:        {}", assessment.score);
    println!("Confidence:   {:.2}", assessment.confidence);
    println!("Version:      {}", assessment.classifier_version);
    println!();
    println!("Reason codes:");
    for code in &assessment.reason_codes {
        println!("  • {:?} — {}", code, code.user_text());
    }
    println!();
    println!("Predicted:");
    println!("  Write files: {}", assessment.predicted_write_files);
    println!("  Commands:    {}", assessment.predicted_commands);
    println!("  Duration:    {} ms", assessment.predicted_duration_ms);
    println!();

    if !assessment.risk_flags.is_empty() {
        println!("Risk flags:");
        for flag in &assessment.risk_flags {
            println!("  ⚠ {flag:?}");
        }
        println!();
    }

    if let Some(ref model) = assessment.model_name {
        println!("Model:        {model}");
        println!("Latency:      {} ms", assessment.latency_ms);
        println!();
    }

    println!("Explanation:  {}", assessment.explanation);
    println!();

    match assessment.route {
        Route::DirectExecute => {
            println!("→ This task would be executed directly.");
        }
        Route::PlanReview => {
            println!("→ This task would enter plan review mode first.");
        }
        Route::Clarify => {
            println!("→ This task needs clarification before proceeding.");
        }
    }
    println!();
    println!("{}", "━".repeat(56));

    Ok(())
}
