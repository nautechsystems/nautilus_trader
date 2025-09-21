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

use std::str::FromStr;

use nautilus_bitmex::http::client::BitmexHttpClient;
use nautilus_model::identifiers::InstrumentId;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    let client = BitmexHttpClient::from_env()?;

    let instrument_id = InstrumentId::from_str("XBTUSD.BITMEX")?;
    let instrument = client.request_instrument(instrument_id).await?;

    match instrument {
        Some(inst) => tracing::info!(instrument = ?inst, "Retrieved instrument"),
        None => tracing::warn!("Instrument XBTUSD.BITMEX not returned from BitMEX"),
    }

    Ok(())
}
