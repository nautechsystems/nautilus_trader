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

use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    live::runner::get_exec_event_sender,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
        GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
        SubmitOrderList,
    },
};
use nautilus_core::{
    UnixNanos,
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
        AccountType, OmsType, OrderStatus, OrderType, PositionSide, TimeInForce, TrailingOffsetType,
    },
    events::OrderDeniedReason,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    instruments::InstrumentAny,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            OKX_CONDITIONAL_ORDER_TYPES, OKX_RECONCILIATION_LOOKBACK_DEFAULT_MINS,
            OKX_RECONCILIATION_LOOKBACK_MAX_MINS, OKX_SUCCESS_CODE, OKX_VENUE,
            OKX_WS_HEARTBEAT_SECS, okx_reduce_only_wire_value, resolve_instrument_families,
            validate_okx_client_order_id,
        },
        enums::{OKXInstrumentType, OKXMarginMode, OKXTradeMode, is_advance_algo_order},
        failure::{classify_okx_http_failure, classify_okx_venue_code, classify_okx_ws_failure},
        parse::{
            is_okx_spread_symbol, is_order_status_report_more_advanced, nanos_to_datetime,
            okx_instrument_type_from_symbol,
        },
        task::{spawn_task, terminate_tasks},
    },
    config::OKXExecutionClientConfig,
    http::{
        client::{
            AlgoOrderReportSweep, FillHistory, OKXHttpClient, OKXPendingAlgoOrderReportsError,
            ReportInstrumentScope,
        },
        models::OKXCancelAlgoOrderRequest,
    },
    websocket::{
        client::OKXWebSocketClient,
        dispatch::{
            AlgoCancelContext, WsDispatchState, dispatch_ws_message, emit_algo_cancel_rejections,
        },
        messages::OKXWsMessage,
        parse::OrderStateSnapshot,
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
        params: &Option<Params>,
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

    fn instrument_types(&self) -> Vec<OKXInstrumentType> {
        if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        }
    }

    fn report_scope<'a>(
        &'a self,
        instrument_types: &'a [OKXInstrumentType],
    ) -> ReportInstrumentScope<'a> {
        ReportInstrumentScope {
            instrument_types,
            load_spreads: self.config.load_spreads,
        }
    }

    async fn collect_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
        require_complete_active_coverage: bool,
    ) -> anyhow::Result<OrderReportSweep> {
        let instrument_types = self.instrument_types();
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
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    start,
                    end,
                    false,
                    None,
                    Some(scope),
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

            if !is_spread_instrument(instrument_id)
                && supports_algo_orders(okx_instrument_type_from_symbol(
                    instrument_id.symbol.as_str(),
                ))
            {
                match self
                    .http_client
                    .request_algo_order_status_reports_sweep(
                        self.core.account_id,
                        None,
                        Some(instrument_id),
                        None,
                        None,
                        None,
                        None,
                        start,
                        end,
                        require_complete_active_coverage,
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
                        self.core.account_id,
                        Some(*inst_type),
                        None,
                        start,
                        end,
                        false,
                        None,
                        Some(scope),
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
                            self.core.account_id,
                            Some(*inst_type),
                            None,
                            None,
                            None,
                            None,
                            None,
                            start,
                            end,
                            require_complete_active_coverage,
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

            if self.config.load_spreads {
                match self
                    .http_client
                    .request_order_status_reports_scoped(
                        self.core.account_id,
                        None,
                        None,
                        start,
                        end,
                        false,
                        None,
                        Some(scope),
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
        }

        retain_order_status_reports(&mut reports, cmd);

        Ok(OrderReportSweep {
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
    ) -> anyhow::Result<(Vec<FillReport>, bool)> {
        let instrument_types = self.instrument_types();
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
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    start_dt,
                    end_dt,
                    None,
                    history,
                    Some(scope),
                )
                .await?;
            reports.extend(sweep.reports);
            complete &= sweep.complete;
        } else {
            for inst_type in &routing_types {
                let sweep = self
                    .http_client
                    .request_fill_reports_scoped(
                        self.core.account_id,
                        Some(*inst_type),
                        None,
                        start_dt,
                        end_dt,
                        None,
                        history,
                        Some(scope),
                    )
                    .await?;
                reports.extend(sweep.reports);
                complete &= sweep.complete;
            }

            if self.config.load_spreads {
                let sweep = self
                    .http_client
                    .request_fill_reports_scoped(
                        self.core.account_id,
                        None,
                        None,
                        start_dt,
                        end_dt,
                        None,
                        history,
                        Some(scope),
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
        let instrument_types = self.instrument_types();
        let scope = self.report_scope(&instrument_types);
        let mut reports = Vec::new();
        let mut complete = true;

        if let Some(instrument_id) = cmd.instrument_id {
            if is_spread_instrument(instrument_id) {
                return Ok((reports, complete));
            }

            let inst_type = okx_instrument_type_from_symbol(instrument_id.symbol.as_str());
            if inst_type != OKXInstrumentType::Spot && inst_type != OKXInstrumentType::Margin {
                let sweep = self
                    .http_client
                    .request_position_status_reports_scoped(
                        self.core.account_id,
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
                        self.core.account_id,
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
            .request_spot_margin_position_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            margin_reports.retain(|report| report.instrument_id == instrument_id);
        }

        reports.append(&mut margin_reports);

        Ok((reports, complete))
    }

    fn update_account_state(&self) {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(account_id)
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
            if is_spread_instrument(instrument_id) {
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

        if is_spread_instrument(instrument_id) {
            Ok(OrderCommandRoute::SpreadHttp)
        } else {
            Ok(OrderCommandRoute::RegularWs)
        }
    }

    fn cancel_order_route(
        &self,
        instrument_id: InstrumentId,
        order_state: Option<(OrderType, Option<bool>)>,
        has_bound_child: bool,
    ) -> OrderCommandRoute {
        if is_spread_instrument(instrument_id) {
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

    fn cancel_all_orders_route(&self, instrument_id: InstrumentId) -> CancelAllOrdersRoute {
        if is_spread_instrument(instrument_id) {
            CancelAllOrdersRoute::SpreadHttp
        } else if self.config.use_mm_mass_cancel {
            CancelAllOrdersRoute::MassCancelHttp
        } else {
            CancelAllOrdersRoute::BatchWs
        }
    }

    fn submit_regular_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache.try_order_owned(&cmd.client_order_id)?
        };
        let ws_private = self.ws_private.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, &cmd.params);

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

        let px_usd = get_param_as_string(&cmd.params, "px_usd");
        let px_vol = get_param_as_string(&cmd.params, "px_vol");
        let speed_bump = get_param_as_string(&cmd.params, "speed_bump");
        let outcome = get_param_as_string(&cmd.params, "outcome");
        let slippage_pct = get_param_as_string(&cmd.params, "slippage_pct");
        let rpi = get_param_as_bool(&cmd.params, "rpi");
        let rpi_taker_access = get_param_as_bool(&cmd.params, "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(&cmd.params, "rpi_px_round");

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
                    None,
                    None,
                    px_usd,
                    px_vol,
                    speed_bump,
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

    fn submit_order_http(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache.try_order_owned(&cmd.client_order_id)?
        };
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, &cmd.params);

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
        let rpi = get_param_as_bool(&cmd.params, "rpi");
        let rpi_taker_access = get_param_as_bool(&cmd.params, "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(&cmd.params, "rpi_px_round");

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
                    None,
                    None,
                    None,
                    None,
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

    fn submit_conditional_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache.try_order_owned(&cmd.client_order_id)?
        };
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, &cmd.params);

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

        let trailing_offset = order.trailing_offset();
        let trailing_offset_type = order.trailing_offset_type();
        let activation_price = order.activation_price();

        let close_fraction = get_param_as_string(&cmd.params, "close_fraction");
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
                    trigger_price,
                    trigger_type,
                    price,
                    reduce_only,
                    close_fraction,
                    callback_ratio,
                    callback_spread,
                    activation_price,
                )
                .await;

            match result {
                Ok(response) => {
                    dispatch_state.bind_algo_parent(
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

    fn cancel_ws_order(&self, cmd: &CancelOrder) {
        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);

        let ws_private = self.ws_private.clone();
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
                emit_cancel_failure(
                    classify_okx_ws_failure(&e),
                    None,
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
                                .then_some((code, r.s_msg.as_deref().unwrap_or("unknown")))
                        })
                    }) {
                        let reason =
                            format!("cancel-algo-order-rejected: s_code={code}, s_msg={msg}");
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
        if is_spread_instrument(instrument_id) {
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
        let order_identity = cache.order(&client_order_id).map(|order| OrderIdentity {
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

        if !regular_requests.is_empty() {
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

        if !advance_requests.is_empty() {
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
            tokio::time::sleep(interval).await;

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
                        log::warn!("No instruments returned for {instrument_type:?}");
                        continue;
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
                            log::warn!(
                                "No instruments returned for {instrument_type:?} family {family}"
                            );
                            continue;
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

            self.ws_private.cache_instruments(&all_instruments);
            self.ws_private
                .cache_inst_id_codes(all_inst_id_codes.clone());
            self.ws_business.cache_instruments(&all_instruments);
            self.ws_business.cache_inst_id_codes(all_inst_id_codes);
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
            let instruments = self.ws_private.instruments_cache_arc();
            let tasks = self
                .session_tasks
                .spawner()
                .context("OKX execution stream task admission is closed")?;
            let cancel = tasks.cancellation_token();
            let clock = self.clock;

            spawn_task(&tasks, async move {
                let mut fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
                let mut filled_qty_cache: AHashMap<Ustr, Quantity> = AHashMap::new();
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
            let instruments = self.ws_business.instruments_cache_arc();
            let tasks = self
                .session_tasks
                .spawner()
                .context("OKX execution stream task admission is closed")?;
            let cancel = tasks.cancellation_token();
            let clock = self.clock;

            spawn_task(&tasks, async move {
                let mut fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
                let mut filled_qty_cache: AHashMap<Ustr, Quantity> = AHashMap::new();
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
            .request_account_state(self.core.account_id)
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
                            log::error!("OKX query_order failed to fetch spread order: {e}");
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
                            log::error!("OKX query_order failed to fetch regular order: {e}");
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
                            log::error!(
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

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            anyhow::bail!("generate_order_status_report requires instrument_id");
        };

        if cmd.client_order_id.is_none() && cmd.venue_order_id.is_none() {
            anyhow::bail!(
                "generate_order_status_report requires client_order_id or venue_order_id"
            );
        }

        let order_state = {
            let cache = self.core.cache();
            cmd.client_order_id.and_then(|client_order_id| {
                cache
                    .order(&client_order_id)
                    .map(|order| CachedQueryOrderState {
                        order_type: order.order_type(),
                        venue_order_id: order.venue_order_id(),
                    })
            })
        };
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
                        self.core.account_id,
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
                            self.core.account_id,
                            instrument_id,
                            venue_order_id,
                        )
                        .await
                } else if let Some(client_order_id) = cmd.client_order_id {
                    self.http_client
                        .request_order_status_report(
                            self.core.account_id,
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
                    self.core.account_id,
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

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        Ok(self.collect_order_status_reports(cmd, false).await?.reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        Ok(self.collect_fill_reports(cmd, FillHistory::Recent).await?.0)
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
        let lookback_ns = lookback_mins * 60 * 1_000_000_000;
        let start = Some(UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns)));

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
            self.collect_order_status_reports(&order_cmd, true),
            self.collect_fill_reports(fill_cmd, fill_history),
            self.collect_position_status_reports(&position_cmd),
        )?;
        let OrderReportSweep {
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
        let route = {
            let cache = self.core.cache();
            let order = cache.try_order(&cmd.client_order_id)?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            let trade_mode = self.trade_mode_for_order(cmd.instrument_id, &cmd.params);
            if let Err(reason) = validate_order(&*order, trade_mode, OrderSubmission::Single) {
                self.emitter.emit_order_denied(&order, &reason.to_string());
                return Ok(());
            }

            let order_type = order.order_type();
            let route = self.submit_order_route(cmd.instrument_id, order_type)?;

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(&order);

            route
        };

        match route {
            OrderCommandRoute::RegularWs => self.submit_regular_order(&cmd),
            OrderCommandRoute::AlgoHttp => self.submit_conditional_order(&cmd),
            OrderCommandRoute::SpreadHttp => self.submit_order_http(&cmd),
        }
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        if is_spread_instrument(cmd.instrument_id) {
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
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, &cmd.params);

        // Validate all orders before emitting any submitted events
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        // Pre-validate every order so an invalid leg denies the whole list atomically;
        // otherwise sibling legs would be left in the cache without a terminal event.
        let invalid: Vec<(ClientOrderId, OrderDeniedReason)> = orders
            .iter()
            .filter_map(|order| {
                validate_order(order, trade_mode, OrderSubmission::List)
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
        let speed_bump = get_param_as_string(&cmd.params, "speed_bump");
        let outcome = get_param_as_string(&cmd.params, "outcome");
        let rpi = get_param_as_bool(&cmd.params, "rpi");
        let rpi_taker_access = get_param_as_bool(&cmd.params, "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(&cmd.params, "rpi_px_round");

        for order in &orders {
            let context = OrderContext::from(order);

            batch_orders.push((
                inst_type,
                cmd.instrument_id,
                trade_mode,
                context.identity.client_order_id,
                context.identity.order_side,
                None, // position_side: WS client defaults to Net for derivatives
                context.identity.order_type,
                context.quantity,
                context.price,
                context.trigger_price,
                Some(context.is_post_only),
                Some(context.is_reduce_only),
                speed_bump.clone(),
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
        if is_spread_instrument(cmd.instrument_id) {
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

        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);

        let ws_private = self.ws_private.clone();
        let mut command = cmd.clone();
        command.venue_order_id = self
            .ws_dispatch_state
            .order_venue_binding(cmd.client_order_id)
            .map(|(venue_order_id, _)| venue_order_id)
            .or(cmd.venue_order_id);

        let new_px_usd = get_param_as_string(&cmd.params, "px_usd");
        let new_px_vol = get_param_as_string(&cmd.params, "px_vol");
        let speed_bump = get_param_as_string(&cmd.params, "speed_bump");
        let rpi_taker_access = get_param_as_bool(&cmd.params, "rpi_taker_access");
        let rpi_px_round = get_param_as_bool(&cmd.params, "rpi_px_round");

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
                    speed_bump,
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
            self.cancel_order_route(
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
        match self.cancel_all_orders_route(cmd.instrument_id) {
            CancelAllOrdersRoute::SpreadHttp | CancelAllOrdersRoute::MassCancelHttp => {
                self.mass_cancel_instrument(cmd.instrument_id);
                Ok(())
            }
            CancelAllOrdersRoute::BatchWs => {
                let cache = self.core.cache();
                let open_orders =
                    cache.orders_open(None, Some(&cmd.instrument_id), None, None, None);

                if open_orders.is_empty() {
                    log::debug!("No open orders to cancel for {}", cmd.instrument_id);
                    return Ok(());
                }

                let mut regular_payload = Vec::new();
                let mut regular_cancel_contexts = Vec::new();
                let mut algo_orders: Vec<(
                    InstrumentId,
                    ClientOrderId,
                    Option<VenueOrderId>,
                    TraderId,
                    StrategyId,
                )> = Vec::new();

                for order in &open_orders {
                    let order_state = Some((order.order_type(), order.is_triggered()));
                    let venue_binding = self
                        .ws_dispatch_state
                        .order_venue_binding(order.client_order_id());
                    let authoritative_venue_order_id = venue_binding
                        .map(|(venue_order_id, _)| venue_order_id)
                        .or(order.venue_order_id());
                    match self.cancel_order_route(
                        order.instrument_id(),
                        order_state,
                        venue_binding.is_some_and(|(_, has_bound_child)| has_bound_child),
                    ) {
                        OrderCommandRoute::RegularWs => {
                            self.ensure_order_identity(
                                order.client_order_id(),
                                order.strategy_id(),
                                order.instrument_id(),
                            );
                            regular_payload.push((
                                order.instrument_id(),
                                Some(order.client_order_id()),
                                authoritative_venue_order_id,
                            ));
                            regular_cancel_contexts.push((
                                order.client_order_id(),
                                order.instrument_id(),
                                order.strategy_id(),
                            ));
                        }
                        OrderCommandRoute::AlgoHttp => {
                            algo_orders.push((
                                order.instrument_id(),
                                order.client_order_id(),
                                authoritative_venue_order_id,
                                order.trader_id(),
                                order.strategy_id(),
                            ));
                        }
                        OrderCommandRoute::SpreadHttp => {}
                    }
                }
                drop(open_orders);
                drop(cache);

                log::debug!(
                    "Canceling {} regular orders and {} algo orders for {}",
                    regular_payload.len(),
                    algo_orders.len(),
                    cmd.instrument_id
                );

                if !regular_payload.is_empty() {
                    let ws_private = self.ws_private.clone();

                    self.spawn_task("batch_cancel_orders", async move {
                        if let Err(e) = ws_private.batch_cancel_orders(regular_payload).await {
                            log_batch_cancel_failure(
                                classify_okx_ws_failure(&e),
                                regular_cancel_contexts.len(),
                            );
                            return Err(anyhow::Error::new(e).context("batch cancel orders failed"));
                        }
                        Ok(())
                    });
                }

                // OKX doesn't support algo cancel via private WebSocket, must use HTTP
                if !algo_orders.is_empty() {
                    let items: Vec<_> = algo_orders
                        .into_iter()
                        .map(
                            |(
                                instrument_id,
                                client_order_id,
                                venue_order_id,
                                _trader_id,
                                strategy_id,
                            )| {
                                let request = OKXCancelAlgoOrderRequest {
                                    inst_id: instrument_id.symbol.to_string(),
                                    inst_id_code: None,
                                    algo_id: venue_order_id.map(|id| id.to_string()),
                                    algo_cl_ord_id: if venue_order_id.is_none() {
                                        Some(client_order_id.to_string())
                                    } else {
                                        None
                                    },
                                };
                                let ctx = AlgoCancelContext {
                                    client_order_id,
                                    instrument_id,
                                    strategy_id,
                                    venue_order_id,
                                };
                                (request, ctx)
                            },
                        )
                        .collect();
                    self.dispatch_algo_cancels(items);
                }

                Ok(())
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
            match self.cancel_order_route(
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
            let cancel_contexts: Vec<_> = cmd
                .cancels
                .iter()
                .filter(|c| {
                    regular_payload
                        .iter()
                        .any(|(_, cid, _)| *cid == Some(c.client_order_id))
                })
                .map(|c| (c.client_order_id, c.instrument_id, c.strategy_id))
                .collect();

            self.spawn_task("batch_cancel_orders", async move {
                if let Err(e) = ws_private.batch_cancel_orders(regular_payload).await {
                    log_batch_cancel_failure(classify_okx_ws_failure(&e), cancel_contexts.len());
                    return Err(anyhow::Error::new(e).context("batch cancel orders failed"));
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
    reports: Vec<OrderStatusReport>,
    complete: bool,
    ambiguous_triggered_child_ids: AHashSet<VenueOrderId>,
    regular_by_venue_order_id: AHashMap<VenueOrderId, OrderStatusReport>,
}

impl OKXExecutionClient {
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
        let mut hedging_instrument_ids = AHashSet::new();

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
            }
        }

        instrument_ids.retain(|instrument_id| {
            !is_spread_instrument(*instrument_id)
                && matches!(
                    okx_instrument_type_from_symbol(instrument_id.symbol.as_str()),
                    OKXInstrumentType::Swap
                        | OKXInstrumentType::Futures
                        | OKXInstrumentType::Option
                )
                && !hedging_instrument_ids.contains(instrument_id)
                && !position_reports
                    .iter()
                    .any(|report| report.instrument_id == *instrument_id)
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
                );
                complete = false;
                continue;
            }

            if request_count >= MAX_TRIGGERED_CHILD_RECOVERIES {
                self.push_external_regular_order_fallback(
                    &parent_report,
                    regular_by_venue_order_id,
                    &mut recovered,
                );
                complete = false;
                continue;
            }
            request_count += 1;

            match self
                .http_client
                .request_order_status_report_by_venue_order_id(
                    self.core.account_id,
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
    ) {
        let cache = self.core.cache();
        let cached_by_client = parent_report
            .client_order_id
            .is_some_and(|client_order_id| cache.order_exists(&client_order_id));
        let cached_by_venue = cache
            .client_order_id(&parent_report.venue_order_id)
            .is_some_and(|client_order_id| cache.order_exists(client_order_id));

        if !cached_by_client
            && !cached_by_venue
            && let Some(regular_report) =
                regular_by_venue_order_id.get(&parent_report.venue_order_id)
        {
            recovered.push(regular_report.clone());
        }
    }
}

fn validate_order(
    order: &impl Order,
    trade_mode: OKXTradeMode,
    submission: OrderSubmission,
) -> Result<(), OrderDeniedReason> {
    if let Err(detail) = validate_okx_client_order_id(order.client_order_id().as_str()) {
        return Err(OrderDeniedReason::InvalidClientOrderId { detail });
    }

    if is_spread_instrument(order.instrument_id()) && order.is_reduce_only() {
        return Err(OrderDeniedReason::UnsupportedReduceOnly);
    }

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
    BatchWs,
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

fn get_param_as_string(params: &Option<Params>, key: &str) -> Option<String> {
    params.as_ref().and_then(|p| {
        p.get(key).and_then(|v| {
            v.as_str()
                .map(ToString::to_string)
                .or_else(|| v.as_f64().map(|n| n.to_string()))
        })
    })
}

fn get_param_as_bool(params: &Option<Params>, key: &str) -> Option<bool> {
    params.as_ref().and_then(|params| params.get_bool(key))
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
) -> QueryOrderRoute {
    if is_spread_instrument(instrument_id) {
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

fn is_spread_instrument(instrument_id: InstrumentId) -> bool {
    is_okx_spread_symbol(instrument_id.symbol.as_str())
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
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use axum::{Json, Router, routing::post};
    use nautilus_common::{cache::Cache, messages::ExecutionEvent, testing::wait_until_async};
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{OrderSide, OrderStatus},
        events::OrderEventAny,
        instruments::Instrument,
        orders::OrderTestBuilder,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::common::consts::OKX_CLIENT_ID;

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
            query_order_route(InstrumentId::from("BTC-USDT.OKX"), Some(order_type), false,),
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
            ),
            QueryOrderRoute::RegularAndAlgo,
        );
    }

    #[rstest]
    fn test_query_order_route_for_unknown_order_type() {
        assert_eq!(
            query_order_route(InstrumentId::from("BTC-USDT.OKX"), None, false),
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
            ),
            QueryOrderRoute::Spread,
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
            validate_order(&order, OKXTradeMode::Cross, OrderSubmission::Single),
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
            validate_order(&order, OKXTradeMode::Cross, OrderSubmission::List),
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
            validate_order(&order, OKXTradeMode::Cross, OrderSubmission::List),
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
            validate_order(&order, trade_mode, OrderSubmission::Single),
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
        let mut params = Params::new();
        params.insert(
            "td_mode".to_string(),
            Value::String(td_mode_value.to_string()),
        );

        let result = get_param_as_string(&Some(params), "td_mode")
            .and_then(|s| s.parse::<OKXTradeMode>().ok());

        assert_eq!(result, Some(expected));
    }

    #[rstest]
    fn test_td_mode_param_invalid_falls_through() {
        let mut params = Params::new();
        params.insert("td_mode".to_string(), Value::String("invalid".to_string()));

        let result = get_param_as_string(&Some(params), "td_mode")
            .and_then(|s| s.parse::<OKXTradeMode>().ok());

        assert_eq!(result, None);
    }

    #[rstest]
    fn test_td_mode_param_absent_falls_through() {
        let result = get_param_as_string(&None, "td_mode");

        assert_eq!(result, None);
    }

    #[rstest]
    fn test_close_fraction_present_sets_reduce_only_true() {
        let mut params = Params::new();
        params.insert("close_fraction".to_string(), Value::String("1".to_string()));
        let params = Some(params);

        let close_fraction = get_param_as_string(&params, "close_fraction");
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

        let close_fraction = get_param_as_string(&params, "close_fraction");
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

        let close_fraction = get_param_as_string(&params, "close_fraction");
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
    fn test_cancel_order_route_uses_bound_child_before_engine_trigger_state_updates() {
        let client = build_test_exec_client();
        let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
        let pending_trigger = Some((OrderType::StopLimit, Some(false)));

        assert_eq!(
            client.cancel_order_route(instrument_id, pending_trigger, false),
            OrderCommandRoute::AlgoHttp
        );
        assert_eq!(
            client.cancel_order_route(instrument_id, pending_trigger, true),
            OrderCommandRoute::RegularWs
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
            private_cache.load().get(&symbol).map(|i| i.id()),
            Some(instrument.id()),
        );
        assert_eq!(
            business_cache.load().get(&symbol).map(|i| i.id()),
            Some(instrument.id()),
        );
    }
}
