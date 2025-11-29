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
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let log_level = env::var("LOG_LEVEL")
        .unwrap_or_else(|_| "INFO".to_string())
        .parse::<LevelFilter>()
        .unwrap_or(LevelFilter::INFO);

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let args: Vec<String> = env::args().collect();
    let subscription_type = args.get(1).map_or("all", String::as_str);
    let symbol = args.get(2).map_or("BTC-USD", String::as_str);
    let testnet = args.get(3).is_none_or(|s| s == "testnet");

    tracing::info!("Starting dYdX WebSocket test");
    tracing::info!("Subscription type: {subscription_type}");
    tracing::info!("Symbol: {symbol}");
    tracing::info!("Testnet: {testnet}");
    tracing::info!("");

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

    tracing::info!("Connecting to dYdX HTTP API: {http_url}");
    tracing::info!("Connecting to dYdX WebSocket: {ws_url}");
    tracing::info!(
        "Environment: {}",
        if is_testnet { "TESTNET" } else { "MAINNET" }
    );
    tracing::info!("");

    let http_client = DydxHttpClient::new(Some(http_url), Some(30), None, is_testnet, None)?;
    let instruments = http_client.request_instruments(None, None, None).await?;

    tracing::info!("Fetched {} instruments from HTTP", instruments.len());

    let instrument_id = InstrumentId::from(format!("{symbol}-PERP.DYDX").as_str());

    tracing::info!("Using instrument: {instrument_id}");
    tracing::info!("");

    let mut ws_client = DydxWebSocketClient::new_public(ws_url, Some(30));
    ws_client.cache_instruments(instruments);

    ws_client.connect().await?;
    tracing::info!("WebSocket connected");
    tracing::info!("");

    tokio::time::sleep(Duration::from_millis(500)).await;

    match subscription_type {
        "trades" => {
            tracing::info!("Subscribing to trades for {instrument_id}");
            ws_client.subscribe_trades(instrument_id).await?;
        }
        "orderbook" | "book" => {
            tracing::info!("Subscribing to orderbook for {instrument_id}");
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

            tracing::info!("Subscribing to 1-minute candles for {instrument_id}");
            ws_client.subscribe_candles(instrument_id, "1MIN").await?;
        }
        "all" => {
            tracing::info!("Subscribing to all available data types for {instrument_id}");
            tracing::info!("");

            tracing::info!("- Subscribing to trades");
            if let Err(e) = ws_client.subscribe_trades(instrument_id).await {
                tracing::error!("Failed to subscribe to trades: {e}");
            } else {
                tracing::info!("  Trades subscription successful");
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            tracing::info!("- Subscribing to orderbook");
            if let Err(e) = ws_client.subscribe_orderbook(instrument_id).await {
                tracing::error!("Failed to subscribe to orderbook: {e}");
            } else {
                tracing::info!("  Orderbook subscription successful");
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

            tracing::info!("- Subscribing to 1-minute candles");
            if let Err(e) = ws_client.subscribe_candles(instrument_id, "1MIN").await {
                tracing::error!("Failed to subscribe to candles: {e}");
            } else {
                tracing::info!("  Candles subscription successful");
            }
        }
        _ => {
            tracing::error!("Unknown subscription type: {subscription_type}");
            tracing::info!("Available types: trades, orderbook, candles, all");
            return Ok(());
        }
    }

    tracing::info!("");
    tracing::info!("Subscriptions completed, waiting for data...");
    tracing::info!("Press CTRL+C to stop");
    tracing::info!("");

    let Some(mut rx) = ws_client.take_receiver() else {
        tracing::warn!("No inbound WebSocket receiver available; exiting");
        return Ok(());
    };

    let sigint = signal::ctrl_c();
    pin!(sigint);

    let mut message_count: u64 = 0;
    let mut should_close = false;

    loop {
        tokio::select! {
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                should_close = true;
                break;
            }
            maybe_msg = rx.recv() => {
                match maybe_msg {
                    Some(msg) => {
                        message_count += 1;
                        tracing::info!("[Message #{message_count}] {msg:?}");
                    }
                    None => {
                        tracing::info!("WebSocket message stream closed");
                        break;
                    }
                }
            }
        }
    }

    if should_close {
        tracing::info!("");
        tracing::info!("Total messages received: {message_count}");
        ws_client.disconnect().await?;
        tracing::info!("Connection closed successfully");
    }

    Ok(())
}
