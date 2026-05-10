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

//! Manual smoke-test: private Bullet HTTP endpoints (account, balances, open orders).
//!
//! Usage:
//!   BULLET_KEY_FILE=~/.config/bullet/testnet-key.json \
//!   cargo run --bin bullet-http-private --features examples
//!
//! Optional env vars:
//!   BULLET_BASE_URL        — override base URL (default: testnet)
//!   BULLET_ACCOUNT_ADDRESS — main account address (if using delegate key)
//!   BULLET_SYMBOL          — symbol for open orders query (default: BTC-USD)

use nautilus_bullet::{
    common::credential::BulletCredential,
    http::client::BulletHttpClient,
    signing::{chain_data::ChainData, tx_builder::sign_user_action},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::var("BULLET_BASE_URL")
        .unwrap_or_else(|_| "https://tradingapi.testnet.bullet.xyz".to_string());
    let symbol = std::env::var("BULLET_SYMBOL").unwrap_or_else(|_| "BTC-USD".to_string());

    println!("Connecting to: {base_url}");

    // Load credentials
    let private_key = std::env::var("BULLET_PRIVATE_KEY").ok();
    let key_file = std::env::var("BULLET_KEY_FILE").ok();
    let account_address_override = std::env::var("BULLET_ACCOUNT_ADDRESS").ok();

    let creds = BulletCredential::resolve(
        private_key.as_deref(),
        key_file.as_deref(),
    )?;
    let main_addr = account_address_override.unwrap_or_else(|| creds.address());
    println!("Signing address: {}", creds.address());
    println!("Account address: {main_addr}");

    let client = BulletHttpClient::new(&base_url, 30, None)?;

    // ── Exchange info (also needed for chain data) ──────────────────────
    println!("\n=== exchange_info ===");
    let info = client.exchange_info().await?;
    println!(
        "chain_id={} symbols={} assets={}",
        info.chain_info.as_ref().map(|c| c.chain_id).unwrap_or(0),
        info.symbols.len(),
        info.assets.len(),
    );

    // Build chain data for signing
    let chain_data = ChainData::from_exchange_info(&info)?;
    println!("chain_hash decoded OK ({} bytes)", 32);

    // ── Account ────────────────────────────────────────────────────────
    println!("\n=== account ({main_addr}) ===");
    match client.account(&main_addr).await {
        Ok(acc) => {
            println!("wallet_balance:   {}", acc.total_wallet_balance);
            println!("unrealized_pnl:   {}", acc.total_unrealized_profit);
            println!("available:        {}", acc.available_balance);
            println!("positions ({}):", acc.positions.len());
            for pos in &acc.positions {
                if !pos.position_amt.is_zero() {
                    println!(
                        "  {} amt={} entry={}",
                        pos.symbol, pos.position_amt, pos.entry_price
                    );
                }
            }
        }
        Err(e) => println!("account error (non-fatal): {e}"),
    }

    // ── Balances ───────────────────────────────────────────────────────
    println!("\n=== balances ({main_addr}) ===");
    match client.balances(&main_addr).await {
        Ok(bals) => {
            for b in &bals {
                println!("  {} balance={}", b.asset, b.balance);
            }
        }
        Err(e) => println!("balances error (non-fatal): {e}"),
    }

    // ── Open orders ────────────────────────────────────────────────────
    println!("\n=== open_orders ({main_addr}, {symbol}) ===");
    match client.open_orders(&main_addr, &symbol).await {
        Ok(orders) => {
            if orders.is_empty() {
                println!("  (none)");
            }
            for o in &orders {
                println!("  {:?}", o);
            }
        }
        Err(e) => println!("open_orders error (non-fatal): {e}"),
    }

    println!("\nDone. Chain data and credentials loaded — ready for order submission.");
    let _ = chain_data;
    let _ = creds;
    Ok(())
}
