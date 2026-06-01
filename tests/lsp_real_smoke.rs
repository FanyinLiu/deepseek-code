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

/// Run the pool over `src/main.rs` and return just the error messages.
async fn pool_errors(
    pool: &mut LspDiagnosticsPool,
    dir: &std::path::Path,
    config: &LspConfig,
) -> Vec<String> {
    pool.diagnostics(dir, &["src/main.rs".to_string()], config)
        .await
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .filter(deepseek_code::lsp::client::Diagnostic::is_error)
        .map(|d| d.message)
        .collect()
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

/// Regression guard for the warm path: one pool/server reused across several
/// edits of the same file must report *fresh* diagnostics each time, not the
/// stale set from the first edit. This only works because we send didChange
/// (not a duplicate didOpen) plus didSave (so flycheck re-runs) per edit.
#[tokio::test]
#[ignore = "spawns real rust-analyzer; run explicitly"]
async fn pool_reflects_repeated_edits() {
    if !rust_analyzer_on_path() {
        eprintln!("SKIP: rust-analyzer not on PATH");
        return;
    }

    let dir = std::env::temp_dir().join(format!("octo_lsp_repeat_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    let main = dir.join("src").join("main.rs");

    let mut servers = HashMap::new();
    servers.insert("rust".to_string(), vec!["rust-analyzer".to_string()]);
    let config = LspConfig {
        enabled: true,
        servers,
    };
    let mut pool = LspDiagnosticsPool::new();

    // Edit 1: a hallucinated function -> caught.
    std::fs::write(&main, "fn main() { nope_not_real_one(); }\n").unwrap();
    let e1 = pool_errors(&mut pool, &dir, &config).await;
    assert!(
        e1.iter().any(|m| m.contains("nope_not_real_one")),
        "edit#1 should report the first hallucination; got: {e1:?}"
    );

    // Edit 2 (same server): fixed -> no errors, and crucially NOT the stale one.
    std::fs::write(&main, "fn main() { println!(\"ok\"); }\n").unwrap();
    let e2 = pool_errors(&mut pool, &dir, &config).await;
    assert!(e2.is_empty(), "edit#2 (fixed) should be clean; got: {e2:?}");

    // Edit 3 (same server): a DIFFERENT hallucination -> the new one, not stale.
    std::fs::write(&main, "fn main() { nope_not_real_three(); }\n").unwrap();
    let e3 = pool_errors(&mut pool, &dir, &config).await;
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        e3.iter().any(|m| m.contains("nope_not_real_three")),
        "edit#3 should report the new hallucination; got: {e3:?}"
    );
    assert!(
        !e3.iter().any(|m| m.contains("nope_not_real_one")),
        "edit#3 must not report the stale first-edit error; got: {e3:?}"
    );
}
