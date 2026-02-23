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

use nautilus_hyperliquid::{
    common::credential::Secrets,
    http::{
        client::HyperliquidHttpClient,
        models::{
            Cloid, HyperliquidExecAction, HyperliquidExecGrouping, HyperliquidExecLimitParams,
            HyperliquidExecOrderKind, HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
        },
    },
};
use nautilus_model::identifiers::ClientOrderId;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    // Check for testnet flag from environment (default to mainnet)
    let is_testnet =
        env::var("HYPERLIQUID_TESTNET").is_ok_and(|v| v.to_lowercase() == "true" || v == "1");

    let network_name = if is_testnet { "TESTNET" } else { "MAINNET" };
    log::info!("Starting Hyperliquid {network_name} Order Placer");

    let client = match HyperliquidHttpClient::from_env(is_testnet) {
        Ok(client) => {
            let is_testnet = client.is_testnet();
            log::info!("Client created (testnet: {is_testnet})");
            client
        }
        Err(e) => {
            log::error!("Failed to create client: {e}");
            let (pk_var, _) = Secrets::env_vars(is_testnet);
            log::error!("Make sure {pk_var} environment variable is set");
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
    let sz_decimals = meta.universe[btc_asset_id].sz_decimals;
    log::info!("BTC sz_decimals: {sz_decimals}");

    // Get the wallet address to verify authentication
    let wallet_address = client
        .get_user_address()
        .expect("Failed to get wallet address");
    log::info!("Wallet address: {wallet_address}");

    // Check account state before placing order
    log::info!("Fetching account state...");
    match client.info_clearinghouse_state(&wallet_address).await {
        Ok(state) => {
            let state_json =
                serde_json::to_string_pretty(&state).unwrap_or_else(|_| "N/A".to_string());
            log::info!("Account state: {state_json}");
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

    // Create cloid from a test ClientOrderId (production-like)
    let client_order_id = ClientOrderId::from("O-20241210-TEST-001-001-1");
    let cloid = Cloid::from_client_order_id(client_order_id);
    log::info!("ClientOrderId: {client_order_id}");
    let cloid_hex = cloid.to_hex();
    log::info!("Cloid: {cloid_hex}");

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
        cloid: Some(cloid),
    };

    log::info!("Order details:");
    log::info!("  Asset: {btc_asset_id} (BTC)");
    log::info!("  Side: BUY");
    log::info!("  Price: ${limit_price}");
    log::info!("  Size: 0.001 BTC");
    let order_cloid = order.cloid.as_ref().unwrap().to_hex();
    log::info!("  Cloid: {order_cloid}");

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
        let action_json_pretty = serde_json::to_string_pretty(&action_json)?;
        log::debug!("Action JSON: {action_json_pretty}");
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
