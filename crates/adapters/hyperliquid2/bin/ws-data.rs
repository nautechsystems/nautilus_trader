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

//! Hyperliquid WebSocket data feed client binary.

use std::sync::Arc;
use std::time::Duration;

use nautilus_hyperliquid::websocket::client::{HyperliquidWebSocketClient, MessageHandler};
use tokio::{signal, time::sleep};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    // Initialize TLS crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    tracing::info!("📡 Hyperliquid WebSocket Data Feed Client");
    tracing::info!("Press Ctrl+C to exit");

    // Create message handler that logs incoming data
    let handler: MessageHandler = Arc::new(|message| {
        match message.get("channel").and_then(|c| c.as_str()) {
            Some("allMids") => {
                if let Some(data) = message.get("data").and_then(|d| d.get("mids")) {
                    let mids_count = data.as_object().map(|o| o.len()).unwrap_or(0);
                    tracing::info!("💰 Received mid prices for {} assets", mids_count);
                }
            }
            Some("l2Book") => {
                if let Some(coin) = message.get("data").and_then(|d| d.get("coin")).and_then(|c| c.as_str()) {
                    tracing::info!("📚 Received L2 book update for {}", coin);
                }
            }
            Some("trades") => {
                if let Some(trades) = message.get("data").and_then(|d| d.as_array()) {
                    tracing::info!("🔄 Received {} trades", trades.len());
                    if let Some(first_trade) = trades.first() {
                        if let (Some(coin), Some(px), Some(sz)) = (
                            first_trade.get("coin").and_then(|c| c.as_str()),
                            first_trade.get("px").and_then(|p| p.as_str()),
                            first_trade.get("sz").and_then(|s| s.as_str())
                        ) {
                            tracing::debug!("   📊 {} trade: {} @ ${}", coin, sz, px);
                        }
                    }
                }
            }
            Some("subscriptionResponse") => {
                tracing::debug!("✅ Subscription confirmed");
            }
            Some(channel) => {
                tracing::debug!("📨 Received {} message", channel);
            }
            None => {
                tracing::debug!("📨 Received message without channel");
            }
        }
    });

    // Create WebSocket client
    let mut client = HyperliquidWebSocketClient::new(None, None)?;
    client.set_message_handler(handler);

    tracing::info!("🔌 Connecting to Hyperliquid WebSocket...");

    // Connect with retry logic
    match client.connect_with_retry(3).await {
        Ok(_) => {
            tracing::info!("✅ Connected successfully!");
        }
        Err(e) => {
            tracing::error!("❌ Failed to connect after retries: {}", e);
            return Ok(());
        }
    }

    // Subscribe to various data streams
    tracing::info!("📊 Setting up data subscriptions...");

    // Subscribe to all mid prices (quotes)
    if let Err(e) = client.subscribe_all_mids().await {
        tracing::error!("❌ Failed to subscribe to allMids: {}", e);
    } else {
        tracing::info!("✅ Subscribed to all mid prices");
    }

    // Subscribe to BTC order book
    if let Err(e) = client.subscribe_l2_book("BTC").await {
        tracing::error!("❌ Failed to subscribe to BTC L2 book: {}", e);
    } else {
        tracing::info!("✅ Subscribed to BTC order book");
    }

    // Subscribe to BTC trades
    if let Err(e) = client.subscribe_trades("BTC").await {
        tracing::error!("❌ Failed to subscribe to BTC trades: {}", e);
    } else {
        tracing::info!("✅ Subscribed to BTC trades");
    }

    // Subscribe to ETH trades
    if let Err(e) = client.subscribe_trades("ETH").await {
        tracing::error!("❌ Failed to subscribe to ETH trades: {}", e);
    } else {
        tracing::info!("✅ Subscribed to ETH trades");
    }

    tracing::info!("🎧 Listening for real-time market data...");

    // Set up CTRL+C handler
    let mut sigint = Box::pin(signal::ctrl_c());

    // Main event loop - monitor connection and wait for CTRL+C
    loop {
        tokio::select! {
            // Handle CTRL+C
            Ok(_) = sigint.as_mut() => {
                tracing::info!("🛑 Received CTRL+C, shutting down...");
                break;
            }
            // Monitor connection every 30 seconds
            _ = sleep(Duration::from_secs(30)) => {
                let connected = client.is_connected();
                let attempts = client.reconnect_attempts();
                let heartbeat = client.time_since_heartbeat().unwrap_or(Duration::from_secs(0));
                
                tracing::info!(
                    "📊 Status: Connected={}, Reconnect attempts={}, Heartbeat={:.1}s ago",
                    connected,
                    attempts,
                    heartbeat.as_secs_f64()
                );
                
                // Attempt reconnection if disconnected
                if !connected {
                    tracing::warn!("🔄 Connection lost, attempting to reconnect...");
                    if let Err(e) = client.connect_with_retry(3).await {
                        tracing::error!("❌ Reconnection failed: {}", e);
                        break;
                    } else {
                        tracing::info!("✅ Reconnected successfully!");
                    }
                }
            }
        }
    }

    // Cleanup
    tracing::info!("🧹 Disconnecting...");
    if let Err(e) = client.disconnect().await {
        tracing::error!("❌ Error during disconnect: {}", e);
    }

    tracing::info!("👋 WebSocket data feed client shutdown complete");
    Ok(())
}
