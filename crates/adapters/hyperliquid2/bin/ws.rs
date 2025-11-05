// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Example: Hyperliquid WebSocket Client
//!
//! Demonstrates how to use the Hyperliquid WebSocket client for real-time data streams.

use nautilus_hyperliquid2::{
    common::HyperliquidWsChannel,
    websocket::Hyperliquid2WebSocketClient,
};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Hyperliquid WebSocket Client Example ===\n");

    // Create WebSocket client
    let client = Hyperliquid2WebSocketClient::new(None, false)?;

    // Connect to WebSocket
    println!("Connecting to Hyperliquid WebSocket...");
    client.connect().await?;
    println!("✓ Connected successfully\n");

    // Subscribe to all mids
    println!("Subscribing to all mids...");
    client.subscribe(HyperliquidWsChannel::AllMids).await?;
    println!("✓ Subscribed to all mids\n");

    // Subscribe to BTC trades
    println!("Subscribing to BTC trades...");
    client.subscribe(HyperliquidWsChannel::Trades {
        coin: "BTC".to_string(),
    }).await?;
    println!("✓ Subscribed to BTC trades\n");

    // Subscribe to BTC L2 book
    println!("Subscribing to BTC L2 book...");
    client.subscribe(HyperliquidWsChannel::L2Book {
        coin: "BTC".to_string(),
    }).await?;
    println!("✓ Subscribed to BTC L2 book\n");

    println!("Receiving messages (Ctrl+C to stop)...\n");

    // Receive messages (limit to 20 for example)
    let mut message_count = 0;
    while message_count < 20 {
        match client.receive().await {
            Ok(Some(message)) => {
                message_count += 1;
                println!("Message {}: {}", message_count, message);
            }
            Ok(None) => {
                // Ping/pong or empty message
                continue;
            }
            Err(e) => {
                eprintln!("Error receiving message: {}", e);
                break;
            }
        }
    }

    println!("\n=== Example completed ===");
    Ok(())
}
