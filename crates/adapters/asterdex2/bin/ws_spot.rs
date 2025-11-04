// Example: Asterdex WebSocket Spot Market Data
//
// Demonstrates how to use the Asterdex WebSocket client to subscribe to spot market data streams.

use nautilus_asterdex2::common::enums::AsterdexWsChannel;
use nautilus_asterdex2::websocket::AsterdexWebSocketClient;
use tokio::time::{sleep, Duration};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Asterdex WebSocket Spot Market Example ===\n");

    // Create WebSocket client
    let client = AsterdexWebSocketClient::new(None, None);

    // Connect to spot market streams
    println!("Connecting to Asterdex spot WebSocket...");
    client.connect(true).await?;
    println!("✓ Connected\n");

    // Subscribe to aggregate trades for BTCUSDT
    println!("Subscribing to BTCUSDT aggregate trades...");
    let channel = AsterdexWsChannel::SpotAggTrade {
        symbol: "BTCUSDT".to_string(),
    };
    client.subscribe(channel).await?;
    println!("✓ Subscribed\n");

    // Subscribe to order book depth
    println!("Subscribing to BTCUSDT order book (5 levels)...");
    let depth_channel = AsterdexWsChannel::SpotDepth {
        symbol: "BTCUSDT".to_string(),
        levels: Some(5),
    };
    client.subscribe(depth_channel).await?;
    println!("✓ Subscribed\n");

    // Receive messages for 10 seconds
    println!("Receiving messages for 10 seconds...\n");
    let start = std::time::Instant::now();
    let mut message_count = 0;

    while start.elapsed() < Duration::from_secs(10) {
        match client.receive().await {
            Ok(Some(message)) => {
                message_count += 1;
                println!("[{}] Message: {}", message_count, message);
            }
            Ok(None) => {
                // No message (ping/pong or connection closed)
                sleep(Duration::from_millis(100)).await;
            }
            Err(e) => {
                eprintln!("✗ Error receiving message: {}", e);
                break;
            }
        }
    }

    println!("\nReceived {} messages", message_count);

    // Disconnect
    println!("\nDisconnecting...");
    client.disconnect().await?;
    println!("✓ Disconnected");

    println!("\n=== Example completed ===");
    Ok(())
}
