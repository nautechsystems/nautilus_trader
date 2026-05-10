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

//! Listen key recovery for the Binance Futures user data stream.
//!
//! Handles listen key rotation, WebSocket reconnection, and open-order
//! reconciliation after a `listenKeyExpired` event or keepalive failure. A
//! single long-lived driver task consumes trigger signals from a channel and
//! serializes concurrent triggers through an internal lock.

use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use anyhow::Context;
use nautilus_core::MUTEX_POISONED;
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::websocket::TransportBackend;
use tokio::{sync::Mutex as TokioMutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{
    client::BinanceFuturesWebSocketClient,
    dispatch::{DispatchCtx, spawn_user_stream_dispatch},
    messages::BinanceFuturesWsStreamsMessage,
};
use crate::{
    common::{
        enums::{BinanceEnvironment, BinanceProductType},
        symbol::format_instrument_id,
    },
    futures::http::{client::BinanceFuturesHttpClient, query::BinanceOpenOrdersParamsBuilder},
};

/// Initial backoff between recovery retries.
const RECOVERY_RETRY_INITIAL_MS: u64 = 1_000;

/// Upper bound on the recovery retry backoff.
const RECOVERY_RETRY_MAX_MS: u64 = 30_000;

/// Parameters needed to construct a fresh user data WebSocket client.
#[derive(Clone)]
pub(crate) struct WsBuildParams {
    pub product_type: BinanceProductType,
    pub environment: BinanceEnvironment,
    pub api_key: String,
    pub api_secret: String,
    pub private_base_url: String,
    pub transport_backend: TransportBackend,
}

/// Context captured by the recovery driver task. All fields are cheaply
/// cloneable (Arc-backed) so the driver can act without holding `&self` on
/// the execution client.
pub(crate) struct RecoveryCtx {
    pub http_client: BinanceFuturesHttpClient,
    pub listen_key: Arc<RwLock<Option<String>>>,
    pub ws_client: Arc<TokioMutex<Option<BinanceFuturesWebSocketClient>>>,
    pub ws_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub recovery_lock: Arc<TokioMutex<()>>,
    pub ws_build_params: WsBuildParams,
    pub dispatch_ctx: Arc<DispatchCtx>,
    pub recovery_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

/// Constructs and connects a private user data WebSocket client bound to the
/// supplied `listen_key`.
pub(crate) async fn build_and_connect_user_stream(
    params: &WsBuildParams,
    listen_key: &str,
) -> anyhow::Result<BinanceFuturesWebSocketClient> {
    let private_url = format!("{}?listenKey={}", params.private_base_url, listen_key);

    let mut ws_client = BinanceFuturesWebSocketClient::new(
        params.product_type,
        params.environment,
        Some(params.api_key.clone()),
        Some(params.api_secret.clone()),
        Some(private_url),
        Some(20),
        params.transport_backend,
    )
    .context("failed to construct Binance Futures private WebSocket client")?;

    log::info!("Connecting to Binance Futures user data stream...");
    ws_client.connect().await.map_err(|e| {
        log::error!("Binance Futures private WebSocket connection failed: {e:?}");
        anyhow::anyhow!("failed to connect Binance Futures private WebSocket: {e}")
    })?;
    log::info!("Connected to Binance Futures user data stream");

    Ok(ws_client)
}

/// Long-lived task that consumes recovery signals and runs
/// [`recover_user_data_stream`] with retry-on-failure semantics.
pub(crate) async fn run_recovery_driver<F>(
    ctx: RecoveryCtx,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<()>,
    cancel: CancellationToken,
    dispatch_fn: F,
) where
    F: Fn(BinanceFuturesWsStreamsMessage, &DispatchCtx, &tokio::sync::mpsc::UnboundedSender<()>)
        + Send
        + Sync
        + Clone
        + 'static,
{
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Some(()) => {
                        // Drain additional pending triggers so we only run once per burst
                        while rx.try_recv().is_ok() {}
                        recover_with_retry(&ctx, dispatch_fn.clone(), &cancel).await;
                    }
                    None => {
                        log::debug!("Recovery driver channel closed");
                        break;
                    }
                }
            }
            () = cancel.cancelled() => {
                log::debug!("Recovery driver task cancelled");
                break;
            }
        }
    }
}

/// Runs recovery with exponential backoff. Retries indefinitely until success
/// or cancellation, because the alternative (giving up) leaves the user data
/// stream blind until the next keepalive tick up to 30 minutes later, which
/// is worse than a persistent error log on a permanent failure.
async fn recover_with_retry<F>(ctx: &RecoveryCtx, dispatch_fn: F, cancel: &CancellationToken)
where
    F: Fn(BinanceFuturesWsStreamsMessage, &DispatchCtx, &tokio::sync::mpsc::UnboundedSender<()>)
        + Send
        + Sync
        + Clone
        + 'static,
{
    let mut delay_ms = RECOVERY_RETRY_INITIAL_MS;
    let mut attempt = 0u32;

    loop {
        attempt += 1;

        match recover_user_data_stream(ctx, dispatch_fn.clone()).await {
            Ok(()) => return,
            Err(e) => {
                log::error!("Listen key recovery attempt {attempt} failed: {e:#}");
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(delay_ms)) => {}
                    () = cancel.cancelled() => return,
                }
                delay_ms = (delay_ms.saturating_mul(2)).min(RECOVERY_RETRY_MAX_MS);
            }
        }
    }
}

async fn recover_user_data_stream<F>(ctx: &RecoveryCtx, dispatch_fn: F) -> anyhow::Result<()>
where
    F: Fn(BinanceFuturesWsStreamsMessage, &DispatchCtx, &tokio::sync::mpsc::UnboundedSender<()>)
        + Send
        + Sync
        + 'static,
{
    let _guard = ctx.recovery_lock.lock().await;

    log::warn!("Rotating Binance Futures listen key after expiry or keepalive failure");

    // Create the new listenKey and emit the REST snapshot first, using only
    // the HTTP client. The old stream is still live during this window, so
    // its events continue to flow through the old dispatcher. If the
    // snapshot fails we bail out before touching ws_client / ws_task, so
    // recover_with_retry can retry cleanly without orphaning a connected
    // socket that has no dispatcher attached.
    let response = ctx
        .http_client
        .create_listen_key()
        .await
        .context("failed to create listen key during recovery")?;
    let new_listen_key = response.listen_key;

    emit_open_order_reports(ctx).await?;

    // Snapshot succeeded; commit the rotation. Build and connect the new
    // stream, swap it in, close the old one, drain its queued events, then
    // spawn the new dispatcher.
    let new_ws = build_and_connect_user_stream(&ctx.ws_build_params, &new_listen_key).await?;
    let new_stream = new_ws.stream();

    let old_ws = {
        let mut guard = ctx.ws_client.lock().await;
        guard.replace(new_ws)
    };

    {
        let mut key_guard = ctx.listen_key.write().expect(MUTEX_POISONED);
        *key_guard = Some(new_listen_key);
    }

    if let Some(mut old) = old_ws
        && let Err(e) = old.close().await
    {
        log::warn!("Failed to close old user data WebSocket cleanly: {e}");
    }

    // Await the old dispatch task so events already queued on the old
    // stream (fills, cancels) drain through the dispatcher before the new
    // dispatcher starts. Scope the std MutexGuard so it does not span the
    // await.
    let old_task = ctx.ws_task.lock().expect(MUTEX_POISONED).take();
    if let Some(task) = old_task {
        let _ = task.await;
    }

    let new_task = spawn_user_stream_dispatch(
        new_stream,
        ctx.dispatch_ctx.clone(),
        ctx.recovery_tx.clone(),
        dispatch_fn,
    );
    *ctx.ws_task.lock().expect(MUTEX_POISONED) = Some(new_task);

    Ok(())
}

/// Emits `OrderStatusReport`s for every open order and open algo order on the
/// venue so the engine can repair any order state missed during the rotation
/// window. Uses the Arc-backed HTTP instruments cache for precision lookups,
/// which does not require `&self` access.
///
/// This does not cover orders that filled or canceled during the rotation
/// gap, because `query_open_orders` returns open orders only. The engine's
/// periodic open-order reconciliation is expected to repair that state.
///
/// # Errors
///
/// Returns an error if both the open-order and open-algo-order REST queries
/// fail, so `recover_with_retry` schedules another attempt instead of
/// silently leaving the gap unrepaired.
async fn emit_open_order_reports(ctx: &RecoveryCtx) -> anyhow::Result<()> {
    let params = BinanceOpenOrdersParamsBuilder::default()
        .build()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("failed to build open orders params for recovery reconcile")?;

    let open_orders_result = ctx.http_client.query_open_orders(&params).await;
    let algo_orders_result = ctx.http_client.query_open_algo_orders(None).await;

    let ts_init = ctx.dispatch_ctx.clock.get_time_ns();
    let instruments = ctx.http_client.instruments_cache();
    let product_type = ctx.dispatch_ctx.product_type;
    let mut emitted = 0usize;

    let open_ok = match open_orders_result {
        Ok(orders) => {
            for order in orders {
                let symbol_ustr = ustr::Ustr::from(order.symbol.as_str());
                let (instrument_id, size_precision) =
                    resolve_precision(&instruments, &symbol_ustr, product_type);

                match order.to_order_status_report(
                    ctx.dispatch_ctx.account_id,
                    instrument_id,
                    size_precision,
                    ctx.dispatch_ctx.treat_expired_as_canceled,
                    ts_init,
                ) {
                    Ok(report) => {
                        ctx.dispatch_ctx.emitter.send_order_status_report(report);
                        emitted += 1;
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to build OrderStatusReport for {} during recovery reconcile: {e}",
                            order.symbol,
                        );
                    }
                }
            }
            true
        }
        Err(e) => {
            log::warn!("Failed to query open orders for recovery reconcile: {e}");
            false
        }
    };

    let algo_ok = match algo_orders_result {
        Ok(algo_orders) => {
            for algo_order in algo_orders {
                let symbol_ustr = ustr::Ustr::from(algo_order.symbol.as_str());
                let (instrument_id, size_precision) =
                    resolve_precision(&instruments, &symbol_ustr, product_type);

                match algo_order.to_order_status_report(
                    ctx.dispatch_ctx.account_id,
                    instrument_id,
                    size_precision,
                    ts_init,
                ) {
                    Ok(report) => {
                        ctx.dispatch_ctx.emitter.send_order_status_report(report);
                        emitted += 1;
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to build OrderStatusReport for algo {} during recovery reconcile: {e}",
                            algo_order.symbol,
                        );
                    }
                }
            }
            true
        }
        Err(e) => {
            log::warn!("Failed to query open algo orders for recovery reconcile: {e}");
            false
        }
    };

    if !open_ok && !algo_ok {
        anyhow::bail!("recovery reconcile failed: both REST queries returned errors");
    }

    log::info!("Recovery reconcile emitted {emitted} OrderStatusReport(s)");
    Ok(())
}

fn resolve_precision(
    instruments: &dashmap::DashMap<
        ustr::Ustr,
        crate::futures::http::client::BinanceFuturesInstrument,
    >,
    symbol_ustr: &ustr::Ustr,
    product_type: BinanceProductType,
) -> (InstrumentId, u8) {
    if let Some(instrument) = instruments.get(symbol_ustr) {
        (instrument.id(), instrument.quantity_precision() as u8)
    } else {
        // Fallback when the instrument is not cached: derive the venue id
        // via the Futures-aware formatter (matches dispatch_ws_message) and
        // use a conservative precision so venue state still propagates.
        (format_instrument_id(symbol_ustr, product_type), 8u8)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use dashmap::DashMap;
    use nautilus_model::identifiers::InstrumentId;
    use rstest::rstest;
    use serde_json::json;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{BinanceContractStatus, BinanceTradingStatus},
        futures::http::{
            client::BinanceFuturesInstrument,
            models::{BinanceFuturesCoinSymbol, BinanceFuturesUsdSymbol},
        },
    };

    fn usdm_instrument(symbol: &str, quantity_precision: i32) -> BinanceFuturesInstrument {
        BinanceFuturesInstrument::UsdM(BinanceFuturesUsdSymbol {
            symbol: Ustr::from(symbol),
            pair: Ustr::from(symbol),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: 4_133_404_800_000,
            onboard_date: 1_569_398_400_000,
            status: BinanceTradingStatus::Trading,
            maint_margin_percent: "2.5000".to_string(),
            required_margin_percent: "5.0000".to_string(),
            base_asset: Ustr::from("BTC"),
            quote_asset: Ustr::from("USDT"),
            margin_asset: Ustr::from("USDT"),
            price_precision: 2,
            quantity_precision,
            base_asset_precision: 8,
            quote_precision: 8,
            underlying_type: None,
            underlying_sub_type: vec![],
            settle_plan: None,
            trigger_protect: None,
            liquidation_fee: None,
            market_take_bound: None,
            order_types: vec![],
            time_in_force: vec![],
            filters: vec![json!({})],
        })
    }

    fn coinm_instrument(symbol: &str, quantity_precision: i32) -> BinanceFuturesInstrument {
        BinanceFuturesInstrument::CoinM(BinanceFuturesCoinSymbol {
            symbol: Ustr::from(symbol),
            pair: Ustr::from("BTCUSD"),
            contract_type: "PERPETUAL".to_string(),
            delivery_date: 4_133_404_800_000,
            onboard_date: 1_569_398_400_000,
            contract_status: Some(BinanceContractStatus::Trading),
            contract_size: 100,
            maint_margin_percent: "2.5000".to_string(),
            required_margin_percent: "5.0000".to_string(),
            base_asset: Ustr::from("BTC"),
            quote_asset: Ustr::from("USD"),
            margin_asset: Ustr::from("BTC"),
            price_precision: 1,
            quantity_precision,
            base_asset_precision: 8,
            quote_precision: 8,
            equal_qty_precision: None,
            trigger_protect: None,
            market_take_bound: None,
            liquidation_fee: None,
            order_types: vec![],
            time_in_force: vec![],
            filters: vec![],
        })
    }

    #[rstest]
    fn test_resolve_precision_returns_cached_instrument() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSDT");
        instruments.insert(symbol, usdm_instrument("BTCUSDT", 3));

        let (id, size_precision) =
            resolve_precision(&instruments, &symbol, BinanceProductType::UsdM);

        assert_eq!(id, InstrumentId::from_str("BTCUSDT-PERP.BINANCE").unwrap());
        assert_eq!(size_precision, 3);
    }

    #[rstest]
    fn test_resolve_precision_falls_back_to_formatted_usdm_id() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSDT");

        let (id, size_precision) =
            resolve_precision(&instruments, &symbol, BinanceProductType::UsdM);

        // The P3 fix: USD-M perps must produce `-PERP` suffix, not raw symbol
        assert_eq!(id, InstrumentId::from_str("BTCUSDT-PERP.BINANCE").unwrap());
        assert_eq!(size_precision, 8);
    }

    #[rstest]
    fn test_resolve_precision_falls_back_to_formatted_coinm_id() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSD_PERP");

        let (id, size_precision) =
            resolve_precision(&instruments, &symbol, BinanceProductType::CoinM);

        assert_eq!(id, InstrumentId::from_str("BTCUSD_PERP.BINANCE").unwrap());
        assert_eq!(size_precision, 8);
    }

    #[rstest]
    fn test_resolve_precision_uses_cached_coinm_precision() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSD_PERP");
        instruments.insert(symbol, coinm_instrument("BTCUSD_PERP", 0));

        let (id, size_precision) =
            resolve_precision(&instruments, &symbol, BinanceProductType::CoinM);

        assert_eq!(id, InstrumentId::from_str("BTCUSD_PERP.BINANCE").unwrap());
        assert_eq!(size_precision, 0);
    }
}
