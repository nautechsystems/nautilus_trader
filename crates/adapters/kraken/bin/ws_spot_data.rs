//! Connects to the Kraken public WebSocket feed and streams market data.
//! Useful when manually validating the Rust WebSocket client implementation.

use futures_util::StreamExt;
use nautilus_kraken::{
    config::KrakenDataClientConfig,
    websocket::spot_v2::{client::KrakenSpotWebSocketClient, enums::KrakenWsChannel},
};
use tokio::{pin, signal};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    nautilus_common::logging::ensure_logging_initialized();

    let config = KrakenDataClientConfig::default();
    let token = CancellationToken::new();

    let mut client = KrakenSpotWebSocketClient::new(config, token.clone());

    client.connect().await?;

    client
        .subscribe(KrakenWsChannel::Ticker, vec![Ustr::from("BTC/USD")], None)
        .await?;

    client
        .subscribe(KrakenWsChannel::Trade, vec![Ustr::from("BTC/USD")], None)
        .await?;

    let stream = client.stream()?;
    let shutdown = signal::ctrl_c();
    pin!(stream);
    pin!(shutdown);

    log::info!("Streaming Kraken market data; press Ctrl+C to exit");

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                log::info!("Received: {msg:#?}");
            }
            _ = &mut shutdown => {
                log::info!("Received Ctrl+C, closing connection");
                client.disconnect().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}
