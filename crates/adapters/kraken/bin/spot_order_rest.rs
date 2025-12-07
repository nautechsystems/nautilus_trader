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

//! Kraken Spot REST API order type testing.
//!
//! Tests all supported order types:
//! 1. Market order (buy and sell)
//! 2. Limit order (with post-only)
//! 3. Stop-loss order
//! 4. Stop-loss-limit order
//! 5. Take-profit order
//! 6. Take-profit-limit order
//!
//! # Environment Variables
//!
//! - `KRAKEN_SPOT_API_KEY`: Your Kraken Spot API key
//! - `KRAKEN_SPOT_API_SECRET`: Your Kraken Spot API secret
//!

use nautilus_kraken::{
    common::{
        consts::NAUTILUS_KRAKEN_BROKER_ID,
        enums::{KrakenEnvironment, KrakenOrderSide, KrakenOrderType},
    },
    http::spot::{
        client::KrakenSpotRawHttpClient,
        query::{KrakenSpotAddOrderParamsBuilder, KrakenSpotCancelOrderParamsBuilder},
    },
};

// Test configuration constants
const PAIR: &str = "ATOMUSDC";
const QTY: &str = "0.5";
const DEFAULT_PRICE: f64 = 5.0;

// Price calculation multipliers
const LIMIT_BUY_MULTIPLIER: f64 = 0.95; // 5% below market
const LIMIT_SELL_MULTIPLIER: f64 = 1.05; // 5% above market
const STOP_LOSS_MULTIPLIER: f64 = 0.90; // 10% below market
const TAKE_PROFIT_MULTIPLIER: f64 = 1.10; // 10% above market
const SECONDARY_PRICE_MULTIPLIER: f64 = 0.99; // 1% below trigger for limit orders

// Timing constants (seconds)
const SHORT_WAIT: u64 = 1;
const LONG_WAIT: u64 = 2;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .finish(),
    )?;

    tracing::info!("Kraken Spot REST API - Order Type Testing");
    tracing::info!("==========================================");

    let api_key = std::env::var("KRAKEN_SPOT_API_KEY")
        .map_err(|_| anyhow::anyhow!("KRAKEN_SPOT_API_KEY not set"))?;
    let api_secret = std::env::var("KRAKEN_SPOT_API_SECRET")
        .map_err(|_| anyhow::anyhow!("KRAKEN_SPOT_API_SECRET not set"))?;

    let client = KrakenSpotRawHttpClient::with_credentials(
        api_key,
        api_secret,
        KrakenEnvironment::Mainnet,
        None,
        Some(60),
        None,
        None,
        None,
        None,
    )?;

    // Cancel all open orders first
    tracing::info!("\n[SETUP] Canceling all open orders...");
    let _ = client.cancel_all_orders().await;
    tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;

    // Get current ticker for reference prices
    let ticker = client.get_ticker(vec![PAIR.to_string()]).await?;
    let current_price = ticker
        .values()
        .next()
        .and_then(|t| t.last.first())
        .and_then(|p| p.parse::<f64>().ok())
        .unwrap_or(DEFAULT_PRICE);
    tracing::info!("Current {} price: ${:.4}", PAIR, current_price);

    // Calculate prices for limit/stop orders
    let limit_buy_price = current_price * LIMIT_BUY_MULTIPLIER;
    let limit_sell_price = current_price * LIMIT_SELL_MULTIPLIER;
    let stop_loss_trigger = current_price * STOP_LOSS_MULTIPLIER;
    let take_profit_trigger = current_price * TAKE_PROFIT_MULTIPLIER;

    // =========================================================================
    // TEST 1: Market Order (BUY)
    // =========================================================================
    tracing::info!("\n[TEST 1] MARKET BUY Order");
    tracing::info!("  Pair: {}, Qty: {}", PAIR, QTY);

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Market)
        .volume(QTY)
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
        }
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    // =========================================================================
    // TEST 2: Market Order (SELL) - close position
    // =========================================================================
    tracing::info!("\n[TEST 2] MARKET SELL Order");
    tracing::info!("  Pair: {}, Qty: {}", PAIR, QTY);

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenOrderType::Market)
        .volume(QTY)
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
        }
        Err(e) => tracing::error!("  FAILED: {}", e),
    }
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    // =========================================================================
    // TEST 3: Limit Order (BUY) with post-only
    // =========================================================================
    tracing::info!("\n[TEST 3] LIMIT BUY Order (post-only)");
    tracing::info!(
        "  Pair: {}, Qty: {}, Price: ${:.4}",
        PAIR,
        QTY,
        limit_buy_price
    );

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Limit)
        .volume(QTY)
        .price(format!("{:.4}", limit_buy_price))
        .oflags("post") // post-only
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    let limit_order_id = match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
            resp.txid.first().cloned()
        }
        Err(e) => {
            tracing::error!("  FAILED: {}", e);
            None
        }
    };
    tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;

    // Cancel the limit order
    if let Some(order_id) = &limit_order_id {
        tracing::info!("  Canceling limit order: {}", order_id);
        let cancel_params = KrakenSpotCancelOrderParamsBuilder::default()
            .txid(order_id.clone())
            .build()?;
        let _ = client.cancel_order(&cancel_params).await;
    }

    // =========================================================================
    // TEST 4: Limit Order (SELL)
    // =========================================================================
    tracing::info!("\n[TEST 4] LIMIT SELL Order");
    tracing::info!(
        "  Pair: {}, Qty: {}, Price: ${:.4}",
        PAIR,
        QTY,
        limit_sell_price
    );

    // First buy some to sell
    let buy_params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Market)
        .volume(QTY)
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;
    let _ = client.add_order(&buy_params).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenOrderType::Limit)
        .volume(QTY)
        .price(format!("{:.4}", limit_sell_price))
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    let limit_sell_id = match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
            resp.txid.first().cloned()
        }
        Err(e) => {
            tracing::error!("  FAILED: {}", e);
            None
        }
    };
    tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;

    // Cancel and sell at market
    if let Some(order_id) = &limit_sell_id {
        tracing::info!("  Canceling limit sell order: {}", order_id);
        let cancel_params = KrakenSpotCancelOrderParamsBuilder::default()
            .txid(order_id.clone())
            .build()?;
        let _ = client.cancel_order(&cancel_params).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;
        // Sell at market
        let sell_params = KrakenSpotAddOrderParamsBuilder::default()
            .pair(PAIR)
            .side(KrakenOrderSide::Sell)
            .order_type(KrakenOrderType::Market)
            .volume(QTY)
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .build()?;
        let _ = client.add_order(&sell_params).await;
    }
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    // =========================================================================
    // TEST 5: Stop-Loss Order
    // =========================================================================
    tracing::info!("\n[TEST 5] STOP-LOSS Order");
    tracing::info!(
        "  Pair: {}, Qty: {}, Trigger: ${:.4}",
        PAIR,
        QTY,
        stop_loss_trigger
    );

    // Buy first so we have something to stop-loss
    let buy_params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Market)
        .volume(QTY)
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;
    let _ = client.add_order(&buy_params).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenOrderType::StopLoss)
        .volume(QTY)
        .price(format!("{:.4}", stop_loss_trigger)) // trigger price
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    let stop_loss_id = match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
            resp.txid.first().cloned()
        }
        Err(e) => {
            tracing::error!("  FAILED: {}", e);
            None
        }
    };
    tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;

    // Cancel and sell at market
    if let Some(order_id) = &stop_loss_id {
        tracing::info!("  Canceling stop-loss order: {}", order_id);
        let cancel_params = KrakenSpotCancelOrderParamsBuilder::default()
            .txid(order_id.clone())
            .build()?;
        let _ = client.cancel_order(&cancel_params).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;
        let sell_params = KrakenSpotAddOrderParamsBuilder::default()
            .pair(PAIR)
            .side(KrakenOrderSide::Sell)
            .order_type(KrakenOrderType::Market)
            .volume(QTY)
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .build()?;
        let _ = client.add_order(&sell_params).await;
    }
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    // =========================================================================
    // TEST 6: Stop-Loss-Limit Order
    // =========================================================================
    tracing::info!("\n[TEST 6] STOP-LOSS-LIMIT Order");
    let stop_limit_price = stop_loss_trigger * SECONDARY_PRICE_MULTIPLIER;
    tracing::info!(
        "  Pair: {}, Qty: {}, Trigger: ${:.4}, Limit: ${:.4}",
        PAIR,
        QTY,
        stop_loss_trigger,
        stop_limit_price
    );

    // Buy first
    let buy_params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Market)
        .volume(QTY)
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;
    let _ = client.add_order(&buy_params).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenOrderType::StopLossLimit)
        .volume(QTY)
        .price(format!("{:.4}", stop_loss_trigger)) // trigger price
        .price2(format!("{:.4}", stop_limit_price)) // limit price
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    let stop_loss_limit_id = match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
            resp.txid.first().cloned()
        }
        Err(e) => {
            tracing::error!("  FAILED: {}", e);
            None
        }
    };
    tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;

    // Cancel and sell at market
    if let Some(order_id) = &stop_loss_limit_id {
        tracing::info!("  Canceling stop-loss-limit order: {}", order_id);
        let cancel_params = KrakenSpotCancelOrderParamsBuilder::default()
            .txid(order_id.clone())
            .build()?;
        let _ = client.cancel_order(&cancel_params).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;
        let sell_params = KrakenSpotAddOrderParamsBuilder::default()
            .pair(PAIR)
            .side(KrakenOrderSide::Sell)
            .order_type(KrakenOrderType::Market)
            .volume(QTY)
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .build()?;
        let _ = client.add_order(&sell_params).await;
    }
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    // =========================================================================
    // TEST 7: Take-Profit Order
    // =========================================================================
    tracing::info!("\n[TEST 7] TAKE-PROFIT Order");
    tracing::info!(
        "  Pair: {}, Qty: {}, Trigger: ${:.4}",
        PAIR,
        QTY,
        take_profit_trigger
    );

    // Buy first
    let buy_params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Market)
        .volume(QTY)
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;
    let _ = client.add_order(&buy_params).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenOrderType::TakeProfit)
        .volume(QTY)
        .price(format!("{:.4}", take_profit_trigger)) // trigger price
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    let take_profit_id = match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
            resp.txid.first().cloned()
        }
        Err(e) => {
            tracing::error!("  FAILED: {}", e);
            None
        }
    };
    tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;

    // Cancel and sell at market
    if let Some(order_id) = &take_profit_id {
        tracing::info!("  Canceling take-profit order: {}", order_id);
        let cancel_params = KrakenSpotCancelOrderParamsBuilder::default()
            .txid(order_id.clone())
            .build()?;
        let _ = client.cancel_order(&cancel_params).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;
        let sell_params = KrakenSpotAddOrderParamsBuilder::default()
            .pair(PAIR)
            .side(KrakenOrderSide::Sell)
            .order_type(KrakenOrderType::Market)
            .volume(QTY)
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .build()?;
        let _ = client.add_order(&sell_params).await;
    }
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    // =========================================================================
    // TEST 8: Take-Profit-Limit Order
    // =========================================================================
    tracing::info!("\n[TEST 8] TAKE-PROFIT-LIMIT Order");
    let tp_limit_price = take_profit_trigger * SECONDARY_PRICE_MULTIPLIER;
    tracing::info!(
        "  Pair: {}, Qty: {}, Trigger: ${:.4}, Limit: ${:.4}",
        PAIR,
        QTY,
        take_profit_trigger,
        tp_limit_price
    );

    // Buy first
    let buy_params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Buy)
        .order_type(KrakenOrderType::Market)
        .volume(QTY)
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;
    let _ = client.add_order(&buy_params).await;
    tokio::time::sleep(tokio::time::Duration::from_secs(LONG_WAIT)).await;

    let params = KrakenSpotAddOrderParamsBuilder::default()
        .pair(PAIR)
        .side(KrakenOrderSide::Sell)
        .order_type(KrakenOrderType::TakeProfitLimit)
        .volume(QTY)
        .price(format!("{:.4}", take_profit_trigger)) // trigger price
        .price2(format!("{:.4}", tp_limit_price)) // limit price
        .broker(NAUTILUS_KRAKEN_BROKER_ID)
        .build()?;

    let tp_limit_id = match client.add_order(&params).await {
        Ok(resp) => {
            tracing::info!("  SUCCESS: {:?}", resp.txid);
            if let Some(d) = &resp.descr {
                tracing::info!("  Description: {:?}", d.order);
            }
            resp.txid.first().cloned()
        }
        Err(e) => {
            tracing::error!("  FAILED: {}", e);
            None
        }
    };
    tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;

    // Cancel and sell at market
    if let Some(order_id) = &tp_limit_id {
        tracing::info!("  Canceling take-profit-limit order: {}", order_id);
        let cancel_params = KrakenSpotCancelOrderParamsBuilder::default()
            .txid(order_id.clone())
            .build()?;
        let _ = client.cancel_order(&cancel_params).await;
        tokio::time::sleep(tokio::time::Duration::from_secs(SHORT_WAIT)).await;
        let sell_params = KrakenSpotAddOrderParamsBuilder::default()
            .pair(PAIR)
            .side(KrakenOrderSide::Sell)
            .order_type(KrakenOrderType::Market)
            .volume(QTY)
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .build()?;
        let _ = client.add_order(&sell_params).await;
    }

    // Final cleanup
    tracing::info!("\n[CLEANUP] Canceling any remaining orders...");
    let _ = client.cancel_all_orders().await;

    tracing::info!("\n==========================================");
    tracing::info!("REST API Order Type Testing Complete!");
    tracing::info!(
        "Tested: Market, Limit, Stop-Loss, Stop-Loss-Limit, Take-Profit, Take-Profit-Limit"
    );

    Ok(())
}
