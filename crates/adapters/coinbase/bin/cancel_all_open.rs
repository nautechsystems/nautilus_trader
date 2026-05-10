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

//! One-shot binary that cancels every open order on the authenticated CDP key.
//!
//! Run with:
//!
//! ```bash
//! cargo run -p nautilus-coinbase --bin coinbase-cancel-all-open
//! ```
//!
//! Reads `COINBASE_API_KEY` and `COINBASE_API_SECRET` from the environment.
//! Useful for operational cleanup of any open orders on the account.

use nautilus_coinbase::{
    common::enums::{CoinbaseEnvironment, CoinbaseProductType},
    http::client::CoinbaseHttpClient,
};
use nautilus_model::identifiers::{AccountId, VenueOrderId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let client = CoinbaseHttpClient::from_env(CoinbaseEnvironment::Live)?;
    let account_id = AccountId::new("COINBASE-001");

    // Bootstrap so the order parser has the instrument cache for precision.
    let _ = client
        .request_instruments(Some(CoinbaseProductType::Spot))
        .await?;

    let reports = client
        .request_order_status_reports(account_id, None, true, None, None, Some(100))
        .await?;

    if reports.is_empty() {
        log::info!("No open orders");
        return Ok(());
    }

    let venue_ids: Vec<VenueOrderId> = reports.iter().map(|r| r.venue_order_id).collect();
    log::info!("Cancelling {} open order(s)", venue_ids.len());
    for id in &venue_ids {
        log::info!("  venue_order_id={id}");
    }

    let response = client.cancel_orders(&venue_ids).await?;
    let success_count = response.results.iter().filter(|r| r.success).count();
    log::info!(
        "Cancel response: {} of {} succeeded",
        success_count,
        response.results.len()
    );

    for result in &response.results {
        if !result.success {
            log::warn!(
                "  Failed to cancel order_id={}: {}",
                result.order_id,
                result.failure_reason
            );
        }
    }

    Ok(())
}
