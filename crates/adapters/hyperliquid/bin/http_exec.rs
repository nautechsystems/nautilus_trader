// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{env, str::FromStr};

use nautilus_hyperliquid::http::{
    client::HyperliquidHttpClient,
    models::{
        HyperliquidExecAction, HyperliquidExecGrouping, HyperliquidExecLimitParams,
        HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
    },
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let _ = env::var("HYPERLIQUID_TESTNET_PK")
        .expect("HYPERLIQUID_TESTNET_PK environment variable not set");

    log::info!("Starting Hyperliquid Testnet Order Placer");

    let client = match HyperliquidHttpClient::from_env() {
        Ok(client) => {
            log::info!("Client created (testnet: {})", client.is_testnet());
            client
        }
        Err(e) => {
            log::error!("Failed to create client: {e}");
            return Err(e.into());
        }
    };

    log::info!("Fetching market metadata...");
    let meta = client.info_meta().await?;

    // Debug: Print all assets
    log::debug!("Available assets:");
    for (idx, asset) in meta.universe.iter().enumerate() {
        log::debug!(
            "  [{}] {} (sz_decimals: {})",
            idx,
            asset.name,
            asset.sz_decimals
        );
    }

    let btc_asset_id = meta
        .universe
        .iter()
        .position(|asset| asset.name == "BTC")
        .expect("BTC not found in universe");

    log::info!("BTC asset ID: {btc_asset_id}");
    log::info!(
        "BTC sz_decimals: {}",
        meta.universe[btc_asset_id].sz_decimals
    );

    // Get the wallet address to verify authentication
    let wallet_address = client
        .get_user_address()
        .expect("Failed to get wallet address");
    log::info!("Wallet address: {wallet_address}");

    // Check account state before placing order
    log::info!("Fetching account state...");
    match client.info_clearinghouse_state(&wallet_address).await {
        Ok(state) => {
            log::info!(
                "Account state: {}",
                serde_json::to_string_pretty(&state).unwrap_or_else(|_| "N/A".to_string())
            );
        }
        Err(e) => {
            log::warn!("Failed to fetch account state: {e}");
        }
    }

    log::info!("Fetching BTC order book...");
    let book = client.info_l2_book("BTC").await?;

    let best_bid_str = &book.levels[0][0].px;
    let best_bid = Decimal::from_str(best_bid_str)?;

    log::info!("Best bid: ${best_bid}");

    // BTC prices on Hyperliquid must be whole dollars (no decimal places)
    let limit_price = (best_bid * dec!(0.95)).round();
    log::info!("Limit order price: ${limit_price}");

    let order = HyperliquidExecPlaceOrderRequest {
        asset: btc_asset_id as u32,
        is_buy: true,
        price: limit_price,
        size: dec!(0.001),
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Limit {
            limit: HyperliquidExecLimitParams {
                tif: HyperliquidExecTif::Gtc,
            },
        },
        cloid: None,
    };

    log::info!("Order details:");
    log::info!("  Asset: {btc_asset_id} (BTC)");
    log::info!("  Side: BUY");
    log::info!("  Price: ${limit_price}");
    log::info!("  Size: 0.001 BTC");

    log::info!("Placing order...");

    // Create the action using the typed HyperliquidExecAction enum
    let action = HyperliquidExecAction::Order {
        orders: vec![order],
        grouping: HyperliquidExecGrouping::Na,
        builder: None,
    };

    log::debug!("ExchangeAction: {action:?}");

    // Also log the action as JSON
    if let Ok(action_json) = serde_json::to_value(&action) {
        log::debug!(
            "Action JSON: {}",
            serde_json::to_string_pretty(&action_json)?
        );
    }

    match client.post_action_exec(&action).await {
        Ok(response) => {
            log::info!("Order placed successfully!");
            log::info!("Response: {response:#?}");

            // Also log as JSON for easier reading
            if let Ok(json) = serde_json::to_string_pretty(&response) {
                log::info!("Response JSON:\n{json}");
            }
        }
        Err(e) => {
            log::error!("Failed to place order: {e}");
            log::error!("Error details: {e:?}");
            return Err(e.into());
        }
    }
    log::info!("Done!");
    Ok(())
}
