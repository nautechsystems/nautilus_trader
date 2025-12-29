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

//! Example demonstrating live data testing with the Binance adapter.
//!
//! This example connects to Binance Futures WebSocket and subscribes to market data streams.
//!
//! Run with: `cargo run --example node-data-tester --package nautilus-binance`
//!
//! Environment variables (optional):
//! - `BINANCE_API_KEY`: API key for authenticated endpoints
//! - `BINANCE_API_SECRET`: API secret for request signing

use futures_util::StreamExt;
use nautilus_binance::{
    common::enums::{BinanceEnvironment, BinanceProductType},
    futures::{
        http::client::BinanceFuturesHttpClient,
        websocket::{client::BinanceFuturesWebSocketClient, messages::NautilusFuturesWsMessage},
    },
};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    dotenvy::dotenv().ok();

    let api_key = std::env::var("BINANCE_API_KEY").ok();
    let api_secret = std::env::var("BINANCE_API_SECRET").ok();

    tracing::info!("Fetching instruments from Binance Futures API...");
    let http_client = BinanceFuturesHttpClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Mainnet,
        api_key.clone(),
        api_secret.clone(),
        None, // base_url_override
        None, // recv_window
        None, // timeout_secs
        None, // proxy_url
    )?;

    let instruments = http_client.instruments().await?;
    tracing::info!(count = instruments.len(), "Fetched instruments");

    tracing::info!("Creating Binance Futures WebSocket client...");
    let mut ws_client = BinanceFuturesWebSocketClient::new(
        BinanceProductType::UsdM,
        BinanceEnvironment::Mainnet,
        api_key,
        api_secret,
        None, // url_override
        None, // heartbeat
    )?;

    ws_client.cache_instruments(instruments);

    tracing::info!("Connecting to Binance Futures WebSocket...");
    ws_client.connect().await?;

    tracing::info!("Subscribing to market data streams...");
    let streams = vec![
        "btcusdt@aggTrade".to_string(),
        "ethusdt@aggTrade".to_string(),
        "btcusdt@bookTicker".to_string(),
        "ethusdt@bookTicker".to_string(),
    ];
    ws_client.subscribe(streams).await?;

    tracing::info!("Listening for messages (Ctrl+C to stop)...");

    let stream = ws_client.stream();
    tokio::pin!(stream);

    let mut message_count = 0u64;
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            Some(msg) = stream.next() => {
                message_count += 1;

                match msg {
                    NautilusFuturesWsMessage::Data(data_vec) => {
                        for data in data_vec {
                            tracing::info!(message_count, "Data: {data:?}");
                        }
                    }
                    NautilusFuturesWsMessage::Deltas(deltas) => {
                        tracing::info!(
                            message_count,
                            instrument_id = %deltas.instrument_id,
                            num_deltas = deltas.deltas.len(),
                            "OrderBook deltas"
                        );
                    }
                    NautilusFuturesWsMessage::Error(err) => {
                        tracing::error!(code = err.code, msg = %err.msg, "WebSocket error");
                    }
                    NautilusFuturesWsMessage::Reconnected => {
                        tracing::warn!("WebSocket reconnected");
                    }
                    NautilusFuturesWsMessage::Instrument(inst) => {
                        tracing::info!("Instrument: {inst:?}");
                    }
                    NautilusFuturesWsMessage::RawJson(json) => {
                        tracing::debug!("Raw JSON: {json}");
                    }
                }

                if message_count.is_multiple_of(100) {
                    let elapsed = start_time.elapsed().as_secs_f64();
                    let rate = message_count as f64 / elapsed;
                    tracing::info!(
                        message_count,
                        elapsed_secs = format!("{elapsed:.1}"),
                        rate = format!("{rate:.1}/s"),
                        "Statistics"
                    );
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received Ctrl+C, shutting down...");
                break;
            }
        }
    }

    ws_client.close().await?;

    let elapsed = start_time.elapsed().as_secs_f64();
    tracing::info!(
        total_messages = message_count,
        elapsed_secs = format!("{elapsed:.1}"),
        avg_rate = format!("{:.1}/s", message_count as f64 / elapsed),
        "Final statistics"
    );

    Ok(())
}
