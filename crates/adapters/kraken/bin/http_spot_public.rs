use nautilus_kraken::http::spot::client::KrakenSpotHttpClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    log::info!("Kraken Spot HTTP client example (public data methods)");

    let _client = KrakenSpotHttpClient::default();

    log::info!("Client created successfully");
    log::info!("TODO: Implement request_instruments, request_bars, request_trades methods");
    log::info!("These methods will parse Kraken responses into Nautilus domain types");

    Ok(())
}
