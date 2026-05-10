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
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
        GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
        SubmitOrderList,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    params::Params,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce, TrailingOffsetType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            OKX_CONDITIONAL_ORDER_TYPES, OKX_SUCCESS_CODE, OKX_VENUE, OKX_WS_HEARTBEAT_SECS,
            resolve_instrument_families,
        },
        enums::{OKXInstrumentType, OKXMarginMode, OKXTradeMode, is_advance_algo_order},
        parse::{nanos_to_datetime, okx_instrument_type_from_symbol},
    },
    config::OKXExecClientConfig,
    http::{client::OKXHttpClient, models::OKXCancelAlgoOrderRequest},
    websocket::{
        client::OKXWebSocketClient,
        dispatch::{
            AlgoCancelContext, OrderIdentity, WsDispatchState, dispatch_ws_message,
            emit_algo_cancel_rejections, emit_batch_cancel_failure,
        },
        parse::OrderStateSnapshot,
    },
};

fn get_param_as_string(params: &Option<Params>, key: &str) -> Option<String> {
    params.as_ref().and_then(|p| {
        p.get(key).and_then(|v| {
            v.as_str()
                .map(ToString::to_string)
                .or_else(|| v.as_f64().map(|n| n.to_string()))
        })
    })
}

#[derive(Debug)]
pub struct OKXExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: OKXExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: OKXHttpClient,
    ws_private: OKXWebSocketClient,
    ws_business: OKXWebSocketClient,
    trade_mode: OKXTradeMode,
    ws_stream_handle: Option<JoinHandle<()>>,
    ws_business_stream_handle: Option<JoinHandle<()>>,
    ws_dispatch_state: Arc<WsDispatchState>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl OKXExecutionClient {
    /// Creates a new [`OKXExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: OKXExecClientConfig) -> anyhow::Result<Self> {
        let http_client = OKXHttpClient::with_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.environment,
            config.proxy_url.clone(),
        )?;

        let account_id = core.account_id;

        let ws_private = OKXWebSocketClient::with_credentials(
            Some(config.ws_private_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
            Some(account_id),
            Some(OKX_WS_HEARTBEAT_SECS),
            None,
            config.transport_backend,
            config.proxy_url.clone(),
        )
        .context("failed to construct OKX private websocket client")?;

        let ws_business = OKXWebSocketClient::with_credentials(
            Some(config.ws_business_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
            Some(account_id),
            Some(OKX_WS_HEARTBEAT_SECS),
            None,
            config.transport_backend,
            config.proxy_url.clone(),
        )
        .context("failed to construct OKX business websocket client")?;

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
            ws_stream_handle: None,
            ws_business_stream_handle: None,
            ws_dispatch_state,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    fn derive_default_trade_mode(
        account_type: AccountType,
        config: &OKXExecClientConfig,
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

    fn submit_regular_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache
                .order(&cmd.client_order_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?
        };
        let ws_private = self.ws_private.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, &cmd.params);

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let trader_id = self.core.trader_id;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();

        self.ws_dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side: order.order_side(),
                order_type: order.order_type(),
            },
        );
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let is_post_only = order.is_post_only();
        let is_reduce_only = order.is_reduce_only();
        let is_quote_quantity = order.is_quote_quantity();

        let px_usd = get_param_as_string(&cmd.params, "px_usd");
        let px_vol = get_param_as_string(&cmd.params, "px_vol");

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
                )
                .await
                .map_err(|e| anyhow::anyhow!("Submit order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &format!("submit-order-error: {e}"),
                    ts_event,
                    false,
                );
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_conditional_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache
                .order(&cmd.client_order_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?
        };
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode_for_order(cmd.instrument_id, &cmd.params);

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();

        self.ws_dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side,
                order_type,
            },
        );
        let quantity = order.quantity();
        let trigger_type = order.trigger_type();
        let trigger_price = order.trigger_price();
        let price = order.price();
        let is_reduce_only = order.is_reduce_only();

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
                .await
                .map_err(|e| anyhow::anyhow!("Submit algo order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &format!("submit-order-error: {e}"),
                    ts_event,
                    false,
                );
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_ws_order(&self, cmd: &CancelOrder) {
        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);

        let ws_private = self.ws_private.clone();
        let command = cmd.clone();

        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_order", async move {
            let result = ws_private
                .cancel_order(
                    command.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    Some(command.client_order_id),
                    command.venue_order_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Cancel order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    &format!("cancel-order-error: {e}"),
                    ts_event,
                );
                return Err(e);
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
                http_client
                    .cancel_advance_algo_orders(vec![request])
                    .await
                    .map_err(|e| anyhow::anyhow!("Cancel advance algo order failed: {e}"))
            } else {
                http_client
                    .cancel_algo_orders(vec![request])
                    .await
                    .map_err(|e| anyhow::anyhow!("Cancel algo order failed: {e}"))
            };

            let reject_reason = match &responses {
                Err(e) => Some(format!("cancel-algo-order-error: {e}")),
                Ok(resps) => {
                    // Check per-order business status code
                    resps.first().and_then(|r| {
                        r.s_code.as_deref().and_then(|code| {
                            if code == OKX_SUCCESS_CODE {
                                None
                            } else {
                                let msg = r.s_msg.as_deref().unwrap_or("unknown");
                                Some(format!(
                                    "cancel-algo-order-rejected: s_code={code}, s_msg={msg}"
                                ))
                            }
                        })
                    })
                }
            };

            if let Some(reason) = reject_reason {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    &reason,
                    ts_event,
                );
                anyhow::bail!("{reason}");
            }

            Ok(())
        });
    }

    fn mass_cancel_instrument(&self, instrument_id: InstrumentId) {
        let ws_private = self.ws_private.clone();

        self.spawn_task("mass_cancel_orders", async move {
            ws_private.mass_cancel_orders(instrument_id).await?;
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
        self.ws_dispatch_state
            .order_identities
            .entry(client_order_id)
            .or_insert_with(|| {
                let cache = self.core.cache();
                let (order_side, order_type) = cache
                    .order(&client_order_id)
                    .map_or((OrderSide::NoOrderSide, OrderType::Market), |o| {
                        (o.order_side(), o.order_type())
                    });
                drop(cache);

                OrderIdentity {
                    instrument_id,
                    strategy_id,
                    order_side,
                    order_type,
                }
            });
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
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
                        let msg = format!("{e}");
                        emit_batch_cancel_failure(&regular_contexts, &msg, &emitter, clock);
                        anyhow::bail!("{e}");
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
                        let msg = format!("{e}");
                        emit_batch_cancel_failure(&advance_contexts, &msg, &emitter, clock);
                        anyhow::bail!("{e}");
                    }
                }
                Ok(())
            });
        }
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);

        for handle in tasks.drain(..) {
            handle.abort();
        }
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
        self.core.cache().account(&self.core.account_id).cloned()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
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

                    log::info!(
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

                        log::info!(
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

        if self.ws_stream_handle.is_none() {
            let stream = self.ws_private.stream();
            let emitter = self.emitter.clone();
            let state = Arc::clone(&self.ws_dispatch_state);
            let account_id = self.core.account_id;
            let instruments = self.ws_private.instruments_snapshot();
            let clock = self.clock;

            let handle = get_runtime().spawn(async move {
                let mut fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
                let mut filled_qty_cache: AHashMap<Ustr, Quantity> = AHashMap::new();
                let mut order_state_cache: AHashMap<ClientOrderId, OrderStateSnapshot> =
                    AHashMap::new();

                pin_mut!(stream);

                while let Some(message) = stream.next().await {
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
            });
            self.ws_stream_handle = Some(handle);
        }

        self.ws_business.connect().await?;
        self.ws_business.wait_until_active(10.0).await?;
        log::info!("Connected to business WebSocket");

        if self.ws_business_stream_handle.is_none() {
            let stream = self.ws_business.stream();
            let emitter = self.emitter.clone();
            let state = Arc::clone(&self.ws_dispatch_state);
            let account_id = self.core.account_id;
            let instruments = self.ws_business.instruments_snapshot();
            let clock = self.clock;

            let handle = get_runtime().spawn(async move {
                let mut fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
                let mut filled_qty_cache: AHashMap<Ustr, Quantity> = AHashMap::new();
                let mut order_state_cache: AHashMap<ClientOrderId, OrderStateSnapshot> =
                    AHashMap::new();

                pin_mut!(stream);

                while let Some(message) = stream.next().await {
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
            });

            self.ws_business_stream_handle = Some(handle);
        }

        for inst_type in &instrument_types {
            log::info!("Subscribing to orders channel for {inst_type:?}");
            self.ws_private.subscribe_orders(*inst_type).await?;

            if self.config.use_fills_channel {
                log::info!("Subscribing to fills channel for {inst_type:?}");
                if let Err(e) = self.ws_private.subscribe_fills(*inst_type).await {
                    log::warn!("Failed to subscribe to fills channel ({inst_type:?}): {e}");
                }
            }
        }

        self.ws_private.subscribe_account().await?;

        // Subscribe to algo orders on business WebSocket (OKX requires this endpoint)
        for inst_type in &instrument_types {
            if *inst_type != OKXInstrumentType::Option {
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
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }
        self.emitter.send_account_state(account_state);

        // Wait for account to be registered in cache before completing connect
        self.await_account_registered(30.0).await?;

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        self.abort_pending_tasks();
        self.http_client.cancel_all_requests();

        if let Err(e) = self.ws_private.close().await {
            log::warn!("Error closing private websocket: {e:?}");
        }

        if let Err(e) = self.ws_business.close().await {
            log::warn!("Error closing business websocket: {e:?}");
        }

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.ws_business_stream_handle.take() {
            handle.abort();
        }

        self.core.set_disconnected();
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

        self.spawn_task("query_order", async move {
            let mut reports = match http_client
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
                Ok(r) => r,
                Err(e) => {
                    log::error!("OKX query_order failed to fetch regular orders: {e}");
                    Vec::new()
                }
            };

            // Merge algo orders (stop, OCO, TP/SL, trailing) so query_order can
            // resolve conditional orders as well.
            match http_client
                .request_algo_order_status_reports(
                    account_id,
                    None,
                    Some(instrument_id),
                    None,
                    Some(client_order_id),
                    None,
                    None,
                )
                .await
            {
                Ok(mut algo) => reports.append(&mut algo),
                Err(e) => {
                    log::warn!("OKX query_order algo lookup failed for {instrument_id}: {e}");
                }
            }

            let Some(report) = select_query_order_report(reports, client_order_id, venue_order_id)
            else {
                log::warn!(
                    "OKX query_order found no order for client_order_id={client_order_id}, venue_order_id={venue_order_id:?}",
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
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        let http_client = self.http_client.clone();
        let ws_private = self.ws_private.clone();
        let ws_business = self.ws_business.clone();
        let instrument_types = self.config.instrument_types.clone();
        let instrument_families = self.config.instrument_families.clone();

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();
            let mut all_inst_id_codes = Vec::new();

            for instrument_type in instrument_types {
                let Some(families) =
                    resolve_instrument_families(&instrument_families, instrument_type)
                else {
                    continue;
                };

                if families.is_empty() {
                    match http_client.request_instruments(instrument_type, None).await {
                        Ok((instruments, inst_id_codes)) => {
                            if instruments.is_empty() {
                                log::warn!("No instruments returned for {instrument_type:?}");
                                continue;
                            }
                            http_client.cache_instruments(&instruments);
                            all_instruments.extend(instruments);
                            all_inst_id_codes.extend(inst_id_codes);
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to request instruments for {instrument_type:?}: {e}"
                            );
                        }
                    }
                } else {
                    for family in &families {
                        match http_client
                            .request_instruments(instrument_type, Some(family.clone()))
                            .await
                        {
                            Ok((instruments, inst_id_codes)) => {
                                if instruments.is_empty() {
                                    log::warn!(
                                        "No instruments returned for {instrument_type:?} family {family}"
                                    );
                                    continue;
                                }
                                http_client.cache_instruments(&instruments);
                                all_instruments.extend(instruments);
                                all_inst_id_codes.extend(inst_id_codes);
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to request instruments for {instrument_type:?} family {family}: {e}"
                                );
                            }
                        }
                    }
                }
            }

            if all_instruments.is_empty() {
                log::error!(
                    "Instrument bootstrap yielded no instruments, order submissions will fail"
                );
            } else {
                ws_private.cache_instruments(&all_instruments);
                ws_private.cache_inst_id_codes(all_inst_id_codes.clone());
                ws_business.cache_instruments(&all_instruments);
                ws_business.cache_inst_id_codes(all_inst_id_codes);
                log::info!("Instruments initialized");
            }
        });

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, trade_mode={:?}, instrument_types={:?}, use_fills_channel={}, environment={}, proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.trade_mode,
            self.config.instrument_types,
            self.config.use_fills_channel,
            self.config.environment,
            self.config.proxy_url,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.ws_business_stream_handle.take() {
            handle.abort();
        }
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
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

        let mut reports = self
            .http_client
            .request_order_status_reports(
                self.core.account_id,
                None,
                Some(instrument_id),
                None,
                None,
                false,
                None,
            )
            .await?;

        // Merge algo orders (stop, OCO, TP/SL, trailing). They live on a
        // separate OKX endpoint and would otherwise be dropped from
        // reconciliation, leaving stop/conditional orders unrecovered after
        // a restart.
        match self
            .http_client
            .request_algo_order_status_reports(
                self.core.account_id,
                None,
                Some(instrument_id),
                None,
                cmd.client_order_id,
                None,
                None,
            )
            .await
        {
            Ok(mut algo_reports) => reports.append(&mut algo_reports),
            Err(e) => {
                log::warn!("Failed to fetch algo order status reports for {instrument_id}: {e}");
            }
        }

        if let Some(client_order_id) = cmd.client_order_id {
            reports.retain(|report| report.client_order_id == Some(client_order_id));
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports.into_iter().next())
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_order_status_reports(
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    None,
                    None,
                    false,
                    None,
                )
                .await?;
            reports.append(&mut fetched);

            // Merge algo orders for the requested instrument so reconciliation
            // recovers stop, OCO, TP/SL, and trailing orders alongside regular
            // ones. Failure here is logged but does not abort the regular
            // reconciliation; an algo-endpoint outage should not blank the
            // entire status report.
            match self
                .http_client
                .request_algo_order_status_reports(
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    None,
                    None,
                    None,
                    None,
                )
                .await
            {
                Ok(mut algo) => reports.append(&mut algo),
                Err(e) => {
                    log::warn!(
                        "Failed to fetch algo order status reports for {instrument_id}: {e}"
                    );
                }
            }
        } else {
            for inst_type in self.instrument_types() {
                let mut fetched = self
                    .http_client
                    .request_order_status_reports(
                        self.core.account_id,
                        Some(inst_type),
                        None,
                        None,
                        None,
                        false,
                        None,
                    )
                    .await?;
                reports.append(&mut fetched);

                match self
                    .http_client
                    .request_algo_order_status_reports(
                        self.core.account_id,
                        Some(inst_type),
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                    .await
                {
                    Ok(mut algo) => reports.append(&mut algo),
                    Err(e) => log::warn!(
                        "Failed to fetch algo order status reports for {inst_type:?}: {e}"
                    ),
                }
            }
        }

        if cmd.open_only {
            reports.retain(|r| r.order_status.is_open());
        }

        if let Some(start) = cmd.start {
            reports.retain(|r| r.ts_last >= start);
        }

        if let Some(end) = cmd.end {
            reports.retain(|r| r.ts_last <= end);
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let start_dt = nanos_to_datetime(cmd.start);
        let end_dt = nanos_to_datetime(cmd.end);
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_fill_reports(
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    start_dt,
                    end_dt,
                    None,
                )
                .await?;
            reports.append(&mut fetched);
        } else {
            for inst_type in self.instrument_types() {
                let mut fetched = self
                    .http_client
                    .request_fill_reports(
                        self.core.account_id,
                        Some(inst_type),
                        None,
                        start_dt,
                        end_dt,
                        None,
                    )
                    .await?;
                reports.append(&mut fetched);
            }
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut reports = Vec::new();

        // Query derivative positions (SWAP/FUTURES/OPTION) from /api/v5/account/positions
        // Note: The positions endpoint does not support Spot or Margin - those are handled separately
        if let Some(instrument_id) = cmd.instrument_id {
            let inst_type = okx_instrument_type_from_symbol(instrument_id.symbol.as_str());
            if inst_type != OKXInstrumentType::Spot && inst_type != OKXInstrumentType::Margin {
                let mut fetched = self
                    .http_client
                    .request_position_status_reports(
                        self.core.account_id,
                        None,
                        Some(instrument_id),
                    )
                    .await?;
                reports.append(&mut fetched);
            }
        } else {
            for inst_type in self.instrument_types() {
                // Skip Spot and Margin - positions API only supports derivatives
                if inst_type == OKXInstrumentType::Spot || inst_type == OKXInstrumentType::Margin {
                    continue;
                }
                let mut fetched = self
                    .http_client
                    .request_position_status_reports(self.core.account_id, Some(inst_type), None)
                    .await?;
                reports.append(&mut fetched);
            }
        }

        // Query spot margin positions from /api/v5/account/balance
        // Spot margin positions appear as balance sheet items (liab/spotInUseAmt fields)
        let mut margin_reports = self
            .http_client
            .request_spot_margin_position_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            margin_reports.retain(|report| report.instrument_id == instrument_id);
        }

        reports.append(&mut margin_reports);

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();

        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins * 60 * 1_000_000_000;
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

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

        let (order_reports, fill_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

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

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order_type = {
            let cache = self.core.cache();
            let order = cache
                .order(&cmd.client_order_id)
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            let order_type = order.order_type();

            // OKX trigger/algo orders are not supported for options.
            // Reject before emitting OrderSubmitted to avoid an invalid state transition.
            if self.is_conditional_order(order_type) {
                let inst_type = okx_instrument_type_from_symbol(cmd.instrument_id.symbol.as_str());

                if inst_type == OKXInstrumentType::Option {
                    anyhow::bail!(
                        "Trigger/conditional orders ({order_type:?}) are not supported for OKX options"
                    );
                }
            }

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);

            order_type
        };

        if self.is_conditional_order(order_type) {
            self.submit_conditional_order(&cmd)
        } else {
            self.submit_regular_order(&cmd)
        }
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let inst_type = okx_instrument_type_from_symbol(cmd.instrument_id.symbol.as_str());

        // Validate all orders before emitting any submitted events
        let cache = self.core.cache();

        for client_order_id in &cmd.order_list.client_order_ids {
            let order = cache
                .order(client_order_id)
                .ok_or_else(|| anyhow::anyhow!("Order not found: {client_order_id}"))?;

            if self.is_conditional_order(order.order_type()) {
                anyhow::bail!("Conditional orders not supported in order lists: {client_order_id}");
            }

            if order.time_in_force() != TimeInForce::Gtc {
                anyhow::bail!(
                    "Only GTC orders supported in order lists: {client_order_id} has {:?}",
                    order.time_in_force()
                );
            }
        }

        // Build batch payload and emit submitted events
        let mut batch_orders = Vec::new();

        for client_order_id in &cmd.order_list.client_order_ids {
            let order = cache.order(client_order_id).expect("validated above");

            batch_orders.push((
                inst_type,
                cmd.instrument_id,
                self.trade_mode_for_order(cmd.instrument_id, &cmd.params),
                order.client_order_id(),
                order.order_side(),
                None, // position_side: WS client defaults to Net for derivatives
                order.order_type(),
                order.quantity(),
                order.price(),
                order.trigger_price(),
                Some(order.is_post_only()),
                Some(order.is_reduce_only()),
            ));

            self.ws_dispatch_state.order_identities.insert(
                order.client_order_id(),
                OrderIdentity {
                    instrument_id: cmd.instrument_id,
                    strategy_id: order.strategy_id(),
                    order_side: order.order_side(),
                    order_type: order.order_type(),
                },
            );

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);
        }

        drop(cache);

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
                .await
                .map_err(|e| anyhow::anyhow!("Batch submit orders failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();

                for cid in &client_order_ids {
                    dispatch_state.order_identities.remove(cid);
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        *cid,
                        &format!("batch-submit-error: {e}"),
                        ts_event,
                        false,
                    );
                }
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);

        let ws_private = self.ws_private.clone();
        let command = cmd.clone();

        let new_px_usd = get_param_as_string(&cmd.params, "px_usd");
        let new_px_vol = get_param_as_string(&cmd.params, "px_vol");

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
                )
                .await
                .map_err(|e| anyhow::anyhow!("Modify order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_modify_rejected_event(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    &format!("modify-order-error: {e}"),
                    ts_event,
                );
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let cache = self.core.cache();
        let is_pending_algo = cache.order(&cmd.client_order_id).is_some_and(|o| {
            self.is_conditional_order(o.order_type()) && o.is_triggered() != Some(true)
        });
        drop(cache);

        if is_pending_algo {
            self.cancel_algo_order(&cmd);
        } else {
            self.cancel_ws_order(&cmd);
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        if self.config.use_mm_mass_cancel {
            // Use OKX's mass-cancel endpoint (requires market maker permissions)
            self.mass_cancel_instrument(cmd.instrument_id);
            Ok(())
        } else {
            // Cancel orders via batch cancel (works for all users)
            let cache = self.core.cache();
            let open_orders = cache.orders_open(None, Some(&cmd.instrument_id), None, None, None);

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
                // Triggered stop orders become regular orders on OKX
                let is_pending_algo = self.is_conditional_order(order.order_type())
                    && order.is_triggered() != Some(true);

                if is_pending_algo {
                    algo_orders.push((
                        order.instrument_id(),
                        order.client_order_id(),
                        order.venue_order_id(),
                        order.trader_id(),
                        order.strategy_id(),
                    ));
                } else {
                    self.ensure_order_identity(
                        order.client_order_id(),
                        order.strategy_id(),
                        order.instrument_id(),
                    );
                    regular_payload.push((
                        order.instrument_id(),
                        Some(order.client_order_id()),
                        order.venue_order_id(),
                    ));
                    regular_cancel_contexts.push((
                        order.client_order_id(),
                        order.instrument_id(),
                        order.strategy_id(),
                    ));
                }
            }
            drop(cache);

            log::debug!(
                "Canceling {} regular orders and {} algo orders for {}",
                regular_payload.len(),
                algo_orders.len(),
                cmd.instrument_id
            );

            if !regular_payload.is_empty() {
                let ws_private = self.ws_private.clone();
                let emitter = self.emitter.clone();
                let clock = self.clock;

                self.spawn_task("batch_cancel_orders", async move {
                    if let Err(e) = ws_private.batch_cancel_orders(regular_payload).await {
                        let ts = clock.get_time_ns();

                        for (cid, inst_id, strat_id) in &regular_cancel_contexts {
                            emitter.emit_order_cancel_rejected_event(
                                *strat_id,
                                *inst_id,
                                *cid,
                                None,
                                &format!("batch-cancel-error: {e}"),
                                ts,
                            );
                        }
                        anyhow::bail!("Batch cancel orders failed: {e}");
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

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        let cache = self.core.cache();

        let mut regular_payload = Vec::new();
        let mut algo_orders = Vec::new();

        for cancel in &cmd.cancels {
            // Triggered stop orders become regular orders on OKX
            let is_pending_algo = cache.order(&cancel.client_order_id).is_some_and(|o| {
                self.is_conditional_order(o.order_type()) && o.is_triggered() != Some(true)
            });

            if is_pending_algo {
                algo_orders.push(cancel.clone());
            } else {
                self.ensure_order_identity(
                    cancel.client_order_id,
                    cancel.strategy_id,
                    cancel.instrument_id,
                );
                regular_payload.push((
                    cancel.instrument_id,
                    Some(cancel.client_order_id),
                    cancel.venue_order_id,
                ));
            }
        }
        drop(cache);

        if !regular_payload.is_empty() {
            let ws_private = self.ws_private.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;
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
                    let ts = clock.get_time_ns();

                    for (cid, inst_id, strat_id) in &cancel_contexts {
                        emitter.emit_order_cancel_rejected_event(
                            *strat_id,
                            *inst_id,
                            *cid,
                            None,
                            &format!("batch-cancel-error: {e}"),
                            ts,
                        );
                    }
                    anyhow::bail!("Batch cancel orders failed: {e}");
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

        Ok(())
    }
}

// Picks the report that best answers the query. Tiered so a strong signal
// wins over a weak one regardless of ordering in the merged result set:
//   1. Exact `client_order_id` match.
//   2. Exact `venue_order_id` match (rare: only when the cached vid is
//      still valid; OKX rotates venue_order_id once an algo order triggers).
//
// Triggered-algo recovery is handled by the algo endpoint in the caller,
// which queries by algo_cl_ord_id and returns the parent's algo record
// directly. `linked_order_ids` is deliberately not consulted here because
// it is also populated with attached TP/SL child ids on the parent order,
// which would otherwise let a query for a child match the parent's report.
fn select_query_order_report(
    reports: Vec<OrderStatusReport>,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
) -> Option<OrderStatusReport> {
    let mut by_vid: Option<OrderStatusReport> = None;

    for report in reports {
        if report.client_order_id == Some(client_order_id) {
            return Some(report);
        }

        if by_vid.is_none()
            && venue_order_id
                .as_ref()
                .is_some_and(|vid| report.venue_order_id.as_str() == vid.as_str())
        {
            by_vid = Some(report);
        }
    }

    by_vid
}

#[cfg(test)]
mod tests {
    use nautilus_model::enums::OrderStatus;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    fn build_config(
        margin_mode: Option<OKXMarginMode>,
        use_spot_margin: bool,
    ) -> OKXExecClientConfig {
        OKXExecClientConfig {
            margin_mode,
            use_spot_margin,
            ..OKXExecClientConfig::default()
        }
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
            OrderSide::Buy,
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
}
