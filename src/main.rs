use kie_mcp::{cli, kie::client::redact};

#[tokio::main]
async fn main() {
    if let Err(err) = cli::run().await {
        eprintln!("Error: {}", redact(&format!("{err:#}")));
        std::process::exit(1);
    }
}
