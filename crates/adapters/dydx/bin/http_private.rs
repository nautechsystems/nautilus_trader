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

//! Manual verification script for dYdX HTTP private endpoints (account data).
//!
//! Tests subaccount queries including balance, open orders, and fill history.
//! Requires a wallet address but no API key authentication (dYdX v4 uses blockchain addresses).
//!
//! Usage:
//! ```bash
//! # Test against testnet (default)
//! DYDX_MNEMONIC="your mnemonic" cargo run --bin dydx-http-private -p nautilus-dydx
//!
//! # Test against mainnet
//! DYDX_MNEMONIC="your mnemonic" \
//! DYDX_HTTP_URL=https://indexer.dydx.trade \
//! cargo run --bin dydx-http-private -p nautilus-dydx -- --mainnet
//!
//! # With custom subaccount and market filter
//! DYDX_MNEMONIC="your mnemonic" cargo run --bin dydx-http-private -p nautilus-dydx -- \
//!   --subaccount 1 \
//!   --market BTC-USD
//! ```

use std::env;

use nautilus_dydx::{
    common::consts::DYDX_TESTNET_HTTP_URL, grpc::wallet::Wallet, http::client::DydxHttpClient,
};
use tracing::level_filters::LevelFilter;

const DEFAULT_SUBACCOUNT: u32 = 0;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::INFO)
        .init();

    let args: Vec<String> = env::args().collect();
    let is_mainnet = args.iter().any(|a| a == "--mainnet");
    let subaccount_number = args
        .iter()
        .position(|a| a == "--subaccount")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(DEFAULT_SUBACCOUNT);

    let market_filter = args
        .iter()
        .position(|a| a == "--market")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str());

    let mnemonic = env::var("DYDX_MNEMONIC").expect("DYDX_MNEMONIC environment variable not set");

    let http_url = if is_mainnet {
        env::var("DYDX_HTTP_URL").unwrap_or_else(|_| "https://indexer.dydx.trade".to_string())
    } else {
        env::var("DYDX_HTTP_URL").unwrap_or_else(|_| DYDX_TESTNET_HTTP_URL.to_string())
    };

    tracing::info!("Connecting to dYdX HTTP API: {}", http_url);
    tracing::info!(
        "Environment: {}",
        if is_mainnet { "MAINNET" } else { "TESTNET" }
    );
    tracing::info!("Subaccount: {}", subaccount_number);
    if let Some(market) = market_filter {
        tracing::info!("Market filter: {}", market);
    }
    tracing::info!("");

    let wallet = Wallet::from_mnemonic(&mnemonic)?;
    let account = wallet.account_offline(subaccount_number)?;
    let wallet_address = account.address.clone();
    tracing::info!("Wallet address: {}", wallet_address);
    tracing::info!("");

    let client = DydxHttpClient::new(Some(http_url), Some(30), None, !is_mainnet, None)?;

    tracing::info!("Fetching subaccount info...");
    let start = std::time::Instant::now();
    let subaccount = client
        .raw_client()
        .get_subaccount(&wallet_address, subaccount_number)
        .await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "SUCCESS: Fetched subaccount data in {:.2}s",
        elapsed.as_secs_f64()
    );
    tracing::info!(
        "   Subaccount: {}/{}",
        subaccount.subaccount.address,
        subaccount.subaccount.subaccount_number
    );
    tracing::info!("   Equity: {}", subaccount.subaccount.equity);
    tracing::info!(
        "   Free collateral: {}",
        subaccount.subaccount.free_collateral
    );
    if !subaccount.subaccount.open_perpetual_positions.is_empty() {
        tracing::info!(
            "   Open positions: {}",
            subaccount.subaccount.open_perpetual_positions.len()
        );
        for pos in subaccount
            .subaccount
            .open_perpetual_positions
            .values()
            .take(5)
        {
            tracing::info!(
                "     - {}: size={}, entry_price={}, unrealized_pnl={}",
                pos.market,
                pos.size,
                pos.entry_price,
                pos.unrealized_pnl
            );
        }
    } else {
        tracing::info!("   Open positions: 0");
    }
    tracing::info!("");

    tracing::info!("Fetching open orders...");
    let start = std::time::Instant::now();
    let orders = client
        .raw_client()
        .get_orders(&wallet_address, subaccount_number, market_filter, None)
        .await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "SUCCESS: Fetched {} open orders in {:.2}s",
        orders.len(),
        elapsed.as_secs_f64()
    );
    if !orders.is_empty() {
        tracing::info!("   Sample orders:");
        for order in orders.iter().take(5) {
            tracing::info!(
                "   - {}: {} {} @ {} ({})",
                order.id,
                order.side,
                order.size,
                order.price,
                order.status
            );
        }
        if orders.len() > 5 {
            tracing::info!("   ... and {} more", orders.len() - 5);
        }
    }
    tracing::info!("");

    tracing::info!("Fetching recent fills...");
    let start = std::time::Instant::now();
    let fills = client
        .raw_client()
        .get_fills(&wallet_address, subaccount_number, market_filter, Some(100))
        .await?;
    let elapsed = start.elapsed();

    tracing::info!(
        "SUCCESS: Fetched {} fills in {:.2}s",
        fills.fills.len(),
        elapsed.as_secs_f64()
    );
    if !fills.fills.is_empty() {
        tracing::info!("   Recent fills:");
        for fill in fills.fills.iter().take(5) {
            tracing::info!(
                "   - {}: {} {} @ {} (fee: {})",
                fill.market_type,
                fill.side,
                fill.size,
                fill.price,
                fill.fee
            );
        }
        if fills.fills.len() > 5 {
            tracing::info!("   ... and {} more", fills.fills.len() - 5);
        }
    }
    tracing::info!("");

    tracing::info!("ALL TESTS COMPLETED SUCCESSFULLY");
    tracing::info!("");
    tracing::info!("Summary:");
    tracing::info!(
        "  [PASS] get_subaccount: Equity={}, Positions={}",
        subaccount.subaccount.equity,
        subaccount.subaccount.open_perpetual_positions.len()
    );
    tracing::info!("  [PASS] get_orders: {} open orders", orders.len());
    tracing::info!("  [PASS] get_fills: {} fills fetched", fills.fills.len());

    Ok(())
}
