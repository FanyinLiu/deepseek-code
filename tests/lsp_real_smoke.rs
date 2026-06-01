//! Empirical end-to-end check of the post-edit LSP path: does the real
//! `LspDiagnosticsPool` catch hallucinated APIs and bad imports via a real
//! rust-analyzer? Spawns rust-analyzer, so it's `#[ignore]`d (not run in CI).
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

/// Build a minimal cargo project containing `main_src`, run the real pool over
/// it with rust-analyzer, and return the error-diagnostic messages.
async fn error_messages_for(slug: &str, main_src: &str) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("octo_lsp_{slug}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("src").join("main.rs"), main_src).unwrap();

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
    eprintln!("[{slug}] pool.diagnostics() returned after {:?}", t0.elapsed());

    let mut errors = Vec::new();
    for (file, diags) in &results {
        for d in diags {
            eprintln!("[{slug}] {file}: {d}");
            if d.is_error() {
                errors.push(d.message.clone());
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    errors
}

#[tokio::test]
#[ignore = "spawns real rust-analyzer; run explicitly"]
async fn pool_catches_hallucinated_rust_api() {
    if !rust_analyzer_on_path() {
        eprintln!("SKIP: rust-analyzer not on PATH");
        return;
    }

    let errors = error_messages_for(
        "api",
        "fn main() {\n    let s = String::new();\n    s.totally_fake_method_xyz();\n    definitely_not_a_real_function_abc();\n}\n",
    )
    .await;

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

#[tokio::test]
#[ignore = "spawns real rust-analyzer; run explicitly"]
async fn pool_catches_bad_import() {
    if !rust_analyzer_on_path() {
        eprintln!("SKIP: rust-analyzer not on PATH");
        return;
    }

    // Two flavours of "wrong import": a bad path inside std, and a crate that
    // isn't a dependency at all.
    let errors = error_messages_for(
        "import",
        "use std::collections::DefinitelyNotAMap;\nuse totally_fake_crate::Thing;\n\nfn main() {\n    let _ = DefinitelyNotAMap::new();\n    let _ = Thing;\n}\n",
    )
    .await;

    assert!(
        errors.iter().any(|m| m.to_lowercase().contains("unresolved import")
            || m.contains("DefinitelyNotAMap")),
        "expected an unresolved-import error for the bad std path; got: {errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|m| m.contains("totally_fake_crate")
                || m.to_lowercase().contains("unresolved")),
        "expected an error for the non-dependency crate import; got: {errors:?}"
    );
}
