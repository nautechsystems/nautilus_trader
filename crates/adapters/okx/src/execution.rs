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

//! Live execution client implementation for the OKX adapter.

use std::{future::Future, sync::Arc};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::{ExecutionClient, ExecutionReportTask},
    live::{
        dst::time::{self, Duration, Instant},
        runner::get_exec_event_sender,
    },
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
        GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
        SubmitOrderList,
    },
};
use nautilus_core::{
    DurationNanos, UnixNanos,
    params::Params,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, SocketControl,
    execution::{
        context::{OrderContext, OrderIdentity},
        failure::CommandFailure,
        reports::retain_order_status_reports,
    },
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{
        AccountType, OmsType, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce,
        TrailingOffsetType,
    },
    events::OrderDeniedReason,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Venue,
        VenueOrderId,
    },
    instruments::InstrumentAny,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    common::{
        consts::{
            OKX_CONDITIONAL_ORDER_TYPES, OKX_RECONCILIATION_LOOKBACK_DEFAULT_MINS,
            OKX_RECONCILIATION_LOOKBACK_MAX_MINS, OKX_SUCCESS_CODE, OKX_VENUE,
            OKX_WS_HEARTBEAT_SECS, okx_algo_target_currency, okx_reduce_only_wire_value,
            resolve_instrument_families, validate_algo_trigger_type, validate_okx_client_order_id,
        },
        enums::{
            OKXAlgoOrderType, OKXInstrumentType, OKXMarginMode, OKXPositionMode, OKXPositionSide,
            OKXTradeMode, OKXTriggerType, conditional_order_to_algo_type, is_advance_algo_order,
        },
        failure::{classify_okx_http_failure, classify_okx_venue_code, classify_okx_ws_failure},
        parse::{
            is_order_status_report_more_advanced, nanos_to_datetime,
            okx_instrument_type_from_symbol, venue_position_id,
        },
        task::{spawn_task, terminate_tasks},
    },
    config::OKXExecutionClientConfig,
    http::{
        client::{
            AlgoOrderReportSweep, FillHistory, OKXHttpClient, OKXPendingAlgoOrderReportsError,
            ReportInstrumentScope,
        },
        models::{OKXAmendAlgoOrderRequest, OKXCancelAlgoOrderRequest},
        query::GetPositionsParams,
    },
    websocket::{
        client::OKXWebSocketClient,
        dispatch::{
            AlgoCancelContext, AlgoOrderState, WsDispatchState, dispatch_ws_message,
            emit_algo_cancel_rejections, emit_send_failed_cancel, format_order_response_reason,
        },
        error::OKXWsError,
        messages::OKXWsMessage,
        parse::{FeeCache, FilledQtyCache, OrderStateSnapshot},
    },
};

#[derive(Debug)]
pub struct OKXExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: OKXExecutionClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: OKXHttpClient,
    ws_private: OKXWebSocketClient,
    ws_business: OKXWebSocketClient,
    trade_mode: OKXTradeMode,
    position_mode: OKXPositionMode,
    ws_dispatch_state: Arc<WsDispatchState>,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
}

impl OKXExecutionClient {
    /// Creates a new [`OKXExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(
        core: ExecutionClientCore,
        config: OKXExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let api_key = config
            .api_key
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let api_secret = config
            .api_secret
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let api_passphrase = config
            .api_passphrase
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let http_client = OKXHttpClient::with_credentials(
            api_key.clone(),
            api_secret.clone(),
            api_passphrase.clone(),
            Some(config.http_base_url()),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.environment,
            proxy_url.clone(),
        )?;

        let account_id = core.account_id;

        let ws_private = OKXWebSocketClient::with_credentials(
            Some(config.ws_private_url()),
            api_key.clone(),
            api_secret.clone(),
            api_passphrase.clone(),
            Some(account_id),
            Some(OKX_WS_HEARTBEAT_SECS),
            config.auth_timeout_secs,
            config.transport_backend,
            proxy_url.clone(),
        )
        .context("failed to construct OKX private websocket client")?
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*OKX_VENUE),
            "okx-private-user-streams",
        ));

        let ws_business = OKXWebSocketClient::with_credentials(
            Some(config.ws_business_url()),
            api_key,
            api_secret,
            api_passphrase,
            Some(account_id),
            Some(OKX_WS_HEARTBEAT_SECS),
            config.auth_timeout_secs,
            config.transport_backend,
            proxy_url,
        )
        .context("failed to construct OKX business websocket client")?
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*OKX_VENUE),
            "okx-business-user-streams",
        ));

        http_client.set_spot_trade_quote_ccy(config.spot_trade_quote_ccy.clone());
        ws_private.set_spot_trade_quote_ccy(config.spot_trade_quote_ccy.clone());
        ws_business.set_spot_trade_quote_ccy(config.spot_trade_quote_ccy.clone());

        let trade_mode = Self::derive_default_trade_mode(core.account_type, &config);
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            None,
        );

        let ws_dispatch_state = Arc::new(WsDispatchState::with_pending_maps(
            ws_private.pending_orders.clone(),
            ws_private.pending_cancels.clone(),
            ws_private.pending_amends.clone(),
        ));

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_private,
            ws_business,
            trade_mode,
            position_mode: OKXPositionMode::NetMode,
            ws_dispatch_state,
            session_tasks: TaskGroup::new(),
            pending_tasks: TaskGroup::new(),
        })
    }

    fn derive_default_trade_mode(
        account_type: AccountType,
        config: &OKXExecutionClientConfig,
    ) -> OKXTradeMode {
        let is_cross_margin = config.margin_mode == Some(OKXMarginMode::Cross);

        if account_type == AccountType::Cash {
            if !config.use_spot_margin {
                return OKXTradeMode::Cash;
            }
            return if is_cross_margin {
                OKXTradeMode::Cross
            } else {
                OKXTradeMode::Isolated
            };
        }

        if is_cross_margin {
            OKXTradeMode::Cross
        } else {
            OKXTradeMode::Isolated
        }
    }

    fn trade_mode_for_order(
        &self,
        instrument_id: InstrumentId,
        params: Option<&Params>,
    ) -> OKXTradeMode {
        if let Some(td_mode_str) = get_param_as_string(params, "td_mode") {
            match td_mode_str.parse::<OKXTradeMode>() {
                Ok(mode) => return mode,
                Err(_) => {
                    log::warn!("Invalid td_mode '{td_mode_str}', using derived trade mode");
                }
            }
        }

        derive_trade_mode_for_instrument(
            instrument_id,
            self.config.margin_mode,
            self.config.use_spot_margin,
        )
    }

    fn report_client(&self) -> OKXReportClient {
        OKXReportClient {
            account_id: self.core.account_id,
            http_client: self.http_client.clone(),
            instrument_types: self.config.instrument_types.clone(),
            load_spreads: self.config.load_spreads,
            margin_mode: (self.position_mode == OKXPositionMode::LongShortMode).then_some(
                self.config
                    .margin_mode
                    .filter(|mode| *mode != OKXMarginMode::None)
                    .unwrap_or(OKXMarginMode::Isolated),
            ),
        }
    }

    fn instrument_types(&self) -> Vec<OKXInstrumentType> {
        OKXReportClient::instrument_types(&self.config.instrument_types)
    }

    async fn collect_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
        require_complete_active_coverage: bool,
        max_history_pages: Option<u32>,
    ) -> anyhow::Result<OrderReportSweep> {
        let sweep = self
            .report_client()
            .collect_order_status_reports(cmd, require_complete_active_coverage, max_history_pages)
            .await?;
        Ok(finish_order_report_sweep(&self.core, sweep, cmd))
    }

    async fn collect_fill_reports(
        &self,
        cmd: GenerateFillReports,
        history: FillHistory,
        max_history_pages: Option<u32>,
    ) -> anyhow::Result<(Vec<FillReport>, bool)> {
        self.report_client()
            .collect_fill_reports(cmd, history, max_history_pages)
            .await
    }

    async fn collect_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<(Vec<PositionStatusReport>, bool)> {
        self.report_client()
            .collect_position_status_reports(cmd)
            .await
    }

    fn query_order_state(&self, cmd: &GenerateOrderStatusReport) -> Option<CachedQueryOrderState> {
        let cache = self.core.cache();
        cmd.client_order_id.and_then(|client_order_id| {
            cache
                .order(&client_order_id)
                .map(|order| CachedQueryOrderState {
                    order_type: order.order_type(),
                    venue_order_id: order.venue_order_id(),
                })
        })
    }

    fn update_account_state(&self) {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let account_type = self.core.account_type;
        let emitter = self.emitter.clone();

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(account_id, account_type)
                .await
                .context("failed to request OKX account state")?;
            emitter.send_account_state(account_state);
            Ok(())
        });
    }

    fn is_conditional_order(&self, order_type: OrderType) -> bool {
        OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type)
    }

    fn submit_order_route(
        &self,
        instrument_id: InstrumentId,
        order_type: OrderType,
    ) -> anyhow::Result<OrderCommandRoute> {
        if self.is_conditional_order(order_type) {
            if self.http_client.is_spread(instrument_id) {
                anyhow::bail!(
                    "Trigger/conditional orders ({order_type:?}) are not supported for OKX spreads"
                );
            }

            let inst_type = okx_instrument_type_from_symbol(instrument_id.symbol.as_str());
            if inst_type == OKXInstrumentType::Option {
                anyhow::bail!(
                    "Trigger/conditional orders ({order_type:?}) are not supported for OKX options"
                );
            }

            return Ok(OrderCommandRoute::AlgoHttp);
        }

        if self.http_client.is_spread(instrument_id) {
            Ok(OrderCommandRoute::SpreadHttp)
        } else {
            Ok(OrderCommandRoute::RegularWs)
        }
    }

    fn order_command_route(
        &self,
        instrument_id: InstrumentId,
        order_state: Option<(OrderType, Option<bool>)>,
        has_bound_child: bool,
    ) -> OrderCommandRoute {
        if self.http_client.is_spread(instrument_id) {
            return OrderCommandRoute::SpreadHttp;
        }

        if has_bound_child {
            return OrderCommandRoute::RegularWs;
        }

        if order_state.is_some_and(|(order_type, is_triggered)| {
            self.is_conditional_order(order_type) && is_triggered != Some(true)
        }) {
            OrderCommandRoute::AlgoHttp
        } else {
            OrderCommandRoute::RegularWs
        }
    }

    fn cancel_all_orders_route(
        &self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
    ) -> CancelAllOrdersRoute {
        if order_side.is_some() {
            CancelAllOrdersRoute::Batch
        } else if self.http_client.is_spread(instrument_id) {
            CancelAllOrdersRoute::SpreadHttp
        } else if self.config.use_mm_mass_cancel {
            CancelAllOrdersRoute::MassCancelHttp
        } else {
            CancelAllOrdersRoute::Batch
        }
    }

    fn submit_regular_order(
        &self,
        cmd: &SubmitOrder,
        position_side: Option<PositionSide>,
    ) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache.try_order_owned(&cmd.client_order_id)?
        };
        let ws_private = self.ws_private.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, cmd.params.as_ref());

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let trader_id = self.core.trader_id;
        let context = OrderContext::from(&order);

        self.ws_dispatch_state
            .order_identities
            .insert(context.identity.client_order_id, context.identity);
        let client_order_id = context.identity.client_order_id;
        let strategy_id = context.identity.strategy_id;
        let instrument_id = context.identity.instrument_id;
        let order_side = context.identity.order_side;
        let order_type = context.identity.order_type;
        let quantity = context.quantity;
        let time_in_force = context.time_in_force;
        let price = context.price;
        let trigger_price = context.trigger_price;
        let is_post_only = context.is_post_only;
        let is_reduce_only = context.is_reduce_only;

        let is_quote_quantity = context.is_quote_quantity;

        let px_usd = get_param_as_string(cmd.params.as_ref(), "px_usd");
        let px_vol = get_param_as_string(cmd.params.as_ref(), "px_vol");
        let outcome = get_param_as_string(cmd.params.as_ref(), "outcome");
        let slippage_pct = get_param_as_string(cmd.params.as_ref(), "slippage_pct");
        let rpi = get_param_as_bool(cmd.params.as_ref(), "rpi");
        let rpi_taker_access = get_param_as_bool(cmd.params.as_ref(), "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(cmd.params.as_ref(), "rpi_px_round");

        self.spawn_task("submit_order", async move {
            let result = ws_private
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    trade_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    Some(time_in_force),
                    price,
                    trigger_price,
                    Some(is_post_only),
                    Some(is_reduce_only),
                    Some(is_quote_quantity),
                    position_side,
                    None,
                    px_usd,
                    px_vol,
                    outcome,
                    slippage_pct,
                    rpi,
                    rpi_taker_access,
                    rpi_px_round,
                )
                .await;

            if let Err(e) = result {
                emit_submit_failure(
                    classify_okx_ws_failure(&e),
                    &emitter,
                    clock,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                );
                return Err(anyhow::Error::new(e).context("submit order failed"));
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_http(
        &self,
        cmd: &SubmitOrder,
        position_side: Option<PositionSide>,
    ) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache.try_order_owned(&cmd.client_order_id)?
        };
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, cmd.params.as_ref());

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let context = OrderContext::from(&order);

        self.ws_dispatch_state
            .order_identities
            .insert(context.identity.client_order_id, context.identity);
        let client_order_id = context.identity.client_order_id;
        let strategy_id = context.identity.strategy_id;
        let instrument_id = context.identity.instrument_id;
        let order_side = context.identity.order_side;
        let order_type = context.identity.order_type;
        let quantity = context.quantity;
        let time_in_force = context.time_in_force;
        let price = context.price;
        let is_post_only = context.is_post_only;
        let is_reduce_only = context.is_reduce_only;
        let rpi = get_param_as_bool(cmd.params.as_ref(), "rpi");
        let rpi_taker_access = get_param_as_bool(cmd.params.as_ref(), "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(cmd.params.as_ref(), "rpi_px_round");

        self.spawn_task("submit_order_http", async move {
            let result = http_client
                .place_order_with_domain_types(
                    instrument_id,
                    trade_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    Some(time_in_force),
                    price,
                    Some(is_post_only),
                    Some(is_reduce_only),
                    None,
                    position_side,
                    None,
                    None,
                    None,
                    None,
                    None,
                    rpi,
                    rpi_taker_access,
                    rpi_px_round,
                )
                .await;

            if let Err(e) = result {
                emit_submit_failure(
                    classify_okx_http_failure(&e),
                    &emitter,
                    clock,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                );
                return Err(anyhow::Error::new(e).context("submit order failed"));
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_conditional_order(
        &self,
        cmd: &SubmitOrder,
        position_side: Option<PositionSide>,
    ) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache.try_order_owned(&cmd.client_order_id)?
        };
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, cmd.params.as_ref());

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let context = OrderContext::from(&order);

        let client_order_id = context.identity.client_order_id;
        let strategy_id = context.identity.strategy_id;
        let instrument_id = context.identity.instrument_id;
        let order_side = context.identity.order_side;
        let order_type = context.identity.order_type;
        let quantity = context.quantity;
        let trigger_type = context.trigger_type;
        let trigger_price = context.trigger_price;
        let price = context.price;
        let is_reduce_only = context.is_reduce_only;

        let tgt_ccy = okx_algo_target_currency(
            okx_instrument_type_from_symbol(instrument_id.symbol.as_str()),
            trade_mode,
            order_side,
            order_type,
            context.is_quote_quantity,
        )
        .map_err(anyhow::Error::msg)?;

        let trailing_offset = order.trailing_offset();
        let trailing_offset_type = order.trailing_offset_type();
        let activation_price = order.activation_price();

        let close_fraction = get_param_as_string(cmd.params.as_ref(), "close_fraction");

        let reduce_only = if close_fraction.is_some() {
            Some(true)
        } else {
            Some(is_reduce_only)
        };

        let (callback_ratio, callback_spread) = if order_type == OrderType::TrailingStopMarket {
            let offset = trailing_offset
                .ok_or_else(|| anyhow::anyhow!("TrailingStopMarket requires trailing_offset"))?;
            let offset_type = trailing_offset_type.ok_or_else(|| {
                anyhow::anyhow!("TrailingStopMarket requires trailing_offset_type")
            })?;

            match offset_type {
                TrailingOffsetType::BasisPoints => {
                    // Convert basis points to ratio (e.g., 100 bps = 0.01)
                    let ratio = offset / Decimal::from(10000);
                    (Some(ratio.to_string()), None)
                }
                TrailingOffsetType::Price => (None, Some(offset.to_string())),
                _ => {
                    anyhow::bail!("Unsupported trailing_offset_type for OKX: {offset_type:?}");
                }
            }
        } else {
            (None, None)
        };

        let algo_order_type = if close_fraction.is_some() || tgt_ccy.is_some() {
            OKXAlgoOrderType::Conditional
        } else {
            conditional_order_to_algo_type(order_type)?
        };

        self.ws_dispatch_state.algo_order_states.insert(
            client_order_id,
            AlgoOrderState {
                order_type: algo_order_type,
                ts_last: UnixNanos::default(),
            },
        );

        self.ws_dispatch_state.track_order_context(context);
        let dispatch_state = Arc::clone(&self.ws_dispatch_state);

        self.spawn_task("submit_algo_order", async move {
            let result = http_client
                .place_algo_order_with_domain_types(
                    instrument_id,
                    trade_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    context.is_quote_quantity,
                    trigger_price,
                    trigger_type,
                    price,
                    reduce_only,
                    position_side,
                    close_fraction,
                    callback_ratio,
                    callback_spread,
                    activation_price,
                )
                .await;

            match result {
                Ok(response) => {
                    dispatch_state.bind_order(
                        client_order_id,
                        VenueOrderId::new(response.algo_id.as_str()),
                    );
                }
                Err(e) => {
                    let failure = classify_okx_http_failure(&e);
                    dispatch_state.resolve_algo_submit_failure(client_order_id, &failure);
                    emit_submit_failure(
                        failure,
                        &emitter,
                        clock,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                    );
                    return Err(anyhow::Error::new(e).context("submit algo order failed"));
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_algo_order(&self, cmd: &ModifyOrder) {
        let algo_type = self
            .ws_dispatch_state
            .algo_order_states
            .get(&cmd.client_order_id)
            .map(|state| state.order_type);
        let context = self.ws_dispatch_state.order_context(cmd.client_order_id);

        let request = match algo_amend_request(cmd, algo_type, context) {
            Ok(request) => request,
            Err(reason) => {
                emit_modify_failure(
                    CommandFailure::NotSent(reason.to_string()),
                    &self.emitter,
                    self.clock,
                    cmd.strategy_id,
                    cmd.instrument_id,
                    cmd.client_order_id,
                    cmd.venue_order_id,
                );
                return;
            }
        };

        let http_client = self.http_client.clone();
        let command = cmd.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        self.spawn_task("modify_algo_order", async move {
            let result = http_client.amend_algo_order(request).await;

            let failure = match result {
                Err(e) => Some(classify_okx_http_failure(&e)),
                Ok(response) => response
                    .s_code
                    .as_deref()
                    .filter(|code| *code != OKX_SUCCESS_CODE)
                    .map(|code| {
                        classify_okx_venue_code(
                            code,
                            format_order_response_reason(
                                code,
                                response.s_msg.as_deref().unwrap_or(""),
                                "",
                            ),
                        )
                    }),
            };

            if let Some(failure) = failure {
                emit_modify_failure(
                    failure,
                    &emitter,
                    clock,
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                );
            }

            Ok(())
        });
    }

    fn cancel_ws_order(&self, cmd: &CancelOrder) {
        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);

        let ws_private = self.ws_private.clone();
        let state = Arc::clone(&self.ws_dispatch_state);
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let mut command = cmd.clone();
        command.venue_order_id = self
            .ws_dispatch_state
            .order_venue_binding(cmd.client_order_id)
            .map(|(venue_order_id, _)| venue_order_id)
            .or(cmd.venue_order_id);

        self.spawn_task("cancel_order", async move {
            let result = ws_private
                .cancel_order(
                    command.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    Some(command.client_order_id),
                    command.venue_order_id,
                )
                .await;

            if let Err(e) = result {
                let failure = classify_okx_ws_failure(&e);

                if matches!(e, OKXWsError::HandlerUnavailable(_)) {
                    emit_send_failed_cancel(
                        &failure,
                        &state,
                        &emitter,
                        clock,
                        command.client_order_id,
                        command.venue_order_id,
                    );
                } else {
                    emit_cancel_failure(
                        failure,
                        None,
                        command.client_order_id,
                        command.instrument_id,
                        command.strategy_id,
                        command.venue_order_id,
                    );
                }

                return Err(anyhow::Error::new(e).context("cancel order failed"));
            }

            Ok(())
        });
    }

    fn cancel_order_http(&self, cmd: &CancelOrder) {
        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);

        let http_client = self.http_client.clone();
        let command = cmd.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_order_http", async move {
            let result = http_client
                .cancel_order(
                    command.instrument_id,
                    Some(command.client_order_id),
                    command.venue_order_id,
                )
                .await;

            if let Err(e) = result {
                emit_cancel_failure(
                    classify_okx_http_failure(&e),
                    Some((&emitter, clock)),
                    command.client_order_id,
                    command.instrument_id,
                    command.strategy_id,
                    command.venue_order_id,
                );
                return Err(anyhow::Error::new(e).context("cancel order failed"));
            }

            Ok(())
        });
    }

    fn cancel_algo_order(&self, cmd: &CancelOrder) {
        let http_client = self.http_client.clone();
        let command = cmd.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let cache = self.core.cache();
        let is_advance = cache
            .order(&cmd.client_order_id)
            .is_some_and(|o| is_advance_algo_order(o.order_type()));
        drop(cache);

        let request = OKXCancelAlgoOrderRequest {
            inst_id: cmd.instrument_id.symbol.to_string(),
            inst_id_code: None,
            algo_id: cmd.venue_order_id.map(|id| id.to_string()),
            algo_cl_ord_id: if cmd.venue_order_id.is_none() {
                Some(cmd.client_order_id.to_string())
            } else {
                None
            },
        };

        self.spawn_task("cancel_algo_order", async move {
            let responses = if is_advance {
                http_client.cancel_advance_algo_orders(vec![request]).await
            } else {
                http_client.cancel_algo_orders(vec![request]).await
            };

            match responses {
                Err(e) => {
                    emit_cancel_failure(
                        classify_okx_http_failure(&e),
                        Some((&emitter, clock)),
                        command.client_order_id,
                        command.instrument_id,
                        command.strategy_id,
                        command.venue_order_id,
                    );
                    return Err(anyhow::Error::new(e).context("cancel algo order failed"));
                }
                Ok(resps) => {
                    if let Some((code, msg)) = resps.first().and_then(|r| {
                        r.s_code.as_deref().and_then(|code| {
                            (code != OKX_SUCCESS_CODE)
                                .then_some((code, r.s_msg.as_deref().unwrap_or("")))
                        })
                    }) {
                        let reason = format_order_response_reason(code, msg, "");
                        let failure = classify_okx_venue_code(code, reason.clone());
                        let is_rejected = matches!(failure, CommandFailure::VenueRejected(_));
                        emit_cancel_failure(
                            failure,
                            Some((&emitter, clock)),
                            command.client_order_id,
                            command.instrument_id,
                            command.strategy_id,
                            command.venue_order_id,
                        );

                        if is_rejected {
                            anyhow::bail!("{reason}");
                        }
                    }
                }
            }

            Ok(())
        });
    }

    fn mass_cancel_instrument(&self, instrument_id: InstrumentId) {
        if self.http_client.is_spread(instrument_id) {
            let http_client = self.http_client.clone();
            self.spawn_task("mass_cancel_orders_http", async move {
                if let Err(e) = http_client.cancel_all_orders(instrument_id).await {
                    log_mass_cancel_failure(classify_okx_http_failure(&e), instrument_id);
                    return Err(anyhow::Error::new(e).context("mass cancel orders failed"));
                }
                Ok(())
            });
            return;
        }

        let ws_private = self.ws_private.clone();

        self.spawn_task("mass_cancel_orders", async move {
            if let Err(e) = ws_private.mass_cancel_orders(instrument_id).await {
                log_mass_cancel_failure(classify_okx_ws_failure(&e), instrument_id);
                return Err(anyhow::Error::new(e).context("mass cancel orders failed"));
            }
            Ok(())
        });
    }

    /// Populates `order_identities` for an order if not already present.
    ///
    /// Needed for cancel/modify commands on orders loaded via reconciliation
    /// (which bypass `submit_order` and therefore have no identity entry).
    /// Uses `DashMap::entry().or_insert_with` to keep the check-and-insert
    /// atomic; without it, two concurrent reconciliation tasks could race
    /// past a `contains_key` check and overwrite each other with stale
    /// cache state.
    fn ensure_order_identity(
        &self,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) {
        if self
            .ws_dispatch_state
            .order_identity(client_order_id)
            .is_some()
        {
            return;
        }

        let cache = self.core.cache();

        let order_identity = cache
            .order(&client_order_id)
            .filter(|order| !OKX_CONDITIONAL_ORDER_TYPES.contains(&order.order_type()))
            .map(|order| OrderIdentity {
                client_order_id,
                instrument_id,
                strategy_id,
                order_side: order.order_side(),
                order_type: order.order_type(),
            });

        drop(cache);

        if let Some(order_identity) = order_identity {
            self.ws_dispatch_state
                .order_identities
                .entry(client_order_id)
                .or_insert(order_identity);
        }
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let fut = async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        };

        match self.pending_tasks.spawner() {
            Ok(spawner) => spawn_task(&spawner, fut),
            Err(e) => log::debug!("Skipping {description} after OKX shutdown began: {e}"),
        }
    }

    // Partitions algo cancel orders into regular and advance, then spawns
    // HTTP tasks for each group with per-item and batch-level rejection handling.
    fn dispatch_algo_cancels(&self, items: Vec<(OKXCancelAlgoOrderRequest, AlgoCancelContext)>) {
        const BATCH_SIZE: usize = 10;

        let mut regular_requests = Vec::new();
        let mut regular_contexts = Vec::new();
        let mut advance_requests = Vec::new();
        let mut advance_contexts = Vec::new();

        let cache = self.core.cache();

        for (request, ctx) in items {
            let is_advance = cache
                .order(&ctx.client_order_id)
                .is_some_and(|o| is_advance_algo_order(o.order_type()));

            if is_advance {
                advance_requests.push(request);
                advance_contexts.push(ctx);
            } else {
                regular_requests.push(request);
                regular_contexts.push(ctx);
            }
        }

        drop(cache);

        for (requests, contexts) in regular_requests
            .chunks(BATCH_SIZE)
            .zip(regular_contexts.chunks(BATCH_SIZE))
        {
            let regular_requests = requests.to_vec();
            let regular_contexts = contexts.to_vec();
            let client = self.http_client.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;

            self.spawn_task("cancel_algo_orders", async move {
                match client.cancel_algo_orders(regular_requests).await {
                    Ok(responses) => {
                        emit_algo_cancel_rejections(&responses, &regular_contexts, &emitter, clock);
                    }
                    Err(e) => {
                        log_algo_batch_cancel_failure(
                            classify_okx_http_failure(&e),
                            &regular_contexts,
                        );
                        return Err(anyhow::Error::new(e).context("cancel algo orders failed"));
                    }
                }
                Ok(())
            });
        }

        for (requests, contexts) in advance_requests
            .chunks(BATCH_SIZE)
            .zip(advance_contexts.chunks(BATCH_SIZE))
        {
            let advance_requests = requests.to_vec();
            let advance_contexts = contexts.to_vec();
            let client = self.http_client.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;

            self.spawn_task("cancel_advance_algo_orders", async move {
                match client.cancel_advance_algo_orders(advance_requests).await {
                    Ok(responses) => {
                        emit_algo_cancel_rejections(&responses, &advance_contexts, &emitter, clock);
                    }
                    Err(e) => {
                        log_algo_batch_cancel_failure(
                            classify_okx_http_failure(&e),
                            &advance_contexts,
                        );
                        return Err(
                            anyhow::Error::new(e).context("cancel advance algo orders failed")
                        );
                    }
                }
                Ok(())
            });
        }
    }

    fn begin_generation_shutdown(&self) {
        self.pending_tasks.begin_shutdown();
        self.session_tasks.begin_shutdown();
        self.ws_private.begin_shutdown();
        self.ws_business.begin_shutdown();
        self.core.set_disconnected();
    }

    /// Polls the cache until the account is registered or timeout is reached.
    async fn await_account_registered(&self, timeout_secs: f64) -> anyhow::Result<()> {
        let account_id = self.core.account_id;

        if self.core.cache().account(&account_id).is_some() {
            log::info!("Account {account_id} registered");
            return Ok(());
        }

        let start = Instant::now();
        let timeout = Duration::from_secs_f64(timeout_secs);
        let interval = Duration::from_millis(10);

        loop {
            time::sleep(interval).await;

            if self.core.cache().account(&account_id).is_some() {
                log::info!("Account {account_id} registered");
                return Ok(());
            }

            if start.elapsed() >= timeout {
                anyhow::bail!(
                    "Timeout waiting for account {account_id} to be registered after {timeout_secs}s"
                );
            }
        }
    }

    /// Establishes instrument context, both WebSocket transports, private
    /// subscriptions, and initial account state.
    ///
    /// Any failure leaves partially started transports for
    /// [`Self::teardown_session`].
    async fn establish_session(&mut self) -> anyhow::Result<()> {
        // Reset leaves the old generation canceled until this async boundary can drain it
        if !self.pending_tasks.is_empty()
            || !self.session_tasks.is_empty()
            || !self.pending_tasks.is_open()
            || !self.session_tasks.is_open()
            || self.ws_private.is_active()
            || self.ws_business.is_active()
            || self.ws_private.has_task()
            || self.ws_business.has_task()
        {
            self.teardown_session().await?;
        }

        if !self.pending_tasks.is_open() {
            self.pending_tasks
                .start_generation()
                .context("failed to start OKX execution request task generation")?;
        }

        if !self.session_tasks.is_open() {
            self.session_tasks
                .start_generation()
                .context("failed to start OKX execution stream task generation")?;
        }
        let instrument_types = self.instrument_types();

        self.update_position_mode().await?;

        if !self.core.instruments_initialized() {
            let mut all_instruments = Vec::new();
            let mut all_inst_id_codes = Vec::new();

            for instrument_type in &instrument_types {
                let Some(families) =
                    resolve_instrument_families(&self.config.instrument_families, *instrument_type)
                else {
                    continue;
                };

                if families.is_empty() {
                    let (instruments, inst_id_codes) = self
                        .http_client
                        .request_instruments(*instrument_type, None)
                        .await
                        .with_context(|| {
                            format!("failed to request OKX instruments for {instrument_type:?}")
                        })?;

                    if instruments.is_empty() {
                        anyhow::bail!(
                            "No usable instruments for {instrument_type:?}, \
                             cannot initialize execution client"
                        );
                    }

                    log::debug!(
                        "Loaded {} {instrument_type:?} instruments",
                        instruments.len()
                    );

                    self.http_client.cache_instruments(&instruments);
                    all_instruments.extend(instruments);
                    all_inst_id_codes.extend(inst_id_codes);
                } else {
                    for family in &families {
                        let (instruments, inst_id_codes) = self
                            .http_client
                            .request_instruments(*instrument_type, Some(family.clone()))
                            .await
                            .with_context(|| {
                                format!(
                                    "failed to request OKX instruments for {instrument_type:?} family {family}"
                                )
                            })?;

                        if instruments.is_empty() {
                            anyhow::bail!(
                                "No usable instruments for {instrument_type:?} family {family}, \
                                 cannot initialize execution client"
                            );
                        }

                        log::debug!(
                            "Loaded {} {instrument_type:?} instruments for family {family}",
                            instruments.len()
                        );

                        self.http_client.cache_instruments(&instruments);
                        all_instruments.extend(instruments);
                        all_inst_id_codes.extend(inst_id_codes);
                    }
                }
            }

            if all_instruments.is_empty() {
                anyhow::bail!(
                    "No instruments loaded for configured types {instrument_types:?}, \
                     cannot initialize execution client"
                );
            }

            if instrument_types.contains(&OKXInstrumentType::Spot)
                && let Err(e) = self
                    .http_client
                    .refresh_account_trade_quote_ccy_lists(OKXInstrumentType::Spot, None)
                    .await
            {
                log::warn!("Failed to refresh account tradeQuoteCcyList: {e}");
            }

            let trade_quote_ccy_lists = self.http_client.trade_quote_ccy_lists_snapshot();
            self.ws_private.cache_instruments(&all_instruments);
            self.ws_private
                .cache_inst_id_codes(all_inst_id_codes.clone());
            self.ws_private
                .cache_trade_quote_ccy_lists(trade_quote_ccy_lists.clone());
            self.ws_business.cache_instruments(&all_instruments);
            self.ws_business.cache_inst_id_codes(all_inst_id_codes);
            self.ws_business
                .cache_trade_quote_ccy_lists(trade_quote_ccy_lists);
            self.core.set_instruments_initialized();
        }

        self.ws_private.connect().await?;
        self.ws_private.wait_until_active(10.0).await?;
        log::info!("Connected to private WebSocket");

        {
            let stream = self.ws_private.stream();
            let emitter = self.emitter.clone();
            let state = Arc::clone(&self.ws_dispatch_state);
            let account_id = self.core.account_id;
            let account_type = self.core.account_type;
            let instruments = self.ws_private.instruments_cache_arc();
            let tasks = self
                .session_tasks
                .spawner()
                .context("OKX execution stream task admission is closed")?;
            let cancel = tasks.cancellation_token();
            let clock = self.clock;

            spawn_task(&tasks, async move {
                let mut fee_cache = FeeCache::new();
                let mut filled_qty_cache = FilledQtyCache::new();
                let mut order_state_cache: AHashMap<ClientOrderId, OrderStateSnapshot> =
                    AHashMap::new();

                pin_mut!(stream);

                loop {
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => break,
                        message = stream.next() => {
                            let Some(message) = message else {
                                break;
                            };
                            dispatch_ws_message(
                                message,
                                &emitter,
                                &state,
                                account_id,
                                account_type,
                                &instruments,
                                &mut fee_cache,
                                &mut filled_qty_cache,
                                &mut order_state_cache,
                                clock,
                            );
                        }
                        () = state.wait_for_linked_child_route() => {
                            dispatch_ws_message(
                                OKXWsMessage::Orders(Vec::new()),
                                &emitter,
                                &state,
                                account_id,
                                account_type,
                                &instruments,
                                &mut fee_cache,
                                &mut filled_qty_cache,
                                &mut order_state_cache,
                                clock,
                            );
                        }
                    }
                }
            });
        }

        self.ws_business.connect().await?;
        self.ws_business.wait_until_active(10.0).await?;
        log::info!("Connected to business WebSocket");

        {
            let stream = self.ws_business.stream();
            let emitter = self.emitter.clone();
            let state = Arc::clone(&self.ws_dispatch_state);
            let account_id = self.core.account_id;
            let account_type = self.core.account_type;
            let instruments = self.ws_business.instruments_cache_arc();
            let tasks = self
                .session_tasks
                .spawner()
                .context("OKX execution stream task admission is closed")?;
            let cancel = tasks.cancellation_token();
            let clock = self.clock;

            spawn_task(&tasks, async move {
                let mut fee_cache = FeeCache::new();
                let mut filled_qty_cache = FilledQtyCache::new();
                let mut order_state_cache: AHashMap<ClientOrderId, OrderStateSnapshot> =
                    AHashMap::new();

                pin_mut!(stream);

                loop {
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => break,
                        message = stream.next() => {
                            let Some(message) = message else {
                                break;
                            };
                            dispatch_ws_message(
                                message,
                                &emitter,
                                &state,
                                account_id,
                                account_type,
                                &instruments,
                                &mut fee_cache,
                                &mut filled_qty_cache,
                                &mut order_state_cache,
                                clock,
                            );
                        }
                    }
                }
            });
        }

        let order_routing_types = order_routing_instrument_types(&instrument_types);

        for inst_type in &order_routing_types {
            log::debug!("Subscribing to orders channel for {inst_type:?}");
            self.ws_private.subscribe_orders(*inst_type).await?;
        }

        self.ws_private.subscribe_account().await?;

        // Liquidation warnings cover margin and derivative positions; SPOT has none
        if order_routing_types.iter().any(|t| {
            matches!(
                t,
                OKXInstrumentType::Margin
                    | OKXInstrumentType::Swap
                    | OKXInstrumentType::Futures
                    | OKXInstrumentType::Option
            )
        }) {
            log::debug!("Subscribing to liquidation warning channel");
            self.ws_private
                .subscribe_liquidation_warning(OKXInstrumentType::Any)
                .await?;
        }

        if self.config.load_spreads {
            log::debug!("Subscribing to Nitro spread orders channel");
            self.ws_business.subscribe_spread_orders().await?;
        }

        // Subscribe to algo orders on business WebSocket (OKX requires this endpoint)
        for inst_type in &order_routing_types {
            if supports_algo_orders(*inst_type) {
                self.ws_business.subscribe_orders_algo(*inst_type).await?;
                self.ws_business.subscribe_algo_advance(*inst_type).await?;
            }
        }

        let account_state = self
            .http_client
            .request_account_state(self.core.account_id, self.core.account_type)
            .await
            .context("failed to request OKX account state")?;

        if !account_state.balances.is_empty() {
            log::debug!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }
        self.emitter.send_account_state(account_state);

        // Wait for account to be registered in cache before completing connect
        self.await_account_registered(30.0).await?;

        Ok(())
    }

    async fn update_position_mode(&mut self) -> anyhow::Result<()> {
        let instrument_types = self.instrument_types();
        if instrument_types.iter().any(|instrument_type| {
            matches!(
                instrument_type,
                OKXInstrumentType::Swap | OKXInstrumentType::Futures
            )
        }) {
            let configurations = self
                .http_client
                .raw_client()
                .get_account_configuration()
                .await?;
            self.position_mode = configurations
                .first()
                .context("OKX account configuration response is empty")?
                .position_mode;

            if self.position_mode == OKXPositionMode::LongShortMode {
                let report_client = self.report_client();
                let scope = report_client.report_scope(&instrument_types);
                for inst_type in &instrument_types {
                    if !matches!(
                        inst_type,
                        OKXInstrumentType::Swap | OKXInstrumentType::Futures
                    ) {
                        continue;
                    }

                    let positions = self
                        .http_client
                        .get_positions(GetPositionsParams {
                            inst_type: Some(*inst_type),
                            ..Default::default()
                        })
                        .await?;

                    for position in positions {
                        scope.validate_position_margin(&position)?;
                    }
                }
            }

            let expected_oms = match self.position_mode {
                OKXPositionMode::NetMode => OmsType::Netting,
                OKXPositionMode::LongShortMode => OmsType::Hedging,
            };

            if self.core.oms_type != expected_oms {
                log::warn!(
                    "OKX position mode {:?} differs from OMS type {:?}, set oms_type to {expected_oms}",
                    self.position_mode,
                    self.core.oms_type,
                );
            }
        }

        Ok(())
    }

    /// Drains application tasks before closing transports so task-owned
    /// WebSocket clones cannot outlive the session.
    async fn teardown_session(&mut self) -> anyhow::Result<()> {
        self.begin_generation_shutdown();
        self.ws_private.request_close().await;
        self.ws_business.request_close().await;
        let pending_result = terminate_tasks(&self.pending_tasks, "OKX execution request").await;
        let session_result = terminate_tasks(&self.session_tasks, "OKX execution stream").await;

        let private_result = self
            .ws_private
            .close()
            .await
            .context("failed to close private websocket");
        let business_result = self
            .ws_business
            .close()
            .await
            .context("failed to close business websocket");

        self.core.set_disconnected();

        let mut errors = Vec::new();
        if let Err(e) = pending_result {
            errors.push(e.to_string());
        }

        if let Err(e) = session_result {
            errors.push(e.to_string());
        }

        if let Err(e) = private_result {
            errors.push(e.to_string());
        }

        if let Err(e) = business_result {
            errors.push(e.to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(errors.join("; "))
        }
    }
}

fn derive_trade_mode_for_instrument(
    instrument_id: InstrumentId,
    margin_mode: Option<OKXMarginMode>,
    use_spot_margin: bool,
) -> OKXTradeMode {
    let inst_type = okx_instrument_type_from_symbol(instrument_id.symbol.as_str());
    let is_cross_margin = margin_mode == Some(OKXMarginMode::Cross);

    match inst_type {
        OKXInstrumentType::Spot => {
            if use_spot_margin {
                if is_cross_margin {
                    OKXTradeMode::Cross
                } else {
                    OKXTradeMode::Isolated
                }
            } else {
                OKXTradeMode::Cash
            }
        }
        _ => {
            if is_cross_margin {
                OKXTradeMode::Cross
            } else {
                OKXTradeMode::Isolated
            }
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for OKXExecutionClient {
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
        *OKX_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        _instrument_id: InstrumentId,
        _strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
        let cache = self.core.cache();

        let Some(order) = cache.order_ref(&client_order_id) else {
            log::warn!("Cannot track imported OKX order {client_order_id}: order not cached");
            return;
        };

        if order.order_type() != OrderType::Limit {
            return;
        }

        let context = OrderContext::from(&*order);
        self.ws_dispatch_state
            .register_external_order(context, venue_order_id);
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() && self.pending_tasks.is_open() && self.session_tasks.is_open()
        {
            return Ok(());
        }
        let ws_private = self.ws_private.clone();
        let ws_business = self.ws_business.clone();
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {
                ws_private.begin_shutdown();
                ws_business.begin_shutdown();
            });

        if let Err(e) = self.establish_session().await {
            if let Err(teardown_error) = self.teardown_session().await {
                return Err(e.context(format!(
                    "OKX execution startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        self.core.set_connected();
        setup_guard.disarm();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected()
            && self.pending_tasks.is_empty()
            && self.session_tasks.is_empty()
            && !self.ws_private.has_task()
            && !self.ws_business.has_task()
        {
            return Ok(());
        }

        self.teardown_session().await?;
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        self.update_account_state();
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let order_state = {
            let cache = self.core.cache();
            cache
                .order(&client_order_id)
                .map(|order| CachedQueryOrderState {
                    order_type: order.order_type(),
                    venue_order_id: order.venue_order_id(),
                })
        };

        let venue_binding = self.ws_dispatch_state.order_venue_binding(client_order_id);
        let authoritative_venue_order_id = venue_binding.map(|(venue_order_id, _)| venue_order_id);
        let has_bound_child = venue_binding.is_some_and(|(_, has_bound_child)| has_bound_child);
        let cached_venue_order_id = order_state.and_then(|state| state.venue_order_id);
        let regular_venue_order_id = if has_bound_child {
            authoritative_venue_order_id
        } else {
            order_state.and_then(|state| {
                if OKX_CONDITIONAL_ORDER_TYPES.contains(&state.order_type) {
                    state.venue_order_id.or(venue_order_id)
                } else {
                    state.venue_order_id
                }
            })
        };

        let selection_venue_order_id = authoritative_venue_order_id
            .or(cached_venue_order_id)
            .or(venue_order_id);
        let route = query_order_route(
            instrument_id,
            order_state.map(|state| state.order_type),
            regular_venue_order_id.is_some(),
            self.http_client.is_spread(instrument_id),
        );
        self.spawn_task("query_order", async move {
            let mut reports = Vec::with_capacity(1);
            let mut query_algo = matches!(
                route,
                QueryOrderRoute::Algo | QueryOrderRoute::RegularAndAlgo
            );

            match route {
                QueryOrderRoute::Spread => {
                    match http_client
                        .request_order_status_reports(
                            account_id,
                            None,
                            Some(instrument_id),
                            None,
                            None,
                            false,
                            None,
                        )
                        .await
                    {
                        Ok(spread_reports) => reports.extend(spread_reports),
                        Err(e) => {
                            log::warn!("OKX query_order failed to fetch spread order: {e}");
                        }
                    }
                }
                QueryOrderRoute::Regular | QueryOrderRoute::RegularThenAlgo => {
                    let result = if let Some(venue_order_id) = regular_venue_order_id {
                        http_client
                            .request_order_status_report_by_venue_order_id(
                                account_id,
                                instrument_id,
                                venue_order_id,
                            )
                            .await
                    } else {
                        http_client
                            .request_order_status_report(
                                account_id,
                                instrument_id,
                                client_order_id,
                            )
                            .await
                    };

                    match result {
                        Ok(Some(report)) => reports.push(report),
                        Ok(None) => {
                            query_algo |= route == QueryOrderRoute::RegularThenAlgo;
                        }
                        Err(e) => {
                            log::warn!("OKX query_order failed to fetch regular order: {e}");
                        }
                    }
                }
                QueryOrderRoute::Algo | QueryOrderRoute::RegularAndAlgo => {}
            }

            // Known conditional orders query the algo endpoint by client ID. If
            // the parent has triggered, query its single latest regular child so
            // a missed child event can supersede the parent state. For an
            // uncached order, only fall back after the regular lookup has no match.
            if query_algo {
                let mut regular_child_venue_order_id = None;

                match http_client
                    .request_algo_order_status_reports(
                        account_id,
                        None,
                        Some(instrument_id),
                        None,
                        Some(client_order_id),
                        None,
                        Some(1),
                    )
                    .await
                {
                    Ok(algo_reports) => {
                        if matches!(
                            route,
                            QueryOrderRoute::Algo | QueryOrderRoute::RegularAndAlgo
                        ) {
                            regular_child_venue_order_id = algo_reports
                                .iter()
                                .find(|report| {
                                    matches!(
                                        report.order_status,
                                        OrderStatus::Triggered | OrderStatus::Filled
                                    )
                                })
                                .map(|report| report.venue_order_id)
                                .or_else(|| {
                                    regular_venue_order_id.filter(|venue_order_id| {
                                        algo_reports.first().is_none_or(|report| {
                                            report.venue_order_id != *venue_order_id
                                        })
                                    })
                                });
                        }

                        merge_order_status_reports(&mut reports, algo_reports);
                    }
                    Err(e) => {
                        if route == QueryOrderRoute::RegularAndAlgo {
                            regular_child_venue_order_id = regular_venue_order_id;
                        }

                        log::warn!("OKX query_order algo lookup failed for {instrument_id}: {e}");
                    }
                }

                if let Some(child_venue_order_id) = regular_child_venue_order_id {
                    match http_client
                        .request_order_status_report_by_venue_order_id(
                            account_id,
                            instrument_id,
                            child_venue_order_id,
                        )
                        .await
                    {
                        Ok(Some(child_report)) => {
                            merge_order_status_reports(&mut reports, vec![child_report]);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::warn!(
                                "OKX query_order failed to fetch regular child order: {e}"
                            );
                        }
                    }
                }
            }

            let Some(report) = select_query_order_report(
                reports,
                client_order_id,
                selection_venue_order_id,
            ) else {
                log::warn!(
                    "OKX query_order found no order for client_order_id={client_order_id}, venue_order_id={selection_venue_order_id:?}",
                );
                return Ok(());
            };

            emitter.send_order_status_report(report);
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
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event, info);
        Ok(())
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        self.http_client.cache_instrument(instrument.clone());
        self.ws_private.cache_instrument(instrument.clone());
        self.ws_business.cache_instrument(instrument);
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, trade_mode={:?}, instrument_types={:?}, environment={}, proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.trade_mode,
            self.config.instrument_types,
            self.config.environment,
            self.config.proxy_url,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        let was_started = self.core.is_started();
        self.core.set_stopped();
        self.begin_generation_shutdown();

        if was_started {
            log::info!("Stopped: client_id={}", self.core.client_id);
        }
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.begin_generation_shutdown();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.begin_generation_shutdown();
        Ok(())
    }

    fn generate_order_status_report_task(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> Option<ExecutionReportTask<Option<OrderStatusReport>>> {
        let client = self.report_client();
        let order_state = self.query_order_state(cmd);
        let command = cmd.clone();
        Some(ExecutionReportTask::new(
            async move {
                client
                    .generate_order_status_report(&command, order_state)
                    .await
            },
            Ok,
        ))
    }

    fn generate_order_status_reports_task(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> Option<ExecutionReportTask<Vec<OrderStatusReport>>> {
        let client = self.report_client();
        let core = self.core.clone();
        let command = cmd.clone();
        let collection_command = command.clone();
        Some(ExecutionReportTask::new(
            async move {
                let mut sweep = client
                    .collect_order_status_reports(&collection_command, false, None)
                    .await?;
                sweep.regular_by_venue_order_id = AHashMap::new();
                sweep.ambiguous_triggered_child_ids = AHashSet::new();
                Ok(sweep)
            },
            move |sweep| Ok(finish_order_report_sweep(&core, sweep, &command).reports),
        ))
    }

    fn generate_fill_reports_task(
        &self,
        cmd: &GenerateFillReports,
    ) -> Option<ExecutionReportTask<Vec<FillReport>>> {
        let client = self.report_client();
        let command = cmd.clone();
        Some(ExecutionReportTask::new(
            async move {
                client
                    .collect_fill_reports(command, FillHistory::Recent, None)
                    .await
            },
            |(reports, _)| Ok(reports),
        ))
    }

    fn generate_position_status_reports_task(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> Option<ExecutionReportTask<Vec<PositionStatusReport>>> {
        let client = self.report_client();
        let command = cmd.clone();
        Some(ExecutionReportTask::new(
            async move { client.collect_position_status_reports(&command).await },
            |(reports, _)| Ok(reports),
        ))
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.report_client()
            .generate_order_status_report(cmd, self.query_order_state(cmd))
            .await
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Ok(self
            .collect_order_status_reports(cmd, false, None)
            .await?
            .reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        Ok(self
            .collect_fill_reports(cmd, FillHistory::Recent, None)
            .await?
            .0)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        Ok(self.collect_position_status_reports(cmd).await?.0)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();

        let lookback_mins = lookback_mins
            .unwrap_or(OKX_RECONCILIATION_LOOKBACK_DEFAULT_MINS)
            .min(OKX_RECONCILIATION_LOOKBACK_MAX_MINS);
        let fill_history = if lookback_mins <= OKX_RECONCILIATION_LOOKBACK_DEFAULT_MINS {
            FillHistory::Recent
        } else {
            FillHistory::Extended
        };
        let lookback = DurationNanos::try_from_mins(lookback_mins)?;
        let start = Some(ts_now.saturating_sub(lookback));

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false) // get all orders for mass status
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let fill_cmd = GenerateFillReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (
            order_sweep,
            (fill_reports, fills_complete),
            (mut position_reports, positions_complete),
        ) = tokio::try_join!(
            self.collect_order_status_reports(
                &order_cmd,
                true,
                self.config.max_reconciliation_history_pages,
            ),
            self.collect_fill_reports(
                fill_cmd,
                fill_history,
                self.config.max_reconciliation_history_pages,
            ),
            self.collect_position_status_reports(&position_cmd),
        )?;
        let OrderReportSweep {
            fallback_parents: _,
            reports: mut order_reports,
            complete: mut orders_complete,
            ambiguous_triggered_child_ids,
            regular_by_venue_order_id,
        } = order_sweep;
        orders_complete &= self
            .recover_triggered_child_order_reports(
                &mut order_reports,
                &ambiguous_triggered_child_ids,
                &regular_by_venue_order_id,
            )
            .await;

        if positions_complete {
            self.add_flat_derivative_position_reports(
                &order_reports,
                &fill_reports,
                &mut position_reports,
                ts_now,
            );
        }
        let reports_complete = orders_complete && fills_complete && positions_complete;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} FillReports", fill_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *OKX_VENUE,
            ts_now,
            None,
        );
        mass_status.set_report_window(start, reports_complete);
        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let (route, position_side) = {
            let cache = self.core.cache();
            let order = cache.try_order(&cmd.client_order_id)?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            let trade_mode = self.trade_mode_for_order(cmd.instrument_id, cmd.params.as_ref());

            if let Err(reason) = validate_order(
                &*order,
                trade_mode,
                OrderSubmission::Single,
                self.http_client.is_spread(order.instrument_id()),
            ) {
                self.emitter.emit_order_denied(&order, &reason.to_string());
                return Ok(());
            }

            let position_side = self.order_position_side(&*order, cmd.params.as_ref());
            if let Err(reason) = self
                .validate_trade_mode(cmd.instrument_id, position_side, trade_mode)
                .and_then(|()| {
                    validate_submit_position_id(cmd.instrument_id, position_side, cmd.position_id)
                })
            {
                self.emitter.emit_order_denied(&order, &reason.to_string());
                return Ok(());
            }

            let order_type = order.order_type();
            let route = self.submit_order_route(cmd.instrument_id, order_type)?;

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(&order);

            (route, position_side)
        };

        match route {
            OrderCommandRoute::RegularWs => self.submit_regular_order(&cmd, position_side),
            OrderCommandRoute::AlgoHttp => self.submit_conditional_order(&cmd, position_side),
            OrderCommandRoute::SpreadHttp => self.submit_order_http(&cmd, position_side),
        }
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        if self.http_client.is_spread(cmd.instrument_id) {
            let cache = self.core.cache();
            let denied = OrderDeniedReason::UnsupportedOrderList {
                detail: "spread instruments are not supported in order lists".to_string(),
            }
            .to_string();

            for client_order_id in &cmd.order_list.client_order_ids {
                let order = cache.try_order(client_order_id)?;
                self.emitter.emit_order_denied(&order, &denied);
            }
            return Ok(());
        }

        let inst_type = okx_instrument_type_from_symbol(cmd.instrument_id.symbol.as_str());
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, cmd.params.as_ref());

        // Validate all orders before emitting any submitted events
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        // Pre-validate every order so an invalid leg denies the whole list atomically;
        // otherwise sibling legs would be left in the cache without a terminal event.
        let invalid: Vec<(ClientOrderId, OrderDeniedReason)> = orders
            .iter()
            .filter_map(|order| {
                validate_order(
                    order,
                    trade_mode,
                    OrderSubmission::List,
                    self.http_client.is_spread(order.instrument_id()),
                )
                .and_then(|()| {
                    self.validate_trade_mode(
                        cmd.instrument_id,
                        self.order_position_side(order, cmd.params.as_ref()),
                        trade_mode,
                    )
                })
                .and_then(|()| {
                    validate_submit_position_id(
                        cmd.instrument_id,
                        self.order_position_side(order, cmd.params.as_ref()),
                        cmd.position_id,
                    )
                })
                .err()
                .map(|reason| (order.client_order_id(), reason))
            })
            .collect();

        if !invalid.is_empty() {
            let order_list_id = cmd.order_list.id;

            for order in &orders {
                let denied = invalid
                    .iter()
                    .find(|(client_order_id, _)| client_order_id == &order.client_order_id())
                    .map_or_else(
                        || OrderDeniedReason::OrderListDenied { order_list_id },
                        |(_, reason)| reason.clone(),
                    );
                self.emitter.emit_order_denied(order, &denied.to_string());
            }
            return Ok(());
        }

        // Build batch payload and emit submitted events
        let mut batch_orders = Vec::new();
        let outcome = get_param_as_string(cmd.params.as_ref(), "outcome");
        let rpi = get_param_as_bool(cmd.params.as_ref(), "rpi");
        let rpi_taker_access = get_param_as_bool(cmd.params.as_ref(), "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(cmd.params.as_ref(), "rpi_px_round");

        for order in &orders {
            let context = OrderContext::from(order);

            batch_orders.push((
                inst_type,
                cmd.instrument_id,
                trade_mode,
                context.identity.client_order_id,
                context.identity.order_side,
                self.order_position_side(order, cmd.params.as_ref()),
                context.identity.order_type,
                context.quantity,
                context.price,
                context.trigger_price,
                Some(context.is_post_only),
                Some(context.is_reduce_only),
                outcome.clone(),
                rpi,
                rpi_taker_access,
                rpi_px_round,
            ));

            self.ws_dispatch_state
                .order_identities
                .insert(context.identity.client_order_id, context.identity);

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);
        }

        let ws_private = self.ws_private.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instrument_id = cmd.instrument_id;
        let strategy_id = cmd.strategy_id;
        let client_order_ids: Vec<_> = cmd.order_list.client_order_ids;
        let dispatch_state = Arc::clone(&self.ws_dispatch_state);

        self.spawn_task("batch_submit_orders", async move {
            let result = ws_private
                .batch_submit_orders(batch_orders)
                .await;

            if let Err(e) = result {
                match classify_okx_ws_failure(&e) {
                    CommandFailure::NotSent(reason) => {
                        let ts_event = clock.get_time_ns();

                        for cid in &client_order_ids {
                            dispatch_state.order_identities.remove(cid);
                            emitter.emit_order_rejected_event(
                                strategy_id,
                                instrument_id,
                                *cid,
                                &reason,
                                ts_event,
                                false,
                            );
                        }
                    }
                    CommandFailure::Ambiguous(reason) | CommandFailure::VenueRejected(reason) => {
                        log::warn!(
                            "Ambiguous batch submit failure for {} orders on {instrument_id}, awaiting reconciliation: {reason}",
                            client_order_ids.len()
                        );
                    }
                }
                return Err(anyhow::Error::new(e).context("batch submit orders failed"));
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        if self.http_client.is_spread(cmd.instrument_id) {
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                cmd.venue_order_id,
                "OKX spread orders do not support modify requests",
                self.clock.get_time_ns(),
            );
            return Ok(());
        }

        let venue_binding = self
            .ws_dispatch_state
            .order_venue_binding(cmd.client_order_id);

        let route = {
            let cache = self.core.cache();
            let order_state = cache
                .order(&cmd.client_order_id)
                .map(|order| (order.order_type(), order.is_triggered()));
            self.order_command_route(
                cmd.instrument_id,
                order_state,
                venue_binding.is_some_and(|(_, has_bound_child)| has_bound_child),
            )
        };

        let mut command = cmd;
        command.venue_order_id = venue_binding
            .map(|(venue_order_id, _)| venue_order_id)
            .or(command.venue_order_id);
        if route == OrderCommandRoute::AlgoHttp {
            self.modify_algo_order(&command);
            return Ok(());
        }

        self.ensure_order_identity(
            command.client_order_id,
            command.strategy_id,
            command.instrument_id,
        );
        let ws_private = self.ws_private.clone();

        let new_px_usd = get_param_as_string(command.params.as_ref(), "px_usd");
        let new_px_vol = get_param_as_string(command.params.as_ref(), "px_vol");
        let rpi_taker_access = get_param_as_bool(command.params.as_ref(), "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(command.params.as_ref(), "rpi_px_round");

        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("modify_order", async move {
            let result = ws_private
                .modify_order(
                    command.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    Some(command.client_order_id),
                    command.price,
                    command.quantity,
                    command.venue_order_id,
                    new_px_usd,
                    new_px_vol,
                    rpi_taker_access,
                    rpi_px_round,
                )
                .await;

            if let Err(e) = result {
                emit_modify_failure(
                    classify_okx_ws_failure(&e),
                    &emitter,
                    clock,
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                );
                return Err(anyhow::Error::new(e).context("modify order failed"));
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, mut cmd: CancelOrder) -> anyhow::Result<()> {
        let venue_binding = self
            .ws_dispatch_state
            .order_venue_binding(cmd.client_order_id);
        let route = {
            let cache = self.core.cache();
            let order_state = cache
                .order(&cmd.client_order_id)
                .map(|order| (order.order_type(), order.is_triggered()));
            self.order_command_route(
                cmd.instrument_id,
                order_state,
                venue_binding.is_some_and(|(_, has_bound_child)| has_bound_child),
            )
        };

        cmd.venue_order_id = venue_binding
            .map(|(venue_order_id, _)| venue_order_id)
            .or(cmd.venue_order_id);

        match route {
            OrderCommandRoute::RegularWs => self.cancel_ws_order(&cmd),
            OrderCommandRoute::AlgoHttp => self.cancel_algo_order(&cmd),
            OrderCommandRoute::SpreadHttp => self.cancel_order_http(&cmd),
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        match self.cancel_all_orders_route(cmd.instrument_id, cmd.order_side) {
            CancelAllOrdersRoute::SpreadHttp | CancelAllOrdersRoute::MassCancelHttp => {
                self.mass_cancel_instrument(cmd.instrument_id);
                Ok(())
            }
            CancelAllOrdersRoute::Batch => {
                let cancels: Vec<_> = {
                    let cache = self.core.cache();
                    cache
                        .orders_open(None, Some(&cmd.instrument_id), None, None, cmd.order_side)
                        .iter()
                        .map(|order| CancelOrder {
                            trader_id: order.trader_id(),
                            client_id: cmd.client_id,
                            strategy_id: order.strategy_id(),
                            instrument_id: order.instrument_id(),
                            client_order_id: order.client_order_id(),
                            venue_order_id: order.venue_order_id(),
                            command_id: cmd.command_id,
                            ts_init: cmd.ts_init,
                            params: cmd.params.clone(),
                            correlation_id: cmd.correlation_id,
                            causation_id: cmd.causation_id,
                        })
                        .collect()
                };

                if cancels.is_empty() {
                    log::debug!("No open orders to cancel for {}", cmd.instrument_id);
                    return Ok(());
                }

                self.batch_cancel_orders(BatchCancelOrders {
                    trader_id: cmd.trader_id,
                    client_id: cmd.client_id,
                    strategy_id: cmd.strategy_id,
                    instrument_id: cmd.instrument_id,
                    cancels,
                    command_id: cmd.command_id,
                    ts_init: cmd.ts_init,
                    params: cmd.params,
                    correlation_id: cmd.correlation_id,
                    causation_id: cmd.causation_id,
                })
            }
        }
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        let cache = self.core.cache();

        let mut regular_payload = Vec::new();
        let mut algo_orders = Vec::new();
        let mut http_orders = Vec::new();

        for cancel in &cmd.cancels {
            let order_state = cache
                .order(&cancel.client_order_id)
                .map(|order| (order.order_type(), order.is_triggered()));

            let venue_binding = self
                .ws_dispatch_state
                .order_venue_binding(cancel.client_order_id);
            let authoritative_venue_order_id = venue_binding
                .map(|(venue_order_id, _)| venue_order_id)
                .or(cancel.venue_order_id);

            match self.order_command_route(
                cancel.instrument_id,
                order_state,
                venue_binding.is_some_and(|(_, has_bound_child)| has_bound_child),
            ) {
                OrderCommandRoute::RegularWs => {
                    self.ensure_order_identity(
                        cancel.client_order_id,
                        cancel.strategy_id,
                        cancel.instrument_id,
                    );
                    regular_payload.push((
                        cancel.instrument_id,
                        Some(cancel.client_order_id),
                        authoritative_venue_order_id,
                    ));
                }
                OrderCommandRoute::AlgoHttp => {
                    let mut cancel = cancel.clone();
                    cancel.venue_order_id = authoritative_venue_order_id;
                    algo_orders.push(cancel);
                }
                OrderCommandRoute::SpreadHttp => {
                    self.ensure_order_identity(
                        cancel.client_order_id,
                        cancel.strategy_id,
                        cancel.instrument_id,
                    );
                    http_orders.push((
                        cancel.client_order_id,
                        cancel.instrument_id,
                        cancel.strategy_id,
                        authoritative_venue_order_id,
                    ));
                }
            }
        }
        drop(cache);

        if !regular_payload.is_empty() {
            let ws_private = self.ws_private.clone();
            let state = Arc::clone(&self.ws_dispatch_state);
            let emitter = self.emitter.clone();
            let clock = self.clock;
            self.spawn_task("batch_cancel_orders", async move {
                const BATCH_SIZE: usize = 20;

                for (index, batch) in regular_payload.chunks(BATCH_SIZE).enumerate() {
                    if let Err(e) = ws_private.batch_cancel_orders(batch.to_vec()).await {
                        let failure = classify_okx_ws_failure(&e);

                        if matches!(e, OKXWsError::HandlerUnavailable(_)) {
                            for (_, client_order_id, venue_order_id) in
                                &regular_payload[index * BATCH_SIZE..]
                            {
                                let Some(client_order_id) = client_order_id else {
                                    continue;
                                };

                                emit_send_failed_cancel(
                                    &failure,
                                    &state,
                                    &emitter,
                                    clock,
                                    *client_order_id,
                                    *venue_order_id,
                                );
                            }
                        } else {
                            log_batch_cancel_failure(failure, batch.len());
                        }

                        return Err(anyhow::Error::new(e).context("batch cancel orders failed"));
                    }
                }
                Ok(())
            });
        }

        // OKX doesn't support algo cancel via private WebSocket, must use HTTP
        if !algo_orders.is_empty() {
            let items: Vec<_> = algo_orders
                .into_iter()
                .map(|cancel| {
                    let request = OKXCancelAlgoOrderRequest {
                        inst_id: cancel.instrument_id.symbol.to_string(),
                        inst_id_code: None,
                        algo_id: cancel.venue_order_id.map(|id| id.to_string()),
                        algo_cl_ord_id: if cancel.venue_order_id.is_none() {
                            Some(cancel.client_order_id.to_string())
                        } else {
                            None
                        },
                    };
                    let ctx = AlgoCancelContext {
                        client_order_id: cancel.client_order_id,
                        instrument_id: cancel.instrument_id,
                        strategy_id: cancel.strategy_id,
                        venue_order_id: cancel.venue_order_id,
                    };
                    (request, ctx)
                })
                .collect();
            self.dispatch_algo_cancels(items);
        }

        if !http_orders.is_empty() {
            let client = self.http_client.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;

            self.spawn_task("cancel_http_orders", async move {
                for (client_order_id, instrument_id, strategy_id, venue_order_id) in http_orders {
                    if let Err(e) = client
                        .cancel_order(instrument_id, Some(client_order_id), venue_order_id)
                        .await
                    {
                        emit_cancel_failure(
                            classify_okx_http_failure(&e),
                            Some((&emitter, clock)),
                            client_order_id,
                            instrument_id,
                            strategy_id,
                            venue_order_id,
                        );
                    }
                }

                Ok(())
            });
        }

        Ok(())
    }
}

const MAX_TRIGGERED_CHILD_RECOVERIES: usize = 100;

struct OrderReportSweep {
    fallback_parents: AHashMap<usize, OrderStatusReport>,
    reports: Vec<OrderStatusReport>,
    complete: bool,
    ambiguous_triggered_child_ids: AHashSet<VenueOrderId>,
    regular_by_venue_order_id: AHashMap<VenueOrderId, OrderStatusReport>,
}

impl OKXExecutionClient {
    fn order_position_side(
        &self,
        order: &impl Order,
        params: Option<&Params>,
    ) -> Option<PositionSide> {
        if self.position_mode != OKXPositionMode::LongShortMode
            || !matches!(
                okx_instrument_type_from_symbol(order.instrument_id().symbol.as_str()),
                OKXInstrumentType::Swap | OKXInstrumentType::Futures
            )
        {
            return None;
        }

        let is_closing = order.is_reduce_only()
            || (self.is_conditional_order(order.order_type())
                && get_param_as_string(params, "close_fraction").is_some());

        Some(match (order.order_side(), is_closing) {
            (OrderSide::Buy, false) | (OrderSide::Sell, true) => PositionSide::Long,
            (OrderSide::Sell, false) | (OrderSide::Buy, true) => PositionSide::Short,
        })
    }

    fn validate_trade_mode(
        &self,
        instrument_id: InstrumentId,
        position_side: Option<PositionSide>,
        trade_mode: OKXTradeMode,
    ) -> Result<(), OrderDeniedReason> {
        if position_side.is_some() {
            let expected = derive_trade_mode_for_instrument(
                instrument_id,
                self.config.margin_mode,
                self.config.use_spot_margin,
            );

            if trade_mode != expected {
                return Err(OrderDeniedReason::ValidationFailed {
                    detail: format!(
                        "OKX long/short mode requires configured trade mode {expected}, received {trade_mode}; mixed margin modes are not supported"
                    ),
                });
            }
        }

        Ok(())
    }

    fn add_flat_derivative_position_reports(
        &self,
        order_reports: &[OrderStatusReport],
        fill_reports: &[FillReport],
        position_reports: &mut Vec<PositionStatusReport>,
        ts_init: UnixNanos,
    ) {
        let mut instrument_ids: AHashSet<InstrumentId> = order_reports
            .iter()
            .map(|report| report.instrument_id)
            .chain(fill_reports.iter().map(|report| report.instrument_id))
            .collect();
        let instrument_types = self.instrument_types();
        let mut hedging_instrument_ids = AHashSet::new();
        let mut hedging_positions = AHashMap::new();

        if self.position_mode == OKXPositionMode::LongShortMode {
            for (instrument_id, position_id, precision) in order_reports
                .iter()
                .map(|report| {
                    (
                        report.instrument_id,
                        report.venue_position_id,
                        report.quantity.precision,
                    )
                })
                .chain(fill_reports.iter().map(|report| {
                    (
                        report.instrument_id,
                        report.venue_position_id,
                        report.last_qty.precision,
                    )
                }))
            {
                if let Some(position_id) = position_id {
                    hedging_positions.insert((instrument_id, position_id), precision);
                }
            }
        }

        {
            let cache = self.core.cache();
            for position in cache.positions_open(
                Some(&OKX_VENUE),
                None,
                None,
                Some(&self.core.account_id),
                None,
            ) {
                instrument_ids.insert(position.instrument_id);
                if cache.oms_type(&position.id) == Some(OmsType::Hedging) {
                    hedging_instrument_ids.insert(position.instrument_id);
                }

                if self.position_mode == OKXPositionMode::LongShortMode
                    && instrument_types.contains(&okx_instrument_type_from_symbol(
                        position.instrument_id.symbol.as_str(),
                    ))
                    && [OKXPositionSide::Long, OKXPositionSide::Short]
                        .iter()
                        .any(|side| {
                            venue_position_id(position.instrument_id, *side) == Some(position.id)
                        })
                {
                    hedging_positions.insert(
                        (position.instrument_id, position.id),
                        position.quantity.precision,
                    );
                }
            }
        }

        for ((instrument_id, position_id), precision) in hedging_positions {
            if !position_reports.iter().any(|report| {
                report.instrument_id == instrument_id
                    && report.venue_position_id == Some(position_id)
            }) {
                position_reports.push(PositionStatusReport::new(
                    self.core.account_id,
                    instrument_id,
                    PositionSide::Flat,
                    Quantity::zero(precision),
                    ts_init,
                    ts_init,
                    None,
                    Some(position_id),
                    None,
                ));
            }
        }

        instrument_ids.retain(|instrument_id| {
            if self.http_client.is_spread(*instrument_id)
                || hedging_instrument_ids.contains(instrument_id)
                || position_reports
                    .iter()
                    .any(|report| report.instrument_id == *instrument_id)
            {
                return false;
            }

            match okx_instrument_type_from_symbol(instrument_id.symbol.as_str()) {
                OKXInstrumentType::Swap | OKXInstrumentType::Futures => {
                    self.position_mode != OKXPositionMode::LongShortMode
                }
                OKXInstrumentType::Option => true,
                _ => false,
            }
        });

        for instrument_id in instrument_ids {
            // A successful OKX positions snapshot omits flat rows
            position_reports.push(PositionStatusReport::new(
                self.core.account_id,
                instrument_id,
                PositionSide::Flat,
                Quantity::zero(0),
                ts_init,
                ts_init,
                None,
                None,
                None,
            ));
        }
    }

    async fn recover_triggered_child_order_reports(
        &self,
        reports: &mut Vec<OrderStatusReport>,
        ambiguous_triggered_child_ids: &AHashSet<VenueOrderId>,
        regular_by_venue_order_id: &AHashMap<VenueOrderId, OrderStatusReport>,
    ) -> bool {
        let mut fallback_parents = AHashMap::new();
        let complete = self
            .report_client()
            .recover_triggered_child_order_reports(
                reports,
                ambiguous_triggered_child_ids,
                regular_by_venue_order_id,
                &mut fallback_parents,
            )
            .await;
        retain_external_regular_order_fallbacks(&self.core, reports, &fallback_parents);
        complete
    }
}

struct OKXReportClient {
    account_id: AccountId,
    http_client: OKXHttpClient,
    instrument_types: Vec<OKXInstrumentType>,
    load_spreads: bool,
    margin_mode: Option<OKXMarginMode>,
}

impl OKXReportClient {
    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
        order_state: Option<CachedQueryOrderState>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            anyhow::bail!("generate_order_status_report requires instrument_id");
        };

        if cmd.client_order_id.is_none() && cmd.venue_order_id.is_none() {
            anyhow::bail!(
                "generate_order_status_report requires client_order_id or venue_order_id"
            );
        }

        let cached_venue_order_id = order_state.and_then(|state| state.venue_order_id);

        let regular_venue_order_id = order_state.and_then(|state| {
            if OKX_CONDITIONAL_ORDER_TYPES.contains(&state.order_type) {
                state.venue_order_id.or(cmd.venue_order_id)
            } else {
                state.venue_order_id
            }
        });

        let selection_venue_order_id = cached_venue_order_id.or(cmd.venue_order_id);
        let route = query_order_route(
            instrument_id,
            order_state.map(|state| state.order_type),
            regular_venue_order_id.is_some(),
            self.http_client.is_spread(instrument_id),
        );

        let mut reports = Vec::with_capacity(1);
        let mut query_algo = matches!(
            route,
            QueryOrderRoute::Algo | QueryOrderRoute::RegularAndAlgo
        );
        let mut lookup_error = None;

        match route {
            QueryOrderRoute::Spread => {
                let targeted_venue_order_id =
                    cmd.venue_order_id.filter(|_| cmd.client_order_id.is_none());

                match self
                    .http_client
                    .request_spread_order_status_report(
                        self.account_id,
                        instrument_id,
                        cmd.client_order_id,
                        targeted_venue_order_id,
                    )
                    .await
                {
                    Ok(Some(report)) => reports.push(report),
                    Ok(None) => {}
                    Err(e) => lookup_error = Some(e),
                }
            }
            QueryOrderRoute::Regular | QueryOrderRoute::RegularThenAlgo => {
                let targeted_venue_order_id = regular_venue_order_id
                    .or(cmd.venue_order_id.filter(|_| cmd.client_order_id.is_none()));

                let result = if let Some(venue_order_id) = targeted_venue_order_id {
                    self.http_client
                        .request_order_status_report_by_venue_order_id(
                            self.account_id,
                            instrument_id,
                            venue_order_id,
                        )
                        .await
                } else if let Some(client_order_id) = cmd.client_order_id {
                    self.http_client
                        .request_order_status_report(
                            self.account_id,
                            instrument_id,
                            client_order_id,
                        )
                        .await
                } else {
                    anyhow::bail!(
                        "generate_order_status_report requires client_order_id or venue_order_id"
                    );
                };

                match result {
                    Ok(Some(report)) => reports.push(report),
                    Ok(None) => {
                        query_algo |= route == QueryOrderRoute::RegularThenAlgo;
                    }
                    Err(e) => {
                        lookup_error = Some(e);
                        query_algo |= route == QueryOrderRoute::RegularThenAlgo;
                    }
                }
            }
            QueryOrderRoute::Algo | QueryOrderRoute::RegularAndAlgo => {}
        }

        if query_algo {
            let (algo_id, algo_client_order_id) = match cmd.client_order_id {
                Some(client_order_id) => (None, Some(client_order_id)),
                None => (cmd.venue_order_id.map(|id| id.as_str().to_string()), None),
            };

            match self
                .http_client
                .request_algo_order_status_reports(
                    self.account_id,
                    None,
                    Some(instrument_id),
                    algo_id,
                    algo_client_order_id,
                    None,
                    Some(1),
                )
                .await
            {
                Ok(algo_reports) => merge_order_status_reports(&mut reports, algo_reports),
                Err(e) => {
                    if lookup_error.is_none() {
                        lookup_error = Some(e);
                    }
                }
            }
        }

        if reports.is_empty() {
            if let Some(e) = lookup_error {
                return Err(e);
            }

            return Ok(None);
        }

        if let Some(client_order_id) = cmd.client_order_id {
            Ok(select_query_order_report(
                reports,
                client_order_id,
                selection_venue_order_id,
            ))
        } else {
            Ok(Some(reports.remove(0)))
        }
    }

    fn instrument_types(instrument_types: &[OKXInstrumentType]) -> Vec<OKXInstrumentType> {
        if instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            instrument_types.to_vec()
        }
    }

    fn report_scope<'a>(
        &'a self,
        instrument_types: &'a [OKXInstrumentType],
    ) -> ReportInstrumentScope<'a> {
        ReportInstrumentScope {
            instrument_types,
            load_spreads: self.load_spreads,
            margin_mode: self.margin_mode,
        }
    }

    async fn collect_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
        require_complete_active_coverage: bool,
        max_history_pages: Option<u32>,
    ) -> anyhow::Result<OrderReportSweep> {
        let instrument_types = Self::instrument_types(&self.instrument_types);
        let routing_types = order_routing_instrument_types(&instrument_types);
        let scope = self.report_scope(&routing_types);
        let start = nanos_to_datetime(cmd.start);
        let end = nanos_to_datetime(cmd.end);
        let mut reports = Vec::new();
        let mut regular_by_venue_order_id = AHashMap::new();
        let mut ambiguous_triggered_child_ids = AHashSet::new();
        let mut complete = true;

        if let Some(instrument_id) = cmd.instrument_id {
            let sweep = self
                .http_client
                .request_order_status_reports_scoped(
                    self.account_id,
                    None,
                    Some(instrument_id),
                    start,
                    end,
                    cmd.open_only,
                    None,
                    Some(scope),
                    max_history_pages,
                )
                .await?;

            regular_by_venue_order_id.extend(
                sweep
                    .reports
                    .iter()
                    .map(|report| (report.venue_order_id, report.clone())),
            );

            reports.extend(sweep.reports);
            complete &= sweep.complete;

            if !self.http_client.is_spread(instrument_id)
                && supports_algo_orders(okx_instrument_type_from_symbol(
                    instrument_id.symbol.as_str(),
                ))
            {
                match self
                    .http_client
                    .request_algo_order_status_reports_sweep(
                        self.account_id,
                        None,
                        Some(instrument_id),
                        None,
                        None,
                        None,
                        None,
                        start,
                        end,
                        require_complete_active_coverage,
                        max_history_pages,
                    )
                    .await
                {
                    Ok(sweep) => {
                        merge_algo_order_status_reports(
                            &mut reports,
                            sweep,
                            &mut ambiguous_triggered_child_ids,
                            &mut complete,
                        );
                    }
                    Err(e)
                        if require_complete_active_coverage
                            && e.downcast_ref::<OKXPendingAlgoOrderReportsError>()
                                .is_some() =>
                    {
                        return Err(e);
                    }
                    Err(e) if is_instrument_cache_miss(&e) => return Err(e),
                    Err(e) => {
                        log::warn!(
                            "Failed to fetch algo order status reports for {instrument_id}: {e}"
                        );
                        complete = false;
                    }
                }
            }
        } else {
            for inst_type in &routing_types {
                let sweep = self
                    .http_client
                    .request_order_status_reports_scoped(
                        self.account_id,
                        Some(*inst_type),
                        None,
                        start,
                        end,
                        cmd.open_only,
                        None,
                        Some(scope),
                        max_history_pages,
                    )
                    .await?;

                regular_by_venue_order_id.extend(
                    sweep
                        .reports
                        .iter()
                        .map(|report| (report.venue_order_id, report.clone())),
                );

                reports.extend(sweep.reports);
                complete &= sweep.complete;

                if supports_algo_orders(*inst_type) {
                    match self
                        .http_client
                        .request_algo_order_status_reports_sweep(
                            self.account_id,
                            Some(*inst_type),
                            None,
                            None,
                            None,
                            None,
                            None,
                            start,
                            end,
                            require_complete_active_coverage,
                            max_history_pages,
                        )
                        .await
                    {
                        Ok(sweep) => {
                            merge_algo_order_status_reports(
                                &mut reports,
                                sweep,
                                &mut ambiguous_triggered_child_ids,
                                &mut complete,
                            );
                        }
                        Err(e)
                            if require_complete_active_coverage
                                && e.downcast_ref::<OKXPendingAlgoOrderReportsError>()
                                    .is_some() =>
                        {
                            return Err(e);
                        }
                        Err(e) if is_instrument_cache_miss(&e) => return Err(e),
                        Err(e) => {
                            log::warn!(
                                "Failed to fetch algo order status reports for {inst_type:?}: {e}"
                            );
                            complete = false;
                        }
                    }
                }
            }
        }

        if cmd.instrument_id.is_none() && self.load_spreads {
            match self
                .http_client
                .request_order_status_reports_scoped(
                    self.account_id,
                    None,
                    None,
                    start,
                    end,
                    cmd.open_only,
                    None,
                    Some(scope),
                    max_history_pages,
                )
                .await
            {
                Ok(sweep) => {
                    reports.extend(sweep.reports);
                    complete &= sweep.complete;
                }
                Err(e) if is_instrument_cache_miss(&e) => return Err(e),
                Err(e) => {
                    log::warn!("Failed to fetch spread order status reports: {e}");
                    complete = false;
                }
            }
        }

        let mut fallback_parents = AHashMap::new();

        if cmd.open_only {
            complete &= self
                .recover_triggered_child_order_reports(
                    &mut reports,
                    &ambiguous_triggered_child_ids,
                    &regular_by_venue_order_id,
                    &mut fallback_parents,
                )
                .await;
        }

        Ok(OrderReportSweep {
            fallback_parents,
            reports,
            complete,
            ambiguous_triggered_child_ids,
            regular_by_venue_order_id,
        })
    }

    async fn collect_fill_reports(
        &self,
        cmd: GenerateFillReports,
        history: FillHistory,
        max_history_pages: Option<u32>,
    ) -> anyhow::Result<(Vec<FillReport>, bool)> {
        let instrument_types = Self::instrument_types(&self.instrument_types);
        let routing_types = order_routing_instrument_types(&instrument_types);
        let scope = self.report_scope(&routing_types);
        let start_dt = nanos_to_datetime(cmd.start);
        let end_dt = nanos_to_datetime(cmd.end);
        let mut reports = Vec::new();
        let mut complete = true;

        if let Some(instrument_id) = cmd.instrument_id {
            let sweep = self
                .http_client
                .request_fill_reports_scoped(
                    self.account_id,
                    None,
                    Some(instrument_id),
                    start_dt,
                    end_dt,
                    None,
                    history,
                    Some(scope),
                    max_history_pages,
                )
                .await?;
            reports.extend(sweep.reports);
            complete &= sweep.complete;
        } else {
            for inst_type in &routing_types {
                let sweep = self
                    .http_client
                    .request_fill_reports_scoped(
                        self.account_id,
                        Some(*inst_type),
                        None,
                        start_dt,
                        end_dt,
                        None,
                        history,
                        Some(scope),
                        max_history_pages,
                    )
                    .await?;
                reports.extend(sweep.reports);
                complete &= sweep.complete;
            }

            if self.load_spreads {
                let sweep = self
                    .http_client
                    .request_fill_reports_scoped(
                        self.account_id,
                        None,
                        None,
                        start_dt,
                        end_dt,
                        None,
                        history,
                        Some(scope),
                        max_history_pages,
                    )
                    .await?;
                reports.extend(sweep.reports);
                complete &= sweep.complete;
            }
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok((reports, complete))
    }

    async fn collect_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<(Vec<PositionStatusReport>, bool)> {
        let instrument_types = Self::instrument_types(&self.instrument_types);
        let scope = self.report_scope(&instrument_types);
        let mut reports = Vec::new();
        let mut complete = true;

        if let Some(instrument_id) = cmd.instrument_id {
            if self.http_client.is_spread(instrument_id) {
                return Ok((reports, complete));
            }

            let inst_type = okx_instrument_type_from_symbol(instrument_id.symbol.as_str());
            if inst_type != OKXInstrumentType::Spot && inst_type != OKXInstrumentType::Margin {
                let sweep = self
                    .http_client
                    .request_position_status_reports_scoped(
                        self.account_id,
                        None,
                        Some(instrument_id),
                        Some(scope),
                    )
                    .await?;
                reports.extend(sweep.reports);
                complete &= sweep.complete;
            }
        } else {
            for inst_type in &instrument_types {
                if *inst_type == OKXInstrumentType::Spot || *inst_type == OKXInstrumentType::Margin
                {
                    continue;
                }

                let sweep = self
                    .http_client
                    .request_position_status_reports_scoped(
                        self.account_id,
                        Some(*inst_type),
                        None,
                        Some(scope),
                    )
                    .await?;
                reports.extend(sweep.reports);
                complete &= sweep.complete;
            }
        }

        let mut margin_reports = self
            .http_client
            .request_spot_margin_position_reports(self.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            margin_reports.retain(|report| report.instrument_id == instrument_id);
        }

        reports.append(&mut margin_reports);

        Ok((reports, complete))
    }

    async fn recover_triggered_child_order_reports(
        &self,
        reports: &mut Vec<OrderStatusReport>,
        ambiguous_triggered_child_ids: &AHashSet<VenueOrderId>,
        regular_by_venue_order_id: &AHashMap<VenueOrderId, OrderStatusReport>,
        fallback_parents: &mut AHashMap<usize, OrderStatusReport>,
    ) -> bool {
        let mut recovered = Vec::with_capacity(reports.len());
        let recovery_candidate_count = reports
            .iter()
            .filter(|report| {
                report.order_status == OrderStatus::Triggered
                    && !ambiguous_triggered_child_ids.contains(&report.venue_order_id)
            })
            .count();
        let mut complete = true;

        if recovery_candidate_count > MAX_TRIGGERED_CHILD_RECOVERIES {
            log::warn!(
                "Triggered child recovery hit {MAX_TRIGGERED_CHILD_RECOVERIES} request cap; omitting unresolved reports"
            );
        }

        let mut request_count = 0;

        for mut parent_report in reports.drain(..) {
            if parent_report.order_status != OrderStatus::Triggered {
                recovered.push(parent_report);
                continue;
            }

            if ambiguous_triggered_child_ids.contains(&parent_report.venue_order_id) {
                log::warn!(
                    "Omitting triggered algo order with ambiguous child identifiers: {} {}",
                    parent_report.instrument_id,
                    parent_report.venue_order_id,
                );
                self.push_external_regular_order_fallback(
                    &parent_report,
                    regular_by_venue_order_id,
                    &mut recovered,
                    fallback_parents,
                );
                complete = false;
                continue;
            }

            if request_count >= MAX_TRIGGERED_CHILD_RECOVERIES {
                self.push_external_regular_order_fallback(
                    &parent_report,
                    regular_by_venue_order_id,
                    &mut recovered,
                    fallback_parents,
                );
                complete = false;
                continue;
            }
            request_count += 1;

            match self
                .http_client
                .request_order_status_report_by_venue_order_id(
                    self.account_id,
                    parent_report.instrument_id,
                    parent_report.venue_order_id,
                )
                .await
            {
                Ok(Some(mut child_report)) => {
                    if child_report.order_status == OrderStatus::Accepted {
                        parent_report.quantity = child_report.quantity;
                        parent_report.price = child_report.price.or(parent_report.price);
                        parent_report.reduce_only |= child_report.reduce_only;
                        parent_report.ts_last = child_report.ts_last;
                        recovered.push(parent_report);
                    } else {
                        child_report.client_order_id = parent_report.client_order_id;
                        recovered.push(child_report);
                    }
                }
                Ok(None) => {
                    log::warn!(
                        "Triggered child order {} {} was not found",
                        parent_report.instrument_id,
                        parent_report.venue_order_id,
                    );
                    self.push_external_regular_order_fallback(
                        &parent_report,
                        regular_by_venue_order_id,
                        &mut recovered,
                        fallback_parents,
                    );
                    complete = false;
                }
                Err(e) => {
                    log::warn!(
                        "Failed to recover triggered child order {} {}: {e}",
                        parent_report.instrument_id,
                        parent_report.venue_order_id,
                    );
                    self.push_external_regular_order_fallback(
                        &parent_report,
                        regular_by_venue_order_id,
                        &mut recovered,
                        fallback_parents,
                    );
                    complete = false;
                }
            }
        }

        *reports = recovered;
        complete
    }

    fn push_external_regular_order_fallback(
        &self,
        parent_report: &OrderStatusReport,
        regular_by_venue_order_id: &AHashMap<VenueOrderId, OrderStatusReport>,
        recovered: &mut Vec<OrderStatusReport>,
        fallback_parents: &mut AHashMap<usize, OrderStatusReport>,
    ) {
        if let Some(regular_report) = regular_by_venue_order_id.get(&parent_report.venue_order_id) {
            fallback_parents.insert(recovered.len(), parent_report.clone());
            recovered.push(regular_report.clone());
        }
    }
}

fn finish_order_report_sweep(
    core: &ExecutionClientCore,
    mut sweep: OrderReportSweep,
    command: &GenerateOrderStatusReports,
) -> OrderReportSweep {
    retain_external_regular_order_fallbacks(core, &mut sweep.reports, &sweep.fallback_parents);
    sweep.fallback_parents.clear();
    retain_order_status_reports(&mut sweep.reports, command);
    sweep
}

fn retain_external_regular_order_fallbacks(
    core: &ExecutionClientCore,
    reports: &mut Vec<OrderStatusReport>,
    fallback_parents: &AHashMap<usize, OrderStatusReport>,
) {
    if fallback_parents.is_empty() {
        return;
    }

    let cache = core.cache();
    let mut index = 0;
    reports.retain(|_| {
        let parent = fallback_parents.get(&index);
        index += 1;
        parent.is_none_or(|parent| {
            let cached_by_client = parent
                .client_order_id
                .is_some_and(|id| cache.order_exists(&id));
            let cached_by_venue = cache
                .client_order_id(&parent.venue_order_id)
                .is_some_and(|id| cache.order_exists(id));
            !cached_by_client && !cached_by_venue
        })
    });
}

fn validate_submit_position_id(
    instrument_id: InstrumentId,
    position_side: Option<PositionSide>,
    position_id: Option<PositionId>,
) -> Result<(), OrderDeniedReason> {
    if position_id.is_none() {
        return Ok(());
    }

    let expected = position_side.and_then(|side| venue_position_id(instrument_id, side.into()));
    if let (Some(position_id), Some(expected)) = (position_id, expected)
        && position_id != expected
    {
        return Err(OrderDeniedReason::InvalidPositionId {
            position_id,
            detail: format!("expected OKX venue position ID {expected}"),
        });
    }

    Ok(())
}

fn validate_order(
    order: &impl Order,
    trade_mode: OKXTradeMode,
    submission: OrderSubmission,
    is_spread: bool,
) -> Result<(), OrderDeniedReason> {
    if let Err(detail) = validate_okx_client_order_id(order.client_order_id().as_str()) {
        return Err(OrderDeniedReason::InvalidClientOrderId { detail });
    }

    if is_spread && order.is_reduce_only() {
        return Err(OrderDeniedReason::UnsupportedReduceOnly);
    }

    okx_algo_target_currency(
        okx_instrument_type_from_symbol(order.instrument_id().symbol.as_str()),
        trade_mode,
        order.order_side(),
        order.order_type(),
        order.is_quote_quantity(),
    )
    .map_err(|detail| OrderDeniedReason::ValidationFailed { detail })?;

    validate_algo_trigger_type(
        okx_instrument_type_from_symbol(order.instrument_id().symbol.as_str()),
        trade_mode,
        order.order_type(),
        order.trigger_type(),
    )
    .map_err(|detail| OrderDeniedReason::ValidationFailed { detail })?;

    if order.is_reduce_only()
        && okx_reduce_only_wire_value(
            okx_instrument_type_from_symbol(order.instrument_id().symbol.as_str()),
            trade_mode,
            order.order_side(),
            None,
            Some(true),
        )
        .is_err()
    {
        return Err(OrderDeniedReason::UnsupportedReduceOnly);
    }

    if matches!(submission, OrderSubmission::List) {
        if OKX_CONDITIONAL_ORDER_TYPES.contains(&order.order_type()) {
            return Err(OrderDeniedReason::UnsupportedOrderList {
                detail: format!(
                    "conditional order {} is not supported",
                    order.client_order_id()
                ),
            });
        }

        if order.time_in_force() != TimeInForce::Gtc {
            return Err(OrderDeniedReason::UnsupportedOrderList {
                detail: format!(
                    "order {} has unsupported time in force {}",
                    order.client_order_id(),
                    order.time_in_force()
                ),
            });
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderCommandRoute {
    RegularWs,
    AlgoHttp,
    SpreadHttp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryOrderRoute {
    Regular,
    Algo,
    RegularThenAlgo,
    RegularAndAlgo,
    Spread,
}

#[derive(Debug, Clone, Copy)]
struct CachedQueryOrderState {
    order_type: OrderType,
    venue_order_id: Option<VenueOrderId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CancelAllOrdersRoute {
    Batch,
    MassCancelHttp,
    SpreadHttp,
}

#[derive(Clone, Copy)]
enum OrderSubmission {
    Single,
    List,
}

fn emit_submit_failure(
    failure: CommandFailure,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
) {
    match failure {
        CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
            emitter.emit_order_rejected_event(
                strategy_id,
                instrument_id,
                client_order_id,
                &reason,
                clock.get_time_ns(),
                false,
            );
        }
        CommandFailure::Ambiguous(reason) => {
            log::warn!(
                "Ambiguous submit failure for {client_order_id}, awaiting reconciliation: {reason}"
            );
        }
    }
}

fn emit_modify_failure(
    failure: CommandFailure,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
) {
    match failure {
        CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
            emitter.emit_order_modify_rejected_event(
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                &reason,
                clock.get_time_ns(),
            );
        }
        CommandFailure::Ambiguous(reason) => {
            log::warn!(
                "Ambiguous modify failure for {client_order_id}, awaiting reconciliation: {reason}"
            );
        }
    }
}

fn algo_amend_request(
    cmd: &ModifyOrder,
    algo_type: Option<OKXAlgoOrderType>,
    context: Option<OrderContext>,
) -> Result<OKXAmendAlgoOrderRequest, &'static str> {
    let algo_type = algo_type.ok_or("Cannot determine OKX algo order terms for modification")?;
    let context = context.ok_or("Cannot determine OKX algo order terms for modification")?;
    let algo_id = cmd
        .venue_order_id
        .ok_or("No algo ID available for pending conditional order")?;
    let trigger_price = cmd.trigger_price.map(|price| price.to_string());
    let order_price = cmd.price.map(|price| price.to_string());

    let trigger_px_type = cmd.trigger_price.map(|_| {
        context
            .trigger_type
            .map_or(OKXTriggerType::Last, Into::into)
            .to_string()
    });

    let mut request = OKXAmendAlgoOrderRequest {
        inst_id: cmd.instrument_id.symbol.to_string(),
        algo_id: algo_id.to_string(),
        algo_cl_ord_id: None,
        new_sz: cmd.quantity.map(|quantity| quantity.to_string()),
        new_trigger_px: None,
        new_trigger_px_type: None,
        new_tp_trigger_px: None,
        new_tp_ord_px: None,
        new_tp_trigger_px_type: None,
        new_sl_trigger_px: None,
        new_sl_ord_px: None,
        new_sl_trigger_px_type: None,
        new_order_px: None,
        new_callback_ratio: None,
        new_callback_spread: None,
        new_active_px: None,
    };

    match (algo_type, context.identity.order_type) {
        (OKXAlgoOrderType::Trigger, _) => {
            request.new_trigger_px = trigger_price;
            request.new_order_px = order_price;
            request.new_trigger_px_type = trigger_px_type;
        }
        (OKXAlgoOrderType::Conditional, OrderType::StopMarket | OrderType::StopLimit) => {
            request.new_sl_trigger_px = trigger_price;
            request.new_sl_ord_px = order_price;
            request.new_sl_trigger_px_type = trigger_px_type;
        }
        (OKXAlgoOrderType::Conditional, OrderType::MarketIfTouched | OrderType::LimitIfTouched) => {
            request.new_tp_trigger_px = trigger_price;
            request.new_tp_ord_px = order_price;
            request.new_tp_trigger_px_type = trigger_px_type;
        }
        _ => return Err("OKX does not support modification of this algo order type"),
    }

    Ok(request)
}

fn emit_cancel_failure(
    failure: CommandFailure,
    emit_venue: Option<(&ExecutionEventEmitter, &'static AtomicTime)>,
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
    strategy_id: StrategyId,
    venue_order_id: Option<VenueOrderId>,
) {
    match failure {
        CommandFailure::VenueRejected(reason) => {
            if let Some((emitter, clock)) = emit_venue {
                emitter.emit_order_cancel_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &reason,
                    clock.get_time_ns(),
                );
            } else {
                log::warn!(
                    "Ambiguous cancel failure for {client_order_id}, awaiting reconciliation: {reason}"
                );
            }
        }
        CommandFailure::NotSent(reason) => {
            log::warn!("Cancel command failed local validation for {client_order_id}: {reason}");
        }
        CommandFailure::Ambiguous(reason) => {
            log::warn!(
                "Ambiguous cancel failure for {client_order_id}, awaiting reconciliation: {reason}"
            );
        }
    }
}

fn log_batch_cancel_failure(failure: CommandFailure, order_count: usize) {
    match failure {
        CommandFailure::NotSent(reason) => {
            log::warn!(
                "Batch cancel command failed local validation for {order_count} orders: {reason}"
            );
        }
        CommandFailure::Ambiguous(reason) | CommandFailure::VenueRejected(reason) => {
            log::warn!(
                "Ambiguous batch cancel failure for {order_count} orders, awaiting reconciliation: {reason}"
            );
        }
    }
}

fn log_mass_cancel_failure(failure: CommandFailure, instrument_id: InstrumentId) {
    match failure {
        CommandFailure::NotSent(reason) => {
            log::warn!("Mass cancel command failed local validation for {instrument_id}: {reason}");
        }
        CommandFailure::Ambiguous(reason) | CommandFailure::VenueRejected(reason) => {
            log::warn!(
                "Ambiguous mass cancel failure for {instrument_id}, awaiting reconciliation: {reason}"
            );
        }
    }
}

fn log_algo_batch_cancel_failure(failure: CommandFailure, contexts: &[AlgoCancelContext]) {
    match failure {
        CommandFailure::NotSent(reason) => {
            for ctx in contexts {
                log::warn!(
                    "Algo batch cancel command failed local validation for {}: {reason}",
                    ctx.client_order_id
                );
            }
        }
        CommandFailure::Ambiguous(reason) | CommandFailure::VenueRejected(reason) => {
            for ctx in contexts {
                log::warn!(
                    "Ambiguous algo batch cancel failure for {}, awaiting reconciliation: {reason}",
                    ctx.client_order_id
                );
            }
        }
    }
}

fn get_param_as_string(params: Option<&Params>, key: &str) -> Option<String> {
    params.and_then(|p| {
        p.get(key).and_then(|v| {
            v.as_str()
                .map(ToString::to_string)
                .or_else(|| v.as_f64().map(|n| n.to_string()))
        })
    })
}

fn get_param_as_bool(params: Option<&Params>, key: &str) -> Option<bool> {
    params.and_then(|params| params.get_bool(key))
}

fn supports_algo_orders(instrument_type: OKXInstrumentType) -> bool {
    !matches!(
        instrument_type,
        OKXInstrumentType::Option | OKXInstrumentType::Events
    )
}

fn order_routing_instrument_types(
    instrument_types: &[OKXInstrumentType],
) -> Vec<OKXInstrumentType> {
    let mut routing_types = instrument_types.to_vec();

    // OKX reports cross-margin spot orders as SPOT on order channels and report endpoints
    if routing_types.contains(&OKXInstrumentType::Margin)
        && !routing_types.contains(&OKXInstrumentType::Spot)
        && !routing_types.contains(&OKXInstrumentType::Any)
    {
        routing_types.push(OKXInstrumentType::Spot);
    }

    routing_types
}

fn query_order_route(
    instrument_id: InstrumentId,
    order_type: Option<OrderType>,
    has_cached_venue_order_id: bool,
    is_spread: bool,
) -> QueryOrderRoute {
    if is_spread {
        return QueryOrderRoute::Spread;
    }

    let supports_algo = supports_algo_orders(okx_instrument_type_from_symbol(
        instrument_id.symbol.as_str(),
    ));

    match order_type {
        Some(order_type)
            if supports_algo
                && OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type)
                && has_cached_venue_order_id =>
        {
            QueryOrderRoute::RegularAndAlgo
        }
        Some(order_type) if supports_algo && OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type) => {
            QueryOrderRoute::Algo
        }
        None if supports_algo => QueryOrderRoute::RegularThenAlgo,
        _ => QueryOrderRoute::Regular,
    }
}

fn is_instrument_cache_miss(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("missing from cache"))
}

fn merge_algo_order_status_reports(
    reports: &mut Vec<OrderStatusReport>,
    sweep: AlgoOrderReportSweep,
    ambiguous_triggered_child_ids: &mut AHashSet<VenueOrderId>,
    complete: &mut bool,
) {
    let AlgoOrderReportSweep {
        reports: incoming,
        complete: sweep_complete,
        ambiguous_triggered_child_ids: ambiguous,
    } = sweep;

    *complete &= sweep_complete && ambiguous.is_empty();
    ambiguous_triggered_child_ids.extend(ambiguous);
    merge_order_status_reports(reports, incoming);
}

fn merge_order_status_reports(
    reports: &mut Vec<OrderStatusReport>,
    incoming: Vec<OrderStatusReport>,
) {
    let mut indexes: AHashMap<VenueOrderId, usize> = reports
        .iter()
        .enumerate()
        .map(|(index, report)| (report.venue_order_id, index))
        .collect();

    for report in incoming {
        if let Some(index) = indexes.get(&report.venue_order_id).copied() {
            if is_order_status_report_more_advanced(&report, &reports[index]) {
                reports[index] = report;
            }
        } else {
            indexes.insert(report.venue_order_id, reports.len());
            reports.push(report);
        }
    }
}

// Picks the report that best answers the query. Tiered so a strong signal
// wins over a weak one regardless of ordering in the merged result set:
//   1. Exact `client_order_id` match.
//   2. Exact `venue_order_id` match (rare: only when the cached vid is
//      still valid; OKX rotates venue_order_id once an algo order triggers).
//
// Triggered-algo recovery queries the regular child by `ord_id` and the algo
// parent by `algo_cl_ord_id`. `linked_order_ids` is deliberately not consulted here
// because it is also populated with attached TP/SL child ids on the parent
// order, which would otherwise let a query for a child match the parent report.
fn select_query_order_report(
    reports: Vec<OrderStatusReport>,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
) -> Option<OrderStatusReport> {
    let mut by_client_id: Option<OrderStatusReport> = None;
    let mut by_vid: Option<OrderStatusReport> = None;

    for report in reports {
        if report.client_order_id == Some(client_order_id) {
            if by_client_id
                .as_ref()
                .is_none_or(|current| is_order_status_report_more_advanced(&report, current))
            {
                by_client_id = Some(report);
            }

            continue;
        }

        if venue_order_id
            .as_ref()
            .is_some_and(|vid| report.venue_order_id.as_str() == vid.as_str())
            && by_vid
                .as_ref()
                .is_none_or(|current| is_order_status_report_more_advanced(&report, current))
        {
            by_vid = Some(report);
        }
    }

    by_client_id.or(by_vid)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, sync::Mutex, time::Duration};

    use axum::{
        Json, Router,
        extract::ws::{Message, WebSocketUpgrade},
        routing::{get, post},
    };
    use nautilus_common::{
        cache::Cache,
        messages::{ExecutionEvent, ExecutionReport as CommonExecutionReport},
        testing::wait_until_async,
    };
    use nautilus_core::{AtomicMap, UUID4};
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, TriggerType},
        events::{OrderEventAny, OrderInitialized},
        identifiers::{OrderListId, Symbol, TradeId, TraderId},
        instruments::{
            CryptoFuture, Instrument,
            stubs::{crypto_future_btcusdt, currency_pair_ethusdt},
        },
        orders::{OrderList, OrderTestBuilder, stubs::TestOrderEventStubs},
        position::Position,
        types::{Money, Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::Value;
    use ustr::Ustr;

    use super::*;
    use crate::common::{
        consts::OKX_CLIENT_ID, enums::OKXEnvironment, models::OKXInstrument,
        parse::parse_instrument_any,
    };

    struct DropSignal(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[derive(Clone, Copy)]
    enum ExecutionTaskBoundary {
        Reset,
        Dispose,
        RepeatedStop,
    }

    #[rstest]
    #[case(OrderType::Market, QueryOrderRoute::Regular)]
    #[case(OrderType::Limit, QueryOrderRoute::Regular)]
    #[case(OrderType::StopMarket, QueryOrderRoute::Algo)]
    #[case(OrderType::TrailingStopMarket, QueryOrderRoute::Algo)]
    fn test_query_order_route_for_known_order_type(
        #[case] order_type: OrderType,
        #[case] expected: QueryOrderRoute,
    ) {
        assert_eq!(
            query_order_route(
                InstrumentId::from("BTC-USDT.OKX"),
                Some(order_type),
                false,
                false
            ),
            expected
        );
    }

    #[rstest]
    fn test_query_order_route_for_conditional_order_with_cached_venue_id() {
        assert_eq!(
            query_order_route(
                InstrumentId::from("BTC-USDT.OKX"),
                Some(OrderType::StopMarket),
                true,
                false,
            ),
            QueryOrderRoute::RegularAndAlgo,
        );
    }

    #[rstest]
    fn test_query_order_route_for_unknown_order_type() {
        assert_eq!(
            query_order_route(InstrumentId::from("BTC-USDT.OKX"), None, false, false),
            QueryOrderRoute::RegularThenAlgo,
        );
    }

    #[rstest]
    fn test_query_order_route_for_spread() {
        assert_eq!(
            query_order_route(
                InstrumentId::from("ETH-USD-SWAP_ETH-USD-231229.OKX"),
                None,
                false,
                true,
            ),
            QueryOrderRoute::Spread,
        );
    }

    #[rstest]
    #[case("BTC-USD_UM_XPERP-310404.OKX")]
    #[case("BTC-USD_UM-261030.OKX")]
    fn test_usd_margined_futures_use_regular_routes(
        mut crypto_future_btcusdt: CryptoFuture,
        #[case] instrument_id: &str,
        #[values(false, true)] cached: bool,
        #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    ) {
        let client = build_test_exec_client();
        let instrument_id = InstrumentId::from(instrument_id);
        crypto_future_btcusdt.id = instrument_id;
        crypto_future_btcusdt.raw_symbol = instrument_id.symbol;

        if cached {
            client
                .http_client
                .cache_instrument(InstrumentAny::CryptoFuture(crypto_future_btcusdt));
        }

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("OREDUCEFUTURE"))
            .side(side)
            .price(Price::from("65000.1"))
            .quantity(Quantity::from("3"))
            .reduce_only(true)
            .build();
        let is_spread = client.http_client.is_spread(instrument_id);

        assert_eq!(
            client
                .submit_order_route(instrument_id, OrderType::Limit)
                .unwrap(),
            OrderCommandRoute::RegularWs
        );
        assert_eq!(
            client
                .submit_order_route(instrument_id, OrderType::StopMarket)
                .unwrap(),
            OrderCommandRoute::AlgoHttp
        );
        assert_eq!(
            client.order_command_route(instrument_id, Some((OrderType::Limit, None)), false),
            OrderCommandRoute::RegularWs
        );
        assert_eq!(
            client.cancel_all_orders_route(instrument_id, None),
            CancelAllOrdersRoute::Batch
        );
        assert_eq!(
            query_order_route(instrument_id, Some(OrderType::Limit), false, is_spread),
            QueryOrderRoute::Regular
        );
        assert_eq!(
            validate_order(
                &order,
                OKXTradeMode::Cross,
                OrderSubmission::Single,
                is_spread
            ),
            Ok(())
        );
    }

    #[rstest]
    fn test_validate_order_allows_conditional_single_submission() {
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(InstrumentId::from("ETH-USDT-SWAP.OKX"))
            .client_order_id(ClientOrderId::from("OCONDITIONALSINGLE"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1"))
            .trigger_price(Price::from("1000.00"))
            .build();

        assert_eq!(
            validate_order(&order, OKXTradeMode::Cross, OrderSubmission::Single, false),
            Ok(())
        );
    }

    #[rstest]
    fn test_validate_order_denies_conditional_order_in_list() {
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(InstrumentId::from("ETH-USDT-SWAP.OKX"))
            .client_order_id(ClientOrderId::from("OCONDITIONALLIST"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("2"))
            .trigger_price(Price::from("900.00"))
            .build();

        assert_eq!(
            validate_order(&order, OKXTradeMode::Cross, OrderSubmission::List, false),
            Err(OrderDeniedReason::UnsupportedOrderList {
                detail: "conditional order OCONDITIONALLIST is not supported".to_string(),
            })
        );
    }

    #[rstest]
    fn test_validate_order_denies_non_gtc_order_in_list() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("ETH-USDT-SWAP.OKX"))
            .client_order_id(ClientOrderId::from("OIOCLIST"))
            .side(OrderSide::Buy)
            .price(Price::from("2000.00"))
            .quantity(Quantity::from("3"))
            .time_in_force(TimeInForce::Ioc)
            .build();

        assert_eq!(
            validate_order(&order, OKXTradeMode::Cross, OrderSubmission::List, false),
            Err(OrderDeniedReason::UnsupportedOrderList {
                detail: "order OIOCLIST has unsupported time in force IOC".to_string(),
            })
        );
    }

    #[rstest]
    #[case::cash("BTC-USDT.OKX", OKXTradeMode::Cash)]
    #[case::option("BTC-USD-241217-92000-C.OKX", OKXTradeMode::Cross)]
    #[case::event("BTC-ABOVE-DAILY-260224-1600-65000.OKX", OKXTradeMode::Cross)]
    fn test_validate_order_denies_unsupported_reduce_only(
        #[case] instrument_id: &str,
        #[case] trade_mode: OKXTradeMode,
    ) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from(instrument_id))
            .client_order_id(ClientOrderId::from("OREDUCEUNSUPPORTED"))
            .side(OrderSide::Sell)
            .price(Price::from("2000.00"))
            .quantity(Quantity::from("1"))
            .reduce_only(true)
            .build();

        assert_eq!(
            validate_order(&order, trade_mode, OrderSubmission::Single, false),
            Err(OrderDeniedReason::UnsupportedReduceOnly)
        );
    }

    fn build_config(
        margin_mode: Option<OKXMarginMode>,
        use_spot_margin: bool,
    ) -> OKXExecutionClientConfig {
        OKXExecutionClientConfig {
            margin_mode,
            use_spot_margin,
            ..OKXExecutionClientConfig::default()
        }
    }

    #[rstest]
    #[case::spot(OKXInstrumentType::Spot, true)]
    #[case::margin(OKXInstrumentType::Margin, true)]
    #[case::swap(OKXInstrumentType::Swap, true)]
    #[case::futures(OKXInstrumentType::Futures, true)]
    #[case::option(OKXInstrumentType::Option, false)]
    #[case::events(OKXInstrumentType::Events, false)]
    fn test_supports_algo_orders(
        #[case] instrument_type: OKXInstrumentType,
        #[case] expected: bool,
    ) {
        assert_eq!(supports_algo_orders(instrument_type), expected);
    }

    #[rstest]
    #[case::margin(
        vec![OKXInstrumentType::Margin],
        vec![OKXInstrumentType::Margin, OKXInstrumentType::Spot]
    )]
    #[case::spot_margin(
        vec![OKXInstrumentType::Spot, OKXInstrumentType::Margin],
        vec![OKXInstrumentType::Spot, OKXInstrumentType::Margin]
    )]
    #[case::any_margin(
        vec![OKXInstrumentType::Any, OKXInstrumentType::Margin],
        vec![OKXInstrumentType::Any, OKXInstrumentType::Margin]
    )]
    #[case::swap(
        vec![OKXInstrumentType::Swap],
        vec![OKXInstrumentType::Swap]
    )]
    fn test_order_routing_instrument_types(
        #[case] instrument_types: Vec<OKXInstrumentType>,
        #[case] expected: Vec<OKXInstrumentType>,
    ) {
        assert_eq!(order_routing_instrument_types(&instrument_types), expected);
    }

    #[rstest]
    #[case::cash_no_spot_margin(AccountType::Cash, None, false, OKXTradeMode::Cash)]
    #[case::cash_spot_margin_cross(
        AccountType::Cash,
        Some(OKXMarginMode::Cross),
        true,
        OKXTradeMode::Cross
    )]
    #[case::cash_spot_margin_isolated(
        AccountType::Cash,
        Some(OKXMarginMode::Isolated),
        true,
        OKXTradeMode::Isolated
    )]
    #[case::cash_spot_margin_none(AccountType::Cash, None, true, OKXTradeMode::Isolated)]
    #[case::margin_cross(
        AccountType::Margin,
        Some(OKXMarginMode::Cross),
        false,
        OKXTradeMode::Cross
    )]
    #[case::margin_isolated(
        AccountType::Margin,
        Some(OKXMarginMode::Isolated),
        false,
        OKXTradeMode::Isolated
    )]
    #[case::margin_none(AccountType::Margin, None, false, OKXTradeMode::Isolated)]
    fn test_derive_default_trade_mode(
        #[case] account_type: AccountType,
        #[case] margin_mode: Option<OKXMarginMode>,
        #[case] use_spot_margin: bool,
        #[case] expected: OKXTradeMode,
    ) {
        let config = build_config(margin_mode, use_spot_margin);

        let result = OKXExecutionClient::derive_default_trade_mode(account_type, &config);

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::spot_no_margin("BTC-USDT", None, false, OKXTradeMode::Cash)]
    #[case::spot_cross_margin("BTC-USDT", Some(OKXMarginMode::Cross), true, OKXTradeMode::Cross)]
    #[case::spot_isolated_margin(
        "ETH-USDT",
        Some(OKXMarginMode::Isolated),
        true,
        OKXTradeMode::Isolated
    )]
    #[case::spot_margin_no_mode("BTC-USDT", None, true, OKXTradeMode::Isolated)]
    #[case::swap_cross(
        "BTC-USDT-SWAP",
        Some(OKXMarginMode::Cross),
        false,
        OKXTradeMode::Cross
    )]
    #[case::swap_isolated(
        "BTC-USDT-SWAP",
        Some(OKXMarginMode::Isolated),
        false,
        OKXTradeMode::Isolated
    )]
    #[case::swap_no_mode("ETH-USDT-SWAP", None, false, OKXTradeMode::Isolated)]
    #[case::futures_cross(
        "BTC-USDT-250328",
        Some(OKXMarginMode::Cross),
        false,
        OKXTradeMode::Cross
    )]
    #[case::futures_isolated("BTC-USDT-250328", None, false, OKXTradeMode::Isolated)]
    #[case::option_cross(
        "BTC-USD-250328-50000-C",
        Some(OKXMarginMode::Cross),
        false,
        OKXTradeMode::Cross
    )]
    #[case::option_isolated("BTC-USD-250328-50000-C", None, false, OKXTradeMode::Isolated)]
    fn test_derive_trade_mode_for_instrument(
        #[case] symbol: &str,
        #[case] margin_mode: Option<OKXMarginMode>,
        #[case] use_spot_margin: bool,
        #[case] expected: OKXTradeMode,
    ) {
        let instrument_id = InstrumentId::from(format!("{symbol}.OKX").as_str());

        let result = derive_trade_mode_for_instrument(instrument_id, margin_mode, use_spot_margin);

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::override_to_cross("cross", OKXTradeMode::Cross)]
    #[case::override_to_cash("cash", OKXTradeMode::Cash)]
    #[case::override_to_isolated("isolated", OKXTradeMode::Isolated)]
    #[case::override_to_spot_isolated("spot_isolated", OKXTradeMode::SpotIsolated)]
    #[case::case_insensitive("CROSS", OKXTradeMode::Cross)]
    fn test_td_mode_param_override(#[case] td_mode_value: &str, #[case] expected: OKXTradeMode) {
        let params = Params::new(
            [(
                "td_mode".to_string(),
                Value::String(td_mode_value.to_string()),
            )]
            .into(),
        );

        let result = get_param_as_string(Some(&params), "td_mode")
            .and_then(|s| s.parse::<OKXTradeMode>().ok());

        assert_eq!(result, Some(expected));
    }

    #[rstest]
    fn test_td_mode_param_invalid_falls_through() {
        let params =
            Params::new([("td_mode".to_string(), Value::String("invalid".to_string()))].into());

        let result = get_param_as_string(Some(&params), "td_mode")
            .and_then(|s| s.parse::<OKXTradeMode>().ok());

        assert_eq!(result, None);
    }

    #[rstest]
    fn test_td_mode_param_absent_falls_through() {
        let result = get_param_as_string(None, "td_mode");

        assert_eq!(result, None);
    }

    #[rstest]
    fn test_close_fraction_present_sets_reduce_only_true() {
        let params =
            Params::new([("close_fraction".to_string(), Value::String("1".to_string()))].into());
        let params = Some(params);

        let close_fraction = get_param_as_string(params.as_ref(), "close_fraction");
        let is_reduce_only = false;
        let reduce_only = if close_fraction.is_some() {
            Some(true)
        } else {
            Some(is_reduce_only)
        };

        assert_eq!(close_fraction, Some("1".to_string()));
        assert_eq!(reduce_only, Some(true));
    }

    #[rstest]
    fn test_close_fraction_absent_preserves_reduce_only() {
        let params: Option<Params> = None;

        let close_fraction = get_param_as_string(params.as_ref(), "close_fraction");
        let is_reduce_only = false;
        let reduce_only = if close_fraction.is_some() {
            Some(true)
        } else {
            Some(is_reduce_only)
        };

        assert_eq!(close_fraction, None);
        assert_eq!(reduce_only, Some(false));
    }

    #[rstest]
    fn test_close_fraction_absent_with_reduce_only_true() {
        let params: Option<Params> = None;

        let close_fraction = get_param_as_string(params.as_ref(), "close_fraction");
        let is_reduce_only = true;
        let reduce_only = if close_fraction.is_some() {
            Some(true)
        } else {
            Some(is_reduce_only)
        };

        assert_eq!(close_fraction, None);
        assert_eq!(reduce_only, Some(true));
    }

    fn make_query_order_report(cid: Option<&str>, vid: &str) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("OKX-001"),
            InstrumentId::from("BTC-USDT.OKX"),
            cid.map(ClientOrderId::from),
            VenueOrderId::from(vid),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::new(1.0, 0),
            Quantity::zero(0),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        )
    }

    fn with_linked(mut report: OrderStatusReport, linked: &[&str]) -> OrderStatusReport {
        report.linked_order_ids = Some(linked.iter().map(|s| ClientOrderId::from(*s)).collect());
        report
    }

    #[rstest]
    fn test_merge_order_status_reports_keeps_filled_regular_child() {
        let mut filled = make_query_order_report(Some("O-PARENT"), "V-CHILD");
        filled.order_status = OrderStatus::Filled;
        filled.filled_qty = Quantity::new(1.0, 0);
        filled.ts_last = UnixNanos::from(100);

        let mut triggered = make_query_order_report(Some("O-PARENT"), "V-CHILD");
        triggered.order_status = OrderStatus::Triggered;
        triggered.filled_qty = Quantity::new(1.0, 0);
        triggered.ts_last = UnixNanos::from(200);

        let mut reports = vec![filled];
        merge_order_status_reports(&mut reports, vec![triggered]);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].order_status, OrderStatus::Filled);
        assert_eq!(
            reports[0].client_order_id,
            Some(ClientOrderId::from("O-PARENT"))
        );
        assert_eq!(reports[0].venue_order_id, VenueOrderId::from("V-CHILD"));
    }

    #[rstest]
    fn test_merge_order_status_reports_replaces_pending_parent_with_triggered_child() {
        let mut accepted = make_query_order_report(Some("O-PARENT"), "V-CHILD");
        accepted.ts_last = UnixNanos::from(100);

        let mut triggered = make_query_order_report(Some("O-PARENT"), "V-CHILD");
        triggered.order_status = OrderStatus::Triggered;
        triggered.ts_last = UnixNanos::from(200);

        let mut reports = vec![accepted];
        merge_order_status_reports(&mut reports, vec![triggered]);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].order_status, OrderStatus::Triggered);
        assert_eq!(reports[0].ts_last, UnixNanos::from(200));
    }

    #[rstest]
    fn test_select_query_order_report_matches_client_order_id() {
        let reports = vec![make_query_order_report(Some("O-001"), "V-1")];
        let selected = select_query_order_report(reports, ClientOrderId::from("O-001"), None);
        assert_eq!(
            selected.and_then(|r| r.client_order_id),
            Some(ClientOrderId::from("O-001"))
        );
    }

    #[rstest]
    fn test_select_query_order_report_client_wins_over_venue_mismatch() {
        let reports = vec![make_query_order_report(Some("O-001"), "V-1")];
        let selected = select_query_order_report(
            reports,
            ClientOrderId::from("O-001"),
            Some(VenueOrderId::from("V-OTHER")),
        );
        assert_eq!(
            selected.and_then(|r| r.client_order_id),
            Some(ClientOrderId::from("O-001"))
        );
    }

    #[rstest]
    #[case(
        OrderStatus::Accepted,
        Quantity::zero(0),
        OrderStatus::Triggered,
        Quantity::zero(0),
        OrderStatus::Triggered
    )]
    #[case(
        OrderStatus::Triggered,
        Quantity::zero(0),
        OrderStatus::PartiallyFilled,
        Quantity::new(0.5, 1),
        OrderStatus::PartiallyFilled
    )]
    #[case(
        OrderStatus::PartiallyFilled,
        Quantity::new(0.5, 1),
        OrderStatus::Filled,
        Quantity::new(1.0, 0),
        OrderStatus::Filled
    )]
    #[case(
        OrderStatus::Triggered,
        Quantity::zero(0),
        OrderStatus::Canceled,
        Quantity::zero(0),
        OrderStatus::Canceled
    )]
    #[case(
        OrderStatus::Triggered,
        Quantity::zero(0),
        OrderStatus::Rejected,
        Quantity::zero(0),
        OrderStatus::Rejected
    )]
    fn test_select_query_order_report_chooses_most_advanced_client_match_regardless_of_order(
        #[case] first_status: OrderStatus,
        #[case] first_filled_qty: Quantity,
        #[case] second_status: OrderStatus,
        #[case] second_filled_qty: Quantity,
        #[case] expected_status: OrderStatus,
    ) {
        let mut first = make_query_order_report(Some("O-001"), "V-PARENT");
        first.order_status = first_status;
        first.filled_qty = first_filled_qty;
        let mut second = make_query_order_report(Some("O-001"), "V-CHILD");
        second.order_status = second_status;
        second.filled_qty = second_filled_qty;

        for reports in [vec![first.clone(), second.clone()], vec![second, first]] {
            let selected = select_query_order_report(
                reports,
                ClientOrderId::from("O-001"),
                Some(VenueOrderId::from("V-PARENT")),
            )
            .unwrap();

            assert_eq!(selected.order_status, expected_status);
        }
    }

    #[rstest]
    fn test_select_query_order_report_falls_back_to_venue_order_id() {
        // Algo child trigger: report's client_order_id is the child, the
        // command still carries the pre-trigger venue_order_id.
        let reports = vec![make_query_order_report(Some("O-CHILD"), "V-1")];
        let selected = select_query_order_report(
            reports,
            ClientOrderId::from("O-PARENT"),
            Some(VenueOrderId::from("V-1")),
        );
        assert_eq!(
            selected.map(|r| r.venue_order_id.as_str().to_string()),
            Some("V-1".to_string()),
        );
    }

    #[rstest]
    fn test_select_query_order_report_rejects_when_nothing_matches() {
        let reports = vec![make_query_order_report(Some("O-OTHER"), "V-OTHER")];
        let selected = select_query_order_report(
            reports,
            ClientOrderId::from("O-001"),
            Some(VenueOrderId::from("V-1")),
        );
        assert!(selected.is_none());
    }

    #[rstest]
    fn test_select_query_order_report_rejects_when_client_differs_and_no_vid_provided() {
        let reports = vec![make_query_order_report(Some("O-OTHER"), "V-1")];
        let selected = select_query_order_report(reports, ClientOrderId::from("O-001"), None);
        assert!(selected.is_none());
    }

    #[rstest]
    fn test_select_query_order_report_ignores_linked_order_ids_for_parent_with_attached_tp() {
        // Parent order has attached TP/SL children listed in its
        // linked_order_ids. A query for one of those children must NOT
        // resolve to the parent's report via the linked_order_ids.
        let child_cid = "O-CHILD-TP";
        let reports = vec![with_linked(
            make_query_order_report(Some("O-PARENT"), "V-PARENT"),
            &[child_cid, "O-CHILD-SL"],
        )];
        let selected = select_query_order_report(reports, ClientOrderId::from(child_cid), None);
        assert!(selected.is_none());
    }

    #[rstest]
    fn test_select_query_order_report_client_match_wins_over_vid_match_elsewhere() {
        // Ordering invariant: the client_order_id match beats a vid match on
        // a different report regardless of which appears first in the list.
        let reports = vec![
            make_query_order_report(Some("O-OTHER"), "V-1"),
            make_query_order_report(Some("O-001"), "V-2"),
        ];
        let selected = select_query_order_report(
            reports,
            ClientOrderId::from("O-001"),
            Some(VenueOrderId::from("V-1")),
        );
        assert_eq!(
            selected.and_then(|r| r.client_order_id),
            Some(ClientOrderId::from("O-001")),
        );
    }

    fn build_test_exec_client() -> OKXExecutionClient {
        build_test_exec_client_with_cache().0
    }

    #[rstest]
    #[case::stop_limit(OrderType::StopLimit, OKXAlgoOrderType::Trigger)]
    #[case::stop_market(OrderType::StopMarket, OKXAlgoOrderType::Trigger)]
    #[case::touched_limit(OrderType::LimitIfTouched, OKXAlgoOrderType::Trigger)]
    #[case::touched_market(OrderType::MarketIfTouched, OKXAlgoOrderType::Trigger)]
    #[case::sl_limit(OrderType::StopLimit, OKXAlgoOrderType::Conditional)]
    #[case::sl_market(OrderType::StopMarket, OKXAlgoOrderType::Conditional)]
    #[case::tp_limit(OrderType::LimitIfTouched, OKXAlgoOrderType::Conditional)]
    #[case::tp_market(OrderType::MarketIfTouched, OKXAlgoOrderType::Conditional)]
    #[tokio::test]
    async fn test_modify_algo_order_http(
        #[case] order_type: OrderType,
        #[case] algo_type: OKXAlgoOrderType,
        #[values("all", "trigger", "quantity")] amendment: &str,
        #[values(("0", "0"), ("0", "51000"), ("0", "50004"), ("1", "51000"), ("1", "50004"))]
        response: (&'static str, &'static str),
    ) {
        let (code, s_code) = response;
        let (client, cache) = build_test_exec_client_with_cache();
        let (submit_tx, mut submit_rx) = tokio::sync::mpsc::unbounded_channel();
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();

        let router = Router::new()
            .route(
                "/api/v5/trade/order-algo",
                post(move |Json(request): Json<Value>| {
                    let tx = submit_tx.clone();
                    async move {
                        tx.send(request).unwrap();
                        Json(serde_json::json!({"code": "0", "msg": "", "data": [{
                            "algoId": "7003", "sCode": "0", "sMsg": ""
                        }]}))
                    }
                }),
            )
            .route(
                "/api/v5/trade/amend-algos",
                post(move |Json(request): Json<Value>| {
                    let tx = request_tx.clone();
                    async move {
                        tx.send(request).unwrap();
                        Json(serde_json::json!({"code": code, "msg": "All operations failed", "data": [{
                            "algoId": "7003", "sCode": s_code, "sMsg": "test message"
                        }]}))
                    }
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut client = client;
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);
        client.http_client = OKXHttpClient::with_credentials(
            Some("test_key".into()),
            Some("test_secret".into()),
            Some("test_pass".into()),
            Some(base_url),
            5,
            0,
            1,
            1,
            crate::common::enums::OKXEnvironment::Live,
            None,
        )
        .unwrap();
        let client_order_id = ClientOrderId::from("OMODIFYALGO");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let order = OrderTestBuilder::new(order_type)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.03"))
            .price(Price::from("54900.0"))
            .trigger_price(Price::from("55000.0"))
            .trigger_type(TriggerType::MarkPrice)
            .build();
        let accepted = TestOrderEventStubs::accepted(
            &order,
            client.core.account_id,
            VenueOrderId::from("7003"),
        );
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        let mut submit = SubmitOrder::from_order(
            &order,
            TraderId::from("TESTER-001"),
            Some(*OKX_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );

        if algo_type == OKXAlgoOrderType::Conditional {
            let params = Params::new([("close_fraction".into(), Value::String("1".into()))].into());
            submit.params = Some(params);
        }

        client.submit_order(submit).unwrap();
        let submitted = tokio::time::timeout(Duration::from_secs(5), submit_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            submitted["ordType"],
            serde_json::to_value(algo_type).unwrap()
        );
        wait_until_async(
            || async { client.pending_tasks.is_empty() },
            Duration::from_secs(5),
        )
        .await;
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Submitted(_))
        ));
        assert!(event_rx.try_recv().is_err());
        cache.borrow_mut().update_order(&accepted).unwrap();
        let limit = matches!(order_type, OrderType::StopLimit | OrderType::LimitIfTouched);
        client
            .modify_order(ModifyOrder::new(
                TraderId::from("TESTER-001"),
                Some(*OKX_CLIENT_ID),
                order.strategy_id(),
                instrument_id,
                client_order_id,
                Some(VenueOrderId::from("stale-parent")),
                (amendment != "trigger").then(|| Quantity::from("0.04")),
                (limit && amendment == "all").then(|| Price::from("53900.0")),
                (amendment != "quantity").then(|| Price::from("54000.0")),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        let request = tokio::time::timeout(Duration::from_secs(5), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let mut expected = serde_json::json!({"instId": "BTC-USDT-SWAP", "algoId": "7003"});
        if amendment != "trigger" {
            expected["newSz"] = Value::String("0.04".into());
        }

        let (trigger_field, type_field, price_field) = if algo_type == OKXAlgoOrderType::Trigger {
            ("newTriggerPx", "newTriggerPxType", "newOrdPx")
        } else if matches!(order_type, OrderType::StopLimit | OrderType::StopMarket) {
            ("newSlTriggerPx", "newSlTriggerPxType", "newSlOrdPx")
        } else {
            ("newTpTriggerPx", "newTpTriggerPxType", "newTpOrdPx")
        };

        if amendment != "quantity" {
            expected[trigger_field] = Value::String("54000.0".into());
            expected[type_field] = Value::String("mark".into());
        }

        if limit && amendment == "all" {
            expected[price_field] = Value::String("53900.0".into());
        }

        assert_eq!(request, expected);
        wait_until_async(
            || async { client.pending_tasks.is_empty() },
            Duration::from_secs(5),
        )
        .await;

        if s_code == "51000" {
            let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) =
                event_rx.try_recv().unwrap()
            else {
                panic!("expected OrderModifyRejected");
            };

            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(rejected.venue_order_id, Some(VenueOrderId::from("7003")));
            assert_eq!(rejected.instrument_id, instrument_id);
            assert_eq!(rejected.strategy_id, order.strategy_id());
            assert_eq!(rejected.reason.as_str(), "OKX error 51000: test message");
        }

        assert!(event_rx.try_recv().is_err());
        server.abort();
    }

    #[rstest]
    #[case::trigger(
        OrderType::StopLimit,
        OKXAlgoOrderType::Trigger,
        "newOrdPx",
        "newTriggerPx",
        "newTriggerPxType"
    )]
    #[case::sl(
        OrderType::StopLimit,
        OKXAlgoOrderType::Conditional,
        "newSlOrdPx",
        "newSlTriggerPx",
        "newSlTriggerPxType"
    )]
    #[case::tp(
        OrderType::LimitIfTouched,
        OKXAlgoOrderType::Conditional,
        "newTpOrdPx",
        "newTpTriggerPx",
        "newTpTriggerPxType"
    )]
    fn test_algo_amend_request_preserves_unconfirmed_terms(
        #[case] order_type: OrderType,
        #[case] algo_type: OKXAlgoOrderType,
        #[case] price_field: &str,
        #[case] trigger_field: &str,
        #[case] type_field: &str,
    ) {
        let order = OrderTestBuilder::new(order_type)
            .instrument_id(InstrumentId::from("BTC-USDT-SWAP.OKX"))
            .client_order_id(ClientOrderId::from("OUNCONFIRMED"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.03"))
            .price(Price::from("54900.0"))
            .trigger_price(Price::from("55000.0"))
            .trigger_type(TriggerType::MarkPrice)
            .build();
        let context = OrderContext::from(&order);

        let mut command = ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*OKX_CLIENT_ID),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            Some(VenueOrderId::from("7003")),
            None,
            Some(Price::from("53900.0")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        let first = algo_amend_request(&command, Some(algo_type), Some(context)).unwrap();
        command.price = None;
        command.trigger_price = Some(Price::from("54000.0"));
        let second = algo_amend_request(&command, Some(algo_type), Some(context)).unwrap();
        command.trigger_price = None;
        command.quantity = Some(Quantity::from("0.04"));
        let third = algo_amend_request(&command, Some(algo_type), Some(context)).unwrap();
        let mut first_expected = serde_json::json!({"instId": "BTC-USDT-SWAP", "algoId": "7003"});
        first_expected[price_field] = Value::String("53900.0".into());
        let mut second_expected = serde_json::json!({"instId": "BTC-USDT-SWAP", "algoId": "7003"});
        second_expected[trigger_field] = Value::String("54000.0".into());
        second_expected[type_field] = Value::String("mark".into());
        assert_eq!(serde_json::to_value(first).unwrap(), first_expected);
        assert_eq!(serde_json::to_value(second).unwrap(), second_expected);
        assert_eq!(
            serde_json::to_value(third).unwrap(),
            serde_json::json!({
                "instId": "BTC-USDT-SWAP", "algoId": "7003", "newSz": "0.04"
            })
        );
    }

    #[rstest]
    #[case::unknown(
        None,
        true,
        true,
        "Cannot determine OKX algo order terms for modification"
    )]
    #[case::missing_context(
        Some(OKXAlgoOrderType::Trigger),
        false,
        true,
        "Cannot determine OKX algo order terms for modification"
    )]
    #[case::missing_id(
        Some(OKXAlgoOrderType::Trigger),
        true,
        false,
        "No algo ID available for pending conditional order"
    )]
    #[case::trailing(
        Some(OKXAlgoOrderType::MoveOrderStop),
        true,
        true,
        "OKX does not support modification of this algo order type"
    )]
    fn test_modify_algo_order_rejects_locally(
        #[case] algo_type: Option<OKXAlgoOrderType>,
        #[case] has_context: bool,
        #[case] has_id: bool,
        #[case] reason: &str,
    ) {
        let mut client = build_test_exec_client();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);
        let client_order_id = ClientOrderId::from("OMODIFYINVALID");
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.03"))
            .trigger_price(Price::from("55000.0"))
            .build();

        if has_context {
            client
                .ws_dispatch_state
                .track_order_context(OrderContext::from(&order));
        }

        if let Some(order_type) = algo_type {
            client.ws_dispatch_state.algo_order_states.insert(
                client_order_id,
                AlgoOrderState {
                    order_type,
                    ts_last: UnixNanos::default(),
                },
            );
        }

        let venue_order_id = has_id.then(|| VenueOrderId::from("7004"));
        client.modify_algo_order(&ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*OKX_CLIENT_ID),
            order.strategy_id(),
            instrument_id,
            client_order_id,
            venue_order_id,
            None,
            None,
            Some(Price::from("54000.0")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ));

        let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) =
            event_rx.try_recv().unwrap()
        else {
            panic!("expected OrderModifyRejected");
        };

        assert_eq!(rejected.client_order_id, client_order_id);
        assert_eq!(rejected.venue_order_id, venue_order_id);
        assert_eq!(rejected.instrument_id, instrument_id);
        assert_eq!(rejected.strategy_id, order.strategy_id());
        assert_eq!(rejected.reason.as_str(), reason);
        assert!(event_rx.try_recv().is_err());
        assert!(client.pending_tasks.is_empty());
    }

    #[rstest]
    #[case::client_id(true)]
    #[case::venue_id(false)]
    fn test_report_fallback_uses_current_cache(#[case] match_client_id: bool) {
        let (client, cache) = build_test_exec_client_with_cache();
        let parent = make_query_order_report(Some("O-PARENT"), "V-PARENT");
        let regular = make_query_order_report(Some("O-REGULAR"), "V-REGULAR");
        let fallback = make_query_order_report(None, "V-CHILD");
        let external = make_query_order_report(None, "V-EXTERNAL");
        let fallback_parents = AHashMap::from_iter([
            (1, parent.clone()),
            (
                2,
                make_query_order_report(Some("O-EXTERNAL"), "V-EXTERNAL-PARENT"),
            ),
        ]);
        let mut reports = vec![regular.clone(), fallback.clone(), external.clone()];
        retain_external_regular_order_fallbacks(&client.core, &mut reports, &fallback_parents);
        assert_eq!(reports, [regular.clone(), fallback, external.clone()]);

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(parent.instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("1"))
            .client_order_id(ClientOrderId::from(if match_client_id {
                "O-PARENT"
            } else {
                "O-OTHER"
            }))
            .build();

        let accepted =
            TestOrderEventStubs::accepted(&order, client.core.account_id, parent.venue_order_id);
        cache
            .borrow_mut()
            .add_order(order, None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        cache.borrow_mut().update_order(&accepted).unwrap();
        retain_external_regular_order_fallbacks(&client.core, &mut reports, &fallback_parents);

        assert_eq!(reports, [regular, external]);
    }

    fn build_test_exec_client_with_cache() -> (OKXExecutionClient, Rc<RefCell<Cache>>) {
        let config = OKXExecutionClientConfig {
            api_key: Some("test_key".into()),
            api_secret: Some("test_secret".into()),
            api_passphrase: Some("test_pass".into()),
            ..OKXExecutionClientConfig::default()
        };

        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("OKX-TEST"),
            *OKX_VENUE,
            OmsType::Hedging,
            config.account_id,
            AccountType::Cash,
            None,
            Rc::clone(&cache),
        );

        (
            OKXExecutionClient::new(core, config).expect("failed to build test client"),
            cache,
        )
    }

    #[rstest]
    #[case::reset(ExecutionTaskBoundary::Reset)]
    #[case::dispose(ExecutionTaskBoundary::Dispose)]
    #[case::repeated_stop(ExecutionTaskBoundary::RepeatedStop)]
    #[tokio::test]
    async fn lifecycle_boundary_terminates_owned_execution_task(
        #[case] boundary: ExecutionTaskBoundary,
    ) {
        let mut client = build_test_exec_client();

        if matches!(boundary, ExecutionTaskBoundary::RepeatedStop) {
            client.stop().expect("initial stop");
        }

        let (drop_tx, drop_rx) = tokio::sync::oneshot::channel();
        let signal = DropSignal(Some(drop_tx));
        client.spawn_task("pending lifecycle task", async move {
            let _signal = signal;
            std::future::pending::<anyhow::Result<()>>().await
        });

        match boundary {
            ExecutionTaskBoundary::Reset => client.reset().expect("reset"),
            ExecutionTaskBoundary::Dispose => client.dispose().expect("dispose"),
            ExecutionTaskBoundary::RepeatedStop => client.stop().expect("repeated stop"),
        }

        tokio::time::timeout(Duration::from_secs(1), drop_rx)
            .await
            .expect("lifecycle boundary must drop the owned task")
            .expect("drop signal");
        terminate_tasks(&client.pending_tasks, "test execution client")
            .await
            .expect("execution task terminated");
        assert!(client.pending_tasks.is_empty());
    }

    #[rstest]
    #[case("BTC-USDT.OKX", false, None, CancelAllOrdersRoute::Batch)]
    #[case("BTC-USDT.OKX", true, None, CancelAllOrdersRoute::MassCancelHttp)]
    #[case(
        "BTC-USDT.OKX",
        true,
        Some(OrderSide::Buy),
        CancelAllOrdersRoute::Batch
    )]
    #[case(
        "BTC-USDT.OKX",
        true,
        Some(OrderSide::Sell),
        CancelAllOrdersRoute::Batch
    )]
    #[case(
        "ETH-USD-SWAP_ETH-USD-231229.OKX",
        false,
        None,
        CancelAllOrdersRoute::SpreadHttp
    )]
    #[case(
        "ETH-USD-SWAP_ETH-USD-231229.OKX",
        false,
        Some(OrderSide::Buy),
        CancelAllOrdersRoute::Batch
    )]
    #[case(
        "ETH-USD-SWAP_ETH-USD-231229.OKX",
        true,
        Some(OrderSide::Sell),
        CancelAllOrdersRoute::Batch
    )]
    fn test_cancel_all_orders_route(
        #[case] instrument_id: &str,
        #[case] use_mm_mass_cancel: bool,
        #[case] order_side: Option<OrderSide>,
        #[case] expected: CancelAllOrdersRoute,
    ) {
        let mut client = build_test_exec_client();
        client.config.use_mm_mass_cancel = use_mm_mass_cancel;

        assert_eq!(
            client.cancel_all_orders_route(InstrumentId::from(instrument_id), order_side),
            expected
        );
    }

    #[rstest]
    #[case::buy(Some(OrderSide::Buy), false, &["OCANCEL0", "OCANCEL2"])]
    #[case::sell(Some(OrderSide::Sell), false, &["OCANCEL1", "OCANCEL3"])]
    #[case::both(None, false, &["OCANCEL0", "OCANCEL1", "OCANCEL2", "OCANCEL3"])]
    #[case::mass_buy(Some(OrderSide::Buy), true, &["OCANCEL0", "OCANCEL2"])]
    #[case::mass_sell(Some(OrderSide::Sell), true, &["OCANCEL1", "OCANCEL3"])]
    #[tokio::test]
    async fn test_cancel_all_orders_filters_cached_orders(
        #[case] order_side: Option<OrderSide>,
        #[case] use_mm_mass_cancel: bool,
        #[case] expected_ids: &[&str],
        #[values(1, 10, 11)] copies: usize,
    ) {
        let (mut client, cache) = build_test_exec_client_with_cache();
        client.config.use_mm_mass_cancel = use_mm_mass_cancel;
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();

        let router = Router::new().route(
            "/ws",
            get(move |ws: WebSocketUpgrade| {
                let tx = request_tx.clone();
                async move {
                    ws.on_upgrade(move |mut socket| async move {
                        while let Some(Ok(Message::Text(text))) = socket.next().await {
                            let request: Value = serde_json::from_str(&text).unwrap();
                            if request["op"] == "login" {
                                let response =
                                    serde_json::json!({"event": "login", "code": "0", "msg": "", "connId": "test-conn"});
                                socket
                                    .send(Message::Text(response.to_string().into()))
                                    .await
                                    .unwrap();
                            } else {
                                tx.send(request).unwrap();
                            }
                        }
                    })
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}/ws", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        client.ws_private = OKXWebSocketClient::with_credentials(
            Some(url),
            Some("test_key".into()),
            Some("test_secret".into()),
            Some("test_pass".into()),
            Some(client.core.account_id),
            None,
            None,
            client.config.transport_backend,
            None,
        )
        .unwrap();
        client
            .ws_private
            .cache_inst_id_code(ustr::Ustr::from("BTC-USDT"), 10459);
        client.ws_private.connect().await.unwrap();
        client.ws_private.wait_until_active(5.0).await.unwrap();
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");

        let expected_ids: Vec<_> = (0..copies)
            .flat_map(|copy| {
                expected_ids.iter().map(move |id| {
                    let index: usize = id.strip_prefix("OCANCEL").unwrap().parse().unwrap();
                    format!("OCANCEL{}", copy * 8 + index)
                })
            })
            .collect();

        let mut expected: Vec<_> = expected_ids
            .iter()
            .map(|id| ClientOrderId::from(id.as_str()))
            .collect();

        for copy in 0..copies {
            for (index, (symbol, side, strategy, open)) in [
                ("BTC-USDT.OKX", OrderSide::Buy, "S-001", true),
                ("BTC-USDT.OKX", OrderSide::Sell, "S-001", true),
                ("BTC-USDT.OKX", OrderSide::Buy, "S-002", true),
                ("BTC-USDT.OKX", OrderSide::Sell, "S-002", true),
                ("ETH-USDT.OKX", OrderSide::Buy, "S-001", true),
                ("ETH-USDT.OKX", OrderSide::Sell, "S-001", true),
                ("BTC-USDT.OKX", OrderSide::Buy, "S-001", false),
                ("BTC-USDT.OKX", OrderSide::Sell, "S-001", false),
            ]
            .into_iter()
            .enumerate()
            {
                let index = copy * 8 + index;
                let client_order_id = ClientOrderId::from(format!("OCANCEL{index}"));
                let mut order = OrderTestBuilder::new(OrderType::Limit)
                    .instrument_id(InstrumentId::from(symbol))
                    .client_order_id(client_order_id)
                    .strategy_id(StrategyId::from(strategy))
                    .side(side)
                    .price(Price::from("100.00"))
                    .quantity(Quantity::from("1"))
                    .build();
                let accepted = TestOrderEventStubs::accepted(
                    &order,
                    client.core.account_id,
                    VenueOrderId::from(format!("{index}")),
                );
                cache
                    .borrow_mut()
                    .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
                    .unwrap();
                order = cache.borrow_mut().update_order(&accepted).unwrap();

                if !open {
                    let canceled = TestOrderEventStubs::canceled(
                        &order,
                        client.core.account_id,
                        order.venue_order_id(),
                    );
                    cache.borrow_mut().update_order(&canceled).unwrap();
                }
            }
        }

        client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*OKX_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                order_side,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        let mut actual: Vec<_> = client
            .ws_dispatch_state
            .order_identities
            .iter()
            .map(|entry| *entry.key())
            .collect();
        actual.sort();

        let expected_sizes: &[usize] = match (copies, order_side) {
            (1, Some(_)) => &[2],
            (1, None) => &[4],
            (10, Some(_)) => &[20],
            (10, None) => &[20, 20],
            (11, Some(_)) => &[20, 2],
            (11, None) => &[20, 20, 4],
            _ => unreachable!(),
        };

        let mut payload = Vec::new();
        let mut batch_sizes = Vec::new();

        for _ in expected_sizes {
            let request = tokio::time::timeout(Duration::from_secs(5), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(request["op"], "batch-cancel-orders");
            let args = request["args"].as_array().unwrap();
            batch_sizes.push(args.len());
            payload.extend(args.iter().cloned());
        }

        payload.sort_by_key(|item| item["clOrdId"].as_str().unwrap().to_string());

        let mut expected_payload: Vec<_> = expected_ids
            .iter()
            .map(|id| {
                serde_json::json!({
                    "instIdCode": 10459,
                    "clOrdId": id,
                    "ordId": id.strip_prefix("OCANCEL").unwrap(),
                })
            })
            .collect();

        expected.sort();
        expected_payload.sort_by_key(|item| item["clOrdId"].as_str().unwrap().to_string());
        client.ws_private.close().await.unwrap();
        server.abort();

        assert_eq!(batch_sizes, expected_sizes);
        assert_eq!(payload, expected_payload);
        assert_eq!(actual, expected);
        terminate_tasks(&client.pending_tasks, "test execution client")
            .await
            .unwrap();
    }

    #[rstest]
    #[case::algo_buy("BTC-USDT.OKX", OrderType::StopLimit, Some(OrderSide::Buy), &["0", "2"], false)]
    #[case::algo_sell("BTC-USDT.OKX", OrderType::StopLimit, Some(OrderSide::Sell), &["1", "3"], false)]
    #[case::algo_both("BTC-USDT.OKX", OrderType::StopLimit, None, &["0", "1", "2", "3"], false)]
    #[case::spread_buy("ETH-USD-SWAP_ETH-USD-231229.OKX", OrderType::Limit, Some(OrderSide::Buy), &["0", "2"], false)]
    #[case::spread_sell("ETH-USD-SWAP_ETH-USD-231229.OKX", OrderType::Limit, Some(OrderSide::Sell), &["1", "3"], false)]
    #[case::advance_buy("BTC-USDT.OKX", OrderType::TrailingStopMarket, Some(OrderSide::Buy), &["0", "2"], false)]
    #[case::advance_sell("BTC-USDT.OKX", OrderType::TrailingStopMarket, Some(OrderSide::Sell), &["1", "3"], false)]
    #[case::advance_both("BTC-USDT.OKX", OrderType::TrailingStopMarket, None, &["0", "1", "2", "3"], false)]
    #[case::mixed_buy("BTC-USDT.OKX", OrderType::StopLimit, Some(OrderSide::Buy), &["0", "2"], true)]
    #[case::mixed_sell("BTC-USDT.OKX", OrderType::StopLimit, Some(OrderSide::Sell), &["1", "3"], true)]
    #[case::mixed_both("BTC-USDT.OKX", OrderType::StopLimit, None, &["0", "1", "2", "3"], true)]
    #[tokio::test]
    async fn test_cancel_all_orders_filters_http_requests(
        #[case] symbol: &str,
        #[case] order_type: OrderType,
        #[case] order_side: Option<OrderSide>,
        #[case] expected_ids: &[&str],
        #[case] mixed_types: bool,
        #[values(1, 5, 6)] copies: usize,
    ) {
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();

        let router = Router::new().fallback(post(
            move |uri: axum::http::Uri, Json(request): Json<Value>| {
                let tx = request_tx.clone();
                async move {
                    let mut response = serde_json::from_str::<Value>(include_str!(
                        "../test_data/http_cancel_algo_order_response.json"
                    ))
                    .unwrap();

                    if let Some(ord_id) = request.get("ordId") {
                        response["data"][0]
                            .as_object_mut()
                            .unwrap()
                            .remove("algoId");
                        response["data"][0]["ordId"] = ord_id.clone();
                    } else {
                        let template = response["data"][0].clone();
                        response["data"] = request
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|item| {
                                let mut result = template.clone();
                                result["algoId"] = item["algoId"].clone();
                                result
                            })
                            .collect();
                    }

                    tx.send((uri.path().to_string(), request)).unwrap();
                    Json(response)
                }
            },
        ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, cache) = build_test_exec_client_with_cache();
        client.http_client = OKXHttpClient::with_credentials(
            Some("test_key".into()),
            Some("test_secret".into()),
            Some("test_pass".into()),
            Some(base_url),
            5,
            0,
            1,
            1,
            client.config.environment,
            None,
        )
        .unwrap();
        let instrument_id = InstrumentId::from(symbol);

        let mut expected_ids: Vec<_> = (0..copies)
            .flat_map(|copy| {
                expected_ids.iter().map(move |id| {
                    let index: usize = id.parse().unwrap();
                    (copy * 4 + index).to_string()
                })
            })
            .collect();

        for copy in 0..copies {
            for (index, side) in [
                OrderSide::Buy,
                OrderSide::Sell,
                OrderSide::Buy,
                OrderSide::Sell,
            ]
            .into_iter()
            .enumerate()
            {
                let index = copy * 4 + index;

                let cached_order_type = if mixed_types && index % 4 >= 2 {
                    OrderType::TrailingStopMarket
                } else {
                    order_type
                };

                let order = OrderTestBuilder::new(cached_order_type)
                    .instrument_id(instrument_id)
                    .client_order_id(ClientOrderId::from(format!("OHTTP{index}")))
                    .strategy_id(StrategyId::from(if index % 4 < 2 {
                        "S-001"
                    } else {
                        "S-002"
                    }))
                    .side(side)
                    .price(Price::from("100.00"))
                    .trigger_price(Price::from("101.00"))
                    .trailing_offset(rust_decimal::Decimal::ONE)
                    .trailing_offset_type(TrailingOffsetType::Price)
                    .quantity(Quantity::from("1"))
                    .build();

                let accepted = TestOrderEventStubs::accepted(
                    &order,
                    client.core.account_id,
                    VenueOrderId::from(format!("{index}")),
                );
                cache
                    .borrow_mut()
                    .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
                    .unwrap();
                cache.borrow_mut().update_order(&accepted).unwrap();
            }
        }

        client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*OKX_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                order_side,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_until_async(
            || async { client.pending_tasks.is_empty() },
            Duration::from_secs(5),
        )
        .await;

        let mut expected_sizes = if order_type == OrderType::Limit {
            vec![1; expected_ids.len()]
        } else if mixed_types {
            match (copies, order_side) {
                (1, Some(_)) => vec![1, 1],
                (1, None) => vec![2, 2],
                (5, Some(_)) => vec![5, 5],
                (5, None) => vec![10, 10],
                (6, Some(_)) => vec![6, 6],
                (6, None) => vec![10, 2, 10, 2],
                _ => unreachable!(),
            }
        } else {
            match (copies, order_side) {
                (1, Some(_)) => vec![2],
                (1, None) => vec![4],
                (5, Some(_)) => vec![10],
                (5, None) => vec![10, 10],
                (6, Some(_)) => vec![10, 2],
                (6, None) => vec![10, 10, 4],
                _ => unreachable!(),
            }
        };

        let mut actual = Vec::new();
        let mut batch_sizes = Vec::new();

        while let Ok((path, request)) = request_rx.try_recv() {
            if order_type == OrderType::Limit {
                assert_eq!(path, "/api/v5/sprd/cancel-order");
                batch_sizes.push(1);
                actual.push(request["ordId"].as_str().unwrap().to_string());
            } else {
                batch_sizes.push(request.as_array().unwrap().len());
                for item in request.as_array().unwrap() {
                    let index: usize = item["algoId"].as_str().unwrap().parse().unwrap();

                    let expected_path = if order_type == OrderType::TrailingStopMarket
                        || (mixed_types && index % 4 >= 2)
                    {
                        "/api/v5/trade/cancel-advance-algos"
                    } else {
                        "/api/v5/trade/cancel-algos"
                    };

                    assert_eq!(path, expected_path);
                    assert_eq!(item["instId"], "BTC-USDT");
                    actual.push(item["algoId"].as_str().unwrap().to_string());
                }
            }
        }

        server.abort();
        actual.sort();
        expected_ids.sort();
        batch_sizes.sort_unstable();
        expected_sizes.sort_unstable();

        assert_eq!(actual, expected_ids);
        assert_eq!(batch_sizes, expected_sizes);
    }

    #[rstest]
    #[case("BTC-USDT.OKX", false)]
    #[case("BTC-USDT.OKX", true)]
    #[case("ETH-USD-SWAP_ETH-USD-231229.OKX", false)]
    #[tokio::test]
    async fn test_cancel_all_orders_empty_cache_sends_nothing(
        #[case] instrument_id: &str,
        #[case] use_mm_mass_cancel: bool,
    ) {
        let mut client = build_test_exec_client();
        client.config.use_mm_mass_cancel = use_mm_mass_cancel;
        client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*OKX_CLIENT_ID),
                StrategyId::from("S-001"),
                InstrumentId::from(instrument_id),
                Some(OrderSide::Buy),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();

        assert!(client.pending_tasks.is_empty());
        assert!(client.ws_dispatch_state.order_identities.is_empty());
    }

    #[rstest]
    fn test_order_command_route_uses_bound_child_before_engine_trigger_state_updates() {
        let client = build_test_exec_client();
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let pending_trigger = Some((OrderType::StopLimit, Some(false)));

        assert_eq!(
            client.order_command_route(instrument_id, pending_trigger, false),
            OrderCommandRoute::AlgoHttp
        );
        assert_eq!(
            client.order_command_route(instrument_id, pending_trigger, true),
            OrderCommandRoute::RegularWs
        );
        assert_eq!(
            client.order_command_route(
                instrument_id,
                Some((OrderType::StopLimit, Some(true))),
                false
            ),
            OrderCommandRoute::RegularWs
        );
    }

    #[rstest]
    #[case(
        OrderType::StopMarket,
        OrderSide::Sell,
        true,
        "OKX Spot STOP_MARKET algo orders do not support quote quantity"
    )]
    #[case(
        OrderType::MarketIfTouched,
        OrderSide::Sell,
        true,
        "OKX Spot MARKET_IF_TOUCHED algo orders do not support quote quantity"
    )]
    #[case(
        OrderType::TrailingStopMarket,
        OrderSide::Sell,
        true,
        "OKX Spot TRAILING_STOP_MARKET algo orders do not support quote quantity"
    )]
    #[case(
        OrderType::StopLimit,
        OrderSide::Sell,
        true,
        "OKX Spot STOP_LIMIT algo orders do not support quote quantity"
    )]
    #[case(
        OrderType::LimitIfTouched,
        OrderSide::Sell,
        true,
        "OKX Spot LIMIT_IF_TOUCHED algo orders do not support quote quantity"
    )]
    #[case(
        OrderType::StopLimit,
        OrderSide::Buy,
        true,
        "OKX Spot STOP_LIMIT algo orders do not support quote quantity"
    )]
    #[case(
        OrderType::LimitIfTouched,
        OrderSide::Buy,
        true,
        "OKX Spot LIMIT_IF_TOUCHED algo orders do not support quote quantity"
    )]
    #[case(
        OrderType::TrailingStopMarket,
        OrderSide::Buy,
        false,
        "OKX Spot TRAILING_STOP_MARKET algo buys require quote quantity"
    )]
    #[case(
        OrderType::StopMarket,
        OrderSide::Buy,
        false,
        "OKX Spot STOP_MARKET algo orders do not support BID_ASK triggers"
    )]
    fn test_submit_order_denies_unsupported_spot_algo_terms_before_submission(
        #[case] order_type: OrderType,
        #[case] side: OrderSide,
        #[case] quote_quantity: bool,
        #[case] expected_reason: &str,
        #[values("cash", "cross", "isolated")] mode: &str,
    ) {
        let (mut client, cache) = build_test_exec_client_with_cache();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let order = OrderTestBuilder::new(order_type)
            .instrument_id(InstrumentId::from("ETH-USDT.OKX"))
            .client_order_id(ClientOrderId::from("OQUOTEDENIED"))
            .side(side)
            .quantity(Quantity::from("12.34"))
            .quote_quantity(quote_quantity)
            .price(Price::from(if side == OrderSide::Buy {
                "2350.12"
            } else {
                "2340.12"
            }))
            .trigger_price(Price::from("2345.67"))
            .trigger_type(if expected_reason.ends_with("triggers") {
                TriggerType::BidAsk
            } else {
                TriggerType::LastPrice
            })
            .trailing_offset(Decimal::new(25, 0))
            .trailing_offset_type(TrailingOffsetType::BasisPoints)
            .build();

        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        let mut command = SubmitOrder::from_order(
            &order,
            client.core.trader_id,
            Some(*OKX_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );

        command.params = Some(Params::new(
            [("td_mode".to_string(), Value::String(mode.to_string()))].into(),
        ));
        let trade_mode = mode.parse::<OKXTradeMode>().unwrap();

        client.submit_order(command).unwrap();
        let event = rx.try_recv().unwrap();

        let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
            panic!("Expected denial, received {event:?}");
        };

        assert_eq!(denied.client_order_id, order.client_order_id());

        let expected = if expected_reason.ends_with("triggers") {
            format!("VALIDATION_FAILED: {expected_reason}")
        } else {
            format!("VALIDATION_FAILED: {expected_reason} in {trade_mode} mode")
        };

        assert_eq!(denied.reason.as_str(), expected);
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
        assert_eq!(
            client
                .ws_dispatch_state
                .order_context(order.client_order_id()),
            None
        );
        assert!(client.ws_dispatch_state.algo_order_states.is_empty());
    }

    #[rstest]
    #[case(OrderType::StopMarket, "slTriggerPx", "newSlTriggerPx")]
    #[case(OrderType::MarketIfTouched, "tpTriggerPx", "newTpTriggerPx")]
    #[tokio::test]
    async fn test_submit_cash_spot_buy_records_conditional_before_stream_update(
        #[case] order_type: OrderType,
        #[case] trigger_field: &str,
        #[case] amend_field: &str,
        #[values(false, true)] quote_quantity: bool,
    ) {
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().route("/api/v5/trade/order-algo", post(
            move |Json(request): Json<Value>| {
                let tx = request_tx.clone();
                async move {
                    tx.send(request).unwrap();
                    Json(serde_json::json!({"code": "0", "msg": "", "data": [{
                        "algoId": "7003", "algoClOrdId": "OSPOTCONDITIONAL", "sCode": "0", "sMsg": ""
                    }]}))
                }
            },
        ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, cache) = build_test_exec_client_with_cache();
        client.http_client = OKXHttpClient::with_credentials(
            Some("test_key".into()),
            Some("test_secret".into()),
            Some("test_pass".into()),
            Some(base_url),
            5,
            0,
            1,
            1,
            OKXEnvironment::Live,
            None,
        )
        .unwrap();
        let client_order_id = ClientOrderId::from("OSPOTCONDITIONAL");
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);

        let quantity = Quantity::from(if quote_quantity {
            "12.34500"
        } else {
            "0.12345"
        });

        let order = OrderTestBuilder::new(order_type)
            .instrument_id(InstrumentId::from("ETH-USDT.OKX"))
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(quantity)
            .quote_quantity(quote_quantity)
            .trigger_price(Price::from("2345.67"))
            .trigger_type(TriggerType::LastPrice)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        let submit = SubmitOrder::from_order(
            &order,
            client.core.trader_id,
            Some(*OKX_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        client.submit_order(submit).unwrap();
        let request = tokio::time::timeout(Duration::from_secs(5), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let algo_type = client
            .ws_dispatch_state
            .algo_order_states
            .get(&client_order_id)
            .unwrap()
            .order_type;
        let context = client.ws_dispatch_state.order_context(client_order_id);

        let amend = ModifyOrder::new(
            client.core.trader_id,
            Some(*OKX_CLIENT_ID),
            order.strategy_id(),
            order.instrument_id(),
            client_order_id,
            Some(VenueOrderId::from("7003")),
            None,
            None,
            Some(Price::from("2456.78")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        let amendment = algo_amend_request(&amend, Some(algo_type), context).unwrap();
        let amendment = serde_json::to_value(amendment).unwrap();
        wait_until_async(
            || async { client.pending_tasks.all_finished() },
            Duration::from_secs(5),
        )
        .await;
        server.abort();

        assert_eq!(request["ordType"], "conditional");
        assert_eq!(
            request["tgtCcy"],
            if quote_quantity {
                "quote_ccy"
            } else {
                "base_ccy"
            }
        );
        assert_eq!(request["sz"], quantity.to_string());
        assert_eq!(request[trigger_field], "2345.67");
        assert_eq!(algo_type, OKXAlgoOrderType::Conditional);
        assert_eq!(context.unwrap().is_quote_quantity, quote_quantity);
        assert_eq!(amendment[amend_field], "2456.78");
        assert_eq!(amendment.get("newTriggerPx"), None);

        let mut instrument = currency_pair_ethusdt();
        instrument.id = order.instrument_id();
        instrument.raw_symbol = Symbol::from("ETH-USDT");
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("ETH-USDT"),
            InstrumentAny::CurrencyPair(instrument),
        );
        let mut fee_cache = FeeCache::new();
        let mut fills = FilledQtyCache::new();
        let mut snapshots = AHashMap::new();
        let mut parent: Value =
            serde_json::from_str(include_str!("../test_data/ws_orders_algo.json")).unwrap();
        let msg = &mut parent["data"][0];
        for (key, value) in [
            ("algoId", Value::from("7003")),
            ("algoClOrdId", Value::from(client_order_id.as_str())),
            ("instId", Value::from("ETH-USDT")),
            ("instType", Value::from("SPOT")),
            ("side", Value::from("buy")),
            ("posSide", Value::from("net")),
            ("tdMode", Value::from("cash")),
            ("reduceOnly", Value::from("false")),
            ("ordType", request["ordType"].clone()),
            ("sz", request["sz"].clone()),
            ("tgtCcy", request["tgtCcy"].clone()),
            (trigger_field, request[trigger_field].clone()),
            ("triggerPx", Value::from("")),
        ] {
            msg[key] = value;
        }

        let (price_field, type_field) = if order_type == OrderType::StopMarket {
            ("slOrdPx", "slTriggerPxType")
        } else {
            ("tpOrdPx", "tpTriggerPxType")
        };

        msg[price_field] = Value::from("-1");
        msg[type_field] = Value::from("last");
        let pending = OKXWsMessage::AlgoOrders(
            serde_json::from_value(serde_json::json!([msg.clone()])).unwrap(),
        );
        let pending_replay = OKXWsMessage::AlgoOrders(
            serde_json::from_value(serde_json::json!([msg.clone()])).unwrap(),
        );
        msg[trigger_field] = Value::from("2456.78");
        msg["uTime"] = Value::from("1737400000050");
        let amended = OKXWsMessage::AlgoOrders(
            serde_json::from_value(serde_json::json!([msg.clone()])).unwrap(),
        );
        let amended_replay = OKXWsMessage::AlgoOrders(
            serde_json::from_value(serde_json::json!([msg.clone()])).unwrap(),
        );
        msg["state"] = Value::from("order_placed");
        msg["ordId"] = Value::from("7004");
        msg["uTime"] = Value::from("1737400000100");
        let triggered = OKXWsMessage::AlgoOrders(
            serde_json::from_value(serde_json::json!([msg.clone()])).unwrap(),
        );
        let mut child: Value = serde_json::from_str(include_str!(
            "../test_data/ws_orders_algo_child_filled_empty_cl_ord_id.json"
        ))
        .unwrap();
        let msg = &mut child["data"][0];

        let filled_qty = if quote_quantity { "0.00500" } else { "0.12345" };

        for (key, value) in [
            ("algoId", "7003"),
            ("algoClOrdId", client_order_id.as_str()),
            ("ordId", "7004"),
            ("ordType", "market"),
            ("tdMode", "cash"),
            (
                "tgtCcy",
                if quote_quantity {
                    "quote_ccy"
                } else {
                    "base_ccy"
                },
            ),
            (
                "sz",
                if quote_quantity {
                    "12.34500"
                } else {
                    "0.12345"
                },
            ),
            ("accFillSz", filled_qty),
            ("fillSz", filled_qty),
            ("avgPx", "2469.00"),
            ("fillPx", "2469.00"),
            ("px", ""),
            ("uTime", "1737400000200"),
            ("cTime", "1737400000000"),
            ("fillTime", "1737400000200"),
        ] {
            msg[key] = Value::from(value);
        }

        let filled = OKXWsMessage::Orders(serde_json::from_value(child["data"].clone()).unwrap());
        let filled_replay =
            OKXWsMessage::Orders(serde_json::from_value(child["data"].clone()).unwrap());

        for message in [
            pending,
            pending_replay,
            amended,
            amended_replay,
            triggered,
            filled,
            filled_replay,
        ] {
            dispatch_ws_message(
                message,
                &client.emitter,
                &client.ws_dispatch_state,
                client.core.account_id,
                client.core.account_type,
                &instruments,
                &mut fee_cache,
                &mut fills,
                &mut snapshots,
                client.clock,
            );
        }

        let events: Vec<_> = std::iter::from_fn(|| event_rx.try_recv().ok()).collect();

        let [
            ExecutionEvent::Order(OrderEventAny::Submitted(submitted)),
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)),
            ExecutionEvent::Order(OrderEventAny::Updated(amended)),
            ExecutionEvent::Order(OrderEventAny::Updated(bound)),
            ExecutionEvent::Order(OrderEventAny::Filled(filled)),
            reports @ ..,
        ] = events.as_slice()
        else {
            panic!(
                "Expected submitted, accepted, amended, bound, filled, and optional status, was {events:?}"
            );
        };

        let mut applied = order.clone();

        for event in &events {
            if let ExecutionEvent::Order(event) = event {
                applied.apply(event.clone()).unwrap();
                assert_eq!(applied.is_quote_quantity(), quote_quantity);
            }
        }

        assert_eq!(submitted.client_order_id, client_order_id);
        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.venue_order_id, VenueOrderId::from("7003"));
        assert_eq!(amended.client_order_id, client_order_id);
        assert_eq!(amended.venue_order_id, Some(VenueOrderId::from("7003")));
        assert_eq!(amended.quantity, quantity);
        assert_eq!(amended.price, None);
        assert_eq!(amended.trigger_price, Some(Price::from("2456.78")));
        assert_eq!(amended.is_quote_quantity, quote_quantity);
        assert_eq!(bound.client_order_id, client_order_id);
        assert_eq!(bound.venue_order_id, Some(VenueOrderId::from("7004")));
        assert_eq!(bound.quantity, quantity);
        assert_eq!(bound.price, None);
        assert_eq!(bound.trigger_price, Some(Price::from("2456.78")));
        assert_eq!(bound.is_quote_quantity, quote_quantity);
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.venue_order_id, VenueOrderId::from("7004"));
        assert_eq!(filled.last_qty, Quantity::from(filled_qty));
        assert_eq!(filled.last_px, Price::from("2469.00"));

        if quote_quantity {
            let [ExecutionEvent::Report(CommonExecutionReport::Order(report))] = reports else {
                panic!("Expected one terminal quote-sized status report, was {reports:?}");
            };

            assert_eq!(report.client_order_id, Some(client_order_id));
            assert_eq!(report.venue_order_id, VenueOrderId::from("7004"));
            assert_eq!(report.order_status, OrderStatus::Filled);
            assert_eq!(report.quantity, Quantity::from(filled_qty));
            assert_eq!(report.filled_qty, Quantity::from(filled_qty));
        } else {
            assert!(reports.is_empty(), "{reports:?}");
            assert_eq!(applied.status(), OrderStatus::Filled);
        }

        assert_eq!(
            client.ws_dispatch_state.order_identity(client_order_id),
            None
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_submit_conditional_order_resolves_routing_context() {
        let router = Router::new().route(
            "/api/v5/trade/order-algo",
            post(|Json(request): Json<Value>| async move {
                let data = match request["algoClOrdId"].as_str() {
                    Some("ORESTBIND001") => serde_json::json!([{
                        "algoId": "3796251408639365120",
                        "algoClOrdId": "ORESTBIND001",
                        "sCode": "0",
                        "sMsg": "",
                    }]),
                    Some("ORESTAMBIGUOUS002") => serde_json::json!([{
                        "algoId": "",
                        "algoClOrdId": "ORESTAMBIGUOUS002",
                        "sCode": "51149",
                        "sMsg": "Order timed out. Please try again.",
                    }]),
                    _ => serde_json::json!([]),
                };
                Json(serde_json::json!({
                    "code": "0",
                    "msg": "",
                    "data": data,
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url_http = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            axum::serve(listener, router.into_make_service())
                .await
                .unwrap();
        });

        let config = OKXExecutionClientConfig {
            api_key: Some("test_key".into()),
            api_secret: Some("test_secret".into()),
            api_passphrase: Some("test_pass".into()),
            base_url_http: Some(base_url_http),
            ..OKXExecutionClientConfig::default()
        };
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("OKX-TEST"),
            *OKX_VENUE,
            OmsType::Hedging,
            config.account_id,
            AccountType::Cash,
            None,
            Rc::clone(&cache),
        );
        let mut client = OKXExecutionClient::new(core, config).unwrap();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);
        let client_order_id = ClientOrderId::from("ORESTBIND001");
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .client_order_id(client_order_id)
            .strategy_id(StrategyId::from("S-REST-BIND-001"))
            .instrument_id(InstrumentId::from("BTC-USDT-SWAP.OKX"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.01"))
            .price(Price::from("94900"))
            .trigger_price(Price::from("95000"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        let command = SubmitOrder::from_order(
            &order,
            TraderId::from("TESTER-001"),
            Some(*OKX_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );

        client.submit_order(command).unwrap();
        wait_until_async(
            || async {
                client
                    .ws_dispatch_state
                    .order_venue_binding(client_order_id)
                    == Some((VenueOrderId::from("3796251408639365120"), false))
            },
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(
            client.ws_dispatch_state.order_identity(client_order_id),
            Some(OrderIdentity {
                client_order_id,
                strategy_id: StrategyId::from("S-REST-BIND-001"),
                instrument_id: InstrumentId::from("BTC-USDT-SWAP.OKX"),
                order_side: OrderSide::Sell,
                order_type: OrderType::StopLimit,
            })
        );

        let ambiguous_client_order_id = ClientOrderId::from("ORESTAMBIGUOUS002");
        let ambiguous_order = OrderTestBuilder::new(OrderType::StopLimit)
            .client_order_id(ambiguous_client_order_id)
            .strategy_id(StrategyId::from("S-REST-BIND-001"))
            .instrument_id(InstrumentId::from("BTC-USDT-SWAP.OKX"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.02"))
            .price(Price::from("94800"))
            .trigger_price(Price::from("95100"))
            .build();
        cache
            .borrow_mut()
            .add_order(ambiguous_order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        let ambiguous_command = SubmitOrder::from_order(
            &ambiguous_order,
            TraderId::from("TESTER-001"),
            Some(*OKX_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );

        client.submit_order(ambiguous_command).unwrap();
        wait_until_async(
            || async {
                client
                    .ws_dispatch_state
                    .order_identity(ambiguous_client_order_id)
                    .is_none()
            },
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(
            client
                .ws_dispatch_state
                .order_venue_binding(ambiguous_client_order_id),
            None
        );

        wait_until_async(
            || async { client.pending_tasks.all_finished() },
            Duration::from_secs(5),
        )
        .await;
        terminate_tasks(&client.pending_tasks, "test execution client")
            .await
            .expect("execution tasks terminated");

        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }

        for expected_client_order_id in [client_order_id, ambiguous_client_order_id] {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event,
                        ExecutionEvent::Order(OrderEventAny::Submitted(submitted))
                            if submitted.client_order_id == expected_client_order_id
                    ))
                    .count(),
                1,
            );
        }
        assert!(
            !events.iter().any(|event| matches!(
                event,
                ExecutionEvent::Order(OrderEventAny::Rejected(rejected))
                    if rejected.client_order_id == ambiguous_client_order_id
            )),
            "ambiguous algo submit failure should not emit OrderRejected: {events:?}",
        );
    }

    #[rstest]
    #[case::single(1, false)]
    #[case::batch(23, true)]
    #[tokio::test]
    async fn closed_handler_rejects_every_unsent_cancel(#[case] count: usize, #[case] batch: bool) {
        let (mut client, cache) = build_test_exec_client_with_cache();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let strategy_id = StrategyId::from("S-CLOSED-HANDLER");
        client
            .ws_private
            .cache_inst_id_code(Ustr::from("BTC-USDT"), 10459);
        let mut cancels = Vec::new();

        for index in 0..count {
            let client_order_id = ClientOrderId::from(format!("OCLOSED{index}"));
            let venue_order_id = VenueOrderId::from(format!("closed-venue-{index}"));
            let mut order = OrderTestBuilder::new(OrderType::Limit)
                .client_order_id(client_order_id)
                .strategy_id(strategy_id)
                .instrument_id(instrument_id)
                .side(OrderSide::Sell)
                .quantity(Quantity::from("0.005"))
                .price(Price::from("110000"))
                .build();
            let accepted =
                TestOrderEventStubs::accepted(&order, client.core.account_id, venue_order_id);
            order.apply(accepted).unwrap();
            cache
                .borrow_mut()
                .add_order(order, None, Some(*OKX_CLIENT_ID), false)
                .unwrap();

            if index % 2 == 0 {
                client.ws_dispatch_state.bind_order(
                    client_order_id,
                    VenueOrderId::from(format!("authoritative-venue-{index}")),
                );
            }

            cancels.push(CancelOrder::new(
                TraderId::from("TESTER-001"),
                Some(*OKX_CLIENT_ID),
                strategy_id,
                instrument_id,
                client_order_id,
                Some(venue_order_id),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ));
        }

        if batch {
            let command = BatchCancelOrders::new(
                TraderId::from("TESTER-001"),
                Some(*OKX_CLIENT_ID),
                strategy_id,
                instrument_id,
                cancels.clone(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            );
            client.batch_cancel_orders(command).unwrap();
        } else {
            client.cancel_order(cancels[0].clone()).unwrap();
        }

        wait_until_async(
            || async { client.pending_tasks.all_finished() },
            Duration::from_secs(5),
        )
        .await;
        terminate_tasks(&client.pending_tasks, "test execution client")
            .await
            .unwrap();
        let events: Vec<_> = std::iter::from_fn(|| event_rx.try_recv().ok()).collect();

        assert_eq!(events.len(), count);

        for (index, (event, cancel)) in events.iter().zip(cancels).enumerate() {
            let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event else {
                panic!("Expected cancel rejection, received {event:?}");
            };

            assert_eq!(rejected.trader_id, client.core.trader_id);
            assert_eq!(rejected.strategy_id, strategy_id);
            assert_eq!(rejected.instrument_id, instrument_id);
            assert_eq!(rejected.client_order_id, cancel.client_order_id);

            let expected_venue_order_id = if index % 2 == 0 {
                Some(VenueOrderId::from(format!("authoritative-venue-{index}")))
            } else {
                cancel.venue_order_id
            };

            assert_eq!(rejected.venue_order_id, expected_venue_order_id);
            assert_eq!(rejected.account_id, Some(client.core.account_id));
            assert_eq!(
                rejected.reason.as_str(),
                "Handler not available: channel closed"
            );
            assert!(!rejected.reconciliation);
            assert_eq!(rejected.causation_id, None);
            assert_eq!(
                cache
                    .borrow()
                    .order(&cancel.client_order_id)
                    .unwrap()
                    .status(),
                OrderStatus::Accepted
            );
        }

        assert!(client.ws_private.pending_cancels.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_local_cancel_validation_failure_does_not_emit_order_cancel_rejected() {
        let (mut client, cache) = build_test_exec_client_with_cache();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);
        let client_order_id = ClientOrderId::from("OLOCALCANCELINVALID001");
        let strategy_id = StrategyId::from("S-LOCAL-CANCEL-INVALID-001");
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("100000"))
            .build();
        cache
            .borrow_mut()
            .add_order(order, None, Some(*OKX_CLIENT_ID), false)
            .expect("cache order");
        let command = CancelOrder {
            trader_id: TraderId::from("TESTER-001"),
            client_id: Some(*OKX_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id: Some(VenueOrderId::from("v-1")),
            command_id: UUID4::new(),
            ts_init: UnixNanos::default(),
            params: None,
            correlation_id: None,
            causation_id: None,
        };

        client.cancel_order(command).expect("cancel order");
        wait_until_async(
            || async { client.pending_tasks.all_finished() },
            Duration::from_secs(5),
        )
        .await;
        terminate_tasks(&client.pending_tasks, "test execution client")
            .await
            .expect("execution task terminated");

        let events: Vec<_> = std::iter::from_fn(|| event_rx.try_recv().ok()).collect();
        assert!(
            !events.iter().any(|event| matches!(
                event,
                ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected))
                    if rejected.client_order_id == client_order_id
            )),
            "local cancel validation failure should not emit OrderCancelRejected: {events:?}",
        );
    }

    #[rstest]
    fn test_imported_conditional_order_preserves_report_routing() {
        let (mut client, cache) = build_test_exec_client_with_cache();
        let cid = ClientOrderId::from("OEEADEMOSTOPLIMIT001");
        let iid = InstrumentId::from("ETH-USDT.OKX");
        let mut instrument = currency_pair_ethusdt();
        instrument.id = iid;
        instrument.raw_symbol = Symbol::from("ETH-USDT");
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("ETH-USDT"),
            InstrumentAny::CurrencyPair(instrument),
        );
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(iid)
            .client_order_id(cid)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.003"))
            .price(Price::from("1886.00"))
            .trigger_price(Price::from("1900.00"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client.core.client_id), false)
            .unwrap();
        client.register_external_order(
            cid,
            VenueOrderId::from("7003"),
            iid,
            order.strategy_id(),
            UnixNanos::default(),
        );
        client.ensure_order_identity(cid, order.strategy_id(), iid);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        let mut fee_cache = FeeCache::new();
        let mut fills = FilledQtyCache::new();
        let mut snapshots = AHashMap::new();
        let mut parent: Value =
            serde_json::from_str(include_str!("../test_data/ws_orders_algo.json")).unwrap();
        let msg = &mut parent["data"][0];
        msg["algoClOrdId"] = serde_json::json!(cid.as_str());
        msg["algoId"] = serde_json::json!("7003");
        msg["instId"] = serde_json::json!("ETH-USDT");
        msg["ordType"] = serde_json::json!("conditional");
        msg["state"] = serde_json::json!("live");
        let message =
            OKXWsMessage::AlgoOrders(serde_json::from_value(parent["data"].clone()).unwrap());
        dispatch_ws_message(
            message,
            &client.emitter,
            &client.ws_dispatch_state,
            client.core.account_id,
            client.core.account_type,
            &instruments,
            &mut fee_cache,
            &mut fills,
            &mut snapshots,
            client.clock,
        );
        let event = rx.try_recv().unwrap();

        let ExecutionEvent::Report(CommonExecutionReport::Order(report)) = event else {
            panic!("expected imported parent report, was {event:?}");
        };

        assert_eq!(report.client_order_id, Some(cid));
        assert_eq!(report.venue_order_id, VenueOrderId::from("7003"));
        assert!(rx.try_recv().is_err());
        let child: Value = serde_json::from_str(include_str!(
            "../test_data/ws_orders_algo_child_filled_empty_cl_ord_id.json"
        ))
        .unwrap();
        let message = OKXWsMessage::Orders(serde_json::from_value(child["data"].clone()).unwrap());
        dispatch_ws_message(
            message,
            &client.emitter,
            &client.ws_dispatch_state,
            client.core.account_id,
            client.core.account_type,
            &instruments,
            &mut fee_cache,
            &mut fills,
            &mut snapshots,
            client.clock,
        );
        let event = rx.try_recv().unwrap();

        let ExecutionEvent::Report(CommonExecutionReport::Fill(report)) = event else {
            panic!("expected imported child fill report, was {event:?}");
        };

        assert_eq!(report.client_order_id, Some(cid));
        assert_eq!(
            report.venue_order_id,
            VenueOrderId::from("2497956918703120501")
        );
        assert_eq!(report.last_qty, Quantity::from("0.003"));
        assert_eq!(report.last_px, Price::from("1886.00"));
        assert!(rx.try_recv().is_err());
        assert_eq!(client.ws_dispatch_state.order_identity(cid), None);
    }

    #[rstest]
    fn test_register_external_order_without_cached_order() {
        let client = build_test_exec_client();
        let cid = ClientOrderId::from("O-MISSING");
        client.register_external_order(
            cid,
            VenueOrderId::from("7002"),
            InstrumentId::from("BTC-USDT.OKX"),
            StrategyId::from("S-MISSING"),
            UnixNanos::default(),
        );
        assert_eq!(client.ws_dispatch_state.order_identity(cid), None);
        assert!(!client.ws_dispatch_state.contains_accepted(&cid));
    }

    #[rstest]
    fn test_register_external_order_tracks_initialized_import() {
        let (client, cache) = build_test_exec_client_with_cache();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-USDT.OKX"))
            .client_order_id(ClientOrderId::from("O-IMPORTED"))
            .strategy_id(StrategyId::from("S-IMPORTED"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.02"))
            .price(Price::from("50000.0"))
            .build();
        let venue_order_id = VenueOrderId::from("7001");
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client.core.client_id), false)
            .unwrap();

        client.register_external_order(
            order.client_order_id(),
            venue_order_id,
            order.instrument_id(),
            order.strategy_id(),
            UnixNanos::default(),
        );

        assert_eq!(
            client
                .ws_dispatch_state
                .order_identity(order.client_order_id()),
            Some(OrderIdentity::from(&order))
        );
        assert!(
            client
                .ws_dispatch_state
                .contains_accepted(&order.client_order_id())
        );
        assert!(
            !client
                .ws_dispatch_state
                .contains_triggered(&order.client_order_id())
        );
        assert_eq!(order.status(), OrderStatus::Initialized);
        assert_eq!(order.account_id(), None);
        assert_eq!(order.venue_order_id(), None);
        client
            .ws_dispatch_state
            .remove_order_tracking(order.client_order_id());
        assert_eq!(
            client
                .ws_dispatch_state
                .order_identity(order.client_order_id()),
            None
        );
    }

    #[rstest]
    fn test_ensure_order_identity_skips_order_without_cached_side() {
        let client = build_test_exec_client();
        let client_order_id = ClientOrderId::from("O-RESTORED-001");
        let strategy_id = StrategyId::from("S-RESTORED-002");
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");

        client.ensure_order_identity(client_order_id, strategy_id, instrument_id);

        assert!(
            client
                .ws_dispatch_state
                .order_identities
                .get(&client_order_id)
                .is_none()
        );
    }

    #[rstest]
    fn test_on_instrument_writes_through_to_client_caches() {
        // Bus-delivered instrument updates must land in both the private and
        // business WebSocket caches, and in the HTTP cache used by reconciliation.
        use nautilus_model::instruments::stubs::crypto_perpetual_ethusdt;

        let mut client = build_test_exec_client();
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
        let symbol = instrument.symbol().inner();
        let raw_symbol = instrument.raw_symbol().inner();

        client.on_instrument(instrument.clone());

        let private_cache = client.ws_private.instruments_cache_arc();
        let business_cache = client.ws_business.instruments_cache_arc();
        assert_eq!(
            client
                .http_client
                .get_instrument(&raw_symbol)
                .map(|i| i.id()),
            Some(instrument.id()),
        );
        assert_eq!(
            private_cache.load().get(&symbol).map(Instrument::id),
            Some(instrument.id()),
        );
        assert_eq!(
            business_cache.load().get(&symbol).map(Instrument::id),
            Some(instrument.id()),
        );
    }

    #[rstest]
    #[case(OrderSide::Buy, false, PositionSide::Long)]
    #[case(OrderSide::Sell, false, PositionSide::Short)]
    #[case(OrderSide::Buy, true, PositionSide::Short)]
    #[case(OrderSide::Sell, true, PositionSide::Long)]
    fn test_order_position_side(
        #[case] side: OrderSide,
        #[case] reduce_only: bool,
        #[case] expected: PositionSide,
        #[values("ETH-USDT-SWAP.OKX", "ETH-USDT-271231.OKX")] instrument_id: &str,
    ) {
        let mut client = build_test_exec_client();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from(instrument_id))
            .side(side)
            .price(Price::from("123.45"))
            .quantity(Quantity::from("2"))
            .reduce_only(reduce_only)
            .build();
        assert_eq!(client.order_position_side(&order, None), None);
        client.position_mode = OKXPositionMode::LongShortMode;
        assert_eq!(client.order_position_side(&order, None), Some(expected));
    }

    #[rstest]
    #[case(OrderSide::Buy, PositionSide::Short)]
    #[case(OrderSide::Sell, PositionSide::Long)]
    fn test_order_position_side_close_fraction(
        #[case] side: OrderSide,
        #[case] expected: PositionSide,
    ) {
        let mut client = build_test_exec_client();
        client.position_mode = OKXPositionMode::LongShortMode;
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(InstrumentId::from("ETH-USDT-SWAP.OKX"))
            .side(side)
            .trigger_price(Price::from("123.45"))
            .quantity(Quantity::from("2"))
            .build();
        let params =
            Params::new([("close_fraction".to_string(), Value::String("1".to_string()))].into());
        assert_eq!(
            client.order_position_side(&order, Some(&params)),
            Some(expected)
        );
    }

    #[rstest]
    #[case("ETH-USDT.OKX")]
    #[case("ETH-USD-271231-2000-C.OKX")]
    fn test_order_position_side_other_products(#[case] instrument_id: &str) {
        let mut client = build_test_exec_client();
        client.position_mode = OKXPositionMode::LongShortMode;
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from(instrument_id))
            .side(OrderSide::Sell)
            .price(Price::from("123.45"))
            .quantity(Quantity::from("2"))
            .build();
        assert_eq!(client.order_position_side(&order, None), None);
    }

    #[rstest]
    #[case(None)]
    #[case(Some("ETH-USDT-SWAP.OKX-LONG"))]
    fn test_validate_submit_position_id_accepts_leg(#[case] position_id: Option<&str>) {
        assert_eq!(
            validate_submit_position_id(
                InstrumentId::from("ETH-USDT-SWAP.OKX"),
                Some(PositionSide::Long),
                position_id.map(PositionId::from),
            ),
            Ok(()),
        );
    }

    #[rstest]
    fn test_validate_submit_position_id_rejects_other_leg() {
        let position_id = PositionId::from("ETH-USDT-SWAP.OKX-SHORT");
        let result = validate_submit_position_id(
            InstrumentId::from("ETH-USDT-SWAP.OKX"),
            Some(PositionSide::Long),
            Some(position_id),
        );
        assert_eq!(
            result,
            Err(OrderDeniedReason::InvalidPositionId {
                position_id,
                detail: "expected OKX venue position ID ETH-USDT-SWAP.OKX-LONG".to_string(),
            })
        );
        assert_eq!(
            validate_submit_position_id(
                InstrumentId::from("ETH-USDT-SWAP.OKX"),
                None,
                Some(position_id),
            ),
            Ok(())
        );
    }

    #[rstest]
    #[case(None, OKXTradeMode::Cross, None)]
    #[case(Some(PositionSide::Long), OKXTradeMode::Isolated, None)]
    #[case(
        Some(PositionSide::Short),
        OKXTradeMode::Cross,
        Some(OKXMarginMode::Cross)
    )]
    fn test_validate_trade_mode_accepts_configured_or_unrestricted(
        #[case] position_side: Option<PositionSide>,
        #[case] trade_mode: OKXTradeMode,
        #[case] margin_mode: Option<OKXMarginMode>,
    ) {
        let mut client = build_test_exec_client();
        client.config.margin_mode = margin_mode;
        assert_eq!(
            client.validate_trade_mode(
                InstrumentId::from("BTC-USDT-SWAP.OKX"),
                position_side,
                trade_mode
            ),
            Ok(())
        );
    }

    #[rstest]
    #[case(None)]
    #[case(Some(OKXMarginMode::None))]
    #[case(Some(OKXMarginMode::Isolated))]
    #[tokio::test]
    async fn test_update_position_mode_on_reconnect(#[case] margin_mode: Option<OKXMarginMode>) {
        let response = Arc::new(Mutex::new(
            serde_json::from_str::<Value>(include_str!(
                "../test_data/http_get_account_configuration.json"
            ))
            .unwrap(),
        ));

        let positions = Arc::new(Mutex::new(
            serde_json::json!({"code":"0", "data":[], "msg":""}),
        ));
        let positions_response = Arc::clone(&positions);
        let handler_response = Arc::clone(&response);
        let router = Router::new()
            .route(
                "/api/v5/account/config",
                get(move || {
                    let response = Arc::clone(&handler_response);
                    async move { Json(response.lock().unwrap().clone()) }
                }),
            )
            .route(
                "/api/v5/account/positions",
                get(move || {
                    let response = Arc::clone(&positions_response);
                    async move { Json(response.lock().unwrap().clone()) }
                }),
            )
            .route(
                "/api/v5/account/balance",
                get(|| async { Json(serde_json::json!({"code":"0", "data":[], "msg":""})) }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let mut client = build_test_exec_client();
        client.config.instrument_types = vec![OKXInstrumentType::Swap];
        client.config.margin_mode = margin_mode;
        client.http_client = OKXHttpClient::with_credentials(
            Some("key".into()),
            Some("secret".into()),
            Some("pass".into()),
            Some(format!("http://{address}")),
            5,
            0,
            1,
            1,
            OKXEnvironment::Demo,
            None,
        )
        .unwrap();
        response.lock().unwrap()["data"][0]["posMode"] = Value::String("long_short_mode".into());
        client.update_position_mode().await.unwrap();
        assert_eq!(client.position_mode, OKXPositionMode::LongShortMode);
        *positions.lock().unwrap() = serde_json::from_str(include_str!(
            "../test_data/http_get_positions_long_short.json"
        ))
        .unwrap();
        client.update_position_mode().await.unwrap();
        client.config.margin_mode = Some(OKXMarginMode::Cross);
        assert_eq!(
            client.update_position_mode().await.unwrap_err().to_string(),
            "OKX long/short position BTC-USDT-SWAP uses margin mode Isolated, expected Cross; mixed margin modes are not supported"
        );
        let instruments: Value =
            serde_json::from_str(include_str!("../test_data/http_get_instruments_swap.json"))
                .unwrap();
        let instrument: OKXInstrument = serde_json::from_value(
            instruments["data"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["instId"] == "BTC-USDT-SWAP")
                .unwrap()
                .clone(),
        )
        .unwrap();
        client.on_instrument(
            parse_instrument_any(&instrument, None, None, UnixNanos::default())
                .unwrap()
                .unwrap(),
        );
        let command = GeneratePositionStatusReportsBuilder::default()
            .ts_init(UnixNanos::from(98765))
            .build()
            .unwrap();
        assert_eq!(
            client
                .collect_position_status_reports(&command)
                .await
                .unwrap_err()
                .to_string(),
            "OKX long/short position BTC-USDT-SWAP uses margin mode Isolated, expected Cross; mixed margin modes are not supported"
        );
        client.config.margin_mode = margin_mode;
        let (reports, complete) = client
            .collect_position_status_reports(&command)
            .await
            .unwrap();
        assert!(complete);
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].quantity, Quantity::from("5.00"));
        assert_eq!(
            reports[0].venue_position_id,
            Some(PositionId::from("BTC-USDT-SWAP.OKX-LONG"))
        );
        assert_eq!(reports[1].quantity, Quantity::from("2.00"));
        assert_eq!(
            reports[1].venue_position_id,
            Some(PositionId::from("BTC-USDT-SWAP.OKX-SHORT"))
        );
        client.config.margin_mode = Some(OKXMarginMode::Cross);

        for position in positions.lock().unwrap()["data"].as_array_mut().unwrap() {
            position["pos"] = Value::String("0".to_string());
        }

        client.update_position_mode().await.unwrap();
        client.config.margin_mode = Some(OKXMarginMode::Isolated);
        client.update_position_mode().await.unwrap();
        response.lock().unwrap()["data"][0]["posMode"] = Value::String("net_mode".into());
        client.update_position_mode().await.unwrap();
        assert_eq!(client.position_mode, OKXPositionMode::NetMode);
        response.lock().unwrap()["data"] = Value::Array(Vec::new());
        assert_eq!(
            client.update_position_mode().await.unwrap_err().to_string(),
            "OKX account configuration response is empty"
        );
        assert_eq!(
            client.connect().await.unwrap_err().to_string(),
            "OKX account configuration response is empty"
        );
        client.config.instrument_types = vec![OKXInstrumentType::Spot, OKXInstrumentType::Option];
        client.update_position_mode().await.unwrap();
        server.abort();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn test_submit_spread_preserves_quote_quantity_handling(#[case] quote_quantity: bool) {
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();
        let router = Router::new().route(
            "/api/v5/sprd/order",
            post(move |Json(request): Json<Value>| {
                let tx = request_tx.clone();
                async move {
                    tx.send(request).unwrap();
                    let order: Value = serde_json::from_str(include_str!(
                        "../test_data/http_place_order_response.json"
                    ))
                    .unwrap();
                    Json(serde_json::json!({"code":"0", "msg":"", "data":[order]}))
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, cache) = build_test_exec_client_with_cache();
        client.http_client = OKXHttpClient::with_credentials(
            Some("key".into()),
            Some("secret".into()),
            Some("pass".into()),
            Some(format!("http://{address}")),
            5,
            0,
            1,
            1,
            OKXEnvironment::Demo,
            None,
        )
        .unwrap();
        let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX"))
            .client_order_id(ClientOrderId::from("OSPREADQUOTE"))
            .side(OrderSide::Sell)
            .quantity(Quantity::from("3"))
            .price(Price::from("12.34"))
            .quote_quantity(quote_quantity)
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        client
            .submit_order(SubmitOrder::from_order(
                &order,
                client.core.trader_id,
                Some(*OKX_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
            ))
            .unwrap();
        let request = tokio::time::timeout(Duration::from_secs(5), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        client.disconnect().await.unwrap();
        server.abort();

        assert_eq!(
            request,
            serde_json::json!({
                "sprdId":"BCH-USDT_BCH-USDT-SWAP",
                "clOrdId":"OSPREADQUOTE",
                "tag":crate::common::consts::OKX_NAUTILUS_BROKER_ID,
                "side":"sell",
                "ordType":"limit",
                "sz":"3",
                "px":"12.34"
            })
        );
    }

    fn swap_instrument() -> InstrumentAny {
        let response: Value =
            serde_json::from_str(include_str!("../test_data/http_get_instruments_swap.json"))
                .unwrap();
        let model: OKXInstrument = serde_json::from_value(response["data"][0].clone()).unwrap();
        parse_instrument_any(&model, None, None, UnixNanos::default())
            .unwrap()
            .unwrap()
    }

    #[rstest]
    #[case(OrderSide::Buy, false, "long")]
    #[case(OrderSide::Sell, false, "short")]
    #[case(OrderSide::Buy, true, "short")]
    #[case(OrderSide::Sell, true, "long")]
    #[tokio::test]
    async fn test_submit_order_position_side_wire(
        #[case] side: OrderSide,
        #[case] reduce_only: bool,
        #[case] expected_side: &str,
        #[values("ws", "batch", "http", "algo", "close_fraction")] transport: &str,
    ) {
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();
        let ws_tx = request_tx.clone();
        let router = Router::new().route("/ws", get(move |ws: WebSocketUpgrade| {
            let tx = ws_tx.clone();
            async move { ws.on_upgrade(move |mut socket| async move {
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    let request: Value = serde_json::from_str(&text).unwrap();
                    if request["op"] == "login" {
                        let response = serde_json::json!({"event":"login", "code":"0", "msg":"", "connId":"test"});
                        socket.send(Message::Text(response.to_string().into())).await.unwrap();
                    } else {
                        tx.send(request).unwrap();
                    }
                }
            }) }
        })).route("/api/v5/trade/order", post({
            let tx = request_tx.clone();
            move |Json(request): Json<Value>| {
                let tx = tx.clone();
                async move {
                    tx.send(request).unwrap();
                    Json(serde_json::from_str::<Value>(include_str!("../test_data/http_place_order_response.json")).unwrap())
                }
            }
        })).route("/api/v5/trade/order-algo", post(move |Json(request): Json<Value>| {
            let tx = request_tx.clone();
            async move {
                tx.send(request).unwrap();
                Json(serde_json::from_str::<Value>(include_str!("../test_data/http_place_algo_order_response.json")).unwrap())
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        let (mut client, cache) = build_test_exec_client_with_cache();
        client.position_mode = OKXPositionMode::LongShortMode;
        client.trade_mode = OKXTradeMode::Cross;
        client.http_client = OKXHttpClient::with_credentials(
            Some("key".into()),
            Some("secret".into()),
            Some("pass".into()),
            Some(format!("http://{address}")),
            5,
            0,
            1,
            1,
            OKXEnvironment::Demo,
            None,
        )
        .unwrap();
        client.ws_private = OKXWebSocketClient::with_credentials(
            Some(format!("ws://{address}/ws")),
            Some("key".into()),
            Some("secret".into()),
            Some("pass".into()),
            Some(client.core.account_id),
            None,
            None,
            client.config.transport_backend,
            None,
        )
        .unwrap();
        let instrument = swap_instrument();
        client.on_instrument(instrument.clone());
        client
            .ws_private
            .cache_inst_id_codes([(instrument.raw_symbol().inner(), 10458)]);

        if transport == "ws" || transport == "batch" {
            client.ws_private.connect().await.unwrap();
            client.ws_private.wait_until_active(5.0).await.unwrap();
        }

        let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(event_tx);

        let order =
            OrderTestBuilder::new(if transport == "algo" || transport == "close_fraction" {
                OrderType::StopMarket
            } else {
                OrderType::Limit
            })
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("OHEDGEWIRE"))
            .side(side)
            .quantity(Quantity::from("2"))
            .price(Price::from("123.45"))
            .trigger_price(Price::from("120.00"))
            .reduce_only(reduce_only)
            .build();

        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        let mut command = SubmitOrder::from_order(
            &order,
            client.core.trader_id,
            Some(*OKX_CLIENT_ID),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );

        if transport == "close_fraction" {
            let params = Params::new([("close_fraction".into(), Value::String("1".into()))].into());
            command.params = Some(params);
        }

        match transport {
            "batch" => {
                let order_list = OrderList::new(
                    OrderListId::from("OL-HEDGE"),
                    instrument.id(),
                    order.strategy_id(),
                    vec![order.client_order_id()],
                    UnixNanos::default(),
                );
                client
                    .submit_order_list(SubmitOrderList::new(
                        client.core.trader_id,
                        Some(*OKX_CLIENT_ID),
                        order.strategy_id(),
                        order_list,
                        vec![OrderInitialized::from(&order)],
                        None,
                        None,
                        None,
                        UUID4::new(),
                        UnixNanos::default(),
                        None,
                    ))
                    .unwrap();
            }
            "http" => client
                .submit_order_http(&command, client.order_position_side(&order, None))
                .unwrap(),
            _ => client.submit_order(command).unwrap(),
        }

        let request = tokio::time::timeout(Duration::from_secs(5), request_rx.recv())
            .await
            .unwrap()
            .unwrap();

        let body = if transport == "ws" || transport == "batch" {
            &request["args"][0]
        } else {
            &request
        };

        let expected_side = if transport == "close_fraction" {
            if side == OrderSide::Buy {
                "short"
            } else {
                "long"
            }
        } else {
            expected_side
        };

        assert_eq!(body["posSide"], expected_side);
        assert_eq!(
            body["side"],
            if side == OrderSide::Buy {
                "buy"
            } else {
                "sell"
            }
        );
        assert_eq!(body.get("reduceOnly"), None);

        if transport == "close_fraction" {
            assert_eq!(body.get("sz"), None);
            assert_eq!(body["closeFraction"], "1");
        } else {
            assert_eq!(body["sz"], "2");
        }

        client.disconnect().await.unwrap();
        server.abort();
    }

    #[rstest]
    #[case(OrderSide::Buy, OKXPositionSide::Long)]
    #[case(OrderSide::Sell, OKXPositionSide::Short)]
    fn test_missing_hedge_leg_reports_flat(
        #[case] side: OrderSide,
        #[case] leg: OKXPositionSide,
        #[values(false, true)] other_leg_present: bool,
        #[values(OKXInstrumentType::Swap, OKXInstrumentType::Futures)]
        configured_type: OKXInstrumentType,
    ) {
        let (mut client, cache) = build_test_exec_client_with_cache();
        client.position_mode = OKXPositionMode::LongShortMode;
        client.config.instrument_types = vec![configured_type];
        let instrument = swap_instrument();
        let position_id = venue_position_id(instrument.id(), leg).unwrap();
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument.id())
            .side(side)
            .quantity(Quantity::from("2"))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::from("T-HEDGE")),
            Some(position_id),
            Some(Price::from("123.4")),
            Some(Quantity::from("2")),
            Some(LiquiditySide::Taker),
            Some(Money::from("0.01 USDT")),
            None,
            Some(client.core.account_id),
        );
        let position = Position::new(&instrument, fill.into());
        cache
            .borrow_mut()
            .add_position(&position, OmsType::Hedging)
            .unwrap();
        let ts_init = UnixNanos::from(98765);

        let other_side = if leg == OKXPositionSide::Long {
            OKXPositionSide::Short
        } else {
            OKXPositionSide::Long
        };

        let mut reports = if other_leg_present {
            vec![PositionStatusReport::new(
                client.core.account_id,
                instrument.id(),
                other_side.into(),
                Quantity::from("3"),
                ts_init,
                ts_init,
                None,
                venue_position_id(instrument.id(), other_side),
                None,
            )]
        } else {
            Vec::new()
        };

        let original_reports = reports.clone();
        client.add_flat_derivative_position_reports(&[], &[], &mut reports, ts_init);

        if configured_type != OKXInstrumentType::Swap {
            assert_eq!(reports, original_reports);
            return;
        }

        assert_eq!(reports.len(), 1 + usize::from(other_leg_present));
        let report = reports.last().unwrap();
        assert_eq!(report.account_id, client.core.account_id);
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(report.venue_position_id, Some(position_id));
        assert_eq!(report.position_side, PositionSide::Flat);
        assert_eq!(report.quantity, Quantity::zero(position.quantity.precision));
        assert_eq!(report.ts_last, ts_init);
        assert_eq!(report.ts_init, ts_init);
        client.add_flat_derivative_position_reports(&[], &[], &mut reports, ts_init);
        assert_eq!(reports.len(), 1 + usize::from(other_leg_present));
    }

    #[rstest]
    #[case(OKXPositionSide::Long, OrderSide::Sell)]
    #[case(OKXPositionSide::Short, OrderSide::Buy)]
    fn test_missing_hedge_history_leg_reports_flat(
        #[case] leg: OKXPositionSide,
        #[case] closing_side: OrderSide,
        #[values("order", "fill", "both")] source: &str,
        #[values(false, true)] other_leg_present: bool,
    ) {
        let mut client = build_test_exec_client();
        client.position_mode = OKXPositionMode::LongShortMode;
        client.config.instrument_types = vec![OKXInstrumentType::Swap];
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let position_id = venue_position_id(instrument_id, leg).unwrap();
        let ts_init = UnixNanos::from(98765);
        let mut order = make_query_order_report(Some("O-CLOSE"), "V-CLOSE");
        order.instrument_id = instrument_id;
        order.order_side = closing_side.into();
        order.order_status = OrderStatus::Filled;
        order.quantity = Quantity::from("2.00");
        order.filled_qty = Quantity::from("2.00");
        order.venue_position_id = Some(position_id);

        let fill = FillReport::new(
            client.core.account_id,
            instrument_id,
            VenueOrderId::from("V-CLOSE"),
            TradeId::from("T-CLOSE"),
            closing_side,
            Quantity::from("2.00"),
            Price::from("123.4"),
            Money::from("0.01 USDT"),
            LiquiditySide::Taker,
            Some(ClientOrderId::from("O-CLOSE")),
            Some(position_id),
            ts_init,
            ts_init,
            None,
        );

        let orders = if source == "fill" {
            vec![]
        } else {
            vec![order]
        };

        let fills = if source == "order" {
            vec![]
        } else {
            vec![fill]
        };

        let other_leg = if leg == OKXPositionSide::Long {
            OKXPositionSide::Short
        } else {
            OKXPositionSide::Long
        };

        let mut reports = if other_leg_present {
            vec![PositionStatusReport::new(
                client.core.account_id,
                instrument_id,
                other_leg.into(),
                Quantity::from("3.50"),
                ts_init,
                ts_init,
                None,
                venue_position_id(instrument_id, other_leg),
                None,
            )]
        } else {
            vec![]
        };

        client.add_flat_derivative_position_reports(&orders, &fills, &mut reports, ts_init);

        assert_eq!(reports.len(), 1 + usize::from(other_leg_present));
        let report = reports.last().unwrap();
        assert_eq!(report.account_id, client.core.account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.venue_position_id, Some(position_id));
        assert_eq!(report.position_side, PositionSide::Flat);
        assert_eq!(report.quantity, Quantity::zero(2));
        assert_eq!(report.quantity.precision, 2);
        assert_eq!(report.ts_last, ts_init);
        assert_eq!(report.ts_init, ts_init);
        assert_eq!(report.avg_px_open, None);

        if other_leg_present {
            assert_eq!(
                reports[0].venue_position_id,
                venue_position_id(instrument_id, other_leg)
            );
            assert_eq!(reports[0].quantity, Quantity::from("3.50"));
            assert_eq!(reports[0].position_side, PositionSide::from(other_leg));
        }

        client.add_flat_derivative_position_reports(&orders, &fills, &mut reports, ts_init);
        assert_eq!(reports.len(), 1 + usize::from(other_leg_present));
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_submit_order_denies_hedge_conflict(#[case] margin_conflict: bool) {
        let (mut client, cache) = build_test_exec_client_with_cache();
        client.position_mode = OKXPositionMode::LongShortMode;
        client.trade_mode = OKXTradeMode::Cross;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("ETH-USDT-SWAP.OKX"))
            .client_order_id(ClientOrderId::from("ODENYHEDGE"))
            .side(OrderSide::Sell)
            .price(Price::from("123.45"))
            .quantity(Quantity::from("2"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
        let mut command = SubmitOrder::from_order(
            &order,
            client.core.trader_id,
            Some(*OKX_CLIENT_ID),
            Some(PositionId::from("ETH-USDT-SWAP.OKX-LONG")),
            UUID4::new(),
            UnixNanos::default(),
        );

        if margin_conflict {
            command.position_id = None;
            let params =
                Params::new([("td_mode".to_string(), Value::String("cross".to_string()))].into());
            command.params = Some(params);
        }

        client.submit_order(command).unwrap();
        let event = rx.try_recv().unwrap();

        let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
            panic!("Expected denial, received {event:?}");
        };

        assert_eq!(denied.client_order_id, order.client_order_id());
        assert_eq!(
            denied.reason.as_str(),
            if margin_conflict {
                "VALIDATION_FAILED: OKX long/short mode requires configured trade mode Isolated, received Cross; mixed margin modes are not supported"
            } else {
                "INVALID_POSITION_ID: ETH-USDT-SWAP.OKX-LONG; expected OKX venue position ID ETH-USDT-SWAP.OKX-SHORT"
            }
        );
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_submit_order_list_denies_hedge_conflict_atomically(#[case] margin_conflict: bool) {
        let (mut client, cache) = build_test_exec_client_with_cache();
        client.position_mode = OKXPositionMode::LongShortMode;
        client.trade_mode = OKXTradeMode::Cross;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");

        let orders: Vec<_> = [OrderSide::Buy, OrderSide::Sell]
            .into_iter()
            .enumerate()
            .map(|(i, side)| {
                OrderTestBuilder::new(OrderType::Limit)
                    .instrument_id(instrument_id)
                    .client_order_id(ClientOrderId::from(format!("OHEDGE{i}")))
                    .side(side)
                    .price(Price::from("123.45"))
                    .quantity(Quantity::from("2"))
                    .build()
            })
            .collect();

        for order in &orders {
            cache
                .borrow_mut()
                .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
                .unwrap();
        }

        let order_list = OrderList::new(
            OrderListId::from("OL-HEDGE"),
            instrument_id,
            orders[0].strategy_id(),
            orders.iter().map(Order::client_order_id).collect(),
            UnixNanos::default(),
        );

        let mut command = SubmitOrderList::new(
            client.core.trader_id,
            Some(*OKX_CLIENT_ID),
            orders[0].strategy_id(),
            order_list,
            orders.iter().map(OrderInitialized::from).collect(),
            None,
            Some(PositionId::from("ETH-USDT-SWAP.OKX-LONG")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );

        if margin_conflict {
            command.position_id = None;
            let params =
                Params::new([("td_mode".to_string(), Value::String("cross".to_string()))].into());
            command.params = Some(params);
        }

        client.submit_order_list(command).unwrap();

        for order in orders {
            let event = rx.try_recv().unwrap();

            let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
                panic!("Expected denial, received {event:?}");
            };

            assert_eq!(denied.client_order_id, order.client_order_id());

            let expected = if margin_conflict {
                "VALIDATION_FAILED: OKX long/short mode requires configured trade mode Isolated, received Cross; mixed margin modes are not supported"
            } else if order.order_side() == OrderSide::Sell {
                "INVALID_POSITION_ID: ETH-USDT-SWAP.OKX-LONG; expected OKX venue position ID ETH-USDT-SWAP.OKX-SHORT"
            } else {
                "ORDER_LIST_DENIED: OL-HEDGE"
            };

            assert_eq!(denied.reason.as_str(), expected);
        }

        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
    }
}
