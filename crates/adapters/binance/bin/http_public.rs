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

//! Test binary for Binance Spot HTTP client with SBE encoding.
//!
//! This binary tests the public endpoints against the live Binance API
//! to verify that SBE request/response handling works correctly.
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin binance-spot-http-public --package nautilus-binance
//! ```

use nautilus_binance::{
    common::enums::BinanceEnvironment,
    spot::http::{BinanceSpotHttpClient, DepthParams, TradesParams},
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting Binance Spot SBE HTTP client test");
    tracing::info!(
        "Using SBE schema version: {}:{}",
        BinanceSpotHttpClient::schema_id(),
        BinanceSpotHttpClient::schema_version()
    );

    // Create client (no credentials needed for public endpoints)
    let client = BinanceSpotHttpClient::new(
        BinanceEnvironment::Mainnet,
        None,     // api_key
        None,     // api_secret
        None,     // base_url_override
        None,     // recv_window
        Some(30), // timeout_secs
        None,     // proxy_url
    )?;

    // Test 1: Ping
    tracing::info!("=== Test 1: Ping ===");
    match client.ping().await {
        Ok(()) => tracing::info!("Ping successful"),
        Err(e) => tracing::error!("Ping failed: {e}"),
    }

    // Test 2: Server Time
    // Note: SBE returns microsecond timestamps
    tracing::info!("=== Test 2: Server Time ===");
    match client.server_time().await {
        Ok(timestamp_us) => {
            let timestamp_ms = timestamp_us / 1000;
            let datetime = chrono::DateTime::from_timestamp_millis(timestamp_ms).map_or_else(
                || "invalid timestamp".to_string(),
                |dt| dt.format("%Y-%m-%d %H:%M:%S%.3f UTC").to_string(),
            );
            tracing::info!("Server time: {timestamp_us} µs ({datetime})");
        }
        Err(e) => tracing::error!("Server time failed: {e}"),
    }

    // Test 3: Depth (Order Book)
    tracing::info!("=== Test 3: Depth (BTCUSDT) ===");
    let depth_params = DepthParams::new("BTCUSDT").with_limit(5);
    match client.depth(&depth_params).await {
        Ok(depth) => {
            tracing::info!("Last update ID: {}", depth.last_update_id);
            tracing::info!(
                "Price exponent: {}, Qty exponent: {}",
                depth.price_exponent,
                depth.qty_exponent
            );
            tracing::info!("Bids ({} levels):", depth.bids.len());
            for (i, level) in depth.bids.iter().take(5).enumerate() {
                let price = level.price_f64(depth.price_exponent);
                let qty = level.qty_f64(depth.qty_exponent);
                tracing::info!("  [{i}] Price: {price:.2}, Qty: {qty:.8}");
            }
            tracing::info!("Asks ({} levels):", depth.asks.len());
            for (i, level) in depth.asks.iter().take(5).enumerate() {
                let price = level.price_f64(depth.price_exponent);
                let qty = level.qty_f64(depth.qty_exponent);
                tracing::info!("  [{i}] Price: {price:.2}, Qty: {qty:.8}");
            }
        }
        Err(e) => tracing::error!("Depth failed: {e}"),
    }

    // Test 4: Recent Trades
    tracing::info!("=== Test 4: Trades (BTCUSDT) ===");
    let trades_params = TradesParams::new("BTCUSDT").with_limit(5);
    match client.trades(&trades_params).await {
        Ok(trades) => {
            tracing::info!(
                "Price exponent: {}, Qty exponent: {}",
                trades.price_exponent,
                trades.qty_exponent
            );
            tracing::info!("Trades ({} total):", trades.trades.len());
            for trade in trades.trades.iter().take(5) {
                let price = trade.price_f64(trades.price_exponent);
                let qty = trade.qty_f64(trades.qty_exponent);
                let side = if trade.is_buyer_maker { "SELL" } else { "BUY" };
                let datetime = chrono::DateTime::from_timestamp_millis(trade.time).map_or_else(
                    || "?".to_string(),
                    |dt| dt.format("%H:%M:%S%.3f").to_string(),
                );
                tracing::info!(
                    "  ID: {}, {side} {qty:.8} @ {price:.2} at {datetime}",
                    trade.id
                );
            }
        }
        Err(e) => tracing::error!("Trades failed: {e}"),
    }

    // Test 5: Depth for another symbol (ETHUSDT)
    tracing::info!("=== Test 5: Depth (ETHUSDT) ===");
    let depth_params = DepthParams::new("ETHUSDT").with_limit(3);
    match client.depth(&depth_params).await {
        Ok(depth) => {
            tracing::info!("Last update ID: {}", depth.last_update_id);
            tracing::info!(
                "Best bid: {:.2}",
                depth
                    .bids
                    .first()
                    .map_or(0.0, |l| l.price_f64(depth.price_exponent))
            );
            tracing::info!(
                "Best ask: {:.2}",
                depth
                    .asks
                    .first()
                    .map_or(0.0, |l| l.price_f64(depth.price_exponent))
            );
        }
        Err(e) => tracing::error!("Depth (ETHUSDT) failed: {e}"),
    }

    tracing::info!("=== All tests completed ===");

    Ok(())
}
