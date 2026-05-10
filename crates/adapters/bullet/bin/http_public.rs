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

//! Manual smoke-test: public Bullet HTTP endpoints (exchange info, depth).
//!
//! Usage:
//!   cargo run --bin bullet-http-public --features examples
//!
//! Optionally set BULLET_BASE_URL to override the testnet default.

use nautilus_bullet::http::client::BulletHttpClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::var("BULLET_BASE_URL")
        .unwrap_or_else(|_| "https://tradingapi.testnet.bullet.xyz".to_string());

    println!("Connecting to: {base_url}");

    let client = BulletHttpClient::new(&base_url, 30, None)?;

    // ── Exchange info ──────────────────────────────────────────────────────
    println!("\n=== exchange_info ===");
    let info = client.exchange_info().await?;
    println!("chain_hash: {:?}", info.chain_hash);
    if let Some(ci) = &info.chain_info {
        println!("chain_id:   {}", ci.chain_id);
        println!("chain_name: {}", ci.chain_name);
    }
    println!("assets:  {} entries", info.assets.len());
    println!("symbols: {} entries", info.symbols.len());

    for sym in info.symbols.iter().take(5) {
        let tick = sym.tick_size().map(|d| d.to_string()).unwrap_or_else(|| "n/a".into());
        let step = sym.step_size().map(|d| d.to_string()).unwrap_or_else(|| "n/a".into());
        println!(
            "  {} (market_id={}) price_prec={} qty_prec={} tick={tick} step={step}",
            sym.symbol, sym.market_id, sym.price_precision, sym.quantity_precision,
        );
    }
    if info.symbols.len() > 5 {
        println!("  ... ({} more)", info.symbols.len() - 5);
    }

    // ── Depth ─────────────────────────────────────────────────────────────
    if let Some(first_sym) = info.symbols.first() {
        println!("\n=== depth ({}) ===", first_sym.symbol);
        match client.depth(&first_sym.symbol, Some(5)).await {
            Ok(book) => {
                println!("last_update_id: {}", book.last_update_id);
                println!("bids (top 3):");
                for [p, q] in book.bids.iter().take(3) {
                    println!("  {p} @ {q}");
                }
                println!("asks (top 3):");
                for [p, q] in book.asks.iter().take(3) {
                    println!("  {p} @ {q}");
                }
            }
            Err(e) => println!("depth error (non-fatal): {e}"),
        }

        // ── Funding rate ───────────────────────────────────────────────────
        println!("\n=== funding_rate ({}) ===", first_sym.symbol);
        match client.funding_rate(&first_sym.symbol).await {
            Ok(fr) => println!("funding_rate: {:?}", fr),
            Err(e) => println!("funding_rate error (non-fatal): {e}"),
        }
    }

    println!("\nDone.");
    Ok(())
}
