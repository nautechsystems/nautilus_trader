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

//! Smoke test for HIP-4 outcome order placement through the standard order
//! path (no `splitOutcome` required).
//!
//! Loads instruments (including outcomes), places a far-from-touch limit buy
//! on `+<encoding>.HYPERLIQUID`, then cancels it. The point is to confirm the
//! venue accepts the order via the ordinary `Order` action — i.e. that a
//! strategy can trade outcome instruments with no special methods.
//!
//! Env vars (mainnet by default; set `HYPERLIQUID_TESTNET=1` for testnet):
//!
//! - `HYPERLIQUID_OUTCOME_SYMBOL` (e.g. `+500.HYPERLIQUID`).
//! - `HYPERLIQUID_OUTCOME_PX` (limit price, e.g. `0.005`).
//! - `HYPERLIQUID_OUTCOME_QTY` (size, e.g. `600`).

use std::{env, str::FromStr};

use nautilus_hyperliquid::{
    common::{credential::Secrets, enums::HyperliquidEnvironment},
    http::client::HyperliquidHttpClient,
};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    types::{Price, Quantity},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let environment =
        if env::var("HYPERLIQUID_TESTNET").is_ok_and(|v| v.to_lowercase() == "true" || v == "1") {
            HyperliquidEnvironment::Testnet
        } else {
            HyperliquidEnvironment::Mainnet
        };

    let symbol =
        env::var("HYPERLIQUID_OUTCOME_SYMBOL").unwrap_or_else(|_| "+500.HYPERLIQUID".to_string());
    let px = Price::from_str(
        env::var("HYPERLIQUID_OUTCOME_PX")
            .unwrap_or_else(|_| "0.0050".to_string())
            .as_str(),
    )?;
    let qty = Quantity::from_str(
        env::var("HYPERLIQUID_OUTCOME_QTY")
            .unwrap_or_else(|_| "2010".to_string())
            .as_str(),
    )?;

    log::info!("Hyperliquid {environment:?} outcome-order smoke: {symbol} buy {qty} @ {px}");

    let mut client = HyperliquidHttpClient::from_env(environment).inspect_err(|e| {
        let (pk_var, _) = Secrets::env_vars(environment);
        log::error!("Failed to create client: {e}; ensure {pk_var} is set");
    })?;

    let wallet = client.get_user_address()?;
    client.set_account_id(AccountId::new(format!("HYPERLIQUID-{wallet}")));
    log::info!("Wallet: {wallet}");

    log::info!("Loading instruments (including outcomes)...");
    let instruments = client.request_instruments().await?;
    for inst in &instruments {
        client.cache_instrument(inst);
    }
    log::info!("Cached {} instruments", instruments.len());

    let instrument_id = InstrumentId::from(symbol.as_str());
    let client_order_id = ClientOrderId::from("O-OUTCOME-SMOKE-001");

    log::info!("Submitting limit buy (no splitOutcome first) ...");
    let report = client
        .submit_order(
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            qty,
            TimeInForce::Gtc,
            Some(px),
            None,
            false, // post_only
            false, // reduce_only
        )
        .await?;
    log::info!(
        "Order accepted: venue_order_id={:?} status={:?}",
        report.venue_order_id,
        report.order_status,
    );

    log::info!("Cancelling the resting order to free up locked USDH...");
    client
        .cancel_order(instrument_id, Some(client_order_id), None)
        .await?;
    log::info!("Cancel acknowledged");

    Ok(())
}
