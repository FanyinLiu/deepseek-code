//! Empirical end-to-end check of the post-edit LSP path: does the real
//! `LspDiagnosticsPool` catch a hallucinated API via a real rust-analyzer?
//! Spawns rust-analyzer, so it's `#[ignore]`d (not run in CI).
//! Run: cargo test --test lsp_real_smoke -- --ignored --nocapture

use std::collections::HashMap;

use deepseek_code::lsp::pool::LspDiagnosticsPool;
use deepseek_code::storage::config::LspConfig;

fn rust_analyzer_on_path() -> bool {
    std::process::Command::new("rust-analyzer")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tokio::test]
#[ignore = "spawns real rust-analyzer; run explicitly"]
async fn pool_catches_hallucinated_rust_api() {
    if !rust_analyzer_on_path() {
        eprintln!("SKIP: rust-analyzer not on PATH");
        return;
    }

    // Minimal cargo project with a hallucinated method AND a hallucinated fn.
    let dir = std::env::temp_dir().join(format!("octo_lsp_pool_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src").join("main.rs"),
        "fn main() {\n    let s = String::new();\n    s.totally_fake_method_xyz();\n    definitely_not_a_real_function_abc();\n}\n",
    )
    .unwrap();

    let mut servers = HashMap::new();
    servers.insert("rust".to_string(), vec!["rust-analyzer".to_string()]);
    let config = LspConfig {
        enabled: true,
        servers,
    };

    let mut pool = LspDiagnosticsPool::new();
    let t0 = std::time::Instant::now();
    let results = pool
        .diagnostics(&dir, &["src/main.rs".to_string()], &config)
        .await;
    eprintln!("pool.diagnostics() returned after {:?}", t0.elapsed());

    let mut errors = Vec::new();
    for (file, diags) in &results {
        for d in diags {
            eprintln!("{file}: {d}");
            if d.is_error() {
                errors.push(d.message.clone());
            }
        }
    }

    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        errors.iter().any(|m| m.contains("totally_fake_method_xyz")),
        "expected an error for the hallucinated method; got: {errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|m| m.contains("definitely_not_a_real_function_abc")),
        "expected an error for the hallucinated function; got: {errors:?}"
    );
}
