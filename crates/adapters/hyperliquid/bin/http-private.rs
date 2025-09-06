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

use nautilus_hyperliquid::http::client::HyperliquidHttpClient;
use tracing::level_filters::LevelFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    // Try to create authenticated client from environment
    let client = match HyperliquidHttpClient::from_env() {
        Ok(client) => client,
        Err(_) => {
            tracing::warn!(
                "No credentials found in environment (HYPERLIQUID_PK). Skipping authenticated examples."
            );
            return Ok(());
        }
    };

    // For demonstration, use a placeholder address
    let user_address = "0x0000000000000000000000000000000000000000";

    // Get user fills
    match client.info_user_fills(user_address).await {
        Ok(fills) => {
            tracing::info!("Fetched {} fills", fills.fills.len());
            for (i, fill) in fills.fills.iter().take(3).enumerate() {
                tracing::info!("Fill {}: {} {} @ {}", i, fill.side, fill.sz, fill.px);
            }
        }
        Err(e) => {
            tracing::info!("Failed to fetch fills: {}", e);
        }
    }

    // Get order status (example with fake order ID)
    let example_order_id = 12345u64;
    match client
        .info_order_status(user_address, example_order_id)
        .await
    {
        Ok(status) => {
            tracing::info!("Order status: {:?}", status);
        }
        Err(e) => {
            tracing::info!("Order status query failed (expected for demo ID): {}", e);
        }
    }

    Ok(())
}
