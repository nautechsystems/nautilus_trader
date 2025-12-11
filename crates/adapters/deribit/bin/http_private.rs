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

//! Example demonstrating Deribit private API usage.
//!
//! # Prerequisites
//!
//! Set environment variables with your Deribit API credentials:
//! - For mainnet: `DERIBIT_API_KEY` and `DERIBIT_API_SECRET`
//! - For testnet: `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET`

use nautilus_deribit::http::client::DeribitHttpClient;
use nautilus_model::identifiers::AccountId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let is_testnet = !std::env::args().any(|x| x == "--mainnet");
    let client =
        DeribitHttpClient::new_with_env(None, None, is_testnet, Some(30), None, None, None, None)?;

    let account_id = AccountId::from("DERIBIT-001");

    // Fetch account state for all currencies
    println!("Fetching account state...");
    match client.request_account_state(account_id).await {
        Ok(account_state) => println!("{account_state:?}"),
        Err(e) => {
            eprintln!("✗ Failed to fetch account state: {e}");
            return Err(e.into());
        }
    }

    Ok(())
}
