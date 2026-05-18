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

use std::env;

use nautilus_hyperliquid::{
    common::enums::HyperliquidEnvironment, http::client::HyperliquidHttpClient,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    nautilus_common::logging::ensure_logging_initialized();

    let args: Vec<String> = env::args().collect();
    let environment = if args.get(1).is_some_and(|s| s == "testnet") {
        HyperliquidEnvironment::Testnet
    } else {
        HyperliquidEnvironment::Mainnet
    };

    log::info!("Starting Hyperliquid HTTP private example");

    match environment {
        HyperliquidEnvironment::Testnet => {
            log::info!("Testnet mode - ensure HYPERLIQUID_TESTNET_PK environment variable is set");
        }
        HyperliquidEnvironment::Mainnet => {
            log::info!("Mainnet mode - ensure HYPERLIQUID_PK environment variable is set");
        }
    }

    let client = match HyperliquidHttpClient::from_env(environment) {
        Ok(client) => {
            log::info!("Environment: {environment:?}");
            client
        }
        Err(e) => {
            let (env_var, _) =
                nautilus_hyperliquid::common::credential::credential_env_vars(environment);
            log::warn!(
                "No credentials found in environment ({env_var}): {e}, skipping authenticated examples"
            );
            return Ok(());
        }
    };

    // For demonstration, use a placeholder address
    let user_address = "0x0000000000000000000000000000000000000000";

    match client.info_user_fills(user_address).await {
        Ok(fills) => {
            log::info!("Fetched {} fills", fills.len());
            for (i, fill) in fills.iter().take(3).enumerate() {
                log::info!("Fill {}: {} {} @ {}", i, fill.side, fill.sz, fill.px);
            }
        }
        Err(e) => {
            log::info!("Failed to fetch fills: {e}");
        }
    }

    let example_order_id = 12345u64;
    match client
        .info_order_status(user_address, example_order_id)
        .await
    {
        Ok(status) => {
            log::info!("Order status: {status:?}");
        }
        Err(e) => {
            log::info!("Order status query failed (expected for demo ID): {e}");
        }
    }

    Ok(())
}
