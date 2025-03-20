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

use nautilus_coinbase_intx::{
    common::consts::COINBASE_INTX_REST_SANDBOX_URL, http::client::CoinbaseIntxHttpClient,
};
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .init();

    // Example of using custom url for the Coinbase International sandbox
    let base_url = COINBASE_INTX_REST_SANDBOX_URL.to_string();
    let client = CoinbaseIntxHttpClient::new(Some(base_url), Some(60));

    let resp = client.request_instruments().await?;
    for inst in resp {
        tracing::info!("{inst:?}");
    }

    Ok(())
}
