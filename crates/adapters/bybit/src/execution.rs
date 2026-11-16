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
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::{ExecutionClient, ExecutionReportTask},
    live::runner::get_exec_event_sender,
    messages::execution::{
        BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports, ModifyOrder,
        QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    DurationNanos, Params, UUID4, UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, unix_nanos_to_iso8601},
    env::get_or_env_var,
    string::secret::SecretString,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, SocketControl,
    execution::failure::CommandFailure,
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    accounts::AccountAny,
    data::CustomData,
    enums::{OmsType, OrderStatus, OrderType},
    events::{OrderCanceled, OrderDeniedReason, OrderEventAny},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price, Quantity},
};
use ustr::Ustr;

use crate::{
    common::{
        consts::BYBIT_VENUE,
        credential::credential_env_vars,
        enums::{
            BybitAccountType, BybitEnvironment, BybitMarginAction, BybitOrderSide,
            BybitOrderSmpType, BybitOrderType, BybitPositionIdx, BybitPositionMode,
            BybitProductType, BybitTpSlMode, resolve_trigger_type,
        },
        parse::{
            BybitTpSlParams, bybit_rejection_due_post_only, extract_raw_symbol, get_price_str,
            make_hedge_venue_position_id, map_time_in_force, nanos_to_millis,
            parse_bybit_tp_sl_params, parse_margin_action,
            resolve_position_idx as resolve_bybit_position_idx, spot_leverage, spot_market_unit,
            trigger_direction,
        },
        rate_limit::batch_send_limit,
        symbol::BybitSymbol,
    },
    config::BybitExecutionClientConfig,
    data_types::{
        BybitMarginBorrowResult, BybitMarginRepayResult, BybitMarginStatusResult,
        register_bybit_custom_data,
    },
    http::{
        client::BybitHttpClient,
        error::{
            BybitCancelOrderError, BybitHttpError, BybitModifyOrderError, BybitSubmitOrderError,
            is_bybit_ambiguous_order_error_code,
        },
        reconciliation::{ReportRead, instrument_in_scope},
    },
    repay::repay_blackout_delay,
    websocket::{
        client::BybitWebSocketClient,
        dispatch::{
            OrderIdentity, OrderStateSnapshot, PendingOperation, WsDispatchState,
            cache_execution_fee_rates, cleanup_terminal, cleanup_terminal_when_ready,
            dispatch_ws_message, drain_pending_fast_executions, flush_pending_fast_executions,
            has_pending_fast_execution, record_venue_order_id,
        },
        messages::{BybitWsAmendOrderParams, BybitWsCancelOrderParams, BybitWsPlaceOrderParams},
    },
};

/// Live execution client for Bybit.
#[derive(Debug)]
pub struct BybitExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: BybitExecutionClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: BybitHttpClient,
    ws_private: BybitWebSocketClient,
    ws_trade: BybitWebSocketClient,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
    shutdown_errors: Vec<String>,
    instruments_cache: Arc<AHashMap<Ustr, InstrumentAny>>,
    dispatch_state: Arc<WsDispatchState>,
}

impl BybitExecutionClient {
    /// Creates a new [`BybitExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(
        core: ExecutionClientCore,
        config: BybitExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let (key_var, secret_var) = credential_env_vars(config.environment);
        let api_key = get_or_env_var(
            config.api_key.clone().map(SecretString::into_inner),
            key_var,
        )?;
        let api_secret = get_or_env_var(
            config.api_secret.clone().map(SecretString::into_inner),
            secret_var,
        )?;
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());

        let http_client = BybitHttpClient::with_credentials(
            api_key.clone(),
            api_secret.clone(),
            Some(config.http_base_url()),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.recv_window_ms,
            proxy_url.clone(),
        )?;
        http_client.set_use_spot_position_reports(config.use_spot_position_reports);

        let mut ws_private = BybitWebSocketClient::new_private(
            config.environment,
            Some(api_key),
            Some(api_secret),
            Some(config.ws_private_url()),
            config.heartbeat_interval_secs,
            config.transport_backend,
            proxy_url.clone(),
        )
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*BYBIT_VENUE),
            "bybit-user-streams",
        ));

        if let Some(secs) = config.auth_timeout_secs {
            ws_private.set_auth_wait_timeout(Duration::from_secs(secs));
        }

        let mut ws_trade = BybitWebSocketClient::new_trade(
            config.environment,
            http_client.clone(),
            Some(config.ws_trade_url()),
            config.heartbeat_interval_secs,
            config.transport_backend,
            proxy_url,
        )
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*BYBIT_VENUE),
            "bybit-trading",
        ));

        if let Some(secs) = config.auth_timeout_secs {
            ws_trade.set_auth_wait_timeout(Duration::from_secs(secs));
        }
        ws_trade.set_recv_window_ms(config.recv_window_ms);

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            None,
        );

        let session_tasks = TaskGroup::new();
        let pending_tasks = TaskGroup::new();
        let dispatch_state = WsDispatchState::new(config.use_ws_execution_fast);

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_private,
            ws_trade,
            session_tasks,
            pending_tasks,
            shutdown_errors: Vec::new(),
            instruments_cache: Arc::new(AHashMap::new()),
            dispatch_state: Arc::new(dispatch_state),
        })
    }

    fn product_types(&self) -> Vec<BybitProductType> {
        if self.config.product_types.is_empty() {
            vec![BybitProductType::Linear]
        } else {
            self.config.product_types.clone()
        }
    }

    async fn refresh_execution_fee_rates(
        http_client: BybitHttpClient,
        product_types: Vec<BybitProductType>,
        instruments: Arc<AHashMap<Ustr, InstrumentAny>>,
        state: Arc<WsDispatchState>,
    ) {
        for product_type in product_types {
            match http_client
                .request_fee_rates(product_type, None, None)
                .await
            {
                Ok(fee_rates) => {
                    state.fee_rates.retain(|instrument_id, _| {
                        BybitProductType::from_suffix(instrument_id.symbol.as_str())
                            != Some(product_type)
                    });

                    let cached =
                        cache_execution_fee_rates(&state, &instruments, product_type, fee_rates);
                    log::debug!("Cached {cached} {product_type:?} execution fee rates");
                }
                Err(e) => log::warn!(
                    "Failed to refresh {product_type:?} execution fee rates, retaining previous rates when available: {e}"
                ),
            }
        }
    }

    async fn initialize_instruments(
        &mut self,
        product_types: Vec<BybitProductType>,
    ) -> anyhow::Result<()> {
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

                log::debug!("Loaded {} {product_type:?} instruments", instruments.len());

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

        if self.config.use_ws_execution_fast {
            let http_client = self.http_client.clone();
            let instruments = Arc::clone(&self.instruments_cache);
            let state = Arc::clone(&self.dispatch_state);
            self.spawn_task("refresh Bybit execution fee rates", async move {
                Self::refresh_execution_fee_rates(http_client, product_types, instruments, state)
                    .await;
                Ok(())
            });
        }

        Ok(())
    }

    async fn connect_private_websocket(&mut self) -> anyhow::Result<()> {
        self.ws_private.connect().await?;
        self.ws_private.wait_until_active(10.0).await?;
        log::debug!("Connected to private WebSocket");

        let account_id = self.core.account_id;
        let instruments = Arc::clone(&self.instruments_cache);
        let state = Arc::clone(&self.dispatch_state);
        let clock = self.clock;
        let stream = self.ws_private.stream();
        let emitter = self.emitter.clone();

        self.session_tasks.spawn(async move {
            pin_mut!(stream);

            while let Some(message) = stream.next().await {
                dispatch_ws_message(&message, account_id, &instruments, clock, &state, &emitter);
            }
        })?;

        self.ws_private.subscribe_orders().await?;

        if self.config.use_ws_execution_fast {
            self.ws_private.subscribe_executions_fast().await?;
        } else {
            self.ws_private.subscribe_executions().await?;
        }

        self.ws_private.subscribe_positions().await?;
        self.ws_private.subscribe_wallet().await?;
        Ok(())
    }

    async fn connect_trade_websocket(&mut self) -> anyhow::Result<()> {
        if self.config.environment == BybitEnvironment::Demo {
            log::info!("Demo mode: Trade WebSocket not available, orders use HTTP REST API");
            return Ok(());
        }

        self.ws_trade.connect().await?;
        self.ws_trade.wait_until_active(10.0).await?;
        log::debug!("Connected to trade WebSocket");

        let account_id = self.core.account_id;
        let instruments = Arc::clone(&self.instruments_cache);
        let state = Arc::clone(&self.dispatch_state);
        let clock = self.clock;
        let stream = self.ws_trade.stream();
        let emitter = self.emitter.clone();

        self.session_tasks.spawn(async move {
            pin_mut!(stream);

            while let Some(message) = stream.next().await {
                dispatch_ws_message(&message, account_id, &instruments, clock, &state, &emitter);
            }
        })?;

        Ok(())
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

    fn borrow_spot_margin(&self, coin: Option<String>, amount: Option<String>, amount_given: bool) {
        let Some(coin) = coin else {
            log::error!("Borrow action requires 'coin' param");
            return;
        };

        let quantity = match Self::margin_quantity(&coin, amount.as_deref(), amount_given) {
            Ok(quantity) => quantity,
            Err(message) => {
                log::error!("{message}");
                let ts_now = self.clock.get_time_ns();

                let result = BybitMarginBorrowResult::new(
                    coin,
                    amount.unwrap_or_default(),
                    false,
                    message.to_string(),
                    ts_now,
                    ts_now,
                );
                self.emitter
                    .send_custom_data(CustomData::from_arc(Arc::new(result)));
                return;
            }
        };

        let amount_str = quantity.to_string();

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        self.spawn_task("borrow_spot", async move {
            let ts_now = clock.get_time_ns();

            let (success, message) = match http_client.borrow_spot(&coin, quantity).await {
                Ok(_) => {
                    log::info!("Borrowed {quantity} {coin}");
                    (true, String::new())
                }
                Err(e) => {
                    let message = format!("{e}");
                    log::error!("Borrow failed: {message}");
                    (false, message)
                }
            };

            let result =
                BybitMarginBorrowResult::new(coin, amount_str, success, message, ts_now, ts_now);
            emitter.send_custom_data(CustomData::from_arc(Arc::new(result)));
            Ok(())
        });
    }

    fn repay_spot_margin(&self, coin: Option<String>, amount: Option<String>, amount_given: bool) {
        let Some(coin) = coin else {
            log::error!("Repay action requires 'coin' param");
            return;
        };

        // A missing amount repays the whole outstanding borrow; a present amount that is
        // not a string or number must fail instead of becoming a repay-all.
        let quantity = match amount.as_deref() {
            Some(amount_str) => match Self::margin_quantity(&coin, Some(amount_str), true) {
                Ok(quantity) => Some(quantity),
                Err(message) => {
                    log::error!("{message}");
                    let ts_now = self.clock.get_time_ns();

                    let result = BybitMarginRepayResult::new(
                        coin,
                        amount,
                        false,
                        "FAIL".to_string(),
                        message.to_string(),
                        ts_now,
                        ts_now,
                    );
                    self.emitter
                        .send_custom_data(CustomData::from_arc(Arc::new(result)));
                    return;
                }
            },
            None if amount_given => {
                let message = format!("invalid 'amount' for {coin}, was not a string or number");
                log::error!("{message}");
                let ts_now = self.clock.get_time_ns();

                let result = BybitMarginRepayResult::new(
                    coin,
                    None,
                    false,
                    "FAIL".to_string(),
                    message,
                    ts_now,
                    ts_now,
                );
                self.emitter
                    .send_custom_data(CustomData::from_arc(Arc::new(result)));
                return;
            }
            None => None,
        };

        let amount_str = quantity.map(|quantity| quantity.to_string());

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        self.spawn_task("repay_spot", async move {
            // Manual repayment uses no-convert repayment, so a balance shortfall fails
            // instead of spending other assets, and only the repay endpoints are blocked
            // by the hourly interest-calculation window. The check runs in the task
            // directly before the request; throttling after the check can still
            // carry a request into the window, which the docs disclose as a venue error.
            let now_ns = clock.get_time_ns();

            if let Some(result) =
                Self::repay_blackout_rejection(&coin, amount_str.as_deref(), now_ns)
            {
                log::warn!("Deferring manual repay: {}", result.message);
                emitter.send_custom_data(CustomData::from_arc(Arc::new(result)));
                return Ok(());
            }

            let ts_now = clock.get_time_ns();

            let (success, result_status, message) =
                match http_client.repay_spot_borrow(&coin, quantity).await {
                    Ok(response) => {
                        let status = response.result.result_status;
                        log::info!(
                            "Repaid {} {coin} (result status {status})",
                            amount_str.as_deref().unwrap_or("all"),
                        );
                        (true, status.to_string(), String::new())
                    }
                    Err(e) => {
                        let message = format!("{e}");
                        log::error!("Repay failed: {message}");
                        (false, "FAIL".to_string(), message)
                    }
                };

            let result = BybitMarginRepayResult::new(
                coin,
                amount_str,
                success,
                result_status,
                message,
                ts_now,
                ts_now,
            );
            emitter.send_custom_data(CustomData::from_arc(Arc::new(result)));
            Ok(())
        });
    }

    fn query_spot_borrow_amount(&self, coin: Option<String>) {
        let Some(coin) = coin else {
            log::error!("Get borrow amount action requires 'coin' param");
            return;
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("get_spot_borrow_amount", async move {
            let borrow_amount = http_client
                .get_spot_borrow_amount(&coin)
                .await
                .with_context(|| format!("failed to query borrow amount for {coin}"))?;
            let ts_now = clock.get_time_ns();
            log::info!("Borrow amount for {coin}: {borrow_amount}");
            let result =
                BybitMarginStatusResult::new(coin, borrow_amount.to_string(), ts_now, ts_now);
            emitter.send_custom_data(CustomData::from_arc(Arc::new(result)));
            Ok(())
        });
    }

    fn margin_quantity(
        coin: &str,
        amount: Option<&str>,
        amount_given: bool,
    ) -> anyhow::Result<Quantity> {
        let Some(amount) = amount else {
            if amount_given {
                anyhow::bail!("invalid 'amount' for {coin}, was not a string or number");
            }

            anyhow::bail!("Borrow action requires an 'amount' param for {coin}");
        };

        let quantity = Quantity::from_str(amount)
            .map_err(|e| anyhow::anyhow!("invalid 'amount' for {coin}: '{amount}', was {e}"))?;
        anyhow::ensure!(
            quantity.as_decimal() > rust_decimal::Decimal::ZERO,
            "invalid 'amount' for {coin}: '{amount}', was not a positive value",
        );
        Ok(quantity)
    }

    /// Builds the failed result for a manual repay inside Bybit's hourly blackout,
    /// or `None` when repayment is allowed.
    #[must_use]
    fn repay_blackout_rejection(
        coin: &str,
        amount: Option<&str>,
        now_ns: UnixNanos,
    ) -> Option<BybitMarginRepayResult> {
        let wait = repay_blackout_delay(now_ns)?;
        let retry_ns = now_ns + DurationNanos::new(wait.as_nanos() as u64);
        let message = format!(
            "Bybit repay blackout window (mm:04:00-mm:05:30 UTC); retry after {}",
            unix_nanos_to_iso8601(retry_ns),
        );
        Some(BybitMarginRepayResult::new(
            coin.to_string(),
            amount.map(str::to_string),
            false,
            "FAIL".to_string(),
            message,
            now_ns,
            now_ns,
        ))
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let future = async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        };

        if let Err(e) = self.pending_tasks.spawn(future) {
            log::warn!("Skipping Bybit {description} after shutdown began: {e}");
        }
    }

    fn abort_pending_tasks(&self) {
        self.pending_tasks.begin_shutdown();
    }

    fn abort_session_tasks(&self) {
        self.session_tasks.begin_shutdown();
    }

    async fn await_pending_tasks(&self) -> anyhow::Result<()> {
        self.pending_tasks.begin_shutdown();
        self.pending_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Bybit execution tasks: {e}"))?;
        Ok(())
    }

    async fn await_session_tasks(&self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.session_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to terminate Bybit execution session tasks: {e}")
            })?;
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.abort_session_tasks();
        self.abort_pending_tasks();
        self.http_client.cancel_all_requests();
        self.ws_private.begin_shutdown();
        self.ws_trade.begin_shutdown();

        if let Err(e) = self.ws_private.close().await {
            self.shutdown_errors
                .push(format!("private WebSocket shutdown failed: {e}"));
        }

        if let Err(e) = self.ws_trade.close().await {
            self.shutdown_errors
                .push(format!("trade WebSocket shutdown failed: {e}"));
        }
        self.dispatch_state.clear_repay_sender();

        if let Err(e) = self.await_session_tasks().await {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = self.await_pending_tasks().await {
            self.shutdown_errors.push(e.to_string());
        }

        if self.dispatch_state.fast_execution {
            flush_pending_fast_executions(
                &self.instruments_cache,
                self.core.account_id,
                self.clock.get_time_ns(),
                &self.dispatch_state,
                &self.emitter,
            );
        }

        self.core.set_disconnected();

        if self.shutdown_errors.is_empty() {
            Ok(())
        } else {
            let errors = std::mem::take(&mut self.shutdown_errors);
            anyhow::bail!("Bybit execution shutdown failed: {}", errors.join("; "))
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

    const fn provides_bulk_position_coverage_for_product_type(
        product_type: BybitProductType,
    ) -> bool {
        !matches!(product_type, BybitProductType::Spot)
    }

    async fn generate_bulk_position_status_reports(
        &self,
        product_types: Vec<BybitProductType>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut reports = Vec::new();

        for product_type in product_types {
            if !Self::provides_bulk_position_coverage_for_product_type(product_type) {
                continue;
            }

            let mut fetched = self
                .http_client
                .request_position_status_reports_scoped(
                    self.core.account_id,
                    product_type,
                    None,
                    self.config.load_ids.as_deref(),
                )
                .await?;
            reports.append(&mut fetched);
        }

        Ok(reports)
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
        resolve_bybit_position_idx(mode, order_side, is_reduce_only, manual_override)
    }

    fn resolve_smp_type(
        &self,
        manual_override: Option<BybitOrderSmpType>,
    ) -> Option<BybitOrderSmpType> {
        manual_override.or(self.config.smp_type)
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
        let Some(symbol) = Self::parse_derivative_symbol(symbol_str) else {
            return;
        };
        let lev = leverage.to_string();
        let result = self
            .http_client
            .set_leverage(symbol.product_type(), symbol.raw_symbol(), &lev, &lev)
            .await;

        match result {
            Ok(_) => log::info!("Set leverage for {symbol_str} to {leverage}"),
            Err(e) if Self::is_unchanged_error(&e, "110043") => {
                log::debug!("Leverage already set for {symbol_str} to {leverage}");
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
        let Some(symbol) = Self::parse_derivative_symbol(symbol_str) else {
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
            Err(e) if Self::is_unchanged_error(&e, "110025") => {
                log::debug!("Symbol `{symbol_str}` position mode already set to `{mode:?}`");
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
            Err(e) if Self::is_unchanged_error(&e, "") => {
                log::debug!("Margin mode already set to {margin_mode:?}");
                Ok(())
            }
            Err(e) if Self::is_low_margin_error(&e) => {
                log::warn!("Cannot set margin mode: {e}");
                Ok(())
            }
            Err(e) => Err(anyhow::Error::from(e).context("failed to set margin mode")),
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

    fn validate_bbo_params(
        order: &OrderAny,
        product_type: BybitProductType,
        tp_sl: &BybitTpSlParams,
    ) -> anyhow::Result<()> {
        if !tp_sl.has_bbo() {
            return Ok(());
        }

        anyhow::ensure!(
            matches!(
                product_type,
                BybitProductType::Linear | BybitProductType::Inverse
            ),
            "`bbo_side_type` and `bbo_level` are only supported for Bybit linear and inverse products"
        );

        let order_type = order.order_type();
        anyhow::ensure!(
            matches!(
                order_type,
                OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
            ),
            "`bbo_side_type` and `bbo_level` are not supported for order type {order_type:?}"
        );

        Ok(())
    }

    fn build_ws_place_params(
        order: &OrderAny,
        product_type: BybitProductType,
        raw_symbol: &str,
        tp_sl: &BybitTpSlParams,
        position_idx: Option<BybitPositionIdx>,
        smp_type: Option<BybitOrderSmpType>,
    ) -> anyhow::Result<BybitWsPlaceOrderParams> {
        let bybit_side = BybitOrderSide::from(order.order_side());
        let (bybit_order_type, is_conditional) = Self::map_order_type(order.order_type())?;
        let time_in_force = map_time_in_force(
            bybit_order_type,
            Some(order.time_in_force()),
            Some(order.is_post_only()),
        )
        .map_err(OrderDeniedReason::UnsupportedTimeInForce)?;
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
            price: if tp_sl.has_bbo() {
                None
            } else {
                order.price().map(|p: Price| p.to_string())
            },
            time_in_force,
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
            tpsl_mode: tp_sl.tpsl_mode.or(has_tp_sl.then_some(BybitTpSlMode::Full)),
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
            smp_type,
            mmp: tp_sl.mmp,
            position_idx,
            bbo_side_type: tp_sl.bbo_side_type,
            bbo_level: tp_sl.bbo_level.clone(),
        })
    }

    fn build_ws_amend_params(
        cmd: &ModifyOrder,
        product_type: BybitProductType,
        raw_symbol: &str,
    ) -> Result<BybitWsAmendOrderParams, String> {
        let order_iv = if let Some(params) = cmd.params.as_ref()
            && let Some(value) = params.get("order_iv")
        {
            Some(get_price_str(params, "order_iv").ok_or_else(|| {
                format!("invalid type for 'order_iv': {value}, expected string or number")
            })?)
        } else {
            None
        };

        Ok(BybitWsAmendOrderParams {
            category: product_type,
            symbol: Ustr::from(raw_symbol),
            order_id: cmd.venue_order_id.map(|v| v.to_string()),
            order_link_id: Some(cmd.client_order_id.to_string()),
            qty: cmd.quantity.map(|q| q.to_string()),
            price: cmd.price.map(|p| p.to_string()),
            trigger_price: cmd.trigger_price.map(|p| p.to_string()),
            take_profit: None,
            stop_loss: None,
            tp_trigger_by: None,
            sl_trigger_by: None,
            order_iv,
        })
    }
}

fn submit_rejection_reason(error: &anyhow::Error) -> Option<&str> {
    for cause in error.chain() {
        if let Some(submit_error) = cause.downcast_ref::<BybitSubmitOrderError>() {
            return match submit_error {
                BybitSubmitOrderError::Rejected { reason } => Some(reason.as_str()),
                BybitSubmitOrderError::MissingOrderId
                | BybitSubmitOrderError::PostSubmitLookup { .. } => None,
            };
        }

        if let Some(BybitHttpError::BybitError {
            error_code,
            message,
        }) = cause.downcast_ref()
            && !http_order_error_is_ambiguous(*error_code)
        {
            return Some(message.as_str());
        }
    }
    None
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
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn provides_bulk_position_coverage(&self, instrument_id: InstrumentId) -> bool {
        // Resolve the suffix directly rather than through `get_product_type_for_instrument`, whose
        // Linear fallback would claim coverage for an identifier this adapter cannot classify. A
        // recognized derivative suffix is still uncovered when its product type is unconfigured,
        // since the bulk loop only queries `product_types()`.
        let Some(product_type) = BybitProductType::from_suffix(instrument_id.symbol.as_str())
        else {
            return false;
        };

        instrument_id.venue == *BYBIT_VENUE
            && instrument_in_scope(instrument_id, self.config.load_ids.as_deref())
            && self.product_types().contains(&product_type)
            && Self::provides_bulk_position_coverage_for_product_type(product_type)
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() && self.pending_tasks.is_open() && self.session_tasks.is_open()
        {
            return Ok(());
        }

        // Margin results publish as custom data, which streaming and catalog writers
        // can only serialize once the types are registered.
        register_bybit_custom_data();

        if !self.pending_tasks.is_open() || !self.session_tasks.is_open() {
            self.teardown_partial_connect().await?;
        }

        if !self.pending_tasks.is_open() {
            self.await_pending_tasks().await?;
            self.pending_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Bybit task generation: {e}"))?;
        }

        if !self.session_tasks.is_open() {
            self.await_session_tasks().await?;
            self.session_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Bybit session generation: {e}"))?;
        }
        let http_client = self.http_client.clone();
        let ws_private = self.ws_private.clone();
        let ws_trade = self.ws_trade.clone();
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {
                http_client.cancel_all_requests();
                ws_private.begin_shutdown();
                ws_trade.begin_shutdown();
            });

        let connect_result: anyhow::Result<()> = async {
            // Reset after a prior disconnect so REST calls are not short-circuited
            self.http_client.reset_cancellation_token();

            let product_types = self.product_types();
            self.initialize_instruments(product_types).await?;

            self.ws_private.set_account_id(self.core.account_id);
            self.ws_trade.set_account_id(self.core.account_id);

            if self.config.auto_repay_spot_borrows {
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                self.dispatch_state.set_repay_sender(tx);

                let http_client = self.http_client.clone();
                let clock = self.clock;
                self.session_tasks.spawn(async move {
                    crate::repay::run_spot_repay_consumer(rx, http_client, clock).await;
                })?;
            }

            self.connect_private_websocket().await?;
            self.connect_trade_websocket().await?;

            self.apply_account_configuration().await?;

            let account_state = self
                .http_client
                .request_account_state(BybitAccountType::Unified, self.core.account_id)
                .await
                .context("failed to request Bybit account state")?;

            if !account_state.balances.is_empty() {
                log::debug!(
                    "Received account state with {} balance(s)",
                    account_state.balances.len()
                );
            }
            self.emitter.send_account_state(account_state);

            self.await_account_registered(30.0).await?;

            Ok(())
        }
        .await;

        if let Err(e) = connect_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!("Bybit startup teardown failed: {teardown_error}")));
            }
            return Err(e);
        }

        setup_guard.disarm();
        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.teardown_partial_connect().await?;
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        let params = cmd.params.as_ref();

        let action = params
            .and_then(|params| params.get_str("action"))
            .and_then(parse_margin_action);

        let Some(action) = action else {
            if let Some(action_str) = params.and_then(|params| params.get_str("action")) {
                log::warn!("Unknown query_account action: {action_str}");
            }

            self.update_account_state();
            return Ok(());
        };

        let coin = params
            .and_then(|params| params.get_str("coin"))
            .map(String::from);
        let amount_given = params.is_some_and(|params| params.contains_key("amount"));
        let amount = params.and_then(|params| get_price_str(params, "amount"));
        match action {
            BybitMarginAction::Borrow => self.borrow_spot_margin(coin, amount, amount_given),
            BybitMarginAction::Repay => self.repay_spot_margin(coin, amount, amount_given),
            BybitMarginAction::GetBorrowAmount => self.query_spot_borrow_amount(coin),
        }

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let account_id = self.core.account_id;
        let strategy_id = cmd.strategy_id;
        let state = Arc::clone(&self.dispatch_state);
        let instruments = Arc::clone(&self.instruments_cache);
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
                    if state.fast_execution {
                        let recoverable = !report.order_status.is_closed()
                            || has_pending_fast_execution(report.venue_order_id, &state);
                        if recoverable
                            && let Some(client_order_id) = report.client_order_id
                            && !state.order_identities.contains_key(&client_order_id)
                            && let Some(order_side) = report.order_side
                        {
                            state.order_identities.insert(
                                client_order_id,
                                OrderIdentity {
                                    instrument_id: report.instrument_id,
                                    strategy_id,
                                    order_side,
                                    order_type: report.order_type,
                                    venue_position_id: report.venue_position_id,
                                },
                            );
                        }

                        cache_reconciliation_order_identity(
                            account_id,
                            None,
                            &state,
                            &report,
                            &instruments,
                            &emitter,
                        );
                    }

                    emitter.send_order_status_report(report);
                }
                Ok(None) => {
                    log::warn!("Order not found: client_order_id={client_order_id}, venue_order_id={venue_order_id:?}");
                }
                Err(e) => {
                    log::warn!("Failed to query order: {e}");
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
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event, info);
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

        self.session_tasks.spawn(async move {
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
                log::debug!("Instruments initialized: count={}", all_instruments.len());
            }
        })?;

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

        self.abort_session_tasks();
        self.abort_pending_tasks();
        self.http_client.cancel_all_requests();
        self.ws_private.begin_shutdown();
        self.ws_trade.begin_shutdown();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn generate_order_status_report_task(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> Option<ExecutionReportTask<Option<OrderStatusReport>>> {
        let http_client = self.http_client.clone();
        let core = self.core.clone();
        let account_id = core.account_id;
        let state = Arc::clone(&self.dispatch_state);
        let instruments = Arc::clone(&self.instruments_cache);
        let emitter = self.emitter.clone();

        let instrument = cmd.instrument_id.map(|instrument_id| {
            (
                instrument_id,
                self.get_product_type_for_instrument(instrument_id),
            )
        });

        let load_ids = self.config.load_ids.clone();
        let command = cmd.clone();
        Some(ExecutionReportTask::new(
            async move {
                let Some((instrument_id, product_type)) = instrument else {
                    anyhow::bail!("generate_order_status_report requires instrument_id: {command}");
                };

                let reports = http_client
                    .request_order_status_reports_scoped(
                        account_id,
                        product_type,
                        Some(instrument_id),
                        false,
                        None,
                        None,
                        None,
                        load_ids.as_deref(),
                        (None, None),
                    )
                    .await?
                    .into_reports()?;

                Ok(reports.into_iter().find(|report| {
                    command
                        .client_order_id
                        .is_none_or(|id| report.client_order_id == Some(id))
                        && command
                            .venue_order_id
                            .is_none_or(|id| report.venue_order_id.as_str() == id.as_str())
                }))
            },
            move |report| {
                if let Some(report) = &report {
                    cache_reconciliation_order_identity(
                        account_id,
                        Some(&core),
                        &state,
                        report,
                        &instruments,
                        &emitter,
                    );
                }

                Ok(report)
            },
        ))
    }

    fn generate_order_status_reports_task(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> Option<ExecutionReportTask<Vec<OrderStatusReport>>> {
        let http_client = self.http_client.clone();
        let core = self.core.clone();
        let account_id = core.account_id;
        let state = Arc::clone(&self.dispatch_state);
        let instruments = Arc::clone(&self.instruments_cache);
        let emitter = self.emitter.clone();
        let product_types = cmd.instrument_id.map_or_else(
            || self.product_types(),
            |instrument_id| vec![self.get_product_type_for_instrument(instrument_id)],
        );

        let load_ids = self.config.load_ids.clone();
        let command = cmd.clone();
        Some(ExecutionReportTask::new(
            async move {
                let mut reports = collect_order_status_reports(
                    &http_client,
                    account_id,
                    product_types,
                    &command,
                    load_ids.as_deref(),
                )
                .await?
                .into_reports()?;

                if let Some(start) = command.start {
                    reports.retain(|report| report.ts_last >= start);
                }

                if let Some(end) = command.end {
                    reports.retain(|report| report.ts_last <= end);
                }

                Ok(reports)
            },
            move |reports| {
                for report in &reports {
                    cache_reconciliation_order_identity(
                        account_id,
                        Some(&core),
                        &state,
                        report,
                        &instruments,
                        &emitter,
                    );
                }

                Ok(reports)
            },
        ))
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let task = self
            .generate_order_status_report_task(cmd)
            .expect("Bybit single-order report task");
        task.collection.await;
        (task.finish)()
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let task = self
            .generate_order_status_reports_task(cmd)
            .expect("Bybit bulk order report task");
        task.collection.await;
        (task.finish)()
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        collect_fill_reports(
            &self.http_client,
            self.core.account_id,
            cmd.instrument_id.map_or_else(
                || self.product_types(),
                |id| vec![self.get_product_type_for_instrument(id)],
            ),
            &cmd,
            self.config.load_ids.as_deref(),
        )
        .await?
        .into_reports()
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        if let Some(instrument_id) = cmd.instrument_id {
            let product_type = self.get_product_type_for_instrument(instrument_id);
            self.http_client
                .request_position_status_reports_scoped(
                    self.core.account_id,
                    product_type,
                    Some(instrument_id),
                    self.config.load_ids.as_deref(),
                )
                .await
        } else {
            self.generate_bulk_position_status_reports(self.product_types())
                .await
        }
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();
        let start = lookback_mins
            .map(DurationNanos::try_from_mins)
            .transpose()?
            .map(|lookback| {
                let cutoff = ts_now.saturating_sub(lookback).as_u64();
                UnixNanos::from(cutoff / NANOSECONDS_IN_MILLISECOND * NANOSECONDS_IN_MILLISECOND)
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

        let position_reports_fut = async {
            let product_types = self.product_types();
            let skip_spot = product_types.iter().any(|product_type| {
                !Self::provides_bulk_position_coverage_for_product_type(*product_type)
            });

            if skip_spot {
                log::info!(
                    "SPOT mass-status position coverage is unavailable because wallet balances cannot be attributed to pairs"
                );
            }

            self.generate_bulk_position_status_reports(product_types)
                .await
        };

        let (order_read, fill_read, position_reports) = tokio::try_join!(
            collect_order_status_reports(
                &self.http_client,
                self.core.account_id,
                self.product_types(),
                &order_cmd,
                self.config.load_ids.as_deref()
            ),
            collect_fill_reports(
                &self.http_client,
                self.core.account_id,
                self.product_types(),
                &fill_cmd,
                self.config.load_ids.as_deref()
            ),
            position_reports_fut,
        )?;
        // Leave room for request latency in the venue's 24-hour terminal-order retention
        let within_history = lookback_mins.is_none_or(|mins| mins < 24 * 60);
        let option_history_covered = !self
            .config
            .product_types
            .contains(&BybitProductType::Option);

        let orders_linked = if start.is_some() {
            let order_ids: AHashSet<_> = order_read
                .reports
                .iter()
                .map(|report| report.venue_order_id)
                .collect();
            let cache = self.core.cache();
            fill_read.reports.iter().all(|fill| {
                order_ids.contains(&fill.venue_order_id)
                    || fill
                        .client_order_id
                        .is_some_and(|id| cache.order_exists(&id))
                    || cache
                        .client_order_id(&fill.venue_order_id)
                        .is_some_and(|id| cache.order_exists(id))
            })
        } else {
            true
        };

        let complete = within_history
            && option_history_covered
            && orders_linked
            && !order_read.historical_incomplete
            && !fill_read.historical_incomplete;

        if !within_history {
            log::warn!(
                "Reconciliation lookback reaches Bybit's 24-hour terminal-order retention, history is incomplete"
            );
        }

        if !option_history_covered {
            log::warn!("Bulk option fill history defaults to BTC, history is incomplete");
        }

        if !orders_linked {
            log::warn!("Reconciliation fills lack order evidence, history is incomplete");
        }

        let order_reports = order_read.reports;
        let fill_reports = fill_read.reports;

        for report in &order_reports {
            cache_reconciliation_order_identity(
                self.core.account_id,
                Some(&self.core),
                &self.dispatch_state,
                report,
                &self.instruments_cache,
                &self.emitter,
            );
        }

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
        if start.is_some() {
            mass_status.set_report_window(start, complete);
        }

        Ok(Some(mass_status))
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let instrument_id = order.instrument_id();
        let product_type = self.get_product_type_for_instrument(instrument_id);

        // Validate order params before emitting submitted event
        if let Err(denied) = validate_order(&order, product_type) {
            self.emitter.emit_order_denied(&order, &denied.to_string());
            return Ok(());
        }

        let tp_sl = match parse_bybit_tp_sl_params(cmd.params.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                let denied = OrderDeniedReason::ValidationFailed {
                    detail: e.to_string(),
                };
                self.emitter.emit_order_denied(&order, &denied.to_string());
                return Ok(());
            }
        };

        if let Err(e) = Self::validate_bbo_params(&order, product_type, &tp_sl) {
            let denied = OrderDeniedReason::ValidationFailed {
                detail: e.to_string(),
            };
            self.emitter.emit_order_denied(&order, &denied.to_string());
            return Ok(());
        }

        // The demo HTTP create-order entry cannot carry TP/SL trigger prices (only the mainnet
        // WS path can), so deny rather than submit an order missing the user's trigger prices.
        if self.config.environment == BybitEnvironment::Demo
            && (tp_sl.tp_trigger_price.is_some() || tp_sl.sl_trigger_price.is_some())
        {
            let denied = OrderDeniedReason::UnsupportedTpSl {
                detail: "TP/SL trigger prices are not supported in demo mode".to_string(),
            };
            self.emitter.emit_order_denied(&order, &denied.to_string());
            return Ok(());
        }

        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        self.emitter.emit_order_submitted(&order);

        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let bybit_side = BybitOrderSide::from(order.order_side());
        let position_idx = self.resolve_position_idx(
            instrument_id,
            bybit_side,
            order.is_reduce_only(),
            tp_sl.position_idx,
        );
        let venue_position_id =
            position_idx.and_then(|idx| make_hedge_venue_position_id(instrument_id, idx as i32));

        self.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side: order.order_side(),
                order_type: order.order_type(),
                venue_position_id,
            },
        );

        // Seed for BBO reconciliation: venue may replace the submitted price
        self.dispatch_state.order_snapshots.insert(
            client_order_id,
            OrderStateSnapshot {
                quantity: order.quantity(),
                price: order.price(),
                trigger_price: order.trigger_price(),
            },
        );

        let smp_type = self.resolve_smp_type(tp_sl.smp_type);

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
            let bbo_side_type = tp_sl.bbo_side_type;
            let bbo_level = tp_sl.bbo_level.clone();
            let native_tp_sl = tp_sl.to_native_tp_sl();
            let dispatch_state = Arc::clone(&self.dispatch_state);

            self.spawn_task("submit_order_http", async move {
                let native_tp_sl_ref = (!native_tp_sl.is_empty()).then_some(&native_tp_sl);
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
                        bbo_side_type,
                        bbo_level,
                        smp_type,
                        native_tp_sl_ref,
                    )
                    .await;

                if let Err(e) = result {
                    if let Some(reason) = submit_rejection_reason(&e) {
                        dispatch_state.order_identities.remove(&client_order_id);
                        dispatch_state.order_snapshots.remove(&client_order_id);
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            reason,
                            ts_event,
                            bybit_rejection_due_post_only(reason),
                        );
                        anyhow::bail!("submit order rejected: {reason}");
                    }

                    log::warn!(
                        "Submit failure without confirmed venue rejection for {client_order_id}: \
                         {e}; awaiting reconciliation",
                    );
                    return Ok(());
                }

                Ok(())
            });

            return Ok(());
        }

        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let params = Self::build_ws_place_params(
            &order,
            product_type,
            raw_symbol,
            &tp_sl,
            position_idx,
            smp_type,
        )?;

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("submit_order", async move {
            let req_id = UUID4::new().to_string();
            dispatch_state.pending_requests.insert(
                req_id.clone(),
                (
                    vec![client_order_id],
                    vec![None],
                    PendingOperation::Place,
                    Vec::new(),
                ),
            );

            if let Err(e) = ws_trade.place_order_with_id(params, req_id.clone()).await {
                dispatch_state.pending_requests.remove(&req_id);
                dispatch_state.order_identities.remove(&client_order_id);
                dispatch_state.order_snapshots.remove(&client_order_id);
                let reason = e.to_string();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &reason,
                    clock.get_time_ns(),
                    false,
                );
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
                let denied = OrderDeniedReason::ValidationFailed {
                    detail: e.to_string(),
                }
                .to_string();

                for cid in &cmd.order_list.client_order_ids {
                    if let Some(order) = cache.order(cid) {
                        self.emitter.emit_order_denied(&order, &denied);
                    }
                }
                return Ok(());
            }
        };

        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);

        // The demo HTTP create-order entry cannot carry TP/SL trigger prices (only the mainnet
        // WS path can), so deny rather than submit orders missing the user's trigger prices.
        if self.config.environment == BybitEnvironment::Demo
            && (tp_sl.tp_trigger_price.is_some() || tp_sl.sl_trigger_price.is_some())
        {
            let cache = self.core.cache();
            let denied = OrderDeniedReason::UnsupportedTpSl {
                detail: "TP/SL trigger prices are not supported in demo mode".to_string(),
            }
            .to_string();

            for cid in &cmd.order_list.client_order_ids {
                if let Some(order) = cache.order(cid) {
                    self.emitter.emit_order_denied(&order, &denied);
                }
            }
            return Ok(());
        }

        let strategy_id = cmd.strategy_id;

        let mut valid_orders = Vec::with_capacity(cmd.order_list.client_order_ids.len());
        {
            let cache = self.core.cache();
            let order_list_id = cmd.order_list.id;
            let list_denied = OrderDeniedReason::OrderListDenied { order_list_id };
            // (offending leg, reason for the offending leg, reason for the remaining legs). A
            // single offending leg carries its specific reason and the rest render
            // `ORDER_LIST_DENIED`; a list-level failure renders the same reason for every leg.
            let mut denial: Option<(ClientOrderId, OrderDeniedReason, OrderDeniedReason)> = None;

            for cid in &cmd.order_list.client_order_ids {
                let Some(order) = cache.order(cid) else {
                    let reason = OrderDeniedReason::OrderListIncomplete { order_list_id };
                    denial = Some((*cid, reason.clone(), reason));
                    break;
                };

                if order.is_closed() {
                    denial = Some((
                        *cid,
                        OrderDeniedReason::ValidationFailed {
                            detail: format!("cannot submit closed order {cid}"),
                        },
                        list_denied,
                    ));
                    break;
                }

                if let Err(reason) = validate_order(&order, product_type) {
                    denial = Some((*cid, reason, list_denied));
                    break;
                }

                if let Err(e) = Self::validate_bbo_params(&order, product_type, &tp_sl) {
                    denial = Some((
                        *cid,
                        OrderDeniedReason::ValidationFailed {
                            detail: e.to_string(),
                        },
                        list_denied,
                    ));
                    break;
                }

                valid_orders.push(order.clone());
            }

            // Deny the entire list if any leg fails validation
            if let Some((offender, offender_reason, rest_reason)) = denial {
                let offender_reason = offender_reason.to_string();
                let rest_reason = rest_reason.to_string();

                for cid in &cmd.order_list.client_order_ids {
                    if let Some(order) = cache.order(cid) {
                        let reason = if *cid == offender {
                            offender_reason.as_str()
                        } else {
                            rest_reason.as_str()
                        };
                        self.emitter.emit_order_denied(&order, reason);
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
            let bybit_side = BybitOrderSide::from(order.order_side());
            let position_idx = self.resolve_position_idx(
                instrument_id,
                bybit_side,
                order.is_reduce_only(),
                tp_sl.position_idx,
            );
            let venue_position_id = position_idx
                .and_then(|idx| make_hedge_venue_position_id(instrument_id, idx as i32));
            self.dispatch_state.order_identities.insert(
                order.client_order_id(),
                OrderIdentity {
                    instrument_id,
                    strategy_id,
                    order_side: order.order_side(),
                    order_type: order.order_type(),
                    venue_position_id,
                },
            );
            self.dispatch_state.order_snapshots.insert(
                order.client_order_id(),
                OrderStateSnapshot {
                    quantity: order.quantity(),
                    price: order.price(),
                    trigger_price: order.trigger_price(),
                },
            );
        }

        let emitter = self.emitter.clone();
        let clock = self.clock;

        let smp_type = self.resolve_smp_type(tp_sl.smp_type);

        // Demo mode: submit individually via HTTP
        if self.config.environment == BybitEnvironment::Demo {
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let is_leverage = tp_sl.is_leverage;
            let bbo_side_type = tp_sl.bbo_side_type;
            let bbo_level = tp_sl.bbo_level.clone();
            let native_tp_sl = tp_sl.to_native_tp_sl();
            let dispatch_state = Arc::clone(&self.dispatch_state);

            let order_data: Vec<_> = valid_orders
                .iter()
                .map(|o| {
                    let bybit_side = BybitOrderSide::from(o.order_side());
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
                let native_tp_sl_ref = (!native_tp_sl.is_empty()).then_some(&native_tp_sl);

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
                            bbo_side_type,
                            bbo_level.clone(),
                            smp_type,
                            native_tp_sl_ref,
                        )
                        .await
                    {
                        if let Some(reason) = submit_rejection_reason(&e) {
                            dispatch_state.order_identities.remove(&cid);
                            dispatch_state.order_snapshots.remove(&cid);
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_rejected_event(
                                strategy_id,
                                instrument_id,
                                cid,
                                reason,
                                ts_event,
                                bybit_rejection_due_post_only(reason),
                            );
                            continue;
                        }

                        log::warn!(
                            "Submit failure without confirmed venue rejection for {cid}: {e}; \
                             awaiting reconciliation",
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
            let bybit_side = BybitOrderSide::from(order.order_side());
            let position_idx = self.resolve_position_idx(
                instrument_id,
                bybit_side,
                order.is_reduce_only(),
                tp_sl.position_idx,
            );
            let params = Self::build_ws_place_params(
                order,
                product_type,
                raw_symbol,
                &tp_sl,
                position_idx,
                smp_type,
            )
            .expect("validated above");
            order_params.push(params);
            client_order_ids.push(order.client_order_id());
        }

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("submit_order_list", async move {
            let req_ids = BybitWebSocketClient::batch_request_ids(product_type, order_params.len());
            for (req_id, chunk_cids) in req_ids.iter().zip(
                client_order_ids
                    .chunks(batch_send_limit(product_type))
                    .map(|chunk| chunk.to_vec()),
            ) {
                let chunk_voids = vec![None; chunk_cids.len()];
                dispatch_state.pending_requests.insert(
                    req_id.clone(),
                    (chunk_cids, chunk_voids, PendingOperation::Place, Vec::new()),
                );
            }

            if let Err(e) = ws_trade
                .batch_place_orders_with_ids(order_params, req_ids.clone())
                .await
            {
                for req_id in req_ids {
                    dispatch_state.pending_requests.remove(&req_id);
                }
                let reason = e.to_string();
                let ts_event = clock.get_time_ns();

                for client_order_id in client_order_ids {
                    dispatch_state.order_identities.remove(&client_order_id);
                    dispatch_state.order_snapshots.remove(&client_order_id);
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        &reason,
                        ts_event,
                        false,
                    );
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
            log::warn!(
                "Modify command failed local validation for {client_order_id}: {}",
                "Option params (order_iv) are not supported in demo mode",
            );
            emitter.emit_order_modify_denied_event(
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                "Option params (order_iv) are not supported in demo mode",
                clock.get_time_ns(),
                Some(cmd.command_id),
            );
            return Ok(());
        }

        if self.config.environment == BybitEnvironment::Demo {
            let http_client = self.http_client.clone();
            let account_id = self.core.account_id;
            let quantity = cmd.quantity;
            let price = cmd.price;
            let trigger_price = cmd.trigger_price;

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
                        trigger_price,
                    )
                    .await;

                if let Err(e) = result {
                    match classify_modify_http_failure(&e) {
                        CommandFailure::VenueRejected(reason) => {
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_modify_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                venue_order_id,
                                &format!("modify-order-error: {reason}"),
                                ts_event,
                                Some(cmd.command_id),
                            );
                            anyhow::bail!("modify order rejected: {reason}");
                        }
                        CommandFailure::NotSent(reason) => {
                            emitter.emit_order_modify_denied_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                venue_order_id,
                                &reason,
                                clock.get_time_ns(),
                                Some(cmd.command_id),
                            );
                        }
                        CommandFailure::Ambiguous(reason) => {
                            log::warn!(
                                "Ambiguous HTTP modify failure for {client_order_id}, awaiting reconciliation: {reason}"
                            );
                        }
                    }
                }

                Ok(())
            });

            return Ok(());
        }

        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());

        let params = match Self::build_ws_amend_params(&cmd, product_type, raw_symbol) {
            Ok(params) => params,
            Err(reason) => {
                log::warn!(
                    "Modify command failed local validation for {client_order_id}: {reason}",
                );
                emitter.emit_order_modify_denied_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &reason,
                    clock.get_time_ns(),
                    Some(cmd.command_id),
                );
                return Ok(());
            }
        };

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("modify_order", async move {
            let req_id = UUID4::new().to_string();
            dispatch_state.pending_requests.insert(
                req_id.clone(),
                (
                    vec![client_order_id],
                    vec![venue_order_id],
                    PendingOperation::Amend,
                    vec![Some(cmd.command_id)],
                ),
            );

            if let Err(e) = ws_trade.amend_order_with_id(params, req_id.clone()).await {
                dispatch_state.pending_requests.remove(&req_id);
                emitter.emit_order_modify_denied_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &e.to_string(),
                    clock.get_time_ns(),
                    Some(cmd.command_id),
                );
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_modify_orders(&self, cmd: BatchModifyOrders) -> anyhow::Result<()> {
        if cmd.modifies.is_empty() {
            return Ok(());
        }

        if self.config.environment == BybitEnvironment::Demo {
            for modify in cmd.modifies {
                self.modify_order(modify)?;
            }

            return Ok(());
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let raw_symbol = extract_raw_symbol(instrument_id.symbol.as_str());
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let mut amend_params = Vec::with_capacity(cmd.modifies.len());
        let mut strategy_ids = Vec::with_capacity(cmd.modifies.len());
        let mut client_order_ids = Vec::with_capacity(cmd.modifies.len());
        let mut venue_order_ids = Vec::with_capacity(cmd.modifies.len());
        let mut command_ids = Vec::with_capacity(cmd.modifies.len());

        for modify in &cmd.modifies {
            match Self::build_ws_amend_params(modify, product_type, raw_symbol) {
                Ok(params) => {
                    amend_params.push(params);
                    strategy_ids.push(modify.strategy_id);
                    client_order_ids.push(modify.client_order_id);
                    venue_order_ids.push(modify.venue_order_id);
                    command_ids.push(modify.command_id);
                }
                Err(reason) => {
                    log::warn!(
                        "Modify command failed local validation for {}: {reason}",
                        modify.client_order_id,
                    );
                    emitter.emit_order_modify_denied_event(
                        modify.strategy_id,
                        instrument_id,
                        modify.client_order_id,
                        modify.venue_order_id,
                        &reason,
                        clock.get_time_ns(),
                        Some(modify.command_id),
                    );
                }
            }
        }

        if amend_params.is_empty() {
            return Ok(());
        }

        let ws_trade = self.ws_trade.clone();
        let dispatch_state = Arc::clone(&self.dispatch_state);

        self.spawn_task("batch_modify_orders", async move {
            let req_ids = BybitWebSocketClient::batch_request_ids(product_type, amend_params.len());
            let chunk_limit = batch_send_limit(product_type);

            for (req_id, ((chunk_cids, chunk_voids), chunk_commands)) in req_ids.iter().zip(
                client_order_ids
                    .chunks(chunk_limit)
                    .map(|chunk| chunk.to_vec())
                    .zip(
                        venue_order_ids
                            .chunks(chunk_limit)
                            .map(|chunk| chunk.to_vec()),
                    )
                    .zip(command_ids.chunks(chunk_limit)),
            ) {
                dispatch_state.pending_requests.insert(
                    req_id.clone(),
                    (
                        chunk_cids,
                        chunk_voids,
                        PendingOperation::Amend,
                        chunk_commands.iter().copied().map(Some).collect(),
                    ),
                );
            }

            if let Err(e) = ws_trade
                .batch_amend_orders_with_ids(amend_params, req_ids.clone())
                .await
            {
                for req_id in req_ids {
                    dispatch_state.pending_requests.remove(&req_id);
                }

                let reason = e.to_string();
                let ts_event = clock.get_time_ns();

                for (((strategy_id, client_order_id), venue_order_id), command_id) in strategy_ids
                    .into_iter()
                    .zip(client_order_ids)
                    .zip(venue_order_ids)
                    .zip(command_ids)
                {
                    emitter.emit_order_modify_denied_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &reason,
                        ts_event,
                        Some(command_id),
                    );
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

                if let Ok(ref report) = result {
                    emit_cancel_confirmation(&emitter, strategy_id, client_order_id, report);
                }

                if let Err(e) = result {
                    match classify_cancel_http_failure(&e) {
                        CommandFailure::VenueRejected(reason) => {
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_cancel_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                venue_order_id,
                                &format!("cancel-order-error: {reason}"),
                                ts_event,
                                Some(cmd.command_id),
                            );
                            anyhow::bail!("cancel order rejected: {reason}");
                        }
                        CommandFailure::NotSent(reason) => {
                            emitter.emit_order_cancel_denied_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                venue_order_id,
                                &reason,
                                clock.get_time_ns(),
                                Some(cmd.command_id),
                            );
                        }
                        CommandFailure::Ambiguous(reason) => {
                            log::warn!(
                                "Ambiguous HTTP cancel failure for {client_order_id}, awaiting reconciliation: {reason}"
                            );
                        }
                    }
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
            let req_id = UUID4::new().to_string();
            dispatch_state.pending_requests.insert(
                req_id.clone(),
                (
                    vec![client_order_id],
                    vec![venue_order_id],
                    PendingOperation::Cancel,
                    vec![Some(cmd.command_id)],
                ),
            );

            if let Err(e) = ws_trade.cancel_order_with_id(params, req_id.clone()).await {
                dispatch_state.pending_requests.remove(&req_id);
                emitter.emit_order_cancel_denied_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &e.to_string(),
                    clock.get_time_ns(),
                    Some(cmd.command_id),
                );
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        if cmd.order_side.is_some() {
            // Bybit cancel-all has no side parameter, so select matching open
            // orders and cancel their explicit IDs through the batch path.
            let cancels: Vec<CancelOrder> = {
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

            return self.batch_cancel_orders(BatchCancelOrders {
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
            });
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let account_id = self.core.account_id;
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();

        let mut orders: Vec<_> = self
            .core
            .cache()
            .orders_open(None, Some(&instrument_id), None, Some(&account_id), None)
            .into_iter()
            .map(|order| {
                (
                    order.client_order_id(),
                    order.venue_order_id(),
                    order.strategy_id(),
                )
            })
            .collect();

        let timeout = Duration::from_secs(self.config.http_timeout_secs);
        let interval = Duration::from_millis(self.config.retry_delay_initial_ms.max(1));

        self.spawn_task("cancel_all_orders", async move {
            let confirmation = async {
                let reports = http_client
                    .cancel_all_orders(account_id, product_type, instrument_id)
                    .await
                    .with_context(|| {
                        format!("failed to cancel Bybit orders for {instrument_id}")
                    })?;

                for report in reports {
                    if !report.order_status.is_closed() {
                        continue;
                    }

                    if let Some(index) =
                        orders
                            .iter()
                            .position(|(client_order_id, venue_order_id, _)| {
                                venue_order_id.map_or(
                                    report.client_order_id == Some(*client_order_id),
                                    |id| id == report.venue_order_id,
                                )
                            })
                    {
                        let (client_order_id, _, strategy_id) = orders.swap_remove(index);
                        emit_cancel_confirmation(&emitter, strategy_id, client_order_id, &report);
                    }
                }

                while !orders.is_empty() {
                    for index in (0..orders.len()).rev() {
                        let (client_order_id, venue_order_id, strategy_id) = orders[index];
                        if let Some(report) = http_client
                            .query_order(
                                account_id,
                                product_type,
                                instrument_id,
                                Some(client_order_id),
                                venue_order_id,
                            )
                            .await?
                            && report.order_status.is_closed()
                        {
                            orders.swap_remove(index);
                            emit_cancel_confirmation(
                                &emitter,
                                strategy_id,
                                client_order_id,
                                &report,
                            );
                        }
                    }

                    if !orders.is_empty() {
                        tokio::time::sleep(interval).await;
                    }
                }

                Ok(())
            };

            tokio::time::timeout(timeout, confirmation)
                .await
                .context("timed out confirming Bybit cancel-all orders")?
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        if self.config.environment == BybitEnvironment::Demo {
            for cancel in cmd.cancels {
                self.cancel_order(cancel)?;
            }

            return Ok(());
        }

        let instrument_id = cmd.instrument_id;
        let product_type = self.get_product_type_for_instrument(instrument_id);
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let raw_symbol = Ustr::from(extract_raw_symbol(instrument_id.symbol.as_str()));

        let mut cancel_params = Vec::with_capacity(cmd.cancels.len());
        let strategy_ids: Vec<_> = cmd.cancels.iter().map(|c| c.strategy_id).collect();
        let client_order_ids: Vec<_> = cmd.cancels.iter().map(|c| c.client_order_id).collect();
        let command_ids: Vec<_> = cmd.cancels.iter().map(|c| c.command_id).collect();
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
            let req_ids =
                BybitWebSocketClient::batch_request_ids(product_type, cancel_params.len());
            let chunk_limit = batch_send_limit(product_type);

            for (req_id, ((chunk_cids, chunk_voids), chunk_commands)) in req_ids.iter().zip(
                client_order_ids
                    .chunks(chunk_limit)
                    .map(|chunk| chunk.to_vec())
                    .zip(
                        venue_order_ids
                            .chunks(chunk_limit)
                            .map(|chunk| chunk.to_vec()),
                    )
                    .zip(command_ids.chunks(chunk_limit)),
            ) {
                dispatch_state.pending_requests.insert(
                    req_id.clone(),
                    (
                        chunk_cids,
                        chunk_voids,
                        PendingOperation::Cancel,
                        chunk_commands.iter().copied().map(Some).collect(),
                    ),
                );
            }

            if let Err(e) = ws_trade
                .batch_cancel_orders_with_ids(cancel_params, req_ids.clone())
                .await
            {
                for req_id in req_ids {
                    dispatch_state.pending_requests.remove(&req_id);
                }
                let reason = e.to_string();
                let ts_event = clock.get_time_ns();

                for (((strategy_id, client_order_id), venue_order_id), command_id) in strategy_ids
                    .into_iter()
                    .zip(client_order_ids)
                    .zip(venue_order_ids)
                    .zip(command_ids)
                {
                    emitter.emit_order_cancel_denied_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &reason,
                        ts_event,
                        Some(command_id),
                    );
                }
            }
            Ok(())
        });

        Ok(())
    }
}

fn validate_order(
    order: &OrderAny,
    product_type: BybitProductType,
) -> Result<(), OrderDeniedReason> {
    let (bybit_order_type, is_conditional) =
        BybitExecutionClient::map_order_type(order.order_type()).map_err(|_| {
            OrderDeniedReason::UnsupportedOrderType {
                order_type: order.order_type(),
            }
        })?;

    map_time_in_force(
        bybit_order_type,
        Some(order.time_in_force()),
        Some(order.is_post_only()),
    )
    .map_err(OrderDeniedReason::UnsupportedTimeInForce)?;

    if product_type == BybitProductType::Option && is_conditional {
        return Err(OrderDeniedReason::UnsupportedOrderType {
            order_type: order.order_type(),
        });
    }

    Ok(())
}

fn emit_cancel_confirmation(
    emitter: &ExecutionEventEmitter,
    strategy_id: StrategyId,
    client_order_id: ClientOrderId,
    report: &OrderStatusReport,
) {
    // Status reports can infer fills before the execution stream delivers them
    if report.order_status != OrderStatus::Canceled {
        return;
    }

    let canceled = OrderCanceled::new(
        emitter.trader_id(),
        strategy_id,
        report.instrument_id,
        client_order_id,
        UUID4::new(),
        report.ts_last,
        report.ts_init,
        false,
        Some(report.venue_order_id),
        Some(report.account_id),
        None,
    );
    emitter.send_order_event(OrderEventAny::Canceled(canceled));
}

fn classify_cancel_http_failure(error: &anyhow::Error) -> CommandFailure {
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<BybitCancelOrderError>().is_some())
    {
        return CommandFailure::ambiguous(error.to_string());
    }

    classify_http_failure(error)
}

fn classify_modify_http_failure(error: &anyhow::Error) -> CommandFailure {
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<BybitModifyOrderError>().is_some())
    {
        return CommandFailure::ambiguous(error.to_string());
    }

    classify_http_failure(error)
}

fn http_order_error_is_ambiguous(code: i32) -> bool {
    is_bybit_ambiguous_order_error_code(i64::from(code))
}

fn classify_http_failure(error: &anyhow::Error) -> CommandFailure {
    let reason = error.to_string();

    for cause in error.chain() {
        let Some(http_error) = cause.downcast_ref::<BybitHttpError>() else {
            continue;
        };

        return match http_error {
            BybitHttpError::BybitError { error_code, .. }
                if http_order_error_is_ambiguous(*error_code) =>
            {
                CommandFailure::Ambiguous(reason)
            }
            BybitHttpError::BybitError { .. } => CommandFailure::VenueRejected(reason),
            BybitHttpError::MissingCredentials
            | BybitHttpError::ValidationError(_)
            | BybitHttpError::BuildError(_) => CommandFailure::NotSent(reason),
            BybitHttpError::JsonError(_)
            | BybitHttpError::Canceled(_)
            | BybitHttpError::NetworkError(_)
            | BybitHttpError::UnexpectedStatus { .. } => CommandFailure::Ambiguous(reason),
        };
    }

    CommandFailure::NotSent(reason)
}

async fn collect_order_status_reports(
    http_client: &BybitHttpClient,
    account_id: AccountId,
    product_types: Vec<BybitProductType>,
    command: &GenerateOrderStatusReports,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<ReportRead<OrderStatusReport>> {
    // Venue history windows filter creation time; reconciliation filters update time
    let history_window = (nanos_to_millis(command.start), nanos_to_millis(command.end));
    let mut read = ReportRead::new();

    for product_type in product_types {
        let mut fetched = http_client
            .request_order_status_reports_scoped(
                account_id,
                product_type,
                command.instrument_id,
                command.open_only,
                None,
                None,
                None,
                load_ids,
                history_window,
            )
            .await?;
        read.historical_incomplete |= fetched.historical_incomplete;
        read.reports.append(&mut fetched.reports);
    }

    Ok(read)
}

async fn collect_fill_reports(
    http_client: &BybitHttpClient,
    account_id: AccountId,
    product_types: Vec<BybitProductType>,
    command: &GenerateFillReports,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<ReportRead<FillReport>> {
    let mut read = ReportRead::new();

    for product_type in product_types {
        let mut fetched = http_client
            .request_fill_reports_scoped(
                account_id,
                product_type,
                command.instrument_id,
                nanos_to_millis(command.start),
                nanos_to_millis(command.end),
                None,
                load_ids,
            )
            .await?;
        read.historical_incomplete |= fetched.historical_incomplete;
        read.reports.append(&mut fetched.reports);
    }

    if let Some(venue_order_id) = command.venue_order_id {
        read.reports
            .retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
    }

    Ok(read)
}

fn cache_reconciliation_order_identity(
    account_id: AccountId,
    core: Option<&ExecutionClientCore>,
    state: &WsDispatchState,
    report: &OrderStatusReport,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    emitter: &ExecutionEventEmitter,
) {
    let Some(client_order_id) = report.client_order_id else {
        if state.fast_execution
            && has_pending_fast_execution(report.venue_order_id, state)
            && let Some(instrument) = instruments.get(&report.instrument_id.symbol.inner())
        {
            drain_pending_fast_executions(
                report.venue_order_id,
                instrument,
                account_id,
                report.ts_init,
                state,
                emitter,
            );
        }

        return;
    };

    if !state.fast_execution {
        if report.order_status.is_closed() {
            cleanup_terminal_when_ready(client_order_id, report.filled_qty, state);
        } else if let Some(identity) = cached_order_identity(core, client_order_id, report) {
            state.order_identities.insert(client_order_id, identity);
        }

        return;
    }

    reconcile_fast_order_report(
        account_id,
        client_order_id,
        core,
        report,
        instruments,
        state,
        emitter,
    );
}

fn reconcile_fast_order_report(
    account_id: AccountId,
    client_order_id: ClientOrderId,
    core: Option<&ExecutionClientCore>,
    report: &OrderStatusReport,
    instruments: &AHashMap<Ustr, InstrumentAny>,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
) {
    let closed = report.order_status.is_closed();
    let has_pending = has_pending_fast_execution(report.venue_order_id, state);
    if !state.order_identities.contains_key(&client_order_id)
        && (!closed || has_pending)
        && let Some(identity) = cached_order_identity(core, client_order_id, report)
    {
        state.order_identities.insert(client_order_id, identity);
    }

    let tracked = state.order_identities.contains_key(&client_order_id);
    if tracked {
        record_venue_order_id(client_order_id, report.venue_order_id, state);
    }

    if let Some(instrument) = instruments.get(&report.instrument_id.symbol.inner()) {
        drain_pending_fast_executions(
            report.venue_order_id,
            instrument,
            account_id,
            report.ts_init,
            state,
            emitter,
        );
    }

    if closed && cached_order_has_terminal_quantity(core, client_order_id, report.filled_qty) {
        cleanup_terminal(client_order_id, state);
        return;
    }

    if tracked && closed {
        cleanup_terminal_when_ready(client_order_id, report.filled_qty, state);
    }
}

fn cached_order_identity(
    core: Option<&ExecutionClientCore>,
    client_order_id: ClientOrderId,
    report: &OrderStatusReport,
) -> Option<OrderIdentity> {
    core.and_then(|core| {
        core.cache()
            .order(&client_order_id)
            .map(|order| OrderIdentity {
                instrument_id: report.instrument_id,
                strategy_id: order.strategy_id(),
                order_side: order.order_side(),
                order_type: order.order_type(),
                venue_position_id: report.venue_position_id,
            })
    })
}

fn cached_order_has_terminal_quantity(
    core: Option<&ExecutionClientCore>,
    client_order_id: ClientOrderId,
    terminal_qty: Quantity,
) -> bool {
    let Some(core) = core else {
        return false;
    };

    core.cache()
        .order(&client_order_id)
        .is_some_and(|order| order.is_closed() && order.filled_qty() >= terminal_qty)
}

#[cfg(test)]
#[path = "../tests/integration/exec_client.rs"]
mod exec_client_tests;

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::atomic::{AtomicBool, Ordering},
        time::Duration,
    };

    use axum::{
        Json, Router,
        extract::Query,
        routing::{get, post},
    };
    use nautilus_common::{
        cache::Cache,
        clients::ExecutionClient,
        messages::{
            ExecutionEvent,
            execution::{
                BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder,
                ExecutionReport, ModifyOrder, SubmitOrder, SubmitOrderList,
            },
        },
        testing::wait_until_async,
    };
    use nautilus_core::{Params, UUID4};
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        enums::{AccountType, OrderSide, OrderStatus, TimeInForce},
        events::OrderEventAny,
        identifiers::{
            ClientOrderId, OrderListId, PositionId, StrategyId, TradeId, TraderId, VenueOrderId,
        },
        instruments::stubs::crypto_perpetual_ethusdt,
        orders::{OrderList, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{
            consts::{BYBIT_CLIENT_ID, BYBIT_VENUE},
            enums::{BybitMarketUnit, BybitTimeInForce},
            parse::parse_linear_instrument,
        },
        http::models::BybitInstrumentLinearResponse,
    };

    fn test_execution_client() -> (BybitExecutionClient, Rc<RefCell<Cache>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            *BYBIT_CLIENT_ID,
            *BYBIT_VENUE,
            OmsType::Netting,
            AccountId::from("BYBIT-001"),
            AccountType::Margin,
            None,
            cache.clone(),
        );
        let config = BybitExecutionClientConfig {
            api_key: Some("test_key".into()),
            api_secret: Some("test_secret".into()),
            ..Default::default()
        };

        (BybitExecutionClient::new(core, config).unwrap(), cache)
    }

    async fn wait_for_spawned_tasks(client: &BybitExecutionClient) {
        for _ in 0..20 {
            if client.pending_tasks.all_finished() {
                return;
            }

            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        panic!("timed out waiting for spawned Bybit execution tasks");
    }

    fn assert_next_submitted(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        client_order_id: ClientOrderId,
    ) {
        let event = rx.try_recv().expect("expected OrderSubmitted event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(ref submitted)) if submitted.client_order_id == client_order_id),
            "expected OrderSubmitted for {client_order_id}, was {event:?}",
        );
    }

    fn assert_next_rejected(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        client_order_id: ClientOrderId,
    ) {
        let event = rx.try_recv().expect("expected OrderRejected event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(ref rejected))
                if rejected.client_order_id == client_order_id),
            "expected OrderRejected for {client_order_id}, was {event:?}",
        );
    }

    fn assert_next_cancel_denied(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        client_order_id: ClientOrderId,
    ) {
        let event = rx.try_recv().expect("expected OrderCancelDenied event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::CancelDenied(ref rejected))
                if rejected.client_order_id == client_order_id),
            "expected OrderCancelDenied for {client_order_id}, was {event:?}",
        );
    }

    fn assert_next_modify_denied(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        client_order_id: ClientOrderId,
    ) {
        let event = rx.try_recv().expect("expected OrderModifyDenied event");
        assert!(
            matches!(event, ExecutionEvent::Order(OrderEventAny::ModifyDenied(ref rejected))
                if rejected.client_order_id == client_order_id),
            "expected OrderModifyDenied for {client_order_id}, was {event:?}",
        );
    }

    fn cancel_command(client_order_id: ClientOrderId) -> CancelOrder {
        CancelOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
            client_order_id,
            Some(VenueOrderId::from("venue-cancel-1")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )
    }

    fn modify_command(client_order_id: ClientOrderId, params: Option<Params>) -> ModifyOrder {
        ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
            client_order_id,
            Some(VenueOrderId::from("venue-modify-1")),
            Some(Quantity::from("1")),
            Some(Price::from("10001.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            params,
            None,
        )
    }

    #[rstest]
    fn test_cancel_http_failure_classification_matches_policy() {
        let venue_reject = anyhow::Error::from(BybitHttpError::BybitError {
            error_code: 110001,
            message: "Order does not exist".to_string(),
        });
        assert_eq!(
            classify_cancel_http_failure(&venue_reject),
            CommandFailure::VenueRejected(venue_reject.to_string()),
        );

        let rate_limit = anyhow::Error::from(BybitHttpError::BybitError {
            error_code: 10006,
            message: "Too many visits".to_string(),
        });
        assert_eq!(
            classify_cancel_http_failure(&rate_limit),
            CommandFailure::Ambiguous(rate_limit.to_string()),
        );
        assert!(submit_rejection_reason(&rate_limit).is_none());

        let post_lookup = anyhow::Error::from(BybitCancelOrderError::PostCancelLookup {
            source: anyhow::anyhow!("history lookup failed"),
        });
        assert_eq!(
            classify_cancel_http_failure(&post_lookup),
            CommandFailure::Ambiguous(post_lookup.to_string()),
        );

        let transport = anyhow::Error::from(BybitHttpError::NetworkError(
            "connection closed".to_string(),
        ));
        assert_eq!(
            classify_cancel_http_failure(&transport),
            CommandFailure::Ambiguous(transport.to_string()),
        );
    }

    #[rstest]
    #[case::window_start(4, 0)]
    #[case::window_end(5, 30)]
    fn test_repay_blackout_rejection_publishes_failed_result_inside_window(
        #[case] minute: u64,
        #[case] second: u64,
    ) {
        let now_ns = UnixNanos::from((100 * 3600 + minute * 60 + second) * 1_000_000_000);

        let result = BybitExecutionClient::repay_blackout_rejection("ETH", Some("0.500"), now_ns)
            .expect("expected a rejection inside the blackout window");

        assert_eq!(result.coin, "ETH");
        assert_eq!(result.amount.as_deref(), Some("0.500"));
        assert!(!result.success);
        assert_eq!(result.result_status, "FAIL");
        assert!(
            result
                .message
                .starts_with("Bybit repay blackout window (mm:04:00-mm:05:30 UTC); retry after ")
        );
        assert!(result.message.contains(":05:31"), "was {}", result.message);
        assert_eq!(result.ts_event, now_ns);
        assert_eq!(result.ts_init, now_ns);
    }

    #[rstest]
    #[case::before_window(3, 59)]
    #[case::after_window(5, 31)]
    #[case::mid_hour(30, 0)]
    fn test_repay_blackout_rejection_allows_repayment_outside_window(
        #[case] minute: u64,
        #[case] second: u64,
    ) {
        let now_ns = UnixNanos::from((100 * 3600 + minute * 60 + second) * 1_000_000_000);

        let result = BybitExecutionClient::repay_blackout_rejection("ETH", None, now_ns);

        assert!(result.is_none());
    }

    #[rstest]
    fn test_modify_http_failure_classification_matches_policy() {
        let venue_reject = anyhow::Error::from(BybitHttpError::BybitError {
            error_code: 110003,
            message: "Order price exceeds allowable range".to_string(),
        });
        assert_eq!(
            classify_modify_http_failure(&venue_reject),
            CommandFailure::VenueRejected(venue_reject.to_string()),
        );

        let server_error = anyhow::Error::from(BybitHttpError::BybitError {
            error_code: 10016,
            message: "Server error".to_string(),
        });
        assert_eq!(
            classify_modify_http_failure(&server_error),
            CommandFailure::Ambiguous(server_error.to_string()),
        );

        let post_lookup = anyhow::Error::from(BybitModifyOrderError::PostModifyLookup {
            source: anyhow::anyhow!("realtime lookup failed"),
        });
        assert_eq!(
            classify_modify_http_failure(&post_lookup),
            CommandFailure::Ambiguous(post_lookup.to_string()),
        );

        let status = anyhow::Error::from(BybitHttpError::UnexpectedStatus {
            status: 503,
            body: "service unavailable".to_string(),
        });
        assert_eq!(
            classify_modify_http_failure(&status),
            CommandFailure::Ambiguous(status.to_string()),
        );
    }

    #[rstest]
    #[case::visits(10006)]
    #[case::frequency_protection(10429)]
    fn test_rate_limit_http_order_failures_stay_ambiguous(#[case] code: i32) {
        let error = anyhow::Error::from(BybitHttpError::BybitError {
            error_code: code,
            message: "Too many visits".to_string(),
        });

        let expected = CommandFailure::Ambiguous(error.to_string());

        assert_eq!(classify_cancel_http_failure(&error), expected);
        assert_eq!(classify_modify_http_failure(&error), expected);
        assert!(submit_rejection_reason(&error).is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn test_ws_cancel_queue_failure_emits_cancel_denied() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.ws_trade.close().await.unwrap();
        let client_order_id = ClientOrderId::from("O-CANCEL-WS-FAIL");

        client
            .cancel_order(cancel_command(client_order_id))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        assert_next_cancel_denied(&mut rx, client_order_id);
    }

    #[rstest]
    #[tokio::test]
    async fn test_ws_modify_queue_failure_emits_modify_denied() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.ws_trade.close().await.unwrap();
        let client_order_id = ClientOrderId::from("O-MODIFY-WS-FAIL");

        client
            .modify_order(modify_command(client_order_id, None))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        assert_next_modify_denied(&mut rx, client_order_id);
    }

    #[rstest]
    #[tokio::test]
    async fn test_http_cancel_local_validation_failure_emits_cancel_denied() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.config.environment = BybitEnvironment::Demo;

        client
            .cancel_order(cancel_command(ClientOrderId::from(
                "O-CANCEL-LOCAL-VALIDATION",
            )))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        assert_next_cancel_denied(&mut rx, ClientOrderId::from("O-CANCEL-LOCAL-VALIDATION"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_modify_local_validation_failure_emits_modify_denied() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let params =
            Params::new([("order_iv".to_string(), serde_json::json!({ "bad": true }))].into());

        client
            .modify_order(modify_command(
                ClientOrderId::from("O-MODIFY-LOCAL-VALIDATION"),
                Some(params),
            ))
            .unwrap();

        assert_next_modify_denied(&mut rx, ClientOrderId::from("O-MODIFY-LOCAL-VALIDATION"));
    }

    #[rstest]
    #[case(OrderType::Limit, false)]
    #[case(OrderType::Limit, true)]
    #[case(OrderType::Market, false)]
    #[case(OrderType::StopLimit, false)]
    #[case(OrderType::StopMarket, false)]
    #[case(OrderType::LimitIfTouched, false)]
    #[case(OrderType::MarketIfTouched, false)]
    #[tokio::test]
    async fn test_submit_order_unsupported_time_in_force_denied(
        #[case] order_type: OrderType,
        #[case] post_only: bool,
        #[values(
            BybitEnvironment::Demo,
            BybitEnvironment::Mainnet,
            BybitEnvironment::Testnet
        )]
        environment: BybitEnvironment,
        #[values(TimeInForce::Day, TimeInForce::AtTheOpen, TimeInForce::AtTheClose)]
        time_in_force: TimeInForce,
    ) {
        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.config.environment = environment;
        let client_order_id = ClientOrderId::from("O-TIF-DENIED");
        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let order = OrderTestBuilder::new(order_type)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.02"))
            .price(Price::from("10000.00"))
            .trigger_price(Price::from("10000.00"))
            .time_in_force(time_in_force)
            .post_only(post_only)
            .build();

        let command = SubmitOrder::new(
            order.trader_id(),
            Some(*BYBIT_CLIENT_ID),
            order.strategy_id(),
            instrument_id,
            client_order_id,
            order.init_event().clone(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        let ts_before = client.clock.get_time_ns();

        client.submit_order(command).unwrap();

        let ts_after = client.clock.get_time_ns();

        let ExecutionEvent::Order(OrderEventAny::Denied(event)) = rx.try_recv().unwrap() else {
            panic!("Expected OrderDenied before submission");
        };

        assert_eq!(event.trader_id, TraderId::from("TESTER-001"));
        assert_eq!(event.strategy_id, StrategyId::from("S-001"));
        assert_eq!(event.instrument_id, instrument_id);
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(
            event.reason.as_str(),
            format!("UNSUPPORTED_TIME_IN_FORCE: {time_in_force}")
        );
        assert!(event.ts_event >= ts_before && event.ts_event <= ts_after);
        assert_eq!(event.ts_init, event.ts_event);
        assert_eq!(event.causation_id, None);
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
        assert!(client.pending_tasks.is_empty());
        assert!(client.dispatch_state.order_identities.is_empty());
        assert!(client.dispatch_state.order_snapshots.is_empty());
        assert!(client.dispatch_state.pending_requests.is_empty());
    }

    #[rstest]
    #[case(OrderType::Limit, false)]
    #[case(OrderType::Limit, true)]
    #[case(OrderType::Market, false)]
    #[tokio::test]
    async fn test_submit_order_list_unsupported_time_in_force_denied(
        #[case] order_type: OrderType,
        #[case] post_only: bool,
        #[values(
            BybitEnvironment::Demo,
            BybitEnvironment::Mainnet,
            BybitEnvironment::Testnet
        )]
        environment: BybitEnvironment,
        #[values(TimeInForce::Day, TimeInForce::AtTheOpen, TimeInForce::AtTheClose)]
        time_in_force: TimeInForce,
    ) {
        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.config.environment = environment;
        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let client_order_ids = [
            ClientOrderId::from("O-TIF-VALID"),
            ClientOrderId::from("O-TIF-DENIED"),
        ];
        let mut inits = Vec::new();

        for (cid, tif) in client_order_ids
            .into_iter()
            .zip([TimeInForce::Gtc, time_in_force])
        {
            let order = OrderTestBuilder::new(order_type)
                .instrument_id(instrument_id)
                .client_order_id(cid)
                .side(OrderSide::Sell)
                .quantity(Quantity::from("0.03"))
                .price(Price::from("12000.00"))
                .time_in_force(tif)
                .post_only(post_only)
                .build();
            inits.push(order.init_event().clone());
            cache
                .borrow_mut()
                .add_order(order, None, None, false)
                .unwrap();
        }

        let list_id = OrderListId::from("OL-TIF-DENIED");

        let order_list = OrderList::new(
            list_id,
            instrument_id,
            StrategyId::from("S-001"),
            client_order_ids.to_vec(),
            UnixNanos::default(),
        );

        let command = SubmitOrderList::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-001"),
            order_list,
            inits,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        );
        let ts_before = client.clock.get_time_ns();

        client.submit_order_list(command).unwrap();

        let ts_after = client.clock.get_time_ns();

        for (cid, reason) in client_order_ids.into_iter().zip([
            format!("ORDER_LIST_DENIED: {list_id}"),
            format!("UNSUPPORTED_TIME_IN_FORCE: {time_in_force}"),
        ]) {
            let ExecutionEvent::Order(OrderEventAny::Denied(event)) = rx.try_recv().unwrap() else {
                panic!("Expected OrderDenied before any list leg submission");
            };

            assert_eq!(event.trader_id, TraderId::from("TESTER-001"));
            assert_eq!(event.strategy_id, StrategyId::from("S-001"));
            assert_eq!(event.instrument_id, instrument_id);
            assert_eq!(event.client_order_id, cid);
            assert_eq!(event.reason.as_str(), reason);
            assert!(event.ts_event >= ts_before && event.ts_event <= ts_after);
            assert_eq!(event.ts_init, event.ts_event);
            assert_eq!(event.causation_id, None);
        }

        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
        assert!(client.pending_tasks.is_empty());
        assert!(client.dispatch_state.order_identities.is_empty());
        assert!(client.dispatch_state.order_snapshots.is_empty());
        assert!(client.dispatch_state.pending_requests.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_ws_submit_queue_failure_emits_order_rejected() {
        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let client_order_id = ClientOrderId::from("O-WS-SEND-FAIL");
        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        let order = builder
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("10000.00"))
            .build();
        let init = order.init_event().clone();
        let trader_id = order.trader_id();
        let strategy_id = order.strategy_id();

        cache
            .borrow_mut()
            .add_order(order, None, Some(*BYBIT_CLIENT_ID), false)
            .unwrap();

        let command = SubmitOrder::new(
            trader_id,
            Some(*BYBIT_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            init,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None, // correlation_id
        );

        client.submit_order(command).unwrap();

        assert_next_submitted(&mut rx, client_order_id);
        wait_for_spawned_tasks(&client).await;

        assert!(
            !client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );
        assert!(
            !client
                .dispatch_state
                .order_snapshots
                .contains_key(&client_order_id)
        );
        assert_next_rejected(&mut rx, client_order_id);
    }

    #[rstest]
    #[tokio::test]
    async fn test_ws_submit_order_list_queue_failure_emits_order_rejected() {
        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let strategy_id = StrategyId::from("S-001");
        let client_order_id_1 = ClientOrderId::from("O-WS-LIST-SEND-FAIL-1");
        let client_order_id_2 = ClientOrderId::from("O-WS-LIST-SEND-FAIL-2");

        let mut builder_1 = OrderTestBuilder::new(OrderType::Limit);
        let order_1 = builder_1
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id_1)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("10000.00"))
            .build();
        let init_1 = order_1.init_event().clone();
        let trader_id = order_1.trader_id();

        let mut builder_2 = OrderTestBuilder::new(OrderType::Limit);
        let order_2 = builder_2
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id_2)
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1"))
            .price(Price::from("10001.00"))
            .build();
        let init_2 = order_2.init_event().clone();

        cache
            .borrow_mut()
            .add_order(order_1, None, Some(*BYBIT_CLIENT_ID), false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(order_2, None, Some(*BYBIT_CLIENT_ID), false)
            .unwrap();

        let order_list = OrderList::new(
            OrderListId::from("OL-WS-SEND-FAIL"),
            instrument_id,
            strategy_id,
            vec![client_order_id_1, client_order_id_2],
            UnixNanos::default(),
        );
        let command = SubmitOrderList::new(
            trader_id,
            Some(*BYBIT_CLIENT_ID),
            strategy_id,
            order_list,
            vec![init_1, init_2],
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None, // correlation_id
        );

        client.submit_order_list(command).unwrap();

        assert_next_submitted(&mut rx, client_order_id_1);
        assert_next_submitted(&mut rx, client_order_id_2);
        wait_for_spawned_tasks(&client).await;

        for client_order_id in [client_order_id_1, client_order_id_2] {
            assert!(
                !client
                    .dispatch_state
                    .order_identities
                    .contains_key(&client_order_id)
            );
            assert!(
                !client
                    .dispatch_state
                    .order_snapshots
                    .contains_key(&client_order_id)
            );
            assert_next_rejected(&mut rx, client_order_id);
        }
    }

    fn sample_order_status_report(
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        order_status: OrderStatus,
        venue_position_id: Option<PositionId>,
    ) -> OrderStatusReport {
        let mut report = OrderStatusReport::new(
            AccountId::from("BYBIT-001"),
            instrument_id,
            Some(client_order_id),
            VenueOrderId::from("BYBIT-ORDER-001"),
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            order_status,
            Quantity::from("1"),
            Quantity::from("0"),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        report.venue_position_id = venue_position_id;
        report
    }

    #[rstest]
    fn test_cache_reconciliation_order_identity_caches_and_clears_hedge_report() {
        let (client, cache) = test_execution_client();
        let client_order_id = ClientOrderId::from("O-HEDGE-RECON");
        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let venue_position_id = PositionId::from("BTCUSDT-LINEAR.BYBIT-LONG");
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        let order = builder
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("10000.00"))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        let report = sample_order_status_report(
            client_order_id,
            instrument_id,
            OrderStatus::Accepted,
            Some(venue_position_id),
        );
        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &report,
            &client.instruments_cache,
            &client.emitter,
        );

        {
            let identity = client
                .dispatch_state
                .order_identities
                .get(&client_order_id)
                .unwrap();
            assert_eq!(identity.instrument_id, instrument_id);
            assert_eq!(identity.strategy_id, order.strategy_id());
            assert_eq!(identity.order_side, order.order_side());
            assert_eq!(identity.order_type, order.order_type());
            assert_eq!(identity.venue_position_id, Some(venue_position_id));
        }

        let terminal_report = sample_order_status_report(
            client_order_id,
            instrument_id,
            OrderStatus::Filled,
            Some(venue_position_id),
        );
        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &terminal_report,
            &client.instruments_cache,
            &client.emitter,
        );

        assert!(
            client
                .dispatch_state
                .order_identities
                .get(&client_order_id)
                .is_none()
        );
    }

    #[rstest]
    fn test_cache_reconciliation_order_identity_defers_fast_terminal_cleanup() {
        let (mut client, cache) = test_execution_client();
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let client_order_id = ClientOrderId::from("O-FAST-RECON");
        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.5"))
            .price(Price::from("10000.00"))
            .build();
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        let accepted =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Accepted, None);
        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &accepted,
            &client.instruments_cache,
            &client.emitter,
        );
        let mut terminal =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Filled, None);
        terminal.filled_qty = Quantity::from("0.5");

        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &terminal,
            &client.instruments_cache,
            &client.emitter,
        );

        assert!(
            client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );
        assert_eq!(
            *client
                .dispatch_state
                .terminal_order_quantities
                .get(&client_order_id)
                .unwrap(),
            Quantity::from("0.5")
        );
        assert_eq!(
            client
                .dispatch_state
                .order_venue_ids
                .get(&client_order_id)
                .map(|id| *id),
            Some(terminal.venue_order_id)
        );
    }

    #[rstest]
    #[case::nonterminal(true)]
    #[case::terminal_cached_order(false)]
    fn test_cache_reconciliation_recovers_fast_fill_without_recreating_state(
        #[case] recover_open: bool,
    ) {
        let (mut client, cache) = test_execution_client();
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let json = crate::common::testing::load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = parse_linear_instrument(
            &response.result.list[1],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let instrument_id = instrument.id();
        let mut instruments = AHashMap::new();
        instruments.insert(instrument_id.symbol.inner(), instrument.clone());
        client.instruments_cache = Arc::new(instruments);
        let client_order_id = ClientOrderId::from("O-FAST-DRAIN");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.5"))
            .price(Price::from("3000.00"))
            .build();
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        let accepted =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Accepted, None);
        let json = crate::common::testing::load_test_json("ws_account_execution_fast.json");
        let mut fast: crate::websocket::messages::BybitWsAccountExecutionFastMsg =
            serde_json::from_str(&json).unwrap();
        fast.data[0].category = BybitProductType::Linear;
        fast.data[0].symbol = Ustr::from("ETHUSDT");
        fast.data[0].order_id = accepted.venue_order_id.as_str().into();
        fast.data[0].order_link_id = Ustr::from("");
        fast.data[0].exec_price = "3000.00".to_string();
        fast.data[0].exec_qty = "0.5".to_string();
        dispatch_ws_message(
            &crate::websocket::messages::BybitWsMessage::AccountExecutionFast(fast),
            client.core.account_id,
            &client.instruments_cache,
            client.clock,
            &client.dispatch_state,
            &client.emitter,
        );
        assert!(rx.try_recv().is_err());
        assert!(
            !client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );
        assert!(
            client
                .instruments_cache
                .contains_key(&instrument_id.symbol.inner())
        );

        if recover_open {
            cache_reconciliation_order_identity(
                client.core.account_id,
                Some(&client.core),
                &client.dispatch_state,
                &accepted,
                &client.instruments_cache,
                &client.emitter,
            );

            assert!(matches!(
                rx.try_recv().unwrap(),
                ExecutionEvent::Order(OrderEventAny::Accepted(_))
            ));

            match rx.try_recv().unwrap() {
                ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                    assert_eq!(report.client_order_id, Some(client_order_id));
                    assert_eq!(report.last_qty, Quantity::from("0.5"));
                }
                other => panic!("Expected FillReport, found {other:?}"),
            }

            assert!(rx.try_recv().is_err());
        }

        let order = cache.borrow().order(&client_order_id).unwrap().clone();
        let accepted_event =
            TestOrderEventStubs::accepted(&order, client.core.account_id, accepted.venue_order_id);
        cache.borrow_mut().update_order(&accepted_event).unwrap();
        let order = cache.borrow().order(&client_order_id).unwrap().clone();
        let filled = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::from("rest-fill-1")),
            None,
            Some(Price::from("3000.00")),
            Some(Quantity::from("0.5")),
            None,
            None,
            None,
            Some(client.core.account_id),
        );
        cache.borrow_mut().update_order(&filled).unwrap();
        let mut terminal = accepted;
        terminal.order_status = OrderStatus::Filled;
        terminal.filled_qty = Quantity::from("0.5");

        for _ in 0..2 {
            cache_reconciliation_order_identity(
                client.core.account_id,
                Some(&client.core),
                &client.dispatch_state,
                &terminal,
                &client.instruments_cache,
                &client.emitter,
            );
        }

        if !recover_open {
            assert!(matches!(
                rx.try_recv().unwrap(),
                ExecutionEvent::Order(OrderEventAny::Accepted(_))
            ));

            match rx.try_recv().unwrap() {
                ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                    assert_eq!(report.client_order_id, Some(client_order_id));
                    assert_eq!(report.last_qty, Quantity::from("0.5"));
                }
                other => panic!("Expected FillReport, found {other:?}"),
            }
        }

        assert!(rx.try_recv().is_err());
        assert!(
            !client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );
        assert!(
            !client
                .dispatch_state
                .order_venue_ids
                .contains_key(&client_order_id)
        );
        assert!(
            !client
                .dispatch_state
                .venue_order_ids
                .contains_key(&terminal.venue_order_id)
        );
        assert!(
            !client
                .dispatch_state
                .terminal_order_quantities
                .contains_key(&client_order_id)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_teardown_flushes_pending_fast_execution() {
        let (mut client, _cache) = test_execution_client();
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let json = crate::common::testing::load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = parse_linear_instrument(
            &response.result.list[1],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let instrument_id = instrument.id();
        let mut instruments = AHashMap::new();
        instruments.insert(instrument_id.symbol.inner(), instrument);
        client.instruments_cache = Arc::new(instruments);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        let json = crate::common::testing::load_test_json("ws_account_execution_fast.json");
        let mut fast: crate::websocket::messages::BybitWsAccountExecutionFastMsg =
            serde_json::from_str(&json).unwrap();
        fast.data[0].category = BybitProductType::Linear;
        fast.data[0].symbol = Ustr::from("ETHUSDT");
        fast.data[0].order_id = Ustr::from("BYBIT-ORDER-001");
        fast.data[0].order_link_id = Ustr::from("");
        fast.data[0].exec_price = "3000.00".to_string();
        fast.data[0].exec_qty = "0.5".to_string();
        dispatch_ws_message(
            &crate::websocket::messages::BybitWsMessage::AccountExecutionFast(fast),
            client.core.account_id,
            &client.instruments_cache,
            client.clock,
            &client.dispatch_state,
            &client.emitter,
        );
        assert!(rx.try_recv().is_err());

        client.teardown_partial_connect().await.unwrap();

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.client_order_id, None);
                assert_eq!(report.last_qty, Quantity::from("0.5"));
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_refresh_execution_fee_rates_retains_previous_on_failure() {
        let fee_response: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_fee_rate.json"),
        )
        .unwrap();
        let failing = Arc::new(AtomicBool::new(false));
        let response = fee_response.clone();
        let failure_flag = Arc::clone(&failing);

        let router = Router::new().route(
            "/v5/account/fee-rate",
            get(move || {
                let response = response.clone();
                let failing = Arc::clone(&failure_flag);
                async move {
                    if failing.load(Ordering::Relaxed) {
                        Json(serde_json::json!({
                            "retCode": 10001,
                            "retMsg": "fee rate request failed",
                            "result": {},
                            "retExtInfo": {},
                            "time": 1697673900000i64
                        }))
                    } else {
                        Json(response)
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, _cache) = test_execution_client();
        client.http_client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            Some(format!("http://{addr}")),
            1,
            0,
            10,
            10,
            5_000,
            None,
        )
        .unwrap();
        let json = crate::common::testing::load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = parse_linear_instrument(
            &response.result.list[1],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let instrument_id = instrument.id();
        let mut instruments = AHashMap::new();
        instruments.insert(instrument_id.symbol.inner(), instrument);
        let instruments = Arc::new(instruments);

        BybitExecutionClient::refresh_execution_fee_rates(
            client.http_client.clone(),
            vec![BybitProductType::Linear],
            Arc::clone(&instruments),
            Arc::clone(&client.dispatch_state),
        )
        .await;
        let previous = *client.dispatch_state.fee_rates.get(&instrument_id).unwrap();
        failing.store(true, Ordering::Relaxed);
        BybitExecutionClient::refresh_execution_fee_rates(
            client.http_client.clone(),
            vec![BybitProductType::Linear],
            instruments,
            Arc::clone(&client.dispatch_state),
        )
        .await;

        assert_eq!(
            *client.dispatch_state.fee_rates.get(&instrument_id).unwrap(),
            previous
        );
        server.abort();
    }

    #[rstest]
    fn test_rest_recovery_cleans_after_core_applies_terminal_report() {
        let (mut client, cache) = test_execution_client();
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let json = crate::common::testing::load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = parse_linear_instrument(
            &response.result.list[1],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let instrument_id = instrument.id();
        let mut instruments = AHashMap::new();
        instruments.insert(instrument_id.symbol.inner(), instrument.clone());
        client.instruments_cache = Arc::new(instruments);
        let client_order_id = ClientOrderId::from("O-REST-ONLY");
        let venue_order_id = VenueOrderId::from("BYBIT-ORDER-001");
        let account_id = client.core.account_id;
        client.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("S-001"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                venue_position_id: None,
            },
        );

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.5"))
            .price(Price::from("3000.00"))
            .build();
        order
            .apply(TestOrderEventStubs::accepted(
                &order,
                account_id,
                venue_order_id,
            ))
            .unwrap();
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        let mut terminal =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Filled, None);
        terminal.filled_qty = Quantity::from("0.5");

        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &terminal,
            &client.instruments_cache,
            &client.emitter,
        );

        assert!(
            client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );

        let order = cache.borrow().order(&client_order_id).unwrap().clone();
        let filled = TestOrderEventStubs::filled(
            &order,
            &instrument,
            Some(TradeId::from("rest-fill-1")),
            None,
            Some(Price::from("3000.00")),
            Some(Quantity::from("0.5")),
            None,
            None,
            None,
            Some(account_id),
        );
        cache.borrow_mut().update_order(&filled).unwrap();
        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &terminal,
            &client.instruments_cache,
            &client.emitter,
        );

        assert!(client.dispatch_state.order_identities.is_empty());
        assert!(client.dispatch_state.order_venue_ids.is_empty());
        assert!(client.dispatch_state.venue_order_ids.is_empty());
        assert!(client.dispatch_state.terminal_order_quantities.is_empty());
    }

    #[rstest]
    fn test_cache_reconciliation_external_order_drains_pending_fill() {
        let (mut client, _cache) = test_execution_client();
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let json = crate::common::testing::load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = parse_linear_instrument(
            &response.result.list[1],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let instrument_id = instrument.id();
        let mut instruments = AHashMap::new();
        instruments.insert(instrument_id.symbol.inner(), instrument);
        client.instruments_cache = Arc::new(instruments);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        let mut report = sample_order_status_report(
            ClientOrderId::from("unused"),
            instrument_id,
            OrderStatus::Accepted,
            None,
        );
        report.client_order_id = None;
        let json = crate::common::testing::load_test_json("ws_account_execution_fast.json");
        let mut fast: crate::websocket::messages::BybitWsAccountExecutionFastMsg =
            serde_json::from_str(&json).unwrap();
        fast.data[0].category = BybitProductType::Linear;
        fast.data[0].symbol = Ustr::from("ETHUSDT");
        fast.data[0].order_id = report.venue_order_id.as_str().into();
        fast.data[0].order_link_id = Ustr::from("");
        fast.data[0].exec_price = "3000.00".to_string();
        fast.data[0].exec_qty = "0.5".to_string();
        dispatch_ws_message(
            &crate::websocket::messages::BybitWsMessage::AccountExecutionFast(fast),
            client.core.account_id,
            &client.instruments_cache,
            client.clock,
            &client.dispatch_state,
            &client.emitter,
        );
        assert!(rx.try_recv().is_err());

        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &report,
            &client.instruments_cache,
            &client.emitter,
        );

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
                assert_eq!(fill.client_order_id, None);
                assert_eq!(fill.last_qty, Quantity::from("0.5"));
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }

        assert!(rx.try_recv().is_err());
        assert!(client.dispatch_state.order_identities.is_empty());
        assert!(client.dispatch_state.order_venue_ids.is_empty());
        assert!(client.dispatch_state.venue_order_ids.is_empty());
    }

    #[rstest]
    fn test_cache_reconciliation_order_identity_keeps_one_way_local_report() {
        let (client, cache) = test_execution_client();
        let client_order_id = ClientOrderId::from("O-ONEWAY-RECON");
        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        let order = builder
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("10000.00"))
            .build();
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        let report =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Accepted, None);
        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &report,
            &client.instruments_cache,
            &client.emitter,
        );

        let identity = client
            .dispatch_state
            .order_identities
            .get(&client_order_id)
            .unwrap();
        assert_eq!(identity.instrument_id, instrument_id);
        assert_eq!(identity.venue_position_id, None);
    }

    #[rstest]
    #[case::single_discard(true, false)]
    #[case::bulk_discard(false, false)]
    #[case::single_finish(true, true)]
    #[case::bulk_finish(false, true)]
    #[tokio::test]
    async fn test_closed_report_collection_preserves_pending_command_identity(
        #[case] single: bool,
        #[case] accepted: bool,
        #[values(PendingOperation::Cancel, PendingOperation::Amend)] operation: PendingOperation,
    ) {
        let client_order_id = ClientOrderId::from("client-open-1");
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let report =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Accepted, None);
        let json = crate::common::testing::load_test_json("http_get_orders_realtime.json");
        let mut orders: serde_json::Value = serde_json::from_str(&json).unwrap();
        orders["result"]["list"].as_array_mut().unwrap().truncate(1);
        orders["result"]["list"][0]["orderStatus"] = serde_json::json!("Cancelled");
        orders["result"]["list"][0]["orderId"] = serde_json::json!(report.venue_order_id.as_str());
        let (mut client, cache, server) = order_report_client(orders).await;
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.010"))
            .price(Price::from("3930.41"))
            .build();
        let event =
            TestOrderEventStubs::accepted(&order, client.core.account_id, report.venue_order_id);
        order.apply(event).unwrap();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();
        cache_reconciliation_order_identity(
            client.core.account_id,
            Some(&client.core),
            &client.dispatch_state,
            &report,
            &client.instruments_cache,
            &client.emitter,
        );
        client.dispatch_state.pending_requests.insert(
            "late-command".to_string(),
            (
                vec![client_order_id],
                vec![Some(report.venue_order_id)],
                operation,
                Vec::new(),
            ),
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let task = if single {
            let command = GenerateOrderStatusReport::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(instrument_id),
                Some(client_order_id),
                Some(report.venue_order_id),
                None,
                None,
            );
            let task = client.generate_order_status_report_task(&command).unwrap();
            ExecutionReportTask {
                collection: task.collection,
                finish: Box::new(move || Ok((task.finish)()?.into_iter().collect::<Vec<_>>())),
            }
        } else {
            let command = GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                Some(instrument_id),
                None,
                None,
                None,
                None,
            );
            client.generate_order_status_reports_task(&command).unwrap()
        };

        tokio::spawn(task.collection).await.unwrap();
        assert!(
            client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );

        if accepted {
            let reports = (task.finish)().unwrap();
            assert_eq!(reports.len(), 1);
            assert_eq!(reports[0].order_status, OrderStatus::Canceled);
        } else {
            drop(task.finish);
        }

        let json = crate::common::testing::load_test_json("ws_order_response.json");
        let mut response: serde_json::Value = serde_json::from_str(&json).unwrap();
        response["op"] = serde_json::json!(match operation {
            PendingOperation::Cancel => "order.cancel",
            PendingOperation::Amend => "order.amend",
            PendingOperation::Place => unreachable!(),
        });
        response["retCode"] = serde_json::json!(110001);
        response["retMsg"] = serde_json::json!("Order does not exist.");
        response["reqId"] = serde_json::json!("late-command");
        response["data"]["orderLinkId"] = serde_json::json!(client_order_id.as_str());
        response["data"]["orderId"] = serde_json::json!(report.venue_order_id.as_str());
        let response = serde_json::from_value(response).unwrap();
        let message = crate::websocket::messages::BybitWsMessage::OrderResponse(response);
        dispatch_ws_message(
            &message,
            client.core.account_id,
            &client.instruments_cache,
            client.clock,
            &client.dispatch_state,
            &client.emitter,
        );
        assert_eq!(
            client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id),
            !accepted
        );
        assert!(client.dispatch_state.pending_requests.is_empty());

        if accepted {
            assert!(rx.try_recv().is_err());
        } else {
            let ExecutionEvent::Order(event) = rx.try_recv().unwrap() else {
                panic!("expected delayed command rejection");
            };

            let (reason, reconciliation) = match (operation, &event) {
                (PendingOperation::Cancel, OrderEventAny::CancelRejected(event)) => {
                    (event.reason, event.reconciliation)
                }
                (PendingOperation::Amend, OrderEventAny::ModifyRejected(event)) => {
                    (event.reason, event.reconciliation)
                }
                _ => panic!("unexpected rejection event: {event:?}"),
            };

            assert_eq!(event.trader_id(), client.core.trader_id);
            assert_eq!(event.account_id(), Some(client.core.account_id));
            assert_eq!(event.client_order_id(), client_order_id);
            assert_eq!(event.venue_order_id(), Some(report.venue_order_id));
            assert_eq!(event.instrument_id(), instrument_id);
            assert_eq!(event.strategy_id(), order.strategy_id());
            assert_eq!(reason.as_str(), "Order does not exist.");
            assert!(!reconciliation);
            assert!(rx.try_recv().is_err());
        }

        server.abort();
    }

    #[rstest]
    #[case::single(true)]
    #[case::bulk(false)]
    #[tokio::test]
    async fn test_open_report_finalization_uses_current_core(#[case] single: bool) {
        let orders = serde_json::from_str(&crate::common::testing::load_test_json(
            "http_get_orders_realtime.json",
        ))
        .unwrap();
        let (client, cache, server) = order_report_client(orders).await;
        let client_order_id = ClientOrderId::from("client-open-1");
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");

        let task = if single {
            let command = GenerateOrderStatusReport::new(
                UUID4::new(),
                UnixNanos::default(),
                Some(instrument_id),
                Some(client_order_id),
                Some(VenueOrderId::from("open-order-1")),
                None,
                None,
            );
            let task = client.generate_order_status_report_task(&command).unwrap();
            ExecutionReportTask {
                collection: task.collection,
                finish: Box::new(move || Ok((task.finish)()?.into_iter().collect::<Vec<_>>())),
            }
        } else {
            let command = GenerateOrderStatusReports::new(
                UUID4::new(),
                UnixNanos::default(),
                false,
                Some(instrument_id),
                None,
                None,
                None,
                None,
            );
            client.generate_order_status_reports_task(&command).unwrap()
        };

        tokio::spawn(task.collection).await.unwrap();
        assert!(client.dispatch_state.order_identities.is_empty());
        let strategy_id = StrategyId::from("LATE_OWNER-007");
        let order = OrderTestBuilder::new(OrderType::Market)
            .strategy_id(strategy_id)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.027"))
            .build();
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        let reports = (task.finish)().unwrap();

        assert_eq!(reports[0].client_order_id, Some(client_order_id));
        let identity = client
            .dispatch_state
            .order_identities
            .get(&client_order_id)
            .unwrap();
        assert_eq!(
            (
                identity.instrument_id,
                identity.strategy_id,
                identity.order_side,
                identity.order_type,
                identity.venue_position_id
            ),
            (
                instrument_id,
                strategy_id,
                OrderSide::Sell,
                OrderType::Market,
                None
            ),
        );
        server.abort();
    }

    #[rstest]
    #[case::single(true)]
    #[case::bulk(false)]
    #[tokio::test]
    async fn test_order_report_collection_error_preserves_identity(#[case] single: bool) {
        let response = serde_json::json!({
            "retCode": 10001, "retMsg": "report collection failure", "result": {"list": []},
            "retExtInfo": {}, "time": 1761355905610u64,
        });
        let (client, _, server) = order_report_client(response).await;
        let client_order_id = ClientOrderId::from("client-open-1");
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let strategy_id = StrategyId::from("ERROR_OWNER-009");
        client.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side: OrderSide::Sell,
                order_type: OrderType::Market,
                venue_position_id: Some(PositionId::from("ERROR-POSITION")),
            },
        );

        let single_command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(instrument_id),
            Some(client_order_id),
            Some(VenueOrderId::from("open-order-1")),
            None,
            None,
        );

        let bulk_command = GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            Some(instrument_id),
            None,
            None,
            None,
            None,
        );

        for fused in [false, true] {
            let error = if fused {
                if single {
                    client
                        .generate_order_status_report(&single_command)
                        .await
                        .unwrap_err()
                } else {
                    client
                        .generate_order_status_reports(&bulk_command)
                        .await
                        .unwrap_err()
                }
            } else if single {
                let task = client
                    .generate_order_status_report_task(&single_command)
                    .unwrap();

                tokio::spawn(task.collection).await.unwrap();
                (task.finish)().unwrap_err()
            } else {
                let task = client
                    .generate_order_status_reports_task(&bulk_command)
                    .unwrap();

                tokio::spawn(task.collection).await.unwrap();
                (task.finish)().unwrap_err()
            };

            let error = error.downcast_ref::<BybitHttpError>().unwrap();
            assert!(
                matches!(error, BybitHttpError::BybitError { error_code: 10001, message } if message == "report collection failure")
            );
            let identity = client
                .dispatch_state
                .order_identities
                .get(&client_order_id)
                .unwrap();
            assert_eq!(
                (
                    identity.instrument_id,
                    identity.strategy_id,
                    identity.order_side,
                    identity.order_type,
                    identity.venue_position_id
                ),
                (
                    instrument_id,
                    strategy_id,
                    OrderSide::Sell,
                    OrderType::Market,
                    Some(PositionId::from("ERROR-POSITION"))
                ),
            );
        }

        server.abort();
    }

    #[rstest]
    #[case::both_match(Some("ETHUSDT-LINEAR.BYBIT"), Some("client-open-1"), Some("open-order-1"), Some(("client-open-1", "open-order-1")))]
    #[case::crossed_identifiers(
        Some("ETHUSDT-LINEAR.BYBIT"),
        Some("client-open-1"),
        Some("open-order-2"),
        None
    )]
    #[case::client_only(Some("ETHUSDT-LINEAR.BYBIT"), Some("client-open-2"), None, Some(("client-open-2", "open-order-2")))]
    #[case::venue_only(Some("ETHUSDT-LINEAR.BYBIT"), None, Some("open-order-2"), Some(("client-open-2", "open-order-2")))]
    #[tokio::test]
    async fn test_single_report_task_preserves_lookup_filters(
        #[case] instrument: Option<&str>,
        #[case] client_order_id: Option<&str>,
        #[case] venue_order_id: Option<&str>,
        #[case] expected: Option<(&str, &str)>,
    ) {
        let orders = serde_json::from_str(&crate::common::testing::load_test_json(
            "http_get_orders_realtime.json",
        ))
        .unwrap();
        let (client, _, server) = order_report_client(orders).await;

        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            instrument.map(InstrumentId::from),
            client_order_id.map(ClientOrderId::from),
            venue_order_id.map(VenueOrderId::from),
            None,
            None,
        );
        let task = client.generate_order_status_report_task(&command).unwrap();
        tokio::spawn(task.collection).await.unwrap();

        let report = (task.finish)().unwrap();

        assert_eq!(
            report.map(|report| (report.client_order_id, report.venue_order_id)),
            expected.map(|(client, venue)| (
                Some(ClientOrderId::from(client)),
                VenueOrderId::from(venue)
            )),
        );
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_single_report_missing_instrument_returns_error() {
        let orders = serde_json::from_str(&crate::common::testing::load_test_json(
            "http_get_orders_realtime.json",
        ))
        .unwrap();
        let (client, _, server) = order_report_client(orders).await;

        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            None,
            Some(ClientOrderId::from("client-open-1")),
            Some(VenueOrderId::from("open-order-1")),
            None,
            None,
        );

        for fused in [false, true] {
            let error = if fused {
                client
                    .generate_order_status_report(&command)
                    .await
                    .unwrap_err()
            } else {
                let task = client.generate_order_status_report_task(&command).unwrap();
                tokio::spawn(task.collection).await.unwrap();
                (task.finish)().unwrap_err()
            };

            assert_eq!(
                error.to_string(),
                format!("generate_order_status_report requires instrument_id: {command}"),
            );
        }

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_single_report_parse_failure_returns_error() {
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["list"].as_array_mut().unwrap().truncate(1);
        orders["result"]["list"][0]["qty"] = serde_json::json!("not-a-number");
        let (client, _, server) = order_report_client(orders).await;

        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETHUSDT-LINEAR.BYBIT")),
            Some(ClientOrderId::from("client-open-1")),
            Some(VenueOrderId::from("open-order-1")),
            None,
            None,
        );

        for fused in [false, true] {
            let error = if fused {
                client
                    .generate_order_status_report(&command)
                    .await
                    .unwrap_err()
            } else {
                let task = client.generate_order_status_report_task(&command).unwrap();
                tokio::spawn(task.collection).await.unwrap();
                (task.finish)().unwrap_err()
            };

            assert!(
                error
                    .to_string()
                    .contains("Failed to parse order.qty='not-a-number' as Decimal"),
                "error was: {error}",
            );
        }

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_single_report_nonmatching_parse_failure_returns_error() {
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["list"][1]["qty"] = serde_json::json!("not-a-number");
        let (client, _, server) = order_report_client(orders).await;

        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETHUSDT-LINEAR.BYBIT")),
            Some(ClientOrderId::from("client-open-1")),
            Some(VenueOrderId::from("open-order-1")),
            None,
            None,
        );

        for fused in [false, true] {
            let error = if fused {
                client
                    .generate_order_status_report(&command)
                    .await
                    .unwrap_err()
            } else {
                let task = client.generate_order_status_report_task(&command).unwrap();
                tokio::spawn(task.collection).await.unwrap();
                (task.finish)().unwrap_err()
            };

            assert!(
                error
                    .to_string()
                    .contains("Failed to parse order.qty='not-a-number' as Decimal"),
                "error was: {error}",
            );
        }

        server.abort();
    }

    #[rstest]
    #[case::unrestricted_miss("UNLOADEDUSDT", None, true)]
    #[case::explicit_miss("UNLOADEDUSDT", Some("UNLOADEDUSDT-LINEAR.BYBIT"), true)]
    #[case::excluded_miss("UNLOADEDUSDT", Some("ETHUSDT-LINEAR.BYBIT"), false)]
    #[case::excluded_cached("ETHUSDT", Some("BTCUSDT-LINEAR.BYBIT"), false)]
    #[case::unknown_identity("", Some("ETHUSDT-LINEAR.BYBIT"), true)]
    #[tokio::test]
    async fn test_bulk_order_reconciliation_scope(
        #[case] symbol: &str,
        #[case] scope: Option<&str>,
        #[case] fails: bool,
    ) {
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["list"].as_array_mut().unwrap().truncate(1);
        orders["result"]["list"][0]["symbol"] = serde_json::json!(symbol);
        let (mut client, _, server) = order_report_client(orders).await;
        client.config.load_ids = scope.map(|id| vec![InstrumentId::from(id)]);
        let command = GenerateOrderStatusReportsBuilder::default()
            .ts_init(UnixNanos::default())
            .open_only(false)
            .build()
            .unwrap();

        let result = client.generate_order_status_reports(&command).await;
        if fails {
            let error = result.unwrap_err();
            assert!(
                error.to_string().contains(if symbol.is_empty() {
                    "order symbol is empty"
                } else {
                    "not found in cache"
                }),
                "{error}"
            );
        } else {
            assert_eq!(result.unwrap(), vec![]);
        }

        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_single_report_outside_scope_is_not_absence() {
        let orders = serde_json::from_str(&crate::common::testing::load_test_json(
            "http_get_orders_realtime.json",
        ))
        .unwrap();
        let (mut client, _, server) = order_report_client(orders).await;
        client.config.load_ids = Some(vec![InstrumentId::from("BTCUSDT-LINEAR.BYBIT")]);

        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from("ETHUSDT-LINEAR.BYBIT")),
            Some(ClientOrderId::from("client-open-1")),
            None,
            None,
            None,
        );

        let error = client
            .generate_order_status_report(&command)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "requested instrument ETHUSDT-LINEAR.BYBIT is outside reconciliation scope"
        );
        assert!(
            !client.provides_bulk_position_coverage(InstrumentId::from("ETHUSDT-LINEAR.BYBIT"))
        );
        assert!(client.provides_bulk_position_coverage(InstrumentId::from("BTCUSDT-LINEAR.BYBIT")));
        server.abort();
    }

    #[rstest]
    #[case::in_window(1761355905606, false)]
    #[case::before_window(1761355905605, true)]
    #[tokio::test]
    async fn test_mass_status_historical_order_completeness(
        #[case] updated_ms: u64,
        #[case] complete: bool,
    ) {
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["list"].as_array_mut().unwrap().truncate(1);
        orders["result"]["list"][0]["symbol"] = serde_json::json!("UNLOADEDUSDT");
        orders["result"]["list"][0]["orderStatus"] = serde_json::json!("Cancelled");
        orders["result"]["list"][0]["updatedTime"] = serde_json::json!(updated_ms.to_string());
        let mut fills: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        fills["result"]["list"] = serde_json::json!([]);
        fills["result"]["nextPageCursor"] = serde_json::json!("");
        let (mut client, _, server) = mass_status_client(fills, false, Some(orders)).await;
        let cutoff = UnixNanos::from(1761355905606 * NANOSECONDS_IN_MILLISECOND);
        let now = cutoff + DurationNanos::try_from_mins(5).unwrap();
        client.clock = Box::leak(Box::new(AtomicTime::new(
            false,
            now + DurationNanos::new(123),
        )));

        let mass = client.generate_mass_status(Some(5)).await.unwrap().unwrap();

        assert_eq!(mass.lookback_start(), Some(cutoff));
        assert_eq!(mass.reports_complete(), complete);
        assert_eq!(mass.order_reports().len(), 0);
        assert_eq!(mass.fill_reports().len(), 0);
        assert_eq!(mass.position_reports().len(), 0);
        server.abort();
    }

    #[rstest]
    #[case::in_scope(None, "BTCUSDT", false)]
    #[case::excluded(Some("ETHUSDT-LINEAR.BYBIT"), "BTCUSDT", true)]
    #[case::unknown_identity(Some("ETHUSDT-LINEAR.BYBIT"), "", false)]
    #[tokio::test]
    async fn test_position_reconciliation_scope(
        #[case] scope: Option<&str>,
        #[case] symbol: &str,
        #[case] succeeds: bool,
    ) {
        let mut response: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_positions.json"),
        )
        .unwrap();
        response["result"]["list"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        response["result"]["nextPageCursor"] = serde_json::json!("");
        response["result"]["list"][0]["symbol"] = serde_json::json!(symbol);
        let router = Router::new().route(
            "/v5/position/list",
            get(move || std::future::ready(Json(response.clone()))),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, _) = test_execution_client();
        client.http_client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            Some(format!("http://{addr}")),
            1,
            0,
            10,
            10,
            5_000,
            None,
        )
        .unwrap();
        client.config.load_ids = scope.map(|id| vec![InstrumentId::from(id)]);

        let result = client
            .generate_bulk_position_status_reports(vec![BybitProductType::Linear])
            .await;

        if succeeds {
            assert_eq!(result.unwrap(), vec![]);
        } else {
            let error = result.unwrap_err();
            assert!(
                error.to_string().contains(if symbol.is_empty() {
                    "Position symbol is empty"
                } else {
                    "not found in cache"
                }),
                "{error}"
            );
        }

        client.http_client.set_use_spot_position_reports(true);
        let error = client
            .http_client
            .request_position_status_reports(
                client.core.account_id,
                BybitProductType::Spot,
                Some(InstrumentId::from("BTCUSDT-SPOT.BYBIT")),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not found in cache"), "{error}");
        server.abort();
    }

    #[rstest]
    #[case::bounded_miss(Some(5), None, 1672282722429, false)]
    #[case::bounded_excluded(Some(5), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, true)]
    #[case::before_window(Some(5), None, 1672282722428, true)]
    #[case::unbounded_miss(None, None, 1672282722429, false)]
    #[case::before_retention_limit(Some(1_439), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, true)]
    #[case::retention_limit(Some(1_440), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, false)]
    #[case::after_retention_limit(Some(1_441), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, false)]
    #[case::two_days(Some(2_880), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, false)]
    #[case::before_venue_limit(Some(10_079), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, false)]
    #[case::venue_limit(Some(10_080), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, false)]
    #[case::venue_window(Some(10_081), Some("ETHUSDT-LINEAR.BYBIT"), 1672282722429, false)]
    #[tokio::test]
    async fn test_mass_status_historical_fill_completeness(
        #[case] lookback: Option<u64>,
        #[case] scope: Option<&str>,
        #[case] exec_ms: u64,
        #[case] complete: bool,
    ) {
        let mut fills: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        fills["result"]["list"].as_array_mut().unwrap().truncate(1);
        fills["result"]["list"][0]["symbol"] = serde_json::json!("UNLOADEDUSDT");
        fills["result"]["list"][0]["execTime"] = serde_json::json!(exec_ms.to_string());
        fills["result"]["nextPageCursor"] = serde_json::json!("");
        let (mut client, _, server) = mass_status_client(fills, false, None).await;
        let cutoff = UnixNanos::from(1672282722429 * NANOSECONDS_IN_MILLISECOND);
        client.clock = Box::leak(Box::new(AtomicTime::new(
            false,
            cutoff + DurationNanos::try_from_mins(lookback.unwrap_or(5)).unwrap(),
        )));
        client.config.load_ids = scope.map(|id| vec![InstrumentId::from(id)]);

        let result = client.generate_mass_status(lookback).await;

        let mass = result.unwrap().unwrap();
        assert_eq!(mass.lookback_start(), lookback.map(|_| cutoff));
        assert_eq!(mass.reports_complete(), lookback.is_none() || complete);
        assert_eq!(mass.order_reports().len(), 0);
        assert_eq!(mass.fill_reports().len(), 0);
        assert_eq!(mass.position_reports().len(), 0);

        server.abort();
    }

    #[rstest]
    #[case::linear(BybitProductType::Linear, None, true)]
    #[case::all_options(BybitProductType::Option, None, false)]
    #[case::eth_option(
        BybitProductType::Option,
        Some("ETH-26JUN26-16000-P-OPTION.BYBIT"),
        false
    )]
    #[case::btc_option(
        BybitProductType::Option,
        Some("BTC-26JUN26-100000-C-OPTION.BYBIT"),
        false
    )]
    #[tokio::test]
    async fn test_mass_status_option_history_completeness(
        #[case] product_type: BybitProductType,
        #[case] scope: Option<&str>,
        #[case] complete: bool,
    ) {
        let mut fills: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        fills["result"]["list"] = serde_json::json!([]);
        fills["result"]["nextPageCursor"] = serde_json::json!("");
        let (mut client, _, server) = mass_status_client(fills, false, None).await;
        let cutoff = UnixNanos::from(1672282722429 * NANOSECONDS_IN_MILLISECOND);
        client.clock = Box::leak(Box::new(AtomicTime::new(
            false,
            cutoff + DurationNanos::try_from_mins(5).unwrap(),
        )));
        client.config.product_types = vec![product_type];
        client.config.load_ids = scope.map(|id| vec![InstrumentId::from(id)]);
        client.config.validate().unwrap();

        let mass = client.generate_mass_status(Some(5)).await.unwrap().unwrap();

        assert_eq!(mass.lookback_start(), Some(cutoff));
        assert_eq!(mass.reports_complete(), complete);
        assert_eq!(mass.order_reports().len(), 0);
        assert_eq!(mass.fill_reports().len(), 0);
        assert_eq!(mass.position_reports().len(), 0);
        server.abort();
    }

    #[rstest]
    #[case::missing("missing", false)]
    #[case::report("report", true)]
    #[case::cached_client("cached_client", true)]
    #[case::cached_venue("cached_venue", true)]
    #[tokio::test]
    async fn test_bounded_mass_status_fill_order_linkage(
        #[case] evidence: &str,
        #[case] complete: bool,
    ) {
        let mut fills: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        fills["result"]["nextPageCursor"] = serde_json::json!("");
        fills["result"]["list"].as_array_mut().unwrap().truncate(1);
        fills["result"]["list"][0]["symbol"] = serde_json::json!("ETHUSDT");
        fills["result"]["list"][0]["orderId"] = serde_json::json!("open-order-1");
        fills["result"]["list"][0]["orderLinkId"] = serde_json::json!("fill-client");

        let orders = (evidence == "report").then(|| {
            let mut orders: serde_json::Value = serde_json::from_str(
                &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
            )
            .unwrap();
            orders["result"]["list"].as_array_mut().unwrap().truncate(1);
            orders
        });

        let (mut client, cache, server) = mass_status_client(fills, false, orders).await;
        let cutoff = UnixNanos::from(1672282722429 * NANOSECONDS_IN_MILLISECOND);
        client.clock = Box::leak(Box::new(AtomicTime::new(
            false,
            cutoff + DurationNanos::try_from_mins(5).unwrap(),
        )));

        if evidence.starts_with("cached_") {
            let client_order_id = ClientOrderId::from(if evidence == "cached_client" {
                "fill-client"
            } else {
                "cached-client"
            });

            let order = OrderTestBuilder::new(OrderType::Limit)
                .client_order_id(client_order_id)
                .instrument_id(InstrumentId::from("ETHUSDT-LINEAR.BYBIT"))
                .side(OrderSide::Buy)
                .quantity(Quantity::from("0.100"))
                .price(Price::from("50000.00"))
                .build();
            cache
                .borrow_mut()
                .add_order(order, None, None, false)
                .unwrap();

            if evidence == "cached_venue" {
                cache
                    .borrow_mut()
                    .add_venue_order_id(
                        &client_order_id,
                        &VenueOrderId::from("open-order-1"),
                        false,
                    )
                    .unwrap();
            }
        }

        let mass = client.generate_mass_status(Some(5)).await.unwrap().unwrap();

        assert_eq!(mass.lookback_start(), Some(cutoff));
        assert_eq!(mass.reports_complete(), complete);
        assert_eq!(
            mass.order_reports().len(),
            usize::from(evidence == "report")
        );
        let reports = mass.fill_reports();
        assert_eq!(reports.len(), 1);
        let fills = reports.get(&VenueOrderId::from("open-order-1")).unwrap();
        assert_eq!(fills.len(), 1);
        assert_eq!(
            fills[0].client_order_id,
            Some(ClientOrderId::from("fill-client"))
        );
        assert_eq!(fills[0].last_qty, Quantity::from("0.100"));
        assert_eq!(mass.position_reports().len(), 0);
        server.abort();
    }

    #[rstest]
    #[case::order_parse(
        true,
        "Failed to parse order.qty='not-a-number' as Decimal: Invalid decimal: unknown character"
    )]
    #[case::commission(
        false,
        "Failed to create commission from execFee='9999999999999999999999999999'"
    )]
    #[tokio::test]
    async fn test_bounded_mass_status_parse_failures(
        #[case] order_failure: bool,
        #[case] message: &str,
    ) {
        let mut fills: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        fills["result"]["nextPageCursor"] = serde_json::json!("");
        fills["result"]["list"].as_array_mut().unwrap().truncate(1);
        fills["result"]["list"][0]["symbol"] = serde_json::json!("ETHUSDT");
        fills["result"]["list"][0]["execFee"] = serde_json::json!("9999999999999999999999999999");
        let cutoff = UnixNanos::from(1672282722429 * NANOSECONDS_IN_MILLISECOND);

        let orders = if order_failure {
            fills["result"]["list"] = serde_json::json!([]);
            let mut orders: serde_json::Value = serde_json::from_str(
                &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
            )
            .unwrap();
            orders["result"]["list"].as_array_mut().unwrap().truncate(1);
            orders["result"]["list"][0]["qty"] = serde_json::json!("not-a-number");
            Some(orders)
        } else {
            None
        };

        let (mut client, _, server) = mass_status_client(fills, false, orders).await;
        client.clock = Box::leak(Box::new(AtomicTime::new(
            false,
            cutoff + DurationNanos::try_from_mins(5).unwrap(),
        )));

        let error = client.generate_mass_status(Some(5)).await.unwrap_err();

        assert_eq!(error.to_string(), message);
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_bounded_mass_status_retains_old_open_order() {
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["list"].as_array_mut().unwrap().truncate(1);
        let mut fills: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        fills["result"]["list"] = serde_json::json!([]);
        fills["result"]["nextPageCursor"] = serde_json::json!("");
        let (mut client, cache, server) = mass_status_client(fills, false, Some(orders)).await;
        let cutoff = UnixNanos::from(1761356000000 * NANOSECONDS_IN_MILLISECOND);
        client.clock = Box::leak(Box::new(AtomicTime::new(
            false,
            cutoff + DurationNanos::try_from_mins(5).unwrap(),
        )));
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let client_order_id = ClientOrderId::from("client-open-1");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.010"))
            .price(Price::from("3930.41"))
            .build();
        let strategy_id = order.strategy_id();
        cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();

        let mass = client.generate_mass_status(Some(5)).await.unwrap().unwrap();

        assert_eq!(mass.lookback_start(), Some(cutoff));
        assert!(mass.reports_complete());
        let reports = mass.order_reports();
        assert_eq!(reports.len(), 1);
        let report = reports.get(&VenueOrderId::from("open-order-1")).unwrap();
        assert_eq!(
            (report.instrument_id, report.order_status, report.ts_last),
            (
                instrument_id,
                OrderStatus::Accepted,
                UnixNanos::from(1761355905606 * NANOSECONDS_IN_MILLISECOND)
            )
        );
        let identity = client
            .dispatch_state
            .order_identities
            .get(&client_order_id)
            .unwrap();
        assert_eq!(
            (
                identity.instrument_id,
                identity.strategy_id,
                identity.order_side,
                identity.order_type,
                identity.venue_position_id
            ),
            (
                instrument_id,
                strategy_id,
                OrderSide::Buy,
                OrderType::Limit,
                None
            )
        );
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_bulk_report_parse_failure_returns_error() {
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["list"][0]["qty"] = serde_json::json!("not-a-number");
        let (client, _, server) = order_report_client(orders).await;

        let command = GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            None,
            None,
            None,
            None,
            None,
        );

        for fused in [false, true] {
            let error = if fused {
                client
                    .generate_order_status_reports(&command)
                    .await
                    .unwrap_err()
            } else {
                let task = client.generate_order_status_reports_task(&command).unwrap();
                tokio::spawn(task.collection).await.unwrap();
                (task.finish)().unwrap_err()
            };

            assert!(
                error
                    .to_string()
                    .contains("Failed to parse order.qty='not-a-number' as Decimal"),
                "error was: {error}",
            );
        }

        server.abort();
    }

    #[rstest]
    #[case::unbounded(None, None, vec!["open-order-1", "open-order-2"])]
    #[case::after_first(Some(1761355905607), None, vec!["open-order-2"])]
    #[case::before_second(None, Some(1761355905608), vec!["open-order-1"])]
    #[case::inclusive_bounds(Some(1761355905606), Some(1761355905606), vec!["open-order-1"])]
    #[tokio::test]
    async fn test_bulk_report_task_preserves_window_and_fused_parity(
        #[case] start_ms: Option<u64>,
        #[case] end_ms: Option<u64>,
        #[case] expected: Vec<&str>,
    ) {
        let orders = serde_json::from_str(&crate::common::testing::load_test_json(
            "http_get_orders_realtime.json",
        ))
        .unwrap();
        let (client, _, server) = order_report_client(orders).await;

        let command = GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            Some(InstrumentId::from("ETHUSDT-LINEAR.BYBIT")),
            start_ms.map(|ms| UnixNanos::from(ms * 1_000_000)),
            end_ms.map(|ms| UnixNanos::from(ms * 1_000_000)),
            None,
            None,
        );
        let task = client.generate_order_status_reports_task(&command).unwrap();
        tokio::spawn(task.collection).await.unwrap();

        let collected = (task.finish)().unwrap();
        let fused = client
            .generate_order_status_reports(&command)
            .await
            .unwrap();

        let expected: Vec<_> = expected.into_iter().map(VenueOrderId::from).collect();
        assert_eq!(
            collected
                .iter()
                .map(|report| report.venue_order_id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            fused
                .iter()
                .map(|report| report.venue_order_id)
                .collect::<Vec<_>>(),
            expected
        );
        server.abort();
    }

    async fn order_report_client(
        response: serde_json::Value,
    ) -> (
        BybitExecutionClient,
        Rc<RefCell<Cache>>,
        tokio::task::JoinHandle<()>,
    ) {
        let history = response.clone();
        let router = Router::new()
            .route(
                "/v5/order/realtime",
                get(move || std::future::ready(Json(response.clone()))),
            )
            .route(
                "/v5/order/history",
                get(move || std::future::ready(Json(history.clone()))),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, cache) = test_execution_client();
        client.http_client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            Some(format!("http://{addr}")),
            1,
            0,
            10,
            10,
            5_000,
            None,
        )
        .unwrap();
        let mut instrument = crypto_perpetual_ethusdt();
        instrument.id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        client.http_client.cache_instrument(instrument.into());
        (client, cache, server)
    }

    #[rstest]
    #[tokio::test]
    async fn test_bulk_report_finalization_emits_fill_before_status_result() {
        let client_order_id = ClientOrderId::from("O-FAST-DRAIN");
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let mut report =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Filled, None);
        report.filled_qty = Quantity::from("0.5");
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["nextPageCursor"] = serde_json::json!("");
        orders["result"]["list"].as_array_mut().unwrap().truncate(1);
        orders["result"]["list"][0]["orderStatus"] = serde_json::json!("Filled");
        orders["result"]["list"][0]["orderId"] = serde_json::json!(report.venue_order_id.as_str());
        orders["result"]["list"][0]["orderLinkId"] = serde_json::json!(client_order_id.as_str());
        orders["result"]["list"][0]["cumExecQty"] = serde_json::json!("0.5");
        let (mut client, _cache, server) = order_report_client(orders).await;
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let json = crate::common::testing::load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = parse_linear_instrument(
            &response.result.list[1],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut instruments = AHashMap::new();
        instruments.insert(instrument_id.symbol.inner(), instrument);
        client.instruments_cache = Arc::new(instruments);
        client.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("S-001"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                venue_position_id: None,
            },
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        let json = crate::common::testing::load_test_json("ws_account_execution_fast.json");
        let mut fast: crate::websocket::messages::BybitWsAccountExecutionFastMsg =
            serde_json::from_str(&json).unwrap();
        fast.data[0].category = BybitProductType::Linear;
        fast.data[0].symbol = Ustr::from("ETHUSDT");
        fast.data[0].order_id = report.venue_order_id.as_str().into();
        fast.data[0].order_link_id = Ustr::from("");
        fast.data[0].exec_price = "3000.00".to_string();
        fast.data[0].exec_qty = "0.5".to_string();
        dispatch_ws_message(
            &crate::websocket::messages::BybitWsMessage::AccountExecutionFast(fast),
            client.core.account_id,
            &client.instruments_cache,
            client.clock,
            &client.dispatch_state,
            &client.emitter,
        );
        assert!(rx.try_recv().is_err());

        let command = GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            Some(instrument_id),
            None,
            None,
            None,
            None,
        );

        let task = client.generate_order_status_reports_task(&command).unwrap();
        task.collection.await;
        let reports = (task.finish)().unwrap();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].filled_qty, Quantity::from("0.5"));
        assert_eq!(reports[0].venue_order_id, report.venue_order_id);
        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
                assert_eq!(fill.last_qty, Quantity::from("0.5"));
                assert_eq!(
                    fill.trade_id.as_str(),
                    "3510f361-0add-5c7b-a2e7-9679810944fc"
                );
            }
            other => panic!("Expected FillReport after finalization, found {other:?}"),
        }

        assert!(rx.try_recv().is_err());
        server.abort();
    }

    #[rstest]
    #[case::pending(true)]
    #[case::late_closed(false)]
    #[tokio::test]
    async fn test_query_order_restores_pending_fill_without_recreating_completed_state(
        #[case] recover_pending: bool,
    ) {
        let client_order_id = ClientOrderId::from("O-FAST-DRAIN");
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let mut report =
            sample_order_status_report(client_order_id, instrument_id, OrderStatus::Filled, None);
        report.filled_qty = Quantity::from("0.5");
        let mut orders: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_orders_realtime.json"),
        )
        .unwrap();
        orders["result"]["nextPageCursor"] = serde_json::json!("");
        orders["result"]["list"].as_array_mut().unwrap().truncate(1);
        orders["result"]["list"][0]["orderStatus"] = serde_json::json!("Filled");
        orders["result"]["list"][0]["orderId"] = serde_json::json!(report.venue_order_id.as_str());
        orders["result"]["list"][0]["orderLinkId"] = serde_json::json!(client_order_id.as_str());
        orders["result"]["list"][0]["cumExecQty"] = serde_json::json!("0.5");
        let (mut client, _cache, server) = order_report_client(orders).await;
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let json = crate::common::testing::load_test_json("http_get_instruments_linear.json");
        let response: BybitInstrumentLinearResponse = serde_json::from_str(&json).unwrap();
        let instrument = parse_linear_instrument(
            &response.result.list[1],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        let mut instruments = AHashMap::new();
        instruments.insert(instrument_id.symbol.inner(), instrument);
        client.instruments_cache = Arc::new(instruments);
        if recover_pending {
            client.dispatch_state.order_identities.insert(
                client_order_id,
                OrderIdentity {
                    instrument_id,
                    strategy_id: StrategyId::from("S-001"),
                    order_side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                    venue_position_id: None,
                },
            );
        }

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        if recover_pending {
            let json = crate::common::testing::load_test_json("ws_account_execution_fast.json");
            let mut fast: crate::websocket::messages::BybitWsAccountExecutionFastMsg =
                serde_json::from_str(&json).unwrap();
            fast.data[0].category = BybitProductType::Linear;
            fast.data[0].symbol = Ustr::from("ETHUSDT");
            fast.data[0].order_id = report.venue_order_id.as_str().into();
            fast.data[0].order_link_id = Ustr::from("");
            fast.data[0].exec_price = "3000.00".to_string();
            fast.data[0].exec_qty = "0.5".to_string();
            dispatch_ws_message(
                &crate::websocket::messages::BybitWsMessage::AccountExecutionFast(fast),
                client.core.account_id,
                &client.instruments_cache,
                client.clock,
                &client.dispatch_state,
                &client.emitter,
            );
        }

        assert!(rx.try_recv().is_err());

        client
            .query_order(QueryOrder::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                client_order_id,
                Some(report.venue_order_id),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        if recover_pending {
            assert!(matches!(
                rx.try_recv().unwrap(),
                ExecutionEvent::Order(OrderEventAny::Accepted(_))
            ));
            assert!(matches!(
                rx.try_recv().unwrap(),
                ExecutionEvent::Report(ExecutionReport::Fill(_))
            ));
        }

        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Report(ExecutionReport::Order(_))
        ));
        assert!(rx.try_recv().is_err());

        if !recover_pending {
            assert!(client.dispatch_state.order_identities.is_empty());
            assert!(client.dispatch_state.order_venue_ids.is_empty());
            assert!(client.dispatch_state.venue_order_ids.is_empty());
        }

        server.abort();
    }

    async fn fill_report_client(
        response: serde_json::Value,
    ) -> (
        BybitExecutionClient,
        Rc<RefCell<Cache>>,
        tokio::task::JoinHandle<()>,
    ) {
        let router = Router::new().route(
            "/v5/execution/list",
            get(move || std::future::ready(Json(response.clone()))),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, cache) = test_execution_client();
        client.http_client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            Some(format!("http://{addr}")),
            1,
            0,
            10,
            10,
            5_000,
            None,
        )
        .unwrap();
        let mut instrument = crypto_perpetual_ethusdt();
        instrument.id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        client.http_client.cache_instrument(instrument.into());
        (client, cache, server)
    }

    async fn mass_status_client(
        execution_response: serde_json::Value,
        position_fails: bool,
        order_response: Option<serde_json::Value>,
    ) -> (
        BybitExecutionClient,
        Rc<RefCell<Cache>>,
        tokio::task::JoinHandle<()>,
    ) {
        let empty = serde_json::json!({
            "retCode": 0,
            "retMsg": "OK",
            "result": {"list": [], "nextPageCursor": ""},
            "retExtInfo": {},
            "time": 1761355906606i64
        });

        let position_response = if position_fails {
            serde_json::json!({
                "retCode": 10001,
                "retMsg": "position request failed",
                "result": {},
                "retExtInfo": {},
                "time": 1761355906606i64
            })
        } else {
            empty.clone()
        };

        let realtime = order_response.clone().unwrap_or_else(|| empty.clone());
        let history = order_response.unwrap_or(empty);

        let router = Router::new()
            .route(
                "/v5/order/realtime",
                get(move || std::future::ready(Json(realtime.clone()))),
            )
            .route(
                "/v5/order/history",
                get(move |Query(params): Query<AHashMap<String, String>>| {
                    assert_eq!(params.get("startTime"), None);
                    assert_eq!(params.get("endTime"), None);
                    std::future::ready(Json(history.clone()))
                }),
            )
            .route(
                "/v5/execution/list",
                get(move || std::future::ready(Json(execution_response.clone()))),
            )
            .route(
                "/v5/position/list",
                get(move || std::future::ready(Json(position_response.clone()))),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let (mut client, cache) = test_execution_client();
        client.http_client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            Some(format!("http://{addr}")),
            1,
            0,
            10,
            10,
            5_000,
            None,
        )
        .unwrap();
        let mut instrument = crypto_perpetual_ethusdt();
        instrument.id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        client.http_client.cache_instrument(instrument.into());
        (client, cache, server)
    }

    fn assert_websocket_fill_after_rest(
        client: &mut BybitExecutionClient,
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
    ) {
        let mut instrument = crypto_perpetual_ethusdt();
        instrument.id = instrument_id;
        let instrument: InstrumentAny = instrument.into();

        client.instruments_cache = Arc::new(AHashMap::from_iter([(
            instrument_id.symbol.inner(),
            instrument,
        )]));
        let json = crate::common::testing::load_test_json("ws_account_execution_fast.json");
        let mut fast: crate::websocket::messages::BybitWsAccountExecutionFastMsg =
            serde_json::from_str(&json).unwrap();
        fast.data[0].category = BybitProductType::Linear;
        fast.data[0].symbol = Ustr::from("ETHUSDT");
        fast.data[0].exec_id = "f1dce92e-1g29-6977-a526-dg420c6ecb4c".to_string();
        fast.data[0].exec_price = "3000.00".to_string();
        fast.data[0].exec_qty = "0.5".to_string();
        fast.data[0].order_id = venue_order_id.as_str().into();
        fast.data[0].order_link_id = client_order_id.as_str().into();
        fast.data[0].side = BybitOrderSide::Sell;
        fast.data[0].is_maker = false;

        dispatch_ws_message(
            &crate::websocket::messages::BybitWsMessage::AccountExecutionFast(fast),
            client.core.account_id,
            &client.instruments_cache,
            client.clock,
            &client.dispatch_state,
            &client.emitter,
        );

        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.client_order_id, Some(client_order_id));
                assert_eq!(report.last_qty, Quantity::from("0.5"));
                assert_eq!(
                    report.trade_id.as_str(),
                    "f1dce92e-1g29-6977-a526-dg420c6ecb4c"
                );
            }
            other => panic!("Expected FillReport, found {other:?}"),
        }

        assert!(rx.try_recv().is_err());
        assert!(
            !client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );
        assert!(
            !client
                .dispatch_state
                .order_venue_ids
                .contains_key(&client_order_id)
        );
        assert!(
            !client
                .dispatch_state
                .terminal_order_quantities
                .contains_key(&client_order_id)
        );
    }

    #[rstest]
    #[case::success(false)]
    #[case::position_failure(true)]
    #[tokio::test]
    async fn test_mass_status_does_not_suppress_websocket_fill(#[case] position_fails: bool) {
        let mut response: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        response["result"]["nextPageCursor"] = serde_json::json!("");
        let (mut client, _cache, server) = mass_status_client(response, position_fails, None).await;
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let client_order_id = ClientOrderId::from("test-order-002");
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        client.config.load_ids = Some(vec![instrument_id]);
        let venue_order_id = VenueOrderId::from("7b954230-6a41-3ba8-bc20-26d20ab44b83");
        client.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("S-001"),
                order_side: OrderSide::Sell,
                order_type: OrderType::Limit,
                venue_position_id: None,
            },
        );

        record_venue_order_id(client_order_id, venue_order_id, &client.dispatch_state);
        client
            .dispatch_state
            .terminal_order_quantities
            .insert(client_order_id, Quantity::from("0.5"));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let result = client.generate_mass_status(None).await;

        if position_fails {
            assert!(result.is_err());
        } else {
            assert!(result.unwrap().is_some());
        }

        assert!(
            client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );
        assert_websocket_fill_after_rest(
            &mut client,
            &mut rx,
            client_order_id,
            instrument_id,
            venue_order_id,
        );
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_generate_fill_reports_does_not_suppress_websocket_fill() {
        let mut response: serde_json::Value = serde_json::from_str(
            &crate::common::testing::load_test_json("http_get_executions.json"),
        )
        .unwrap();
        response["result"]["nextPageCursor"] = serde_json::json!("");
        let (mut client, _cache, server) = fill_report_client(response).await;
        Arc::get_mut(&mut client.dispatch_state)
            .unwrap()
            .fast_execution = true;
        let client_order_id = ClientOrderId::from("test-order-002");
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let venue_order_id = VenueOrderId::from("7b954230-6a41-3ba8-bc20-26d20ab44b83");
        client.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("S-001"),
                order_side: OrderSide::Sell,
                order_type: OrderType::Limit,
                venue_position_id: None,
            },
        );

        record_venue_order_id(client_order_id, venue_order_id, &client.dispatch_state);
        client
            .dispatch_state
            .terminal_order_quantities
            .insert(client_order_id, Quantity::from("0.5"));

        let command = GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(instrument_id),
            Some(venue_order_id),
            None,
            None,
            None,
            None,
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let reports = client.generate_fill_reports(command.clone()).await.unwrap();

        assert_eq!(reports.len(), 1);
        assert!(client.generate_fill_reports_task(&command).is_none());
        assert!(
            client
                .dispatch_state
                .order_identities
                .contains_key(&client_order_id)
        );
        assert_websocket_fill_after_rest(
            &mut client,
            &mut rx,
            client_order_id,
            instrument_id,
            venue_order_id,
        );
        server.abort();
    }

    #[rstest]
    #[case(TimeInForce::Gtc, Ok(Some(BybitTimeInForce::Gtc)))]
    #[case(TimeInForce::Gtd, Ok(Some(BybitTimeInForce::Gtc)))]
    #[case(TimeInForce::Ioc, Ok(Some(BybitTimeInForce::Ioc)))]
    #[case(TimeInForce::Fok, Ok(Some(BybitTimeInForce::Fok)))]
    #[case(
        TimeInForce::Day,
        Err(OrderDeniedReason::UnsupportedTimeInForce(TimeInForce::Day))
    )]
    #[case(
        TimeInForce::AtTheOpen,
        Err(OrderDeniedReason::UnsupportedTimeInForce(TimeInForce::AtTheOpen))
    )]
    #[case(
        TimeInForce::AtTheClose,
        Err(OrderDeniedReason::UnsupportedTimeInForce(TimeInForce::AtTheClose))
    )]
    fn test_ws_params_time_in_force(
        #[case] tif: TimeInForce,
        #[case] expected: Result<Option<BybitTimeInForce>, OrderDeniedReason>,
    ) {
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        builder
            .instrument_id(InstrumentId::from("BTCUSDT-LINEAR.BYBIT"))
            .time_in_force(tif)
            .quantity(Quantity::from("0.02"))
            .price(Price::from("10000.00"));

        if tif == TimeInForce::Gtd {
            builder.expire_time(UnixNanos::from(1_900_000_000_000_000_000));
        }

        let order = builder.build();
        let params = BybitExecutionClient::build_ws_place_params(
            &order,
            BybitProductType::Linear,
            "BTCUSDT",
            &BybitTpSlParams::default(),
            None,
            None,
        );
        assert_eq!(
            params
                .map(|p| p.time_in_force)
                .map_err(|e| e.downcast::<OrderDeniedReason>().unwrap()),
            expected,
        );
    }

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
            smp_type: None,
            mmp: None,
            position_idx: None,
            bbo_side_type: None,
            bbo_level: None,
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
        BybitExecutionClient::map_order_type(OrderType::TrailingStopMarket).unwrap_err();
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
        let result = BybitExecutionClient::parse_derivative_symbol(symbol_str);
        assert_eq!(result.is_some(), keeps);
    }

    #[rstest]
    fn test_parse_derivative_symbol_rejects_malformed() {
        assert!(BybitExecutionClient::parse_derivative_symbol("not-a-real-symbol").is_none());
    }

    #[rstest]
    #[case::matches_msg("Position mode has not been modified", "110025", true)]
    #[case::matches_code("retCode 110025: noop", "110025", true)]
    #[case::matches_msg_only("Already not been modified", "", true)]
    #[case::wrong_code("retCode 99999: other", "110025", false)]
    #[case::empty_no_modified_msg("retCode 99999", "", false)]
    fn test_is_unchanged_error(#[case] msg: &str, #[case] code: &str, #[case] expected: bool) {
        let err = anyhow::anyhow!("{msg}");
        assert_eq!(
            BybitExecutionClient::is_unchanged_error(&err, code),
            expected
        );
    }

    #[rstest]
    #[case::matches("Margin needs to be equal to or greater than 0.5", true)]
    #[case::no_match("Some other error", false)]
    fn test_is_low_margin_error(#[case] msg: &str, #[case] expected: bool) {
        let err = anyhow::anyhow!("{msg}");
        assert_eq!(BybitExecutionClient::is_low_margin_error(&err), expected);
    }

    #[rstest]
    fn test_submit_rejection_reason_matches_confirmed_rejection() {
        let err = anyhow::Error::from(BybitSubmitOrderError::Rejected {
            reason: "EC_PostOnlyWillTakeLiquidity".to_string(),
        });

        assert_eq!(
            submit_rejection_reason(&err),
            Some("EC_PostOnlyWillTakeLiquidity"),
        );
    }

    #[rstest]
    fn test_submit_rejection_reason_ignores_post_submit_lookup_failure() {
        let err = anyhow::Error::from(BybitSubmitOrderError::PostSubmitLookup {
            source: anyhow::Error::from(BybitHttpError::BybitError {
                error_code: 110017,
                message: "current position is zero, cannot fix reduce-only order qty".to_string(),
            }),
        })
        .context("Submit order failed");

        assert_eq!(submit_rejection_reason(&err), None);
    }

    #[rstest]
    fn test_submit_rejection_reason_ignores_missing_order_id() {
        let err = anyhow::Error::from(BybitSubmitOrderError::MissingOrderId);

        assert_eq!(submit_rejection_reason(&err), None);
    }

    #[rstest]
    fn test_submit_rejection_reason_matches_venue_http_error() {
        let err = anyhow::Error::from(BybitHttpError::BybitError {
            error_code: 110017,
            message: "current position is zero, cannot fix reduce-only order qty".to_string(),
        });

        assert_eq!(
            submit_rejection_reason(&err),
            Some("current position is zero, cannot fix reduce-only order qty"),
        );
    }

    #[rstest]
    fn test_submit_rejection_reason_ignores_ambiguous_http_error() {
        let err = anyhow::Error::from(BybitHttpError::BybitError {
            error_code: 10016,
            message: "rate limit exceeded".to_string(),
        });

        assert_eq!(submit_rejection_reason(&err), None);
    }

    #[rstest]
    #[case::buy(Some(OrderSide::Buy), vec![0, 2])]
    #[case::sell(Some(OrderSide::Sell), vec![1, 3])]
    #[tokio::test]
    async fn test_cancel_all_orders_filters_by_side_and_preserves_owners(
        #[case] order_side: Option<OrderSide>,
        #[case] expected_indices: Vec<usize>,
    ) {
        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.ws_trade.close().await.unwrap();

        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let other_instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let account_id = client.core.account_id;
        let mut orders = Vec::new();

        for (index, (instrument, owner, side, open)) in [
            (instrument_id, "S-001", OrderSide::Buy, true),
            (instrument_id, "S-001", OrderSide::Sell, true),
            (instrument_id, "S-002", OrderSide::Buy, true),
            (instrument_id, "S-002", OrderSide::Sell, true),
            (other_instrument_id, "S-001", OrderSide::Buy, true),
            (other_instrument_id, "S-002", OrderSide::Sell, true),
            (instrument_id, "S-001", OrderSide::Buy, false),
            (instrument_id, "S-002", OrderSide::Sell, false),
        ]
        .into_iter()
        .enumerate()
        {
            let client_order_id = ClientOrderId::from(format!("O-CANCEL-ALL-{index}"));
            let mut builder = OrderTestBuilder::new(OrderType::Limit);
            let order = builder
                .instrument_id(instrument)
                .client_order_id(client_order_id)
                .strategy_id(StrategyId::from(owner))
                .side(side)
                .quantity(Quantity::from("1"))
                .price(Price::from("10000.00"))
                .build();
            let venue_order_id = VenueOrderId::from(format!("BYBIT-{index}"));
            let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
            cache
                .borrow_mut()
                .add_order(order, None, Some(*BYBIT_CLIENT_ID), false)
                .unwrap();
            let order = cache.borrow_mut().update_order(&accepted).unwrap();

            if !open {
                let canceled =
                    TestOrderEventStubs::canceled(&order, account_id, Some(venue_order_id));
                cache.borrow_mut().update_order(&canceled).unwrap();
            }

            orders.push((order, venue_order_id));
        }

        client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                order_side,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        // The closed transport rejects the batch, exposing selection and
        // event ownership through the existing failure path.
        let mut actual = Vec::new();

        for _ in &expected_indices {
            let event = rx.try_recv().expect("expected OrderCancelDenied event");
            match event {
                ExecutionEvent::Order(OrderEventAny::CancelDenied(rejected)) => {
                    actual.push((rejected.client_order_id, rejected.strategy_id));
                }
                event => panic!("expected OrderCancelDenied, was {event:?}"),
            }
        }

        let mut expected: Vec<_> = expected_indices
            .iter()
            .map(|&index| {
                let (order, _) = &orders[index];
                (order.client_order_id(), order.strategy_id())
            })
            .collect();

        actual.sort();
        expected.sort();

        assert_eq!(actual, expected);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_cancel_all_orders_with_side_and_empty_cache_sends_nothing() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.ws_trade.close().await.unwrap();

        client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from("S-001"),
                InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
                Some(OrderSide::Buy),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        assert!(rx.try_recv().is_err());
        assert!(client.dispatch_state.pending_requests.is_empty());
    }

    #[rstest]
    #[case::working("New", "0")]
    #[case::filled("Filled", "0.010")]
    #[tokio::test]
    async fn test_cancel_all_orders_leaves_unconfirmed_cancellation_open(
        #[case] status: &str,
        #[case] filled_qty: &str,
    ) {
        let json = crate::common::testing::load_test_json("http_get_orders_realtime.json");
        let mut orders: serde_json::Value = serde_json::from_str(&json).unwrap();
        orders["result"]["list"][0]["orderStatus"] = serde_json::json!(status);
        orders["result"]["list"][0]["cumExecQty"] = serde_json::json!(filled_qty);
        orders["result"]["list"][0]["avgPrice"] = serde_json::json!("3930.41");
        let mut empty = orders.clone();
        empty["result"]["list"] = serde_json::json!([]);
        let history = empty.clone();
        let queried = Arc::new(AtomicBool::new(false));
        let query_observed = Arc::clone(&queried);

        let router = Router::new()
            .route(
                "/v5/order/cancel-all",
                post(move || std::future::ready(Json(empty.clone()))),
            )
            .route(
                "/v5/order/history",
                get(move || std::future::ready(Json(history.clone()))),
            )
            .route(
                "/v5/order/realtime",
                get(move || {
                    query_observed.store(true, Ordering::Relaxed);
                    std::future::ready(Json(orders.clone()))
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.config.http_timeout_secs = 1;
        client.config.retry_delay_initial_ms = 10;
        client.http_client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            Some(format!("http://{addr}")),
            1,
            0,
            10,
            10,
            5_000,
            None,
        )
        .unwrap();
        let instrument_id = InstrumentId::from("ETHUSDT-LINEAR.BYBIT");
        let mut instrument = crypto_perpetual_ethusdt();
        instrument.id = instrument_id;
        client.http_client.cache_instrument(instrument.into());
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        let order = builder
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("client-open-1"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.01"))
            .price(Price::from("3930.41"))
            .build();
        let accepted = TestOrderEventStubs::accepted(
            &order,
            client.core.account_id,
            VenueOrderId::from("open-order-1"),
        );
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*BYBIT_CLIENT_ID), false)
            .unwrap();
        cache.borrow_mut().update_order(&accepted).unwrap();
        client
            .cancel_all_orders(CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                order.strategy_id(),
                instrument_id,
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_until_async(
            || async { client.pending_tasks.all_finished() },
            Duration::from_secs(5),
        )
        .await;

        assert!(queried.load(Ordering::Relaxed));
        assert_eq!(
            cache
                .borrow()
                .order(&order.client_order_id())
                .unwrap()
                .status(),
            OrderStatus::Accepted
        );
        assert!(rx.try_recv().is_err());
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_batch_cancel_orders_failure_preserves_owning_strategies() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.ws_trade.close().await.unwrap();

        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");

        let cancels = ["S-001", "S-002"]
            .into_iter()
            .enumerate()
            .map(|(index, owner)| {
                CancelOrder::new(
                    TraderId::from("TESTER-001"),
                    Some(*BYBIT_CLIENT_ID),
                    StrategyId::from(owner),
                    instrument_id,
                    ClientOrderId::from(format!("O-BATCH-OWNER-{index}")),
                    Some(VenueOrderId::from(format!("BYBIT-BATCH-{index}"))),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )
            })
            .collect::<Vec<_>>();

        client
            .batch_cancel_orders(BatchCancelOrders::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                cancels.clone(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        let mut actual = Vec::new();

        for _ in &cancels {
            let event = rx.try_recv().expect("expected OrderCancelDenied event");
            match event {
                ExecutionEvent::Order(OrderEventAny::CancelDenied(rejected)) => {
                    actual.push((
                        rejected.client_order_id,
                        rejected.strategy_id,
                        rejected.causation_id,
                    ));
                }
                event => panic!("expected OrderCancelDenied, was {event:?}"),
            }
        }

        let mut expected: Vec<_> = cancels
            .iter()
            .map(|cancel| {
                (
                    cancel.client_order_id,
                    cancel.strategy_id,
                    Some(cancel.command_id),
                )
            })
            .collect();

        actual.sort_by_key(|entry| entry.0);
        expected.sort_by_key(|entry| entry.0);

        assert_eq!(actual, expected);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_batch_modify_orders_failure_preserves_owning_strategies() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.ws_trade.close().await.unwrap();

        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");

        let modifies = ["S-001", "S-002"]
            .into_iter()
            .enumerate()
            .map(|(index, owner)| {
                ModifyOrder::new(
                    TraderId::from("TESTER-001"),
                    Some(*BYBIT_CLIENT_ID),
                    StrategyId::from(owner),
                    instrument_id,
                    ClientOrderId::from(format!("O-BATCH-MODIFY-{index}")),
                    Some(VenueOrderId::from(format!("BYBIT-BATCH-MODIFY-{index}"))),
                    Some(Quantity::from("1")),
                    Some(Price::from("10001.00")),
                    None,
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )
            })
            .collect::<Vec<_>>();

        client
            .batch_modify_orders(BatchModifyOrders::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from("S-001"),
                instrument_id,
                modifies.clone(),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        let mut actual = Vec::new();

        for _ in &modifies {
            let event = rx.try_recv().expect("expected OrderModifyDenied event");
            match event {
                ExecutionEvent::Order(OrderEventAny::ModifyDenied(denied)) => {
                    actual.push((
                        denied.client_order_id,
                        denied.strategy_id,
                        denied.venue_order_id,
                        denied.causation_id,
                    ));
                }
                event => panic!("expected OrderModifyDenied, was {event:?}"),
            }
        }

        let mut expected: Vec<_> = modifies
            .iter()
            .map(|modify| {
                (
                    modify.client_order_id,
                    modify.strategy_id,
                    modify.venue_order_id,
                    Some(modify.command_id),
                )
            })
            .collect();

        actual.sort_by_key(|entry| entry.0);
        expected.sort_by_key(|entry| entry.0);

        assert_eq!(actual, expected);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_batch_modify_orders_denies_invalid_order_iv_and_unsent_valid_sibling() {
        let (mut client, _cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);
        client.ws_trade.close().await.unwrap();

        let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
        let invalid_params =
            Params::new([("order_iv".to_string(), serde_json::json!({ "bad": true }))].into());

        let invalid = ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-INVALID"),
            instrument_id,
            ClientOrderId::from("O-BATCH-IV-INVALID"),
            Some(VenueOrderId::from("BYBIT-IV-INVALID")),
            Some(Quantity::from("1")),
            Some(Price::from("10001.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            Some(invalid_params),
            None,
        );

        let valid = ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*BYBIT_CLIENT_ID),
            StrategyId::from("S-VALID"),
            instrument_id,
            ClientOrderId::from("O-BATCH-IV-VALID"),
            Some(VenueOrderId::from("BYBIT-IV-VALID")),
            Some(Quantity::from("2")),
            Some(Price::from("10002.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        client
            .batch_modify_orders(BatchModifyOrders::new(
                TraderId::from("TESTER-001"),
                Some(*BYBIT_CLIENT_ID),
                StrategyId::from("S-BATCH"),
                instrument_id,
                vec![invalid.clone(), valid.clone()],
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_for_spawned_tasks(&client).await;

        for modify in [&invalid, &valid] {
            let event = rx.try_recv().expect("expected OrderModifyDenied event");

            let ExecutionEvent::Order(OrderEventAny::ModifyDenied(denied)) = event else {
                panic!("expected OrderModifyDenied, was {event:?}");
            };

            assert_eq!(denied.client_order_id, modify.client_order_id);
            assert_eq!(denied.strategy_id, modify.strategy_id);
            assert_eq!(denied.venue_order_id, modify.venue_order_id);
            assert_eq!(denied.causation_id, Some(modify.command_id));
        }

        assert!(rx.try_recv().is_err());
    }
}
