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
use futures_util::{StreamExt, pin_mut};
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
        AccountType, LiquiditySide, OmsType, OrderSide, OrderType, PositionSideSpecified,
        TrailingOffsetType, TriggerType,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny,
        OrderExpired, OrderFilled, OrderModifyRejected, OrderRejected, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, Symbol, TradeId, Venue,
        VenueOrderId,
    },
    instruments::Instrument,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
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
            messages::{
                BinanceExecutionType, BinanceFuturesAlgoUpdateMsg, BinanceFuturesOrderUpdateMsg,
                BinanceFuturesWsStreamsMessage,
            },
            parse_exec::{
                decode_algo_client_id, parse_futures_account_update,
                parse_futures_algo_update_to_order_status, parse_futures_order_update_to_fill,
                parse_futures_order_update_to_order_status,
            },
        },
        trading::{
            client::BinanceFuturesWsTradingClient, messages::BinanceFuturesWsTradingMessage,
        },
    },
};
use crate::{
    common::{
        consts::{
            BINANCE_FUTURES_USD_WS_API_TESTNET_URL, BINANCE_FUTURES_USD_WS_API_URL,
            BINANCE_GTX_ORDER_REJECT_CODE, BINANCE_NAUTILUS_FUTURES_BROKER_ID, BINANCE_VENUE,
        },
        credential::resolve_credentials,
        dispatch::{
            OrderIdentity, PendingOperation, PendingRequest, WsDispatchState,
            ensure_accepted_emitted,
        },
        encoder::{decode_broker_id, encode_broker_id},
        enums::{
            BinanceEnvironment, BinancePositionSide, BinancePriceMatch, BinanceProductType,
            BinanceSide, BinanceTimeInForce, BinanceWorkingType,
        },
        symbol::format_binance_symbol,
    },
    config::BinanceExecClientConfig,
    futures::http::{
        client::order_type_to_binance_futures,
        models::BinanceFuturesAccountInfo,
        query::{
            BinanceCancelOrderParamsBuilder, BinanceModifyOrderParamsBuilder, BinanceNewOrderParams,
        },
    },
};

/// Listen key keepalive interval (30 minutes).
const LISTEN_KEY_KEEPALIVE_SECS: u64 = 30 * 60;

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
    ws_client: Option<BinanceFuturesWebSocketClient>,
    ws_trading_client: Option<BinanceFuturesWsTradingClient>,
    ws_trading_handle: Mutex<Option<JoinHandle<()>>>,
    listen_key: Arc<RwLock<Option<String>>>,
    cancellation_token: CancellationToken,
    triggered_algo_order_ids: Arc<AtomicSet<ClientOrderId>>,
    algo_client_order_ids: Arc<AtomicSet<ClientOrderId>>,
    ws_task: Mutex<Option<JoinHandle<()>>>,
    keepalive_task: Mutex<Option<JoinHandle<()>>>,
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
                api_key.clone(),
                api_secret.clone(),
                None, // heartbeat
            ))
        } else {
            None
        };

        let ws_client = BinanceFuturesWebSocketClient::new(
            product_type,
            config.environment,
            Some(api_key),
            Some(api_secret),
            config.base_url_ws.clone(),
            Some(20), // Heartbeat interval
        )
        .context("failed to construct Binance Futures WebSocket client")?;

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
            ws_client: Some(ws_client),
            ws_trading_client,
            ws_trading_handle: Mutex::new(None),
            listen_key: Arc::new(RwLock::new(None)),
            cancellation_token: CancellationToken::new(),
            triggered_algo_order_ids: Arc::new(AtomicSet::new()),
            algo_client_order_ids: Arc::new(AtomicSet::new()),
            ws_task: Mutex::new(None),
            keepalive_task: Mutex::new(None),
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

    /// Determines the position side for hedge mode based on order direction.
    fn determine_position_side(
        &self,
        order_side: OrderSide,
        reduce_only: bool,
    ) -> Option<BinancePositionSide> {
        if !self.is_hedge_mode() {
            return None;
        }

        // In hedge mode, position side depends on whether we're opening or closing
        Some(if reduce_only {
            // Closing: Buy closes Short, Sell closes Long
            match order_side {
                OrderSide::Buy => BinancePositionSide::Short,
                OrderSide::Sell => BinancePositionSide::Long,
                _ => BinancePositionSide::Both,
            }
        } else {
            // Opening: Buy opens Long, Sell opens Short
            match order_side {
                OrderSide::Buy => BinancePositionSide::Long,
                OrderSide::Sell => BinancePositionSide::Short,
                _ => BinancePositionSide::Both,
            }
        })
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
                let wallet_balance: f64 = b.wallet_balance.parse().unwrap_or(0.0);
                let available_balance: f64 = b.available_balance.parse().unwrap_or(0.0);
                let locked = wallet_balance - available_balance;

                if wallet_balance == 0.0 {
                    return None;
                }

                let currency = Currency::from(&b.asset);
                Some(AccountBalance::new(
                    Money::new(wallet_balance, currency),
                    Money::new(locked.max(0.0), currency),
                    Money::new(available_balance, currency),
                ))
            })
            .collect();

        let mut margins: Vec<MarginBalance> = Vec::new();

        let initial_margin_dec = account_info
            .total_initial_margin
            .as_ref()
            .and_then(|s| Decimal::from_str_exact(s).ok());
        let maint_margin_dec = account_info
            .total_maint_margin
            .as_ref()
            .and_then(|s| Decimal::from_str_exact(s).ok());

        if let (Some(initial_dec), Some(maint_dec)) = (initial_margin_dec, maint_margin_dec)
            && (!initial_dec.is_zero() || !maint_dec.is_zero())
        {
            let margin_currency = Currency::USDT();
            let margin_instrument_id = InstrumentId::new(Symbol::new("ACCOUNT"), *BINANCE_VENUE);
            let initial_margin = Money::from_decimal(initial_dec, margin_currency)
                .unwrap_or_else(|_| Money::zero(margin_currency));
            let maintenance_margin = Money::from_decimal(maint_dec, margin_currency)
                .unwrap_or_else(|_| Money::zero(margin_currency));
            margins.push(MarginBalance::new(
                initial_margin,
                maintenance_margin,
                margin_instrument_id,
            ));
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
        let position_side = self.determine_position_side(order_side, reduce_only);

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

        // Connect WebSocket and set up execution handler
        if let Some(ref mut ws_client) = self.ws_client {
            log::info!("Connecting to Binance Futures user data stream WebSocket...");
            ws_client.connect().await.map_err(|e| {
                log::error!("Binance Futures WebSocket connection failed: {e:?}");
                anyhow::anyhow!("failed to connect Binance Futures WebSocket: {e}")
            })?;
            log::info!("Binance Futures WebSocket connected");

            // Subscribe to user data stream using listen key
            log::info!("Subscribing to user data stream...");
            ws_client
                .subscribe(vec![listen_key.clone()])
                .await
                .map_err(|e| anyhow::anyhow!("failed to subscribe to user data stream: {e}"))?;
            log::info!("Subscribed to user data stream");

            let stream = ws_client.stream();
            let emitter = self.emitter.clone();
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let clock = self.clock;
            let product_type = self.product_type;
            let use_position_ids = self.config.use_position_ids;
            let default_taker_fee = self.config.default_taker_fee;
            let treat_expired_as_canceled = self.config.treat_expired_as_canceled;
            let dispatch_state = self.dispatch_state.clone();
            let triggered_algo_ids = self.triggered_algo_order_ids.clone();
            let algo_client_ids = self.algo_client_order_ids.clone();
            let cancel = self.cancellation_token.clone();
            let seen_trade_ids: Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>> =
                Arc::new(Mutex::new(FifoCache::new()));

            let ws_task = get_runtime().spawn(async move {
                pin_mut!(stream);

                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            dispatch_ws_message(
                                message,
                                &emitter,
                                &http_client,
                                account_id,
                                product_type,
                                clock,
                                &dispatch_state,
                                &triggered_algo_ids,
                                &algo_client_ids,
                                use_position_ids,
                                default_taker_fee,
                                treat_expired_as_canceled,
                                &seen_trade_ids,
                            );
                        }
                        () = cancel.cancelled() => {
                            log::debug!("WS dispatch task cancelled");
                            break;
                        }
                    }
                }
            });
            *self.ws_task.lock().expect(MUTEX_POISONED) = Some(ws_task);

            // Start listen key keepalive task
            let http_client = self.http_client.clone();
            let listen_key_ref = self.listen_key.clone();
            let cancel = self.cancellation_token.clone();

            let keepalive_task = get_runtime().spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(LISTEN_KEY_KEEPALIVE_SECS));
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
                                    }
                                    Err(e) => {
                                        log::warn!("Listen key keepalive failed: {e}");
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

        // Wait for keepalive task to complete
        let keepalive_task = self.keepalive_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = keepalive_task {
            let _ = task.await;
        }

        // Close WebSocket
        if let Some(ref mut ws_client) = self.ws_client {
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
            "submit_order_list not yet implemented for Binance Futures (got {} orders)",
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
        let ws_active = self.ws_trading_active();
        let ws_client_clone = self.ws_trading_client.clone();

        self.spawn_task("cancel_all_orders", async move {
            if ws_active {
                if let Some(ref ws_client) = ws_client_clone {
                    let symbol = instrument_id.symbol.to_string();
                    if let Err(e) = ws_client.cancel_all_orders(symbol).await {
                        log::error!("WS cancel_all_orders failed: {e}");
                    } else {
                        log::info!(
                            "WS cancel all regular orders request accepted for {instrument_id}"
                        );
                    }
                }
            } else {
                match http_client.cancel_all_orders(instrument_id).await {
                    Ok(_) => {
                        log::info!(
                            "Cancel all regular orders request accepted for {instrument_id}"
                        );
                    }
                    Err(e) => {
                        log::error!("Failed to cancel all regular orders for {instrument_id}: {e}");
                    }
                }
            }

            // Algo orders always go through HTTP (WS API does not support algo service)
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

#[expect(clippy::too_many_arguments)]
fn dispatch_ws_message(
    msg: BinanceFuturesWsStreamsMessage,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceFuturesHttpClient,
    account_id: AccountId,
    product_type: BinanceProductType,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    triggered_algo_ids: &Arc<AtomicSet<ClientOrderId>>,
    algo_client_ids: &Arc<AtomicSet<ClientOrderId>>,
    use_position_ids: bool,
    default_taker_fee: Decimal,
    treat_expired_as_canceled: bool,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
) {
    match msg {
        BinanceFuturesWsStreamsMessage::OrderUpdate(update) => {
            dispatch_order_update(
                &update,
                emitter,
                http_client,
                account_id,
                product_type,
                clock,
                dispatch_state,
                use_position_ids,
                default_taker_fee,
                treat_expired_as_canceled,
                seen_trade_ids,
            );
        }
        BinanceFuturesWsStreamsMessage::AlgoUpdate(update) => {
            dispatch_algo_update(
                &update,
                emitter,
                http_client,
                account_id,
                product_type,
                clock,
                dispatch_state,
                triggered_algo_ids,
                algo_client_ids,
            );
        }
        BinanceFuturesWsStreamsMessage::AccountUpdate(update) => {
            let ts_init = clock.get_time_ns();
            if let Some(state) = parse_futures_account_update(&update, account_id, ts_init) {
                emitter.send_account_state(state);
            }
        }
        BinanceFuturesWsStreamsMessage::MarginCall(mc) => {
            log::warn!(
                "Margin call: cross_wallet_balance={}, positions_at_risk={}",
                mc.cross_wallet_balance,
                mc.positions.len()
            );
        }
        BinanceFuturesWsStreamsMessage::AccountConfigUpdate(cfg) => {
            if let Some(ref lc) = cfg.leverage_config {
                log::info!(
                    "Account config update: symbol={}, leverage={}",
                    lc.symbol,
                    lc.leverage
                );
            }
        }
        BinanceFuturesWsStreamsMessage::ListenKeyExpired => {
            log::warn!("Listen key expired, awaiting reconnection");
        }
        BinanceFuturesWsStreamsMessage::Reconnected => {
            log::info!("User data stream WebSocket reconnected");
        }
        BinanceFuturesWsStreamsMessage::Error(err) => {
            log::error!(
                "User data stream WebSocket error: code={}, msg={}",
                err.code,
                err.msg
            );
        }
        // Market data messages ignored by execution client
        BinanceFuturesWsStreamsMessage::AggTrade(_)
        | BinanceFuturesWsStreamsMessage::Trade(_)
        | BinanceFuturesWsStreamsMessage::BookTicker(_)
        | BinanceFuturesWsStreamsMessage::DepthUpdate(_)
        | BinanceFuturesWsStreamsMessage::MarkPrice(_)
        | BinanceFuturesWsStreamsMessage::Kline(_)
        | BinanceFuturesWsStreamsMessage::ForceOrder(_)
        | BinanceFuturesWsStreamsMessage::Ticker(_) => {}
    }
}

/// Dispatches a Futures order update with tracked/untracked routing.
///
/// Tracked orders produce proper order events. Untracked orders fall back
/// to execution reports for reconciliation.
#[expect(clippy::too_many_arguments)]
fn dispatch_order_update(
    msg: &BinanceFuturesOrderUpdateMsg,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceFuturesHttpClient,
    account_id: AccountId,
    product_type: BinanceProductType,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    use_position_ids: bool,
    default_taker_fee: Decimal,
    treat_expired_as_canceled: bool,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
) {
    let order = &msg.order;
    let symbol_ustr = ustr::Ustr::from(order.symbol.as_str());
    let ts_init = clock.get_time_ns();
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);

    let cache = http_client.instruments_cache();
    let cached_instrument = cache.get(&symbol_ustr);

    let (instrument_id, price_precision, size_precision) = if let Some(ref inst) = cached_instrument
    {
        (
            inst.id(),
            inst.price_precision() as u8,
            inst.quantity_precision() as u8,
        )
    } else {
        let id = crate::common::symbol::format_instrument_id(&symbol_ustr, product_type);
        log::warn!(
            "Instrument not in cache for {}, using default precision",
            order.symbol
        );
        (id, 8, 8)
    };

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &order.client_order_id,
        BINANCE_NAUTILUS_FUTURES_BROKER_ID,
    ));

    // Exchange-generated orders (liquidation/ADL/settlement) are routed through
    // reconciliation reports regardless of tracked/untracked state, because
    // they have no locally submitted identity
    if order.is_exchange_generated() {
        let is_linear = cached_instrument
            .as_ref()
            .map_or(product_type == BinanceProductType::UsdM, |inst| {
                matches!(inst.value(), BinanceFuturesInstrument::UsdM(_))
            });

        let quote_currency = cached_instrument
            .as_ref()
            .map_or_else(Currency::USDT, |inst| inst.value().quote_currency());

        let taker_fee = if is_linear {
            Some(default_taker_fee)
        } else {
            None
        };

        let venue_position_id =
            make_venue_position_id(use_position_ids, instrument_id, order.position_side);

        dispatch_exchange_generated_fill(
            msg,
            emitter,
            instrument_id,
            price_precision,
            size_precision,
            account_id,
            ts_init,
            taker_fee,
            quote_currency,
            venue_position_id,
            seen_trade_ids,
        );
        return;
    }

    let identity = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone());

    if let Some(identity) = identity {
        let venue_order_id = VenueOrderId::new(order.order_id.to_string());

        match order.execution_type {
            BinanceExecutionType::New => {
                if dispatch_state.has_emitted_accepted(&client_order_id)
                    || dispatch_state.has_filled(&client_order_id)
                {
                    log::debug!("Skipping duplicate Accepted for {client_order_id}");
                    return;
                }
                dispatch_state.insert_accepted(client_order_id);
                let accepted = OrderAccepted::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    venue_order_id,
                    account_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                );
                emitter.send_order_event(OrderEventAny::Accepted(accepted));

                // Detect venue-assigned price changes (e.g. priceMatch orders)
                if let Some(submitted_price) = identity.price {
                    let venue_price: f64 = order.original_price.parse().unwrap_or(0.0);
                    if venue_price > 0.0 {
                        let venue_price = Price::new(venue_price, price_precision);
                        let submitted_at_precision =
                            Price::new(submitted_price.as_f64(), price_precision);

                        if venue_price != submitted_at_precision {
                            let quantity: f64 = order.original_qty.parse().unwrap_or(0.0);
                            let trigger_price: f64 = order.stop_price.parse().unwrap_or(0.0);
                            let updated = OrderUpdated::new(
                                emitter.trader_id(),
                                identity.strategy_id,
                                identity.instrument_id,
                                client_order_id,
                                Quantity::new(quantity, size_precision),
                                UUID4::new(),
                                ts_event,
                                ts_init,
                                false,
                                Some(venue_order_id),
                                Some(account_id),
                                Some(venue_price),
                                if trigger_price > 0.0 {
                                    Some(Price::new(trigger_price, price_precision))
                                } else {
                                    None
                                },
                                None,
                                false,
                            );
                            emitter.send_order_event(OrderEventAny::Updated(updated));
                        }
                    }
                }
            }
            BinanceExecutionType::Trade => {
                let dedup_key = (order.symbol, order.trade_id);
                let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
                let is_duplicate = guard.contains(&dedup_key);
                guard.add(dedup_key);
                drop(guard);

                if is_duplicate {
                    log::debug!(
                        "Duplicate trade_id={} for {}, skipping",
                        order.trade_id,
                        order.symbol
                    );
                    return;
                }

                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    dispatch_state,
                    ts_init,
                );

                let last_qty: f64 = order.last_filled_qty.parse().unwrap_or(0.0);
                let last_px: f64 = order.last_filled_price.parse().unwrap_or(0.0);
                let commission: f64 = order
                    .commission
                    .as_deref()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0.0);
                let commission_currency = order
                    .commission_asset
                    .as_ref()
                    .map_or_else(Currency::USDT, |a| Currency::from(a.as_str()));

                let liquidity_side = if order.is_maker {
                    LiquiditySide::Maker
                } else {
                    LiquiditySide::Taker
                };

                let filled = OrderFilled::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    account_id,
                    TradeId::new(order.trade_id.to_string()),
                    identity.order_side,
                    identity.order_type,
                    Quantity::new(last_qty, size_precision),
                    Price::new(last_px, price_precision),
                    commission_currency,
                    liquidity_side,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    None,
                    Some(Money::new(commission, commission_currency)),
                );

                dispatch_state.insert_filled(client_order_id);
                emitter.send_order_event(OrderEventAny::Filled(filled));

                let cum_qty: f64 = order.cumulative_filled_qty.parse().unwrap_or(0.0);
                let orig_qty: f64 = order.original_qty.parse().unwrap_or(0.0);
                if (orig_qty - cum_qty) <= 0.0 {
                    dispatch_state.cleanup_terminal(client_order_id);
                }
            }
            BinanceExecutionType::Canceled => {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    dispatch_state,
                    ts_init,
                );
                let canceled = OrderCanceled::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                dispatch_state.cleanup_terminal(client_order_id);
                emitter.send_order_event(OrderEventAny::Canceled(canceled));
            }
            BinanceExecutionType::Expired => {
                ensure_accepted_emitted(
                    client_order_id,
                    account_id,
                    venue_order_id,
                    &identity,
                    emitter,
                    dispatch_state,
                    ts_init,
                );
                dispatch_state.cleanup_terminal(client_order_id);

                if treat_expired_as_canceled {
                    let canceled = OrderCanceled::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    );
                    emitter.send_order_event(OrderEventAny::Canceled(canceled));
                } else {
                    let expired = OrderExpired::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        client_order_id,
                        UUID4::new(),
                        ts_event,
                        ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    );
                    emitter.send_order_event(OrderEventAny::Expired(expired));
                }
            }
            BinanceExecutionType::Amendment => {
                let quantity: f64 = order.original_qty.parse().unwrap_or(0.0);
                let price: f64 = order.original_price.parse().unwrap_or(0.0);

                let updated = OrderUpdated::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    Quantity::new(quantity, size_precision),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    Some(Price::new(price, price_precision)),
                    None,
                    None,
                    false, // is_quote_quantity
                );
                emitter.send_order_event(OrderEventAny::Updated(updated));
            }
            BinanceExecutionType::Calculated => {
                log::warn!(
                    "CALCULATED for non-exchange-generated order: symbol={}, client_order_id={}",
                    order.symbol,
                    order.client_order_id,
                );
            }
        }
    } else {
        // Untracked: fall back to reports for reconciliation.
        // venue_position_id is intentionally None here: the engine assigns
        // position IDs during event processing, and setting one from the
        // adapter could split a partially filled order across two positions.
        match order.execution_type {
            BinanceExecutionType::Trade => {
                let dedup_key = (order.symbol, order.trade_id);
                let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
                let is_duplicate = guard.contains(&dedup_key);
                guard.add(dedup_key);
                drop(guard);

                if is_duplicate {
                    log::debug!(
                        "Duplicate trade_id={} for {}, skipping",
                        order.trade_id,
                        order.symbol
                    );
                    return;
                }

                match parse_futures_order_update_to_fill(
                    msg,
                    account_id,
                    instrument_id,
                    price_precision,
                    size_precision,
                    None,
                    None,
                    None,
                    ts_init,
                ) {
                    Ok(fill) => emitter.send_fill_report(fill),
                    Err(e) => log::error!("Failed to parse fill report: {e}"),
                }

                match parse_futures_order_update_to_order_status(
                    msg,
                    instrument_id,
                    price_precision,
                    size_precision,
                    account_id,
                    treat_expired_as_canceled,
                    ts_init,
                ) {
                    Ok(status) => emitter.send_order_status_report(status),
                    Err(e) => log::error!("Failed to parse order status report: {e}"),
                }
            }
            BinanceExecutionType::New
            | BinanceExecutionType::Canceled
            | BinanceExecutionType::Expired
            | BinanceExecutionType::Amendment => {
                match parse_futures_order_update_to_order_status(
                    msg,
                    instrument_id,
                    price_precision,
                    size_precision,
                    account_id,
                    treat_expired_as_canceled,
                    ts_init,
                ) {
                    Ok(status) => emitter.send_order_status_report(status),
                    Err(e) => log::error!("Failed to parse order status report: {e}"),
                }
            }
            BinanceExecutionType::Calculated => {
                log::warn!(
                    "CALCULATED for non-exchange-generated order: symbol={}, client_order_id={}",
                    order.symbol,
                    order.client_order_id,
                );
            }
        }
    }
}

/// Dispatches exchange-generated order fills (liquidation, ADL, settlement).
///
/// Sends a `FillReport` first, then an `OrderStatusReport`. The fill report
/// is dropped by the engine (order not yet in cache). The status report
/// triggers `create_external_order`, which builds the order and applies an
/// inferred fill from `avg_px`/`filled_qty`. Real fill metadata (commission,
/// trade_id) is lost; see `engine-bundled-fill-reconciliation` plan for the
/// path to preserving it.
///
/// Derives a venue position ID from the instrument and Binance position side.
///
/// Returns `None` when `use_position_ids` is false.
fn make_venue_position_id(
    use_position_ids: bool,
    instrument_id: InstrumentId,
    position_side: BinancePositionSide,
) -> Option<PositionId> {
    if !use_position_ids {
        return None;
    }

    let side = match position_side {
        BinancePositionSide::Long => "LONG",
        BinancePositionSide::Short => "SHORT",
        BinancePositionSide::Both => "BOTH",
        _ => "UNKNOWN",
    };
    Some(PositionId::new(format!("{instrument_id}-{side}")))
}

/// Skips events with zero fill quantity (pending liquidation notifications).
#[expect(clippy::too_many_arguments)]
fn dispatch_exchange_generated_fill(
    msg: &BinanceFuturesOrderUpdateMsg,
    emitter: &ExecutionEventEmitter,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    account_id: AccountId,
    ts_init: UnixNanos,
    taker_fee: Option<Decimal>,
    quote_currency: Currency,
    venue_position_id: Option<PositionId>,
    seen_trade_ids: &Arc<Mutex<FifoCache<(ustr::Ustr, i64), 10_000>>>,
) {
    let order = &msg.order;
    let last_qty: f64 = order.last_filled_qty.parse().unwrap_or(0.0);

    let order_kind = if order.is_liquidation() {
        "liquidation"
    } else if order.is_adl() {
        "ADL"
    } else {
        "settlement"
    };

    if last_qty == 0.0 {
        log::warn!(
            "Exchange-generated {order_kind} pending: symbol={}, client_order_id={}, status={:?}",
            order.symbol,
            order.client_order_id,
            order.order_status,
        );
        return;
    }

    let dedup_key = (order.symbol, order.trade_id);
    let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
    let is_duplicate = guard.contains(&dedup_key);
    guard.add(dedup_key);
    drop(guard);

    if is_duplicate {
        log::debug!(
            "Duplicate trade_id={} for {}, skipping",
            order.trade_id,
            order.symbol
        );
        return;
    }

    log::warn!(
        "Exchange-generated {order_kind} fill: symbol={}, client_order_id={}, qty={last_qty}, exec_type={:?}",
        order.symbol,
        order.client_order_id,
        order.execution_type,
    );

    match parse_futures_order_update_to_fill(
        msg,
        account_id,
        instrument_id,
        price_precision,
        size_precision,
        taker_fee,
        Some(quote_currency),
        venue_position_id,
        ts_init,
    ) {
        Ok(fill) => emitter.send_fill_report(fill),
        Err(e) => log::error!("Failed to parse fill report: {e}"),
    }

    match parse_futures_order_update_to_order_status(
        msg,
        instrument_id,
        price_precision,
        size_precision,
        account_id,
        false, // Exchange-generated fills are not subject to expired-as-canceled
        ts_init,
    ) {
        Ok(status) => emitter.send_order_status_report(status),
        Err(e) => log::error!("Failed to parse order status report: {e}"),
    }
}

#[expect(clippy::too_many_arguments)]
fn dispatch_algo_update(
    msg: &BinanceFuturesAlgoUpdateMsg,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceFuturesHttpClient,
    account_id: AccountId,
    product_type: BinanceProductType,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    triggered_algo_ids: &Arc<AtomicSet<ClientOrderId>>,
    algo_client_ids: &Arc<AtomicSet<ClientOrderId>>,
) {
    use crate::common::enums::BinanceAlgoStatus;

    let algo_data = &msg.algo_order;
    let ts_init = clock.get_time_ns();
    let ts_event = UnixNanos::from_millis(msg.event_time as u64);
    let client_order_id = decode_algo_client_id(algo_data);

    let symbol_ustr = ustr::Ustr::from(algo_data.symbol.as_str());
    let (instrument_id, _price_precision, _size_precision) =
        if let Some(inst) = http_client.instruments_cache().get(&symbol_ustr) {
            (
                inst.id(),
                inst.price_precision() as u8,
                inst.quantity_precision() as u8,
            )
        } else {
            let id = crate::common::symbol::format_instrument_id(&symbol_ustr, product_type);
            log::warn!(
                "Instrument not in cache for {}, using default precision",
                algo_data.symbol
            );
            (id, 8, 8)
        };

    let identity = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone());

    match algo_data.algo_status {
        BinanceAlgoStatus::New => {
            algo_client_ids.insert(client_order_id);
        }
        BinanceAlgoStatus::Triggering => {
            log::info!(
                "Algo order triggering: client_order_id={}, algo_id={}, symbol={}",
                algo_data.client_algo_id,
                algo_data.algo_id,
                algo_data.symbol
            );
        }
        BinanceAlgoStatus::Triggered => {
            triggered_algo_ids.insert(client_order_id);
            log::info!(
                "Algo order triggered: client_order_id={}, algo_id={}, actual_order_id={:?}",
                algo_data.client_algo_id,
                algo_data.algo_id,
                algo_data.actual_order_id
            );
        }
        BinanceAlgoStatus::Canceled | BinanceAlgoStatus::Expired => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);

            if let Some(identity) = identity {
                let venue_order_id = algo_data
                    .actual_order_id
                    .as_ref()
                    .filter(|id| !id.is_empty())
                    .map(|id| VenueOrderId::new(id.clone()));

                let canceled = OrderCanceled::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    venue_order_id,
                    Some(account_id),
                );
                dispatch_state.cleanup_terminal(client_order_id);
                emitter.send_order_event(OrderEventAny::Canceled(canceled));
            } else if let Some(report) = parse_futures_algo_update_to_order_status(
                algo_data,
                msg.event_time,
                instrument_id,
                _price_precision,
                _size_precision,
                account_id,
                ts_init,
            ) {
                emitter.send_order_status_report(report);
            }
        }
        BinanceAlgoStatus::Rejected => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);

            if let Some(identity) = identity {
                dispatch_state.cleanup_terminal(client_order_id);
                emitter.emit_order_rejected_event(
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    "REJECTED",
                    ts_init,
                    false,
                );
            } else if let Some(report) = parse_futures_algo_update_to_order_status(
                algo_data,
                msg.event_time,
                instrument_id,
                _price_precision,
                _size_precision,
                account_id,
                ts_init,
            ) {
                emitter.send_order_status_report(report);
            }
        }
        BinanceAlgoStatus::Finished => {
            algo_client_ids.remove(&client_order_id);
            triggered_algo_ids.remove(&client_order_id);
            dispatch_state.cleanup_terminal(client_order_id);

            let executed_qty: f64 = algo_data
                .executed_qty
                .as_ref()
                .and_then(|q| q.parse().ok())
                .unwrap_or(0.0);

            if executed_qty > 0.0 {
                log::debug!(
                    "Algo order finished with fills: client_order_id={}, executed_qty={}",
                    algo_data.client_algo_id,
                    executed_qty
                );
            } else {
                log::debug!(
                    "Algo order finished without fills: client_order_id={}",
                    algo_data.client_algo_id
                );
            }
        }
        BinanceAlgoStatus::Unknown => {
            log::warn!(
                "Unknown algo status: client_order_id={}, algo_id={}",
                algo_data.client_algo_id,
                algo_data.algo_id
            );
        }
    }
}

fn dispatch_ws_trading_message(
    msg: BinanceFuturesWsTradingMessage,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    match msg {
        BinanceFuturesWsTradingMessage::OrderAccepted {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order accepted: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderAccepted event comes from user data stream (ORDER_TRADE_UPDATE)
        }
        BinanceFuturesWsTradingMessage::OrderRejected {
            request_id,
            code,
            msg,
        } => {
            log::debug!("WS order rejected: request_id={request_id}, code={code}, msg={msg}");
            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id) {
                // Clone to drop the DashMap read guard before cleanup_terminal
                let identity = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
                    .map(|r| r.clone());

                if let Some(identity) = identity {
                    let due_post_only = i64::from(code) == BINANCE_GTX_ORDER_REJECT_CODE;
                    let ts_now = clock.get_time_ns();
                    let rejected = OrderRejected::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        pending.client_order_id,
                        account_id,
                        ustr::Ustr::from(&format!("code={code}: {msg}")),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        due_post_only,
                    );
                    dispatch_state.cleanup_terminal(pending.client_order_id);
                    emitter.send_order_event(OrderEventAny::Rejected(rejected));
                } else {
                    log::warn!(
                        "No order identity for {}, cannot emit OrderRejected",
                        pending.client_order_id
                    );
                }
            } else {
                log::warn!("No pending request for {request_id}, cannot emit OrderRejected");
            }
        }
        BinanceFuturesWsTradingMessage::OrderCanceled {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order canceled: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderCanceled event comes from user data stream (ORDER_TRADE_UPDATE)
        }
        BinanceFuturesWsTradingMessage::CancelRejected {
            request_id,
            code,
            msg,
        } => {
            log::warn!("WS cancel rejected: request_id={request_id}, code={code}, msg={msg}");
            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id)
                && let Some(identity) = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
            {
                let ts_now = clock.get_time_ns();
                let rejected = OrderCancelRejected::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    pending.client_order_id,
                    ustr::Ustr::from(&format!("code={code}: {msg}")),
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    false,
                    pending.venue_order_id,
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::CancelRejected(rejected));
            }
        }
        BinanceFuturesWsTradingMessage::OrderModified {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order modified: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderUpdated event comes from user data stream (ORDER_TRADE_UPDATE)
        }
        BinanceFuturesWsTradingMessage::ModifyRejected {
            request_id,
            code,
            msg,
        } => {
            log::warn!("WS modify rejected: request_id={request_id}, code={code}, msg={msg}");
            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id)
                && let Some(identity) = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
            {
                let ts_now = clock.get_time_ns();
                let rejected = OrderModifyRejected::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    pending.client_order_id,
                    ustr::Ustr::from(&format!("code={code}: {msg}")),
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    false,
                    pending.venue_order_id,
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::ModifyRejected(rejected));
            }
        }
        BinanceFuturesWsTradingMessage::AllOrdersCanceled { request_id } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!("WS all orders canceled: request_id={request_id}");
        }
        BinanceFuturesWsTradingMessage::Connected => {
            log::info!("WS trading API connected");
        }
        BinanceFuturesWsTradingMessage::Reconnected => {
            log::info!("WS trading API reconnected");
        }
        BinanceFuturesWsTradingMessage::Error(err) => {
            log::error!("WS trading API error: {err}");
        }
    }
}

fn trailing_offset_to_callback_rate(offset: Decimal) -> anyhow::Result<Decimal> {
    let rate = offset / rust_decimal::Decimal::ONE_HUNDRED;
    let min_rate = rust_decimal::Decimal::new(1, 1);
    let max_rate = rust_decimal::Decimal::new(100, 1);

    if rate < min_rate || rate > max_rate {
        anyhow::bail!("callbackRate {rate}% out of Binance range [{min_rate}, {max_rate}]");
    }

    Ok(rate)
}

fn trailing_offset_to_callback_rate_string(offset: Decimal) -> anyhow::Result<String> {
    let rate = trailing_offset_to_callback_rate(offset)?;
    Ok(format_callback_rate(rate))
}

fn format_callback_rate(rate: Decimal) -> String {
    let normalized = rate.normalize();

    if normalized.scale() == 0 {
        format!("{normalized}.0")
    } else {
        normalized.to_string()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::{AccountType, OrderStatus},
        identifiers::{StrategyId, TraderId},
    };
    use rstest::rstest;
    use serde::de::DeserializeOwned;

    use super::*;
    use crate::common::testing::load_fixture_string;

    #[rstest]
    #[case::long(BinancePositionSide::Long, "ETHUSDT-PERP.BINANCE-LONG")]
    #[case::short(BinancePositionSide::Short, "ETHUSDT-PERP.BINANCE-SHORT")]
    #[case::both(BinancePositionSide::Both, "ETHUSDT-PERP.BINANCE-BOTH")]
    #[case::unknown(BinancePositionSide::Unknown, "ETHUSDT-PERP.BINANCE-UNKNOWN")]
    fn test_make_venue_position_id_enabled(
        #[case] side: BinancePositionSide,
        #[case] expected: &str,
    ) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let result = make_venue_position_id(true, instrument_id, side);
        assert_eq!(result, Some(PositionId::from(expected)));
    }

    #[rstest]
    #[case::long(BinancePositionSide::Long)]
    #[case::short(BinancePositionSide::Short)]
    #[case::both(BinancePositionSide::Both)]
    fn test_make_venue_position_id_disabled(#[case] side: BinancePositionSide) {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let result = make_venue_position_id(false, instrument_id, side);
        assert_eq!(result, None);
    }

    #[rstest]
    fn test_trailing_offset_to_callback_rate_preserves_precision() {
        let rate = trailing_offset_to_callback_rate(Decimal::from(25)).unwrap();
        assert_eq!(rate, Decimal::new(25, 2));
    }

    #[rstest]
    fn test_trailing_offset_to_callback_rate_string_formats_whole_percent() {
        let rate = trailing_offset_to_callback_rate_string(Decimal::from(100)).unwrap();
        assert_eq!(rate, "1.0");
    }

    #[rstest]
    fn test_trailing_offset_to_callback_rate_rejects_out_of_range_values() {
        let error = trailing_offset_to_callback_rate(Decimal::from(5)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "callbackRate 0.05% out of Binance range [0.1, 10.0]"
        );
    }

    #[rstest]
    fn test_dispatch_order_update_skips_duplicate_tracked_trade() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            false,
            &seen_trade_ids,
        );
        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(_))))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Order(OrderEventAny::Filled(fill))
                        if fill.trade_id == TradeId::new("12345678")
                ))
                .count(),
            1
        );
    }

    #[rstest]
    fn test_dispatch_order_update_skips_duplicate_untracked_trade() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg = load_user_data_fixture("order_update_trade.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            false,
            &seen_trade_ids,
        );
        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Fill(fill))
                        if fill.trade_id == TradeId::new("12345678")
                ))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Order(status))
                        if status.client_order_id == Some(ClientOrderId::from("TEST"))
                ))
                .count(),
            1
        );
    }

    #[rstest]
    fn test_dispatch_order_update_skips_duplicate_exchange_generated_fill() {
        let clock = get_atomic_clock_realtime();
        let msg: BinanceFuturesOrderUpdateMsg =
            load_user_data_fixture("order_update_calculated.json");
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = WsDispatchState::default();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            false,
            &seen_trade_ids,
        );
        dispatch_order_update(
            &msg,
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            BinanceProductType::UsdM,
            clock,
            &dispatch_state,
            true,
            Decimal::new(4, 4),
            false,
            &seen_trade_ids,
        );

        let events = collect_events(&mut rx);

        assert_eq!(events.len(), 2);
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Fill(fill))
                        if fill.trade_id == TradeId::new("12345999")
                ))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    ExecutionEvent::Report(ExecutionReport::Order(status))
                        if status.order_status == OrderStatus::Filled
                ))
                .count(),
            1
        );
    }

    #[rstest]
    fn test_dispatch_ws_trading_message_emits_cancel_rejected_and_clears_pending_request() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_state.pending_requests.insert(
            "req-cancel".to_string(),
            PendingRequest {
                client_order_id: ClientOrderId::from("TEST"),
                venue_order_id: Some(VenueOrderId::from("12345")),
                operation: PendingOperation::Cancel,
            },
        );

        dispatch_ws_trading_message(
            BinanceFuturesWsTradingMessage::CancelRejected {
                request_id: "req-cancel".to_string(),
                code: -2011,
                msg: "Unknown order sent".to_string(),
            },
            &emitter,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
        );

        assert!(dispatch_state.pending_requests.get("req-cancel").is_none());

        match rx
            .try_recv()
            .expect("Cancel rejection event should be emitted")
        {
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
                assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                assert!(event.reason.as_str().contains("code=-2011"));
            }
            other => panic!("Expected CancelRejected event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_ws_trading_message_emits_modify_rejected_and_clears_pending_request() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT-PERP.BINANCE"),
        );
        dispatch_state.pending_requests.insert(
            "req-modify".to_string(),
            PendingRequest {
                client_order_id: ClientOrderId::from("TEST"),
                venue_order_id: Some(VenueOrderId::from("12345")),
                operation: PendingOperation::Modify,
            },
        );

        dispatch_ws_trading_message(
            BinanceFuturesWsTradingMessage::ModifyRejected {
                request_id: "req-modify".to_string(),
                code: -4028,
                msg: "Price or quantity not changed".to_string(),
            },
            &emitter,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
        );

        assert!(dispatch_state.pending_requests.get("req-modify").is_none());

        match rx
            .try_recv()
            .expect("Modify rejection event should be emitted")
        {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
                assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                assert!(event.reason.as_str().contains("code=-4028"));
            }
            other => panic!("Expected ModifyRejected event, was {other:?}"),
        }
    }

    fn collect_events(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<ExecutionEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    fn create_test_emitter(
        clock: &'static AtomicTime,
    ) -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            AccountId::from("BINANCE-001"),
            AccountType::Margin,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn create_test_http_client(clock: &'static AtomicTime) -> BinanceFuturesHttpClient {
        BinanceFuturesHttpClient::new(
            BinanceProductType::UsdM,
            BinanceEnvironment::Mainnet,
            clock,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
        )
        .expect("Test HTTP client should be created")
    }

    fn create_tracked_dispatch_state(
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
    ) -> WsDispatchState {
        let dispatch_state = WsDispatchState::default();
        dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("TEST-STRATEGY"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                price: None,
            },
        );
        dispatch_state
    }

    fn load_user_data_fixture<T: DeserializeOwned>(filename: &str) -> T {
        let path = format!("futures/user_data_json/{filename}");
        serde_json::from_str(&load_fixture_string(&path))
            .unwrap_or_else(|e| panic!("Failed to parse fixture {path}: {e}"))
    }
}
