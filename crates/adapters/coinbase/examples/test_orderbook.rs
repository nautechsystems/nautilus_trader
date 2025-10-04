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

//! Example demonstrating order book management with Coinbase Advanced Trade API.

use std::env;

use anyhow::Result;
use nautilus_coinbase::{
    orderbook::OrderBook,
    websocket::{client::CoinbaseWebSocketClient, types::WebSocketMessage, Channel},
};
use tracing::{info, warn};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Initialize Rustls crypto provider (REQUIRED for WebSocket TLS)
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    info!("🚀 Coinbase Order Book Manager Example");
    info!("{}", "=".repeat(80));

    // Get API credentials from environment
    let api_key = env::var("COINBASE_API_KEY").expect("COINBASE_API_KEY must be set");
    let api_secret = env::var("COINBASE_API_SECRET").expect("COINBASE_API_SECRET must be set");

    // Create WebSocket client
    let client = CoinbaseWebSocketClient::new_market_data(api_key, api_secret);

    // Connect to WebSocket
    info!("🔌 Connecting to Coinbase WebSocket...");
    client.connect().await?;
    info!("✅ Connected!");

    // Subscribe to Level2 channel for BTC-USD
    let product_id = "BTC-USD";
    info!("📡 Subscribing to Level2 channel for {}...", product_id);
    client
        .subscribe(vec![product_id.to_string()], Channel::Level2)
        .await?;
    info!("✅ Subscribed!");

    // Create order book manager
    let mut orderbook = OrderBook::new(product_id.to_string());
    info!("📖 Order book manager created for {}", product_id);

    // Process messages
    info!("\n📨 Processing Level2 updates...\n");

    let mut snapshot_received = false;
    let mut update_count = 0;
    let max_updates = 20;

    loop {
        match client.receive_message().await? {
            Some(msg) => {
                match serde_json::from_str::<WebSocketMessage>(&msg) {
                    Ok(ws_msg) => {
                        if let WebSocketMessage::Level2 { events } = ws_msg {
                            for event in events {
                                // Process the event
                                orderbook.process_event(&event)?;

                                if event.event_type == "snapshot" {
                                    snapshot_received = true;
                                    info!("📸 Snapshot received!");
                                    info!("   Bids: {} levels", orderbook.bids.depth());
                                    info!("   Asks: {} levels", orderbook.asks.depth());

                                    // Display top 5 levels
                                    info!("\n   Top 5 Bids:");
                                    for (i, level) in orderbook.bids.top_levels(5).iter().enumerate() {
                                        info!(
                                            "   {}. ${} - {}",
                                            i + 1,
                                            level.price,
                                            level.size
                                        );
                                    }

                                    info!("\n   Top 5 Asks:");
                                    for (i, level) in orderbook.asks.top_levels(5).iter().enumerate() {
                                        info!(
                                            "   {}. ${} - {}",
                                            i + 1,
                                            level.price,
                                            level.size
                                        );
                                    }

                                    // Display market metrics
                                    if let Some(best_bid) = orderbook.best_bid() {
                                        info!("\n   Best Bid: ${}", best_bid);
                                    }
                                    if let Some(best_ask) = orderbook.best_ask() {
                                        info!("   Best Ask: ${}", best_ask);
                                    }
                                    if let Some(mid) = orderbook.mid_price() {
                                        info!("   Mid Price: ${}", mid);
                                    }
                                    if let Some(spread) = orderbook.spread() {
                                        info!("   Spread: ${}", spread);
                                    }
                                    if let Some(spread_bps) = orderbook.spread_bps() {
                                        info!("   Spread (bps): {:.2}", spread_bps);
                                    }
                                } else if snapshot_received {
                                    update_count += 1;

                                    if update_count <= 5 || update_count % 5 == 0 {
                                        info!(
                                            "📊 Update #{}: {} changes | Bids: {} | Asks: {} | Mid: ${} | Spread: {:.2} bps",
                                            update_count,
                                            event.updates.len(),
                                            orderbook.bids.depth(),
                                            orderbook.asks.depth(),
                                            orderbook.mid_price().unwrap_or_default(),
                                            orderbook.spread_bps().unwrap_or_default()
                                        );
                                    }

                                    if update_count >= max_updates {
                                        info!("\n✅ Processed {} updates. Stopping...", update_count);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse WebSocket message: {}", e);
                    }
                }
            }
            None => {
                info!("WebSocket connection closed");
                break;
            }
        }
    }

    // Final statistics
    info!("\n{}", "=".repeat(80));
    info!("📊 Final Order Book Statistics");
    info!("{}", "=".repeat(80));
    info!("Product: {}", orderbook.product_id);
    info!("Bid Levels: {}", orderbook.bids.depth());
    info!("Ask Levels: {}", orderbook.asks.depth());
    if let Some(best_bid) = orderbook.best_bid() {
        info!("Best Bid: ${}", best_bid);
    }
    if let Some(best_ask) = orderbook.best_ask() {
        info!("Best Ask: ${}", best_ask);
    }
    if let Some(mid) = orderbook.mid_price() {
        info!("Mid Price: ${}", mid);
    }
    if let Some(spread) = orderbook.spread() {
        info!("Spread: ${}", spread);
    }
    if let Some(spread_bps) = orderbook.spread_bps() {
        info!("Spread (bps): {:.2}", spread_bps);
    }

    // Disconnect
    info!("\n🔌 Disconnecting...");
    client.disconnect().await?;
    info!("✅ Disconnected!");

    info!("\n🎉 Order book example completed successfully!");

    Ok(())
}

