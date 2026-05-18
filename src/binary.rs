use anyhow::Context;

/// Run the shared octocode/octo binary entrypoint.
pub fn run() {
    if let Err(error) = run_with_platform_stack() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn run_with_platform_stack() -> Result<(), anyhow::Error> {
    std::thread::Builder::new()
        .name("octocode-main".to_string())
        .stack_size(8 * 1024 * 1024)
        .spawn(run_async_entrypoint)
        .context("failed to spawn Windows CLI entry thread")?
        .join()
        .map_err(|panic| {
            anyhow::anyhow!("octocode main thread panicked: {}", panic_payload(panic))
        })?
}

#[cfg(not(windows))]
fn run_with_platform_stack() -> Result<(), anyhow::Error> {
    run_async_entrypoint()
}

fn run_async_entrypoint() -> Result<(), anyhow::Error> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to initialize async runtime")?;

    runtime.block_on(async {
        if std::env::args_os().len() == 1 {
            crate::tui::run_tui(None, false, None, None).await
        } else {
            crate::cli_entry::run().await
        }
    })
}

#[cfg(windows)]
fn panic_payload(panic: Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}
