// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Manual smoke-test: Bullet WebSocket market data stream.
//!
//! Connects to the Bullet WS endpoint, subscribes to bookTicker + aggTrade +
//! markPrice for a symbol, and prints received messages for 30 seconds.
//!
//! Usage:
//!   BULLET_SYMBOL=SOL-USD cargo run --bin bullet-ws-data --features examples
//!
//! Optional env:
//!   BULLET_BASE_URL  — override base URL (default: testnet)
//!   BULLET_SYMBOL    — market symbol (default: SOL-USD)
//!
//! To see raw WS frames, set RUST_LOG=nautilus_bullet::websocket=debug

use nautilus_bullet::{
    http::client::BulletHttpClient,
    websocket::{client::BulletWebSocketClient, messages::ServerMessage},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::var("BULLET_BASE_URL")
        .unwrap_or_else(|_| "https://tradingapi.testnet.bullet.xyz".to_string());
    let symbol = std::env::var("BULLET_SYMBOL").unwrap_or_else(|_| "SOL-USD".to_string());

    // Derive WS URL from HTTP base URL
    let ws_url = base_url
        .replace("https://", "wss://")
        .replace("http://", "ws://")
        + "/ws";

    println!("HTTP: {base_url}");
    println!("WS:   {ws_url}");
    println!("Symbol: {symbol}");

    // Fetch exchange info to confirm symbol exists
    let http = BulletHttpClient::new(&base_url, 30, None)?;
    let info = http.exchange_info().await?;
    let sym_info = info.symbols.iter().find(|s| s.symbol == symbol);
    match sym_info {
        Some(s) => println!(
            "Symbol found: {} (market_id={})",
            s.symbol, s.market_id
        ),
        None => {
            let available: Vec<&str> = info.symbols.iter().map(|s| s.symbol.as_str()).collect();
            anyhow::bail!("Symbol '{symbol}' not found. Available: {available:?}");
        }
    }

    // Connect WebSocket
    println!("\nConnecting to WebSocket...");
    let client = BulletWebSocketClient::new(&ws_url);
    client.connect().await?;
    println!("Connected.");

    // Subscribe to streams
    client.subscribe_quotes_for_symbol(&symbol).await?;
    client.subscribe_trades_for_symbol(&symbol).await?;
    client.subscribe_mark_price_for_symbol(&symbol).await?;
    client.subscribe_book_for_symbol(&symbol).await?;
    println!(
        "Subscribed to bookTicker + aggTrade + markPrice + depth20 for {symbol}"
    );
    println!("Streaming for 30 seconds...\n");

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut msg_count = 0usize;

    loop {
        if tokio::time::Instant::now() >= deadline {
            break;
        }

        let timeout = tokio::time::sleep_until(deadline);
        tokio::select! {
            event = client.next_event() => {
                match event {
                    Some(msg) => {
                        msg_count += 1;
                        match &msg {
                            ServerMessage::BookTicker(t) => {
                                println!(
                                    "[bookTicker] {} bid={} ask={}",
                                    t.symbol, t.bid_price, t.ask_price
                                );
                            }
                            ServerMessage::AggTrade(t) => {
                                println!(
                                    "[aggTrade]   {} price={} qty={} maker={}",
                                    t.symbol, t.price, t.quantity, t.is_buyer_maker
                                );
                            }
                            ServerMessage::MarkPrice(m) => {
                                println!(
                                    "[markPrice]  {} mark={} funding={}",
                                    m.symbol, m.mark_price, m.funding_rate
                                );
                            }
                            ServerMessage::DepthUpdate(d) => {
                                println!(
                                    "[depth]      {} u={} bids={} asks={} mt={:?}",
                                    d.symbol,
                                    d.update_id,
                                    d.bids.len(),
                                    d.asks.len(),
                                    d.mt,
                                );
                            }
                            ServerMessage::Result(r) => {
                                if let Some(err) = &r.error {
                                    eprintln!("[error] {:?}", err);
                                } else {
                                    println!("[result] id={:?} result={:?}", r.id, r.result);
                                }
                            }
                            ServerMessage::OrderUpdate(o) => {
                                let c = o.order.common();
                                println!("[orderUpdate] {} id={} status={}", c.symbol, c.order_id, c.status);
                            }
                            ServerMessage::Unknown(v) => {
                                println!("[unknown] {}", serde_json::to_string(&v).unwrap_or_default());
                            }
                        }
                    }
                    None => {
                        println!("Connection closed.");
                        break;
                    }
                }
            }
            _ = timeout => break,
        }
    }

    println!("\nReceived {msg_count} messages in 30 seconds.");
    client.disconnect();
    Ok(())
}
