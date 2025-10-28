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

use std::env;

use nautilus_hyperliquid::http::{
    client::HyperliquidHttpClient,
    models::{
        HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest, HyperliquidExecTpSl,
        HyperliquidExecTriggerParams,
    },
};
use rust_decimal_macros::dec;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    let args: Vec<String> = env::args().collect();
    let testnet = args.get(1).is_some_and(|s| s == "testnet");
    let test_conditional = args.get(1).is_some_and(|s| s == "conditional")
        || args.get(2).is_some_and(|s| s == "conditional");

    tracing::info!("Starting Hyperliquid HTTP private example");
    if testnet {
        tracing::info!(
            "Testnet parameter provided - ensure HYPERLIQUID_TESTNET_PK environment variable is set"
        );
    } else {
        tracing::info!("Mainnet mode - ensure HYPERLIQUID_PK environment variable is set");
    }
    if test_conditional {
        tracing::info!("Conditional orders test mode enabled");
    }

    // Try to create authenticated client from environment
    let client = match HyperliquidHttpClient::from_env() {
        Ok(client) => {
            tracing::info!("Testnet mode: {}", client.is_testnet());
            client
        }
        Err(_) => {
            tracing::warn!(
                "No credentials found in environment (HYPERLIQUID_PK). Skipping authenticated examples."
            );
            return Ok(());
        }
    };

    // For demonstration, use a placeholder address
    let user_address = "0x0000000000000000000000000000000000000000";

    // Test conditional orders if requested
    if test_conditional {
        tracing::info!("=== Testing Conditional Orders ===");
        test_conditional_orders(&client).await?;
        return Ok(());
    }

    // Get user fills
    match client.info_user_fills(user_address).await {
        Ok(fills) => {
            tracing::info!("Fetched {} fills", fills.len());
            for (i, fill) in fills.iter().take(3).enumerate() {
                tracing::info!("Fill {}: {} {} @ {}", i, fill.side, fill.sz, fill.px);
            }
        }
        Err(e) => {
            tracing::info!("Failed to fetch fills: {}", e);
        }
    }

    // Get order status (example with fake order ID)
    let example_order_id = 12345u64;
    match client
        .info_order_status(user_address, example_order_id)
        .await
    {
        Ok(status) => {
            tracing::info!("Order status: {:?}", status);
        }
        Err(e) => {
            tracing::info!("Order status query failed (expected for demo ID): {}", e);
        }
    }

    Ok(())
}

/// Test conditional orders (stop market, stop limit, market-if-touched, limit-if-touched).
async fn test_conditional_orders(
    _client: &HyperliquidHttpClient,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("Testing conditional order types:");
    tracing::info!("  - StopMarket (is_market=true, tpsl=Sl)");
    tracing::info!("  - StopLimit (is_market=false, tpsl=Sl)");
    tracing::info!("  - MarketIfTouched (is_market=true, tpsl=Tp)");
    tracing::info!("  - LimitIfTouched (is_market=false, tpsl=Tp)");
    tracing::info!("");

    // Example: Stop Market order (BUY)
    // Triggers when price goes ABOVE trigger_px, executes at market
    let stop_market_buy = HyperliquidExecPlaceOrderRequest {
        asset: 0, // BTC-USD (asset 0)
        is_buy: true,
        price: dec!(0), // Price is 0 for market execution after trigger
        size: dec!(0.001),
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Trigger {
            trigger: HyperliquidExecTriggerParams {
                is_market: true,
                trigger_px: dec!(45000),       // Trigger at $45,000
                tpsl: HyperliquidExecTpSl::Sl, // Stop Loss semantics
            },
        },
        cloid: None,
    };

    // Example: Stop Limit order (SELL)
    // Triggers when price goes BELOW trigger_px, places limit order at specified price
    let _stop_limit_sell = HyperliquidExecPlaceOrderRequest {
        asset: 0,
        is_buy: false,
        price: dec!(44900), // Limit price after trigger
        size: dec!(0.001),
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Trigger {
            trigger: HyperliquidExecTriggerParams {
                is_market: false,
                trigger_px: dec!(45000),       // Trigger at $45,000
                tpsl: HyperliquidExecTpSl::Sl, // Stop Loss semantics
            },
        },
        cloid: None,
    };

    // Example: Market If Touched order (BUY)
    // Triggers when price goes ABOVE trigger_px, executes at market
    let _market_if_touched_buy = HyperliquidExecPlaceOrderRequest {
        asset: 0,
        is_buy: true,
        price: dec!(0), // Price is 0 for market execution after trigger
        size: dec!(0.001),
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Trigger {
            trigger: HyperliquidExecTriggerParams {
                is_market: true,
                trigger_px: dec!(46000),       // Trigger at $46,000
                tpsl: HyperliquidExecTpSl::Tp, // Take Profit semantics
            },
        },
        cloid: None,
    };

    // Example: Limit If Touched order (SELL)
    // Triggers when price goes BELOW trigger_px, places limit order at specified price
    let _limit_if_touched_sell = HyperliquidExecPlaceOrderRequest {
        asset: 0,
        is_buy: false,
        price: dec!(45900), // Limit price after trigger
        size: dec!(0.001),
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Trigger {
            trigger: HyperliquidExecTriggerParams {
                is_market: false,
                trigger_px: dec!(46000),       // Trigger at $46,000
                tpsl: HyperliquidExecTpSl::Tp, // Take Profit semantics
            },
        },
        cloid: None,
    };

    tracing::info!("Example conditional order structures created:");
    tracing::info!("  1. Stop Market BUY @ trigger $45,000");
    tracing::info!("  2. Stop Limit SELL @ trigger $45,000, limit $44,900");
    tracing::info!("  3. Market If Touched BUY @ trigger $46,000");
    tracing::info!("  4. Limit If Touched SELL @ trigger $46,000, limit $45,900");
    tracing::info!("");
    tracing::info!(
        "To actually place orders, create an ExchangeAction::Order and call client.post_action()"
    );
    tracing::info!("");
    tracing::info!("Example code:");
    tracing::info!(
        r#"
    let action = HyperliquidExecAction::Order {{
        orders: vec![stop_market_buy],
        grouping: HyperliquidExecGrouping::Na,
        builder: None,
    }};
    let response = client.post_action(&action).await?;
    "#
    );

    // Display the JSON serialization to show the exact API format
    let example_json = serde_json::to_string_pretty(&stop_market_buy)?;
    tracing::info!("Stop Market order JSON format:");
    tracing::info!("{}", example_json);

    Ok(())
}
