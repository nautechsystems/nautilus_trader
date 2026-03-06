//! Example demonstrating Deribit private API usage.
//!
//! # Prerequisites
//!
//! Set environment variables with your Deribit API credentials:
//! - For mainnet: `DERIBIT_API_KEY` and `DERIBIT_API_SECRET`
//! - For testnet: `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET`

use nautilus_deribit::http::client::DeribitHttpClient;
use nautilus_model::identifiers::AccountId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let is_testnet = !std::env::args().any(|x| x == "--mainnet");
    let client = DeribitHttpClient::new_with_env(
        None,
        None,
        None,
        is_testnet,
        Some(30),
        None,
        None,
        None,
        None,
    )?;

    let account_id = AccountId::from("DERIBIT-001");

    // Fetch account state for all currencies
    println!("Fetching account state...");
    match client.request_account_state(account_id).await {
        Ok(account_state) => println!("{account_state:?}"),
        Err(e) => {
            eprintln!("✗ Failed to fetch account state: {e}");
            return Err(e.into());
        }
    }

    Ok(())
}
