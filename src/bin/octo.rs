#[tokio::main]
async fn main() {
    let result = if std::env::args_os().len() == 1 {
        deepseek_code::tui::run_tui(None, false, None, None).await
    } else {
        deepseek_code::cli_entry::run().await
    };

    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
