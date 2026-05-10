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

//! Manual smoke-test: place a limit order then cancel it on Bullet testnet.
//!
//! Usage:
//!   BULLET_KEY_FILE=~/.config/bullet/id.json \
//!   BULLET_SYMBOL=BTC-USD \
//!   cargo run --bin bullet-ws-exec --features examples
//!
//! Optional env:
//!   BULLET_BASE_URL        — override base URL (default: testnet)
//!   BULLET_ACCOUNT_ADDRESS — main account if using delegate key
//!   BULLET_SYMBOL          — market symbol (default: BTC-USD)

use bullet_exchange_interface::{
    address::Address,
    decimals::PositiveDecimal,
    message::{CancelOrderArgs, NewOrderArgs, UserAction},
    types::{ClientOrderId as BulletClientOrderId, MarketId, OrderId, OrderType, Side},
};
use nautilus_bullet::{
    common::{
        credential::BulletCredential,
        models::SymbolPrecision,
        parse::{snap_price, snap_qty},
    },
    http::client::BulletHttpClient,
    signing::{chain_data::ChainData, tx_builder::sign_user_action},
};
use rust_decimal::Decimal;
use std::str::FromStr;

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s).expect("valid decimal literal")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let base_url = std::env::var("BULLET_BASE_URL")
        .unwrap_or_else(|_| "https://tradingapi.testnet.bullet.xyz".to_string());
    let symbol = std::env::var("BULLET_SYMBOL").unwrap_or_else(|_| "BTC-USD".to_string());

    println!("Connecting to: {base_url}  symbol: {symbol}");

    let private_key = std::env::var("BULLET_PRIVATE_KEY").ok();
    let key_file = std::env::var("BULLET_KEY_FILE").ok();
    let account_address_override = std::env::var("BULLET_ACCOUNT_ADDRESS").ok();

    let creds = BulletCredential::resolve(private_key.as_deref(), key_file.as_deref())?;
    let main_addr = account_address_override.unwrap_or_else(|| creds.address());
    println!("Signing address: {}", creds.address());
    println!("Account address: {main_addr}");

    let client = BulletHttpClient::new(&base_url, 30, None)?;

    // Fetch chain data + exchange info
    let info = client.exchange_info().await?;
    let chain_data = ChainData::from_exchange_info(&info)?;
    println!("chain_id={}", info.chain_info.as_ref().map(|c| c.chain_id).unwrap_or(0));

    let sym_info = info
        .symbols
        .iter()
        .find(|s| s.symbol == symbol)
        .ok_or_else(|| anyhow::anyhow!("symbol {symbol} not found in exchangeInfo"))?;

    let prec = SymbolPrecision::from_symbol_info(sym_info);
    println!("tick={:?}  step={:?}", prec.tick_size, prec.step_size);

    // ── Fetch current mid price from depth ────────────────────────────────
    let book = client.depth(&symbol, Some(1)).await?;
    let best_bid: Decimal = book
        .bids
        .first()
        .and_then(|[p, _]| p.parse().ok())
        .unwrap_or_else(|| dec("50000"));
    // Place limit buy 5% below best bid (safely away from the market)
    let raw_price = best_bid * dec("0.95");
    let price = snap_price(raw_price, prec.tick_size, true);
    let qty = snap_qty(dec("0.001"), prec.step_size);

    let price_pd = PositiveDecimal::try_from(price)?;
    let size_pd = PositiveDecimal::try_from(qty)?;

    println!("\nbest_bid={best_bid}  order_price={price}  order_qty={qty}");

    // Use microsecond timestamp as client_order_id for uniqueness across runs
    let client_order_id = BulletClientOrderId(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64,
    );

    let new_order = NewOrderArgs {
        price: price_pd,
        size: size_pd,
        side: Side::Bid,
        order_type: OrderType::Limit,
        reduce_only: false,
        client_order_id: Some(client_order_id),
        pending_tpsl_pair: None,
    };

    let place_action = UserAction::<Address>::PlaceOrders {
        market_id: MarketId(prec.market_id),
        orders: vec![new_order],
        replace: false,
        sub_account_index: None,
    };

    // ── Place order ───────────────────────────────────────────────────────
    println!("\n=== place_order ===");
    let tx_b64 = sign_user_action(place_action, &creds, &chain_data, None)?;
    println!("tx (base64, first 40 chars): {}...", &tx_b64[..tx_b64.len().min(40)]);

    let resp = client.submit_tx(tx_b64).await?;
    println!("tx_id: {}", resp.id);

    // Wait a moment for settlement
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    // ── Check open orders ─────────────────────────────────────────────────
    println!("\n=== open_orders after place ===");
    let orders = client.open_orders(&main_addr, &symbol).await?;
    println!("count: {}", orders.len());
    let venue_order_id = orders.first().map(|o| o.order_id);
    for o in &orders {
        println!("  {:?}", o);
    }

    // ── Cancel order ──────────────────────────────────────────────────────
    println!("\n=== cancel_order ===");
    // Prefer venue_order_id only — passing both may cause rejection
    let cancel_args = if venue_order_id.is_some() {
        CancelOrderArgs { order_id: venue_order_id.map(OrderId), client_order_id: None }
    } else {
        CancelOrderArgs { order_id: None, client_order_id: Some(client_order_id) }
    };
    let cancel_action = UserAction::<Address>::CancelOrders {
        market_id: MarketId(prec.market_id),
        orders: vec![cancel_args],
        sub_account_index: None,
    };
    let cancel_tx = sign_user_action(cancel_action, &creds, &chain_data, None)?;
    let cancel_resp = client.submit_tx(cancel_tx).await?;
    println!("cancel tx_id: {}", cancel_resp.id);

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // ── Verify canceled ───────────────────────────────────────────────────
    let orders_after = client.open_orders(&main_addr, &symbol).await?;
    println!("\nopen_orders after cancel: {}", orders_after.len());
    if orders_after.is_empty() {
        println!("Order successfully placed and canceled");
    } else {
        println!("{} order(s) still open (may include orders from previous runs)", orders_after.len());
        // Cancel all remaining BTC-USD orders
        println!("Sending CancelMarketOrders to clean up...");
        let cleanup_action = UserAction::<Address>::CancelMarketOrders {
            market_id: MarketId(prec.market_id),
            sub_account_index: None,
        };
        let cleanup_tx = sign_user_action(cleanup_action, &creds, &chain_data, None)?;
        let cleanup_resp = client.submit_tx(cleanup_tx).await?;
        println!("cleanup tx_id: {}", cleanup_resp.id);
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let final_orders = client.open_orders(&main_addr, &symbol).await?;
        println!("open_orders after cleanup: {}", final_orders.len());
    }

    Ok(())
}
