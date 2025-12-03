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

//! Demonstration of Kraken adapter HTTP client capabilities.
//!
//! Tests both Spot and Futures endpoints with public and authenticated methods.
//!
//! Usage:
//! ```bash
//! # Test public endpoints only
//! cargo run -p nautilus-kraken --bin kraken-demo
//!
//! # Test Futures demo environment (includes order placement tests)
//! export KRAKEN_TESTNET_API_KEY=your_demo_key
//! export KRAKEN_TESTNET_API_SECRET=your_demo_secret
//! cargo run -p nautilus-kraken --bin kraken-demo
//! ```

use std::env;

use nautilus_kraken::{
    common::enums::{KrakenEnvironment, KrakenFuturesOrderType, KrakenOrderSide},
    http::{
        futures::{client::KrakenFuturesRawHttpClient, query::KrakenFuturesSendOrderParamsBuilder},
        spot::client::KrakenSpotRawHttpClient,
    },
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    // Detect testnet credentials
    let has_testnet_creds = env::var("KRAKEN_TESTNET_API_KEY").is_ok();

    if has_testnet_creds {
        tracing::info!("=== Kraken Adapter Demo (TESTNET) ===");
        tracing::info!("Using KRAKEN_TESTNET_API_KEY and KRAKEN_TESTNET_API_SECRET");
    } else {
        tracing::info!("=== Kraken Adapter Demo ===");
        tracing::info!("No credentials detected - running public endpoints only");
    }

    test_spot_public().await?;
    test_futures_public(has_testnet_creds).await?;
    test_futures_authenticated(has_testnet_creds).await?;

    // Run order placement tests if testnet credentials are available
    if has_testnet_creds {
        test_futures_order_placement(has_testnet_creds).await?;
    }

    tracing::info!("=== Demo Complete ===");

    Ok(())
}

async fn test_spot_public() -> anyhow::Result<()> {
    tracing::info!("=== Spot Public Endpoints ===");

    let client = KrakenSpotRawHttpClient::default();

    match client.get_server_time().await {
        Ok(response) => {
            tracing::info!("Server time: {} ({})", response.unixtime, response.rfc1123);
        }
        Err(e) => {
            tracing::error!("Failed to get server time: {e}");
            return Err(e.into());
        }
    }

    match client.get_system_status().await {
        Ok(response) => {
            tracing::info!("System status: {:?}", response.status);
        }
        Err(e) => {
            tracing::error!("Failed to get system status: {e}");
            return Err(e.into());
        }
    }

    match client
        .get_asset_pairs(Some(vec!["XBTUSDT".to_string()]))
        .await
    {
        Ok(pairs) => {
            tracing::info!("Found {} asset pair(s)", pairs.len());
        }
        Err(e) => {
            tracing::error!("Failed to get asset pairs: {e}");
            return Err(e.into());
        }
    }

    match client.get_ticker(vec!["XBTUSDT".to_string()]).await {
        Ok(tickers) => {
            if let Some((symbol, ticker)) = tickers.iter().next()
                && let Some(last) = ticker.last.first()
            {
                tracing::info!("Ticker {symbol}: last={last}");
            }
        }
        Err(e) => {
            tracing::error!("Failed to get ticker: {e}");
            return Err(e.into());
        }
    }

    Ok(())
}

async fn test_futures_public(testnet: bool) -> anyhow::Result<()> {
    tracing::info!("=== Futures Public Endpoints ===");

    let environment = if testnet {
        KrakenEnvironment::Testnet
    } else {
        KrakenEnvironment::Mainnet
    };

    let client = KrakenFuturesRawHttpClient::new(environment, None, None, None, None, None, None)?;

    match client.get_instruments().await {
        Ok(instruments) => {
            tracing::info!("Found {} instrument(s)", instruments.instruments.len());
            if let Some(instrument) = instruments.instruments.first() {
                tracing::info!(
                    "Sample instrument: {} (type: {})",
                    instrument.symbol,
                    instrument.instrument_type
                );
            }
        }
        Err(e) => {
            tracing::error!("Failed to get instruments: {e}");
            return Err(e.into());
        }
    }

    match client.get_tickers().await {
        Ok(response) => {
            tracing::info!("Found {} ticker(s)", response.tickers.len());
            if let Some(ticker) = response.tickers.first() {
                if let Some(last) = ticker.last {
                    tracing::info!("Sample ticker: {} (last={})", ticker.symbol, last);
                } else {
                    tracing::info!(
                        "Sample ticker: {} (last price not available)",
                        ticker.symbol
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to get tickers (may not be available in demo environment): {e}");
        }
    }

    Ok(())
}

async fn test_futures_authenticated(testnet: bool) -> anyhow::Result<()> {
    tracing::info!("=== Futures Authenticated Endpoints ===");

    let environment = if testnet {
        KrakenEnvironment::Testnet
    } else {
        KrakenEnvironment::Mainnet
    };

    let (api_key_var, api_secret_var) = if testnet {
        ("KRAKEN_TESTNET_API_KEY", "KRAKEN_TESTNET_API_SECRET")
    } else {
        ("KRAKEN_FUTURES_API_KEY", "KRAKEN_FUTURES_API_SECRET")
    };

    let api_key = env::var(api_key_var).ok();
    let api_secret = env::var(api_secret_var).ok();

    let client = match (api_key, api_secret) {
        (Some(key), Some(secret)) => KrakenFuturesRawHttpClient::with_credentials(
            key,
            secret,
            environment,
            None,
            None,
            None,
            None,
            None,
            None,
        )?,
        _ => {
            tracing::warn!("No credentials found - skipping authenticated tests");
            return Ok(());
        }
    };

    match client.get_open_orders().await {
        Ok(response) => {
            tracing::info!("Open orders: {}", response.open_orders.len());
        }
        Err(e) => {
            tracing::error!("Failed to get open orders: {e}");
        }
    }

    match client.get_open_positions().await {
        Ok(response) => {
            tracing::info!("Open positions: {}", response.open_positions.len());
            for pos in response.open_positions.iter().take(3) {
                tracing::info!(
                    "  Position: {} {:?} size={}",
                    pos.symbol,
                    pos.side,
                    pos.size
                );
            }
        }
        Err(e) => {
            tracing::error!("Failed to get open positions: {e}");
        }
    }

    match client.get_fills(None).await {
        Ok(response) => {
            tracing::info!("Recent fills: {}", response.fills.len());
            for fill in response.fills.iter().take(3) {
                tracing::info!(
                    "  Fill: {} {:?} price={} size={}",
                    fill.symbol,
                    fill.side,
                    fill.price,
                    fill.size
                );
            }
        }
        Err(e) => {
            tracing::error!("Failed to get fills: {e}");
        }
    }

    Ok(())
}

async fn test_futures_order_placement(testnet: bool) -> anyhow::Result<()> {
    tracing::info!("=== Futures Order Placement Tests ===");

    let environment = if testnet {
        KrakenEnvironment::Testnet
    } else {
        KrakenEnvironment::Mainnet
    };

    let (api_key_var, api_secret_var) = if testnet {
        ("KRAKEN_TESTNET_API_KEY", "KRAKEN_TESTNET_API_SECRET")
    } else {
        ("KRAKEN_FUTURES_API_KEY", "KRAKEN_FUTURES_API_SECRET")
    };

    let api_key = env::var(api_key_var).map_err(|_| anyhow::anyhow!("Missing {}", api_key_var))?;
    let api_secret =
        env::var(api_secret_var).map_err(|_| anyhow::anyhow!("Missing {}", api_secret_var))?;

    let client = KrakenFuturesRawHttpClient::with_credentials(
        api_key,
        api_secret,
        environment,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    let current_price: f64 = 95000.0;
    tracing::info!("Using reference PI_XBTUSD price: {}", current_price);

    tracing::info!("Test 1: LIMIT order (post-only, far from market)");
    let limit_price = (current_price * 0.5).round();
    let params = KrakenFuturesSendOrderParamsBuilder::default()
        .symbol("PI_XBTUSD".to_string())
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenFuturesOrderType::Post)
        .size("1".to_string())
        .limit_price(limit_price.to_string())
        .cli_ord_id(format!("limit_{}", chrono::Utc::now().timestamp()))
        .build()?;

    let order_response = client.send_order_params(&params).await?;
    tracing::info!("LIMIT order result: {:?}", order_response.result);

    let mut limit_order_id = None;
    if let Some(send_status) = &order_response.send_status {
        tracing::info!("Order status: {}", send_status.status);
        if let Some(order_id) = &send_status.order_id {
            tracing::info!("Order ID: {}", order_id);
            limit_order_id = Some(order_id.clone());
        }
    } else {
        tracing::error!("Order placement failed. Error: {:?}", order_response.error);
        tracing::warn!("This may indicate:");
        tracing::warn!("  1. API keys lack trading permissions");
        tracing::warn!("  2. Demo account needs additional setup");
        tracing::warn!("  3. Credentials are expired or invalid");
        tracing::warn!("Note: Read operations (get_open_orders, etc.) worked successfully");
        return Ok(());
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    if let Some(order_id) = limit_order_id {
        tracing::info!("Test 2: Cancel LIMIT order");
        let cancel_response = client.cancel_order(Some(order_id.clone()), None).await?;
        tracing::info!("Cancel status: {}", cancel_response.cancel_status.status);
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    tracing::info!("Test 3: STOP_MARKET order (stop-loss, far from market)");
    let stop_price = (current_price * 0.6).round();
    let params = KrakenFuturesSendOrderParamsBuilder::default()
        .symbol("PI_XBTUSD".to_string())
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenFuturesOrderType::Stop)
        .size("1".to_string())
        .stop_price(stop_price.to_string())
        .cli_ord_id(format!("stop_{}", chrono::Utc::now().timestamp()))
        .build()?;

    let stop_order_response = client.send_order_params(&params).await?;
    let mut stop_order_id = None;
    if let Some(send_status) = &stop_order_response.send_status {
        tracing::info!("STOP order status: {}", send_status.status);
        if let Some(order_id) = &send_status.order_id {
            tracing::info!("STOP Order ID: {}", order_id);
            stop_order_id = Some(order_id.clone());
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    tracing::info!("Test 4: STOP_LIMIT order (stop-loss with limit, far from market)");
    let stop_limit_trigger = (current_price * 0.65).round();
    let stop_limit_price = (current_price * 0.64).round();
    let params = KrakenFuturesSendOrderParamsBuilder::default()
        .symbol("PI_XBTUSD".to_string())
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenFuturesOrderType::Stop)
        .size("1".to_string())
        .stop_price(stop_limit_trigger.to_string())
        .limit_price(stop_limit_price.to_string())
        .cli_ord_id(format!("stop_limit_{}", chrono::Utc::now().timestamp()))
        .build()?;

    let stop_limit_response = client.send_order_params(&params).await?;
    let mut stop_limit_order_id = None;
    if let Some(send_status) = &stop_limit_response.send_status {
        tracing::info!("STOP_LIMIT order status: {}", send_status.status);
        if let Some(order_id) = &send_status.order_id {
            tracing::info!("STOP_LIMIT Order ID: {}", order_id);
            stop_limit_order_id = Some(order_id.clone());
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    tracing::info!("Test 5: MARKET_IF_TOUCHED order (take-profit, above market)");
    let take_profit_price = (current_price * 1.5).round();
    let params = KrakenFuturesSendOrderParamsBuilder::default()
        .symbol("PI_XBTUSD".to_string())
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenFuturesOrderType::TakeProfit)
        .size("1".to_string())
        .stop_price(take_profit_price.to_string())
        .cli_ord_id(format!("tp_{}", chrono::Utc::now().timestamp()))
        .build()?;

    let tp_order_response = client.send_order_params(&params).await?;
    let mut tp_order_id = None;
    if let Some(send_status) = &tp_order_response.send_status {
        tracing::info!("TAKE_PROFIT order status: {}", send_status.status);
        if let Some(order_id) = &send_status.order_id {
            tracing::info!("TAKE_PROFIT Order ID: {}", order_id);
            tp_order_id = Some(order_id.clone());
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    tracing::info!("Test 6: MARKET order (small size to open position)");
    let params = KrakenFuturesSendOrderParamsBuilder::default()
        .symbol("PI_XBTUSD".to_string())
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenFuturesOrderType::Market)
        .size("1".to_string())
        .cli_ord_id(format!("market_{}", chrono::Utc::now().timestamp()))
        .build()?;

    let market_response = client.send_order_params(&params).await?;
    if let Some(send_status) = &market_response.send_status {
        tracing::info!("MARKET order status: {}", send_status.status);
        if let Some(order_id) = &send_status.order_id {
            tracing::info!("MARKET Order ID: {}", order_id);
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    let positions = client.get_open_positions().await?;
    tracing::info!(
        "Open positions after MARKET order: {}",
        positions.open_positions.len()
    );

    if !positions.open_positions.is_empty() {
        tracing::info!("Test 7: Close position with MARKET order (reduce_only)");
        let params = KrakenFuturesSendOrderParamsBuilder::default()
            .symbol("PI_XBTUSD".to_string())
            .side(KrakenOrderSide::Sell)
            .order_type(KrakenFuturesOrderType::Market)
            .size("1".to_string())
            .reduce_only(true)
            .cli_ord_id(format!("close_{}", chrono::Utc::now().timestamp()))
            .build()?;

        let close_response = client.send_order_params(&params).await?;
        if let Some(send_status) = &close_response.send_status {
            tracing::info!("CLOSE order status: {}", send_status.status);
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let positions_after = client.get_open_positions().await?;
        tracing::info!(
            "Open positions after close: {}",
            positions_after.open_positions.len()
        );
    } else {
        tracing::warn!("No position opened by MARKET order - skipping close test");
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    if let Some(order_id) = stop_order_id {
        tracing::info!("Cancelling STOP order");
        let _ = client.cancel_order(Some(order_id), None).await;
    }

    if let Some(order_id) = stop_limit_order_id {
        tracing::info!("Cancelling STOP_LIMIT order");
        let _ = client.cancel_order(Some(order_id), None).await;
    }

    if let Some(order_id) = tp_order_id {
        tracing::info!("Cancelling TAKE_PROFIT order");
        let _ = client.cancel_order(Some(order_id), None).await;
    }

    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    let open_orders = client.get_open_orders().await?;
    tracing::info!("Remaining open orders: {}", open_orders.open_orders.len());

    let final_positions = client.get_open_positions().await?;
    tracing::info!(
        "Final open positions: {}",
        final_positions.open_positions.len()
    );

    tracing::info!("All order type tests complete");

    Ok(())
}
