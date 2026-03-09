use std::env;

use nautilus_deribit::http::{client::DeribitHttpClient, models::DeribitCurrency};
use nautilus_model::identifiers::InstrumentId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let args: Vec<String> = env::args().collect();
    let is_testnet = args.iter().any(|a| a == "--testnet");

    // Create HTTP client
    let client = DeribitHttpClient::new(None, is_testnet, None, None, None, None, None)?;

    // Fetch BTC-PERPETUAL instrument
    log::info!("Fetching BTC-PERPETUAL instrument...");
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    let instrument = client.request_instrument(instrument_id).await?;
    println!("Single instrument:");
    println!("{instrument:?}\n");

    // Fetch BTC instruments
    log::info!("Fetching BTC instruments...");
    let instruments = client
        .request_instruments(DeribitCurrency::BTC, None)
        .await?;
    println!("First 2 instruments from BTC:");
    for (i, inst) in instruments.iter().take(2).enumerate() {
        let num = i + 1;
        println!("{num}. {inst:?}");
    }

    Ok(())
}
