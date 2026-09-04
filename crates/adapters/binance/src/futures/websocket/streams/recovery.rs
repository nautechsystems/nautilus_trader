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

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use dashmap::DashMap;
#[cfg(test)]
use nautilus_core::string::secret::REDACTED;
use nautilus_core::string::secret::SecretString;
use nautilus_live::{
    SocketControlFactory,
    task::{TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::websocket::TransportBackend;
use parking_lot::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use super::{
    client::BinanceFuturesWebSocketClient,
    dispatch::{
        DispatchCtx, make_venue_position_id, run_user_stream_dispatch, with_venue_position_id,
    },
    messages::BinanceFuturesWsStreamsMessage,
};
use crate::{
    common::{
        consts::BINANCE_WS_HEARTBEAT_SECS,
        enums::{BinanceEnvironment, BinanceProductType},
        symbol::format_instrument_id,
        urls::get_futures_user_stream_url,
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
    pub api_key: SecretString,
    pub api_secret: SecretString,
    pub private_base_url: String,
    pub transport_backend: TransportBackend,
    pub proxy_url: Option<SecretString>,
    pub socket_factory: SocketControlFactory,
}

/// Context captured by the recovery driver task. All fields are cheaply
/// cloneable (Arc-backed) so the driver can act without holding `&self` on
/// the execution client.
pub(crate) struct RecoveryCtx {
    pub http_client: BinanceFuturesHttpClient,
    pub listen_key: Arc<RwLock<Option<SecretString>>>,
    pub recovery_listen_key: Arc<RwLock<Option<SecretString>>>,
    pub ws_client: Arc<Mutex<Option<BinanceFuturesWebSocketClient>>>,
    pub ws_task: Arc<tokio::sync::Mutex<TaskSlot<()>>>,
    pub recovery_lock: Arc<tokio::sync::Mutex<()>>,
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
    let private_url =
        get_futures_user_stream_url(params.product_type, &params.private_base_url, listen_key);

    let mut ws_client = BinanceFuturesWebSocketClient::new(
        params.product_type,
        params.environment,
        Some(params.api_key.expose_secret().to_owned()),
        Some(params.api_secret.expose_secret().to_owned()),
        Some(private_url),
        Some(BINANCE_WS_HEARTBEAT_SECS),
        params.transport_backend,
    )
    .context("failed to construct Binance Futures private WebSocket client")?
    .with_proxy(
        params
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned()),
    )
    .with_socket_control(
        params.socket_factory.clone(),
        "binance-futures-user-streams",
    );

    log::debug!("Connecting to Binance Futures user data stream...");
    ws_client.connect().await.map_err(|_| {
        log::error!("Binance Futures private WebSocket connection failed");
        anyhow::anyhow!("failed to connect Binance Futures private WebSocket")
    })?;
    log::debug!("Connected to Binance Futures user data stream");

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

    close_recovery_listen_key(ctx).await?;

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
    let new_listen_key = response.into_listen_key();
    *ctx.recovery_listen_key.write() = Some(new_listen_key.clone());

    emit_open_order_reports(ctx).await?;

    let new_ws =
        build_and_connect_user_stream(&ctx.ws_build_params, new_listen_key.expose_secret()).await?;
    let new_stream = new_ws.stream();

    let old_ws = ctx.ws_client.lock().take();
    if let Some(mut old_ws) = old_ws {
        old_ws
            .close()
            .await
            .context("failed to close old user data WebSocket")?;
    }

    // Drain queued events from the old stream while the replacement buffers new events.
    let mut task_slot = ctx.ws_task.lock().await;
    if let Some(outcome) = finish_task(
        &mut task_slot,
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .await
    {
        match outcome {
            TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
            TaskJoinOutcome::Failed(error) => {
                anyhow::bail!("old user stream dispatch task failed: {error}");
            }
            TaskJoinOutcome::Incomplete => {
                anyhow::bail!("old user stream dispatch task did not stop after abort");
            }
        }
    }

    let mut new_task = TaskSlot::new();
    new_task
        .spawn(run_user_stream_dispatch(
            new_stream,
            ctx.dispatch_ctx.clone(),
            ctx.recovery_tx.clone(),
            dispatch_fn,
        ))
        .map_err(|e| anyhow::anyhow!("failed to start recovered user stream dispatch task: {e}"))?;

    *ctx.ws_client.lock() = Some(new_ws);
    *task_slot = new_task;
    *ctx.listen_key.write() = Some(new_listen_key);
    *ctx.recovery_listen_key.write() = None;

    Ok(())
}

async fn close_recovery_listen_key(ctx: &RecoveryCtx) -> anyhow::Result<()> {
    let key = ctx.recovery_listen_key.read().clone();
    let Some(key) = key else {
        return Ok(());
    };

    ctx.http_client
        .close_listen_key(key.expose_secret())
        .await
        .context("failed to close uncommitted recovery listen key")?;
    let mut pending = ctx.recovery_listen_key.write();
    if pending.as_ref().map(SecretString::expose_secret) == Some(key.expose_secret()) {
        *pending = None;
    }
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
/// Returns an error if both REST queries fail or a returned open order cannot
/// be resolved, so `recover_with_retry` schedules another attempt instead of
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
                let symbol_ustr = order.symbol;
                let (instrument_id, price_precision, size_precision) =
                    resolve_precision(&instruments, &symbol_ustr, product_type).with_context(
                        || {
                            format!(
                                "failed to resolve open order {} during recovery reconcile",
                                order.symbol
                            )
                        },
                    )?;
                let venue_position_id = make_venue_position_id(
                    ctx.dispatch_ctx.use_position_ids,
                    instrument_id,
                    order.position_side,
                )?;

                match order.to_order_status_report(
                    ctx.dispatch_ctx.account_id,
                    instrument_id,
                    price_precision,
                    size_precision,
                    ctx.dispatch_ctx.treat_expired_as_canceled,
                    ts_init,
                ) {
                    Ok(report) => {
                        ctx.dispatch_ctx
                            .emitter
                            .send_order_status_report(with_venue_position_id(
                                report,
                                venue_position_id,
                            ));
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
                let symbol_ustr = algo_order.symbol;
                let (instrument_id, price_precision, size_precision) =
                    resolve_precision(&instruments, &symbol_ustr, product_type).with_context(
                        || {
                            format!(
                                "failed to resolve open algo order {} during recovery reconcile",
                                algo_order.symbol
                            )
                        },
                    )?;
                let venue_position_id = make_venue_position_id(
                    ctx.dispatch_ctx.use_position_ids,
                    instrument_id,
                    algo_order.position_side,
                )?;

                match algo_order.to_order_status_report(
                    ctx.dispatch_ctx.account_id,
                    instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                ) {
                    Ok(report) => {
                        ctx.dispatch_ctx
                            .emitter
                            .send_order_status_report(with_venue_position_id(
                                report,
                                venue_position_id,
                            ));
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
    instruments: &DashMap<ustr::Ustr, crate::futures::http::client::BinanceFuturesInstrument>,
    symbol_ustr: &ustr::Ustr,
    product_type: BinanceProductType,
) -> anyhow::Result<(InstrumentId, u8, u8)> {
    let instrument_id = format_instrument_id(symbol_ustr, product_type);
    let instrument = instruments
        .get(symbol_ustr)
        .map(|instrument| instrument.value().clone())
        .with_context(|| format!("missing instrument metadata for {instrument_id}"))?;
    anyhow::ensure!(
        instrument.id() == instrument_id,
        "instrument metadata ID {} does not match {instrument_id}",
        instrument.id()
    );
    let (price_precision, size_precision) = instrument.precisions()?;

    Ok((instrument_id, price_precision, size_precision))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use axum::{
        Json, Router,
        extract::State,
        http::{StatusCode, Uri},
        response::{IntoResponse, Response},
        routing::get,
    };
    use nautilus_common::{
        cache::fifo::FifoCache,
        messages::{ExecutionEvent, ExecutionReport},
    };
    use nautilus_core::{AtomicSet, time::get_atomic_clock_realtime};
    use nautilus_live::ExecutionEventEmitter;
    use nautilus_model::{
        enums::AccountType,
        identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TraderId},
        types::Currency,
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use serde_json::json;
    use tokio::task::JoinHandle;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{
            consts::{BINANCE_CLIENT_ID, BINANCE_VENUE},
            dispatch::WsDispatchState,
            enums::{BinanceContractStatus, BinanceTradingStatus},
        },
        futures::http::{
            client::BinanceFuturesInstrument,
            models::{BinanceFuturesCoinSymbol, BinanceFuturesUsdSymbol},
        },
    };

    #[rstest]
    fn test_ws_build_params_protects_credentials() {
        let params = WsBuildParams {
            product_type: BinanceProductType::UsdM,
            environment: BinanceEnvironment::Live,
            api_key: SecretString::from("test-api-key"),
            api_secret: SecretString::from("test-api-secret"),
            private_base_url: "wss://fstream.binance.com".to_string(),
            transport_backend: TransportBackend::default(),
            proxy_url: Some(SecretString::from("http://user:password@proxy:8080")),
            socket_factory: SocketControlFactory::new(*BINANCE_CLIENT_ID, Some(*BINANCE_VENUE)),
        };

        assert_eq!(format!("{:?}", params.api_key), REDACTED);
        assert_eq!(format!("{:?}", params.api_secret), REDACTED);
        assert_eq!(
            format!("{:?}", params.proxy_url),
            format!("Some({REDACTED})")
        );
        assert_eq!(params.private_base_url, "wss://fstream.binance.com");
    }

    #[rstest]
    fn test_listen_key_redacts_debug() {
        let key = SecretString::from("listen-key-secret".to_string());

        assert_eq!(key.expose_secret(), "listen-key-secret");
        assert_eq!(format!("{key:?}"), REDACTED);
    }

    #[derive(Clone, Copy)]
    enum RecoveryServerMode {
        MissingRegular,
        MissingAlgo,
        FailedQueries,
    }

    async fn recovery_response(State(mode): State<RecoveryServerMode>, uri: Uri) -> Response {
        if matches!(mode, RecoveryServerMode::FailedQueries) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": -1, "msg": "test query failure"})),
            )
                .into_response();
        }

        let body = if uri.path().ends_with("/openOrders") {
            match mode {
                RecoveryServerMode::MissingRegular => json!([{
                    "symbol": "NEWUSDT",
                    "orderId": 1,
                    "clientOrderId": "external-regular",
                    "origQty": "1",
                    "executedQty": "0",
                    "price": "1",
                    "status": "NEW",
                    "timeInForce": "GTC",
                    "type": "LIMIT",
                    "side": "BUY",
                    "positionSide": "LONG"
                }]),
                RecoveryServerMode::MissingAlgo => json!([]),
                RecoveryServerMode::FailedQueries => unreachable!(),
            }
        } else {
            match mode {
                RecoveryServerMode::MissingRegular => json!([]),
                RecoveryServerMode::MissingAlgo => json!([{
                    "algoId": 2,
                    "clientAlgoId": "external-algo",
                    "algoType": "CONDITIONAL",
                    "orderType": "STOP_MARKET",
                    "symbol": "ALGOUSDT",
                    "side": "SELL",
                    "positionSide": "SHORT",
                    "quantity": "1",
                    "algoStatus": "NEW",
                    "triggerPrice": "1"
                }]),
                RecoveryServerMode::FailedQueries => unreachable!(),
            }
        };

        (StatusCode::OK, Json(body)).into_response()
    }

    async fn start_recovery_server(mode: RecoveryServerMode) -> (String, JoinHandle<()>) {
        let app = Router::new()
            .fallback(get(recovery_response))
            .with_state(mode);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        (format!("http://{address}"), task)
    }

    fn recovery_context(
        base_url: String,
    ) -> (
        RecoveryCtx,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("BINANCE-001");
        let http_client = BinanceFuturesHttpClient::new(
            BinanceProductType::UsdM,
            BinanceEnvironment::Live,
            clock,
            Some("test-api-key".to_string()),
            Some("test-api-secret".to_string()),
            Some(base_url),
            None,
            Some(1),
            None,
            false,
        )
        .unwrap();
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(event_tx);
        let (recovery_tx, _recovery_rx) = tokio::sync::mpsc::unbounded_channel();
        let dispatch_ctx = Arc::new(DispatchCtx {
            emitter,
            http_client: http_client.clone(),
            account_id,
            product_type: BinanceProductType::UsdM,
            clock,
            dispatch_state: Arc::new(WsDispatchState::default()),
            triggered_algo_ids: Arc::new(AtomicSet::<ClientOrderId>::new()),
            algo_client_ids: Arc::new(AtomicSet::<ClientOrderId>::new()),
            use_position_ids: true,
            default_taker_fee: Decimal::new(4, 4),
            bnfcr_currency: Currency::USDT(),
            treat_expired_as_canceled: false,
            use_trade_lite: false,
            seen_trade_ids: Arc::new(Mutex::new(FifoCache::new())),
            cancellation_token: CancellationToken::new(),
        });
        let context = RecoveryCtx {
            http_client,
            listen_key: Arc::new(RwLock::new(None)),
            recovery_listen_key: Arc::new(RwLock::new(None)),
            ws_client: Arc::new(Mutex::new(None)),
            ws_task: Arc::new(tokio::sync::Mutex::new(TaskSlot::new())),
            recovery_lock: Arc::new(tokio::sync::Mutex::new(())),
            ws_build_params: WsBuildParams {
                product_type: BinanceProductType::UsdM,
                environment: BinanceEnvironment::Live,
                api_key: SecretString::from("test-api-key"),
                api_secret: SecretString::from("test-api-secret"),
                private_base_url: "ws://127.0.0.1:1".to_string(),
                transport_backend: TransportBackend::default(),
                proxy_url: None,
                socket_factory: SocketControlFactory::new(*BINANCE_CLIENT_ID, Some(*BINANCE_VENUE)),
            },
            dispatch_ctx,
            recovery_tx,
        };

        (context, event_rx)
    }

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
    #[case(
        RecoveryServerMode::MissingRegular,
        "failed to resolve open order NEWUSDT during recovery reconcile",
        "missing instrument metadata for NEWUSDT-PERP.BINANCE"
    )]
    #[case(
        RecoveryServerMode::MissingAlgo,
        "failed to resolve open algo order ALGOUSDT during recovery reconcile",
        "missing instrument metadata for ALGOUSDT-PERP.BINANCE"
    )]
    #[tokio::test]
    async fn test_recovery_reconcile_fails_orders_with_unresolved_metadata(
        #[case] mode: RecoveryServerMode,
        #[case] expected_error: &str,
        #[case] expected_root_cause: &str,
    ) {
        let (base_url, server_task) = start_recovery_server(mode).await;
        let (context, mut event_rx) = recovery_context(base_url);

        let error = emit_open_order_reports(&context).await.unwrap_err();
        let event = event_rx.try_recv();
        server_task.abort();

        assert_eq!(error.to_string(), expected_error);
        assert_eq!(error.root_cause().to_string(), expected_root_cause);
        assert!(event.is_err());
    }

    #[rstest]
    #[case(
        RecoveryServerMode::MissingRegular,
        "NEWUSDT",
        "NEWUSDT-PERP.BINANCE-LONG"
    )]
    #[case(
        RecoveryServerMode::MissingAlgo,
        "ALGOUSDT",
        "ALGOUSDT-PERP.BINANCE-SHORT"
    )]
    #[tokio::test]
    async fn test_recovery_reconcile_includes_position_id(
        #[case] mode: RecoveryServerMode,
        #[case] symbol: &str,
        #[case] expected_position_id: &str,
    ) {
        let (base_url, server_task) = start_recovery_server(mode).await;
        let (context, mut event_rx) = recovery_context(base_url);
        context
            .http_client
            .instruments_cache()
            .insert(Ustr::from(symbol), usdm_instrument(symbol, 3));

        emit_open_order_reports(&context).await.unwrap();
        let event = event_rx.try_recv().unwrap();
        server_task.abort();

        let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
            panic!("Expected recovery order status report, was {event:?}");
        };
        assert_eq!(
            report.venue_position_id,
            Some(PositionId::from(expected_position_id)),
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_recovery_reconcile_fails_when_both_queries_fail() {
        let (base_url, server_task) =
            start_recovery_server(RecoveryServerMode::FailedQueries).await;
        let (context, _event_rx) = recovery_context(base_url);

        let error = emit_open_order_reports(&context).await.unwrap_err();
        server_task.abort();

        assert_eq!(
            error.to_string(),
            "recovery reconcile failed: both REST queries returned errors"
        );
    }

    #[rstest]
    fn test_resolve_precision_returns_cached_instrument() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSDT");
        instruments.insert(symbol, usdm_instrument("BTCUSDT", 3));

        let (id, price_precision, size_precision) =
            resolve_precision(&instruments, &symbol, BinanceProductType::UsdM).unwrap();

        assert_eq!(id, InstrumentId::from_str("BTCUSDT-PERP.BINANCE").unwrap());
        assert_eq!(price_precision, 2);
        assert_eq!(size_precision, 3);
    }

    #[rstest]
    fn test_resolve_precision_rejects_missing_usdm_instrument() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSDT");

        let error = resolve_precision(&instruments, &symbol, BinanceProductType::UsdM).unwrap_err();

        assert_eq!(
            error.to_string(),
            "missing instrument metadata for BTCUSDT-PERP.BINANCE"
        );
    }

    #[rstest]
    fn test_resolve_precision_rejects_missing_coinm_instrument() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSD_PERP");

        let error =
            resolve_precision(&instruments, &symbol, BinanceProductType::CoinM).unwrap_err();

        assert_eq!(
            error.to_string(),
            "missing instrument metadata for BTCUSD_PERP.BINANCE"
        );
    }

    #[rstest]
    fn test_resolve_precision_rejects_product_mismatch() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSDT");
        instruments.insert(symbol, coinm_instrument("BTCUSDT", 0));

        let error = resolve_precision(&instruments, &symbol, BinanceProductType::UsdM).unwrap_err();

        assert_eq!(
            error.to_string(),
            "instrument metadata ID BTCUSDT.BINANCE does not match BTCUSDT-PERP.BINANCE"
        );
    }

    #[rstest]
    fn test_resolve_precision_uses_cached_coinm_precision() {
        let instruments: DashMap<Ustr, BinanceFuturesInstrument> = DashMap::new();
        let symbol = Ustr::from("BTCUSD_PERP");
        instruments.insert(symbol, coinm_instrument("BTCUSD_PERP", 0));

        let (id, price_precision, size_precision) =
            resolve_precision(&instruments, &symbol, BinanceProductType::CoinM).unwrap();

        assert_eq!(id, InstrumentId::from_str("BTCUSD_PERP.BINANCE").unwrap());
        assert_eq!(price_precision, 1);
        assert_eq!(size_precision, 0);
    }
}
