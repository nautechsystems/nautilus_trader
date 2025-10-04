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

//! Example demonstrating Coinbase Advanced Trade WebSocket API

use anyhow::Result;
use nautilus_coinbase::websocket::{Channel, CoinbaseWebSocketClient, WebSocketMessage};
use rustls;
use tracing::{info, warn};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize Rustls crypto provider
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Get API credentials from environment variables
    let api_key = std::env::var("COINBASE_API_KEY")
        .expect("COINBASE_API_KEY environment variable not set");
    let api_secret = std::env::var("COINBASE_API_SECRET")
        .expect("COINBASE_API_SECRET environment variable not set");

    info!("=== Coinbase Advanced Trade WebSocket API Test ===\n");

    // Create WebSocket client for market data
    let client = CoinbaseWebSocketClient::new_market_data(api_key.clone(), api_secret.clone());

    // Connect to WebSocket
    info!("1. Connecting to Coinbase WebSocket...");
    client.connect().await?;
    info!("   ✓ Connected successfully\n");

    // Subscribe to heartbeats to keep connection alive
    info!("2. Subscribing to heartbeats channel...");
    client.subscribe_heartbeats().await?;
    info!("   ✓ Subscribed to heartbeats\n");

    // Subscribe to ticker for BTC-USD, ETH-USD, SOL-USD
    info!("3. Subscribing to ticker channel...");
    let products = vec![
        "BTC-USD".to_string(),
        "ETH-USD".to_string(),
        "SOL-USD".to_string(),
    ];
    client.subscribe(products.clone(), Channel::Ticker).await?;
    info!("   ✓ Subscribed to ticker for BTC-USD, ETH-USD, SOL-USD\n");

    // Subscribe to candles for BTC-USD
    info!("4. Subscribing to candles channel...");
    client.subscribe(vec!["BTC-USD".to_string()], Channel::Candles).await?;
    info!("   ✓ Subscribed to candles for BTC-USD\n");

    // Subscribe to market trades for BTC-USD
    info!("5. Subscribing to market_trades channel...");
    client.subscribe(vec!["BTC-USD".to_string()], Channel::MarketTrades).await?;
    info!("   ✓ Subscribed to market_trades for BTC-USD\n");

    // Subscribe to level2 order book for BTC-USD
    info!("6. Subscribing to level2 channel...");
    client.subscribe(vec!["BTC-USD".to_string()], Channel::Level2).await?;
    info!("   ✓ Subscribed to level2 for BTC-USD\n");

    // Subscribe to status channel
    info!("7. Subscribing to status channel...");
    client.subscribe(products.clone(), Channel::Status).await?;
    info!("   ✓ Subscribed to status\n");

    // Subscribe to user channel (requires authentication)
    info!("8. Subscribing to user channel...");
    client.subscribe(vec![], Channel::User).await?;
    info!("   ✓ Subscribed to user channel\n");

    info!("=== Listening for WebSocket messages (press Ctrl+C to stop) ===\n");

    // Receive and process messages
    let mut heartbeat_count = 0;
    let mut ticker_count = 0;
    let mut candle_count = 0;
    let mut trade_count = 0;
    let mut level2_count = 0;
    let mut status_count = 0;
    let mut user_count = 0;

    loop {
        match client.receive_message().await? {
            Some(msg) => {
                // Try to parse as WebSocketMessage
                match serde_json::from_str::<WebSocketMessage>(&msg) {
                    Ok(ws_msg) => {
                        match ws_msg {
                            WebSocketMessage::Heartbeats { events } => {
                                heartbeat_count += 1;
                                if heartbeat_count <= 3 {
                                    info!("💓 Heartbeat #{}: counter={}",
                                        heartbeat_count,
                                        events.first().map(|e| e.heartbeat_counter).unwrap_or(0)
                                    );
                                }
                            }
                            WebSocketMessage::Ticker { events } => {
                                ticker_count += 1;
                                for event in &events {
                                    for ticker in &event.tickers {
                                        info!("📊 Ticker: {} = ${} (24h: {}%)",
                                            ticker.product_id,
                                            ticker.price,
                                            ticker.price_percent_chg_24_h
                                        );
                                    }
                                }
                            }
                            WebSocketMessage::Candles { events } => {
                                candle_count += 1;
                                for event in &events {
                                    for candle in &event.candles {
                                        info!("🕯️  Candle {}: O={} H={} L={} C={} V={}",
                                            candle.product_id,
                                            candle.open,
                                            candle.high,
                                            candle.low,
                                            candle.close,
                                            candle.volume
                                        );
                                    }
                                }
                            }
                            WebSocketMessage::MarketTrades { events } => {
                                trade_count += 1;
                                for event in &events {
                                    for trade in &event.trades {
                                        info!("💱 Trade {}: {} {} @ ${} ({})",
                                            trade.product_id,
                                            trade.side,
                                            trade.size,
                                            trade.price,
                                            trade.time
                                        );
                                    }
                                }
                            }
                            WebSocketMessage::Level2 { events } => {
                                level2_count += 1;
                                if level2_count <= 5 {
                                    for event in &events {
                                        info!("📖 Level2 {}: {} updates ({})",
                                            event.product_id,
                                            event.updates.len(),
                                            event.event_type
                                        );
                                    }
                                }
                            }
                            WebSocketMessage::Status { events } => {
                                status_count += 1;
                                if status_count <= 2 {
                                    for event in &events {
                                        info!("ℹ️  Status: {} products ({})",
                                            event.products.len(),
                                            event.event_type
                                        );
                                    }
                                }
                            }
                            WebSocketMessage::User { events } => {
                                user_count += 1;
                                for event in &events {
                                    info!("👤 User event: {} (orders: {}, positions: {})",
                                        event.event_type,
                                        event.orders.as_ref().map(|o| o.len()).unwrap_or(0),
                                        event.positions.as_ref().map(|p| p.len()).unwrap_or(0)
                                    );
                                }
                            }
                            WebSocketMessage::Subscriptions { events } => {
                                info!("✅ Subscription confirmed: {} events", events.len());
                            }
                            WebSocketMessage::TickerBatch { events } => {
                                info!("📊 Ticker batch: {} events", events.len());
                            }
                            WebSocketMessage::FuturesBalanceSummary { events } => {
                                info!("💰 Futures balance: {} events", events.len());
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse WebSocket message: {}", e);
                        warn!("Raw message: {}", &msg[..std::cmp::min(200, msg.len())]);
                    }
                }
            }
            None => {
                info!("WebSocket connection closed");
                break;
            }
        }

        // Print summary every 50 messages
        let total = heartbeat_count + ticker_count + candle_count + trade_count + level2_count + status_count + user_count;
        if total > 0 && total % 50 == 0 {
            info!("\n📈 Summary: {} total messages (💓{} 📊{} 🕯️{} 💱{} 📖{} ℹ️{} 👤{})\n",
                total, heartbeat_count, ticker_count, candle_count, trade_count, level2_count, status_count, user_count
            );
        }
    }

    // Disconnect
    info!("\nDisconnecting...");
    client.disconnect().await?;
    info!("✓ Disconnected\n");

    info!("=== Final Summary ===");
    info!("💓 Heartbeats: {}", heartbeat_count);
    info!("📊 Tickers: {}", ticker_count);
    info!("🕯️  Candles: {}", candle_count);
    info!("💱 Trades: {}", trade_count);
    info!("📖 Level2: {}", level2_count);
    info!("ℹ️  Status: {}", status_count);
    info!("👤 User: {}", user_count);

    Ok(())
}

