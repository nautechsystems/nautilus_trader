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

//! Manual verification script for dYdX WebSocket public data streams.
//!
//! Exercises live subscriptions for trades, order book updates, and candles
//! for a single instrument to validate the end-to-end streaming pipeline.
//!
//! Usage:
//! ```bash
//! # Test against testnet (default) - subscribe to all data types
//! cargo run --bin dydx-ws-data -p nautilus-dydx
//!
//! # Subscribe to specific channel
//! cargo run --bin dydx-ws-data -p nautilus-dydx -- trades BTC-USD
//! cargo run --bin dydx-ws-data -p nautilus-dydx -- orderbook ETH-USD
//! cargo run --bin dydx-ws-data -p nautilus-dydx -- candles BTC-USD
//!
//! # Use testnet explicitly
//! cargo run --bin dydx-ws-data -p nautilus-dydx -- all BTC-USD testnet
//!
//! # Override endpoints via environment
//! DYDX_HTTP_URL=https://indexer.v4testnet.dydx.exchange \
//! DYDX_WS_URL=wss://indexer.v4testnet.dydx.exchange/v4/ws \
//! cargo run --bin dydx-ws-data -p nautilus-dydx
//! ```

use std::{env, time::Duration};

use nautilus_dydx::{
    common::consts::{DYDX_TESTNET_HTTP_URL, DYDX_TESTNET_WS_URL},
    http::client::DydxHttpClient,
    websocket::{client::DydxWebSocketClient, handler::HandlerCommand},
};
use nautilus_model::{
    data::{BarSpecification, BarType},
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::InstrumentId,
};
use tokio::{pin, signal};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let args: Vec<String> = env::args().collect();
    let subscription_type = args.get(1).map_or("all", String::as_str);
    let symbol = args.get(2).map_or("BTC-USD", String::as_str);
    let testnet = args.get(3).is_none_or(|s| s == "testnet");

    log::info!("Starting dYdX WebSocket test");
    log::info!("Subscription type: {subscription_type}");
    log::info!("Symbol: {symbol}");
    log::info!("Testnet: {testnet}");
    log::info!("");

    let (http_url, ws_url) = if testnet {
        (
            env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string()),
            env::var("DYDX_WS_URL").unwrap_or_else(|_| DYDX_TESTNET_WS_URL.to_string()),
        )
    } else {
        (
            env::var("DYDX_HTTP_URL").expect("DYDX_HTTP_URL required for mainnet"),
            env::var("DYDX_WS_URL").expect("DYDX_WS_URL required for mainnet"),
        )
    };

    let is_testnet = http_url.contains("testnet") || ws_url.contains("testnet");

    log::info!("Connecting to dYdX HTTP API: {http_url}");
    log::info!("Connecting to dYdX WebSocket: {ws_url}");
    log::info!(
        "Environment: {}",
        if is_testnet { "TESTNET" } else { "MAINNET" }
    );
    log::info!("");

    let http_client = DydxHttpClient::new(Some(http_url), Some(30), None, is_testnet, None)?;
    let instruments = http_client.request_instruments(None, None, None).await?;

    log::info!("Fetched {} instruments from HTTP", instruments.len());

    let instrument_id = InstrumentId::from(format!("{symbol}-PERP.DYDX"));

    log::info!("Using instrument: {instrument_id}");
    log::info!("");

    let mut ws_client = DydxWebSocketClient::new_public(ws_url, Some(30));
    ws_client.cache_instruments(instruments);

    ws_client.connect().await?;
    log::info!("WebSocket connected");
    log::info!("");

    tokio::time::sleep(Duration::from_millis(500)).await;

    match subscription_type {
        "trades" => {
            log::info!("Subscribing to trades for {instrument_id}");
            ws_client.subscribe_trades(instrument_id).await?;
        }
        "orderbook" | "book" => {
            log::info!("Subscribing to orderbook for {instrument_id}");
            ws_client.subscribe_orderbook(instrument_id).await?;
        }
        "candles" | "bars" => {
            let bar_spec = BarSpecification {
                step: std::num::NonZeroUsize::new(1).unwrap(),
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            };
            let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
            let ticker = instrument_id.symbol.as_str().trim_end_matches("-PERP");
            let topic = format!("{ticker}/1MIN");

            ws_client.send_command(HandlerCommand::RegisterBarType { topic, bar_type })?;

            log::info!("Subscribing to 1-minute candles for {instrument_id}");
            ws_client.subscribe_candles(instrument_id, "1MIN").await?;
        }
        "all" => {
            log::info!("Subscribing to all available data types for {instrument_id}");
            log::info!("");

            log::info!("- Subscribing to trades");
            if let Err(e) = ws_client.subscribe_trades(instrument_id).await {
                log::error!("Failed to subscribe to trades: {e}");
            } else {
                log::info!("  Trades subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            log::info!("- Subscribing to orderbook");
            if let Err(e) = ws_client.subscribe_orderbook(instrument_id).await {
                log::error!("Failed to subscribe to orderbook: {e}");
            } else {
                log::info!("  Orderbook subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            let bar_spec = BarSpecification {
                step: std::num::NonZeroUsize::new(1).unwrap(),
                aggregation: BarAggregation::Minute,
                price_type: PriceType::Last,
            };
            let bar_type = BarType::new(instrument_id, bar_spec, AggregationSource::External);
            let ticker = instrument_id.symbol.as_str().trim_end_matches("-PERP");
            let topic = format!("{ticker}/1MIN");

            ws_client.send_command(HandlerCommand::RegisterBarType { topic, bar_type })?;

            log::info!("- Subscribing to 1-minute candles");
            if let Err(e) = ws_client.subscribe_candles(instrument_id, "1MIN").await {
                log::error!("Failed to subscribe to candles: {e}");
            } else {
                log::info!("  Candles subscription successful");
            }
        }
        _ => {
            log::error!("Unknown subscription type: {subscription_type}");
            log::info!("Available types: trades, orderbook, candles, all");
            return Ok(());
        }
    }

    log::info!("");
    log::info!("Subscriptions completed, waiting for data...");
    log::info!("Press CTRL+C to stop");
    log::info!("");

    let Some(mut rx) = ws_client.take_receiver() else {
        log::warn!("No inbound WebSocket receiver available; exiting");
        return Ok(());
    };

    let sigint = signal::ctrl_c();
    pin!(sigint);

    let mut message_count: u64 = 0;
    let mut should_close = false;

    loop {
        tokio::select! {
            _ = &mut sigint => {
                log::info!("Received SIGINT, closing connection...");
                should_close = true;
                break;
            }
            maybe_msg = rx.recv() => {
                match maybe_msg {
                    Some(msg) => {
                        message_count += 1;
                        log::info!("[Message #{message_count}] {msg:?}");
                    }
                    None => {
                        log::info!("WebSocket message stream closed");
                        break;
                    }
                }
            }
        }
    }

    if should_close {
        log::info!("");
        log::info!("Total messages received: {message_count}");
        ws_client.disconnect().await?;
        log::info!("Connection closed successfully");
    }

    Ok(())
}
