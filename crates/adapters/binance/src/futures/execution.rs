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

//! Live execution client implementation for the Binance Futures adapter.

use std::{
    future::Future,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    cache::fifo::FifoCache,
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GenerateOrderStatusReportsBuilder,
        GeneratePositionStatusReports, GeneratePositionStatusReportsBuilder, ModifyOrder,
        QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    AtomicSet, MUTEX_POISONED, UUID4, UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, mins_to_nanos},
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{
        AccountType, OmsType, OrderType, PositionSideSpecified, TrailingOffsetType, TriggerType,
    },
    events::{
        AccountState, OrderCancelRejected, OrderCanceled, OrderEventAny, OrderModifyRejected,
        OrderRejected, OrderUpdated,
    },
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, Venue, VenueOrderId},
    instruments::Instrument,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Quantity},
};
use rust_decimal::Decimal;
use tokio::{sync::Mutex as TokioMutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use super::{
    http::{
        BinanceFuturesHttpError,
        client::{BinanceFuturesHttpClient, BinanceFuturesInstrument, is_algo_order_type},
        models::{BatchOrderResult, BinancePositionRisk},
        query::{
            BatchCancelItem, BinanceAllOrdersParamsBuilder, BinanceOpenOrdersParamsBuilder,
            BinanceOrderQueryParamsBuilder, BinancePositionRiskParamsBuilder,
            BinanceSetLeverageParams, BinanceSetMarginTypeParams, BinanceUserTradesParamsBuilder,
        },
    },
    websocket::{
        streams::{
            client::BinanceFuturesWebSocketClient,
            dispatch::{DispatchCtx, dispatch_user_stream_message, spawn_user_stream_dispatch},
            recovery::{
                RecoveryCtx, WsBuildParams, build_and_connect_user_stream, run_recovery_driver,
            },
        },
        trading::{client::BinanceFuturesWsTradingClient, dispatch::dispatch_ws_trading_message},
    },
};
use crate::{
    common::{
        consts::{
            BINANCE_FUTURES_USD_WS_API_TESTNET_URL, BINANCE_FUTURES_USD_WS_API_URL,
            BINANCE_GTX_ORDER_REJECT_CODE, BINANCE_NAUTILUS_FUTURES_BROKER_ID, BINANCE_VENUE,
        },
        credential::resolve_credentials,
        dispatch::{OrderIdentity, PendingOperation, PendingRequest, WsDispatchState},
        encoder::encode_broker_id,
        enums::{
            BinanceEnvironment, BinancePriceMatch, BinanceProductType, BinanceSide,
            BinanceTimeInForce, BinanceWorkingType,
        },
        symbol::format_binance_symbol,
        urls::{get_usdm_ws_route_base_url, get_ws_private_base_url},
    },
    config::BinanceExecClientConfig,
    futures::{
        conversions::{
            determine_position_side, trailing_offset_to_callback_rate,
            trailing_offset_to_callback_rate_string,
        },
        http::{
            client::order_type_to_binance_futures,
            models::BinanceFuturesAccountInfo,
            query::{
                BinanceCancelOrderParamsBuilder, BinanceModifyOrderParamsBuilder,
                BinanceNewOrderParams,
            },
        },
    },
};

/// Listen key keepalive interval (30 minutes).
const LISTEN_KEY_KEEPALIVE_SECS: u64 = 30 * 60;

/// Consecutive keepalive failures before a listenKey rotation is triggered.
const MAX_KEEPALIVE_FAILURES: u32 = 1;

/// Live execution client for Binance Futures trading.
///
/// Implements the [`ExecutionClient`] trait for order management on Binance
/// USD-M and COIN-M Futures markets. Uses HTTP API for order operations and
/// WebSocket for real-time order updates via user data stream.
///
/// Uses a two-tier architecture with an execution handler that maintains
/// pending order maps for correlating WebSocket updates with order context.
#[derive(Debug)]
pub struct BinanceFuturesExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: BinanceExecClientConfig,
    emitter: ExecutionEventEmitter,
    dispatch_state: Arc<WsDispatchState>,
    product_type: BinanceProductType,
    http_client: BinanceFuturesHttpClient,
    ws_client: Arc<TokioMutex<Option<BinanceFuturesWebSocketClient>>>,
    ws_trading_client: Option<BinanceFuturesWsTradingClient>,
    ws_trading_handle: Mutex<Option<JoinHandle<()>>>,
    listen_key: Arc<RwLock<Option<String>>>,
    cancellation_token: CancellationToken,
    triggered_algo_order_ids: Arc<AtomicSet<ClientOrderId>>,
    algo_client_order_ids: Arc<AtomicSet<ClientOrderId>>,
    ws_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    keepalive_task: Mutex<Option<JoinHandle<()>>>,
    recovery_task: Mutex<Option<JoinHandle<()>>>,
    recovery_lock: Arc<TokioMutex<()>>,
    recovery_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<()>>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    is_hedge_mode: AtomicBool,
}

impl BinanceFuturesExecutionClient {
    /// Creates a new [`BinanceFuturesExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize or credentials are missing.
    pub fn new(core: ExecutionClientCore, config: BinanceExecClientConfig) -> anyhow::Result<Self> {
        let product_type = config
            .product_types
            .iter()
            .find(|pt| matches!(pt, BinanceProductType::UsdM | BinanceProductType::CoinM))
            .copied()
            .unwrap_or(BinanceProductType::UsdM);

        let (api_key, api_secret) = resolve_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.environment,
            product_type,
        )?;

        let clock = get_atomic_clock_realtime();

        let http_client = BinanceFuturesHttpClient::new(
            product_type,
            config.environment,
            clock,
            Some(api_key.clone()),
            Some(api_secret.clone()),
            config.base_url_http.clone(),
            None, // recv_window
            None, // timeout_secs
            None, // proxy_url
            config.treat_expired_as_canceled,
        )
        .context("failed to construct Binance Futures HTTP client")?;

        let ws_trading_client = if config.use_ws_trading && product_type == BinanceProductType::UsdM
        {
            let ws_trading_url =
                config
                    .base_url_ws_trading
                    .clone()
                    .or_else(|| match config.environment {
                        BinanceEnvironment::Testnet => {
                            Some(BINANCE_FUTURES_USD_WS_API_TESTNET_URL.to_string())
                        }
                        _ => Some(BINANCE_FUTURES_USD_WS_API_URL.to_string()),
                    });

            Some(BinanceFuturesWsTradingClient::new(
                ws_trading_url,
                api_key,
                api_secret,
                None, // heartbeat
                config.transport_backend,
            ))
        } else {
            None
        };

        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            dispatch_state: Arc::new(WsDispatchState::default()),
            product_type,
            http_client,
            ws_client: Arc::new(TokioMutex::new(None)),
            ws_trading_client,
            ws_trading_handle: Mutex::new(None),
            listen_key: Arc::new(RwLock::new(None)),
            cancellation_token: CancellationToken::new(),
            triggered_algo_order_ids: Arc::new(AtomicSet::new()),
            algo_client_order_ids: Arc::new(AtomicSet::new()),
            ws_task: Arc::new(Mutex::new(None)),
            keepalive_task: Mutex::new(None),
            recovery_task: Mutex::new(None),
            recovery_lock: Arc::new(TokioMutex::new(())),
            recovery_tx: Mutex::new(None),
            pending_tasks: Mutex::new(Vec::new()),
            is_hedge_mode: AtomicBool::new(false),
        })
    }

    /// Returns whether the account is in hedge mode (dual side position).
    #[must_use]
    pub fn is_hedge_mode(&self) -> bool {
        self.is_hedge_mode.load(Ordering::Acquire)
    }

    /// Returns a clone of the HTTP client's instruments cache Arc.
    #[doc(hidden)]
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<DashMap<ustr::Ustr, BinanceFuturesInstrument>> {
        self.http_client.instruments_cache()
    }

    /// Converts Binance futures account info to Nautilus account state.
    fn create_account_state(&self, account_info: &BinanceFuturesAccountInfo) -> AccountState {
        Self::create_account_state_from(
            account_info,
            self.core.account_id,
            self.core.account_type,
            self.clock,
        )
    }

    fn create_account_state_from(
        account_info: &BinanceFuturesAccountInfo,
        account_id: AccountId,
        account_type: AccountType,
        clock: &'static AtomicTime,
    ) -> AccountState {
        let ts_now = clock.get_time_ns();

        let balances: Vec<AccountBalance> = account_info
            .assets
            .iter()
            .filter_map(|b| {
                if b.wallet_balance.is_zero() {
                    return None;
                }

                let currency = Currency::from(&b.asset);
                AccountBalance::from_total_and_free(b.wallet_balance, b.available_balance, currency)
                    .ok()
            })
            .collect();

        // Emit account-wide (cross-margin) margin balances per collateral asset.
        // Binance reports per-asset `initialMargin` / `maintMargin` which covers both
        // USDT-M (typically USDT, or USDT+BNB under multi-assets mode) and COIN-M
        // (one entry per base coin, e.g. BTC / ETH).
        let mut margins: Vec<MarginBalance> = Vec::new();

        for asset in &account_info.assets {
            let initial_dec = asset.initial_margin.unwrap_or_default();
            let maint_dec = asset.maint_margin.unwrap_or_default();

            if initial_dec.is_zero() && maint_dec.is_zero() {
                continue;
            }

            let currency = Currency::from(&asset.asset);
            let initial = Money::from_decimal(initial_dec, currency)
                .unwrap_or_else(|_| Money::zero(currency));
            let maintenance =
                Money::from_decimal(maint_dec, currency).unwrap_or_else(|_| Money::zero(currency));
            margins.push(MarginBalance::new(initial, maintenance, None));
        }

        AccountState::new(
            account_id,
            account_type,
            balances,
            margins,
            true, // reported
            UUID4::new(),
            ts_now,
            ts_now,
            None, // base currency
        )
    }

    async fn refresh_account_state(&self) -> anyhow::Result<AccountState> {
        let account_info = match self.http_client.query_account().await {
            Ok(info) => info,
            Err(e) => {
                log::error!("Binance Futures account state request failed: {e}");
                anyhow::bail!("Binance Futures account state request failed: {e}");
            }
        };

        Ok(self.create_account_state(&account_info))
    }

    fn update_account_state(&self) {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let account_type = self.core.account_type;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("query_account", async move {
            let account_info = http_client
                .query_account()
                .await
                .context("Binance Futures account state request failed")?;
            let account_state =
                Self::create_account_state_from(&account_info, account_id, account_type, clock);
            let ts_now = clock.get_time_ns();
            emitter.emit_account_state(
                account_state.balances.clone(),
                account_state.margins.clone(),
                account_state.is_reported,
                ts_now,
            );
            Ok(())
        });
    }

    async fn init_hedge_mode(&self) -> anyhow::Result<bool> {
        let response = self.http_client.query_hedge_mode().await?;
        Ok(response.dual_side_position)
    }

    /// Returns whether the WS trading client is connected and active.
    fn ws_trading_active(&self) -> bool {
        self.ws_trading_client
            .as_ref()
            .is_some_and(|c| c.is_active())
    }

    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

        let emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let reduce_only = order.is_reduce_only();
        let post_only = order.is_post_only();
        let activation_price = order.activation_price();
        let trailing_offset = order.trailing_offset();
        let trigger_type = order.trigger_type();
        let position_side = determine_position_side(self.is_hedge_mode(), order_side, reduce_only);

        // Register identity for tracked/external dispatch routing
        self.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side,
                order_type,
                price,
            },
        );

        let use_algo_api = is_algo_order_type(order_type);

        let close_position = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_bool("close_position"))
            .unwrap_or(false);

        let price_match = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_str("price_match"))
            .map(BinancePriceMatch::from_param)
            .transpose()?;

        let callback_rate = trailing_offset
            .map(trailing_offset_to_callback_rate_string)
            .transpose()?;

        let working_type = match trigger_type {
            Some(TriggerType::MarkPrice) => Some(BinanceWorkingType::MarkPrice),
            Some(TriggerType::LastPrice | TriggerType::Default) => {
                Some(BinanceWorkingType::ContractPrice)
            }
            _ => None,
        };

        // Non-algo orders can route through WS trading API when active
        if self.ws_trading_active() && !use_algo_api {
            let ws_client = self.ws_trading_client.as_ref().unwrap().clone();
            let dispatch_state = self.dispatch_state.clone();
            let ts_init = clock.get_time_ns();

            let symbol = format_binance_symbol(&instrument_id);
            let binance_side = BinanceSide::try_from(order_side)?;
            let binance_order_type = order_type_to_binance_futures(order_type)?;
            let binance_tif = if post_only {
                BinanceTimeInForce::Gtx
            } else {
                BinanceTimeInForce::try_from(time_in_force)?
            };

            let requires_time_in_force = matches!(
                order_type,
                OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
            );

            let client_id_str =
                encode_broker_id(&client_order_id, BINANCE_NAUTILUS_FUTURES_BROKER_ID);

            let params = BinanceNewOrderParams {
                symbol,
                side: binance_side,
                order_type: binance_order_type,
                time_in_force: if requires_time_in_force {
                    Some(binance_tif)
                } else {
                    None
                },
                quantity: Some(quantity.to_string()),
                price: if price_match.is_some() {
                    None
                } else {
                    price.map(|p| p.to_string())
                },
                new_client_order_id: Some(client_id_str),
                stop_price: trigger_price.map(|p| p.to_string()),
                reduce_only: if reduce_only { Some(true) } else { None },
                position_side,
                close_position: None,
                activation_price: activation_price.map(|p| p.to_string()),
                callback_rate,
                working_type,
                price_protect: None,
                new_order_resp_type: None,
                good_till_date: None,
                recv_window: None,
                price_match,
                self_trade_prevention_mode: None,
            };

            // Pre-register before sending to avoid response racing the insert
            let request_id = ws_client.next_request_id();
            dispatch_state.pending_requests.insert(
                request_id.clone(),
                PendingRequest {
                    client_order_id,
                    venue_order_id: None,
                    operation: PendingOperation::Place,
                },
            );

            self.spawn_task("submit_order_ws", async move {
                if let Err(e) = ws_client
                    .place_order_with_id(request_id.clone(), params)
                    .await
                {
                    dispatch_state.pending_requests.remove(&request_id);
                    let rejected = OrderRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        account_id,
                        format!("ws-submit-order-error: {e}").into(),
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                        false,
                    );
                    emitter.send_order_event(OrderEventAny::Rejected(rejected));
                    anyhow::bail!("WS submit order failed: {e}");
                }
                Ok(())
            });

            return Ok(());
        }

        let http_client = self.http_client.clone();

        self.spawn_task("submit_order", async move {
            let result = if use_algo_api {
                http_client
                    .submit_algo_order(
                        account_id,
                        instrument_id,
                        client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        time_in_force,
                        price,
                        trigger_price,
                        reduce_only,
                        close_position,
                        position_side,
                        activation_price,
                        callback_rate,
                        working_type,
                    )
                    .await
            } else {
                http_client
                    .submit_order(
                        account_id,
                        instrument_id,
                        client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        time_in_force,
                        price,
                        trigger_price,
                        reduce_only,
                        post_only,
                        position_side,
                        price_match,
                    )
                    .await
            };

            match result {
                Ok(report) => {
                    log::debug!(
                        "Order submit accepted: client_order_id={}, venue_order_id={}",
                        client_order_id,
                        report.venue_order_id
                    );
                }
                Err(e) => {
                    // Keep order registered - if HTTP failed due to timeout but order
                    // reached Binance, WebSocket updates will still arrive. The order
                    // will be cleaned up via WebSocket rejection or reconciliation.
                    let due_post_only =
                        e.downcast_ref::<BinanceFuturesHttpError>()
                            .is_some_and(|be| {
                                matches!(
                                    be,
                                    BinanceFuturesHttpError::BinanceError { code, .. }
                                        if *code == BINANCE_GTX_ORDER_REJECT_CODE
                                )
                            });
                    let ts_now = clock.get_time_ns();
                    let rejected_event = OrderRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        account_id,
                        format!("submit-order-error: {e}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        due_post_only,
                    );

                    emitter.send_order_event(OrderEventAny::Rejected(rejected_event));

                    return Err(e);
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order_internal(&self, cmd: &CancelOrder) {
        let command = cmd.clone();

        // Non-triggered algo orders use algo cancel endpoint, triggered use regular
        let is_algo = self
            .core
            .cache()
            .order(&command.client_order_id)
            .is_some_and(|order| is_algo_order_type(order.order_type()));
        let is_triggered = self
            .triggered_algo_order_ids
            .contains(&command.client_order_id);
        let use_algo_cancel = is_algo && !is_triggered;

        let emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;
        let instrument_id = command.instrument_id;
        let venue_order_id = command.venue_order_id;
        let client_order_id = command.client_order_id;

        // Non-algo cancels can route through WS trading API when active
        if self.ws_trading_active() && !use_algo_cancel {
            let ws_client = self.ws_trading_client.as_ref().unwrap().clone();
            let dispatch_state = self.dispatch_state.clone();

            let mut cancel_builder = BinanceCancelOrderParamsBuilder::default();
            cancel_builder.symbol(instrument_id.symbol.to_string());

            if let Some(venue_id) = venue_order_id {
                match venue_id.inner().parse::<i64>() {
                    Ok(order_id) => {
                        cancel_builder.order_id(order_id);
                    }
                    Err(e) => {
                        let ts_now = clock.get_time_ns();
                        let rejected = OrderCancelRejected::new(
                            trader_id,
                            command.strategy_id,
                            instrument_id,
                            client_order_id,
                            format!("failed to parse venue_order_id: {e}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        emitter.send_order_event(OrderEventAny::CancelRejected(rejected));
                        return;
                    }
                }
            }

            cancel_builder.orig_client_order_id(encode_broker_id(
                &client_order_id,
                BINANCE_NAUTILUS_FUTURES_BROKER_ID,
            ));

            let params = cancel_builder.build().unwrap();

            // Pre-register before sending to avoid response racing the insert
            let request_id = ws_client.next_request_id();
            dispatch_state.pending_requests.insert(
                request_id.clone(),
                PendingRequest {
                    client_order_id,
                    venue_order_id,
                    operation: PendingOperation::Cancel,
                },
            );

            self.spawn_task("cancel_order_ws", async move {
                if let Err(e) = ws_client
                    .cancel_order_with_id(request_id.clone(), params)
                    .await
                {
                    dispatch_state.pending_requests.remove(&request_id);
                    let ts_now = clock.get_time_ns();
                    let rejected = OrderCancelRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        client_order_id,
                        format!("ws-cancel-order-error: {e}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );
                    emitter.send_order_event(OrderEventAny::CancelRejected(rejected));
                    anyhow::bail!("WS cancel order failed: {e}");
                }
                Ok(())
            });

            return;
        }

        let http_client = self.http_client.clone();

        self.spawn_task("cancel_order", async move {
            let result = if use_algo_cancel {
                // Try algo cancel first; if it fails, the order may have been triggered
                // before this session started, so fall back to regular cancel
                match http_client.cancel_algo_order(client_order_id).await {
                    Ok(()) => Ok(()),
                    Err(algo_err) => {
                        log::debug!("Algo cancel failed, trying regular cancel: {algo_err}");
                        http_client
                            .cancel_order(instrument_id, venue_order_id, Some(client_order_id))
                            .await
                            .map(|_| ())
                    }
                }
            } else {
                http_client
                    .cancel_order(instrument_id, venue_order_id, Some(client_order_id))
                    .await
                    .map(|_| ())
            };

            match result {
                Ok(()) => {
                    log::debug!("Cancel request accepted: client_order_id={client_order_id}");
                }
                Err(e) => {
                    let ts_now = clock.get_time_ns();
                    let rejected_event = OrderCancelRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        client_order_id,
                        format!("cancel-order-error: {e}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );

                    emitter.send_order_event(OrderEventAny::CancelRejected(rejected_event));

                    return Err(e);
                }
            }

            Ok(())
        });
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        crate::common::execution::spawn_task(&self.pending_tasks, description, fut);
    }

    fn abort_pending_tasks(&self) {
        crate::common::execution::abort_pending_tasks(&self.pending_tasks);
    }

    /// Returns the (price_precision, size_precision) for an instrument.
    fn get_instrument_precision(&self, instrument_id: InstrumentId) -> (u8, u8) {
        let cache = self.core.cache();
        cache
            .instrument(&instrument_id)
            .map_or((8, 8), |i| (i.price_precision(), i.size_precision()))
    }

    /// Creates a position status report from Binance position risk data.
    fn create_position_report(
        &self,
        position: &BinancePositionRisk,
        instrument_id: InstrumentId,
        size_precision: u8,
    ) -> anyhow::Result<PositionStatusReport> {
        let position_amount: Decimal = position
            .position_amt
            .parse()
            .context("invalid position_amt")?;

        if position_amount.is_zero() {
            anyhow::bail!("Position is flat");
        }

        let entry_price: Decimal = position
            .entry_price
            .parse()
            .context("invalid entry_price")?;

        let position_side = if position_amount > Decimal::ZERO {
            PositionSideSpecified::Long
        } else {
            PositionSideSpecified::Short
        };

        let ts_now = self.clock.get_time_ns();

        Ok(PositionStatusReport::new(
            self.core.account_id,
            instrument_id,
            position_side,
            Quantity::new(position_amount.abs().to_string().parse()?, size_precision),
            ts_now,
            ts_now,
            Some(UUID4::new()),
            None, // venue_position_id
            Some(entry_price),
        ))
    }

    async fn apply_futures_config(&self) -> anyhow::Result<()> {
        if let Some(ref leverages) = self.config.futures_leverages {
            for (symbol, leverage) in leverages {
                let params = BinanceSetLeverageParams {
                    symbol: symbol.clone(),
                    leverage: *leverage,
                    recv_window: None,
                };
                let response = self
                    .http_client
                    .set_leverage(&params)
                    .await
                    .context(format!("failed to set leverage for {symbol}"))?;
                log::info!("Set leverage {} {}X", response.symbol, response.leverage);
            }
        }

        if let Some(ref margin_types) = self.config.futures_margin_types {
            for (symbol, margin_type) in margin_types {
                let params = BinanceSetMarginTypeParams {
                    symbol: symbol.clone(),
                    margin_type: *margin_type,
                    recv_window: None,
                };

                match self.http_client.set_margin_type(&params).await {
                    Ok(_) => {
                        log::info!("Set {symbol} margin type to {margin_type:?}");
                    }
                    Err(e) => {
                        let err_str = format!("{e}");
                        if err_str.contains("-4046") {
                            log::info!("{symbol} margin type already {margin_type:?}");
                        } else {
                            return Err(e)
                                .context(format!("failed to set margin type for {symbol}"));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BinanceFuturesExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *BINANCE_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // Reinitialize cancellation token in case of reconnection
        self.cancellation_token = CancellationToken::new();

        // Check hedge mode
        let is_hedge_mode = self
            .init_hedge_mode()
            .await
            .context("failed to query hedge mode")?;
        self.is_hedge_mode.store(is_hedge_mode, Ordering::Release);
        log::info!("Hedge mode (dual side position): {is_hedge_mode}");

        // Load instruments if not already done
        let _instruments = if self.core.instruments_initialized() {
            Vec::new()
        } else {
            let instruments = self
                .http_client
                .request_instruments()
                .await
                .context("failed to request Binance Futures instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned for Binance Futures");
            } else {
                log::info!("Loaded {} Futures instruments", instruments.len());
            }

            self.core.set_instruments_initialized();
            instruments
        };

        // Apply configured leverage and margin types
        self.apply_futures_config()
            .await
            .context("failed to apply futures config")?;

        // Create listen key for user data stream
        log::info!("Creating listen key for user data stream...");
        let listen_key_response = self
            .http_client
            .create_listen_key()
            .await
            .context("failed to create listen key")?;
        let listen_key = listen_key_response.listen_key;
        log::info!("Listen key created successfully");

        {
            let mut key_guard = self.listen_key.write().expect(MUTEX_POISONED);
            *key_guard = Some(listen_key.clone());
        }

        let (api_key, api_secret) = resolve_credentials(
            self.config.api_key.clone(),
            self.config.api_secret.clone(),
            self.config.environment,
            self.product_type,
        )?;

        let private_base_url = self.config.base_url_ws.clone().map_or_else(
            || get_ws_private_base_url(self.product_type, self.config.environment).to_string(),
            |url| {
                if self.product_type == BinanceProductType::UsdM
                    && self.config.environment == BinanceEnvironment::Mainnet
                {
                    get_usdm_ws_route_base_url(&url, "private")
                } else {
                    url
                }
            },
        );

        let (recovery_tx, recovery_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        *self.recovery_tx.lock().expect(MUTEX_POISONED) = Some(recovery_tx.clone());

        let seen_trade_ids: Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>> =
            Arc::new(Mutex::new(FifoCache::new()));

        let dispatch_ctx = Arc::new(DispatchCtx {
            emitter: self.emitter.clone(),
            http_client: self.http_client.clone(),
            account_id: self.core.account_id,
            product_type: self.product_type,
            clock: self.clock,
            dispatch_state: self.dispatch_state.clone(),
            triggered_algo_ids: self.triggered_algo_order_ids.clone(),
            algo_client_ids: self.algo_client_order_ids.clone(),
            use_position_ids: self.config.use_position_ids,
            default_taker_fee: self.config.default_taker_fee,
            treat_expired_as_canceled: self.config.treat_expired_as_canceled,
            use_trade_lite: self.config.use_trade_lite,
            seen_trade_ids,
            cancellation_token: self.cancellation_token.clone(),
        });

        let ws_build_params = WsBuildParams {
            product_type: self.product_type,
            environment: self.config.environment,
            api_key: api_key.clone(),
            api_secret: api_secret.clone(),
            private_base_url: private_base_url.clone(),
            transport_backend: self.config.transport_backend,
        };

        let ws_client = build_and_connect_user_stream(&ws_build_params, &listen_key).await?;
        let stream = ws_client.stream();
        *self.ws_client.lock().await = Some(ws_client);

        let ws_task = spawn_user_stream_dispatch(
            stream,
            dispatch_ctx.clone(),
            recovery_tx.clone(),
            dispatch_user_stream_message,
        );
        *self.ws_task.lock().expect(MUTEX_POISONED) = Some(ws_task);

        // Start listen key keepalive task
        {
            let http_client = self.http_client.clone();
            let listen_key_ref = self.listen_key.clone();
            let cancel = self.cancellation_token.clone();
            let recovery_tx = recovery_tx.clone();

            let keepalive_task = get_runtime().spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(LISTEN_KEY_KEEPALIVE_SECS));
                let mut consecutive_failures: u32 = 0;

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let key = {
                                let guard = listen_key_ref.read().expect(MUTEX_POISONED);
                                guard.clone()
                            };

                            if let Some(ref key) = key {
                                match http_client.keepalive_listen_key(key).await {
                                    Ok(()) => {
                                        log::debug!("Listen key keepalive sent successfully");
                                        consecutive_failures = 0;
                                    }
                                    Err(e) => {
                                        consecutive_failures += 1;
                                        log::warn!(
                                            "Listen key keepalive failed ({consecutive_failures}/{MAX_KEEPALIVE_FAILURES}): {e}",
                                        );

                                        if consecutive_failures >= MAX_KEEPALIVE_FAILURES
                                            && recovery_tx.send(()).is_err()
                                        {
                                            log::warn!(
                                                "Recovery channel closed, keepalive exiting",
                                            );
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Listen key keepalive task cancelled");
                            break;
                        }
                    }
                }
            });
            *self.keepalive_task.lock().expect(MUTEX_POISONED) = Some(keepalive_task);
        }

        // Start listen key recovery driver task
        {
            let recovery_ctx = RecoveryCtx {
                http_client: self.http_client.clone(),
                listen_key: self.listen_key.clone(),
                ws_client: self.ws_client.clone(),
                ws_task: self.ws_task.clone(),
                recovery_lock: self.recovery_lock.clone(),
                ws_build_params,
                dispatch_ctx,
                recovery_tx: recovery_tx.clone(),
            };
            let cancel = self.cancellation_token.clone();

            let recovery_task = get_runtime().spawn(async move {
                run_recovery_driver(
                    recovery_ctx,
                    recovery_rx,
                    cancel,
                    dispatch_user_stream_message,
                )
                .await;
            });
            *self.recovery_task.lock().expect(MUTEX_POISONED) = Some(recovery_task);
        }

        // Request initial account state
        let account_state = self
            .refresh_account_state()
            .await
            .context("failed to request Binance Futures account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s) and {} margin(s)",
                account_state.balances.len(),
                account_state.margins.len()
            );
        }

        self.emitter.send_account_state(account_state);

        crate::common::execution::await_account_registered(&self.core, self.core.account_id, 30.0)
            .await?;

        // Connect WS trading client (primary order transport for USD-M)
        if let Some(ref mut ws_trading) = self.ws_trading_client {
            match ws_trading.connect().await {
                Ok(()) => {
                    log::info!("Connected to Binance Futures WS trading API");

                    let ws_trading_clone = ws_trading.clone();
                    let emitter = self.emitter.clone();
                    let account_id = self.core.account_id;
                    let clock = self.clock;
                    let dispatch_state = self.dispatch_state.clone();

                    let handle = get_runtime().spawn(async move {
                        while let Some(msg) = ws_trading_clone.recv().await {
                            dispatch_ws_trading_message(
                                msg,
                                &emitter,
                                account_id,
                                clock,
                                &dispatch_state,
                            );
                        }
                    });

                    *self.ws_trading_handle.lock().expect(MUTEX_POISONED) = Some(handle);
                }
                Err(e) => {
                    log::error!(
                        "Failed to connect WS trading API: {e}. \
                         Order operations will use HTTP fallback"
                    );
                }
            }
        }

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        // Drop the recovery tx so the driver exits its recv loop
        self.recovery_tx.lock().expect(MUTEX_POISONED).take();

        // Cancel all background tasks
        self.cancellation_token.cancel();

        // Abort WS trading task and disconnect
        if let Some(handle) = self.ws_trading_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        if let Some(ref mut ws_trading) = self.ws_trading_client {
            ws_trading.disconnect().await;
        }

        // Wait for WebSocket task to complete
        let ws_task = self.ws_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = ws_task {
            let _ = task.await;
        }

        // Abort the keepalive task. An in-flight keepalive_listen_key HTTP
        // call ignores the cancellation token until it returns, so awaiting
        // without aborting can stall disconnect for the full HTTP timeout.
        let keepalive_task = self.keepalive_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = keepalive_task {
            task.abort();
            let _ = task.await;
        }

        // Abort the recovery driver task. Waiting would block disconnect until
        // any in-flight HTTP or WebSocket call inside recover_user_data_stream
        // returns, which can be many seconds under a network outage.
        let recovery_task = self.recovery_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = recovery_task {
            task.abort();
            let _ = task.await;
        }

        // Close WebSocket
        if let Some(mut ws_client) = self.ws_client.lock().await.take() {
            let _ = ws_client.close().await;
        }

        // Close listen key
        let listen_key = self.listen_key.read().expect(MUTEX_POISONED).clone();
        if let Some(ref key) = listen_key
            && let Err(e) = self.http_client.close_listen_key(key).await
        {
            log::warn!("Failed to close listen key: {e}");
        }
        *self.listen_key.write().expect(MUTEX_POISONED) = None;

        self.abort_pending_tasks();

        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            log::warn!("generate_order_status_report requires instrument_id: {cmd:?}");
            return Ok(None);
        };

        let symbol = instrument_id.symbol.to_string();
        let order_id = cmd
            .venue_order_id
            .as_ref()
            .map(|id| {
                id.inner()
                    .parse::<i64>()
                    .context("failed to parse venue_order_id as numeric")
            })
            .transpose()?;
        let orig_client_order_id = cmd
            .client_order_id
            .map(|id| encode_broker_id(&id, BINANCE_NAUTILUS_FUTURES_BROKER_ID));

        let mut builder = BinanceOrderQueryParamsBuilder::default();
        builder.symbol(symbol);

        if let Some(oid) = order_id {
            builder.order_id(oid);
        }

        if let Some(ref coid) = orig_client_order_id {
            builder.orig_client_order_id(coid.clone());
        }
        let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

        let (_, size_precision) = self.get_instrument_precision(instrument_id);
        let ts_init = self.clock.get_time_ns();

        match self.http_client.query_order(&params).await {
            Ok(order) => {
                let report = order.to_order_status_report(
                    self.core.account_id,
                    instrument_id,
                    size_precision,
                    self.config.treat_expired_as_canceled,
                    ts_init,
                )?;
                Ok(Some(report))
            }
            Err(BinanceFuturesHttpError::BinanceError { code: -2013, .. }) => {
                // Order not found in regular API, try algo order API
                let Some(client_order_id) = cmd.client_order_id else {
                    return Ok(None);
                };

                match self.http_client.query_algo_order(client_order_id).await {
                    Ok(algo_order) => {
                        let report = algo_order.to_order_status_report(
                            self.core.account_id,
                            instrument_id,
                            size_precision,
                            ts_init,
                        )?;
                        Ok(Some(report))
                    }
                    Err(e) => {
                        log::debug!("Algo order query also failed: {e}");
                        Ok(None)
                    }
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let ts_init = self.clock.get_time_ns();
        let mut reports = Vec::new();

        if cmd.open_only {
            let symbol = cmd.instrument_id.map(|id| id.symbol.to_string());
            let mut builder = BinanceOpenOrdersParamsBuilder::default();

            if let Some(s) = symbol {
                builder.symbol(s);
            }
            let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

            let (orders, algo_orders) = tokio::try_join!(
                self.http_client.query_open_orders(&params),
                self.http_client.query_open_algo_orders(cmd.instrument_id),
            )?;

            for order in orders {
                if let Some(instrument_id) = cmd.instrument_id {
                    let (_, size_precision) = self.get_instrument_precision(instrument_id);

                    if let Ok(report) = order.to_order_status_report(
                        self.core.account_id,
                        instrument_id,
                        size_precision,
                        self.config.treat_expired_as_canceled,
                        ts_init,
                    ) {
                        reports.push(report);
                    }
                } else {
                    let cache = self.core.cache();
                    if let Some(instrument) = cache
                        .instruments(&BINANCE_VENUE, None)
                        .into_iter()
                        .find(|i| i.symbol().as_str() == order.symbol.as_str())
                        && let Ok(report) = order.to_order_status_report(
                            self.core.account_id,
                            instrument.id(),
                            instrument.size_precision(),
                            self.config.treat_expired_as_canceled,
                            ts_init,
                        )
                    {
                        reports.push(report);
                    }
                }
            }

            for algo_order in algo_orders {
                if let Some(instrument_id) = cmd.instrument_id {
                    let (_, size_precision) = self.get_instrument_precision(instrument_id);

                    if let Ok(report) = algo_order.to_order_status_report(
                        self.core.account_id,
                        instrument_id,
                        size_precision,
                        ts_init,
                    ) {
                        reports.push(report);
                    }
                } else {
                    let cache = self.core.cache();
                    if let Some(instrument) = cache
                        .instruments(&BINANCE_VENUE, None)
                        .into_iter()
                        .find(|i| i.symbol().as_str() == algo_order.symbol.as_str())
                        && let Ok(report) = algo_order.to_order_status_report(
                            self.core.account_id,
                            instrument.id(),
                            instrument.size_precision(),
                            ts_init,
                        )
                    {
                        reports.push(report);
                    }
                }
            }
        } else if let Some(instrument_id) = cmd.instrument_id {
            let symbol = instrument_id.symbol.to_string();
            let start_time = cmd
                .start
                .map(|t| t.as_i64() / NANOSECONDS_IN_MILLISECOND as i64);
            let end_time = cmd
                .end
                .map(|t| t.as_i64() / NANOSECONDS_IN_MILLISECOND as i64);

            let mut builder = BinanceAllOrdersParamsBuilder::default();
            builder.symbol(symbol);

            if let Some(st) = start_time {
                builder.start_time(st);
            }

            if let Some(et) = end_time {
                builder.end_time(et);
            }
            let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

            let orders = self.http_client.query_all_orders(&params).await?;
            let (_, size_precision) = self.get_instrument_precision(instrument_id);

            for order in orders {
                if let Ok(report) = order.to_order_status_report(
                    self.core.account_id,
                    instrument_id,
                    size_precision,
                    self.config.treat_expired_as_canceled,
                    ts_init,
                ) {
                    reports.push(report);
                }
            }
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            log::warn!("generate_fill_reports requires instrument_id for Binance Futures");
            return Ok(Vec::new());
        };

        let symbol = instrument_id.symbol.to_string();
        let start_time = cmd
            .start
            .map(|t| t.as_i64() / NANOSECONDS_IN_MILLISECOND as i64);
        let end_time = cmd
            .end
            .map(|t| t.as_i64() / NANOSECONDS_IN_MILLISECOND as i64);

        let mut builder = BinanceUserTradesParamsBuilder::default();
        builder.symbol(symbol);

        if let Some(st) = start_time {
            builder.start_time(st);
        }

        if let Some(et) = end_time {
            builder.end_time(et);
        }
        let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

        let trades = self.http_client.query_user_trades(&params).await?;
        let (price_precision, size_precision) = self.get_instrument_precision(instrument_id);
        let ts_init = self.clock.get_time_ns();

        let mut reports = Vec::new();

        for trade in trades {
            if let Ok(report) = trade.to_fill_report(
                self.core.account_id,
                instrument_id,
                price_precision,
                size_precision,
                ts_init,
            ) {
                reports.push(report);
            }
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let symbol = cmd.instrument_id.map(|id| id.symbol.to_string());

        let mut builder = BinancePositionRiskParamsBuilder::default();

        if let Some(s) = symbol {
            builder.symbol(s);
        }
        let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

        let positions = self.http_client.query_positions(&params).await?;

        let mut reports = Vec::new();

        for position in positions {
            let position_amt: f64 = position.position_amt.parse().unwrap_or(0.0);
            if position_amt == 0.0 {
                continue;
            }

            let cache = self.core.cache();
            if let Some(instrument) = cache
                .instruments(&BINANCE_VENUE, None)
                .into_iter()
                .find(|i| i.symbol().as_str() == position.symbol.as_str())
                && let Ok(report) = self.create_position_report(
                    &position,
                    instrument.id(),
                    instrument.size_precision(),
                )
            {
                reports.push(report);
            }
        }

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();

        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins_to_nanos(mins);
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(true)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (order_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *BINANCE_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        self.update_account_state();
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::debug!("query_order: client_order_id={}", cmd.client_order_id);

        let http_client = self.http_client.clone();
        let command = cmd;
        let emitter = self.emitter.clone();
        let account_id = self.core.account_id;
        let clock = self.clock;

        let symbol = command.instrument_id.symbol.to_string();
        let order_id = command
            .venue_order_id
            .map(|id| {
                id.inner()
                    .parse::<i64>()
                    .map_err(|e| anyhow::anyhow!("failed to parse venue_order_id: {e}"))
            })
            .transpose()?;
        let orig_client_order_id = Some(encode_broker_id(
            &command.client_order_id,
            BINANCE_NAUTILUS_FUTURES_BROKER_ID,
        ));
        let (_, size_precision) = self.get_instrument_precision(command.instrument_id);
        let treat_expired_as_canceled = self.config.treat_expired_as_canceled;

        self.spawn_task("query_order", async move {
            let mut builder = BinanceOrderQueryParamsBuilder::default();
            builder.symbol(symbol.clone());

            if let Some(oid) = order_id {
                builder.order_id(oid);
            }

            if let Some(coid) = orig_client_order_id {
                builder.orig_client_order_id(coid);
            }
            let params = builder
                .build()
                .map_err(|e| anyhow::anyhow!("failed to build order query params: {e}"))?;

            let result = http_client.query_order(&params).await;

            match result {
                Ok(order) => {
                    let ts_init = clock.get_time_ns();
                    let report = order.to_order_status_report(
                        account_id,
                        command.instrument_id,
                        size_precision,
                        treat_expired_as_canceled,
                        ts_init,
                    )?;

                    emitter.send_order_status_report(report);
                }
                Err(e) => log::warn!("Failed to query order status: {e}"),
            }

            Ok(())
        });

        Ok(())
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        self.emitter.set_sender(get_exec_event_sender());
        self.core.set_started();

        let http_client = self.http_client.clone();

        get_runtime().spawn(async move {
            match http_client.request_instruments().await {
                Ok(instruments) => {
                    if instruments.is_empty() {
                        log::warn!("No instruments returned for Binance Futures");
                    } else {
                        log::info!("Loaded {} Futures instruments", instruments.len());
                    }
                }
                Err(e) => {
                    log::error!("Failed to request Binance Futures instruments: {e}");
                }
            }
        });

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, environment={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.environment,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        if let Some(handle) = self.ws_trading_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        if let Some(handle) = self.ws_task.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        if let Some(handle) = self.keepalive_task.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.recovery_tx.lock().expect(MUTEX_POISONED).take();
        if let Some(handle) = self.recovery_task.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

        if order.is_closed() {
            let client_order_id = order.client_order_id();
            log::warn!("Cannot submit closed order {client_order_id}");
            return Ok(());
        }

        // Validate before submission (Initialized -> Denied is valid,
        // but Submitted -> Denied is not, so validate before emitting OrderSubmitted)
        if let Some(offset_type) = order.trailing_offset_type() {
            if offset_type != TrailingOffsetType::BasisPoints {
                anyhow::bail!(
                    "Binance only supports TrailingOffsetType::BasisPoints, received {offset_type:?}"
                );
            }

            if let Some(offset) = order.trailing_offset() {
                trailing_offset_to_callback_rate(offset)?;
            }
        }

        let close_position = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_bool("close_position"))
            .unwrap_or(false);

        if close_position {
            let order_type = order.order_type();

            if !matches!(
                order_type,
                OrderType::StopMarket | OrderType::MarketIfTouched
            ) {
                anyhow::bail!(
                    "`close_position` is not supported for order type {order_type:?} on Binance"
                );
            }

            if order.is_reduce_only() {
                anyhow::bail!("`close_position` cannot be combined with `reduce_only` on Binance");
            }
        }

        if let Some(pm_str) = cmd.params.as_ref().and_then(|p| p.get_str("price_match")) {
            BinancePriceMatch::from_param(pm_str)?;
            let order_type = order.order_type();
            anyhow::ensure!(
                !order.is_post_only(),
                "price_match cannot be combined with post-only orders"
            );
            anyhow::ensure!(
                order_type == OrderType::Limit,
                "price_match is not supported for order type {order_type:?}"
            );
        }

        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        self.emitter.emit_order_submitted(&order);

        self.submit_order_internal(&cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for Binance Futures (received {} orders)",
            cmd.order_list.client_order_ids.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache.order(&cmd.client_order_id).cloned()
        };

        let Some(order) = order else {
            log::warn!(
                "Cannot modify order {}: not found in cache",
                cmd.client_order_id
            );
            let ts_init = self.clock.get_time_ns();
            let rejected_event = OrderModifyRejected::new(
                self.core.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                "Order not found in cache for modify".into(),
                UUID4::new(),
                ts_init, // no venue timestamp, rejected locally
                ts_init,
                false,
                cmd.venue_order_id,
                Some(self.core.account_id),
            );

            self.emitter
                .send_order_event(OrderEventAny::ModifyRejected(rejected_event));
            return Ok(());
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let instrument_id = cmd.instrument_id;
        let venue_order_id = cmd.venue_order_id;
        let client_order_id = Some(cmd.client_order_id);
        let order_side = order.order_side();
        let quantity = cmd.quantity.unwrap_or_else(|| order.quantity());
        let price = cmd.price.or_else(|| order.price());

        let Some(price) = price else {
            log::warn!(
                "Cannot modify order {}: price required",
                cmd.client_order_id
            );
            let ts_init = self.clock.get_time_ns();
            let rejected_event = OrderModifyRejected::new(
                self.core.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                "Price required for order modification".into(),
                UUID4::new(),
                ts_init, // no venue timestamp, rejected locally
                ts_init,
                false,
                cmd.venue_order_id,
                Some(self.core.account_id),
            );

            self.emitter
                .send_order_event(OrderEventAny::ModifyRejected(rejected_event));
            return Ok(());
        };
        let command = cmd;
        let clock = self.clock;

        if self.ws_trading_active() {
            let ws_client = self.ws_trading_client.as_ref().unwrap().clone();
            let dispatch_state = self.dispatch_state.clone();

            let binance_side = BinanceSide::try_from(order_side)?;
            let orig_client_order_id =
                client_order_id.map(|id| encode_broker_id(&id, BINANCE_NAUTILUS_FUTURES_BROKER_ID));

            let mut modify_builder = BinanceModifyOrderParamsBuilder::default();
            modify_builder
                .symbol(format_binance_symbol(&instrument_id))
                .side(binance_side)
                .quantity(quantity.to_string())
                .price(price.to_string());

            if let Some(venue_id) = venue_order_id {
                let order_id: i64 = venue_id
                    .inner()
                    .parse()
                    .context("failed to parse venue_order_id as numeric")?;
                modify_builder.order_id(order_id);
            }

            if let Some(client_id) = orig_client_order_id {
                modify_builder.orig_client_order_id(client_id);
            }

            let params = modify_builder
                .build()
                .context("failed to build modify params")?;

            // Pre-register before sending to avoid response racing the insert
            let request_id = ws_client.next_request_id();
            dispatch_state.pending_requests.insert(
                request_id.clone(),
                PendingRequest {
                    client_order_id: command.client_order_id,
                    venue_order_id,
                    operation: PendingOperation::Modify,
                },
            );

            self.spawn_task("modify_order_ws", async move {
                if let Err(e) = ws_client
                    .modify_order_with_id(request_id.clone(), params)
                    .await
                {
                    dispatch_state.pending_requests.remove(&request_id);
                    let ts_now = clock.get_time_ns();
                    let rejected = OrderModifyRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        format!("ws-modify-order-error: {e}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );
                    emitter.send_order_event(OrderEventAny::ModifyRejected(rejected));
                    anyhow::bail!("WS modify order failed: {e}");
                }
                Ok(())
            });

            return Ok(());
        }

        self.spawn_task("modify_order", async move {
            let result = http_client
                .modify_order(
                    account_id,
                    instrument_id,
                    venue_order_id,
                    client_order_id,
                    order_side,
                    quantity,
                    price,
                )
                .await;

            match result {
                Ok(report) => {
                    let ts_now = clock.get_time_ns();
                    let updated_event = OrderUpdated::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        quantity,
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        Some(report.venue_order_id),
                        Some(account_id),
                        Some(price),
                        None,
                        None,
                        false, // is_quote_quantity
                    );

                    emitter.send_order_event(OrderEventAny::Updated(updated_event));
                }
                Err(e) => {
                    let ts_now = clock.get_time_ns();
                    let rejected_event = OrderModifyRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        format!("modify-order-failed: {e}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );

                    emitter.send_order_event(OrderEventAny::ModifyRejected(rejected_event));

                    anyhow::bail!("Modify order failed: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_internal(&cmd);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let instrument_id = cmd.instrument_id;

        // USD-M Futures WS Trading API does not expose an openOrders.cancelAll
        // method, so regular and algo cancel-all both go through HTTP.
        self.spawn_task("cancel_all_orders", async move {
            match http_client.cancel_all_orders(instrument_id).await {
                Ok(_) => {
                    log::info!("Cancel all regular orders request accepted for {instrument_id}");
                }
                Err(e) => {
                    log::error!("Failed to cancel all regular orders for {instrument_id}: {e}");
                }
            }

            match http_client.cancel_all_algo_orders(instrument_id).await {
                Ok(()) => {
                    log::info!("Cancel all algo orders request accepted for {instrument_id}");
                }
                Err(e) => {
                    log::error!("Failed to cancel all algo orders for {instrument_id}: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        const BATCH_SIZE: usize = 5;

        if cmd.cancels.is_empty() {
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let command = cmd;

        let emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;

        self.spawn_task("batch_cancel_orders", async move {
            for chunk in command.cancels.chunks(BATCH_SIZE) {
                let batch_items: Vec<BatchCancelItem> = chunk
                    .iter()
                    .map(|cancel| {
                        if let Some(venue_order_id) = cancel.venue_order_id {
                            let order_id = venue_order_id.inner().parse::<i64>().unwrap_or(0);
                            if order_id != 0 {
                                BatchCancelItem::by_order_id(
                                    command.instrument_id.symbol.to_string(),
                                    order_id,
                                )
                            } else {
                                BatchCancelItem::by_client_order_id(
                                    command.instrument_id.symbol.to_string(),
                                    encode_broker_id(
                                        &cancel.client_order_id,
                                        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
                                    ),
                                )
                            }
                        } else {
                            BatchCancelItem::by_client_order_id(
                                command.instrument_id.symbol.to_string(),
                                encode_broker_id(
                                    &cancel.client_order_id,
                                    BINANCE_NAUTILUS_FUTURES_BROKER_ID,
                                ),
                            )
                        }
                    })
                    .collect();

                match http_client.batch_cancel_orders(&batch_items).await {
                    Ok(results) => {
                        for (i, result) in results.iter().enumerate() {
                            let cancel = &chunk[i];

                            match result {
                                BatchOrderResult::Success(response) => {
                                    let venue_order_id =
                                        VenueOrderId::new(response.order_id.to_string());
                                    let canceled_event = OrderCanceled::new(
                                        trader_id,
                                        cancel.strategy_id,
                                        cancel.instrument_id,
                                        cancel.client_order_id,
                                        UUID4::new(),
                                        cancel.ts_init,
                                        clock.get_time_ns(),
                                        false,
                                        Some(venue_order_id),
                                        Some(account_id),
                                    );

                                    emitter
                                        .send_order_event(OrderEventAny::Canceled(canceled_event));
                                }
                                BatchOrderResult::Error(error) => {
                                    let rejected_event = OrderCancelRejected::new(
                                        trader_id,
                                        cancel.strategy_id,
                                        cancel.instrument_id,
                                        cancel.client_order_id,
                                        format!(
                                            "batch-cancel-error: code={}, msg={}",
                                            error.code, error.msg
                                        )
                                        .into(),
                                        UUID4::new(),
                                        clock.get_time_ns(),
                                        cancel.ts_init,
                                        false,
                                        cancel.venue_order_id,
                                        Some(account_id),
                                    );

                                    emitter.send_order_event(OrderEventAny::CancelRejected(
                                        rejected_event,
                                    ));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        for cancel in chunk {
                            let rejected_event = OrderCancelRejected::new(
                                trader_id,
                                cancel.strategy_id,
                                cancel.instrument_id,
                                cancel.client_order_id,
                                format!("batch-cancel-request-failed: {e}").into(),
                                UUID4::new(),
                                clock.get_time_ns(),
                                cancel.ts_init,
                                false,
                                cancel.venue_order_id,
                                Some(account_id),
                            );

                            emitter.send_order_event(OrderEventAny::CancelRejected(rejected_event));
                        }
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }
}
