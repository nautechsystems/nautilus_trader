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

//! Live execution client implementation for the Bybit adapter.

use std::{
    future::Future,
    sync::Mutex,
    time::{Duration, Instant},
};

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
    env::get_or_env_var,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    common::{
        consts::BYBIT_VENUE,
        credential::credential_env_vars,
        enums::{
            BybitAccountType, BybitEnvironment, BybitOrderSide, BybitOrderType, BybitProductType,
            BybitTimeInForce,
        },
        parse::extract_raw_symbol,
    },
    config::BybitExecClientConfig,
    http::client::BybitHttpClient,
    websocket::{
        client::BybitWebSocketClient,
        messages::{
            BybitWsAmendOrderParams, BybitWsCancelOrderParams, BybitWsPlaceOrderParams,
            NautilusWsMessage,
        },
    },
};

/// Live execution client for Bybit.
#[derive(Debug)]
pub struct BybitExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: BybitExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: BybitHttpClient,
    ws_private: BybitWebSocketClient,
    ws_trade: BybitWebSocketClient,
    ws_private_stream_handle: Option<JoinHandle<()>>,
    ws_trade_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl BybitExecutionClient {
    /// Creates a new [`BybitExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: BybitExecClientConfig) -> anyhow::Result<Self> {
        let (key_var, secret_var) = credential_env_vars(config.environment);
        let api_key = get_or_env_var(config.api_key.clone(), key_var)?;
        let api_secret = get_or_env_var(config.api_secret.clone(), secret_var)?;

        let http_client = BybitHttpClient::with_credentials(
            api_key.clone(),
            api_secret.clone(),
            Some(config.http_base_url()),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.recv_window_ms,
            config.http_proxy_url.clone(),
        )?;

        let ws_private = BybitWebSocketClient::new_private(
            config.environment,
            Some(api_key.clone()),
            Some(api_secret.clone()),
            Some(config.ws_private_url()),
            config.heartbeat_interval_secs,
        );

        let ws_trade = BybitWebSocketClient::new_trade(
            config.environment,
            Some(api_key),
            Some(api_secret),
            Some(config.ws_trade_url()),
            config.heartbeat_interval_secs,
        );

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            None,
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_private,
            ws_trade,
            ws_private_stream_handle: None,
            ws_trade_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    fn product_types(&self) -> Vec<BybitProductType> {
        if self.config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            self.config.product_types.clone()
        }
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let account_state = self
            .http_client
            .request_account_state(BybitAccountType::Unified, self.core.account_id)
            .await
            .context("failed to request Bybit account state")?;

        self.emitter.send_account_state(account_state);
        Ok(())
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
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

    fn get_product_type_for_instrument(&self, instrument_id: InstrumentId) -> BybitProductType {
        BybitProductType::from_suffix(instrument_id.symbol.as_str()).unwrap_or_else(|| {
            log::warn!("No product-type suffix on {instrument_id}, defaulting to Linear");
            BybitProductType::Linear
        })
    }

    fn map_order_type(order_type: OrderType) -> anyhow::Result<BybitOrderType> {
        match order_type {
            OrderType::Market => Ok(BybitOrderType::Market),
            OrderType::Limit => Ok(BybitOrderType::Limit),
            _ => anyhow::bail!("unsupported order type for Bybit: {order_type}"),
        }
    }

    fn map_time_in_force(tif: TimeInForce, is_post_only: bool) -> BybitTimeInForce {
        if is_post_only {
            return BybitTimeInForce::PostOnly;
        }
        match tif {
            TimeInForce::Gtc => BybitTimeInForce::Gtc,
            TimeInForce::Ioc => BybitTimeInForce::Ioc,
            TimeInForce::Fok => BybitTimeInForce::Fok,
            _ => BybitTimeInForce::Gtc,
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BybitExecutionClient {
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
        *BYBIT_VENUE
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

        let product_types = self.product_types();

        if !self.core.instruments_initialized() {
            let mut all_instruments = Vec::new();
            for product_type in &product_types {
                let instruments = self
                    .http_client
                    .request_instruments(*product_type, None)
                    .await
                    .with_context(|| {
                        format!("failed to request Bybit instruments for {product_type:?}")
                    })?;

                if instruments.is_empty() {
                    log::warn!("No instruments returned for {product_type:?}");
                    continue;
                }

                log::info!("Loaded {} {product_type:?} instruments", instruments.len());

                self.http_client.cache_instruments(instruments.clone());
                all_instruments.extend(instruments);
            }

            if !all_instruments.is_empty() {
                self.ws_private.cache_instruments(all_instruments.clone());
                self.ws_trade.cache_instruments(all_instruments);
            }
            self.core.set_instruments_initialized();
        }

        self.ws_private.set_account_id(self.core.account_id);
        self.ws_trade.set_account_id(self.core.account_id);

        self.ws_private.connect().await?;
        self.ws_private.wait_until_active(10.0).await?;
        log::info!("Connected to private WebSocket");

        if self.ws_private_stream_handle.is_none() {
            let stream = self.ws_private.stream();
            let emitter = self.emitter.clone();
            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(message, &emitter);
                }
            });
            self.ws_private_stream_handle = Some(handle);
        }

        // Demo environment does not support Trade WebSocket API
        if self.config.environment == BybitEnvironment::Demo {
            log::warn!("Demo mode: Trade WebSocket not available, orders use HTTP REST API");
        } else {
            self.ws_trade.connect().await?;
            self.ws_trade.wait_until_active(10.0).await?;
            log::info!("Connected to trade WebSocket");

            if self.ws_trade_stream_handle.is_none() {
                let stream = self.ws_trade.stream();
                let emitter = self.emitter.clone();
                let handle = get_runtime().spawn(async move {
                    pin_mut!(stream);
                    while let Some(message) = stream.next().await {
                        dispatch_ws_message(message, &emitter);
                    }
                });
                self.ws_trade_stream_handle = Some(handle);
            }
        }

        self.ws_private.subscribe_orders().await?;
        self.ws_private.subscribe_executions().await?;
        self.ws_private.subscribe_positions().await?;
        self.ws_private.subscribe_wallet().await?;

        let account_state = self
            .http_client
            .request_account_state(BybitAccountType::Unified, self.core.account_id)
            .await
            .context("failed to request Bybit account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }
        self.emitter.send_account_state(account_state);

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

        if let Err(e) = self.ws_trade.close().await {
            log::warn!("Error closing trade websocket: {e:?}");
        }

        if let Some(handle) = self.ws_private_stream_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.ws_trade_stream_handle.take() {
            handle.abort();
        }

        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        self.update_account_state()
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        log::debug!(
            "query_order not implemented for Bybit execution client (client_order_id={})",
            cmd.client_order_id
        );
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
        let mut ws_private = self.ws_private.clone();
        let mut ws_trade = self.ws_trade.clone();
        let product_types = self.config.product_types.clone();

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();
            for product_type in product_types {
                match http_client.request_instruments(product_type, None).await {
                    Ok(instruments) => {
                        if instruments.is_empty() {
                            log::warn!("No instruments returned for {product_type:?}");
                            continue;
                        }
                        http_client.cache_instruments(instruments.clone());
                        all_instruments.extend(instruments);
                    }
                    Err(e) => {
                        log::error!("Failed to request instruments for {product_type:?}: {e}");
                    }
                }
            }

            if all_instruments.is_empty() {
                log::warn!(
                    "Instrument bootstrap yielded no instruments; WebSocket submissions may fail"
                );
            } else {
                ws_private.cache_instruments(all_instruments.clone());
                ws_trade.cache_instruments(all_instruments);
                log::info!("Instruments initialized");
            }
        });

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, product_types={:?}, environment={:?}, http_proxy_url={:?}, ws_proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.product_types,
            self.config.environment,
            self.config.http_proxy_url,
            self.config.ws_proxy_url,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();

        if let Some(handle) = self.ws_private_stream_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.ws_trade_stream_handle.take() {
            handle.abort();
        }
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            let order = cache
                .order(&cmd.client_order_id)
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            order.clone()
        };

        // Validate order params before emitting submitted event
        if let Err(e) = BybitOrderSide::try_from(order.order_side()) {
            self.emitter.emit_order_denied(&order, &e.to_string());
            return Ok(());
        }

        if let Err(e) = Self::map_order_type(order.order_type()) {
            self.emitter.emit_order_denied(&order, &e.to_string());
            return Ok(());
        }

        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        self.emitter.emit_order_submitted(&order);

        let instrument_id = order.instrument_id();
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        if self.config.environment == BybitEnvironment::Demo {
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let order_side = order.order_side();
            let order_type = order.order_type();
            let quantity = order.quantity();
            let time_in_force = order.time_in_force();
            let price = order.price();
            let trigger_price = order.trigger_price();
            let post_only = order.is_post_only();
            let reduce_only = order.is_reduce_only();

            self.spawn_task("submit_order_http", async move {
                let result = http_client
                    .submit_order(
                        account_id,
                        product_type,
                        instrument_id,
                        client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        Some(time_in_force),
                        price,
                        trigger_price,
                        Some(post_only),
                        reduce_only,
                        false, // is_quote_quantity
                        false, // is_leverage
                    )
                    .await;

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
                    anyhow::bail!("submit order failed: {e}");
                }

                Ok(())
            });

            return Ok(());
        }

        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let bybit_side = BybitOrderSide::try_from(order.order_side())?;
        let bybit_order_type = Self::map_order_type(order.order_type())?;

        let params = BybitWsPlaceOrderParams {
            category: product_type,
            symbol: Ustr::from(raw_symbol),
            side: bybit_side,
            order_type: bybit_order_type,
            qty: order.quantity().to_string(),
            is_leverage: None,
            market_unit: None,
            price: order.price().map(|p| p.to_string()),
            time_in_force: Some(Self::map_time_in_force(
                order.time_in_force(),
                order.is_post_only(),
            )),
            order_link_id: Some(order.client_order_id().to_string()),
            reduce_only: if order.is_reduce_only() {
                Some(true)
            } else {
                None
            },
            close_on_trigger: None,
            trigger_price: order.trigger_price().map(|p| p.to_string()),
            trigger_by: None,
            trigger_direction: None,
            tpsl_mode: None,
            take_profit: None,
            stop_loss: None,
            tp_trigger_by: None,
            sl_trigger_by: None,
            sl_trigger_price: None,
            tp_trigger_price: None,
            sl_order_type: None,
            tp_order_type: None,
            sl_limit_price: None,
            tp_limit_price: None,
        };

        let ws_trade = self.ws_trade.clone();
        let trader_id = self.core.trader_id;

        self.spawn_task("submit_order", async move {
            let result = ws_trade
                .place_order(
                    params,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("submit order failed: {e}"));

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

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for Bybit execution client (got {} orders)",
            cmd.order_list.client_order_ids.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let venue_order_id = cmd.venue_order_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        if self.config.environment == BybitEnvironment::Demo {
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let quantity = cmd.quantity;
            let price = cmd.price;

            self.spawn_task("modify_order_http", async move {
                let result = http_client
                    .modify_order(
                        account_id,
                        product_type,
                        instrument_id,
                        Some(client_order_id),
                        venue_order_id,
                        quantity,
                        price,
                    )
                    .await;

                if let Err(e) = result {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_modify_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("modify-order-error: {e}"),
                        ts_event,
                    );
                    anyhow::bail!("modify order failed: {e}");
                }

                Ok(())
            });

            return Ok(());
        }

        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());

        let params = BybitWsAmendOrderParams {
            category: product_type,
            symbol: Ustr::from(raw_symbol),
            order_id: cmd.venue_order_id.map(|v| v.to_string()),
            order_link_id: Some(cmd.client_order_id.to_string()),
            qty: cmd.quantity.map(|q| q.to_string()),
            price: cmd.price.map(|p| p.to_string()),
            trigger_price: None,
            take_profit: None,
            stop_loss: None,
            tp_trigger_by: None,
            sl_trigger_by: None,
        };

        let ws_trade = self.ws_trade.clone();
        let trader_id = cmd.trader_id;

        self.spawn_task("modify_order", async move {
            let result = ws_trade
                .amend_order(
                    params,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("modify order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_modify_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("modify-order-error: {e}"),
                    ts_event,
                );
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let venue_order_id = cmd.venue_order_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        if self.config.environment == BybitEnvironment::Demo {
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;

            self.spawn_task("cancel_order_http", async move {
                let result = http_client
                    .cancel_order(
                        account_id,
                        product_type,
                        instrument_id,
                        Some(client_order_id),
                        venue_order_id,
                    )
                    .await;

                if let Err(e) = result {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("cancel-order-error: {e}"),
                        ts_event,
                    );
                    anyhow::bail!("cancel order failed: {e}");
                }

                Ok(())
            });

            return Ok(());
        }

        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());

        let params = BybitWsCancelOrderParams {
            category: product_type,
            symbol: Ustr::from(raw_symbol),
            order_id: cmd.venue_order_id.map(|v| v.to_string()),
            order_link_id: Some(cmd.client_order_id.to_string()),
        };

        let ws_trade = self.ws_trade.clone();
        let trader_id = cmd.trader_id;

        self.spawn_task("cancel_order", async move {
            let result = ws_trade
                .cancel_order(
                    params,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    venue_order_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("cancel order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("cancel-order-error: {e}"),
                    ts_event,
                );
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        if cmd.order_side != OrderSide::NoOrderSide {
            log::warn!(
                "Bybit does not support order_side filtering for cancel all orders; \
                ignoring order_side={:?} and canceling all orders",
                cmd.order_side,
            );
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let account_id = self.core.account_id;
        let http_client = self.http_client.clone();

        self.spawn_task("cancel_all_orders", async move {
            match http_client
                .cancel_all_orders(account_id, product_type, instrument_id)
                .await
            {
                Ok(reports) => {
                    for report in reports {
                        log::debug!("Cancelled order: {report:?}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to cancel all orders for {instrument_id}: {e}");
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);

        // Demo mode: cancel individually via HTTP (batch not supported)
        if self.config.environment == BybitEnvironment::Demo {
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let strategy_id = cmd.strategy_id;
            let emitter = self.emitter.clone();
            let clock = self.clock;
            let cancels: Vec<_> = cmd
                .cancels
                .iter()
                .map(|c| (c.client_order_id, c.venue_order_id))
                .collect();

            self.spawn_task("batch_cancel_orders_http", async move {
                for (client_order_id, venue_order_id) in cancels {
                    if let Err(e) = http_client
                        .cancel_order(
                            account_id,
                            product_type,
                            instrument_id,
                            Some(client_order_id),
                            venue_order_id,
                        )
                        .await
                    {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_cancel_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            venue_order_id,
                            &format!("cancel-order-error: {e}"),
                            ts_event,
                        );
                    }
                }
                Ok(())
            });

            return Ok(());
        }

        let raw_symbol = Ustr::from(extract_raw_symbol(instrument_id.symbol.as_str()));

        let mut cancel_params = Vec::with_capacity(cmd.cancels.len());
        for cancel in &cmd.cancels {
            cancel_params.push(BybitWsCancelOrderParams {
                category: product_type,
                symbol: raw_symbol,
                order_id: cancel.venue_order_id.map(|v| v.to_string()),
                order_link_id: Some(cancel.client_order_id.to_string()),
            });
        }

        let ws_trade = self.ws_trade.clone();
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;

        self.spawn_task("batch_cancel_orders", async move {
            ws_trade
                .batch_cancel_orders(trader_id, strategy_id, cancel_params)
                .await?;
            Ok(())
        });

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

        let product_type = self.get_product_type_for_instrument(instrument_id);

        let mut reports = self
            .http_client
            .request_order_status_reports(
                self.core.account_id,
                product_type,
                Some(instrument_id),
                false,
                None,
                None,
                None,
            )
            .await?;

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
            let product_type = self.get_product_type_for_instrument(instrument_id);
            let mut fetched = self
                .http_client
                .request_order_status_reports(
                    self.core.account_id,
                    product_type,
                    Some(instrument_id),
                    cmd.open_only,
                    None,
                    None,
                    None,
                )
                .await?;
            reports.append(&mut fetched);
        } else {
            for product_type in self.product_types() {
                let mut fetched = self
                    .http_client
                    .request_order_status_reports(
                        self.core.account_id,
                        product_type,
                        None,
                        cmd.open_only,
                        None,
                        None,
                        None,
                    )
                    .await?;
                reports.append(&mut fetched);
            }
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
        let start_ms = nanos_to_millis(cmd.start);
        let end_ms = nanos_to_millis(cmd.end);
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let product_type = self.get_product_type_for_instrument(instrument_id);
            let mut fetched = self
                .http_client
                .request_fill_reports(
                    self.core.account_id,
                    product_type,
                    Some(instrument_id),
                    start_ms,
                    end_ms,
                    None,
                )
                .await?;
            reports.append(&mut fetched);
        } else {
            for product_type in self.product_types() {
                let mut fetched = self
                    .http_client
                    .request_fill_reports(
                        self.core.account_id,
                        product_type,
                        None,
                        start_ms,
                        end_ms,
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

        if let Some(instrument_id) = cmd.instrument_id {
            let product_type = self.get_product_type_for_instrument(instrument_id);

            // Skip Spot - positions API only supports derivatives
            if product_type != BybitProductType::Spot {
                let mut fetched = self
                    .http_client
                    .request_position_status_reports(
                        self.core.account_id,
                        product_type,
                        Some(instrument_id),
                    )
                    .await?;
                reports.append(&mut fetched);
            }
        } else {
            for product_type in self.product_types() {
                // Skip Spot - positions API only supports derivatives
                if product_type == BybitProductType::Spot {
                    continue;
                }
                let mut fetched = self
                    .http_client
                    .request_position_status_reports(self.core.account_id, product_type, None)
                    .await?;
                reports.append(&mut fetched);
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
            let lookback_ns = mins * 60 * 1_000_000_000;
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false)
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
            *BYBIT_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }
}

/// Dispatches a WebSocket message using the event emitter.
fn dispatch_ws_message(message: NautilusWsMessage, emitter: &ExecutionEventEmitter) {
    match message {
        NautilusWsMessage::AccountState(state) => {
            emitter.send_account_state(state);
        }
        NautilusWsMessage::PositionStatusReport(report) => {
            emitter.send_position_report(report);
        }
        NautilusWsMessage::OrderStatusReports(reports) => {
            log::debug!("Processing {} order status report(s)", reports.len());
            for report in reports {
                emitter.send_order_status_report(report);
            }
        }
        NautilusWsMessage::FillReports(reports) => {
            log::debug!("Processing {} fill report(s)", reports.len());
            for report in reports {
                emitter.send_fill_report(report);
            }
        }
        NautilusWsMessage::OrderRejected(event) => {
            emitter.send_order_event(OrderEventAny::Rejected(event));
        }
        NautilusWsMessage::OrderCancelRejected(event) => {
            emitter.send_order_event(OrderEventAny::CancelRejected(event));
        }
        NautilusWsMessage::OrderModifyRejected(event) => {
            emitter.send_order_event(OrderEventAny::ModifyRejected(event));
        }
        NautilusWsMessage::Error(e) => {
            log::warn!("Websocket error: code={} message={}", e.code, e.message);
        }
        NautilusWsMessage::Reconnected => {
            log::info!("Websocket reconnected");
        }
        NautilusWsMessage::Authenticated => {
            log::debug!("Websocket authenticated");
        }
        NautilusWsMessage::Deltas(_)
        | NautilusWsMessage::Data(_)
        | NautilusWsMessage::FundingRates(_)
        | NautilusWsMessage::MarkPrices(_)
        | NautilusWsMessage::IndexPrices(_) => {
            log::debug!("Ignoring websocket data message");
        }
    }
}

fn nanos_to_millis(value: Option<UnixNanos>) -> Option<i64> {
    value.map(|nanos| (nanos.as_u64() / 1_000_000) as i64)
}
