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
    env::get_or_env_var,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price},
};
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    common::{
        consts::BYBIT_VENUE,
        credential::credential_env_vars,
        enums::{
            BybitAccountType, BybitEnvironment, BybitOrderSide, BybitOrderType, BybitPositionIdx,
            BybitPositionMode, BybitProductType, BybitTimeInForce, BybitTpSlMode,
            resolve_trigger_type,
        },
        parse::{
            BybitTpSlParams, extract_raw_symbol, get_price_str, nanos_to_millis,
            parse_bybit_tp_sl_params, spot_leverage, spot_market_unit, trigger_direction,
        },
        symbol::BybitSymbol,
    },
    config::BybitExecClientConfig,
    http::client::BybitHttpClient,
    websocket::{
        client::BybitWebSocketClient,
        dispatch::{OrderIdentity, PendingOperation, WsDispatchState, dispatch_ws_message},
        messages::{BybitWsAmendOrderParams, BybitWsCancelOrderParams, BybitWsPlaceOrderParams},
    },
};

/// Resolves the `positionIdx` to send with an order under a given position mode.
///
/// In hedge mode `positionIdx` identifies the position being affected (1 = long,
/// 2 = short), not the trade direction. A reduce-only sell closes a long position
/// and a reduce-only buy closes a short position. A manual override always wins.
#[must_use]
pub fn resolve_position_idx(
    position_mode: Option<BybitPositionMode>,
    order_side: BybitOrderSide,
    is_reduce_only: bool,
    manual_override: Option<BybitPositionIdx>,
) -> Option<BybitPositionIdx> {
    if manual_override.is_some() {
        return manual_override;
    }
    let mode = position_mode?;
    match mode {
        BybitPositionMode::BothSides => Some(match (order_side, is_reduce_only) {
            (BybitOrderSide::Buy, false) | (BybitOrderSide::Sell, true) => {
                BybitPositionIdx::BuyHedge
            }
            (BybitOrderSide::Sell, false) | (BybitOrderSide::Buy, true) => {
                BybitPositionIdx::SellHedge
            }
            (BybitOrderSide::Unknown, _) => BybitPositionIdx::OneWay,
        }),
        BybitPositionMode::MergedSingle => Some(BybitPositionIdx::OneWay),
    }
}

fn parse_derivative_symbol(symbol_str: &str) -> Option<BybitSymbol> {
    let symbol = match BybitSymbol::new(symbol_str) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to parse symbol {symbol_str}: {e}");
            return None;
        }
    };
    matches!(
        symbol.product_type(),
        BybitProductType::Linear | BybitProductType::Inverse
    )
    .then_some(symbol)
}

fn is_unchanged_error<E: std::fmt::Display>(err: &E, code: &str) -> bool {
    let msg = err.to_string().to_lowercase();
    if msg.contains("not been modified") {
        return true;
    }
    !code.is_empty() && msg.contains(code)
}

fn is_low_margin_error<E: std::fmt::Display>(err: &E) -> bool {
    err.to_string()
        .contains("needs to be equal to or greater than")
}

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
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
    dispatch_state: Arc<WsDispatchState>,
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
            config.proxy_url.clone(),
        )?;

        let ws_private = BybitWebSocketClient::new_private(
            config.environment,
            Some(api_key.clone()),
            Some(api_secret.clone()),
            Some(config.ws_private_url()),
            config.heartbeat_interval_secs,
            config.transport_backend,
            config.proxy_url.clone(),
        );

        let ws_trade = BybitWebSocketClient::new_trade(
            config.environment,
            Some(api_key),
            Some(api_secret),
            Some(config.ws_trade_url()),
            config.heartbeat_interval_secs,
            config.transport_backend,
            config.proxy_url.clone(),
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
            instruments_cache: Arc::new(AHashMap::new()),
            dispatch_state: Arc::new(WsDispatchState::default()),
        })
    }

    fn product_types(&self) -> Vec<BybitProductType> {
        if self.config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            self.config.product_types.clone()
        }
    }

    fn update_account_state(&self) {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(BybitAccountType::Unified, account_id)
                .await
                .context("failed to request Bybit account state")?;
            emitter.send_account_state(account_state);
            Ok(())
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

    fn resolve_position_idx(
        &self,
        instrument_id: InstrumentId,
        order_side: BybitOrderSide,
        is_reduce_only: bool,
        manual_override: Option<BybitPositionIdx>,
    ) -> Option<BybitPositionIdx> {
        let product_type = self.get_product_type_for_instrument(instrument_id);
        if !matches!(
            product_type,
            BybitProductType::Linear | BybitProductType::Inverse
        ) {
            return None;
        }
        let mode = self
            .config
            .position_mode
            .as_ref()
            .and_then(|map| map.get(instrument_id.symbol.as_str()).copied());
        resolve_position_idx(mode, order_side, is_reduce_only, manual_override)
    }

    async fn apply_account_configuration(&self) -> anyhow::Result<()> {
        self.apply_leverages_setting().await;
        self.apply_position_modes_setting().await;
        self.apply_margin_mode_setting().await
    }

    async fn apply_leverages_setting(&self) {
        let Some(leverages) = &self.config.futures_leverages else {
            return;
        };

        for (symbol_str, leverage) in leverages {
            self.apply_leverage_entry(symbol_str, *leverage).await;
        }
    }

    async fn apply_leverage_entry(&self, symbol_str: &str, leverage: u32) {
        let Some(symbol) = parse_derivative_symbol(symbol_str) else {
            return;
        };
        let lev = leverage.to_string();
        let result = self
            .http_client
            .set_leverage(symbol.product_type(), symbol.raw_symbol(), &lev, &lev)
            .await;

        match result {
            Ok(_) => log::info!("Set leverage for {symbol_str} to {leverage}"),
            Err(e) if is_unchanged_error(&e, "110043") => {
                log::info!("Leverage already set for {symbol_str} to {leverage}");
            }
            Err(e) => log::error!("Failed to set leverage for {symbol_str}: {e}"),
        }
    }

    async fn apply_position_modes_setting(&self) {
        let Some(modes) = &self.config.position_mode else {
            return;
        };

        for (symbol_str, mode) in modes {
            self.apply_position_mode_entry(symbol_str, *mode).await;
        }
    }

    async fn apply_position_mode_entry(&self, symbol_str: &str, mode: BybitPositionMode) {
        let Some(symbol) = parse_derivative_symbol(symbol_str) else {
            return;
        };
        let result = self
            .http_client
            .switch_mode(
                symbol.product_type(),
                mode,
                Some(symbol.raw_symbol().to_string()),
                None,
            )
            .await;

        match result {
            Ok(_) => log::info!("Set symbol `{symbol_str}` position mode to `{mode:?}`"),
            Err(e) if is_unchanged_error(&e, "110025") => {
                log::info!("Symbol `{symbol_str}` position mode already set to `{mode:?}`");
            }
            Err(e) => log::error!("Failed to set position mode for {symbol_str}: {e}"),
        }
    }

    async fn apply_margin_mode_setting(&self) -> anyhow::Result<()> {
        let Some(margin_mode) = self.config.margin_mode else {
            return Ok(());
        };

        let result = self.http_client.set_margin_mode(margin_mode).await;

        match result {
            Ok(_) => {
                log::info!("Set account margin mode to {margin_mode:?}");
                Ok(())
            }
            Err(e) if is_unchanged_error(&e, "") => {
                log::info!("Margin mode already set to {margin_mode:?}");
                Ok(())
            }
            Err(e) if is_low_margin_error(&e) => {
                log::warn!("Cannot set margin mode: {e}");
                Ok(())
            }
            Err(e) => Err(anyhow::Error::from(e).context("failed to set margin mode")),
        }
    }

    fn map_order_type(order_type: OrderType) -> anyhow::Result<(BybitOrderType, bool)> {
        match order_type {
            OrderType::Market => Ok((BybitOrderType::Market, false)),
            OrderType::Limit => Ok((BybitOrderType::Limit, false)),
            OrderType::StopMarket | OrderType::MarketIfTouched => {
                Ok((BybitOrderType::Market, true))
            }
            OrderType::StopLimit | OrderType::LimitIfTouched => Ok((BybitOrderType::Limit, true)),
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

    fn build_ws_place_params(
        order: &OrderAny,
        product_type: BybitProductType,
        raw_symbol: &str,
        tp_sl: &BybitTpSlParams,
        position_idx: Option<BybitPositionIdx>,
    ) -> anyhow::Result<BybitWsPlaceOrderParams> {
        let bybit_side = BybitOrderSide::try_from(order.order_side())?;
        let (bybit_order_type, is_conditional) = Self::map_order_type(order.order_type())?;
        let has_tp_sl = tp_sl.has_tp_sl();
        let trigger_dir = trigger_direction(order.order_type(), order.order_side(), is_conditional);

        Ok(BybitWsPlaceOrderParams {
            category: product_type,
            symbol: Ustr::from(raw_symbol),
            side: bybit_side,
            order_type: bybit_order_type,
            qty: order.quantity().to_string(),
            is_leverage: spot_leverage(product_type, tp_sl.is_leverage),
            market_unit: spot_market_unit(
                product_type,
                bybit_order_type,
                order.is_quote_quantity(),
            ),
            price: order.price().map(|p: Price| p.to_string()),
            time_in_force: if bybit_order_type == BybitOrderType::Market {
                None
            } else {
                Some(Self::map_time_in_force(
                    order.time_in_force(),
                    order.is_post_only(),
                ))
            },
            order_link_id: Some(order.client_order_id().to_string()),
            reduce_only: if order.is_reduce_only() {
                Some(true)
            } else {
                None
            },
            close_on_trigger: tp_sl.close_on_trigger,
            trigger_price: order.trigger_price().map(|p: Price| p.to_string()),
            trigger_by: if is_conditional {
                Some(resolve_trigger_type(order.trigger_type()))
            } else {
                None
            },
            trigger_direction: trigger_dir.map(|d| d as i32),
            tpsl_mode: if has_tp_sl {
                Some(BybitTpSlMode::Full)
            } else {
                None
            },
            take_profit: tp_sl.take_profit.map(|p| p.to_string()),
            stop_loss: tp_sl.stop_loss.map(|p| p.to_string()),
            tp_trigger_by: tp_sl.tp_trigger_by.or(tp_sl
                .take_profit
                .map(|_| resolve_trigger_type(order.trigger_type()))),
            sl_trigger_by: tp_sl.sl_trigger_by.or(tp_sl
                .stop_loss
                .map(|_| resolve_trigger_type(order.trigger_type()))),
            sl_trigger_price: tp_sl.sl_trigger_price.clone(),
            tp_trigger_price: tp_sl.tp_trigger_price.clone(),
            sl_order_type: tp_sl.sl_order_type,
            tp_order_type: tp_sl.tp_order_type,
            sl_limit_price: tp_sl.sl_limit_price.clone(),
            tp_limit_price: tp_sl.tp_limit_price.clone(),
            order_iv: tp_sl.order_iv.clone(),
            mmp: tp_sl.mmp,
            position_idx,
        })
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

        // Reset after a prior disconnect so REST calls are not short-circuited
        self.http_client.reset_cancellation_token();

        let product_types = self.product_types();

        if !self.core.instruments_initialized() {
            let mut all_instruments = Vec::new();

            for product_type in &product_types {
                let instruments = self
                    .http_client
                    .request_instruments(*product_type, None, None)
                    .await
                    .with_context(|| {
                        format!("failed to request Bybit instruments for {product_type:?}")
                    })?;

                if instruments.is_empty() {
                    log::warn!("No instruments returned for {product_type:?}");
                    continue;
                }

                log::info!("Loaded {} {product_type:?} instruments", instruments.len());

                self.http_client.cache_instruments(&instruments);
                all_instruments.extend(instruments);
            }

            if !all_instruments.is_empty() {
                let mut instruments_map = AHashMap::new();
                for instrument in &all_instruments {
                    instruments_map.insert(instrument.id().symbol.inner(), instrument.clone());
                }
                self.instruments_cache = Arc::new(instruments_map);
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
            let account_id = self.core.account_id;
            let instruments = Arc::clone(&self.instruments_cache);
            let state = Arc::clone(&self.dispatch_state);
            let clock = self.clock;

            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(
                        &message,
                        &emitter,
                        &state,
                        account_id,
                        &instruments,
                        clock,
                    );
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
                let account_id = self.core.account_id;
                let instruments = Arc::clone(&self.instruments_cache);
                let state = Arc::clone(&self.dispatch_state);
                let clock = self.clock;

                let handle = get_runtime().spawn(async move {
                    pin_mut!(stream);
                    while let Some(message) = stream.next().await {
                        dispatch_ws_message(
                            &message,
                            &emitter,
                            &state,
                            account_id,
                            &instruments,
                            clock,
                        );
                    }
                });
                self.ws_trade_stream_handle = Some(handle);
            }
        }

        self.ws_private.subscribe_orders().await?;
        self.ws_private.subscribe_executions().await?;
        self.ws_private.subscribe_positions().await?;
        self.ws_private.subscribe_wallet().await?;

        self.apply_account_configuration().await?;

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

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        self.update_account_state();
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let account_id = self.core.account_id;
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();

        self.spawn_task("query_order", async move {
            match http_client
                .query_order(
                    account_id,
                    product_type,
                    instrument_id,
                    Some(client_order_id),
                    venue_order_id,
                )
                .await
            {
                Ok(Some(report)) => {
                    emitter.send_order_status_report(report);
                }
                Ok(None) => {
                    log::warn!("Order not found: client_order_id={client_order_id}, venue_order_id={venue_order_id:?}");
                }
                Err(e) => {
                    log::error!("Failed to query order: {e}");
                }
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

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        let http_client = self.http_client.clone();
        let product_types = self.config.product_types.clone();

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();

            for product_type in product_types {
                match http_client
                    .request_instruments(product_type, None, None)
                    .await
                {
                    Ok(instruments) => {
                        if instruments.is_empty() {
                            log::warn!("No instruments returned for {product_type:?}");
                            continue;
                        }
                        http_client.cache_instruments(&instruments);
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
                log::info!("Instruments initialized: count={}", all_instruments.len());
            }
        });

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, product_types={:?}, environment={:?}, proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.product_types,
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

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
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

        let tp_sl = match parse_bybit_tp_sl_params(cmd.params.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }
        };

        if self.config.environment == BybitEnvironment::Demo
            && (tp_sl.has_tp_sl() || tp_sl.order_iv.is_some() || tp_sl.mmp.is_some())
        {
            self.emitter.emit_order_denied(
                &order,
                "Native TP/SL and option params are not supported in demo mode",
            );
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

        // Store identity for WS dispatch to produce proper order events
        self.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side: order.order_side(),
                order_type: order.order_type(),
            },
        );

        let bybit_side =
            BybitOrderSide::try_from(order.order_side()).expect("order side validated above");
        let position_idx = self.resolve_position_idx(
            instrument_id,
            bybit_side,
            order.is_reduce_only(),
            tp_sl.position_idx,
        );

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
            let is_quote_quantity = order.is_quote_quantity();
            let is_leverage = tp_sl.is_leverage;

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
                        is_quote_quantity,
                        is_leverage,
                        position_idx,
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
        let params =
            Self::build_ws_place_params(&order, product_type, raw_symbol, &tp_sl, position_idx)?;

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("submit_order", async move {
            match ws_trade.place_order(params).await {
                Ok(req_id) => {
                    dispatch_state.pending_requests.insert(
                        req_id,
                        (vec![client_order_id], vec![None], PendingOperation::Place),
                    );
                }
                Err(e) => {
                    dispatch_state.order_identities.remove(&client_order_id);
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
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        if cmd.order_list.client_order_ids.is_empty() {
            return Ok(());
        }

        let tp_sl = match parse_bybit_tp_sl_params(cmd.params.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                let cache = self.core.cache();

                for cid in &cmd.order_list.client_order_ids {
                    if let Some(order) = cache.order(cid) {
                        self.emitter.emit_order_denied(order, &e.to_string());
                    }
                }
                return Ok(());
            }
        };

        if self.config.environment == BybitEnvironment::Demo
            && (tp_sl.has_tp_sl() || tp_sl.order_iv.is_some() || tp_sl.mmp.is_some())
        {
            let cache = self.core.cache();

            for cid in &cmd.order_list.client_order_ids {
                if let Some(order) = cache.order(cid) {
                    self.emitter.emit_order_denied(
                        order,
                        "Native TP/SL and option params are not supported in demo mode",
                    );
                }
            }
            return Ok(());
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let strategy_id = cmd.strategy_id;

        let mut valid_orders = Vec::with_capacity(cmd.order_list.client_order_ids.len());
        {
            let cache = self.core.cache();
            let mut deny_reason: Option<String> = None;

            for cid in &cmd.order_list.client_order_ids {
                let Some(order) = cache.order(cid) else {
                    deny_reason = Some(format!("Order not found in cache: {cid}"));
                    break;
                };

                if order.is_closed() {
                    deny_reason = Some(format!("Cannot submit closed order {cid}"));
                    break;
                }

                if let Err(e) = BybitOrderSide::try_from(order.order_side()) {
                    deny_reason = Some(e.to_string());
                    break;
                }

                if let Err(e) = Self::map_order_type(order.order_type()) {
                    deny_reason = Some(e.to_string());
                    break;
                }

                valid_orders.push(order.clone());
            }

            // Deny entire list if any order fails validation
            if let Some(reason) = deny_reason {
                for cid in &cmd.order_list.client_order_ids {
                    if let Some(order) = cache.order(cid) {
                        self.emitter.emit_order_denied(order, &reason);
                    }
                }
                return Ok(());
            }
        }

        if valid_orders.is_empty() {
            return Ok(());
        }

        for order in &valid_orders {
            self.emitter.emit_order_submitted(order);
            self.dispatch_state.order_identities.insert(
                order.client_order_id(),
                OrderIdentity {
                    instrument_id,
                    strategy_id,
                    order_side: order.order_side(),
                    order_type: order.order_type(),
                },
            );
        }

        let emitter = self.emitter.clone();
        let clock = self.clock;

        // Demo mode: submit individually via HTTP
        if self.config.environment == BybitEnvironment::Demo {
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let is_leverage = tp_sl.is_leverage;

            let order_data: Vec<_> = valid_orders
                .iter()
                .map(|o| {
                    let bybit_side = BybitOrderSide::try_from(o.order_side())
                        .expect("order side validated above");
                    let position_idx = self.resolve_position_idx(
                        instrument_id,
                        bybit_side,
                        o.is_reduce_only(),
                        tp_sl.position_idx,
                    );
                    (
                        o.client_order_id(),
                        o.order_side(),
                        o.order_type(),
                        o.quantity(),
                        o.time_in_force(),
                        o.price(),
                        o.trigger_price(),
                        o.is_post_only(),
                        o.is_reduce_only(),
                        o.is_quote_quantity(),
                        position_idx,
                    )
                })
                .collect();

            self.spawn_task("submit_order_list_http", async move {
                for (
                    cid,
                    side,
                    otype,
                    qty,
                    tif,
                    price,
                    trigger,
                    post_only,
                    reduce,
                    quote_qty,
                    position_idx,
                ) in order_data
                {
                    if let Err(e) = http_client
                        .submit_order(
                            account_id,
                            product_type,
                            instrument_id,
                            cid,
                            side,
                            otype,
                            qty,
                            Some(tif),
                            price,
                            trigger,
                            Some(post_only),
                            reduce,
                            quote_qty,
                            is_leverage,
                            position_idx,
                        )
                        .await
                    {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            cid,
                            &format!("submit-order-error: {e}"),
                            ts_event,
                            false,
                        );
                    }
                }
                Ok(())
            });

            return Ok(());
        }

        // Live mode: batch submit via WebSocket
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());

        let mut order_params = Vec::with_capacity(valid_orders.len());
        let mut client_order_ids = Vec::with_capacity(valid_orders.len());

        for order in &valid_orders {
            let bybit_side =
                BybitOrderSide::try_from(order.order_side()).expect("order side validated above");
            let position_idx = self.resolve_position_idx(
                instrument_id,
                bybit_side,
                order.is_reduce_only(),
                tp_sl.position_idx,
            );
            let params =
                Self::build_ws_place_params(order, product_type, raw_symbol, &tp_sl, position_idx)
                    .expect("validated above");
            order_params.push(params);
            client_order_ids.push(order.client_order_id());
        }

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("submit_order_list", async move {
            match ws_trade.batch_place_orders(order_params).await {
                Ok(req_ids) => {
                    for (req_id, chunk_cids) in req_ids
                        .into_iter()
                        .zip(client_order_ids.chunks(20).map(|c| c.to_vec()))
                    {
                        let chunk_voids = vec![None; chunk_cids.len()];
                        dispatch_state
                            .pending_requests
                            .insert(req_id, (chunk_cids, chunk_voids, PendingOperation::Place));
                    }
                }
                Err(e) => {
                    for cid in &client_order_ids {
                        dispatch_state.order_identities.remove(cid);
                    }

                    let ts_event = clock.get_time_ns();

                    for cid in &client_order_ids {
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            *cid,
                            &format!("submit-order-list-error: {e}"),
                            ts_event,
                            false,
                        );
                    }
                    anyhow::bail!("submit order list failed: {e}");
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let venue_order_id = cmd.venue_order_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let has_order_iv = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("order_iv"))
            .is_some();

        if self.config.environment == BybitEnvironment::Demo && has_order_iv {
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_modify_rejected_event(
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                "Option params (order_iv) are not supported in demo mode",
                ts_event,
            );
            return Ok(());
        }

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

        let order_iv = if let Some(value) = cmd.params.as_ref().and_then(|p| p.get("order_iv")) {
            match get_price_str(cmd.params.as_ref().unwrap(), "order_iv") {
                Some(s) => Some(s),
                None => {
                    let ts_event = self.clock.get_time_ns();
                    self.emitter.emit_order_modify_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("invalid type for 'order_iv': {value}, expected string or number"),
                        ts_event,
                    );
                    return Ok(());
                }
            }
        } else {
            None
        };

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
            order_iv,
        };

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("modify_order", async move {
            match ws_trade.amend_order(params).await {
                Ok(req_id) => {
                    dispatch_state.pending_requests.insert(
                        req_id,
                        (
                            vec![client_order_id],
                            vec![venue_order_id],
                            PendingOperation::Amend,
                        ),
                    );
                }
                Err(e) => {
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
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
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
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("cancel_order", async move {
            match ws_trade.cancel_order(params).await {
                Ok(req_id) => {
                    dispatch_state.pending_requests.insert(
                        req_id,
                        (
                            vec![client_order_id],
                            vec![venue_order_id],
                            PendingOperation::Cancel,
                        ),
                    );
                }
                Err(e) => {
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
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
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

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
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
        let client_order_ids: Vec<_> = cmd.cancels.iter().map(|c| c.client_order_id).collect();
        let venue_order_ids: Vec<_> = cmd.cancels.iter().map(|c| c.venue_order_id).collect();
        for cancel in &cmd.cancels {
            cancel_params.push(BybitWsCancelOrderParams {
                category: product_type,
                symbol: raw_symbol,
                order_id: cancel.venue_order_id.map(|v| v.to_string()),
                order_link_id: Some(cancel.client_order_id.to_string()),
            });
        }

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("batch_cancel_orders", async move {
            match ws_trade.batch_cancel_orders(cancel_params).await {
                Ok(req_ids) => {
                    for (req_id, (chunk_cids, chunk_voids)) in req_ids.into_iter().zip(
                        client_order_ids
                            .chunks(20)
                            .map(|c| c.to_vec())
                            .zip(venue_order_ids.chunks(20).map(|c| c.to_vec())),
                    ) {
                        dispatch_state
                            .pending_requests
                            .insert(req_id, (chunk_cids, chunk_voids, PendingOperation::Cancel));
                    }
                }
                Err(e) => {
                    anyhow::bail!("batch cancel orders failed: {e}");
                }
            }
            Ok(())
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::enums::BybitMarketUnit;

    #[rstest]
    #[case::spot_market_base(
        BybitProductType::Spot,
        BybitOrderType::Market,
        false,
        Some(BybitMarketUnit::BaseCoin)
    )]
    #[case::spot_market_quote(
        BybitProductType::Spot,
        BybitOrderType::Market,
        true,
        Some(BybitMarketUnit::QuoteCoin)
    )]
    #[case::spot_limit(BybitProductType::Spot, BybitOrderType::Limit, true, None)]
    #[case::linear_market(BybitProductType::Linear, BybitOrderType::Market, true, None)]
    fn test_ws_params_market_unit(
        #[case] product_type: BybitProductType,
        #[case] order_type: BybitOrderType,
        #[case] is_quote_quantity: bool,
        #[case] expected: Option<BybitMarketUnit>,
    ) {
        let params = BybitWsPlaceOrderParams {
            category: product_type,
            symbol: ustr::Ustr::from("BTCUSDT"),
            side: BybitOrderSide::Buy,
            order_type,
            qty: "1.0".to_string(),
            is_leverage: None,
            market_unit: spot_market_unit(product_type, order_type, is_quote_quantity),
            price: None,
            time_in_force: None,
            order_link_id: None,
            reduce_only: None,
            close_on_trigger: None,
            trigger_price: None,
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
            order_iv: None,
            mmp: None,
            position_idx: None,
        };

        assert_eq!(params.market_unit, expected);
    }

    #[rstest]
    #[case::market(OrderType::Market, BybitOrderType::Market, false)]
    #[case::limit(OrderType::Limit, BybitOrderType::Limit, false)]
    #[case::stop_market(OrderType::StopMarket, BybitOrderType::Market, true)]
    #[case::stop_limit(OrderType::StopLimit, BybitOrderType::Limit, true)]
    #[case::market_if_touched(OrderType::MarketIfTouched, BybitOrderType::Market, true)]
    #[case::limit_if_touched(OrderType::LimitIfTouched, BybitOrderType::Limit, true)]
    fn test_map_order_type(
        #[case] input: OrderType,
        #[case] expected_type: BybitOrderType,
        #[case] expected_conditional: bool,
    ) {
        let (bybit_type, is_conditional) = BybitExecutionClient::map_order_type(input).unwrap();
        assert_eq!(bybit_type, expected_type);
        assert_eq!(is_conditional, expected_conditional);
    }

    #[rstest]
    fn test_map_order_type_rejects_trailing_stop() {
        assert!(BybitExecutionClient::map_order_type(OrderType::TrailingStopMarket).is_err());
    }

    #[rstest]
    #[case::buy_open(BybitOrderSide::Buy, false, BybitPositionIdx::BuyHedge)]
    #[case::sell_open(BybitOrderSide::Sell, false, BybitPositionIdx::SellHedge)]
    #[case::sell_close_long(BybitOrderSide::Sell, true, BybitPositionIdx::BuyHedge)]
    #[case::buy_close_short(BybitOrderSide::Buy, true, BybitPositionIdx::SellHedge)]
    fn test_resolve_position_idx_hedge_mode(
        #[case] side: BybitOrderSide,
        #[case] is_reduce_only: bool,
        #[case] expected: BybitPositionIdx,
    ) {
        let idx = resolve_position_idx(
            Some(BybitPositionMode::BothSides),
            side,
            is_reduce_only,
            None,
        );
        assert_eq!(idx, Some(expected));
    }

    #[rstest]
    fn test_resolve_position_idx_one_way_mode() {
        let idx = resolve_position_idx(
            Some(BybitPositionMode::MergedSingle),
            BybitOrderSide::Buy,
            false,
            None,
        );
        assert_eq!(idx, Some(BybitPositionIdx::OneWay));
    }

    #[rstest]
    fn test_resolve_position_idx_manual_override_wins() {
        let idx = resolve_position_idx(
            Some(BybitPositionMode::BothSides),
            BybitOrderSide::Buy,
            false,
            Some(BybitPositionIdx::SellHedge),
        );
        assert_eq!(idx, Some(BybitPositionIdx::SellHedge));
    }

    #[rstest]
    fn test_resolve_position_idx_returns_none_when_unconfigured() {
        let idx = resolve_position_idx(None, BybitOrderSide::Buy, false, None);
        assert!(idx.is_none());
    }

    #[rstest]
    #[case::linear("BTCUSDT-LINEAR", true)]
    #[case::inverse("BTCUSD-INVERSE", true)]
    #[case::spot("BTCUSDT-SPOT", false)]
    #[case::option("BTC-30JUN25-100000-C-OPTION", false)]
    fn test_parse_derivative_symbol_filters_product_type(
        #[case] symbol_str: &str,
        #[case] keeps: bool,
    ) {
        let result = parse_derivative_symbol(symbol_str);
        assert_eq!(result.is_some(), keeps);
    }

    #[rstest]
    fn test_parse_derivative_symbol_rejects_malformed() {
        assert!(parse_derivative_symbol("not-a-real-symbol").is_none());
    }

    #[rstest]
    #[case::matches_msg("Position mode has not been modified", "110025", true)]
    #[case::matches_code("retCode 110025: noop", "110025", true)]
    #[case::matches_msg_only("Already not been modified", "", true)]
    #[case::wrong_code("retCode 99999: other", "110025", false)]
    #[case::empty_no_modified_msg("retCode 99999", "", false)]
    fn test_is_unchanged_error(#[case] msg: &str, #[case] code: &str, #[case] expected: bool) {
        let err = anyhow::anyhow!("{msg}");
        assert_eq!(is_unchanged_error(&err, code), expected);
    }

    #[rstest]
    #[case::matches("Margin needs to be equal to or greater than 0.5", true)]
    #[case::no_match("Some other error", false)]
    fn test_is_low_margin_error(#[case] msg: &str, #[case] expected: bool) {
        let err = anyhow::anyhow!("{msg}");
        assert_eq!(is_low_margin_error(&err), expected);
    }
}
