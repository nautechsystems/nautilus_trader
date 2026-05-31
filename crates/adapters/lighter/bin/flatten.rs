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

//! Cancels every open order on the Lighter account and closes any remaining
//! position with an IOC market order on the opposite side.
//!
//! Usage:
//! ```bash
//! cargo run --bin lighter-flatten -p nautilus-lighter
//! ```
//!
//! Environment variables: `LIGHTER_API_KEY_INDEX`, `LIGHTER_API_SECRET`,
//! `LIGHTER_ACCOUNT_INDEX` (or the testnet variants). Mainnet only for now;
//! flip to testnet by setting `LIGHTER_ENVIRONMENT=testnet`.
//!
//! Best-effort: per-market fan-out across every registered market is bounded
//! by Lighter's 60 req/min REST quota, so the run takes around three minutes
//! when many markets are scanned.

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use nautilus_common::logging::{init_logging, logger::LoggerConfig};
use nautilus_core::UUID4;
use nautilus_lighter::{
    common::{
        credential::Credential,
        enums::{LighterEnvironment, LighterOrderType, LighterTimeInForce, LighterTxType},
        symbol::MarketRegistry,
        urls::lighter_chain_id,
    },
    http::{
        client::{LighterHttpClient, LighterRawHttpClient},
        query::{LighterAccountActiveOrdersQuery, LighterOrderBookDetailsQuery},
    },
    signing::{
        auth_token::{build_auth_token_for, fresh_k},
        nonce::NonceManager,
        tx::{
            CancelOrderTxInfo, CreateOrderTxInfo, L2TxAttributes, OrderInfo, TxContext, TxInfoJson,
            sign_tx,
        },
    },
    websocket::{LighterWebSocketClient, LighterWsChannel, NautilusWsMessage},
};
use nautilus_model::{
    enums::PositionSideSpecified,
    identifiers::{AccountId, TraderId},
    instruments::Instrument,
    reports::PositionStatusReport,
};
use nautilus_network::websocket::TransportBackend;
use rust_decimal::Decimal;

const DEFAULT_TX_EXPIRY_MS: i64 = 5 * 60 * 1_000;
const POSITION_WAIT: Duration = Duration::from_secs(8);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _log_guard = init_logging(
        TraderId::from("FLATTEN-001"),
        UUID4::new(),
        LoggerConfig {
            stdout_level: log::LevelFilter::Info,
            ..Default::default()
        },
        Default::default(),
    )?;

    let environment = match std::env::var("LIGHTER_ENVIRONMENT").as_deref() {
        Ok("testnet" | "Testnet") => LighterEnvironment::Testnet,
        _ => LighterEnvironment::Mainnet,
    };
    log::info!("Environment: {environment:?}");

    let credential = Credential::resolve(None, None, None, environment)?
        .ok_or_else(|| anyhow::anyhow!("no Lighter credentials in env"))?;
    log::info!(
        "Account: account_index={}, api_key_index={}",
        credential.account_index(),
        credential.api_key_index(),
    );

    let registry = Arc::new(MarketRegistry::new());
    let raw_http = LighterRawHttpClient::new(environment, None, 30, None)?;
    let http = LighterHttpClient::from_raw_with_registry(raw_http, Arc::clone(&registry));
    let mut ws = LighterWebSocketClient::new(
        None,
        environment,
        Arc::clone(&registry),
        TransportBackend::Tungstenite,
        None,
    );

    let instruments = http
        .request_instruments()
        .await
        .context("failed to bootstrap Lighter instruments")?;
    let ws_cache: Vec<_> = instruments
        .iter()
        .filter_map(|i| {
            let id = i.id();
            registry.market_index(&id).map(|idx| (idx, i.clone()))
        })
        .collect();
    ws.cache_instruments(ws_cache);
    log::info!("Bootstrapped {} markets", registry.len());

    let nonce_mgr = NonceManager::default();
    let next_nonce = http
        .get_next_nonce(credential.account_index(), credential.api_key_index())
        .await
        .context("failed to refresh Lighter nonce")?;
    nonce_mgr.refresh(
        credential.account_index(),
        credential.api_key_index(),
        next_nonce.nonce,
    );
    log::info!("Nonce baseline: {}", next_nonce.nonce);

    ws.connect().await?;
    ws.set_execution_context(
        AccountId::new("LIGHTER-FLATTEN-001"),
        credential.account_index(),
    )
    .await?;
    let auth = build_auth_token_for(&credential)?;
    ws.subscribe_account(
        LighterWsChannel::AccountAllPositions(credential.account_index()),
        auth.clone(),
    )
    .await?;

    let mut cancelled = 0_usize;

    for market_id in registry.all_market_indices() {
        match http
            .get_account_active_orders(&LighterAccountActiveOrdersQuery {
                authorization: None,
                auth: Some(auth.clone()),
                account_index: credential.account_index(),
                market_id,
            })
            .await
        {
            Ok(resp) if !resp.orders.is_empty() => {
                let id = registry
                    .instrument_id(market_id)
                    .map_or_else(|| format!("market_index={market_id}"), |i| i.to_string());
                log::info!("Cancelling {} order(s) on {id}", resp.orders.len());
                for order in &resp.orders {
                    cancel_one_order(
                        &ws,
                        &credential,
                        &nonce_mgr,
                        environment,
                        market_id,
                        order.order_index,
                    )
                    .await?;
                    cancelled += 1;
                }
            }
            Ok(_) => {}
            Err(e) => {
                // Use top-level Display (not `{e:#}` chain) so the
                // reqwest::Error inside doesn't leak the auth-bearing URL.
                log::warn!("active orders fetch failed (market_id={market_id}): {e}");
            }
        }
    }
    log::info!("Cancel pass complete; submitted {cancelled} cancel(s)");

    log::info!("Waiting up to {POSITION_WAIT:?} for account_all_positions snapshot...");
    let positions = collect_positions(&mut ws, POSITION_WAIT).await;
    if positions.is_empty() {
        log::info!("No open positions");
        return Ok(());
    }

    log::info!("Closing {} position(s)", positions.len());
    for pos in &positions {
        log::info!(
            "position seen: instrument={} side={:?} qty={}",
            pos.instrument_id,
            pos.position_side,
            pos.quantity,
        );
        let Some(market_id) = registry.market_index(&pos.instrument_id) else {
            log::warn!("skip position {}: no market_index", pos.instrument_id);
            continue;
        };
        let Some(instrument) = instruments.iter().find(|i| i.id() == pos.instrument_id) else {
            log::warn!("skip position {}: no instrument", pos.instrument_id);
            continue;
        };

        let qty = pos.quantity.as_decimal();
        if qty.is_zero() {
            log::info!("skip position {}: qty already zero", pos.instrument_id);
            continue;
        }

        let size_precision = instrument.size_precision();
        let base_amount = match base_ticks(qty, size_precision) {
            Some(v) => v,
            None => {
                log::warn!(
                    "skip position {}: qty {} not in size ticks",
                    pos.instrument_id,
                    qty
                );
                continue;
            }
        };
        let is_ask = matches!(pos.position_side, PositionSideSpecified::Long);

        let price_decimals = instrument.price_precision();
        let crossing_price =
            match fetch_crossing_price(&http, market_id, price_decimals, is_ask).await {
                Some(p) => p,
                None => {
                    log::warn!(
                        "skip position {}: could not derive crossing price",
                        pos.instrument_id,
                    );
                    continue;
                }
            };

        log::info!(
            "{}: {} {} (base_ticks={base_amount}, crossing_price={crossing_price})",
            pos.instrument_id,
            if is_ask { "SELL" } else { "BUY " },
            pos.quantity,
        );
        close_one_position(
            &ws,
            &credential,
            &nonce_mgr,
            environment,
            market_id,
            base_amount,
            is_ask,
            crossing_price,
        )
        .await?;
    }

    log::info!("Flatten submitted; waiting briefly for venue confirmations...");
    tokio::time::sleep(Duration::from_secs(4)).await;
    ws.disconnect().await?;
    Ok(())
}

async fn fetch_crossing_price(
    http: &LighterHttpClient,
    market_id: i16,
    price_decimals: u8,
    is_ask: bool,
) -> Option<u32> {
    let query = LighterOrderBookDetailsQuery {
        market_id: Some(market_id),
        filter: None,
    };
    let details = http.get_order_book_details(&query).await.ok()?;
    let ob = details.order_book_details.first()?;
    let last = ob.last_trade_price;
    let slip = if is_ask {
        Decimal::new(99, 2) // 0.99 (SELL 1% below)
    } else {
        Decimal::new(101, 2) // 1.01 (BUY 1% above)
    };
    let crossing = last * slip;
    let scaled = crossing * Decimal::from(10_i64.pow(u32::from(price_decimals)));
    let int_str = scaled.trunc().to_string();
    int_str.parse::<u32>().ok()
}

fn base_ticks(qty: Decimal, decimals: u8) -> Option<i64> {
    let scaled = qty * Decimal::from(10_i64.pow(u32::from(decimals)));
    if !scaled.fract().is_zero() {
        return None;
    }

    scaled.trunc().to_string().parse::<i64>().ok()
}

async fn cancel_one_order(
    ws: &LighterWebSocketClient,
    credential: &Credential,
    nonce_mgr: &NonceManager,
    environment: LighterEnvironment,
    market_index: i16,
    order_index: i64,
) -> anyhow::Result<()> {
    let context = build_context(credential, nonce_mgr)?;
    let tx = CancelOrderTxInfo {
        context,
        market_index,
        index: order_index,
        skip_nonce: 0,
    };
    let signed = sign_tx(
        &tx,
        lighter_chain_id(environment),
        &credential.private_key()?,
        fresh_k(),
    );
    let tx_info = serde_json::value::RawValue::from_string(TxInfoJson::cancel_order(&tx, &signed))?;
    ws.send_tx(LighterTxType::CancelOrder as u8, tx_info)
        .await?;
    Ok(())
}

#[expect(clippy::too_many_arguments, reason = "one-off operational script")]
async fn close_one_position(
    ws: &LighterWebSocketClient,
    credential: &Credential,
    nonce_mgr: &NonceManager,
    environment: LighterEnvironment,
    market_index: i16,
    base_amount: i64,
    is_ask: bool,
    crossing_price: u32,
) -> anyhow::Result<()> {
    let context = build_context(credential, nonce_mgr)?;
    let order = OrderInfo {
        market_index,
        client_order_index: fresh_client_order_index(),
        base_amount,
        price: crossing_price,
        is_ask,
        order_type: LighterOrderType::Limit as u8,
        time_in_force: LighterTimeInForce::ImmediateOrCancel as u8,
        reduce_only: true,
        trigger_price: 0,
        order_expiry: 0,
    };
    let tx = CreateOrderTxInfo {
        context,
        order,
        attributes: L2TxAttributes::default(),
    };
    let signed = sign_tx(
        &tx,
        lighter_chain_id(environment),
        &credential.private_key()?,
        fresh_k(),
    );
    let tx_info = serde_json::value::RawValue::from_string(TxInfoJson::create_order(&tx, &signed))?;
    ws.send_tx(LighterTxType::CreateOrder as u8, tx_info)
        .await?;
    Ok(())
}

fn build_context(credential: &Credential, nonce_mgr: &NonceManager) -> anyhow::Result<TxContext> {
    let nonce = nonce_mgr
        .next_nonce(credential.account_index(), credential.api_key_index())
        .map_err(|e| anyhow::anyhow!("nonce alloc: {e}"))?;
    let now_ms = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis()) as i64;
    Ok(TxContext {
        account_index: credential.account_index(),
        api_key_index: credential.api_key_index(),
        nonce,
        expired_at: now_ms + DEFAULT_TX_EXPIRY_MS,
    })
}

fn fresh_client_order_index() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static COUNTER: AtomicI64 = AtomicI64::new(0);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let bump = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Lighter rejects client_order_index above 2^31-1 with `21727
    // invalid client order index`; mask to 31 positive bits so the
    // close-position IOC frames the venue accepts.
    i64::from((seed.wrapping_add(bump)) as u32 & 0x7FFF_FFFF)
}

async fn collect_positions(
    ws: &mut LighterWebSocketClient,
    timeout: Duration,
) -> Vec<PositionStatusReport> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut latest: Vec<PositionStatusReport> = Vec::new();

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if let Ok(Some(NautilusWsMessage::PositionSnapshot(reports))) =
            tokio::time::timeout(remaining.min(Duration::from_millis(500)), ws.next_event()).await
        {
            latest = reports;
        }
    }
    latest
}
