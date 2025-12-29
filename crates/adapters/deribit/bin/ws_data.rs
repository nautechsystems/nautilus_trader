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

//! Example binary demonstrating Deribit WebSocket data streaming.
//!
//! This example connects to the Deribit WebSocket API and subscribes to trade data.
//! Supports both aggregated (100ms) and raw data streams.
//!
//! # Environment Variables
//!
//! For raw streams (required):
//! - `DERIBIT_TESTNET_API_KEY`: Your Deribit testnet API key
//! - `DERIBIT_TESTNET_API_SECRET`: Your Deribit testnet API secret
//!
//! For mainnet raw streams:
//! - `DERIBIT_API_KEY`: Your Deribit mainnet API key
//! - `DERIBIT_API_SECRET`: Your Deribit mainnet API secret
//!
//! # Usage
//!
//! ```bash
//! # Aggregated 100ms streams (no auth required)
//! cargo run -p nautilus-deribit --bin deribit-ws-data
//!
//! # Raw streams (requires auth)
//! cargo run -p nautilus-deribit --bin deribit-ws-data -- --raw --testnet
//! ```

use std::env;

use futures_util::StreamExt;
use nautilus_deribit::{
    http::{client::DeribitHttpClient, models::DeribitCurrency},
    websocket::{client::DeribitWebSocketClient, enums::DeribitUpdateInterval},
};
use nautilus_model::identifiers::InstrumentId;
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .init();

    let args: Vec<String> = env::args().collect();
    let is_testnet = args.iter().any(|a| a == "--testnet");
    let use_raw = args.iter().any(|a| a == "--raw");

    tracing::info!(
        "Starting Deribit WebSocket data example ({}, {})",
        if is_testnet { "testnet" } else { "mainnet" },
        if use_raw { "raw" } else { "100ms" }
    );

    // Fetch instruments via HTTP to get proper instrument metadata
    let http_client = DeribitHttpClient::new(
        None, // base_url
        is_testnet, None, // timeout_secs
        None, // max_retries
        None, // retry_delay_ms
        None, // retry_delay_max_ms
        None, // proxy_url
    )?;
    tracing::info!("Fetching BTC instruments from Deribit...");
    let instruments = http_client
        .request_instruments(DeribitCurrency::BTC, None)
        .await?;
    tracing::info!("Fetched {} instruments", instruments.len());

    // Create WebSocket client based on whether raw streams are requested
    let mut ws_client = if use_raw {
        tracing::info!("Creating authenticated client for raw streams");
        DeribitWebSocketClient::with_credentials(is_testnet)?
    } else {
        tracing::info!("Creating public client for 100ms streams");
        DeribitWebSocketClient::new_public(is_testnet)?
    };

    ws_client.cache_instruments(instruments);
    tracing::info!("Connecting to Deribit WebSocket...");
    ws_client.connect().await?;
    tracing::info!("Connected to Deribit WebSocket");

    // Authenticate if using raw streams
    if use_raw {
        tracing::info!("Authenticating WebSocket connection for raw streams...");
        ws_client.authenticate_session().await?;
        tracing::info!("Authentication successful");
    }

    // Set interval based on mode
    let interval = if use_raw {
        Some(DeribitUpdateInterval::Raw)
    } else {
        None // Uses default 100ms
    };

    // Subscribe to trades for BTC-PERPETUAL
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    tracing::info!(
        "Subscribing to trades for {instrument_id} (interval: {})",
        interval.map_or("100ms", |i| i.as_str())
    );
    ws_client.subscribe_trades(instrument_id, interval).await?;

    // Optional: Subscribe to other data types
    // ws_client.subscribe_book(instrument_id, interval).await?;
    // ws_client.subscribe_ticker(instrument_id, interval).await?;
    // ws_client.subscribe_quotes(instrument_id).await?;

    // Create a future that completes on CTRL+C
    let sigint = signal::ctrl_c();
    pin!(sigint);

    let stream = ws_client.stream();
    tokio::pin!(stream);

    tracing::info!("Listening for market data... Press Ctrl+C to exit");

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                tracing::info!("{msg:?}");
            }
            _ = &mut sigint => {
                tracing::info!("Received SIGINT, closing connection...");
                ws_client.close().await?;
                break;
            }
            else => break,
        }
    }

    tracing::info!("Deribit WebSocket example finished");
    Ok(())
}
