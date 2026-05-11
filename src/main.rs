#[tokio::main]
async fn main() {
    if let Err(e) = deepseek_code::cli_entry::run().await {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
