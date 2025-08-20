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

// Enhanced WebSocket data testing script for debugging

use std::env;

use futures_util::StreamExt;
use nautilus_bitmex::websocket::client::BitmexWebSocketClient;
use nautilus_model::{data::bar::BarType, identifiers::InstrumentId};
use tokio::{
    pin, signal,
    time::{Duration, sleep},
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_level = env::var("LOG_LEVEL")
        .unwrap_or_else(|_| "INFO".to_string())
        .parse::<LevelFilter>()
        .unwrap_or(LevelFilter::INFO);

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let args: Vec<String> = env::args().collect();
    let subscription_type = args.get(1).map(String::as_str).unwrap_or("all");
    let symbol = args.get(2).map(String::as_str).unwrap_or("XBTUSD");
    let testnet = args.get(3).map(|s| s == "testnet").unwrap_or(false);

    tracing::info!("Starting Bitmex WebSocket test");
    tracing::info!("Subscription type: {subscription_type}");
    tracing::info!("Symbol: {symbol}");
    tracing::info!("Testnet: {testnet}");

    // Configure URL
    let url = if testnet {
        Some("wss://ws.testnet.bitmex.com/realtime".to_string())
    } else {
        None // Use default production URL
    };

    // Create client
    let mut client = BitmexWebSocketClient::new(
        url,     // url: defaults to wss://ws.bitmex.com/realtime
        None,    // No API key for public feeds
        None,    // No API secret
        Some(5), // 5 second heartbeat
    )
    .unwrap();

    tracing::info!("Connecting to WebSocket...");
    client.connect().await?;
    tracing::info!("Connected successfully");

    // Give the connection a moment to stabilize
    sleep(Duration::from_millis(500)).await;

    let instrument_id = InstrumentId::from(format!("{}.BITMEX", symbol).as_str());
    tracing::info!("Using instrument_id: {instrument_id}");

    match subscription_type {
        "quotes" => {
            tracing::info!("Subscribing to quotes for {instrument_id}");
            client.subscribe_quotes(instrument_id).await?;
        }
        "trades" => {
            tracing::info!("Subscribing to trades for {instrument_id}");
            client.subscribe_trades(instrument_id).await?;
        }
        "orderbook" | "book" => {
            tracing::info!("Subscribing to order book L2 for {instrument_id}");
            client.subscribe_book(instrument_id).await?;
        }
        "orderbook25" | "book25" => {
            tracing::info!("Subscribing to order book L2_25 for {instrument_id}");
            client.subscribe_book_25(instrument_id).await?;
        }
        "depth10" | "book10" => {
            tracing::info!("Subscribing to order book depth 10 for {instrument_id}");
            client.subscribe_book_depth10(instrument_id).await?;
        }
        "bars" => {
            let bar_type =
                BarType::from(format!("{}.BITMEX-1-MINUTE-LAST-EXTERNAL", symbol).as_str());
            tracing::info!("Subscribing to bars: {bar_type}");
            client.subscribe_bars(bar_type).await?;
        }
        "funding" => {
            tracing::info!("Subscribing to funding rates");
            // Note: This might need implementation
            tracing::warn!("Funding rate subscription may not be implemented yet");
        }
        "liquidation" => {
            tracing::info!("Subscribing to liquidations");
            // Note: This might need implementation
            tracing::warn!("Liquidation subscription may not be implemented yet");
        }
        "all" => {
            tracing::info!("Subscribing to all available data types for {instrument_id}",);

            tracing::info!("- Subscribing to quotes");
            if let Err(e) = client.subscribe_quotes(instrument_id).await {
                tracing::error!("Failed to subscribe to quotes: {e}");
            } else {
                tracing::info!("  ✓ Quotes subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to trades");
            if let Err(e) = client.subscribe_trades(instrument_id).await {
                tracing::error!("Failed to subscribe to trades: {e}");
            } else {
                tracing::info!("  ✓ Trades subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to order book L2");
            if let Err(e) = client.subscribe_book(instrument_id).await {
                tracing::error!("Failed to subscribe to order book: {e}");
            } else {
                tracing::info!("  ✓ Order book L2 subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to order book L2_25");
            if let Err(e) = client.subscribe_book_25(instrument_id).await {
                tracing::error!("Failed to subscribe to order book 25: {e}");
            } else {
                tracing::info!("  ✓ Order book L2_25 subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to order book depth 10");
            if let Err(e) = client.subscribe_book_depth10(instrument_id).await {
                tracing::error!("Failed to subscribe to depth 10: {e}");
            } else {
                tracing::info!("  ✓ Order book depth 10 subscription successful");
            }

            sleep(Duration::from_millis(100)).await;

            let bar_type =
                BarType::from(format!("{}.BITMEX-1-MINUTE-LAST-EXTERNAL", symbol).as_str());
            tracing::info!("- Subscribing to bars: {bar_type}");
            if let Err(e) = client.subscribe_bars(bar_type).await {
                tracing::error!("Failed to subscribe to bars: {e}");
            } else {
                tracing::info!("  ✓ Bars subscription successful");
            }
        }
        _ => {
            tracing::error!("Unknown subscription type: {subscription_type}");
            tracing::info!(
                "Available types: quotes, trades, orderbook, orderbook25, depth10, bars, funding, liquidation, all"
            );
            return Ok(());
        }
    }

    tracing::info!("Subscriptions completed, waiting for data...");
    tracing::info!("Press CTRL+C to stop");

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let stream = client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    // Use a flag to track if we should close
    let mut should_close = false;
    let mut message_count = 0u64;

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                message_count += 1;
                tracing::info!("[Message #{message_count}] {msg:?}");
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                should_close = true;
                break;
            }
            else => {
                tracing::warn!("Stream ended unexpectedly");
                break;
            }
        }
    }

    if should_close {
        tracing::info!("Total messages received: {message_count}");
        client.close().await?;
        tracing::info!("Connection closed successfully");
    }

    Ok(())
}
