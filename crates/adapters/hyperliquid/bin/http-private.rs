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

//! Example demonstrating the NT-aware HTTP client for Hyperliquid (private authenticated endpoints).
//!
//! This example shows:
//! - Private API calls (authentication required)
//! - Account information queries
//! - Order and fill history
//! - NT integration with rate limiting
//!
//! ## Setup
//!
//! Set environment variables:
//! - `HYPERLIQUID_PK`: Your private key
//! - `HYPERLIQUID_NETWORK`: "mainnet" or "testnet" (default: "testnet")
//!
//! ## NT Integration
//!
//! To wire this client to your existing NT infrastructure:
//!
//! ```rust,no_run
//! use nautilus_hyperliquid::http::{IntegrationDeps, HttpProvider, RateLimitMode};
//!
//! let client = HyperliquidHttpClient::private(&secrets, RateLimitMode::Noop)?
//!     .with_nt(IntegrationDeps {
//!         http: Arc::new(YourHttpProviderImpl { /* ... */ }),
//!         limiter: Some(Arc::new(YourRateLimitProvider)),
//!         canonical_json: None,
//!     });
//! ```

use std::time::Duration;

use nautilus_hyperliquid::{
    common::credential::Secrets,
    http::{
        RateLimitMode,
        client::{HyperliquidHttpClient, RetryPolicy},
    },
};
use tracing::{error, info, level_filters::LevelFilter, warn};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Structured logging with env-controlled filter (e.g. RUST_LOG=debug)
    let env = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(env)
        .with_max_level(LevelFilter::INFO)
        .init();

    // Try to load credentials from environment
    let secrets = match Secrets::from_env() {
        Ok(secrets) => secrets,
        Err(_) => {
            warn!(
                component = "http_private",
                "No HYPERLIQUID_PK environment variable found. This example requires credentials."
            );
            return Ok(());
        }
    };

    info!(
        component = "http_private",
        network = ?secrets.network,
        "creating authenticated NT-aware HTTP client"
    );

    // Create a private client (with signing capability)
    // Use Noop mode if you have NT global rate limiting, otherwise LocalTokenBucket
    let client = HyperliquidHttpClient::private(&secrets, RateLimitMode::LocalTokenBucket)?
        .with_retry_policy(RetryPolicy {
            max_retries: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(5),
            jitter: true,
        });

    info!(
        component = "http_private",
        "authenticated HTTP client configured"
    );

    // For demonstration purposes, use a placeholder address
    // In practice, you would derive this from the private key
    let user_address = "0x0000000000000000000000000000000000000000"; // Placeholder

    // Example 1: Get user fills (trading history)
    info!(component = "http_private", "fetching user fills");
    match client.info_user_fills(user_address).await {
        Ok(fills) => {
            info!(
                component = "http_private",
                fill_count = fills.fills.len(),
                "fetched user fills successfully"
            );

            // Show details of recent fills (up to 3)
            for (i, fill) in fills.fills.iter().take(3).enumerate() {
                info!(
                    component = "http_private",
                    fill_index = i,
                    coin = %fill.coin,
                    side = %fill.side,
                    px = %fill.px,
                    sz = %fill.sz,
                    "recent fill"
                );
            }
        }
        Err(e) => {
            error!(component = "http_private", error = %e, "failed to fetch user fills");
        }
    }

    // Example 2: Get order status (if we had an order ID)
    // This would typically be used after placing an order
    info!(
        component = "http_private",
        "demonstrating order status query"
    );
    let example_order_id = 12345u64; // This would be a real order ID in practice
    match client
        .info_order_status(user_address, example_order_id)
        .await
    {
        Ok(status) => {
            info!(
                component = "http_private",
                order_id = example_order_id,
                status = ?status,
                "fetched order status"
            );
        }
        Err(e) => {
            // Expected to fail since this is a fake order ID
            info!(
                component = "http_private",
                order_id = example_order_id,
                error = %e,
                "order status query failed (expected for demo order ID)"
            );
        }
    }

    // Example 3: Demonstrate NT integration patterns
    info!(component = "http_private", "NT integration ready");
    info!(
        component = "http_private",
        "To integrate with NT, provide IntegrationDeps with your HTTP client, rate limiter, and JSON canonicalizer"
    );

    info!(component = "http_private", "HTTP private example completed");
    Ok(())
}
