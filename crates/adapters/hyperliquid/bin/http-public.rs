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

//! Example demonstrating the NT-aware HTTP client for Hyperliquid (public endpoints).
//!
//! This example shows:
//! - Public API calls (no authentication required)
//! - Rate limiting with local token bucket (fallback when NT not available)
//! - Retry logic with exponential backoff
//! - Deterministic JSON serialization for consistent behavior
//!
//! ## NT Integration
//!
//! To wire this client to your existing NT infrastructure, implement the `HttpProvider` trait
//! and optionally `RateLimitProvider` and `JsonProvider`, then use:
//!
//! ```rust,no_run
//! use nautilus_hyperliquid::http::{IntegrationDeps, HttpProvider};
//!
//! let client = HyperliquidHttpClient::public(network)
//!     .with_nt(IntegrationDeps {
//!         http: Arc::new(YourHttpProviderImpl { /* ... */ }),
//!         limiter: Some(Arc::new(YourRateLimitProvider)),
//!         canonical_json: None,
//!     });
//! ```

use std::time::Duration;

use nautilus_hyperliquid::{
    common::consts::HyperliquidNetwork,
    http::{
        RateLimitPolicy,
        client::{HyperliquidHttpClient, RetryPolicy},
    },
};
use tracing::{info, level_filters::LevelFilter, warn};
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Structured logging with env-controlled filter (e.g. RUST_LOG=debug)
    let env = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(env)
        .with_max_level(LevelFilter::INFO)
        .init();

    let network = HyperliquidNetwork::from_env();
    info!(
        component = "http_public",
        ?network,
        "creating NT-aware HTTP client"
    );

    // Create a public client (no signing, local rate limiter as fallback)
    let client = HyperliquidHttpClient::public(network)?.with_retry_policy(RetryPolicy {
        max_retries: 3,
        base_delay: Duration::from_millis(100),
        max_delay: Duration::from_secs(2),
        jitter: true,
    });

    // Adjust rate limiting policy if needed
    client
        .set_rate_limit_policy(RateLimitPolicy {
            capacity: 600,
            refill_per_min: 600,
        })
        .await;

    info!(component = "http_public", "HTTP client configured");

    // Fetch metadata
    match client.info_meta().await {
        Ok(meta) => {
            info!(
                component = "http_public",
                universe_count = meta.universe.len(),
                "fetched metadata"
            );
        }
        Err(e) => {
            warn!(component = "http_public", error = %e, "failed to fetch metadata");
        }
    }

    // Fetch BTC orderbook
    match client.info_l2_book("BTC").await {
        Ok(book) => {
            let best_bid = book
                .levels
                .first()
                .and_then(|bids| bids.first())
                .map(|l| l.px.clone())
                .unwrap_or_default();
            let best_ask = book
                .levels
                .get(1)
                .and_then(|asks| asks.first())
                .map(|l| l.px.clone())
                .unwrap_or_default();

            info!(
                component = "http_public",
                best_bid = %best_bid,
                best_ask = %best_ask,
                "fetched BTC order book"
            );
        }
        Err(e) => {
            warn!(component = "http_public", error = %e, "failed to fetch order book");
        }
    }

    // Example with Noop rate limiter (for when you have a global limiter)
    info!(component = "http_public", "demonstrating Noop mode");
    let noop_client = HyperliquidHttpClient::public(network)?;
    // In reality you would configure this with RateLimitMode::Noop when creating a private client

    // This would work the same way, but rate limiting is disabled locally
    match noop_client.info_meta().await {
        Ok(_) => {
            info!(
                component = "http_public",
                "Noop client fetched metadata successfully"
            );
        }
        Err(e) => {
            warn!(component = "http_public", error = %e, "Noop client failed");
        }
    }

    info!(component = "http_public", "HTTP public example completed");
    Ok(())
}
