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

//! Demonstration binary for testing Bybit HTTP client endpoints.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-bybit --bin bybit-http
//! ```
//!
//! For authenticated endpoints, set environment variables:
//! ```bash
//! export BYBIT_API_KEY=your_key
//! export BYBIT_API_SECRET=your_secret
//! cargo run -p nautilus-bybit --bin bybit-http
//! ```

use nautilus_bybit::{
    common::enums::{BybitKlineInterval, BybitProductType},
    http::{
        client::BybitHttpClient,
        query::{
            BybitInstrumentsInfoParamsBuilder, BybitKlinesParamsBuilder, BybitTradesParamsBuilder,
        },
    },
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    println!("=== Bybit HTTP Client Demo ===\n");

    // Test public endpoints
    test_public_endpoints().await?;

    // Test authenticated endpoints if credentials are provided
    let api_key = std::env::var("BYBIT_API_KEY").ok();
    let api_secret = std::env::var("BYBIT_API_SECRET").ok();

    if let (Some(key), Some(secret)) = (api_key, api_secret) {
        println!("\n=== Testing Authenticated Endpoints ===");
        test_authenticated_endpoints(&key, &secret).await?;
    } else {
        println!(
            "\n[SKIP] Skipping authenticated endpoints (set BYBIT_API_KEY and BYBIT_API_SECRET to test)"
        );
    }

    Ok(())
}

async fn test_public_endpoints() -> anyhow::Result<()> {
    let client = BybitHttpClient::new(None, Some(60), None, None, None)?;

    // Test 1: Get server time
    println!("1. Testing GET /v5/market/time");
    match client.http_get_server_time().await {
        Ok(response) => {
            println!(
                "   [OK] Server time: {} (seconds)",
                response.result.time_second
            );
            println!("   [OK] Server time: {} (nanos)", response.result.time_nano);
        }
        Err(e) => {
            println!("   [ERROR] {e}");
            return Err(e.into());
        }
    }

    // Test 2: Get linear instruments
    println!("\n2. Testing GET /v5/market/instruments-info (linear)");
    let params = BybitInstrumentsInfoParamsBuilder::default()
        .category(BybitProductType::Linear)
        .symbol("BTCUSDT")
        .build()?;

    match client.http_get_instruments_linear(&params).await {
        Ok(response) => {
            println!("   [OK] Found {} instruments", response.result.list.len());
            if let Some(first) = response.result.list.first() {
                println!("   [OK] First instrument: {}", first.symbol);
                println!("   [OK] Status: {:?}", first.status);
            }
        }
        Err(e) => {
            println!("   [ERROR] {e}");
            return Err(e.into());
        }
    }

    // Test 3: Get spot instruments
    println!("\n3. Testing GET /v5/market/instruments-info (spot)");
    let params = BybitInstrumentsInfoParamsBuilder::default()
        .category(BybitProductType::Spot)
        .limit(5u32)
        .build()?;

    match client.http_get_instruments_spot(&params).await {
        Ok(response) => {
            println!("   [OK] Found {} instruments", response.result.list.len());
            for instrument in response.result.list.iter().take(3) {
                println!("   - {}: {:?}", instrument.symbol, instrument.status);
            }
        }
        Err(e) => {
            println!("   [ERROR] {e}");
            return Err(e.into());
        }
    }

    // Test 4: Get klines
    println!("\n4. Testing GET /v5/market/kline");
    let params = BybitKlinesParamsBuilder::default()
        .category(BybitProductType::Linear)
        .symbol("BTCUSDT")
        .interval(BybitKlineInterval::Minute1)
        .limit(5u32)
        .build()?;

    match client.http_get_klines(&params).await {
        Ok(response) => {
            println!("   [OK] Found {} klines", response.result.list.len());
            if let Some(first) = response.result.list.first() {
                println!(
                    "   [OK] First kline: O={}, H={}, L={}, C={}",
                    first.open, first.high, first.low, first.close
                );
            }
        }
        Err(e) => {
            println!("   [ERROR] {e}");
            return Err(e.into());
        }
    }

    // Test 5: Get recent trades
    println!("\n5. Testing GET /v5/market/recent-trade");
    let params = BybitTradesParamsBuilder::default()
        .category(BybitProductType::Linear)
        .symbol("BTCUSDT")
        .limit(5u32)
        .build()?;

    match client.http_get_recent_trades(&params).await {
        Ok(response) => {
            println!("   [OK] Found {} recent trades", response.result.list.len());
            for trade in response.result.list.iter().take(3) {
                println!(
                    "   - Price: {}, Size: {}, Side: {:?}",
                    trade.price, trade.size, trade.side
                );
            }
        }
        Err(e) => {
            println!("   [ERROR] {e}");
            return Err(e.into());
        }
    }

    println!("\n[SUCCESS] All public endpoint tests passed!");
    Ok(())
}

async fn test_authenticated_endpoints(api_key: &str, api_secret: &str) -> anyhow::Result<()> {
    let base_url = std::env::var("BYBIT_BASE_URL")
        .unwrap_or_else(|_| "https://api-testnet.bybit.com".to_string());

    let client = BybitHttpClient::with_credentials(
        api_key.to_string(),
        api_secret.to_string(),
        Some(base_url),
        Some(60),
        None,
        None,
        None,
    )?;

    // Test 1: Get open orders
    println!("\n1. Testing GET /v5/order/realtime (open orders)");
    match client
        .http_get_open_orders(BybitProductType::Linear, Some("BTCUSDT"))
        .await
    {
        Ok(response) => {
            println!("   [OK] Found {} open orders", response.result.list.len());
            for order in response.result.list.iter().take(3) {
                println!(
                    "   - Order: {} | {:?} | {} @ {}",
                    order.order_id, order.side, order.qty, order.price
                );
            }
        }
        Err(e) => {
            println!("   [WARN] Error (may be expected if no orders): {e}");
        }
    }

    // Test 2: Place order (commented out to avoid actual order placement)
    println!("\n2. Testing POST /v5/order/create (order placement)");
    println!("   [SKIP] Skipping actual order placement to avoid unintended trades");
    println!("   [INFO] Uncomment in code to test order placement");

    /*
    let order_request = serde_json::json!({
        "category": "linear",
        "symbol": "BTCUSDT",
        "side": "Buy",
        "orderType": "Limit",
        "qty": "0.001",
        "price": "20000",
        "timeInForce": "GTC",
        "orderLinkId": format!("test-{}", chrono::Utc::now().timestamp())
    });

    match client.place_order(&order_request).await {
        Ok(response) => {
            println!("   [OK] Order placed successfully");
            println!("   [OK] Order ID: {:?}", response.result.order_id);
            println!("   [OK] Order Link ID: {:?}", response.result.order_link_id);
        }
        Err(e) => {
            println!("   [ERROR] {e}");
            return Err(e.into());
        }
    }
    */

    println!("\n[SUCCESS] Authenticated endpoint tests completed!");
    Ok(())
}
