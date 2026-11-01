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

//! Live execution client implementation for the Deribit adapter.

use std::{future::Future, io, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    cache::InstrumentLookupError,
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
    DurationNanos, Params, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, SocketControl,
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderType, PositionSide, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Quantity},
};
use serde::Serialize;

use crate::{
    common::{
        capabilities::validate_order,
        consts::{DERIBIT_VENUE, DERIBIT_WS_HEARTBEAT_SECS},
        enums::resolve_trigger_type,
    },
    config::DeribitExecutionClientConfig,
    http::{
        client::DeribitHttpClient,
        error::DeribitHttpError,
        models::DeribitCurrency,
        query::GetOrderStateParams,
        reconciliation::{ReconciliationScope, ReportRead, historical_miss_message},
    },
    websocket::{
        auth::DERIBIT_EXECUTION_SESSION_NAME,
        client::DeribitWebSocketClient,
        messages::{DeribitEditParams, DeribitOrderParams, NautilusWsMessage},
        parse::parse_user_order_msg,
    },
};

/// Deribit live execution client.
#[derive(Debug)]
pub struct DeribitExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: DeribitExecutionClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: DeribitHttpClient,
    ws_client: DeribitWebSocketClient,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
}

impl DeribitExecutionClient {
    /// Creates a new [`DeribitExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(
        core: ExecutionClientCore,
        config: DeribitExecutionClientConfig,
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
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let http_client = if config.has_api_credentials() {
            DeribitHttpClient::new_with_env(
                api_key.clone(),
                api_secret.clone(),
                config.base_url_http.clone(),
                config.environment,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                proxy_url.clone(),
            )?
        } else {
            DeribitHttpClient::new(
                config.base_url_http.clone(),
                config.environment,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                proxy_url.clone(),
            )?
        };

        let mut ws_client = DeribitWebSocketClient::new(
            config.base_url_ws.clone(),
            api_key,
            api_secret,
            DERIBIT_WS_HEARTBEAT_SECS,
            config.auth_timeout_secs,
            config.environment,
            config.transport_backend,
            proxy_url,
        )
        .context("failed to create WebSocket client for execution")?
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*DERIBIT_VENUE),
            "deribit-user-streams",
        ));
        // Set account ID for order/fill reports
        ws_client.set_account_id(core.account_id);

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Margin,
            None,
        );

        let session_tasks = TaskGroup::new();
        let pending_tasks = TaskGroup::new();

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_client,
            session_tasks,
            pending_tasks,
        })
    }

    /// Spawns an async task for execution operations.
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
            log::warn!("Skipping Deribit {description} after shutdown began: {e}");
        }
    }

    /// Aborts all pending async tasks.
    fn abort_pending_tasks(&self) {
        self.pending_tasks.begin_shutdown();
    }

    fn abort_session_tasks(&self) {
        self.session_tasks.begin_shutdown();
        self.ws_client.begin_shutdown();
    }

    async fn await_pending_tasks(&self) -> anyhow::Result<()> {
        self.pending_tasks.begin_shutdown();
        self.pending_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Deribit execution tasks: {e}"))?;
        Ok(())
    }

    async fn await_session_tasks(&self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.session_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Deribit session tasks: {e}"))?;
        Ok(())
    }

    async fn teardown_partial_connect(&self) -> anyhow::Result<()> {
        self.abort_session_tasks();
        self.abort_pending_tasks();

        let mut errors = Vec::new();
        if let Err(e) = self.ws_client.close().await {
            errors.push(format!("WebSocket shutdown failed: {e}"));
        }
        let (session_result, pending_result) =
            tokio::join!(self.await_session_tasks(), self.await_pending_tasks());

        if let Err(e) = session_result {
            errors.push(e.to_string());
        }

        if let Err(e) = pending_result {
            errors.push(e.to_string());
        }
        self.core.set_disconnected();

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(errors.join("; "))
        }
    }

    // Rejects unsupported order types, time-in-force values, and params without exact JSON numbers
    fn build_order_params(&self, order: &dyn Order) -> anyhow::Result<DeribitOrderParams> {
        let instrument_id = order.instrument_id();
        let instrument = self
            .http_client
            .get_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;
        validate_order(&instrument, order)?;

        let order_type = match order.order_type() {
            OrderType::Limit => "limit",
            OrderType::Market => "market",
            OrderType::StopLimit => "stop_limit",
            OrderType::StopMarket => "stop_market",
            OrderType::LimitIfTouched => "take_limit",
            OrderType::MarketIfTouched => "take_market",
            other => {
                anyhow::bail!("Unsupported order type {other:?} for Deribit");
            }
        }
        .to_string();

        let time_in_force = if matches!(
            order.order_type(),
            OrderType::Market | OrderType::StopMarket | OrderType::MarketIfTouched
        ) {
            // Deribit rejects `time_in_force` on market-style order types
            None
        } else {
            Some(
                match order.time_in_force() {
                    TimeInForce::Gtc => "good_til_cancelled",
                    TimeInForce::Ioc => "immediate_or_cancel",
                    TimeInForce::Fok => "fill_or_kill",
                    TimeInForce::Gtd => {
                        if order.expire_time().is_some() {
                            log::warn!(
                                "Deribit GTD orders expire at 8:00 UTC only - custom expire_time is ignored. \
                                For custom expiry times, use managed GTD with emulation_trigger"
                            );
                        }
                        "good_til_day"
                    }
                    other => {
                        anyhow::bail!("Unsupported time_in_force {other:?} for Deribit");
                    }
                }
                .to_string(),
            )
        };

        // Deribit's `valid_until` is a REQUEST timeout, not order expiry.
        // Deribit's `good_til_day` expires at end of trading session (8 UTC).
        let valid_until = None;

        let trigger = resolve_trigger_type(order.trigger_type());

        let params = DeribitOrderParams {
            instrument_name: order.instrument_id().symbol.to_string(),
            amount: order.quantity().as_decimal(),
            order_type,
            label: Some(order.client_order_id().to_string()),
            price: order.price().map(|p| p.as_decimal()),
            time_in_force,
            post_only: if order.is_post_only() {
                Some(true)
            } else {
                None
            },
            reject_post_only: if order.is_post_only() {
                Some(true)
            } else {
                None
            },
            reduce_only: if order.is_reduce_only() {
                Some(true)
            } else {
                None
            },
            trigger_price: order.trigger_price().map(|p| p.as_decimal()),
            trigger,
            max_show: None,
            valid_until,
        };

        check_params_serialize(&params)?;

        Ok(params)
    }

    fn spawn_order_submit(
        &self,
        order: &OrderAny,
        params: DeribitOrderParams,
        task_name: &'static str,
    ) {
        let client_order_id = order.client_order_id();
        let trader_id = order.trader_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();

        log::debug!("OrderSubmitted client_order_id={client_order_id}");

        let ws_client = self.ws_client.clone();
        self.spawn_task(task_name, async move {
            let result = ws_client
                .submit_order(
                    order_side,
                    params,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await;

            if let Err(e) = result {
                log::error!(
                    "Submit order request failed: task={task_name}, client_order_id={client_order_id}, error={e}"
                );
                return Err(e.into());
            }

            Ok(())
        });
    }

    /// Spawns a stream handler to dispatch WebSocket messages to the execution engine.
    fn spawn_stream_handler(
        &self,
        stream: impl futures_util::Stream<Item = NautilusWsMessage> + Send + 'static,
    ) -> anyhow::Result<()> {
        let emitter = self.emitter.clone();

        self.session_tasks.spawn(async move {
            pin_mut!(stream);
            while let Some(message) = stream.next().await {
                dispatch_ws_message(message, &emitter);
            }
        })?;

        log::debug!("WebSocket stream handler started");
        Ok(())
    }

    async fn load_execution_instruments(&self) -> anyhow::Result<()> {
        if self.core.instruments_initialized() {
            return Ok(());
        }

        for product_type in &self.config.product_types {
            let instruments = self
                .http_client
                .request_instruments(DeribitCurrency::ANY, Some(*product_type))
                .await
                .with_context(|| format!("failed to request instruments for {product_type:?}"))?;

            if instruments.is_empty() {
                log::warn!("No instruments returned for {product_type:?}");
                continue;
            }

            log::debug!("Fetched {} {product_type:?} instruments", instruments.len());
            self.ws_client.cache_instruments(&instruments);
            self.http_client.cache_instruments(&instruments);
        }

        let required = self
            .http_client
            .request_uncached_instruments(self.config.load_ids.as_deref().unwrap_or(&[]))
            .await
            .context("failed to load reconciliation instruments")?;

        if !required.is_empty() {
            self.ws_client.cache_instruments(&required);
            self.http_client.cache_instruments(&required);
        }

        self.core.set_instruments_initialized();
        Ok(())
    }

    fn reconciliation_scope(&self) -> ReconciliationScope {
        ReconciliationScope::from_load_ids(self.config.load_ids.as_deref())
    }

    fn reports_or_historical_error<T>(read: ReportRead<T>) -> anyhow::Result<Vec<T>> {
        if read.historical_incomplete() {
            anyhow::bail!(historical_miss_message(&read.historical_misses));
        }

        Ok(read.reports)
    }
}

#[async_trait(?Send)]
impl ExecutionClient for DeribitExecutionClient {
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
        *DERIBIT_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
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

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, product_types={:?}, environment={}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.product_types,
            self.config.environment
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
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() && self.pending_tasks.is_open() && self.session_tasks.is_open()
        {
            return Ok(());
        }

        if !self.pending_tasks.is_open() {
            self.await_pending_tasks().await?;
            self.pending_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Deribit task generation: {e}"))?;
        }

        if !self.session_tasks.is_open() || !self.session_tasks.is_empty() {
            self.abort_session_tasks();

            if self.ws_client.is_active() {
                self.ws_client
                    .close()
                    .await
                    .context("failed to close stale Deribit WebSocket")?;
            }
            self.await_session_tasks().await?;
            self.session_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Deribit session generation: {e}"))?;
        } else if self.ws_client.is_active() {
            self.ws_client
                .close()
                .await
                .context("failed to close stale Deribit WebSocket")?;
        }
        let ws_client = self.ws_client.clone();
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {
                ws_client.begin_shutdown();
            });

        // Check if credentials are available before requesting account state
        if !self.config.has_api_credentials() {
            anyhow::bail!("Missing API credentials; set Deribit environment variables");
        }

        // Set account ID for order/fill reports
        self.ws_client.set_account_id(self.core.account_id);

        self.load_execution_instruments().await?;

        // Fetch initial account state
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request account state")?;

        self.emitter.send_account_state(account_state);

        let session_result = async {
            self.ws_client
                .connect()
                .await
                .context("failed to connect WebSocket client for execution")?;

            self.ws_client
                .authenticate_session(DERIBIT_EXECUTION_SESSION_NAME)
                .await
                .map_err(|e| anyhow::anyhow!("failed to authenticate WebSocket session: {e}"))?;

            log::debug!("WebSocket client authenticated for execution");

            // Subscribe to user order and trade updates for all instruments
            self.ws_client
                .subscribe_user_orders()
                .await
                .map_err(|e| anyhow::anyhow!("failed to subscribe to user orders: {e}"))?;
            self.ws_client
                .subscribe_user_trades()
                .await
                .map_err(|e| anyhow::anyhow!("failed to subscribe to user trades: {e}"))?;
            self.ws_client
                .subscribe_user_portfolio()
                .await
                .map_err(|e| anyhow::anyhow!("failed to subscribe to user portfolio: {e}"))?;

            if let Err(e) = self.ws_client.wait_for_subscriptions_confirmed(30.0).await {
                // Roll back subscription state so a retry re-sends subscribe requests
                let _ = self.ws_client.unsubscribe_user_orders().await;
                let _ = self.ws_client.unsubscribe_user_trades().await;
                let _ = self.ws_client.unsubscribe_user_portfolio().await;
                anyhow::bail!("subscription confirmation failed: {e}");
            }

            log::debug!("Subscribed to user order, trade, and portfolio updates");

            // Spawn stream handler to dispatch WebSocket messages to the execution engine
            let stream = self.ws_client.stream()?;
            self.spawn_stream_handler(stream)?;

            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Deribit execution startup teardown failed: {teardown_error}"
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
        self.teardown_partial_connect().await?;
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let report = if let Some(venue_order_id) = &cmd.venue_order_id {
            let params = GetOrderStateParams {
                order_id: venue_order_id.to_string(),
            };
            let ts_init = self.clock.get_time_ns();

            let response = match self.http_client.inner.get_order_state(params).await {
                Ok(response) => response,
                Err(DeribitHttpError::DeribitError {
                    error_code: 10004, ..
                }) => return Ok(None),
                Err(e) => return Err(e.into()),
            };

            let order = response.result.context("missing order state result")?;
            let instrument_id = InstrumentId::new(order.instrument_name.into(), *DERIBIT_VENUE);
            let instrument = self
                .http_client
                .get_instrument(&order.instrument_name)
                .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

            parse_user_order_msg(&order, &instrument, self.core.account_id, ts_init)?
        } else if let Some(client_order_id) = &cmd.client_order_id {
            let reports = self
                .http_client
                .request_order_status_reports_in_scope(
                    self.core.account_id,
                    cmd.instrument_id,
                    None,
                    None,
                    false, // Search all orders, not just open
                    self.reconciliation_scope(),
                )
                .await?
                .reports;

            // Bulk reports can omit orders outside the cached products or venue history window
            reports
                .into_iter()
                .find(|report| report.client_order_id == Some(*client_order_id))
                .context("order lookup by client order ID was inconclusive")?
        } else {
            anyhow::bail!("Order lookup requires a client order ID or venue order ID");
        };

        // External orders can lack a venue label after Nautilus assigns a client order ID
        anyhow::ensure!(
            cmd.instrument_id
                .is_none_or(|id| report.instrument_id == id)
                && report
                    .client_order_id
                    .zip(cmd.client_order_id)
                    .is_none_or(|(reported, queried)| reported == queried)
                && cmd
                    .venue_order_id
                    .is_none_or(|id| report.venue_order_id == id),
            "Order status report identity does not match query: instrument_id={}, client_order_id={:?}, venue_order_id={}",
            report.instrument_id,
            report.client_order_id,
            report.venue_order_id,
        );

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let read = self
            .http_client
            .request_order_status_reports_in_scope(
                self.core.account_id,
                cmd.instrument_id,
                cmd.start,
                cmd.end,
                cmd.open_only,
                self.reconciliation_scope(),
            )
            .await?;
        Self::reports_or_historical_error(read)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let read = self
            .http_client
            .request_fill_reports_scoped(
                self.core.account_id,
                cmd.instrument_id,
                cmd.start,
                cmd.end,
                cmd.venue_order_id.as_ref(),
                self.reconciliation_scope(),
            )
            .await?;
        Self::reports_or_historical_error(read)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        Ok(self
            .http_client
            .request_position_status_reports_in_scope(
                self.core.account_id,
                cmd.instrument_id,
                self.reconciliation_scope(),
            )
            .await?
            .reports)
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
            .map(|lookback| ts_now.saturating_sub(lookback));

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false) // get all orders for mass status
            .start(start)
            .build()
            .context("Failed to build GenerateOrderStatusReports")?;

        let fill_cmd = GenerateFillReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .context("Failed to build GenerateFillReports")?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .context("Failed to build GeneratePositionStatusReports")?;

        let scope = self.reconciliation_scope();
        let (order_read, fill_read, position_read) = tokio::try_join!(
            self.http_client.request_order_status_reports_in_scope(
                self.core.account_id,
                order_cmd.instrument_id,
                order_cmd.start,
                order_cmd.end,
                order_cmd.open_only,
                scope.clone(),
            ),
            self.http_client.request_fill_reports_scoped(
                self.core.account_id,
                fill_cmd.instrument_id,
                fill_cmd.start,
                fill_cmd.end,
                fill_cmd.venue_order_id.as_ref(),
                scope.clone(),
            ),
            self.http_client.request_position_status_reports_in_scope(
                self.core.account_id,
                position_cmd.instrument_id,
                scope,
            ),
        )?;
        let historical_incomplete = order_read.historical_incomplete()
            || fill_read.historical_incomplete()
            || position_read.historical_incomplete();
        let order_reports = order_read.reports;
        let fill_reports = fill_read.reports;
        let position_reports = position_read.reports;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} FillReports", fill_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *DERIBIT_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        if start.is_some() && historical_incomplete {
            add_flat_reports_without_position(
                &mut mass_status,
                &self.http_client,
                self.core.account_id,
                ts_now,
            );
            mass_status.set_report_window(start, false);
        }

        Ok(Some(mass_status))
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(account_id)
                .await
                .context("failed to query account state (check API credentials are valid)")?;

            emitter.send_account_state(account_state);
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();

        // Extract venue order ID (Deribit's order_id)
        let order_id = cmd
            .venue_order_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("venue_order_id required for query_order"))?
            .to_string();

        let client_order_id = cmd.client_order_id;
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;

        log::debug!("Querying order state: order_id={order_id}, client_order_id={client_order_id}");

        // Spawn async task to query order state via WebSocket
        // Response will be dispatched through the WebSocket stream handler as OrderStatusReport
        self.spawn_task("query_order", async move {
            ws_client
                .query_order(
                    &order_id,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Query order state failed: {e}"))?;
            Ok(())
        });

        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let params = self.build_order_params(&order)?;
        self.emitter.try_emit_order_submitted(&order)?;
        self.spawn_order_submit(&order, params, "submit_order");
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        if cmd.order_list.client_order_ids.is_empty() {
            log::debug!("submit_order_list called with empty order list");
            return Ok(());
        }

        let mut orders = self.core.get_orders_for_list(&cmd.order_list)?;
        orders.retain(|order| {
            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                false
            } else {
                true
            }
        });

        if orders.is_empty() {
            return Ok(());
        }

        let params = orders
            .iter()
            .map(|order| self.build_order_params(order))
            .collect::<anyhow::Result<Vec<_>>>()?;

        for order in &orders {
            self.emitter.try_emit_order_submitted(order)?;
        }

        log::debug!(
            "Submitting order list {} with {} orders for instrument={}",
            cmd.order_list.id,
            orders.len(),
            cmd.instrument_id
        );

        for (order, params) in orders.iter().zip(params) {
            self.spawn_order_submit(order, params, "submit_order_list_item");
        }

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();
        let cached_order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .map(|order| order.clone());

        // Extract venue order ID (Deribit's order_id)
        let order_id = match cmd.venue_order_id.as_ref() {
            Some(venue_order_id) => venue_order_id.to_string(),
            None => {
                return deny_modify_command(
                    &self.emitter,
                    self.clock,
                    &cmd,
                    "venue_order_id required for modify_order",
                    cached_order.as_ref(),
                );
            }
        };

        // Extract quantity - if not provided, get from order in cache
        let quantity = if let Some(qty) = cmd.quantity {
            qty
        } else {
            // Get order from cache to use its current quantity
            let cache = self.core.cache();
            match cache.order(&cmd.client_order_id) {
                Some(order) => order.quantity(),
                None => {
                    return deny_modify_command(
                        &self.emitter,
                        self.clock,
                        &cmd,
                        &format!("Order not found: {}", cmd.client_order_id),
                        cached_order.as_ref(),
                    );
                }
            }
        };

        let price = match cmd.price {
            Some(price) => price,
            None => {
                return deny_modify_command(
                    &self.emitter,
                    self.clock,
                    &cmd,
                    "price required for modify_order",
                    cached_order.as_ref(),
                );
            }
        };

        let client_order_id = cmd.client_order_id;
        let trader_id = cached_order
            .as_ref()
            .map_or(cmd.trader_id, |order| order.trader_id());
        let strategy_id = cached_order
            .as_ref()
            .map_or(cmd.strategy_id, |order| order.strategy_id());
        let instrument_id = cached_order
            .as_ref()
            .map_or(cmd.instrument_id, |order| order.instrument_id());

        log::debug!(
            "Modifying order: order_id={order_id}, quantity={quantity}, price={price}, client_order_id={client_order_id}"
        );

        let params = DeribitEditParams {
            order_id: order_id.clone(),
            amount: quantity.as_decimal(),
            price: Some(price.as_decimal()),
            post_only: None,
            reject_post_only: None,
            reduce_only: None,
            trigger_price: None,
        };

        if let Err(e) = check_params_serialize(&params) {
            return deny_modify_command(
                &self.emitter,
                self.clock,
                &cmd,
                &e.to_string(),
                cached_order.as_ref(),
            );
        }

        let emitter = self.emitter.clone();
        let clock = self.clock;
        // Spawn async task to send modify via WebSocket
        self.spawn_task("modify_order", async move {
            if let Err(e) = ws_client
                .modify_order_with_causation(
                    params,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    Some(cmd.command_id),
                )
                .await
            {
                log::error!(
                    "Modify order failed: order_id={order_id}, client_order_id={client_order_id}, error={e}"
                );
                return deny_modify_command(
                    &emitter,
                    clock,
                    &cmd,
                    &e.to_string(),
                    cached_order.as_ref(),
                );
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();
        let cached_order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .map(|order| order.clone());

        // Extract venue order ID (Deribit's order_id)
        let order_id = match cmd.venue_order_id.as_ref() {
            Some(venue_order_id) => venue_order_id.to_string(),
            None => {
                deny_cancel_command(
                    &self.emitter,
                    self.clock,
                    &cmd,
                    cached_order.as_ref(),
                    "venue_order_id required for cancel_order",
                );
                return Ok(());
            }
        };

        let client_order_id = cmd.client_order_id;
        let trader_id = cached_order
            .as_ref()
            .map_or(cmd.trader_id, |order| order.trader_id());
        let strategy_id = cached_order
            .as_ref()
            .map_or(cmd.strategy_id, |order| order.strategy_id());
        let instrument_id = cached_order
            .as_ref()
            .map_or(cmd.instrument_id, |order| order.instrument_id());

        log::debug!("Canceling order: order_id={order_id}, client_order_id={client_order_id}");

        let emitter = self.emitter.clone();
        let clock = self.clock;
        // Spawn async task to send cancel via WebSocket
        self.spawn_task("cancel_order", async move {
            if let Err(e) = ws_client
                .cancel_order_with_causation(
                    &order_id,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                    Some(cmd.command_id),
                )
                .await
            {
                log::error!(
                    "Cancel order failed: order_id={order_id}, client_order_id={client_order_id}, error={e}"
                );
                deny_cancel_command(&emitter, clock, &cmd, cached_order.as_ref(), &e.to_string());
                anyhow::bail!("Cancel order failed: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Without a side filter, use efficient bulk cancel via Deribit API
        let Some(order_side) = cmd.order_side else {
            log::debug!(
                "Cancelling all orders: instrument={instrument_id}, order_side=None (bulk)"
            );

            let ws_client = self.ws_client.clone();
            self.spawn_task("cancel_all_orders", async move {
                if let Err(e) = ws_client.cancel_all_orders(instrument_id, None).await {
                    log::error!("Cancel all orders failed for instrument {instrument_id}: {e}");
                    anyhow::bail!("Cancel all orders failed: {e}");
                }
                Ok(())
            });

            return Ok(());
        };

        // For specific side (Buy/Sell), filter from cache and cancel individually
        // Deribit API doesn't support side filtering, so we implement it locally
        log::debug!(
            "Cancelling orders by side: instrument={instrument_id}, order_side={order_side}"
        );

        let open_orders: Vec<_> = {
            let cache = self.core.cache();
            let open_orders = cache.orders_open(None, Some(&instrument_id), None, None, None);

            open_orders
                .into_iter()
                .filter(|order| order.order_side() == order_side)
                .map(|order| order.clone())
                .collect()
        };

        let orders_to_cancel: Vec<_> = open_orders
            .into_iter()
            .filter_map(|order| {
                let Some(venue_order_id) = order.venue_order_id() else {
                    self.emitter.emit_order_cancel_denied(
                        &order,
                        "venue_order_id required for cancel_order",
                        self.clock.get_time_ns(),
                        Some(cmd.command_id),
                    );
                    return None;
                };

                Some((
                    venue_order_id.to_string(),
                    order.client_order_id(),
                    order.instrument_id(),
                    Some(venue_order_id),
                    order.trader_id(),
                    order.strategy_id(),
                ))
            })
            .collect();

        if orders_to_cancel.is_empty() {
            log::debug!("No open {order_side} orders to cancel for {instrument_id}");
            return Ok(());
        }

        log::debug!(
            "Cancelling {} {order_side} orders for {instrument_id}",
            orders_to_cancel.len(),
        );

        // Cancel each matching order individually
        for (
            venue_order_id_str,
            client_order_id,
            order_instrument_id,
            venue_order_id,
            trader_id,
            strategy_id,
        ) in orders_to_cancel
        {
            let ws_client = self.ws_client.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;
            let cause = cmd.command_id;

            self.spawn_task("cancel_order_by_side", async move {
                if let Err(e) = ws_client
                    .cancel_order_with_causation(
                        &venue_order_id_str,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        order_instrument_id,
                        Some(cause),
                    )
                    .await
                {
                    emitter.emit_order_cancel_denied_event(
                        strategy_id,
                        order_instrument_id,
                        client_order_id,
                        venue_order_id,
                        &e.to_string(),
                        clock.get_time_ns(),
                        Some(cause),
                    );
                }
                Ok(())
            });
        }

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            log::debug!("batch_cancel_orders called with empty cancels list");
            return Ok(());
        }

        log::debug!(
            "Batch cancelling {} orders for instrument={}",
            cmd.cancels.len(),
            cmd.instrument_id
        );

        // Deribit doesn't have native batch cancel by order ID
        // Loop through and cancel each order individually
        for cancel in &cmd.cancels {
            let cached_order = self
                .core
                .cache()
                .order(&cancel.client_order_id)
                .map(|order| order.clone());

            let order_id = match &cancel.venue_order_id {
                Some(id) => id.to_string(),
                None => {
                    deny_cancel_command(
                        &self.emitter,
                        self.clock,
                        cancel,
                        cached_order.as_ref(),
                        "venue_order_id required for cancel_order",
                    );
                    continue;
                }
            };

            let emitter = self.emitter.clone();
            let clock = self.clock;
            let command = cancel.clone();
            let ws_client = self.ws_client.clone();
            let client_order_id = cancel.client_order_id;
            let trader_id = cached_order
                .as_ref()
                .map_or(cancel.trader_id, |order| order.trader_id());
            let strategy_id = cached_order
                .as_ref()
                .map_or(cancel.strategy_id, |order| order.strategy_id());
            let instrument_id = cached_order
                .as_ref()
                .map_or(cancel.instrument_id, |order| order.instrument_id());

            self.spawn_task("batch_cancel_order", async move {
                if let Err(e) = ws_client
                    .cancel_order_with_causation(
                        &order_id,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                        Some(command.command_id),
                    )
                    .await
                {
                    log::error!(
                        "Batch cancel order failed: order_id={order_id}, client_order_id={client_order_id}, error={e}"
                    );
                    deny_cancel_command(
                        &emitter,
                        clock,
                        &command,
                        cached_order.as_ref(),
                        &e.to_string(),
                    );
                    anyhow::bail!("Batch cancel order failed: {e}");
                }
                Ok(())
            });
        }

        Ok(())
    }
}

fn add_flat_reports_without_position(
    mass_status: &mut ExecutionMassStatus,
    http_client: &DeribitHttpClient,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let reported: Vec<InstrumentId> = mass_status.position_reports().keys().copied().collect();
    let mut missing = Vec::new();

    for report in mass_status.order_reports().values() {
        if position_snapshot_covers(http_client, report.instrument_id)
            && !reported.contains(&report.instrument_id)
            && !missing.contains(&report.instrument_id)
        {
            missing.push(report.instrument_id);
        }
    }

    for fills in mass_status.fill_reports().values() {
        let Some(fill) = fills.first() else {
            continue;
        };

        if position_snapshot_covers(http_client, fill.instrument_id)
            && !reported.contains(&fill.instrument_id)
            && !missing.contains(&fill.instrument_id)
        {
            missing.push(fill.instrument_id);
        }
    }

    missing.sort();
    if missing.is_empty() {
        return;
    }

    let flats = missing
        .into_iter()
        .map(|instrument_id| {
            PositionStatusReport::new(
                account_id,
                instrument_id,
                PositionSide::Flat,
                Quantity::from("0"),
                ts_init,
                ts_init,
                None,
                None,
                None,
            )
        })
        .collect();

    mass_status.add_position_reports(flats);
}

fn position_snapshot_covers(http_client: &DeribitHttpClient, instrument_id: InstrumentId) -> bool {
    http_client
        .get_instrument(&instrument_id.symbol.inner())
        .is_some_and(|instrument| !matches!(instrument, InstrumentAny::CurrencyPair(_)))
}

/// Dispatches a WebSocket message using the event emitter.
fn dispatch_ws_message(message: NautilusWsMessage, emitter: &ExecutionEventEmitter) {
    match message {
        NautilusWsMessage::AccountState(state) => {
            emitter.send_account_state(state);
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
        NautilusWsMessage::OrderFilled(event) => {
            emitter.send_order_event(OrderEventAny::Filled(event));
        }
        NautilusWsMessage::OrderRejected(event) => {
            emitter.send_order_event(OrderEventAny::Rejected(event));
        }
        NautilusWsMessage::OrderAccepted(event) => {
            emitter.send_order_event(OrderEventAny::Accepted(event));
        }
        NautilusWsMessage::OrderCanceled(event) => {
            emitter.send_order_event(OrderEventAny::Canceled(event));
        }
        NautilusWsMessage::OrderExpired(event) => {
            emitter.send_order_event(OrderEventAny::Expired(event));
        }
        NautilusWsMessage::OrderUpdated(event) => {
            emitter.send_order_event(OrderEventAny::Updated(event));
        }
        NautilusWsMessage::OrderModifyDenied(event) => {
            emitter.send_order_event(OrderEventAny::ModifyDenied(event));
        }
        NautilusWsMessage::OrderCancelDenied(event) => {
            emitter.send_order_event(OrderEventAny::CancelDenied(event));
        }
        NautilusWsMessage::OrderCancelRejected(event) => {
            emitter.send_order_event(OrderEventAny::CancelRejected(event));
        }
        NautilusWsMessage::OrderModifyRejected(event) => {
            emitter.send_order_event(OrderEventAny::ModifyRejected(event));
        }
        NautilusWsMessage::Error(e) => {
            log::warn!("WebSocket error: {e}");
        }
        NautilusWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        NautilusWsMessage::Authenticated(auth) => {
            log::debug!("WebSocket authenticated: scope={}", auth.scope);
        }
        NautilusWsMessage::AuthenticationFailed(reason) => {
            log::error!("Authentication failed in execution client: {reason}");
        }
        NautilusWsMessage::Data(_)
        | NautilusWsMessage::Deltas(_)
        | NautilusWsMessage::Instrument(_)
        | NautilusWsMessage::InstrumentStatus(_)
        | NautilusWsMessage::FundingRates(_)
        | NautilusWsMessage::OptionGreeks(_)
        | NautilusWsMessage::Raw(_) => {
            // Data messages are handled by the data client, not execution
            log::trace!("Ignoring data message in execution client");
        }
    }
}

fn deny_modify_command(
    emitter: &ExecutionEventEmitter,
    clock: &AtomicTime,
    cmd: &ModifyOrder,
    reason: &str,
    cached_order: Option<&OrderAny>,
) -> anyhow::Result<()> {
    let ts_event = clock.get_time_ns();
    emitter.emit_order_modify_denied_event(
        cached_order.map_or(cmd.strategy_id, Order::strategy_id),
        cached_order.map_or(cmd.instrument_id, Order::instrument_id),
        cmd.client_order_id,
        cached_order
            .and_then(Order::venue_order_id)
            .or(cmd.venue_order_id),
        reason,
        ts_event,
        Some(cmd.command_id),
    );

    anyhow::bail!("{reason}");
}

fn deny_cancel_command(
    emitter: &ExecutionEventEmitter,
    clock: &AtomicTime,
    cmd: &CancelOrder,
    cached_order: Option<&OrderAny>,
    reason: &str,
) {
    let ts_event = clock.get_time_ns();
    emitter.emit_order_cancel_denied_event(
        cached_order.map_or(cmd.strategy_id, Order::strategy_id),
        cached_order.map_or(cmd.instrument_id, Order::instrument_id),
        cmd.client_order_id,
        cached_order
            .and_then(Order::venue_order_id)
            .or(cmd.venue_order_id),
        reason,
        ts_event,
        Some(cmd.command_id),
    );
}

// Serializes the params as the handler will, so a decimal without an exact JSON number is rejected
// locally instead of failing after the order is submitted.
fn check_params_serialize<T: Serialize>(params: &T) -> anyhow::Result<()> {
    serde_json::to_writer(io::sink(), params)
        .map_err(|e| anyhow::anyhow!("Cannot serialize order params: {e}"))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        messages::{ExecutionEvent, execution::ExecutionReport},
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, TriggerType},
        events::{OrderDenied, OrderFilled},
        identifiers::{
            ClientOrderId, InstrumentId, OrderListId, StrategyId, TradeId, TraderId, VenueOrderId,
        },
        instruments::InstrumentAny,
        orders::{OrderList, OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{parse::parse_deribit_instrument_any, testing::routed_spot_instrument},
        http::models::{DeribitInstrument, DeribitJsonRpcResponse},
    };

    #[rstest]
    #[case(OrderType::Limit, TimeInForce::Gtc, true)]
    #[case(OrderType::Limit, TimeInForce::Ioc, false)]
    #[case(OrderType::Limit, TimeInForce::Fok, false)]
    #[case(OrderType::Market, TimeInForce::Ioc, false)]
    #[case(OrderType::StopLimit, TimeInForce::Gtc, false)]
    fn routed_spot_supported_order_params(
        #[case] order_type: OrderType,
        #[case] tif: TimeInForce,
        #[case] post_only: bool,
    ) {
        let (client, _, _) = submission_test_rig(TimeInForce::Gtc);
        let instrument = routed_spot_instrument();
        client.http_client.cache_instruments(&[instrument]);
        let order = OrderTestBuilder::new(order_type)
            .instrument_id(InstrumentId::from("BTC_USDC.DERIBIT"))
            .quantity(Quantity::from("0.0042"))
            .price(Price::from("45000.25"))
            .trigger_price(Price::from("46000.50"))
            .trigger_type(TriggerType::LastPrice)
            .time_in_force(tif)
            .post_only(post_only)
            .build();
        let params = client.build_order_params(&order).unwrap();

        assert_eq!(params.instrument_name, "BTC_USDC");
        assert_eq!(
            params.amount,
            rust_decimal::Decimal::from_str_exact("0.0042").unwrap()
        );
        assert_eq!(
            params.order_type,
            match order_type {
                OrderType::Limit => "limit",
                OrderType::Market => "market",
                OrderType::StopLimit => "stop_limit",
                _ => unreachable!(),
            }
        );
        assert_eq!(params.price, order.price().map(|price| price.as_decimal()));
        assert_eq!(
            params.time_in_force.as_deref(),
            if order_type == OrderType::Market {
                None
            } else {
                Some(match tif {
                    TimeInForce::Gtc => "good_til_cancelled",
                    TimeInForce::Ioc => "immediate_or_cancel",
                    TimeInForce::Fok => "fill_or_kill",
                    _ => unreachable!(),
                })
            }
        );
        assert_eq!(params.post_only, post_only.then_some(true));
        assert_eq!(params.reject_post_only, post_only.then_some(true));
        assert_eq!(params.reduce_only, None);
        assert_eq!(params.max_show, None);
        assert_eq!(params.valid_until, None);
        assert_eq!(
            params.trigger_price,
            order.trigger_price().map(|price| price.as_decimal())
        );
        assert_eq!(
            params.trigger.as_deref(),
            order.trigger_type().map(|_| "last_price")
        );
        assert_eq!(
            params.label.as_deref(),
            Some(order.client_order_id().as_str())
        );
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn all_spot_rejects_gtd(#[case] routed: bool) {
        let (client, _, _) = submission_test_rig(TimeInForce::Gtc);
        let mut instrument = routed_spot_instrument();
        if !routed && let InstrumentAny::CurrencyPair(pair) = &mut instrument {
            pair.info = None;
        }

        client.http_client.cache_instruments(&[instrument]);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC_USDC.DERIBIT"))
            .quantity(Quantity::from("0.0042"))
            .price(Price::from("45000.25"))
            .time_in_force(TimeInForce::Gtd)
            .expire_time(UnixNanos::from(1_800_000_000_000_000_000))
            .build();

        assert_eq!(
            client.build_order_params(&order).unwrap_err().to_string(),
            "Deribit spot instruments do not support good_til_day"
        );
    }

    #[rstest]
    fn derivatives_preserve_gtd() {
        let (client, _, _) = submission_test_rig(TimeInForce::Gtc);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-PERPETUAL.DERIBIT"))
            .quantity(Quantity::from("20"))
            .price(Price::from("51000"))
            .time_in_force(TimeInForce::Gtd)
            .expire_time(UnixNanos::from(1_800_000_000_000_000_000))
            .build();
        let params = client.build_order_params(&order).unwrap();

        assert_eq!(params.time_in_force.as_deref(), Some("good_til_day"));
    }

    #[rstest]
    fn uncached_order_returns_instrument_lookup_error() {
        let (client, _, _) = submission_test_rig(TimeInForce::Gtc);
        let instrument_id = InstrumentId::from("UNCACHED.DERIBIT");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("0.0042"))
            .price(Price::from("45000.25"))
            .build();

        let error = client.build_order_params(&order).unwrap_err();

        assert_eq!(
            error.downcast_ref::<InstrumentLookupError>(),
            Some(&InstrumentLookupError::not_found(instrument_id))
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn routed_spot_rejection_precedes_submission(#[case] batch: bool) {
        let (mut client, _, cache) = submission_test_rig(TimeInForce::Gtc);
        let instrument = routed_spot_instrument();
        client.http_client.cache_instruments(&[instrument]);

        let orders = (0..2)
            .map(|index| {
                let order = OrderTestBuilder::new(OrderType::Limit)
                    .trader_id(TraderId::from("TESTER-001"))
                    .strategy_id(StrategyId::from("S-001"))
                    .instrument_id(InstrumentId::from("BTC_USDC.DERIBIT"))
                    .client_order_id(ClientOrderId::new(format!("routed-{index}")))
                    .quantity(Quantity::from("0.0042"))
                    .price(Price::from("45000.25"))
                    .reduce_only(!batch || index == 1)
                    .build();
                cache
                    .borrow_mut()
                    .add_order(order.clone(), None, None, false)
                    .unwrap();
                order
            })
            .collect::<Vec<_>>();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        client.emitter.set_sender(tx);

        let error = submit_test_orders(&client, &orders, batch).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Deribit routed spot does not support reduce_only"
        );
        assert_eq!(client.pending_tasks.len(), 0);
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );

        for order in orders {
            assert_eq!(
                cache
                    .borrow()
                    .order(&order.client_order_id())
                    .unwrap()
                    .status(),
                OrderStatus::Initialized
            );
        }
    }

    fn dispatch_test_rig() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let trader_id = TraderId::from("TRADER-001");
        let account_id = AccountId::from("DERIBIT-001");
        let mut emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            trader_id,
            account_id,
            AccountType::Margin,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    #[rstest]
    fn test_local_command_denial_preserves_owner_and_venue_id(
        #[values(false, true)] modify: bool,
        #[values(false, true)] cached_venue: bool,
    ) {
        let (emitter, mut rx) = dispatch_test_rig();
        let client_order_id = ClientOrderId::from("O-LOCAL-DENIAL-017");
        let instrument_id = InstrumentId::from("BTC-PERPETUAL.DERIBIT");
        let strategy_id = StrategyId::from("OWNER-019");
        let account_id = AccountId::from("DERIBIT-001");
        let command_venue_id = VenueOrderId::from("V-COMMAND-023");
        let cached_venue_id = VenueOrderId::from("V-CACHED-029");
        let command_id = UUID4::new();
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .quantity(Quantity::from("2"))
            .price(Price::from("50000.0"))
            .build();

        if cached_venue {
            let accepted = TestOrderEventStubs::accepted(&order, account_id, cached_venue_id);
            order.apply(accepted).unwrap();
        }

        let requester = StrategyId::from("REQUESTER-031");
        let requested_instrument = InstrumentId::from("ETH-PERPETUAL.DERIBIT");
        let clock = get_atomic_clock_realtime();

        if modify {
            let command = ModifyOrder::new(
                TraderId::from("TRADER-001"),
                None,
                requester,
                requested_instrument,
                client_order_id,
                Some(command_venue_id),
                Some(Quantity::from("3")),
                Some(Price::from("51000.0")),
                None,
                command_id,
                UnixNanos::default(),
                None,
                None,
            );
            let error = deny_modify_command(&emitter, clock, &command, "not sent", Some(&order))
                .unwrap_err();
            assert_eq!(error.to_string(), "not sent");
        } else {
            let command = CancelOrder::new(
                TraderId::from("TRADER-001"),
                None,
                requester,
                requested_instrument,
                client_order_id,
                Some(command_venue_id),
                command_id,
                UnixNanos::default(),
                None,
                None,
            );
            deny_cancel_command(&emitter, clock, &command, Some(&order), "not sent");
        }

        let event = rx.try_recv().unwrap();

        let (trader, strategy, instrument, client, venue, account, reason, cause) = match event {
            ExecutionEvent::Order(OrderEventAny::ModifyDenied(event)) if modify => (
                event.trader_id,
                event.strategy_id,
                event.instrument_id,
                event.client_order_id,
                event.venue_order_id,
                event.account_id,
                event.reason,
                event.causation_id,
            ),
            ExecutionEvent::Order(OrderEventAny::CancelDenied(event)) if !modify => (
                event.trader_id,
                event.strategy_id,
                event.instrument_id,
                event.client_order_id,
                event.venue_order_id,
                event.account_id,
                event.reason,
                event.causation_id,
            ),
            other => panic!("Expected matching local command denial, was {other:?}"),
        };

        assert_eq!(trader, TraderId::from("TRADER-001"));
        assert_eq!(strategy, strategy_id);
        assert_eq!(instrument, instrument_id);
        assert_eq!(client, client_order_id);
        assert_eq!(
            venue,
            Some(if cached_venue {
                cached_venue_id
            } else {
                command_venue_id
            })
        );
        assert_eq!(account, Some(account_id));
        assert_eq!(reason, "not sent");
        assert_eq!(cause, Some(command_id));
        assert!(rx.try_recv().is_err());
    }

    fn fill_report() -> FillReport {
        FillReport::new(
            AccountId::from("DERIBIT-001"),
            InstrumentId::from("BTC-PERPETUAL.DERIBIT"),
            VenueOrderId::from("ETH-584830574"),
            TradeId::from("ETH-2696068"),
            OrderSide::Buy,
            Quantity::from("1.000000"),
            Price::from("203.80"),
            Money::from("0.00036801 USDT"),
            LiquiditySide::Taker,
            Some(ClientOrderId::from("O-19700101-000000-001-001-1")),
            None,
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
        )
    }

    #[rstest]
    fn dispatch_tracked_fill_uses_order_event_path() {
        let (emitter, mut rx) = dispatch_test_rig();
        let report = fill_report();
        let filled = OrderFilled::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            report.instrument_id,
            report.client_order_id.unwrap(),
            report.venue_order_id,
            report.account_id,
            report.trade_id,
            report.order_side,
            OrderType::Market,
            report.last_qty,
            report.last_px,
            Currency::USDT(),
            report.liquidity_side,
            UUID4::new(),
            report.ts_event,
            report.ts_init,
            false,
            None,
            Some(report.commission),
            None,
        );

        dispatch_ws_message(NautilusWsMessage::OrderFilled(filled), &emitter);

        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Filled(event))
                if event.client_order_id == ClientOrderId::from("O-19700101-000000-001-001-1")
                    && event.trade_id == TradeId::from("ETH-2696068")
        ));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn dispatch_untracked_fill_keeps_report_path() {
        let (emitter, mut rx) = dispatch_test_rig();
        let report = fill_report();

        dispatch_ws_message(NautilusWsMessage::FillReports(vec![report]), &emitter);

        assert!(matches!(
            rx.try_recv().unwrap(),
            ExecutionEvent::Report(ExecutionReport::Fill(_))
        ));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case(false, false, "Cannot send order event: sender not initialized")]
    #[case(false, true, "Failed to send order event: channel closed")]
    #[case(true, false, "Cannot send order event: sender not initialized")]
    #[case(true, true, "Failed to send order event: channel closed")]
    #[tokio::test]
    async fn test_submit_event_failure_returns_error_before_spawning(
        #[case] batch: bool,
        #[case] closed: bool,
        #[case] expected_error: &str,
    ) {
        let (mut client, orders, _cache) = submission_test_rig(TimeInForce::Gtc);

        if closed {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
            client.emitter.set_sender(tx);
            drop(rx);
        }

        let error = submit_test_orders(&client, &orders, batch).unwrap_err();

        assert_eq!(error.to_string(), expected_error);
        assert_eq!(client.pending_tasks.len(), 0);

        for order in orders {
            let cached = client
                .core
                .cache()
                .try_order_owned(&order.client_order_id())
                .unwrap();
            assert_eq!(cached.status(), OrderStatus::Initialized);
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_submit_list_validation_failure_precedes_events_and_tasks() {
        let (mut client, orders, _cache) = submission_test_rig(TimeInForce::AtTheOpen);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        client.emitter.set_sender(tx);

        let error = submit_test_orders(&client, &orders, true).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unsupported time_in_force AtTheOpen for Deribit"
        );
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
        assert_eq!(client.pending_tasks.len(), 0);
    }

    #[rstest]
    #[case(false, false)]
    #[case(true, false)]
    #[case(true, true)]
    #[tokio::test]
    async fn test_submit_queues_expected_events(#[case] batch: bool, #[case] close_first: bool) {
        let (mut client, orders, cache) = submission_test_rig(TimeInForce::Gtc);
        if close_first {
            let order = &orders[0];

            let denied = OrderDenied::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                "Already denied".into(),
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
            );
            cache
                .borrow_mut()
                .update_order(&OrderEventAny::Denied(denied))
                .unwrap();
        }

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        client.emitter.set_sender(tx);

        submit_test_orders(&client, &orders, batch).unwrap();
        let expected = if close_first {
            &orders[1..]
        } else if batch {
            &orders[..]
        } else {
            &orders[..1]
        };

        let events = expected
            .iter()
            .map(|_| match rx.try_recv().unwrap() {
                ExecutionEvent::Order(OrderEventAny::Submitted(event)) => event,
                other => panic!("Unexpected submitted message: {other:?}"),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );

        for (event, order) in events.iter().zip(expected) {
            assert_eq!(event.trader_id, client.core.trader_id);
            assert_eq!(event.strategy_id, order.strategy_id());
            assert_eq!(event.instrument_id, order.instrument_id());
            assert_eq!(event.client_order_id, order.client_order_id());
            assert_eq!(event.account_id, client.core.account_id);
            assert_eq!(event.ts_event, event.ts_init);
            assert_eq!(event.causation_id, None);
        }
    }

    fn submission_test_rig(
        second_time_in_force: TimeInForce,
    ) -> (DeribitExecutionClient, Vec<OrderAny>, Rc<RefCell<Cache>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("DERIBIT"),
            *DERIBIT_VENUE,
            OmsType::Netting,
            AccountId::from("DERIBIT-001"),
            AccountType::Margin,
            None,
            cache.clone(),
        );

        let config = DeribitExecutionClientConfig {
            api_key: Some("test_api_key".into()),
            api_secret: Some("test_api_secret".into()),
            ..Default::default()
        };

        let client = DeribitExecutionClient::new(core, config).unwrap();
        let response: DeribitJsonRpcResponse<Vec<DeribitInstrument>> =
            serde_json::from_str(include_str!("../test_data/http_get_instruments.json")).unwrap();
        let instrument = parse_deribit_instrument_any(
            &response.result.unwrap()[0],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap()
        .unwrap();
        client.http_client.cache_instruments(&[instrument]);

        let orders = [TimeInForce::Gtc, second_time_in_force]
            .into_iter()
            .enumerate()
            .map(|(index, time_in_force)| {
                let order = OrderTestBuilder::new(OrderType::Limit)
                    .trader_id(TraderId::from("TESTER-001"))
                    .strategy_id(StrategyId::from("S-001"))
                    .instrument_id(InstrumentId::from("BTC-PERPETUAL.DERIBIT"))
                    .client_order_id(ClientOrderId::new(format!("submit-{index}")))
                    .quantity(Quantity::from(if index == 0 { "10" } else { "20" }))
                    .price(Price::from(if index == 0 { "50000" } else { "51000" }))
                    .time_in_force(time_in_force)
                    .build();
                cache
                    .borrow_mut()
                    .add_order(order.clone(), None, None, false)
                    .unwrap();
                order
            })
            .collect();

        (client, orders, cache)
    }

    fn submit_test_orders(
        client: &DeribitExecutionClient,
        orders: &[OrderAny],
        batch: bool,
    ) -> anyhow::Result<()> {
        let first = &orders[0];

        if batch {
            let order_list = OrderList::new(
                OrderListId::from("submit-list-001"),
                first.instrument_id(),
                first.strategy_id(),
                orders.iter().map(Order::client_order_id).collect(),
                UnixNanos::default(),
            );
            client.submit_order_list(SubmitOrderList::new(
                first.trader_id(),
                Some(client.client_id()),
                first.strategy_id(),
                order_list,
                orders
                    .iter()
                    .map(|order| order.init_event().clone())
                    .collect(),
                None,
                None,
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
            ))
        } else {
            client.submit_order(SubmitOrder::new(
                first.trader_id(),
                Some(client.client_id()),
                first.strategy_id(),
                first.instrument_id(),
                first.client_order_id(),
                first.init_event().clone(),
                None,
                None,
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
            ))
        }
    }
}
