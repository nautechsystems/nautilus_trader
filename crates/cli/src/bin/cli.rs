use clap::Parser;
use nautilus_cli::opt::NautilusCli;
use nautilus_common::logging::ensure_logging_initialized;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    ensure_logging_initialized();

    if let Err(e) = nautilus_cli::run(NautilusCli::parse()).await {
        log::error!("Error executing Nautilus CLI: {e}");
    }
}
