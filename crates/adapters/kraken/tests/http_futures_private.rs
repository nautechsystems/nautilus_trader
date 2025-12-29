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

//! Integration tests for Kraken Futures private HTTP endpoints.
//!
//! These tests require valid API credentials set via environment variables:
//! - KRAKEN_FUTURES_API_KEY
//! - KRAKEN_FUTURES_API_SECRET
//!
//! Run with: cargo test -p nautilus-kraken --test http_futures_private -- --ignored --nocapture

use nautilus_kraken::{common::enums::KrakenEnvironment, http::KrakenFuturesRawHttpClient};
use rstest::rstest;

fn get_client() -> Option<KrakenFuturesRawHttpClient> {
    let api_key = std::env::var("KRAKEN_FUTURES_API_KEY").ok()?;
    let api_secret = std::env::var("KRAKEN_FUTURES_API_SECRET").ok()?;

    Some(
        KrakenFuturesRawHttpClient::with_credentials(
            api_key,
            api_secret,
            KrakenEnvironment::Mainnet,
            None,
            Some(30),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create client"),
    )
}

#[rstest]
#[tokio::test]
#[ignore] // Requires real API credentials
async fn test_get_accounts() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: KRAKEN_FUTURES_API_KEY/SECRET not set");
        return;
    };

    println!("Testing get_accounts...");
    let result = client.get_accounts().await;
    println!("Result: {result:?}");
    assert!(result.is_ok(), "get_accounts failed: {result:?}");
}

#[rstest]
#[tokio::test]
#[ignore] // Requires real API credentials
async fn test_get_open_orders() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: KRAKEN_FUTURES_API_KEY/SECRET not set");
        return;
    };

    println!("Testing get_open_orders...");
    let result = client.get_open_orders().await;
    println!("Result: {result:?}");
    assert!(result.is_ok(), "get_open_orders failed: {result:?}");
}

#[rstest]
#[tokio::test]
#[ignore] // Requires real API credentials
async fn test_get_open_positions() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: KRAKEN_FUTURES_API_KEY/SECRET not set");
        return;
    };

    println!("Testing get_open_positions...");
    let result = client.get_open_positions().await;
    println!("Result: {result:?}");
    assert!(result.is_ok(), "get_open_positions failed: {result:?}");
}

#[rstest]
#[tokio::test]
#[ignore] // Requires real API credentials
async fn test_get_fills() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: KRAKEN_FUTURES_API_KEY/SECRET not set");
        return;
    };

    println!("Testing get_fills (no params)...");
    let result = client.get_fills(None).await;
    println!("Result: {result:?}");
    assert!(result.is_ok(), "get_fills failed: {result:?}");
}

#[rstest]
#[tokio::test]
#[ignore] // Requires real API credentials
async fn test_get_order_events() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: KRAKEN_FUTURES_API_KEY/SECRET not set");
        return;
    };

    println!("Testing get_order_events (no params)...");
    let result = client.get_order_events(None, None, None).await;
    println!("Result: {result:?}");
    assert!(result.is_ok(), "get_order_events failed: {result:?}");
}

#[rstest]
#[tokio::test]
#[ignore] // Requires real API credentials
async fn test_get_order_events_with_since() {
    let Some(client) = get_client() else {
        eprintln!("Skipping: KRAKEN_FUTURES_API_KEY/SECRET not set");
        return;
    };

    // 7 days ago in milliseconds
    let since_ms = chrono::Utc::now().timestamp_millis() - (7 * 24 * 60 * 60 * 1000);

    println!("Testing get_order_events with since={since_ms}...");
    let result = client.get_order_events(None, Some(since_ms), None).await;
    println!("Result: {result:?}");
    assert!(
        result.is_ok(),
        "get_order_events with since failed: {result:?}"
    );
}
