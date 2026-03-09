use nautilus_kraken::http::spot::client::KrakenSpotRawHttpClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    log::info!("Kraken Spot HTTP client example");

    let client = KrakenSpotRawHttpClient::default();

    log::info!("Fetching server time...");
    let server_time = client.get_server_time().await?;
    log::info!("Server time: {server_time:?}");

    log::info!("Fetching system status...");
    let status = client.get_system_status().await?;
    log::info!("System status: {status:?}");

    log::info!("Fetching asset pairs for BTC/USD...");
    let pairs = client
        .get_asset_pairs(Some(vec!["XBTUSDT".to_string()]))
        .await?;
    log::info!("Asset pairs count: {}", pairs.len());

    log::info!("Fetching ticker for BTC/USD...");
    let ticker = client.get_ticker(vec!["XBTUSDT".to_string()]).await?;
    log::info!("Ticker count: {}", ticker.len());

    log::info!("HTTP client example completed successfully");

    Ok(())
}
