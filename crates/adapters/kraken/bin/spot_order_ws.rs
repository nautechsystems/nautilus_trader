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

//! Kraken Spot WebSocket API order type testing.
//!
//! Tests all supported order types via WebSocket:
//! 1. Market order (buy and sell)
//! 2. Limit order (with post-only)
//! 3. Stop-loss order
//! 4. Stop-loss-limit order
//! 5. Take-profit order
//! 6. Take-profit-limit order
//! 7. IOC (Immediate-Or-Cancel) order
//! 8. FOK (Fill-Or-Kill) order - requires special account permissions
//!
//! Note: WebSocket API does NOT support broker_id (only REST API does)
//! Note: FOK time-in-force requires Kraken Pro or institutional account permissions
//!
//! # Environment Variables
//!
//! - `KRAKEN_SPOT_API_KEY`: Your Kraken Spot API key
//! - `KRAKEN_SPOT_API_SECRET`: Your Kraken Spot API secret

use nautilus_kraken::{
    common::{consts::KRAKEN_SPOT_WS_PRIVATE_URL, enums::KrakenEnvironment},
    config::KrakenDataClientConfig,
    http::spot::client::KrakenSpotRawHttpClient,
    websocket::spot_v2::client::KrakenSpotWebSocketClient,
};
use nautilus_model::identifiers::AccountId;
use tokio_util::sync::CancellationToken;

// Test configuration constants
const SYMBOL: &str = "ATOM/USDC"; // WebSocket symbol format
const PAIR: &str = "ATOMUSDC"; // REST API pair format
const QTY: f64 = 0.5;
const ACCOUNT_ID: &str = "KRAKEN-001";

// Price calculation multipliers
const LIMIT_BUY_MULTIPLIER: f64 = 0.95; // 5% below market
const LIMIT_SELL_MULTIPLIER: f64 = 1.05; // 5% above market
const STOP_LOSS_MULTIPLIER: f64 = 0.90; // 10% below market
const TAKE_PROFIT_MULTIPLIER: f64 = 1.10; // 10% above market
const SECONDARY_PRICE_MULTIPLIER: f64 = 0.99; // 1% below trigger for limit orders

// Timing constants
const WAIT_DURATION_SECS: u64 = 2;
const WS_CONNECT_TIMEOUT_SECS: f64 = 10.0;
const DEFAULT_PRICE: f64 = 5.0;
const PRICE_DECIMALS: f64 = 10000.0; // 4 decimal places

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .finish(),
    )?;

    tracing::info!("Kraken Spot WebSocket API - Order Type Testing");
    tracing::info!("===============================================");

    let api_key = std::env::var("KRAKEN_SPOT_API_KEY")
        .map_err(|_| anyhow::anyhow!("KRAKEN_SPOT_API_KEY not set"))?;
    let api_secret = std::env::var("KRAKEN_SPOT_API_SECRET")
        .map_err(|_| anyhow::anyhow!("KRAKEN_SPOT_API_SECRET not set"))?;

    // Create REST client for ticker and cancel operations
    let rest_client = KrakenSpotRawHttpClient::with_credentials(
        api_key.clone(),
        api_secret.clone(),
        KrakenEnvironment::Mainnet,
        None,
        Some(60),
        None,
        None,
        None,
        None,
    )?;

    // Cancel all open orders first via REST
    tracing::info!("\n[SETUP] Canceling all open orders via REST...");
    let _ = rest_client.cancel_all_orders().await;

    // Get current ticker for reference prices
    let ticker = rest_client.get_ticker(vec![PAIR.to_string()]).await?;
    let current_price = ticker
        .values()
        .next()
        .and_then(|t| t.last.first())
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(DEFAULT_PRICE);
    tracing::info!("Current {} price: ${:.4}", SYMBOL, current_price);

    // Helper to round to 4 decimal places (ATOM/USDC precision)
    fn round_price(price: f64) -> f64 {
        (price * PRICE_DECIMALS).round() / PRICE_DECIMALS
    }

    // Calculate prices for limit/stop orders (rounded to 4 decimals)
    let limit_buy_price = round_price(current_price * LIMIT_BUY_MULTIPLIER);
    let limit_sell_price = round_price(current_price * LIMIT_SELL_MULTIPLIER);
    let stop_loss_trigger = round_price(current_price * STOP_LOSS_MULTIPLIER);
    let take_profit_trigger = round_price(current_price * TAKE_PROFIT_MULTIPLIER);

    // Create WebSocket client
    let ws_config = KrakenDataClientConfig {
        environment: KrakenEnvironment::Mainnet,
        api_key: Some(api_key),
        api_secret: Some(api_secret),
        ws_public_url: Some(KRAKEN_SPOT_WS_PRIVATE_URL.to_string()),
        ..Default::default()
    };

    let token = CancellationToken::new();
    let mut ws_client = KrakenSpotWebSocketClient::new(ws_config, token.clone());

    // Connect and authenticate
    tracing::info!("\n[SETUP] Connecting WebSocket...");
    ws_client.connect().await?;
    ws_client.wait_until_active(WS_CONNECT_TIMEOUT_SECS).await?;
    ws_client.authenticate().await?;
    ws_client.set_account_id(AccountId::new(ACCOUNT_ID));
    ws_client.subscribe_executions(true, true).await?;
    tracing::info!("WebSocket connected and authenticated");

    // Helper to wait for order processing
    async fn wait_short() {
        tokio::time::sleep(tokio::time::Duration::from_secs(WAIT_DURATION_SECS)).await;
    }

    // =========================================================================
    // TEST 1: Market Order (BUY)
    // =========================================================================
    tracing::info!("\n[TEST 1] MARKET BUY Order via WebSocket");
    tracing::info!("  Symbol: {}, Qty: {}", SYMBOL, QTY);

    match ws_client
        .add_order(
            "market", "buy", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // =========================================================================
    // TEST 2: Market Order (SELL)
    // =========================================================================
    tracing::info!("\n[TEST 2] MARKET SELL Order via WebSocket");
    tracing::info!("  Symbol: {}, Qty: {}", SYMBOL, QTY);

    match ws_client
        .add_order(
            "market", "sell", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // =========================================================================
    // TEST 3: Limit Order (BUY) with post-only
    // =========================================================================
    tracing::info!("\n[TEST 3] LIMIT BUY Order (post-only) via WebSocket");
    tracing::info!(
        "  Symbol: {}, Qty: {}, Price: ${:.4}",
        SYMBOL,
        QTY,
        limit_buy_price
    );

    match ws_client
        .add_order(
            "limit",
            "buy",
            QTY,
            SYMBOL,
            Some(limit_buy_price),
            None,                             // no trigger
            None,                             // no trigger reference
            Some("testlimitbuy".to_string()), // cl_ord_id (no hyphens)
            Some("gtc".to_string()),          // time_in_force
            Some(true),                       // post-only
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // Cancel limit order via REST
    tracing::info!("  Canceling limit orders via REST...");
    let _ = rest_client.cancel_all_orders().await;

    // =========================================================================
    // TEST 4: Limit Order (SELL)
    // =========================================================================
    tracing::info!("\n[TEST 4] LIMIT SELL Order via WebSocket");

    // Buy first via WS
    let _ = ws_client
        .add_order(
            "market", "buy", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    tracing::info!(
        "  Symbol: {}, Qty: {}, Price: ${:.4}",
        SYMBOL,
        QTY,
        limit_sell_price
    );

    match ws_client
        .add_order(
            "limit",
            "sell",
            QTY,
            SYMBOL,
            Some(limit_sell_price),
            None,
            None,
            Some("testlimitsell".to_string()),
            Some("gtc".to_string()),
            None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // Cancel and close position
    let _ = rest_client.cancel_all_orders().await;
    let _ = ws_client
        .add_order(
            "market", "sell", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    // =========================================================================
    // TEST 5: Stop-Loss Order
    // =========================================================================
    tracing::info!("\n[TEST 5] STOP-LOSS Order via WebSocket");

    // Buy first
    let _ = ws_client
        .add_order(
            "market", "buy", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    tracing::info!(
        "  Symbol: {}, Qty: {}, Trigger: ${:.4}",
        SYMBOL,
        QTY,
        stop_loss_trigger
    );

    match ws_client
        .add_order(
            "stop-loss",
            "sell",
            QTY,
            SYMBOL,
            None,                    // no limit price for pure stop-loss
            Some(stop_loss_trigger), // trigger price
            Some("last"),            // trigger reference
            Some("teststoploss".to_string()),
            None,
            None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // Cancel and close
    let _ = rest_client.cancel_all_orders().await;
    let _ = ws_client
        .add_order(
            "market", "sell", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    // =========================================================================
    // TEST 6: Stop-Loss-Limit Order
    // =========================================================================
    tracing::info!("\n[TEST 6] STOP-LOSS-LIMIT Order via WebSocket");

    // Buy first
    let _ = ws_client
        .add_order(
            "market", "buy", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    let stop_limit_price = round_price(stop_loss_trigger * SECONDARY_PRICE_MULTIPLIER);
    tracing::info!(
        "  Symbol: {}, Qty: {}, Trigger: ${:.4}, Limit: ${:.4}",
        SYMBOL,
        QTY,
        stop_loss_trigger,
        stop_limit_price
    );

    match ws_client
        .add_order(
            "stop-loss-limit",
            "sell",
            QTY,
            SYMBOL,
            Some(stop_limit_price),  // limit price
            Some(stop_loss_trigger), // trigger price
            Some("last"),            // trigger reference
            Some("teststoplosslimit".to_string()),
            None,
            None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // Cancel and close
    let _ = rest_client.cancel_all_orders().await;
    let _ = ws_client
        .add_order(
            "market", "sell", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    // =========================================================================
    // TEST 7: Take-Profit Order
    // =========================================================================
    tracing::info!("\n[TEST 7] TAKE-PROFIT Order via WebSocket");

    // Buy first
    let _ = ws_client
        .add_order(
            "market", "buy", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    tracing::info!(
        "  Symbol: {}, Qty: {}, Trigger: ${:.4}",
        SYMBOL,
        QTY,
        take_profit_trigger
    );

    match ws_client
        .add_order(
            "take-profit",
            "sell",
            QTY,
            SYMBOL,
            None,                      // no limit price
            Some(take_profit_trigger), // trigger price
            Some("last"),              // trigger reference
            Some("testtakeprofit".to_string()),
            None,
            None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // Cancel and close
    let _ = rest_client.cancel_all_orders().await;
    let _ = ws_client
        .add_order(
            "market", "sell", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    // =========================================================================
    // TEST 8: Take-Profit-Limit Order
    // =========================================================================
    tracing::info!("\n[TEST 8] TAKE-PROFIT-LIMIT Order via WebSocket");

    // Buy first
    let _ = ws_client
        .add_order(
            "market", "buy", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    let tp_limit_price = round_price(take_profit_trigger * SECONDARY_PRICE_MULTIPLIER);
    tracing::info!(
        "  Symbol: {}, Qty: {}, Trigger: ${:.4}, Limit: ${:.4}",
        SYMBOL,
        QTY,
        take_profit_trigger,
        tp_limit_price
    );

    match ws_client
        .add_order(
            "take-profit-limit",
            "sell",
            QTY,
            SYMBOL,
            Some(tp_limit_price),                 // limit price
            Some(take_profit_trigger),            // trigger price
            Some("last"),                         // trigger reference
            Some("testtprofitlimit".to_string()), // cl_ord_id max 18 chars
            None,
            None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // Cancel and close
    let _ = rest_client.cancel_all_orders().await;
    let _ = ws_client
        .add_order(
            "market", "sell", QTY, SYMBOL, None, None, None, None, None, None,
        )
        .await;
    wait_short().await;

    // =========================================================================
    // TEST 9: IOC (Immediate-Or-Cancel) Limit Order
    // =========================================================================
    tracing::info!("\n[TEST 9] IOC LIMIT BUY Order via WebSocket");
    tracing::info!(
        "  Symbol: {}, Qty: {}, Price: ${:.4}, TIF: IOC",
        SYMBOL,
        QTY,
        limit_buy_price
    );

    match ws_client
        .add_order(
            "limit",
            "buy",
            QTY,
            SYMBOL,
            Some(limit_buy_price),
            None,
            None,
            Some("testioc".to_string()),
            Some("ioc".to_string()), // immediate-or-cancel
            None,
        )
        .await
    {
        Ok(()) => tracing::info!("  Order submitted successfully (should cancel if not filled)"),
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    wait_short().await;

    // =========================================================================
    // TEST 10: FOK (Fill-Or-Kill) Limit Order
    // Note: FOK requires special account permissions on Kraken
    // =========================================================================
    tracing::info!("\n[TEST 10] FOK LIMIT BUY Order via WebSocket");
    tracing::info!(
        "  Symbol: {}, Qty: {}, Price: ${:.4}, TIF: FOK",
        SYMBOL,
        QTY,
        limit_buy_price
    );
    tracing::info!("  Note: FOK may require special account permissions");

    match ws_client
        .add_order(
            "limit",
            "buy",
            QTY,
            SYMBOL,
            Some(limit_buy_price),
            None,
            None,
            Some("testfok".to_string()),
            Some("fok".to_string()), // fill-or-kill
            None,
        )
        .await
    {
        Ok(()) => {
            tracing::info!("  Order submitted successfully (should cancel if not fully filled)")
        }
        Err(e) => tracing::warn!("  Expected: FOK requires account permission: {}", e),
    }
    wait_short().await;

    // Final cleanup
    tracing::info!("\n[CLEANUP] Canceling any remaining orders...");
    let _ = rest_client.cancel_all_orders().await;

    // Disconnect WebSocket
    tracing::info!("Disconnecting WebSocket...");
    ws_client.disconnect().await?;

    tracing::info!("\n===============================================");
    tracing::info!("WebSocket API Order Type Testing Complete!");
    tracing::info!(
        "Tested: Market, Limit, Stop-Loss, Stop-Loss-Limit, Take-Profit, Take-Profit-Limit, IOC, FOK"
    );

    Ok(())
}
