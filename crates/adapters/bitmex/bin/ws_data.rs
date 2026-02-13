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

use std::{env, time::Duration};

use futures_util::StreamExt;
use nautilus_bitmex::{http::client::BitmexHttpClient, websocket::client::BitmexWebSocketClient};
use nautilus_model::{data::bar::BarType, identifiers::InstrumentId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let args: Vec<String> = env::args().collect();
    let subscription_type = args.get(1).map_or("all", String::as_str);
    let symbol = args.get(2).map_or("XBTUSD", String::as_str);
    let testnet = args.get(3).is_some_and(|s| s == "testnet");

    log::info!("Starting Bitmex WebSocket test");
    log::info!("Subscription type: {subscription_type}");
    log::info!("Symbol: {symbol}");
    log::info!("Testnet: {testnet}");

    // Configure URLs
    let (http_url, ws_url) = if testnet {
        (
            Some("https://testnet.bitmex.com".to_string()),
            Some("wss://ws.testnet.bitmex.com/realtime".to_string()),
        )
    } else {
        (None, None) // Use default production URLs
    };

    log::info!("Fetching instruments from HTTP API...");
    let http_client = BitmexHttpClient::new(
        http_url, // base_url
        None,     // api_key
        None,     // api_secret
        testnet,  // testnet
        Some(60), // timeout_secs
        None,     // max_retries
        None,     // retry_delay_ms
        None,     // retry_delay_max_ms
        None,     // recv_window_ms
        None,     // max_requests_per_second
        None,     // max_requests_per_minute
        None,     // proxy_url
    )
    .expect("Failed to create HTTP client");

    let instruments = http_client
        .request_instruments(true) // active_only
        .await?;

    log::info!("Fetched {} instruments", instruments.len());

    // Create WebSocket client
    let mut ws_client = BitmexWebSocketClient::new(
        ws_url,  // url: defaults to wss://ws.bitmex.com/realtime
        None,    // No API key for public feeds
        None,    // No API secret
        None,    // Account ID
        Some(5), // 5 second heartbeat
    )
    .unwrap();
    ws_client.cache_instruments(instruments);
    ws_client.connect().await?;

    // Give the connection a moment to stabilize
    tokio::time::sleep(Duration::from_millis(500)).await;

    let instrument_id = InstrumentId::from(format!("{symbol}.BITMEX"));
    log::info!("Using instrument_id: {instrument_id}");

    match subscription_type {
        "quotes" => {
            log::info!("Subscribing to quotes for {instrument_id}");
            ws_client.subscribe_quotes(instrument_id).await?;
        }
        "trades" => {
            log::info!("Subscribing to trades for {instrument_id}");
            ws_client.subscribe_trades(instrument_id).await?;
        }
        "orderbook" | "book" => {
            log::info!("Subscribing to order book L2 for {instrument_id}");
            ws_client.subscribe_book(instrument_id).await?;
        }
        "orderbook25" | "book25" => {
            log::info!("Subscribing to order book L2_25 for {instrument_id}");
            ws_client.subscribe_book_25(instrument_id).await?;
        }
        "depth10" | "book10" => {
            log::info!("Subscribing to order book depth 10 for {instrument_id}");
            ws_client.subscribe_book_depth10(instrument_id).await?;
        }
        "bars" => {
            let bar_type = BarType::from(format!("{symbol}.BITMEX-1-MINUTE-LAST-EXTERNAL"));
            log::info!("Subscribing to bars: {bar_type}");
            ws_client.subscribe_bars(bar_type).await?;
        }
        "funding" => {
            log::info!("Subscribing to funding rates");
            // Note: This might need implementation
            log::warn!("Funding rate subscription may not be implemented yet");
        }
        "liquidation" => {
            log::info!("Subscribing to liquidations");
            // Note: This might need implementation
            log::warn!("Liquidation subscription may not be implemented yet");
        }
        "all" => {
            log::info!("Subscribing to all available data types for {instrument_id}",);

            log::info!("- Subscribing to quotes");
            if let Err(e) = ws_client.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to quotes: {e}");
            } else {
                log::info!("  ✓ Quotes subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            log::info!("- Subscribing to trades");
            if let Err(e) = ws_client.subscribe_trades(instrument_id).await {
                log::error!("Failed to subscribe to trades: {e}");
            } else {
                log::info!("  ✓ Trades subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            log::info!("- Subscribing to order book L2");
            if let Err(e) = ws_client.subscribe_book(instrument_id).await {
                log::error!("Failed to subscribe to order book: {e}");
            } else {
                log::info!("  ✓ Order book L2 subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            log::info!("- Subscribing to order book L2_25");
            if let Err(e) = ws_client.subscribe_book_25(instrument_id).await {
                log::error!("Failed to subscribe to order book 25: {e}");
            } else {
                log::info!("  ✓ Order book L2_25 subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            log::info!("- Subscribing to order book depth 10");
            if let Err(e) = ws_client.subscribe_book_depth10(instrument_id).await {
                log::error!("Failed to subscribe to depth 10: {e}");
            } else {
                log::info!("  ✓ Order book depth 10 subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            let bar_type = BarType::from(format!("{symbol}.BITMEX-1-MINUTE-LAST-EXTERNAL"));
            log::info!("- Subscribing to bars: {bar_type}");
            if let Err(e) = ws_client.subscribe_bars(bar_type).await {
                log::error!("Failed to subscribe to bars: {e}");
            } else {
                log::info!("  ✓ Bars subscription successful");
            }
        }
        _ => {
            log::error!("Unknown subscription type: {subscription_type}");
            log::info!(
                "Available types: quotes, trades, orderbook, orderbook25, depth10, bars, funding, liquidation, all"
            );
            return Ok(());
        }
    }

    log::info!("Subscriptions completed, waiting for data...");
    log::info!("Press CTRL+C to stop");

    // Create a future that completes on CTRL+C
    let sigint = tokio::signal::ctrl_c();
    tokio::pin!(sigint);

    let stream = ws_client.stream();
    tokio::pin!(stream); // Pin the stream to allow polling in the loop

    // Use a flag to track if we should close
    let mut should_close = false;
    let mut message_count = 0u64;

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                message_count += 1;
                log::info!("[Message #{message_count}] {msg:?}");
            }
            _ = &mut sigint => {
                log::info!("Received SIGINT, closing connection...");
                should_close = true;
                break;
            }
            else => {
                log::warn!("Stream ended unexpectedly");
                break;
            }
        }
    }

    if should_close {
        log::info!("Total messages received: {message_count}");
        ws_client.close().await?;
        log::info!("Connection closed successfully");
    }

    Ok(())
}
