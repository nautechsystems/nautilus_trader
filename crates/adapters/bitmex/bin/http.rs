use std::str::FromStr;

use nautilus_bitmex::http::client::BitmexHttpClient;
use nautilus_model::identifiers::InstrumentId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let client = BitmexHttpClient::from_env()?;

    let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX")?;
    let instrument = client.request_instrument(instrument_id).await?;

    match instrument {
        Some(inst) => log::info!("Retrieved instrument: {inst:?}"),
        None => log::warn!("Instrument XBTUSD.BITMEX not returned from BitMEX"),
    }

    Ok(())
}
