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
//! This example connects to the Deribit testnet WebSocket API,
//! subscribes to trade data for BTC-PERPETUAL, and prints received trade ticks.
//!
//! # Environment Variables
//!
//! For authenticated streams (optional):
//! - `DERIBIT_TESTNET_API_KEY`: Your Deribit testnet API key
//! - `DERIBIT_TESTNET_API_SECRET`: Your Deribit testnet API secret
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin ws_data
//! ```

use std::env;

use futures_util::StreamExt;
use nautilus_cryptography::providers::install_cryptographic_provider;
use nautilus_deribit::{
    http::{client::DeribitHttpClient, models::DeribitCurrency},
    websocket::client::DeribitWebSocketClient,
};
use nautilus_model::identifiers::InstrumentId;
use tokio::{pin, signal};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    install_cryptographic_provider();
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .init();
    let args: Vec<String> = env::args().collect();
    let is_testnet = args.iter().any(|a| a == "--testnet");

    tracing::info!(
        "Starting Deribit WebSocket data example ({})",
        if is_testnet { "testnet" } else { "mainnet" }
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

    // Create WebSocket client for public data (no auth required for market data)
    // Must match HTTP client's is_testnet setting for instrument consistency
    let mut ws_client = DeribitWebSocketClient::new_public(is_testnet)?;
    ws_client.cache_instruments(instruments);
    tracing::info!("Connecting to Deribit WebSocket...");
    ws_client.connect().await?;
    tracing::info!("Connected to Deribit WebSocket");

    // Subscribe to trades for BTC-PERPETUAL
    let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
    tracing::info!("Subscribing to trades for {instrument_id}");
    ws_client.subscribe_trades(instrument_id).await?;

    // Optional: Subscribe to other data types
    // ws_client.subscribe_book(instrument_id).await?;
    // ws_client.subscribe_ticker(instrument_id).await?;
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
