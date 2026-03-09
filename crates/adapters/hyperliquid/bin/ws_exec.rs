use std::{env, time::Duration};

use nautilus_hyperliquid::{common::consts::ws_url, websocket::client::HyperliquidWebSocketClient};
use tokio::{pin, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let args: Vec<String> = env::args().collect();
    let testnet = args.get(1).is_some_and(|s| s == "testnet");

    log::info!("Starting Hyperliquid WebSocket execution example");
    log::info!("Testnet: {testnet}");

    let ws_url = ws_url(testnet);
    log::info!("WebSocket URL: {ws_url}");

    let mut client = HyperliquidWebSocketClient::new(Some(ws_url.to_string()), testnet, None);
    client.connect().await?;
    log::info!("Connected to Hyperliquid WebSocket");

    // Subscribe to execution channels
    let user_addr = env::var("HYPERLIQUID_USER_ADDRESS")
        .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string());

    // Subscribe to all user channels using the convenience method
    client.subscribe_all_user_channels(&user_addr).await?;
    log::info!("Subscribed to all user channels for {user_addr}");

    // Wait briefly to ensure subscriptions are active
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    loop {
        tokio::select! {
            Some(message) = client.next_event() => {
                log::debug!("{message:?}");
            }
            _ = &mut sigint => {
                log::info!("Received SIGINT, closing connection...");
                client.disconnect().await?;
                break;
            }
            else => break,
        }
    }

    Ok(())
}
