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

//! Example: Using Gate.io HTTP client for futures markets.

use nautilus_gateio2::http::GateioHttpClient;
use nautilus_model::instruments::Instrument;
use tracing::info;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Starting Gate.io HTTP client example (futures markets)");

    // Create HTTP client (no credentials needed for public endpoints)
    let client = GateioHttpClient::new(
        None, // Use default HTTP URL
        None, // Use default spot WS URL
        None, // Use default futures WS URL
        None, // Use default options WS URL
        None, // No credentials for public endpoints
    );

    // Fetch USDT-margined futures contracts
    info!("\nFetching USDT-margined futures contracts...");
    match client.request_futures_contracts("usdt").await {
        Ok(contracts) => {
            info!("Successfully fetched {} futures contracts", contracts.len());
            for (i, contract) in contracts.iter().take(5).enumerate() {
                info!(
                    "Contract {}: {} (type: {}, leverage: {}-{})",
                    i + 1,
                    contract.name,
                    contract.contract_type,
                    contract.leverage_min,
                    contract.leverage_max
                );
            }
        }
        Err(e) => {
            eprintln!("Error fetching futures contracts: {}", e);
        }
    }

    // Load all instruments (spot + futures)
    info!("\nLoading all instruments...");
    match client.load_instruments().await {
        Ok(instruments) => {
            let futures_instruments: Vec<_> = instruments
                .iter()
                .filter(|i| matches!(i, nautilus_model::instruments::InstrumentAny::CryptoPerpetual(_)))
                .collect();

            info!("Successfully loaded {} futures instruments", futures_instruments.len());
            for (i, instrument) in futures_instruments.iter().take(5).enumerate() {
                info!("Instrument {}: {:?}", i + 1, instrument.id());
            }
        }
        Err(e) => {
            eprintln!("Error loading instruments: {}", e);
        }
    }

    info!("\nExample completed!");
    Ok(())
}
