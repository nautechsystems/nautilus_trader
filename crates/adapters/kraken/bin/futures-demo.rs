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

//! Demonstration binary for testing Kraken Futures HTTP client endpoints.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-kraken --bin futures-demo
//! ```
//!
//! For authenticated endpoints, set environment variables:
//! ```bash
//! export KRAKEN_TESTNET_API_KEY=your_key
//! export KRAKEN_TESTNET_API_SECRET=your_secret
//! cargo run -p nautilus-kraken --bin futures-demo
//! ```

use std::env;

use nautilus_kraken::{
    common::enums::KrakenEnvironment, http::futures::client::KrakenFuturesRawHttpClient,
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::DEBUG)
        .init();

    println!("=== Kraken Futures HTTP Client Demo ===\n");

    test_public_endpoints().await?;

    let api_key = env::var("KRAKEN_TESTNET_API_KEY").ok();
    let api_secret = env::var("KRAKEN_TESTNET_API_SECRET").ok();

    if let (Some(key), Some(secret)) = (api_key, api_secret) {
        println!("\n=== Testing Authenticated Endpoints ===");
        test_authenticated_endpoints(&key, &secret).await?;
    } else {
        println!(
            "\n[SKIP] Set KRAKEN_TESTNET_API_KEY and KRAKEN_TESTNET_API_SECRET to test authenticated endpoints"
        );
    }

    Ok(())
}

async fn test_public_endpoints() -> anyhow::Result<()> {
    let client = KrakenFuturesRawHttpClient::new(
        KrakenEnvironment::Testnet,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    println!("1. Testing GET /derivatives/api/v3/instruments");
    match client.get_instruments().await {
        Ok(response) => {
            println!("   [OK] Found {} instruments", response.instruments.len());
            if let Some(inst) = response.instruments.first() {
                println!("   [OK] Sample: {} ({})", inst.symbol, inst.instrument_type);
            }
        }
        Err(e) => {
            println!("   [ERROR] {e}");
            return Err(e.into());
        }
    }

    println!("\n2. Testing GET /derivatives/api/v3/tickers");
    match client.get_tickers().await {
        Ok(response) => {
            println!("   [OK] Found {} tickers", response.tickers.len());
            if let Some(ticker) = response.tickers.first()
                && let Some(last) = ticker.last
            {
                println!("   [OK] Sample: {} (last={})", ticker.symbol, last);
            }
        }
        Err(e) => {
            println!("   [WARN] {e}");
        }
    }

    println!("\n[SUCCESS] Public endpoint tests passed!");
    Ok(())
}

async fn test_authenticated_endpoints(api_key: &str, api_secret: &str) -> anyhow::Result<()> {
    let client = KrakenFuturesRawHttpClient::with_credentials(
        api_key.to_string(),
        api_secret.to_string(),
        KrakenEnvironment::Testnet,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    println!("\n1. Testing GET /derivatives/api/v3/openorders");
    match client.get_open_orders().await {
        Ok(response) => {
            println!("   [OK] Open orders: {}", response.open_orders.len());
        }
        Err(e) => {
            println!("   [WARN] {e}");
        }
    }

    println!("\n2. Testing GET /derivatives/api/v3/openpositions");
    match client.get_open_positions().await {
        Ok(response) => {
            println!("   [OK] Open positions: {}", response.open_positions.len());
            for pos in response.open_positions.iter().take(3) {
                println!("   - {} {:?} size={}", pos.symbol, pos.side, pos.size);
            }
        }
        Err(e) => {
            println!("   [WARN] {e}");
        }
    }

    println!("\n3. Testing GET /derivatives/api/v3/fills");
    match client.get_fills(None).await {
        Ok(response) => {
            println!("   [OK] Recent fills: {}", response.fills.len());
            for fill in response.fills.iter().take(3) {
                println!(
                    "   - {} {:?} price={} size={}",
                    fill.symbol, fill.side, fill.price, fill.size
                );
            }
        }
        Err(e) => {
            println!("   [WARN] {e}");
        }
    }

    println!("\n[SUCCESS] Authenticated endpoint tests completed!");
    Ok(())
}
