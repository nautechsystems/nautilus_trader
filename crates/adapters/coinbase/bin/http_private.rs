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

//! Sanity-check binary that exercises the Coinbase authenticated REST API.
//!
//! Run with:
//!
//! ```bash
//! cargo run -p nautilus-coinbase --bin coinbase-http-private
//! ```
//!
//! Requires `COINBASE_API_KEY` and `COINBASE_API_SECRET` in the environment
//! (CDP API key name and PEM-encoded EC private key). Reads only; submits
//! no orders. Exercises the new typed domain methods end-to-end so the
//! parse path and HTTP signing can be verified against a live account.

use nautilus_coinbase::{
    common::enums::{CoinbaseEnvironment, CoinbaseProductType},
    http::client::CoinbaseHttpClient,
};
use nautilus_model::identifiers::{AccountId, InstrumentId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let client = CoinbaseHttpClient::from_env(CoinbaseEnvironment::Live)?;
    let account_id = AccountId::new("COINBASE-001");

    log::info!("Bootstrapping spot instruments");
    let instruments = client
        .request_instruments(Some(CoinbaseProductType::Spot))
        .await?;
    log::info!("Cached {} spot instruments", instruments.len());

    // Print the venue's gating flags for common products so we can tell
    // whether a specific pair (e.g. BTC-USD vs BTC-USDC) is order-eligible
    // for this account before we try to submit against it.
    for product_id in ["BTC-USD", "BTC-USDC"] {
        match client.get_product(product_id).await {
            Ok(value) => {
                let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                let trading_disabled = value
                    .get("trading_disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let is_disabled = value
                    .get("is_disabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let view_only = value
                    .get("view_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let cancel_only = value
                    .get("cancel_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let limit_only = value
                    .get("limit_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let post_only = value
                    .get("post_only")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let auction_mode = value
                    .get("auction_mode")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let base_min = value
                    .get("base_min_size")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let quote_min = value
                    .get("quote_min_size")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                log::info!(
                    "{product_id}: status={status} trading_disabled={trading_disabled} is_disabled={is_disabled} view_only={view_only} cancel_only={cancel_only} limit_only={limit_only} post_only={post_only} auction_mode={auction_mode} base_min={base_min} quote_min={quote_min}"
                );
            }
            Err(e) => log::warn!("Failed to look up {product_id}: {e:?}"),
        }
    }

    log::info!("Listing portfolios visible to this API key");

    match client.get_portfolios().await {
        Ok(value) => {
            let portfolios = value
                .get("portfolios")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            log::info!("Found {} portfolio(s)", portfolios.len());
            for p in &portfolios {
                let name = p
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unnamed>");
                let uuid = p
                    .get("uuid")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<no uuid>");
                let type_ = p
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<no type>");
                let deleted = p.get("deleted").and_then(|v| v.as_bool()).unwrap_or(false);
                log::info!("  name={name} type={type_} uuid={uuid} deleted={deleted}");
            }
        }
        Err(e) => log::error!("Failed to list portfolios: {e:?}"),
    }

    log::info!("Requesting account state");
    match client.request_account_state(account_id).await {
        Ok(state) => {
            log::info!("Account has {} balance(s)", state.balances.len());
            for balance in &state.balances {
                log::info!(
                    "  {} total={} free={} locked={}",
                    balance.currency.code,
                    balance.total,
                    balance.free,
                    balance.locked,
                );
            }
        }
        Err(e) => log::error!("{e:?}"),
    }

    log::info!("Requesting open order status reports");

    match client
        .request_order_status_reports(account_id, None, true, None, None, Some(50))
        .await
    {
        Ok(reports) => {
            log::info!("Received {} open order report(s)", reports.len());
            for report in reports.iter().take(5) {
                log::debug!("{report:?}");
            }
        }
        Err(e) => log::error!("{e:?}"),
    }

    log::info!("Requesting recent BTC-USD order history");
    let btc_usd = InstrumentId::from("BTC-USD.COINBASE");
    match client
        .request_order_status_reports(account_id, Some(btc_usd), false, None, None, Some(25))
        .await
    {
        Ok(reports) => {
            log::info!("Received {} BTC-USD order report(s)", reports.len());
            for report in reports.iter().take(5) {
                log::debug!("{report:?}");
            }
        }
        Err(e) => log::error!("{e:?}"),
    }

    // Ask Coinbase to validate (not submit) a tiny limit order for each
    // candidate product. The `/orders/preview` endpoint returns the same
    // error shape as `/orders` without placing anything, so we can tell
    // which product the account can actually trade.
    for (product_id, quote_size) in [("BTC-USD", "1"), ("BTC-USDC", "1")] {
        let body = serde_json::json!({
            "product_id": product_id,
            "side": "BUY",
            "order_configuration": {
                "market_market_ioc": {
                    "quote_size": quote_size
                }
            }
        });

        match client.preview_order(&body).await {
            Ok(value) => {
                let err = value.get("error_response");
                let preview_failure_reason = value
                    .get("preview_failure_reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                log::info!(
                    "Preview {product_id} quote_size={quote_size}: preview_failure_reason={preview_failure_reason} error={err:?}"
                );
            }
            Err(e) => log::warn!("Preview {product_id} failed: {e:?}"),
        }
    }

    log::info!("Requesting recent fill reports");

    match client
        .request_fill_reports(account_id, None, None, None, None, Some(25))
        .await
    {
        Ok(reports) => {
            log::info!("Received {} fill report(s)", reports.len());
            for report in reports.iter().take(5) {
                log::debug!("{report:?}");
            }
        }
        Err(e) => log::error!("{e:?}"),
    }

    Ok(())
}
