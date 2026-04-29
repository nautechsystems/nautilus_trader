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

//! Sanity-check binary that exercises the Coinbase public REST API.
//!
//! Run with:
//!
//! ```bash
//! cargo run -p nautilus-coinbase --bin coinbase-http-public
//! ```
//!
//! Requires no credentials. Hits the live Coinbase Advanced Trade endpoints
//! and logs a short summary for each call.

use nautilus_coinbase::{common::enums::CoinbaseProductType, http::client::CoinbaseHttpClient};
use nautilus_model::instruments::Instrument;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let client = CoinbaseHttpClient::default();

    log::info!("Requesting all spot instruments");
    let instruments = client
        .request_instruments(Some(CoinbaseProductType::Spot))
        .await?;
    log::info!("Received {} spot instruments", instruments.len());
    if let Some(btc_usd) = instruments
        .iter()
        .find(|i| i.id().symbol.as_str() == "BTC-USD")
    {
        log::info!(
            "BTC-USD precision: price={}, size={}",
            btc_usd.price_precision(),
            btc_usd.size_precision(),
        );
    }

    log::info!("Requesting BTC-USD product book");
    match client.get_product_book("BTC-USD", Some(5)).await {
        Ok(book) => log::debug!("{book:?}"),
        Err(e) => log::error!("{e:?}"),
    }

    log::info!("Requesting recent BTC-USD market trades");
    match client.get_market_trades("BTC-USD", 3).await {
        Ok(trades) => log::debug!("{trades:?}"),
        Err(e) => log::error!("{e:?}"),
    }

    Ok(())
}
