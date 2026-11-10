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

//! Live execution client implementation for the Coinbase Advanced Trade adapter.

use std::{
    collections::hash_map::Entry,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    factories::OrderEventFactory,
    live::runner::get_exec_event_sender,
    messages::{
        ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
            GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder, SubmitOrderList,
        },
    },
    msgbus::{self, TypedHandler},
};
use nautilus_core::{
    DurationNanos, Params, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, SocketControl,
    execution::failure::CommandFailure,
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderStatus, OrderType, TriggerType},
    events::{OrderDeniedReason, OrderEventAny, OrderUpdated},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, TraderId,
        Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price, Quantity},
};
use nautilus_network::retry::RetryConfig;
use parking_lot::Mutex;
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::COINBASE_VENUE,
        credential::CoinbaseCredential,
        enums::{CoinbaseProductType, CoinbaseWsChannel},
    },
    config::CoinbaseExecutionClientConfig,
    http::{
        client::CoinbaseHttpClient, error::Error as CoinbaseHttpError,
        parse::parse_ws_cfm_account_state,
    },
    websocket::{
        client::CoinbaseWebSocketClient,
        handler::{NautilusWsMessage, UserOrderUpdate},
        messages::WsOrderUpdate,
    },
};

// Coinbase does not publish a formal max for batch_cancel; conservative chunk
// size mirrors the 100 used by other adapters and keeps request bodies small.
const BATCH_CANCEL_CHUNK: usize = 100;

// Coinbase spot account is ready as soon as the REST account state lands, but
// the engine registers it asynchronously; wait up to 30s for that to happen.
const ACCOUNT_REGISTERED_TIMEOUT_SECS: f64 = 30.0;

/// Live execution client for Coinbase Advanced Trade.
#[derive(Debug)]
pub struct CoinbaseExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: CoinbaseExecutionClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: CoinbaseHttpClient,
    ws_user: CoinbaseWebSocketClient,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
    shutdown_errors: Vec<String>,
    instruments_cache: Arc<AHashMap<String, InstrumentAny>>,
    fills_observed: Arc<Mutex<AHashMap<VenueOrderId, AHashSet<TradeId>>>>,
    fills_pending: Arc<Mutex<AHashMap<VenueOrderId, OrderStatusReport>>>,
    fills_published: Arc<tokio::sync::Mutex<AHashMap<VenueOrderId, OrderStatusReport>>>,
    fills_start: Option<UnixNanos>,
    lifecycle: Arc<Mutex<RecoveryLifecycle>>,
    order_event_handler: Option<TypedHandler<OrderEventAny>>,
    order_contexts: Arc<Mutex<AHashMap<String, OrderContext>>>,
    // Caches REST-derived metadata for orders this client did not submit
    // (keyed by `venue_order_id`). Populated lazily when the user-channel
    // handler encounters an unknown order whose `OrderStatusReport` would
    // otherwise lack `price` / `trigger_price` / `trigger_type` and panic
    // the engine's reconstruction path. Separate from `order_contexts`
    // because external orders may carry a `client_order_id` we never set.
    external_order_contexts: Arc<Mutex<AHashMap<String, OrderContext>>>,
}

impl CoinbaseExecutionClient {
    /// Creates a new [`CoinbaseExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials cannot be resolved or the underlying
    /// HTTP / WebSocket client cannot be constructed.
    pub fn new(
        core: ExecutionClientCore,
        config: CoinbaseExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        let credential = CoinbaseCredential::resolve(
            config.api_key.as_ref().map(|value| value.expose_secret()),
            config
                .api_secret
                .as_ref()
                .map(|value| value.expose_secret()),
        )
        .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Coinbase credentials not available; set COINBASE_API_KEY and COINBASE_API_SECRET or pass them in the config"
                    )
                })?;
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());

        let retry_config = RetryConfig {
            max_retries: config.max_retries,
            initial_delay_ms: config.retry_delay_initial_ms,
            max_delay_ms: config.retry_delay_max_ms,
            backoff_factor: 2.0,
            jitter_ms: 250,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let http_client = CoinbaseHttpClient::with_credentials(
            credential.clone(),
            config.environment,
            config.http_timeout_secs,
            proxy_url.clone(),
            Some(retry_config),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create Coinbase HTTP client: {e}"))?;

        if let Some(ref url) = config.base_url_rest {
            http_client.set_base_url(url.clone());
        }

        let ws_url = config.ws_url();
        let ws_user = CoinbaseWebSocketClient::with_credential(
            &ws_url,
            credential,
            config.transport_backend,
            proxy_url,
        )
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*COINBASE_VENUE),
            "coinbase-user-streams",
        ));

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

        let lifecycle = RecoveryLifecycle::new(core.trader_id, core.account_id, core.account_type);

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_user,
            session_tasks,
            pending_tasks,
            shutdown_errors: Vec::new(),
            instruments_cache: Arc::new(AHashMap::new()),
            fills_observed: Arc::new(Mutex::new(AHashMap::new())),
            fills_pending: Arc::new(Mutex::new(AHashMap::new())),
            fills_published: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
            fills_start: None,
            lifecycle: Arc::new(Mutex::new(lifecycle)),
            order_event_handler: None,
            order_contexts: Arc::new(Mutex::new(AHashMap::new())),
            external_order_contexts: Arc::new(Mutex::new(AHashMap::new())),
        })
    }

    fn order_recovery(&self) -> UserOrderRecovery {
        UserOrderRecovery {
            http_client: self.http_client.clone(),
            emitter: self.emitter.clone(),
            instruments: Arc::clone(&self.instruments_cache),
            observed: Arc::clone(&self.fills_observed),
            pending: Arc::clone(&self.fills_pending),
            published: Arc::clone(&self.fills_published),
            order_contexts: Arc::clone(&self.order_contexts),
            external_order_contexts: Arc::clone(&self.external_order_contexts),
            start: self.fills_start.unwrap_or_default(),
            lifecycle: Arc::clone(&self.lifecycle),
        }
    }

    fn subscribe_order_events(&mut self) {
        if self.order_event_handler.is_some() {
            return;
        }

        let core = self.core.clone();
        let lifecycle = Arc::clone(&self.lifecycle);
        let emitter = self.emitter.clone();

        let handler = TypedHandler::from(move |event: &OrderEventAny| {
            if event.instrument_id().venue != core.venue
                || matches!(event, OrderEventAny::Initialized(_))
            {
                return;
            }

            let order = core
                .cache()
                .order(&event.client_order_id())
                .map(|order| order.clone());

            if let Some(order) = order
                && order.account_id().is_none_or(|id| id == core.account_id)
            {
                let mut lifecycle = lifecycle.lock();
                lifecycle.track(order);
                if let Err(e) = lifecycle.dispatch(&emitter) {
                    log::warn!("Retaining Coinbase lifecycle recovery: {e}");
                }
            }
        });

        msgbus::subscribe_order_events("events.order.*".into(), handler.clone(), Some(10));
        self.order_event_handler = Some(handler);

        let cache = self.core.cache();
        let mut lifecycle = self.lifecycle.lock();

        for order in cache.orders_refs(Some(&self.core.venue), None, None, None, None) {
            if order
                .account_id()
                .is_none_or(|id| id == self.core.account_id)
            {
                lifecycle.track(order.clone());
            }
        }
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
            log::warn!("Skipping Coinbase {description} after shutdown began: {e}");
        }
    }

    fn abort_pending_tasks(&self) {
        self.pending_tasks.begin_shutdown();
    }

    fn abort_session_tasks(&self) {
        self.session_tasks.begin_shutdown();
        self.ws_user.begin_shutdown();
    }

    async fn await_pending_tasks(&self) -> anyhow::Result<()> {
        self.pending_tasks.begin_shutdown();
        self.pending_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Coinbase execution tasks: {e}"))?;
        Ok(())
    }

    async fn await_session_tasks(&self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.session_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Coinbase session tasks: {e}"))?;
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.abort_session_tasks();
        self.abort_pending_tasks();

        if let Err(e) = self.ws_user.disconnect().await {
            self.shutdown_errors.push(e.to_string());
        }
        let (session_result, pending_result) =
            tokio::join!(self.await_session_tasks(), self.await_pending_tasks());
        self.core.set_disconnected();

        if let Err(e) = session_result {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = pending_result {
            self.shutdown_errors.push(e.to_string());
        }

        if !self.shutdown_errors.is_empty() {
            anyhow::bail!(std::mem::take(&mut self.shutdown_errors).join("; "));
        }
        Ok(())
    }

    // Returns true when the exec client was created with a Margin account,
    // indicating it should handle CFM-backed derivatives traffic.
    fn is_margin(&self) -> bool {
        self.core.account_type == AccountType::Margin
    }

    // Returns true when the instrument resides in the connect-time bootstrap
    // cache. For the Cash (spot) factory this gates spot-only traffic; for the
    // Margin factory the cache contains CFM perp + future products.
    fn is_instrument_cached(&self, instrument_id: &InstrumentId) -> bool {
        self.instruments_cache
            .contains_key(instrument_id.symbol.as_str())
    }

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

#[async_trait(?Send)]
impl ExecutionClient for CoinbaseExecutionClient {
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
        *COINBASE_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn provides_bulk_position_coverage(&self, instrument_id: InstrumentId) -> bool {
        self.is_margin()
            && self
                .instruments_cache
                .get(instrument_id.symbol.as_str())
                .is_some_and(|instrument| instrument.id() == instrument_id)
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
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
                .map_err(|e| anyhow::anyhow!("Failed to start Coinbase task generation: {e}"))?;
        }

        if !self.session_tasks.is_open() || !self.session_tasks.is_empty() {
            self.abort_session_tasks();
            self.ws_user
                .disconnect()
                .await
                .context("failed to close stale Coinbase user WebSocket")?;
            self.await_session_tasks().await?;
            self.session_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Coinbase session generation: {e}"))?;
        }
        let ws_user = self.ws_user.clone();
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {
                ws_user.begin_shutdown();
            });

        // If the underlying WS is still alive from a prior stop() that did not
        // explicitly disconnect, tear it down before reconnecting. The
        // in-handler signal path can race with the Disconnect command, leaving
        // the inner connection_mode stale even after disconnect().await, so
        // we rebuild the client outright to guarantee clean cmd_tx/out_rx
        // pairs and a fresh signal.
        if self.ws_user.is_active() || self.ws_user.is_reconnecting() {
            log::debug!("Tearing down stale user WS before reconnect");
            self.ws_user
                .disconnect()
                .await
                .context("failed to close stale Coinbase user WebSocket")?;
            let credential = CoinbaseCredential::resolve(
                self.config
                    .api_key
                    .as_ref()
                    .map(|value| value.expose_secret()),
                self.config
                    .api_secret
                    .as_ref()
                    .map(|value| value.expose_secret()),
            )
            .ok_or_else(|| anyhow::anyhow!("Coinbase credentials unavailable for WS reset"))?;
            self.ws_user = CoinbaseWebSocketClient::with_credential(
                &self.config.ws_url(),
                credential,
                self.config.transport_backend,
                self.config
                    .proxy_url
                    .as_ref()
                    .map(|value| value.expose_secret().to_owned()),
            );
        }

        if self.core.instruments_initialized() {
            // Instruments were loaded externally; still propagate the cached
            // set to the WS client on reconnect scenarios.
            let cached: Vec<InstrumentAny> = self.instruments_cache.values().cloned().collect();
            if !cached.is_empty() {
                self.ws_user.initialize_instruments(cached).await;
            }
        } else {
            // The Cash (spot) factory loads only spot products; the Margin
            // (derivatives) factory loads the futures universe so CFM perps
            // and dated futures can be reconciled. Mixing the two through a
            // single client is intentionally unsupported, so each factory
            // picks one branch.
            let instruments = if self.is_margin() {
                self.http_client
                    .request_instruments(Some(CoinbaseProductType::Future))
                    .await
                    .context("failed to load Coinbase futures instruments")?
            } else {
                self.http_client
                    .request_instruments(Some(CoinbaseProductType::Spot))
                    .await
                    .context("failed to load Coinbase instruments")?
            };

            let product_kind = if self.is_margin() { "futures" } else { "spot" };

            if instruments.is_empty() {
                log::warn!("Coinbase instrument bootstrap returned no {product_kind} instruments");
            } else {
                log::debug!(
                    "Coinbase exec client loaded {} {product_kind} instruments",
                    instruments.len()
                );
            }

            let mut map: AHashMap<String, InstrumentAny> =
                AHashMap::with_capacity(instruments.len());
            for inst in &instruments {
                map.insert(inst.id().symbol.as_str().to_string(), inst.clone());
            }
            self.instruments_cache = Arc::new(map);

            // Propagate to the WS client so the feed handler can resolve
            // user-channel product IDs to cached instruments.
            self.ws_user.initialize_instruments(instruments).await;

            self.core.set_instruments_initialized();
        }

        if self.fills_start.is_none() {
            let time = self.http_client.request_server_time().await?;
            self.fills_start = Some(time.saturating_sub(DurationNanos::from_millis(1)));
        }

        let session_result = async {
            self.ws_user.set_account_id(self.core.account_id).await;
            self.ws_user.connect().await?;

            // Subscribe to the user channel (product-agnostic). User channel with
            // an empty product list returns events for all products.
            self.ws_user
                .subscribe(CoinbaseWsChannel::User, &[])
                .await
                .context("failed to subscribe to Coinbase user channel")?;

            if self.is_margin() {
                self.ws_user
                    .subscribe(CoinbaseWsChannel::FuturesBalanceSummary, &[])
                    .await
                    .context("failed to subscribe to Coinbase futures_balance_summary channel")?;
            }

            if let Some(rx) = self.ws_user.take_out_rx() {
                let recovery = self.order_recovery();
                let retry_interval = Duration::from_millis(self.config.retry_delay_max_ms.max(1));

                let clock = self.clock;
                let is_margin = self.is_margin();
                self.session_tasks.spawn(async move {
                    recovery.run(rx, retry_interval, clock, is_margin).await;
                })?;
            }

            let account_state = if self.is_margin() {
                self.http_client
                    .request_cfm_account_state(self.core.account_id)
                    .await
                    .context("failed to request Coinbase CFM account state")?
            } else {
                self.http_client
                    .request_account_state(self.core.account_id)
                    .await
                    .context("failed to request Coinbase account state")?
            };

            if !account_state.balances.is_empty() {
                log::debug!(
                    "Received account state with {} balance(s)",
                    account_state.balances.len()
                );
            }
            self.emitter.send_account_state(account_state);

            self.await_account_registered(ACCOUNT_REGISTERED_TIMEOUT_SECS)
                .await?;

            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Coinbase execution startup teardown failed: {teardown_error}"
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

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.subscribe_order_events();
        self.core.set_started();

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

        if let Some(handler) = self.order_event_handler.take() {
            msgbus::unsubscribe_order_events("events.order.*".into(), &handler);
        }

        self.core.set_stopped();
        self.core.set_disconnected();

        self.abort_session_tasks();
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let is_margin = self.is_margin();

        self.spawn_task("query_account", async move {
            let account_state = if is_margin {
                http_client
                    .request_cfm_account_state(account_id)
                    .await
                    .context("failed to request Coinbase CFM account state")?
            } else {
                http_client
                    .request_account_state(account_id)
                    .await
                    .context("failed to request Coinbase account state")?
            };
            emitter.send_account_state(account_state);
            Ok(())
        });
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        if let Some(order) = self.core.cache().order(&cmd.client_order_id) {
            self.lifecycle.lock().track(order.clone());
        }

        let recovery = self.order_recovery();
        self.spawn_task("query_order", async move {
            let report = recovery
                .http_client
                .request_order_status_report(
                    recovery.emitter.account_id(),
                    Some(cmd.client_order_id),
                    cmd.venue_order_id,
                )
                .await?;
            recovery.queue(report.clone());
            recovery
                .lifecycle
                .lock()
                .accept(&report, &recovery.emitter)?;
            recovery.retry().await;
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

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let report = self
            .http_client
            .request_order_status_report(
                self.core.account_id,
                cmd.client_order_id,
                cmd.venue_order_id,
            )
            .await
            .ok();

        // Filter reports to instruments this client bootstrapped. A Cash
        // client drops derivatives reports (and vice-versa) so mixed activity
        // on the same venue account does not poison the engine state
        // associated with either exec client.
        Ok(report.filter(|r| self.is_instrument_cached(&r.instrument_id)))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let start = cmd.start.map(|ts| ts.to_datetime_utc());
        let end = cmd.end.map(|ts| ts.to_datetime_utc());

        let mut reports = self
            .http_client
            .request_order_status_reports(
                self.core.account_id,
                cmd.instrument_id,
                cmd.open_only,
                start,
                end,
                None,
            )
            .await?;

        let before = reports.len();
        reports.retain(|r| self.is_instrument_cached(&r.instrument_id));
        if reports.len() != before {
            let scope = if self.is_margin() {
                "non-futures"
            } else {
                "non-spot"
            };
            log::debug!("Filtered {} {scope} order reports", before - reports.len());
        }
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let start = cmd.start.map(|ts| ts.to_datetime_utc());
        let end = cmd.end.map(|ts| ts.to_datetime_utc());

        let mut reports = self
            .http_client
            .request_fill_reports(
                self.core.account_id,
                cmd.instrument_id,
                cmd.venue_order_id,
                start,
                end,
                None,
            )
            .await?;

        let before = reports.len();
        reports.retain(|r| self.is_instrument_cached(&r.instrument_id));
        if reports.len() != before {
            let scope = if self.is_margin() {
                "non-futures"
            } else {
                "non-spot"
            };
            log::debug!("Filtered {} {scope} fill reports", before - reports.len());
        }
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Coinbase spot has no positions.
        if !self.is_margin() {
            return Ok(Vec::new());
        }

        // Errors propagate (matching `generate_order_status_reports` /
        // `generate_fill_reports`) so `generate_mass_status` and the live
        // manager's reconciliation path see venue failures rather than
        // receive a silently-empty report set.
        if let Some(instrument_id) = cmd.instrument_id {
            let report = self
                .http_client
                .request_position_status_report(self.core.account_id, instrument_id)
                .await
                .with_context(|| format!("failed to request CFM position for {instrument_id}"))?;
            Ok(report.map(|r| vec![r]).unwrap_or_default())
        } else {
            self.http_client
                .request_position_status_reports(self.core.account_id)
                .await
                .context("failed to request CFM positions")
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
            .map(|lookback| ts_now.saturating_sub(lookback));

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
            *COINBASE_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        if let Err(reason) = validate_order(&order, &self.instruments_cache) {
            self.emitter.emit_order_denied(&order, &reason.to_string());
            return Ok(());
        }

        self.lifecycle.lock().track(order.clone());
        let instrument_id = order.instrument_id();

        // The user channel does not need a product-wide alias registration:
        // `order_contexts` (keyed by `client_order_id`) records the
        // submitted `product_id` to restore on working user-channel reports. A product-wide map would
        // misroute external or canonical-side orders that share the same
        // wire `product_id`.

        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        self.emitter.emit_order_submitted(&order);

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let order_contexts = Arc::clone(&self.order_contexts);
        let clock = self.clock;
        let strategy_id = order.strategy_id();
        let client_order_id = order.client_order_id();
        let side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let trigger_type = order.trigger_type();
        let expire_time = order.expire_time();
        let post_only = order.is_post_only();
        let is_quote_quantity = order.is_quote_quantity();
        let reduce_only = order.is_reduce_only();

        // Cache limit/trigger metadata under `client_order_id` synchronously
        // before the spawn so user-channel updates that race the REST submit
        // response can still patch their reports. Coinbase's user channel does
        // not echo `price`, `stop_price`, `trigger_type`, or whether the order
        // is `post_only`, so without this the engine reconciler would clear
        // the local price or lose maker-only semantics.
        {
            let mut map = self.order_contexts.lock();
            map.insert(
                client_order_id.to_string(),
                OrderContext {
                    price,
                    trigger_price,
                    trigger_type,
                    post_only,
                    submitted_product_id: Some(instrument_id.symbol.inner()),
                },
            );
        }
        let (leverage, margin_type) = if self.core.account_type == AccountType::Margin {
            (
                self.config.default_leverage,
                self.config.default_margin_type,
            )
        } else {
            (None, None)
        };
        let retail_portfolio_id = self.config.retail_portfolio_id.clone();

        self.spawn_task("submit_order", async move {
            let result = http_client
                .submit_order(
                    client_order_id,
                    instrument_id,
                    side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    expire_time,
                    post_only,
                    is_quote_quantity,
                    leverage,
                    margin_type,
                    reduce_only,
                    retail_portfolio_id,
                )
                .await;

            match result {
                Ok(response) => {
                    if response.success {
                        let venue_id = response
                            .success_response
                            .as_ref()
                            .map(|s| s.order_id.clone())
                            .unwrap_or(response.order_id);

                        if venue_id.is_empty() {
                            log::warn!(
                                "Submit succeeded but no order_id returned for {client_order_id}"
                            );
                        } else {
                            let venue_order_id = VenueOrderId::new(&venue_id);
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_accepted(&order, venue_order_id, ts_event);
                        }
                    } else {
                        let reason = response.error_response.as_ref().map_or_else(
                            || response.failure_reason.clone(),
                            |e| format!("{}: {}", e.error, e.message),
                        );
                        // `INVALID_LIMIT_PRICE_POST_ONLY` is Coinbase's reject
                        // code when a `post_only` order would have crossed
                        // the spread by the time it reached the matching
                        // engine. Mark the rejection so strategies can react
                        // (typically: re-quote at the new TOB).
                        let due_post_only = reason.contains("INVALID_LIMIT_PRICE_POST_ONLY")
                            || response.error_response.as_ref().is_some_and(|e| {
                                e.preview_failure_reason == "PREVIEW_INVALID_LIMIT_PRICE_POSTONLY"
                                    || e.new_order_failure_reason == "INVALID_LIMIT_PRICE_POST_ONLY"
                            });
                        // Order never made it to the venue: drop the cached
                        // metadata so the map does not grow unbounded with
                        // dead entries.
                        order_contexts.lock().remove(client_order_id.as_str());
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            &format!("submit-order-rejected: {reason}"),
                            ts_event,
                            due_post_only,
                        );
                    }
                }
                Err(e) => {
                    handle_coinbase_submit_failure(
                        &e,
                        &order_contexts,
                        &emitter,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        clock.get_time_ns(),
                    );
                    return Err(e.context("submit order failed"));
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;
        let denied = OrderDeniedReason::UnsupportedOrderList {
            detail: "order lists are not supported by Coinbase Advanced Trade".to_string(),
        }
        .to_string();

        for order in &orders {
            self.emitter.emit_order_denied(order, &denied);
        }

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let ts_event = self.clock.get_time_ns();

        let Some(venue_order_id) = cmd.venue_order_id else {
            self.emitter.emit_order_modify_denied_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                None,
                "modify-order requires venue_order_id",
                ts_event,
                Some(cmd.command_id),
            );
            return Ok(());
        };

        if cmd.price.is_none() && cmd.quantity.is_none() && cmd.trigger_price.is_none() {
            self.emitter.emit_order_modify_denied_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                "modify-order requires price, quantity, or trigger_price",
                ts_event,
                Some(cmd.command_id),
            );
            return Ok(());
        }

        // Coinbase's `/orders/edit` requires both `price` and `size` to be
        // present in the request even when only one is changing; omitting
        // `size` is interpreted as 0 and rejected with `INVALID_EDITED_SIZE` /
        // `CANNOT_EDIT_TO_BELOW_FILLED_SIZE`. Auto-fill missing fields from
        // the cached order so strategies can call `modify_order(price=...)`
        // without having to look up the current quantity themselves.
        let (auto_price, auto_quantity) = {
            let cache = self.core.cache();
            let cached = cache.order(&cmd.client_order_id);
            let cached_price = cached.as_ref().and_then(|o| o.price());
            let cached_qty = cached.as_ref().map(|o| o.quantity());
            (cmd.price.or(cached_price), cmd.quantity.or(cached_qty))
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let order_contexts = Arc::clone(&self.order_contexts);
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let price = auto_price;
        let quantity = auto_quantity;
        let trigger_price = cmd.trigger_price;

        self.spawn_task("modify_order", async move {
            let result = http_client
                .modify_order(venue_order_id, price, quantity, trigger_price)
                .await;

            match result {
                Ok(resp) => {
                    if resp.success {
                        // Refresh the submit-time metadata cache so subsequent
                        // user-channel updates patch with the new price /
                        // trigger_price (Coinbase user channel does not echo
                        // these fields, so a stale cache would let the
                        // reconciler revert the local order to the pre-edit
                        // values).
                        let mut map = order_contexts.lock();
                        if let Some(meta) = map.get_mut(client_order_id.as_str()) {
                            if price.is_some() {
                                meta.price = price;
                            }

                            if trigger_price.is_some() {
                                meta.trigger_price = trigger_price;
                            }
                        }
                    } else {
                        let reason = resp
                            .errors
                            .iter()
                            .map(|e| {
                                if e.edit_failure_reason.is_empty() {
                                    e.preview_failure_reason.clone()
                                } else {
                                    e.edit_failure_reason.clone()
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(",");
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_modify_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &format!("modify-order-rejected: {reason}"),
                            ts_event,
                            Some(cmd.command_id),
                        );
                    }
                }
                Err(e) => {
                    match classify_coinbase_command_failure(&e) {
                        Some(CommandFailure::NotSent(reason)) => emitter
                            .emit_order_modify_denied_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                Some(venue_order_id),
                                &reason,
                                clock.get_time_ns(),
                                Some(cmd.command_id),
                            ),
                        Some(CommandFailure::VenueRejected(reason)) => emitter
                            .emit_order_modify_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                Some(venue_order_id),
                                &reason,
                                clock.get_time_ns(),
                                Some(cmd.command_id),
                            ),
                        Some(CommandFailure::Ambiguous(_)) => log::warn!(
                            "Ambiguous modify failure for {client_order_id}, awaiting reconciliation: {e}"
                        ),
                        None => log::warn!(
                            "Modify command failed without venue-declared outcome for {client_order_id}: {e}"
                        ),
                    }
                    return Err(e.context("modify order failed"));
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let Some(venue_order_id) = cmd.venue_order_id else {
            self.emitter.emit_order_cancel_denied_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                None,
                "cancel-order requires venue_order_id",
                self.clock.get_time_ns(),
                Some(cmd.command_id),
            );
            return Ok(());
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;

        self.spawn_task("cancel_order", async move {
            match http_client.cancel_orders(&[venue_order_id]).await {
                Ok(resp) => {
                    if let Some(result) = resp.results.first()
                        && !result.success
                    {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_cancel_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &format!("cancel-order-rejected: {}", result.failure_reason),
                            ts_event,
                            Some(cmd.command_id),
                        );
                    }
                }
                Err(e) => {
                    if is_coinbase_local_command_failure(&e) {
                        emitter.emit_order_cancel_denied_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &e.to_string(),
                            clock.get_time_ns(),
                            Some(cmd.command_id),
                        );
                    } else if is_coinbase_explicit_command_rejection(&e) {
                        emitter.emit_order_cancel_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &e.to_string(),
                            clock.get_time_ns(),
                            Some(cmd.command_id),
                        );
                    } else if is_coinbase_ambiguous_command_failure(&e) {
                        log::warn!(
                            "Ambiguous cancel failure for {client_order_id}, awaiting reconciliation: {e}"
                        );
                    } else {
                        log::warn!(
                            "Cancel command failed without venue-declared outcome for {client_order_id}: {e}"
                        );
                    }
                    return Err(e.context("cancel order failed"));
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let instrument_id = cmd.instrument_id;
        let side_filter = cmd.order_side;
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;

        self.spawn_task("cancel_all_orders", async move {
            // Coinbase's `order_status=OPEN` filter excludes PENDING / QUEUED
            // orders that were submitted very recently and are still cancelable.
            // Fetch all reports and filter to any open status locally so a cancel-
            // all issued right after submission does not leave working orders behind.
            let reports = http_client
                .request_order_status_reports(
                    account_id,
                    Some(instrument_id),
                    false,
                    None,
                    None,
                    None,
                )
                .await
                .context("failed to list orders for cancel_all")?;

            // Filter to statuses that are safe to cancel and to the requested
            // side since Coinbase's batch-cancel endpoint has no side parameter.
            //
            // Coinbase's `PENDING` / `QUEUED` / `OPEN` all map to `Accepted`
            // and are cancelable. We can't use `OrderStatus::is_open()` because
            // it includes `PendingCancel`, and re-cancelling a `CANCEL_QUEUED`
            // order risks `CancelRejected` flipping the order back to its prior
            // working status.
            let filtered: Vec<CancelRequest> = reports
                .into_iter()
                .filter(|r| {
                    matches!(
                        r.order_status,
                        OrderStatus::Accepted
                            | OrderStatus::Triggered
                            | OrderStatus::PendingUpdate
                            | OrderStatus::PartiallyFilled
                    )
                })
                .filter(|r| side_filter.is_none_or(|side| r.order_side == side.into()))
                .map(|r| CancelRequest {
                    strategy_id,
                    instrument_id,
                    client_order_id: r.client_order_id,
                    venue_order_id: r.venue_order_id,
                    command_id: cmd.command_id,
                })
                .collect();

            cancel_order_batches(&http_client, &emitter, clock, &filtered, "cancel-all").await;
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let cancels = cmd.cancels;

        self.spawn_task("batch_cancel_orders", async move {
            let mut requests = Vec::new();

            for cancel in cancels {
                let Some(venue_order_id) = cancel.venue_order_id else {
                    emitter.emit_order_cancel_denied_event(
                        cancel.strategy_id,
                        cancel.instrument_id,
                        cancel.client_order_id,
                        None,
                        "cancel-order requires venue_order_id",
                        clock.get_time_ns(),
                        Some(cancel.command_id),
                    );
                    continue;
                };

                requests.push(CancelRequest {
                    strategy_id: cancel.strategy_id,
                    instrument_id: cancel.instrument_id,
                    client_order_id: Some(cancel.client_order_id),
                    venue_order_id,
                    command_id: cancel.command_id,
                });
            }

            cancel_order_batches(&http_client, &emitter, clock, &requests, "batch-cancel").await;
            Ok(())
        });

        Ok(())
    }
}

struct CancelRequest {
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: Option<ClientOrderId>,
    venue_order_id: VenueOrderId,
    command_id: UUID4,
}

async fn cancel_order_batches(
    http: &CoinbaseHttpClient,
    emitter: &ExecutionEventEmitter,
    clock: &AtomicTime,
    requests: &[CancelRequest],
    task_name: &str,
) {
    let mut offset = 0;
    while offset < requests.len() {
        let end = offset + cancel_batch_len(&requests[offset..]);
        let chunk = &requests[offset..end];
        offset = end;
        let venue_ids: Vec<_> = chunk.iter().map(|request| request.venue_order_id).collect();
        match http.cancel_orders(&venue_ids).await {
            Ok(response) => {
                for result in response.results {
                    if result.success {
                        continue;
                    }

                    let Some(request) = chunk
                        .iter()
                        .find(|request| request.venue_order_id.as_str() == result.order_id)
                    else {
                        continue;
                    };

                    let Some(client_order_id) = request.client_order_id else {
                        continue;
                    };

                    emitter.emit_order_cancel_rejected_event(
                        request.strategy_id,
                        request.instrument_id,
                        client_order_id,
                        Some(request.venue_order_id),
                        &format!("{task_name}-rejected: {}", result.failure_reason),
                        clock.get_time_ns(),
                        Some(request.command_id),
                    );
                }
            }
            Err(e) => emit_batch_cancel_failure(emitter, clock, chunk, task_name, &e),
        }
    }
}

fn cancel_batch_len(requests: &[CancelRequest]) -> usize {
    let mut targets = AHashSet::new();
    requests
        .iter()
        .take(BATCH_CANCEL_CHUNK)
        .take_while(|request| targets.insert(request.venue_order_id))
        .count()
}

fn emit_batch_cancel_failure(
    emitter: &ExecutionEventEmitter,
    clock: &AtomicTime,
    requests: &[CancelRequest],
    task_name: &str,
    error: &anyhow::Error,
) {
    match classify_coinbase_command_failure(error) {
        Some(CommandFailure::NotSent(reason)) => {
            for request in requests {
                let Some(client_order_id) = request.client_order_id else {
                    continue;
                };

                emitter.emit_order_cancel_denied_event(
                    request.strategy_id,
                    request.instrument_id,
                    client_order_id,
                    Some(request.venue_order_id),
                    &reason,
                    clock.get_time_ns(),
                    Some(request.command_id),
                );
            }
        }
        Some(CommandFailure::VenueRejected(reason)) => {
            for request in requests {
                let Some(client_order_id) = request.client_order_id else {
                    continue;
                };

                emitter.emit_order_cancel_rejected_event(
                    request.strategy_id,
                    request.instrument_id,
                    client_order_id,
                    Some(request.venue_order_id),
                    &reason,
                    clock.get_time_ns(),
                    Some(request.command_id),
                );
            }
        }
        Some(CommandFailure::Ambiguous(_)) => log::warn!(
            "Ambiguous {task_name} failure for {} orders, awaiting reconciliation: {error}",
            requests.len()
        ),
        None => log::warn!(
            "{task_name} command failed without venue-declared outcome for {} orders: {error}",
            requests.len()
        ),
    }
}

fn classify_coinbase_command_failure(error: &anyhow::Error) -> Option<CommandFailure> {
    if is_coinbase_local_command_failure(error) {
        Some(CommandFailure::not_sent(error.to_string()))
    } else if is_coinbase_explicit_command_rejection(error) {
        Some(CommandFailure::venue_rejected(error.to_string()))
    } else if is_coinbase_ambiguous_command_failure(error) {
        Some(CommandFailure::ambiguous(error.to_string()))
    } else {
        None
    }
}

fn validate_order(
    order: &impl Order,
    instruments_cache: &AHashMap<String, InstrumentAny>,
) -> Result<(), OrderDeniedReason> {
    if order.is_reduce_only() {
        return Err(OrderDeniedReason::UnsupportedReduceOnly);
    }

    let instrument_id = order.instrument_id();
    if !instruments_cache.contains_key(instrument_id.symbol.as_str()) {
        return Err(OrderDeniedReason::InstrumentNotFound { instrument_id });
    }

    Ok(())
}

fn handle_coinbase_submit_failure(
    err: &anyhow::Error,
    order_contexts: &Mutex<AHashMap<String, OrderContext>>,
    emitter: &ExecutionEventEmitter,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    ts_event: UnixNanos,
) {
    if is_coinbase_local_command_failure(err) {
        order_contexts.lock().remove(client_order_id.as_str());
        emitter.emit_order_rejected_event(
            strategy_id,
            instrument_id,
            client_order_id,
            &format!("submit-order-error: {err}"),
            ts_event,
            false,
        );
    } else if is_coinbase_explicit_command_rejection(err) {
        order_contexts.lock().remove(client_order_id.as_str());
        emitter.emit_order_rejected_event(
            strategy_id,
            instrument_id,
            client_order_id,
            &format!("submit-order-rejected: {err}"),
            ts_event,
            false,
        );
    } else if is_coinbase_ambiguous_submit_failure(err) {
        log::warn!(
            "Ambiguous submit failure for {client_order_id}, awaiting reconciliation: {err}"
        );
    } else {
        order_contexts.lock().remove(client_order_id.as_str());
        log::warn!(
            "Submit command failed without venue-declared outcome for {client_order_id}: {err}"
        );
    }
}

fn is_coinbase_local_command_failure(err: &anyhow::Error) -> bool {
    match coinbase_http_error(err) {
        None => true,
        Some(CoinbaseHttpError::Auth(message)) => !message.starts_with("HTTP "),
        _ => false,
    }
}

fn is_coinbase_explicit_command_rejection(err: &anyhow::Error) -> bool {
    match coinbase_http_error(err) {
        Some(CoinbaseHttpError::Auth(message) | CoinbaseHttpError::BadRequest(message)) => {
            message.starts_with("HTTP ")
        }
        _ => false,
    }
}

fn is_coinbase_ambiguous_submit_failure(err: &anyhow::Error) -> bool {
    matches!(
        coinbase_http_error(err),
        Some(CoinbaseHttpError::RateLimit { .. })
    ) || is_coinbase_ambiguous_command_failure(err)
}

fn is_coinbase_ambiguous_command_failure(err: &anyhow::Error) -> bool {
    matches!(
        coinbase_http_error(err),
        Some(
            CoinbaseHttpError::Transport(_)
                | CoinbaseHttpError::Serde(_)
                | CoinbaseHttpError::Exchange(_)
                | CoinbaseHttpError::Timeout
                | CoinbaseHttpError::Decode(_)
        )
    ) || matches!(
        coinbase_http_error(err),
        Some(CoinbaseHttpError::Http { status, .. }) if *status >= 500
    )
}

fn coinbase_http_error(err: &anyhow::Error) -> Option<&CoinbaseHttpError> {
    err.chain()
        .find_map(|cause| cause.downcast_ref::<CoinbaseHttpError>())
}

#[derive(Debug)]
struct UserOrderRecovery {
    http_client: CoinbaseHttpClient,
    emitter: ExecutionEventEmitter,
    instruments: Arc<AHashMap<String, InstrumentAny>>,
    observed: Arc<Mutex<AHashMap<VenueOrderId, AHashSet<TradeId>>>>,
    pending: Arc<Mutex<AHashMap<VenueOrderId, OrderStatusReport>>>,
    published: Arc<tokio::sync::Mutex<AHashMap<VenueOrderId, OrderStatusReport>>>,
    order_contexts: Arc<Mutex<AHashMap<String, OrderContext>>>,
    external_order_contexts: Arc<Mutex<AHashMap<String, OrderContext>>>,
    start: UnixNanos,
    lifecycle: Arc<Mutex<RecoveryLifecycle>>,
}

impl UserOrderRecovery {
    async fn run(
        &self,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>,
        retry_interval: Duration,
        clock: &'static AtomicTime,
        is_margin: bool,
    ) {
        let mut interval = tokio::time::interval(retry_interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                message = rx.recv() => {
                    let Some(message) = message else { break };
                    match message {
                        NautilusWsMessage::UserOrder(carrier) => self.handle_update(*carrier).await,
                        NautilusWsMessage::FuturesBalanceSummary(summary) => {
                            let ts = clock.get_time_ns();
                            let account_id = self.emitter.account_id();
                            match parse_ws_cfm_account_state(&summary, account_id, ts, ts) {
                                Ok(state) => self.emitter.send_account_state(state),
                                Err(e) => log::warn!("Failed to parse futures_balance_summary into AccountState: {e}"),
                            }
                        }
                        NautilusWsMessage::Reconnected => {
                            log::info!("Coinbase user WebSocket reconnected");
                            let account_id = self.emitter.account_id();
                            let refresh = if is_margin {
                                self.http_client.request_cfm_account_state(account_id).await
                            } else {
                                self.http_client.request_account_state(account_id).await
                            };

                            match refresh {
                                Ok(state) => self.emitter.send_account_state(state),
                                Err(e) => log::warn!("Failed to refresh account state on reconnect: {e}"),
                            }
                        }
                        NautilusWsMessage::Error(e) => log::warn!("Coinbase user WebSocket error: {e}"),
                        _ => {}
                    }
                }
                _ = interval.tick() => {
                    self.retry().await;
                }
            }
        }
    }

    async fn handle_update(&self, carrier: UserOrderUpdate) {
        let needs_fills =
            carrier.report.filled_qty.is_positive() || carrier.update.status.is_terminal();
        let is_snapshot = carrier.is_snapshot;
        let own_context = self
            .order_contexts
            .lock()
            .contains_key(&carrier.update.client_order_id);
        if needs_fills {
            self.queue((*carrier.report).clone());
            if let Err(e) = self.lifecycle.lock().accept(&carrier.report, &self.emitter) {
                log::warn!("Retaining Coinbase acceptance recovery: {e}");
            }
        }

        if needs_fills && !own_context {
            self.retry().await;
            return;
        }

        let context = resolve_order_context(
            &carrier.update,
            carrier.report.order_type,
            carrier.report.price.is_none(),
            &self.order_contexts,
            &self.external_order_contexts,
            &self.http_client,
            self.emitter.account_id(),
        )
        .await;

        if let Some(report) = prepare_user_order_report(carrier, context) {
            if !is_snapshot
                && own_context
                && let Err(e) = self.lifecycle.lock().update(&report, &self.emitter)
            {
                log::warn!("Failed to publish Coinbase amendment: {e}");
            }

            if !needs_fills && let Err(e) = self.publish(&[(report, Vec::new())]) {
                log::warn!("Failed to publish Coinbase acceptance: {e}");
            }
        }

        if needs_fills {
            self.retry().await;
        }
    }

    async fn retry(&self) {
        if let Err(e) = self.discover().await {
            log::warn!("Coinbase fill recovery scan remains pending: {e}");
            return;
        }

        if let Err(e) = self.recover().await {
            log::warn!("Retaining pending Coinbase recovery: {e}");
        }
    }

    async fn discover(&self) -> anyhow::Result<()> {
        let fills = self.http_client.request_fill_ids(self.start).await?;
        {
            let mut observed = self.observed.lock();

            for (venue_order_id, instrument_id, trade_id) in fills {
                if self.instruments.contains_key(instrument_id.symbol.as_str()) {
                    observed.entry(venue_order_id).or_default().insert(trade_id);
                }
            }
        }

        let mut reports: AHashMap<_, _> = self
            .http_client
            .request_order_status_reports(self.emitter.account_id(), None, false, None, None, None)
            .await?
            .into_iter()
            .filter(|report| {
                self.instruments
                    .contains_key(report.instrument_id.symbol.as_str())
                    && report.filled_qty.is_positive()
                    && report.ts_last >= self.start
            })
            .map(|report| (report.venue_order_id, report))
            .collect();

        let orders: Vec<_> = self.observed.lock().keys().copied().collect();

        for venue_order_id in orders {
            if let Entry::Vacant(entry) = reports.entry(venue_order_id) {
                let report = self
                    .http_client
                    .request_order_status_report(
                        self.emitter.account_id(),
                        None,
                        Some(venue_order_id),
                    )
                    .await?;
                entry.insert(report);
            }
        }

        for report in reports.into_values() {
            self.queue(report);
        }

        Ok(())
    }

    fn queue(&self, report: OrderStatusReport) {
        let mut pending = self.pending.lock();
        let entry = pending
            .entry(report.venue_order_id)
            .or_insert_with(|| report.clone());
        if report.filled_qty > entry.filled_qty
            || (report.filled_qty == entry.filled_qty && !entry.order_status.is_closed())
        {
            *entry = report;
        }
    }

    async fn recover(&self) -> anyhow::Result<()> {
        let mut published = self.published.lock().await;
        let targets = self.pending.lock().clone();
        let mut reports = Vec::with_capacity(targets.len());

        for (venue_order_id, expected) in &targets {
            let report = self
                .http_client
                .request_order_status_report(self.emitter.account_id(), None, Some(*venue_order_id))
                .await?;
            anyhow::ensure!(
                self.instruments
                    .contains_key(report.instrument_id.symbol.as_str()),
                "order instrument is outside the configured product family"
            );
            anyhow::ensure!(
                report.filled_qty >= expected.filled_qty
                    && (!expected.order_status.is_closed() || report.order_status.is_closed()),
                "REST order state has not reached the user-channel update"
            );

            if let Some(prior) = published.get(venue_order_id) {
                anyhow::ensure!(
                    report.filled_qty >= prior.filled_qty
                        && (!prior.order_status.is_closed() || report.order_status.is_closed()),
                    "REST order state precedes published recovery"
                );
            }

            let fills = if report.filled_qty.is_positive()
                || self.observed.lock().contains_key(venue_order_id)
            {
                self.http_client
                    .request_fill_reports(
                        self.emitter.account_id(),
                        None,
                        Some(*venue_order_id),
                        None,
                        None,
                        None,
                    )
                    .await?
            } else {
                Vec::new()
            };

            let fills = prepare_order_fills(&report, fills)?;
            reports.push((report, fills));
        }

        let mut pending = self.pending.lock();
        let mut observed = self.observed.lock();
        anyhow::ensure!(
            pending.len() == targets.len()
                && targets
                    .iter()
                    .all(|(id, target)| pending.get(id) == Some(target)),
            "recovery target changed while reading REST"
        );

        let trades: AHashMap<_, AHashSet<_>> = reports
            .iter()
            .map(|(report, fills)| {
                (
                    report.venue_order_id,
                    fills.iter().map(|fill| fill.trade_id).collect(),
                )
            })
            .collect();

        anyhow::ensure!(
            observed
                .iter()
                .all(|(id, ids)| trades.get(id).is_some_and(|trades| ids.is_subset(trades))),
            "REST fills have not reached discovered trades"
        );
        self.publish(&reports)?;
        observed.clear();

        for (report, _) in reports {
            let venue_order_id = report.venue_order_id;
            if pending.get(&venue_order_id) == targets.get(&venue_order_id) {
                pending.remove(&venue_order_id);
            }

            if report.order_status.is_closed() {
                if let Some(client_order_id) = report.client_order_id {
                    self.order_contexts.lock().remove(client_order_id.as_str());
                }

                self.external_order_contexts
                    .lock()
                    .remove(venue_order_id.as_str());
            }

            published.insert(venue_order_id, report);
        }

        Ok(())
    }

    fn publish(&self, reports: &[(OrderStatusReport, Vec<FillReport>)]) -> anyhow::Result<()> {
        let mut external = Vec::new();
        {
            let mut lifecycle = self.lifecycle.lock();
            for (report, _) in reports {
                if lifecycle.order(report).is_some() {
                    lifecycle
                        .pending
                        .insert(report.venue_order_id, report.clone());
                } else {
                    // Establish order context without inferring fills before the real trades
                    let mut accepted = report.clone();
                    if accepted.order_status != OrderStatus::Rejected {
                        accepted.order_status = OrderStatus::Accepted;
                    }

                    accepted.filled_qty = Quantity::zero(report.filled_qty.precision);
                    accepted.avg_px = None;
                    accepted.ts_last = accepted.ts_accepted;
                    self.emitter
                        .try_send_execution_report(ExecutionReport::Order(Box::new(accepted)))?;
                    external.push(report);
                }
            }

            lifecycle.dispatch(&self.emitter)?;
        }

        let mut fills: Vec<_> = reports.iter().flat_map(|(_, fills)| fills).collect();
        fills.sort_by_key(|fill| (fill.ts_event, fill.trade_id, fill.venue_order_id));
        for fill in fills {
            self.emitter
                .try_send_execution_report(ExecutionReport::Fill(Box::new(fill.clone())))?;
        }

        for report in external {
            self.emitter
                .try_send_execution_report(ExecutionReport::Order(Box::new(report.clone())))?;
        }

        Ok(())
    }
}

#[derive(Debug)]
struct RecoveryLifecycle {
    factory: OrderEventFactory,
    orders: AHashMap<ClientOrderId, OrderAny>,
    venue_ids: AHashMap<VenueOrderId, ClientOrderId>,
    pending: AHashMap<VenueOrderId, OrderStatusReport>,
    accepted: AHashSet<VenueOrderId>,
}

impl RecoveryLifecycle {
    fn new(trader_id: TraderId, account_id: AccountId, account_type: AccountType) -> Self {
        Self {
            factory: OrderEventFactory::new(trader_id, account_id, account_type, None),
            orders: AHashMap::new(),
            venue_ids: AHashMap::new(),
            pending: AHashMap::new(),
            accepted: AHashSet::new(),
        }
    }

    fn track(&mut self, order: OrderAny) {
        if let Some(venue_order_id) = order.venue_order_id() {
            if !matches!(
                order.status(),
                OrderStatus::Initialized
                    | OrderStatus::Submitted
                    | OrderStatus::PendingUpdate
                    | OrderStatus::PendingCancel
            ) {
                self.accepted.remove(&venue_order_id);
            }

            self.venue_ids
                .insert(venue_order_id, order.client_order_id());
        }

        self.orders.insert(order.client_order_id(), order);
    }

    fn dispatch(&mut self, emitter: &ExecutionEventEmitter) -> anyhow::Result<()> {
        let reports: Vec<_> = self.pending.values().cloned().collect();
        for report in reports {
            if self.dispatch_order(&report, emitter)? {
                self.pending.remove(&report.venue_order_id);
            }
        }

        Ok(())
    }

    fn dispatch_order(
        &mut self,
        report: &OrderStatusReport,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<bool> {
        self.accept(report, emitter)?;

        let Some(order) = self.order(report).cloned() else {
            return Ok(false);
        };

        if order.is_closed() {
            return Ok(true);
        }

        let factory = &self.factory;
        let now = get_atomic_clock_realtime().get_time_ns();

        if (order.order_type() == OrderType::Market
            && order.is_quote_quantity()
            && report.order_status.is_closed()
            && report.filled_qty.is_positive())
            || (!order.is_quote_quantity() && report.filled_qty > order.quantity())
            || (report.order_status == OrderStatus::Filled && report.quantity != order.quantity())
        {
            let updated = OrderUpdated::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                report.quantity,
                UUID4::new(),
                report.ts_last,
                now,
                true,
                Some(report.venue_order_id),
                Some(emitter.account_id()),
                None,
                None,
                None,
                false,
            );
            emitter.try_send_order_event(OrderEventAny::Updated(updated))?;
        }

        if order.filled_qty() < report.filled_qty {
            return Ok(false);
        }

        let event = match report.order_status {
            OrderStatus::Canceled => factory.generate_order_canceled(
                &order,
                Some(report.venue_order_id),
                report.ts_last,
                now,
            ),
            OrderStatus::Expired => factory.generate_order_expired(
                &order,
                Some(report.venue_order_id),
                report.ts_last,
                now,
            ),
            OrderStatus::Rejected => factory.generate_order_rejected(
                &order,
                report.cancel_reason.as_deref().unwrap_or("rejected"),
                report.ts_last,
                now,
                false,
            ),
            _ => return Ok(true),
        };

        emitter.try_send_order_event(event)?;
        Ok(true)
    }

    fn accept(
        &mut self,
        report: &OrderStatusReport,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let Some(order) = self.order(report) else {
            return Ok(());
        };

        let factory = &self.factory;
        let now = get_atomic_clock_realtime().get_time_ns();
        let awaiting_acceptance = matches!(
            order.status(),
            OrderStatus::Initialized | OrderStatus::Submitted
        );

        if awaiting_acceptance
            && report.order_status != OrderStatus::Rejected
            && !self.accepted.contains(&report.venue_order_id)
        {
            let event = factory.generate_order_accepted(
                order,
                report.venue_order_id,
                report.ts_accepted,
                now,
            );
            emitter.try_send_order_event(event)?;
            self.accepted.insert(report.venue_order_id);
        }

        Ok(())
    }

    fn update(
        &self,
        report: &OrderStatusReport,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let Some(order) = self.order(report) else {
            return Ok(());
        };

        if order.is_closed()
            || (order.order_type() == OrderType::Market && order.is_quote_quantity())
            || (order.quantity() == report.quantity
                && order.price() == report.price
                && order.trigger_price() == report.trigger_price)
        {
            return Ok(());
        }

        let factory = &self.factory;
        let now = get_atomic_clock_realtime().get_time_ns();
        let event = factory.generate_order_updated(
            order,
            report.venue_order_id,
            report.quantity,
            report.price,
            report.trigger_price,
            None,
            report.ts_last,
            now,
        );
        emitter.try_send_order_event(event)
    }

    fn order(&self, report: &OrderStatusReport) -> Option<&OrderAny> {
        report
            .client_order_id
            .and_then(|id| self.orders.get(&id))
            .or_else(|| {
                self.venue_ids
                    .get(&report.venue_order_id)
                    .and_then(|id| self.orders.get(id))
            })
    }
}

fn prepare_order_fills(
    report: &OrderStatusReport,
    fills: Vec<FillReport>,
) -> anyhow::Result<Vec<FillReport>> {
    // Quote-sized market orders have no final base quantity before terminal REST state
    anyhow::ensure!(
        report.order_type != OrderType::Market
            || report.order_status.is_closed()
            || report.quantity > report.filled_qty,
        "market order base quantity is not final"
    );
    let mut unique = AHashMap::new();

    for mut fill in fills {
        anyhow::ensure!(
            fill.account_id == report.account_id
                && fill.venue_order_id == report.venue_order_id
                && fill.instrument_id == report.instrument_id
                && report.order_side == Some(fill.order_side),
            "fill identity does not match the order report"
        );
        fill.client_order_id = report.client_order_id;
        unique.entry(fill.trade_id).or_insert(fill);
    }

    let mut fills: Vec<_> = unique.into_values().collect();
    fills.sort_by_key(|fill| (fill.ts_event, fill.trade_id));

    let quantity = fills.iter().try_fold(Decimal::ZERO, |quantity, fill| {
        quantity
            .checked_add(fill.last_qty.as_decimal())
            .context("fill quantity overflow")
    })?;

    anyhow::ensure!(
        quantity == report.filled_qty.as_decimal(),
        "REST fills do not cover the order filled quantity"
    );
    Ok(fills)
}

fn prepare_user_order_report(
    carrier: UserOrderUpdate,
    context: Option<OrderContext>,
) -> Option<OrderStatusReport> {
    let UserOrderUpdate {
        mut report,
        update,
        is_snapshot,
        ..
    } = carrier;
    let have_context = context.is_some();
    if let Some(meta) = context {
        if report.price.is_none() {
            report.price = meta.price;
        }

        if report.trigger_price.is_none() {
            report.trigger_price = meta.trigger_price;
        }

        if report.trigger_type.is_none() {
            report.trigger_type = meta.trigger_type;
        }

        report.post_only = meta.post_only;
        if let Some(submitted) = meta.submitted_product_id {
            report.instrument_id = InstrumentId::new(Symbol::new(submitted), *COINBASE_VENUE);
        }
    }

    let safe = match report.order_type {
        OrderType::Limit | OrderType::LimitIfTouched => report.price.is_some(),
        OrderType::StopLimit => report.price.is_some() && report.trigger_price.is_some(),
        OrderType::StopMarket | OrderType::MarketIfTouched => report.trigger_price.is_some(),
        _ => true,
    };

    if !safe {
        log::warn!(
            "Suppressed unsafe OrderStatusReport for {} {}: missing price/trigger after enrichment",
            report.order_type,
            update.order_id
        );
        return None;
    }

    (!is_snapshot || have_context).then_some(*report)
}

// Returns the submit-time / enriched metadata for `update`, fetching from
// REST and populating the enrichment cache the first time an external order
// is seen. `order_contexts` (keyed by `client_order_id`) covers orders this
// client placed; `external_order_contexts` (keyed by venue `order_id`) covers
// external orders whose `OrderStatusReport` would otherwise be unsafe to
// reconstruct (LIMIT / STOP_LIMIT with `price = None`).
async fn resolve_order_context(
    update: &WsOrderUpdate,
    order_type: OrderType,
    report_price_missing: bool,
    order_contexts: &Arc<Mutex<AHashMap<String, OrderContext>>>,
    external_order_contexts: &Arc<Mutex<AHashMap<String, OrderContext>>>,
    http_client: &CoinbaseHttpClient,
    account_id: AccountId,
) -> Option<OrderContext> {
    if !update.client_order_id.is_empty() {
        let map = order_contexts.lock();
        if let Some(meta) = map.get(&update.client_order_id) {
            return Some(meta.clone());
        }
    }

    if let Some(meta) = external_order_contexts.lock().get(&update.order_id) {
        return Some(meta.clone());
    }

    let needs_enrichment = report_price_missing
        && matches!(
            order_type,
            OrderType::Limit
                | OrderType::StopLimit
                | OrderType::LimitIfTouched
                | OrderType::StopMarket
                | OrderType::MarketIfTouched
        );

    if !needs_enrichment {
        return None;
    }

    let venue_order_id = VenueOrderId::new(update.order_id.as_str());

    match http_client
        .request_order_status_report(account_id, None, Some(venue_order_id))
        .await
    {
        Ok(rest_report) => {
            let post_only_from_rest = matches!(order_type, OrderType::Limit | OrderType::StopLimit)
                && rest_report.post_only;

            let meta = OrderContext {
                price: rest_report.price,
                trigger_price: rest_report.trigger_price,
                trigger_type: rest_report.trigger_type,
                post_only: post_only_from_rest,
                submitted_product_id: None,
            };
            external_order_contexts
                .lock()
                .insert(update.order_id.clone(), meta.clone());
            Some(meta)
        }
        Err(e) => {
            log::warn!(
                "Failed to enrich external order {} via REST: {e}",
                update.order_id
            );
            None
        }
    }
}

// Restores price and trigger fields omitted from unfilled user-channel reports
// using submit-time metadata keyed by client_order_id.
#[derive(Debug, Default, Clone)]
struct OrderContext {
    price: Option<Price>,
    trigger_price: Option<Price>,
    trigger_type: Option<TriggerType>,
    post_only: bool,
    // The `product_id` the order was submitted with. Coinbase rewrites
    // aliased products to the canonical id on the user channel, so
    // `update.product_id` always reads as the canonical (e.g. `BTC-USD`)
    // even for an order placed on the alias side (`BTC-USDC`). Looking the
    // submitted id up by `client_order_id` lets us re-key user-channel
    // echoes back to the caller's id without rewriting *every* canonical
    // event globally.
    submitted_product_id: Option<Ustr>,
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        enums::Environment,
        messages::{ExecutionEvent, ExecutionReport},
    };
    use nautilus_live::{manager::ExecutionManager, node::LiveNode};
    use nautilus_model::{
        accounts::CashAccount,
        enums::{AccountType, OrderSide, TimeInForce},
        events::{AccountState, OrderEventAny, OrderPendingCancel, OrderPendingUpdate},
        identifiers::{Symbol, TraderId, Venue},
        instruments::{
            CurrencyPair,
            stubs::{crypto_future_btcusdt, crypto_perpetual_ethusdt},
        },
        orders::OrderTestBuilder,
        types::{Currency, Money, Quantity},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{
            consts::COINBASE_VENUE,
            enums::{
                CoinbaseContractExpiryType, CoinbaseOrderSide as CbSide,
                CoinbaseOrderStatus as CbStatus, CoinbaseOrderType as CbType,
                CoinbaseProductType as CbProductType, CoinbaseRiskManagedBy,
                CoinbaseTimeInForce as CbTif, CoinbaseTriggerStatus,
            },
        },
        websocket::messages::WsOrderUpdate,
    };

    #[rstest]
    #[case::limit(101, false, 100)]
    #[case::repeated(3, true, 1)]
    fn test_cancel_batch_len(
        #[case] count: usize,
        #[case] repeated: bool,
        #[case] expected: usize,
    ) {
        let requests: Vec<_> = (0..count)
            .map(|index| CancelRequest {
                strategy_id: StrategyId::from("S-BATCH"),
                instrument_id: InstrumentId::from("BTC-USD.COINBASE"),
                client_order_id: Some(ClientOrderId::from(format!("C-{index}"))),
                venue_order_id: VenueOrderId::from(format!(
                    "V-{}",
                    if repeated { 0 } else { index }
                )),
                command_id: UUID4::new(),
            })
            .collect();

        assert_eq!(cancel_batch_len(&requests), expected);
    }

    #[rstest]
    #[case::not_sent(CoinbaseHttpError::auth("No credentials configured"), Some(false))]
    #[case::venue_rejected(
        CoinbaseHttpError::bad_request("HTTP 400: invalid cancellation"),
        Some(true)
    )]
    #[case::ambiguous(CoinbaseHttpError::Timeout, None)]
    #[case::unclassified(CoinbaseHttpError::rate_limit(None), None)]
    fn test_batch_cancel_failure_preserves_child_identity(
        #[case] error: CoinbaseHttpError,
        #[case] rejected: Option<bool>,
    ) {
        let (emitter, mut receiver) = make_emitter();
        let clock = AtomicTime::new(false, UnixNanos::from(53));

        let requests = [0, 1, 2].map(|index| CancelRequest {
            strategy_id: StrategyId::from(format!("S-BATCH-{index}")),
            instrument_id: InstrumentId::from(if index == 0 {
                "BTC-USD.COINBASE"
            } else {
                "ETH-USD.COINBASE"
            }),
            client_order_id: (index != 1).then(|| ClientOrderId::from(format!("O-BATCH-{index}"))),
            venue_order_id: VenueOrderId::from(format!("V-BATCH-{index}")),
            command_id: UUID4::new(),
        });

        let error = anyhow::Error::new(error);
        emit_batch_cancel_failure(&emitter, &clock, &requests, "batch_cancel_orders", &error);
        if let Some(rejected) = rejected {
            for request in requests
                .iter()
                .filter(|request| request.client_order_id.is_some())
            {
                let ExecutionEvent::Order(event) = receiver.try_recv().unwrap() else {
                    panic!("expected cancellation outcome");
                };

                let (strategy, instrument, client, venue, cause, reason, ts_event) = match event {
                    OrderEventAny::CancelDenied(event) if !rejected => (
                        event.strategy_id,
                        event.instrument_id,
                        event.client_order_id,
                        event.venue_order_id,
                        event.causation_id,
                        event.reason,
                        event.ts_event,
                    ),
                    OrderEventAny::CancelRejected(event) if rejected => (
                        event.strategy_id,
                        event.instrument_id,
                        event.client_order_id,
                        event.venue_order_id,
                        event.causation_id,
                        event.reason,
                        event.ts_event,
                    ),
                    other => panic!("unexpected cancellation outcome {other:?}"),
                };

                assert_eq!(
                    (
                        strategy,
                        instrument,
                        client,
                        venue,
                        cause,
                        reason.as_str(),
                        ts_event
                    ),
                    (
                        request.strategy_id,
                        request.instrument_id,
                        request.client_order_id.unwrap(),
                        Some(request.venue_order_id),
                        Some(request.command_id),
                        error.to_string().as_str(),
                        UnixNanos::from(53)
                    )
                );
            }
        }

        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    #[rstest]
    #[case::cash(AccountType::Cash, true, false)]
    #[case::margin_cached(AccountType::Margin, true, true)]
    #[case::margin_uncached(AccountType::Margin, false, false)]
    fn test_bulk_position_coverage(
        #[case] account_type: AccountType,
        #[case] cached: bool,
        #[case] expected: bool,
        #[values(false, true)] perpetual: bool,
    ) {
        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from("COINBASE-TEST"),
            *COINBASE_VENUE,
            OmsType::Netting,
            AccountId::from("COINBASE-001"),
            account_type,
            None,
            cache,
        );

        let config = CoinbaseExecutionClientConfig {
            api_key: Some("test-key".into()),
            api_secret: Some("test-secret".into()),
            account_type,
            ..Default::default()
        };

        let mut client = CoinbaseExecutionClient::new(core, config).unwrap();

        let instrument = if perpetual {
            let mut instrument = crypto_perpetual_ethusdt();
            instrument.id = InstrumentId::from("ETH-PERP.COINBASE");
            InstrumentAny::CryptoPerpetual(instrument)
        } else {
            let mut instrument =
                crypto_future_btcusdt(2, 6, Price::from("0.01"), Quantity::from("0.000001"));
            instrument.id = InstrumentId::from("BIP-20DEC30-CDE.COINBASE");
            InstrumentAny::CryptoFuture(instrument)
        };

        let instrument_id = instrument.id();

        if cached {
            client.instruments_cache = Arc::new(AHashMap::from([(
                instrument_id.symbol.as_str().to_owned(),
                instrument,
            )]));
        }

        assert_eq!(
            client.provides_bulk_position_coverage(instrument_id),
            expected
        );
        let foreign_id = InstrumentId::new(instrument_id.symbol, Venue::from("OTHER"));
        assert!(!client.provides_bulk_position_coverage(foreign_id));
        assert!(!client.provides_bulk_position_coverage(InstrumentId::from("BTC-USD.COINBASE")));
        assert!(!client.provides_bulk_position_coverage(InstrumentId::from("UNKNOWN.COINBASE")));
    }

    #[rstest]
    fn test_submit_local_command_failure_classification() {
        let err = anyhow::anyhow!("Unsupported Coinbase order configuration");

        assert!(is_coinbase_local_command_failure(&err));
        assert!(!is_coinbase_explicit_command_rejection(&err));
        assert!(!is_coinbase_ambiguous_command_failure(&err));
    }

    #[rstest]
    fn test_submit_http_exchange_failure_classification() {
        let err = anyhow::Error::new(CoinbaseHttpError::exchange("HTTP 500: unavailable"))
            .context("failed to submit order");

        assert!(!is_coinbase_local_command_failure(&err));
        assert!(!is_coinbase_explicit_command_rejection(&err));
        assert!(is_coinbase_ambiguous_command_failure(&err));
    }

    #[rstest]
    fn test_submit_http_bad_request_failure_classification() {
        let err = anyhow::Error::new(CoinbaseHttpError::bad_request("HTTP 400: bad request"))
            .context("failed to submit order");

        assert!(!is_coinbase_local_command_failure(&err));
        assert!(is_coinbase_explicit_command_rejection(&err));
        assert!(!is_coinbase_ambiguous_command_failure(&err));
    }

    #[rstest]
    #[case(401)]
    #[case(403)]
    fn test_submit_http_auth_failure_classification(#[case] status: u16) {
        let err = anyhow::Error::new(CoinbaseHttpError::auth(format!(
            "HTTP {status}: authentication failed"
        )))
        .context("failed to submit order");

        assert!(!is_coinbase_local_command_failure(&err));
        assert!(is_coinbase_explicit_command_rejection(&err));
        assert!(!is_coinbase_ambiguous_command_failure(&err));
    }

    #[rstest]
    fn test_submit_http_rate_limit_failure_classification() {
        let err = anyhow::Error::new(CoinbaseHttpError::rate_limit(None))
            .context("failed to submit order");

        assert!(!is_coinbase_local_command_failure(&err));
        assert!(!is_coinbase_explicit_command_rejection(&err));
        assert!(is_coinbase_ambiguous_submit_failure(&err));
        assert!(!is_coinbase_ambiguous_command_failure(&err));
    }

    #[tokio::test]
    async fn test_rate_limited_submit_retains_context() {
        let (emitter, mut rx) = make_emitter();
        let order_contexts = Arc::new(Mutex::new(AHashMap::new()));

        let context = OrderContext {
            price: Some(Price::from("100.00")),
            trigger_price: Some(Price::from("99.00")),
            trigger_type: Some(TriggerType::LastPrice),
            post_only: true,
            submitted_product_id: Some(Ustr::from("BTC-USDC")),
        };
        order_contexts
            .lock()
            .insert("client-1".to_string(), context.clone());
        let err = anyhow::Error::new(CoinbaseHttpError::rate_limit(Some(1_000)))
            .context("failed to submit order");

        handle_coinbase_submit_failure(
            &err,
            &order_contexts,
            &emitter,
            StrategyId::from("S-001"),
            InstrumentId::from("BTC-USD.COINBASE"),
            ClientOrderId::from("client-1"),
            UnixNanos::default(),
        );

        assert!(rx.try_recv().is_err());
        {
            let map = order_contexts.lock();
            let retained = map.get("client-1").expect("submit context retained");
            assert_eq!(retained.price, context.price);
            assert_eq!(retained.trigger_price, context.trigger_price);
            assert_eq!(retained.trigger_type, context.trigger_type);
            assert_eq!(retained.post_only, context.post_only);
            assert_eq!(retained.submitted_product_id, context.submitted_product_id);
        }
    }

    #[rstest]
    #[case(None)]
    #[case(Some(400))]
    #[case(Some(401))]
    #[case(Some(403))]
    fn test_definitive_submit_failure_rejects_and_removes_context(#[case] status: Option<u16>) {
        let (emitter, mut rx) = make_emitter();
        let order_contexts = Mutex::new(AHashMap::from_iter([(
            "client-rejected".to_string(),
            make_limit_context(),
        )]));
        let (err, reason) = match status {
            None => (
                anyhow::anyhow!("Unsupported Coinbase order configuration"),
                "submit-order-error: Unsupported Coinbase order configuration".to_string(),
            ),
            Some(400) => (
                anyhow::Error::new(CoinbaseHttpError::bad_request("HTTP 400: refused")),
                "submit-order-rejected: bad request: HTTP 400: refused".to_string(),
            ),
            Some(status @ (401 | 403)) => (
                anyhow::Error::new(CoinbaseHttpError::auth(format!("HTTP {status}: refused"))),
                format!("submit-order-rejected: auth error: HTTP {status}: refused"),
            ),
            Some(status) => panic!("unsupported status {status}"),
        };

        handle_coinbase_submit_failure(
            &err,
            &order_contexts,
            &emitter,
            StrategyId::from("S-REJECT"),
            InstrumentId::from("BTC-USD.COINBASE"),
            ClientOrderId::from("client-rejected"),
            UnixNanos::from(42_u64),
        );

        assert!(order_contexts.lock().is_empty());
        let event = rx.try_recv().expect("order rejection emitted");
        let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event else {
            panic!("expected OrderRejected event, was {event:?}");
        };
        assert_eq!(rejected.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(rejected.strategy_id, StrategyId::from("S-REJECT"));
        assert_eq!(
            rejected.instrument_id,
            InstrumentId::from("BTC-USD.COINBASE")
        );
        assert_eq!(
            rejected.client_order_id,
            ClientOrderId::from("client-rejected")
        );
        assert_eq!(rejected.account_id, AccountId::from("COINBASE-001"));
        assert_eq!(rejected.reason, reason);
        assert_eq!(rejected.ts_event, UnixNanos::from(42_u64));
        assert!(!rejected.reconciliation);
        assert!(!rejected.due_post_only);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_submit_unmapped_http_4xx_failure_classification() {
        let err = anyhow::Error::new(CoinbaseHttpError::http(409, "conflict"))
            .context("failed to submit order");

        assert!(!is_coinbase_local_command_failure(&err));
        assert!(!is_coinbase_explicit_command_rejection(&err));
        assert!(!is_coinbase_ambiguous_command_failure(&err));
    }

    #[rstest]
    fn test_submit_local_auth_failure_classification() {
        let err = anyhow::Error::new(CoinbaseHttpError::auth("No credentials configured"))
            .context("failed to submit order");

        assert!(is_coinbase_local_command_failure(&err));
        assert!(!is_coinbase_explicit_command_rejection(&err));
        assert!(!is_coinbase_ambiguous_command_failure(&err));
    }

    fn test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD"), *COINBASE_VENUE);
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("BTC-USD"))
                .base_currency(Currency::get_or_create_crypto("BTC"))
                .quote_currency(Currency::get_or_create_crypto("USD"))
                .price_precision(2)
                .size_precision(8)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.00000001"))
                .min_quantity(Quantity::from("0.00000001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
    }

    fn make_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            TraderId::from("TRADER-001"),
            AccountId::new("COINBASE-001"),
            AccountType::Cash,
            None,
        );
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn make_user_order_update(
        cumulative: &str,
        leaves: &str,
        avg_price: &str,
        total_fees: &str,
        status: CbStatus,
    ) -> WsOrderUpdate {
        WsOrderUpdate {
            order_id: "venue-1".to_string(),
            client_order_id: "client-1".to_string(),
            contract_expiry_type: CoinbaseContractExpiryType::Unknown,
            cumulative_quantity: cumulative.to_string(),
            leaves_quantity: leaves.to_string(),
            avg_price: avg_price.to_string(),
            total_fees: total_fees.to_string(),
            status,
            product_id: Ustr::from("BTC-USD"),
            product_type: CbProductType::Spot,
            creation_time: String::new(),
            order_side: CbSide::Buy,
            order_type: CbType::Limit,
            risk_managed_by: CoinbaseRiskManagedBy::Unknown,
            time_in_force: CbTif::GoodUntilCancelled,
            trigger_status: CoinbaseTriggerStatus::InvalidOrderType,
            cancel_reason: String::new(),
            reject_reason: String::new(),
            total_value_after_fees: String::new(),
        }
    }

    fn make_carrier(update: WsOrderUpdate) -> UserOrderUpdate {
        make_carrier_with_kind(update, false)
    }

    fn make_limit_context() -> OrderContext {
        OrderContext {
            price: Some(Price::from("100.00")),
            ..OrderContext::default()
        }
    }

    fn make_carrier_with_kind(update: WsOrderUpdate, is_snapshot: bool) -> UserOrderUpdate {
        let instrument = test_instrument();
        let report = crate::websocket::parse::parse_ws_user_event_to_order_status_report(
            &update,
            &instrument,
            AccountId::new("COINBASE-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap();
        UserOrderUpdate {
            report: Box::new(report),
            update: Box::new(update),
            instrument,
            is_snapshot,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    struct RecoverySpec {
        recovery: UserOrderRecovery,
        rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        state: Arc<Mutex<RecoveryResponses>>,
        server: tokio::task::JoinHandle<()>,
        client: Option<CoinbaseExecutionClient>,
    }

    struct RecoveryResponses {
        order: serde_json::Value,
        fills: serde_json::Value,
        fills_targeted: Option<serde_json::Value>,
        fail_fills: bool,
        orders: AHashMap<String, serde_json::Value>,
        discover_orders: bool,
        on_discovery: Option<Box<dyn FnOnce() + Send>>,
        requests: Vec<String>,
        on_order: Option<Box<dyn FnOnce() + Send>>,
    }

    impl RecoverySpec {
        async fn new() -> Self {
            let mut order: serde_json::Value =
                serde_json::from_str(include_str!("../test_data/http_order.json")).unwrap();
            order["order"]["order_id"] = "venue-1".into();
            order["order"]["client_order_id"] = "client-1".into();
            order["order"]["order_configuration"]["limit_limit_gtc"]["base_size"] = "1.0".into();
            order["order"]["status"] = "FILLED".into();
            order["order"]["filled_size"] = "1.0".into();
            order["order"]["average_filled_price"] = "112.00".into();
            order["order"]["created_time"] = "2024-01-15T09:59:00Z".into();
            order["order"]["last_fill_time"] = "2024-01-15T10:31:00Z".into();
            let fixture: serde_json::Value =
                serde_json::from_str(include_str!("../test_data/http_fills.json")).unwrap();
            let mut first = fixture["fills"][0].clone();
            first["order_id"] = "venue-1".into();
            first["trade_id"] = "trade-alpha".into();
            first["size"] = "0.4".into();
            first["price"] = "100.00".into();
            first["commission"] = "0.04".into();
            let mut second = first.clone();
            second["trade_id"] = "trade-beta".into();
            second["size"] = "0.6".into();
            second["price"] = "120.00".into();
            second["commission"] = "0.06".into();
            second["trade_time"] = "2024-01-15T10:31:00Z".into();
            second["liquidity_indicator"] = "TAKER".into();

            let state = Arc::new(Mutex::new(RecoveryResponses {
                order,
                fills: serde_json::json!({"fills": [second, first], "cursor": ""}),
                fills_targeted: None,
                fail_fills: false,
                orders: AHashMap::new(),
                discover_orders: true,
                on_discovery: None,
                requests: Vec::new(),
                on_order: None,
            }));

            let router = axum::Router::new().fallback(axum::routing::get({
                let state = Arc::clone(&state);
                move |uri: axum::http::Uri| {
                    let state = Arc::clone(&state);
                    async move {
                        let (status, value) = state.lock().response(&uri);
                        (status, axum::Json(value))
                    }
                }
            }));

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
            let rng = aws_lc_rs::rand::SystemRandom::new();
            let key = aws_lc_rs::signature::EcdsaKeyPair::generate_pkcs8(
                &aws_lc_rs::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                &rng,
            )
            .unwrap();
            let pem = pem::encode(&pem::Pem::new("PRIVATE KEY", key.as_ref().to_vec()));
            let credential = CoinbaseCredential::new("test-key".into(), pem);

            let http_client = CoinbaseHttpClient::with_credentials(
                credential,
                crate::common::enums::CoinbaseEnvironment::Live,
                5,
                None,
                Some(RetryConfig {
                    max_retries: 0,
                    ..Default::default()
                }),
            )
            .unwrap();

            http_client.set_base_url(format!("http://{addr}"));
            let instrument = test_instrument();
            http_client.cache_instrument(&instrument);
            let (emitter, rx) = make_emitter();

            let recovery = UserOrderRecovery {
                http_client,
                emitter,
                instruments: Arc::new(AHashMap::from([("BTC-USD".into(), instrument)])),
                observed: Arc::new(Mutex::new(AHashMap::new())),
                pending: Arc::new(Mutex::new(AHashMap::new())),
                published: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
                order_contexts: Arc::new(Mutex::new(AHashMap::new())),
                external_order_contexts: Arc::new(Mutex::new(AHashMap::new())),
                lifecycle: Arc::new(Mutex::new(RecoveryLifecycle::new(
                    TraderId::from("TRADER-001"),
                    AccountId::from("COINBASE-001"),
                    AccountType::Cash,
                ))),
                start: crate::http::parse::parse_rfc3339_timestamp("2024-01-01T00:00:00Z").unwrap(),
            };

            Self {
                recovery,
                rx,
                state,
                server,
                client: None,
            }
        }

        fn attach(&mut self, node: &LiveNode) {
            let core = ExecutionClientCore::new(
                TraderId::from("TRADER-001"),
                ClientId::from("COINBASE-TEST"),
                *COINBASE_VENUE,
                OmsType::Netting,
                AccountId::from("COINBASE-001"),
                AccountType::Cash,
                None,
                node.kernel().cache.clone(),
            );

            let config = CoinbaseExecutionClientConfig {
                api_key: Some("test-key".into()),
                api_secret: Some("test-secret".into()),
                ..Default::default()
            };

            let mut client = CoinbaseExecutionClient::new(core, config).unwrap();
            client.emitter = self.recovery.emitter.clone();
            client.lifecycle = Arc::clone(&self.recovery.lifecycle);
            client.subscribe_order_events();
            self.client = Some(client);
        }

        fn apply(&mut self, node: &LiveNode) {
            while let Ok(event) = self.rx.try_recv() {
                match event {
                    ExecutionEvent::Report(report) => node
                        .kernel()
                        .exec_engine
                        .borrow_mut()
                        .reconcile_execution_report(&report),
                    ExecutionEvent::Order(event) => {
                        node.kernel().exec_engine.borrow_mut().process(&event);
                    }
                    other => panic!("unexpected recovery event: {other:?}"),
                }
            }
        }

        fn reports(&mut self) -> Vec<ExecutionReport> {
            let mut reports = Vec::new();

            while let Ok(event) = self.rx.try_recv() {
                let ExecutionEvent::Report(report) = event else {
                    panic!("expected execution report, was {event:?}");
                };

                reports.push(report);
            }

            reports
        }

        fn bundle(&mut self) -> (OrderStatusReport, Vec<FillReport>) {
            let mut order = None;
            let mut fills = Vec::new();

            for report in self.reports() {
                match report {
                    ExecutionReport::Order(report) => order = Some(*report),
                    ExecutionReport::Fill(fill) => fills.push(*fill),
                    other => panic!("unexpected recovery report: {other:?}"),
                }
            }

            (order.expect("final order report emitted"), fills)
        }
    }

    impl RecoveryResponses {
        fn response(
            &mut self,
            uri: &axum::http::Uri,
        ) -> (axum::http::StatusCode, serde_json::Value) {
            self.requests.push(uri.to_string());
            let value = if uri.path().ends_with("/fills") {
                return self.response_fills(uri);
            } else if uri.path().ends_with("/orders/historical/batch") {
                self.response_orders()
            } else {
                if let Some(action) = self.on_order.take() {
                    action();
                }

                let order_id = uri.path().rsplit('/').next().unwrap();
                self.orders.get(order_id).unwrap_or(&self.order).clone()
            };

            (axum::http::StatusCode::OK, value)
        }

        fn response_fills(
            &mut self,
            uri: &axum::http::Uri,
        ) -> (axum::http::StatusCode, serde_json::Value) {
            if self.fail_fills {
                return (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    serde_json::Value::Null,
                );
            }

            let mut fills = self.fills.clone();
            let order_id = url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
                .find(|(key, _)| key == "order_ids")
                .map(|(_, value)| value.into_owned());

            if let Some(order_id) = order_id {
                fills = self.fills_targeted.as_ref().unwrap_or(&fills).clone();
                fills["fills"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|fill| fill["order_id"] == order_id);
            } else if let Some(action) = self.on_discovery.take() {
                action();
            }

            (axum::http::StatusCode::OK, fills)
        }

        fn response_orders(&self) -> serde_json::Value {
            let orders: Vec<_> = if self.discover_orders {
                std::iter::once(self.order["order"].clone())
                    .chain(self.orders.values().map(|order| order["order"].clone()))
                    .collect()
            } else {
                Vec::new()
            };

            serde_json::json!({"orders": orders, "has_next": false, "cursor": ""})
        }
    }

    impl Drop for RecoverySpec {
        fn drop(&mut self) {
            if let Some(client) = &mut self.client
                && let Some(handler) = client.order_event_handler.take()
            {
                msgbus::unsubscribe_order_events("events.order.*".into(), &handler);
            }

            self.server.abort();
        }
    }

    #[tokio::test]
    async fn test_user_fill_update_preserves_distinct_rest_trades() {
        let mut spec = RecoverySpec::new().await;
        let update = make_user_order_update("1.0", "0", "112.00", "0.10", CbStatus::Filled);

        spec.recovery.handle_update(make_carrier(update)).await;
        let (report, fills) = spec.bundle();
        let rest = spec
            .recovery
            .http_client
            .request_fill_reports(
                report.account_id,
                None,
                Some(report.venue_order_id),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let mut expected = prepare_order_fills(&report, rest).unwrap();
        for (expected, actual) in expected.iter_mut().zip(&fills) {
            expected.report_id = actual.report_id;
            expected.ts_init = actual.ts_init;
        }

        assert_eq!(fills, expected);
        assert_eq!(report.quantity, Quantity::from("1.00000000"));
        assert_eq!(report.filled_qty, Quantity::from("1.00000000"));
        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0].trade_id.as_str(), "trade-alpha");
        assert_eq!(fills[0].last_qty, Quantity::from("0.40000000"));
        assert_eq!(fills[0].last_px, Price::from("100.00"));
        assert_eq!(fills[0].commission.to_string(), "0.04 USD");
        assert_eq!(fills[1].trade_id.as_str(), "trade-beta");
        assert_eq!(fills[1].last_qty, Quantity::from("0.60000000"));
        assert_eq!(fills[1].last_px, Price::from("120.00"));
        assert_eq!(fills[1].commission.to_string(), "0.06 USD");
        assert!(spec.recovery.pending.lock().is_empty());
    }

    #[rstest]
    #[case::missing_fill(false)]
    #[case::request_failure(true)]
    #[tokio::test]
    async fn test_terminal_fill_retries_without_another_user_update(#[case] request_failure: bool) {
        let mut spec = RecoverySpec::new().await;
        spec.recovery
            .order_contexts
            .lock()
            .insert("client-1".into(), make_limit_context());
        let complete = spec.state.lock().fills.clone();
        {
            let mut state = spec.state.lock();
            state.fail_fills = request_failure;
            state.fills["fills"].as_array_mut().unwrap().pop();
        }

        let update = make_user_order_update("1.0", "0", "112.00", "0.10", CbStatus::Filled);
        spec.recovery.handle_update(make_carrier(update)).await;

        assert!(spec.rx.try_recv().is_err());
        assert_eq!(spec.recovery.pending.lock().len(), 1);
        assert_eq!(spec.recovery.order_contexts.lock().len(), 1);

        {
            let mut state = spec.state.lock();
            state.fail_fills = false;
            state.fills = complete;
        }

        spec.recovery.retry().await;
        let (report, fills) = spec.bundle();

        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(fills.len(), 2);
        assert!(spec.recovery.pending.lock().is_empty());
        assert!(spec.recovery.order_contexts.lock().is_empty());
    }

    #[tokio::test]
    async fn test_reconnect_discovers_closed_order_and_fetches_full_history() {
        let mut spec = RecoverySpec::new().await;
        let all = spec.state.lock().fills.clone();
        // Discovery sees only the later fill; the targeted request must recover the earlier partial too
        spec.state.lock().fills["fills"]
            .as_array_mut()
            .unwrap()
            .pop();
        spec.recovery.discover().await.unwrap();
        spec.state.lock().fills = all;
        spec.recovery.retry().await;
        let (report, fills) = spec.bundle();
        let requests = &spec.state.lock().requests;
        let discovery = url::Url::parse(&format!("http://localhost{}", requests[0])).unwrap();
        let target =
            url::Url::parse(&format!("http://localhost{}", requests.last().unwrap())).unwrap();

        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(fills.len(), 2);
        assert!(
            discovery
                .query_pairs()
                .any(|(key, value)| key == "start_sequence_timestamp"
                    && value == "2024-01-01T00:00:00+00:00")
        );
        assert!(
            target
                .query_pairs()
                .any(|(key, value)| key == "order_ids" && value == "venue-1")
        );
        assert!(
            !target
                .query_pairs()
                .any(|(key, _)| key == "start_sequence_timestamp")
        );
    }

    #[tokio::test]
    async fn test_snapshot_and_replay_keep_rest_identity() {
        let mut spec = RecoverySpec::new().await;
        let update = make_user_order_update("1.0", "0", "112.00", "0.10", CbStatus::Filled);
        spec.recovery
            .handle_update(make_carrier_with_kind(update.clone(), true))
            .await;
        let (_, first) = spec.bundle();
        spec.recovery.handle_update(make_carrier(update)).await;
        let (_, replay) = spec.bundle();

        assert_eq!(first.len(), replay.len());

        for (first, mut replay) in first.iter().zip(replay) {
            replay.report_id = first.report_id;
            replay.ts_init = first.ts_init;
            assert_eq!(*first, replay);
        }

        assert_eq!(
            first
                .iter()
                .map(|fill| fill.trade_id.as_str())
                .collect::<Vec<_>>(),
            vec!["trade-alpha", "trade-beta"]
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_prepare_user_order_report_requires_snapshot_context(#[case] snapshot: bool) {
        let update = make_user_order_update("0", "1.0", "0", "0", CbStatus::Open);
        let carrier = make_carrier_with_kind(update, snapshot);
        let mut context = make_limit_context();
        context.post_only = true;
        context.submitted_product_id = Some(Ustr::from("BTC-USDC"));

        let report = prepare_user_order_report(carrier.clone(), Some(context)).unwrap();
        let unknown = prepare_user_order_report(carrier, None);

        assert_eq!(report.price, Some(Price::from("100.00")));
        assert_eq!(
            report.instrument_id,
            InstrumentId::from("BTC-USDC.COINBASE")
        );
        assert_eq!(report.quantity, Quantity::from("1.00000000"));
        assert!(report.post_only);
        assert!(unknown.is_none());
    }

    #[rstest]
    #[case(CbType::Limit, false)]
    #[case(CbType::StopLimit, false)]
    #[case(CbType::Market, true)]
    fn test_prepare_user_order_report_preserves_safety_gate(
        #[case] order_type: CbType,
        #[case] safe: bool,
    ) {
        let mut update = make_user_order_update("0", "1.0", "0", "0", CbStatus::Open);
        update.order_type = order_type;
        let report = prepare_user_order_report(make_carrier(update), None);
        assert_eq!(report.is_some(), safe);
    }

    #[rstest]
    #[case::startup_first(true)]
    #[case::live_first(false)]
    #[tokio::test]
    async fn test_startup_and_reconnect_overlap_apply_each_fill_once(#[case] startup_first: bool) {
        let mut spec = RecoverySpec::new().await;
        let node = recovery_node();
        let kernel = node.kernel();
        let account_id = AccountId::from("COINBASE-001");
        let client_id = ClientId::from("COINBASE-TEST");

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            client_id,
            *COINBASE_VENUE,
            OmsType::Netting,
            account_id,
            AccountType::Cash,
            None,
            kernel.cache.clone(),
        );

        let config = CoinbaseExecutionClientConfig {
            api_key: Some("test-key".into()),
            api_secret: Some("test-secret".into()),
            ..Default::default()
        };

        let mut client = CoinbaseExecutionClient::new(core, config).unwrap();
        client.instruments_cache = Arc::clone(&spec.recovery.instruments);
        kernel
            .exec_engine
            .borrow_mut()
            .register_client(Box::new(client))
            .unwrap();
        let mut manager = ExecutionManager::new(
            kernel.clock.clone(),
            kernel.cache.clone(),
            Default::default(),
        )
        .unwrap();
        let full_order = spec.state.lock().order.clone();
        let full_fills = spec.state.lock().fills.clone();
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = "0.4".into();
            state.order["order"]["average_filled_price"] = "100.00".into();
            state.fills["fills"].as_array_mut().unwrap().remove(0);
        }

        let report = spec
            .recovery
            .http_client
            .request_order_status_report(account_id, None, Some(VenueOrderId::from("venue-1")))
            .await
            .unwrap();
        let fills = spec
            .recovery
            .http_client
            .request_fill_reports(
                account_id,
                None,
                Some(report.venue_order_id),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        let mut mass = ExecutionMassStatus::new(
            client_id,
            account_id,
            *COINBASE_VENUE,
            UnixNanos::default(),
            None,
        );
        mass.add_order_reports(vec![report.clone()]);
        mass.add_fill_reports(fills.clone());
        if startup_first {
            manager.reconcile_execution_mass_status(&mass, &kernel.exec_engine);
        }

        let update = make_user_order_update("0.4", "0.6", "100.00", "0.04", CbStatus::Open);
        spec.recovery.handle_update(make_carrier(update)).await;
        let live_reports = spec.reports();
        apply_recovery_reports(&node, &live_reports);

        if !startup_first {
            manager.reconcile_execution_mass_status(&mass, &kernel.exec_engine);
        }

        {
            let mut state = spec.state.lock();
            state.order = full_order;
            state.fills = full_fills;
        }

        spec.recovery.retry().await;
        let reports = spec.reports();
        apply_recovery_reports(&node, &reports);
        let event_count = kernel
            .cache
            .borrow()
            .order(&ClientOrderId::from("client-1"))
            .unwrap()
            .event_count();
        apply_recovery_reports(&node, &reports);

        let cache = kernel.cache.borrow();
        let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
        let positions = cache.positions_refs(None, None, None, None, None);
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from("1.00000000"));
        assert_eq!(order.event_count(), event_count);
        assert_eq!(
            order
                .trade_ids()
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>(),
            vec!["trade-alpha", "trade-beta"]
        );
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, Quantity::from("1.00000000"));
        assert_eq!(positions[0].avg_px_open, rust_decimal_macros::dec!(112));
        assert_eq!(positions[0].commissions(), vec![Money::from("0.10 USD")]);
        assert_eq!(positions[0].event_count(), 2);
    }

    #[rstest]
    fn test_prepare_user_order_report_retains_trigger_metadata() {
        let mut update = make_user_order_update("0", "1", "0", "0", CbStatus::Open);
        update.order_type = CbType::StopLimit;

        let context = OrderContext {
            price: Some(Price::from("100.50")),
            trigger_price: Some(Price::from("99.00")),
            trigger_type: Some(TriggerType::LastPrice),
            post_only: true,
            submitted_product_id: Some(Ustr::from("BTC-USDC")),
        };

        let report = prepare_user_order_report(make_carrier(update), Some(context)).unwrap();
        assert_eq!(report.price, Some(Price::from("100.50")));
        assert_eq!(report.trigger_price, Some(Price::from("99.00")));
        assert_eq!(report.trigger_type, Some(TriggerType::LastPrice));
        assert_eq!(
            report.instrument_id,
            InstrumentId::from("BTC-USDC.COINBASE")
        );
        assert!(report.post_only);
    }

    #[rstest]
    #[case::partial_cancel("0.4", CbStatus::Cancelled, "CANCELLED")]
    #[case::empty_cancel("0", CbStatus::Cancelled, "CANCELLED")]
    #[case::empty_expire("0", CbStatus::Expired, "EXPIRED")]
    #[tokio::test]
    async fn test_terminal_recovery_retains_original_qty(
        #[case] cumulative: &str,
        #[case] status: CbStatus,
        #[case] rest_status: &str,
    ) {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = rest_status.into();
            state.order["order"]["filled_size"] = cumulative.into();
            let fills = state.fills["fills"].as_array_mut().unwrap();
            if cumulative == "0" {
                fills.clear();
            } else {
                fills.remove(0);
            }
        }

        let update = make_user_order_update(cumulative, "0", "100", "0.04", status);
        spec.recovery.handle_update(make_carrier(update)).await;
        let (report, fills) = spec.bundle();
        assert_eq!(report.quantity, Quantity::from("1.00000000"));
        assert_eq!(
            report.filled_qty.as_decimal(),
            cumulative.parse::<Decimal>().unwrap()
        );
        assert_eq!(fills.len(), usize::from(cumulative != "0"));
    }

    #[tokio::test]
    async fn test_recovery_uses_rest_alias_and_fee_currency_after_context_removal() {
        let mut spec = RecoverySpec::new().await;

        let InstrumentAny::CurrencyPair(mut pair) = test_instrument() else {
            unreachable!()
        };

        pair.id = InstrumentId::from("BTC-USDC.COINBASE");
        pair.raw_symbol = Symbol::from("BTC-USDC");
        pair.quote_currency = Currency::USDC();
        let instrument = InstrumentAny::CurrencyPair(pair);
        spec.recovery.http_client.cache_instrument(&instrument);
        spec.recovery.instruments = Arc::new(AHashMap::from([("BTC-USDC".into(), instrument)]));
        {
            let mut state = spec.state.lock();
            state.order["order"]["product_id"] = "BTC-USDC".into();
            for fill in state.fills["fills"].as_array_mut().unwrap() {
                fill["product_id"] = "BTC-USDC".into();
            }
        }

        let update = make_user_order_update("1", "0", "112", "0.10", CbStatus::Filled);
        spec.recovery
            .handle_update(make_carrier(update.clone()))
            .await;
        let (first_order, first_fills) = spec.bundle();
        spec.recovery.handle_update(make_carrier(update)).await;
        let (replay_order, replay_fills) = spec.bundle();

        assert_eq!(
            first_order.instrument_id,
            InstrumentId::from("BTC-USDC.COINBASE")
        );
        assert_eq!(replay_order.instrument_id, first_order.instrument_id);
        assert_eq!(first_fills.len(), 2);
        assert_eq!(replay_fills.len(), 2);

        for (first, replay) in first_fills.iter().zip(&replay_fills) {
            assert_eq!(first.instrument_id, first_order.instrument_id);
            assert_eq!(replay.instrument_id, first.instrument_id);
            assert_eq!(first.commission.currency, Currency::USDC());
            assert_eq!(replay.commission, first.commission);
            assert_eq!(replay.trade_id, first.trade_id);
        }
    }

    #[tokio::test]
    async fn test_recovery_deduplicates_rest_rows_before_coverage() {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            let duplicate = state.fills["fills"][1].clone();
            state.fills["fills"].as_array_mut().unwrap().push(duplicate);
        }

        let update = make_user_order_update("1", "0", "112", "0.10", CbStatus::Filled);
        spec.recovery.handle_update(make_carrier(update)).await;
        let (_, fills) = spec.bundle();
        assert_eq!(fills.len(), 2);
        assert_eq!(
            fills
                .iter()
                .map(|fill| fill.last_qty.as_decimal())
                .sum::<Decimal>(),
            rust_decimal_macros::dec!(1)
        );
    }

    #[tokio::test]
    async fn test_discovery_does_not_hide_an_unparsable_fill() {
        let mut spec = RecoverySpec::new().await;
        spec.state.lock().fills["fills"][0]["size"] = "invalid".into();
        spec.recovery.retry().await;
        assert_eq!(spec.recovery.pending.lock().len(), 1);
        assert!(spec.rx.try_recv().is_err());

        spec.state.lock().fills["fills"][0]["size"] = "0.6".into();
        spec.recovery.retry().await;
        let (_, fills) = spec.bundle();
        assert_eq!(fills.len(), 2);
        assert!(spec.recovery.pending.lock().is_empty());
    }

    #[tokio::test]
    async fn test_pending_terminal_is_not_replaced_by_stale_update() {
        let spec = RecoverySpec::new().await;
        let terminal = make_carrier(make_user_order_update(
            "1",
            "0",
            "112",
            "0.10",
            CbStatus::Filled,
        ));
        let stale = make_carrier(make_user_order_update(
            "0.4",
            "0.6",
            "100",
            "0.04",
            CbStatus::Open,
        ));
        spec.recovery.queue(*terminal.report.clone());
        spec.recovery.queue(*stale.report);
        let pending = spec.recovery.pending.lock();
        assert_eq!(pending[&VenueOrderId::from("venue-1")], *terminal.report);
    }

    #[tokio::test]
    async fn test_recovery_timer_publishes_pending_terminal_without_user_message() {
        let mut spec = RecoverySpec::new().await;
        let carrier = make_carrier(make_user_order_update(
            "1",
            "0",
            "112",
            "0.10",
            CbStatus::Filled,
        ));
        spec.recovery.queue(*carrier.report);
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let reports = {
            let run = spec.recovery.run(
                rx,
                Duration::from_millis(1),
                get_atomic_clock_realtime(),
                false,
            );
            tokio::pin!(run);

            let collect = async {
                let mut reports = Vec::new();

                loop {
                    let ExecutionEvent::Report(report) = spec.rx.recv().await.unwrap() else {
                        panic!("expected execution report");
                    };

                    let complete = matches!(&report, ExecutionReport::Fill(fill) if fill.trade_id.as_str() == "trade-beta");
                    reports.push(report);
                    if complete {
                        return reports;
                    }
                }
            };

            tokio::select! {
                () = &mut run => panic!("recovery loop stopped while the input channel is open"),
                result = tokio::time::timeout(Duration::from_secs(2), collect) => result.unwrap(),
            }
        };

        assert_eq!(reports.len(), 3);

        let trade_ids: Vec<_> = reports
            .iter()
            .filter_map(|report| match report {
                ExecutionReport::Fill(fill) => Some(fill.trade_id.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(trade_ids, vec!["trade-alpha", "trade-beta"]);
        assert!(spec.recovery.pending.lock().is_empty());
    }

    #[tokio::test]
    async fn test_recovery_preserves_new_target_queued_during_request() {
        let mut spec = RecoverySpec::new().await;
        let full_order = spec.state.lock().order.clone();
        let full_fills = spec.state.lock().fills.clone();
        let partial = make_carrier(make_user_order_update(
            "0.4",
            "0.6",
            "100",
            "0.04",
            CbStatus::Open,
        ));
        let terminal = make_carrier(make_user_order_update(
            "1",
            "0",
            "112",
            "0.10",
            CbStatus::Filled,
        ));
        spec.recovery.queue(*partial.report);
        let pending = Arc::clone(&spec.recovery.pending);
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = "0.4".into();
            state.fills["fills"].as_array_mut().unwrap().remove(0);
            state.on_order = Some(Box::new(move || {
                pending
                    .lock()
                    .insert(terminal.report.venue_order_id, *terminal.report);
            }));
        }

        let error = spec.recovery.recover().await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "recovery target changed while reading REST"
        );
        assert!(spec.rx.try_recv().is_err());
        assert_eq!(
            spec.recovery.pending.lock()[&VenueOrderId::from("venue-1")].filled_qty,
            Quantity::from("1.00000000")
        );

        {
            let mut state = spec.state.lock();
            state.order = full_order;
            state.fills = full_fills;
        }

        spec.recovery.retry().await;
        let (terminal, fills) = spec.bundle();
        assert_eq!(terminal.order_status, OrderStatus::Filled);
        assert_eq!(fills.len(), 2);
        assert!(spec.recovery.pending.lock().is_empty());
    }

    #[tokio::test]
    async fn test_recovery_retains_pending_on_closed_event_receiver() {
        let mut spec = RecoverySpec::new().await;
        spec.rx.close();
        let carrier = make_carrier(make_user_order_update(
            "1",
            "0",
            "112",
            "0.10",
            CbStatus::Filled,
        ));
        spec.recovery.handle_update(carrier).await;
        assert_eq!(spec.recovery.pending.lock().len(), 1);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        spec.recovery.emitter.set_sender(tx);
        spec.rx = rx;
        spec.recovery.retry().await;
        let (terminal, fills) = spec.bundle();
        assert_eq!(terminal.order_status, OrderStatus::Filled);
        assert_eq!(fills.len(), 2);
        assert!(spec.recovery.pending.lock().is_empty());
    }

    #[tokio::test]
    async fn test_server_time_uses_venue_timestamp_and_rejects_missing_field() {
        let spec = RecoverySpec::new().await;
        spec.state.lock().order["iso"] = "2024-01-02T03:04:05.123Z".into();
        let time = spec
            .recovery
            .http_client
            .request_server_time()
            .await
            .unwrap();
        assert_eq!(
            time,
            crate::http::parse::parse_rfc3339_timestamp("2024-01-02T03:04:05.123Z").unwrap()
        );

        spec.state.lock().order["iso"] = serde_json::Value::Null;
        let error = spec
            .recovery
            .http_client
            .request_server_time()
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "server time response is missing iso");
    }

    #[rstest]
    #[case::older_partial("1", CbStatus::Filled)]
    #[case::active_before_cancel("0.4", CbStatus::Cancelled)]
    #[tokio::test]
    async fn test_recovery_waits_for_rest_to_reach_user_state(
        #[case] expected_qty: &str,
        #[case] status: CbStatus,
    ) {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = "0.4".into();
            state.fills["fills"].as_array_mut().unwrap().remove(0);
        }

        let carrier = make_carrier(make_user_order_update(
            expected_qty,
            "0",
            "112",
            "0.10",
            status,
        ));
        spec.recovery.queue(*carrier.report);
        let error = spec.recovery.recover().await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "REST order state has not reached the user-channel update"
        );
        assert_eq!(spec.recovery.pending.lock().len(), 1);
        assert!(spec.rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_query_order_uses_ordered_rest_recovery() {
        let mut spec = RecoverySpec::new().await;
        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from("COINBASE-TEST"),
            *COINBASE_VENUE,
            OmsType::Netting,
            AccountId::from("COINBASE-001"),
            AccountType::Cash,
            None,
            cache,
        );

        let config = CoinbaseExecutionClientConfig {
            api_key: Some("test-key".into()),
            api_secret: Some("test-secret".into()),
            ..Default::default()
        };

        let mut client = CoinbaseExecutionClient::new(core, config).unwrap();
        client.http_client = spec.recovery.http_client.clone();
        client.emitter = spec.recovery.emitter.clone();
        client.instruments_cache = Arc::clone(&spec.recovery.instruments);

        let command = QueryOrder::new(
            TraderId::from("TRADER-001"),
            None,
            StrategyId::from("S-001"),
            InstrumentId::from("BTC-USD.COINBASE"),
            ClientOrderId::from("client-1"),
            Some(VenueOrderId::from("venue-1")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.query_order(command).unwrap();
        tokio::time::timeout(Duration::from_secs(2), client.await_pending_tasks())
            .await
            .unwrap()
            .unwrap();
        let (report, fills) = spec.bundle();

        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(
            fills
                .iter()
                .map(|fill| fill.trade_id.as_str())
                .collect::<Vec<_>>(),
            vec!["trade-alpha", "trade-beta"]
        );
        client.await_pending_tasks().await.unwrap();
    }

    #[rstest]
    #[case::visible(false)]
    #[case::delayed_closing_fill(true)]
    #[tokio::test]
    async fn test_recovery_orders_interleaved_fills_across_orders(#[case] delayed: bool) {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            let mut closing = state.order.clone();
            closing["order"]["order_id"] = "venue-2".into();
            closing["order"]["client_order_id"] = "client-2".into();
            closing["order"]["side"] = "SELL".into();
            closing["order"]["filled_size"] = "0.4".into();
            closing["order"]["average_filled_price"] = "110.00".into();
            closing["order"]["last_fill_time"] = "2024-01-15T10:01:00Z".into();
            closing["order"]["order_configuration"]["limit_limit_gtc"]["base_size"] = "0.4".into();
            state.orders.insert("venue-2".into(), closing);
            state.order["order"]["last_fill_time"] = "2024-01-15T10:02:00Z".into();
            state.fills["fills"][1]["trade_time"] = "2024-01-15T10:00:00Z".into();
            state.fills["fills"][0]["trade_time"] = "2024-01-15T10:02:00Z".into();
            let mut fill = state.fills["fills"][1].clone();
            fill["order_id"] = "venue-2".into();
            fill["trade_id"] = "trade-gamma".into();
            fill["side"] = "SELL".into();
            fill["price"] = "110.00".into();
            fill["commission"] = "0.08".into();
            fill["trade_time"] = "2024-01-15T10:01:00Z".into();
            state.fills["fills"].as_array_mut().unwrap().insert(0, fill);
        }

        let node = recovery_node();

        if delayed {
            let closing = spec.state.lock().fills["fills"]
                .as_array_mut()
                .unwrap()
                .remove(0);
            spec.recovery.retry().await;
            assert!(spec.rx.try_recv().is_err());
            assert_eq!(spec.recovery.pending.lock().len(), 2);
            spec.state.lock().fills["fills"]
                .as_array_mut()
                .unwrap()
                .push(closing);
        }

        spec.recovery.retry().await;
        let reports = spec.reports();

        let trades: Vec<_> = reports
            .iter()
            .filter_map(|report| match report {
                ExecutionReport::Fill(fill) => Some(fill.trade_id.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(trades, vec!["trade-alpha", "trade-gamma", "trade-beta"]);
        apply_recovery_reports(&node, &reports);
        let events_before: Vec<_> = ["client-1", "client-2"]
            .iter()
            .map(|id| {
                node.kernel()
                    .cache
                    .borrow()
                    .order(&ClientOrderId::from(*id))
                    .unwrap()
                    .event_count()
            })
            .collect();

        spec.recovery.retry().await;
        apply_recovery_reports(&node, &spec.reports());
        let cache = node.kernel().cache.borrow();
        let positions = cache.positions_open_refs(None, None, None, None, None);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, Quantity::from("0.600000"));
        assert_eq!(positions[0].avg_px_open, rust_decimal_macros::dec!(120));

        for (id, events) in ["client-1", "client-2"].iter().zip(events_before) {
            let order = cache.order(&ClientOrderId::from(*id)).unwrap();
            assert_eq!(order.status(), OrderStatus::Filled);
            assert_eq!(order.event_count(), events);
            assert_eq!(
                order
                    .events()
                    .iter()
                    .filter(|event| matches!(event, OrderEventAny::FillVoided(_)))
                    .count(),
                0
            );
        }
    }

    #[rstest]
    #[case::already_published(true)]
    #[case::startup_only(false)]
    #[tokio::test]
    async fn test_stale_recovery_cannot_void_newer_fills(#[case] published: bool) {
        let mut spec = RecoverySpec::new().await;
        let node = recovery_node();
        spec.recovery.retry().await;
        apply_recovery_reports(&node, &spec.reports());
        if !published {
            spec.recovery.published.lock().await.clear();
        }

        let before = node
            .kernel()
            .cache
            .borrow()
            .order(&ClientOrderId::from("client-1"))
            .unwrap()
            .event_count();
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = "0.4".into();
            state.order["order"]["average_filled_price"] = "100.00".into();
            state.fills["fills"].as_array_mut().unwrap().remove(0);
        }

        let partial = make_carrier(make_user_order_update(
            "0.4",
            "0.6",
            "100",
            "0.04",
            CbStatus::Open,
        ));
        spec.recovery.queue(*partial.report);
        let result = spec.recovery.recover().await;

        if published {
            assert_eq!(
                result.unwrap_err().to_string(),
                "REST order state precedes published recovery"
            );
            assert!(spec.rx.try_recv().is_err());
        } else {
            result.unwrap();
            apply_recovery_reports(&node, &spec.reports());
        }

        let cache = node.kernel().cache.borrow();
        let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
        let positions = cache.positions_open_refs(None, None, None, None, None);
        assert_eq!(order.filled_qty(), Quantity::from("1.000000"));
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.event_count(), before);
        assert_eq!(
            order
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::FillVoided(_)))
                .count(),
            0
        );
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, Quantity::from("1.000000"));
        assert_eq!(positions[0].avg_px_open, rust_decimal_macros::dec!(112));
    }

    #[rstest]
    #[case::closed_order(false)]
    #[case::query_pending(true)]
    #[tokio::test]
    async fn test_idle_recovery_discovers_fills_visible_after_empty_scan(
        #[case] query_pending: bool,
    ) {
        let mut spec = RecoverySpec::new().await;
        let complete = spec.state.lock().fills.clone();
        let scanned = Arc::new(tokio::sync::Notify::new());
        {
            let scanned = Arc::clone(&scanned);
            let mut state = spec.state.lock();
            state.fills["fills"] = serde_json::Value::Array(Vec::new());
            state.discover_orders = false;
            state.on_discovery = Some(Box::new(move || scanned.notify_one()));
        }

        let state = Arc::clone(&spec.state);
        let pending = Arc::clone(&spec.recovery.pending);
        let target = make_carrier(make_user_order_update(
            "1",
            "0",
            "112",
            "0.10",
            CbStatus::Filled,
        ));

        let make_visible = async move {
            scanned.notified().await;
            state.lock().fills = complete;

            if query_pending {
                pending
                    .lock()
                    .insert(target.report.venue_order_id, *target.report);
            }
        };

        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let run = spec.recovery.run(
            rx,
            Duration::from_millis(1),
            get_atomic_clock_realtime(),
            false,
        );

        let collect = async {
            make_visible.await;
            let mut trades = Vec::new();

            loop {
                let ExecutionEvent::Report(report) = spec.rx.recv().await.unwrap() else {
                    panic!("expected execution report");
                };

                match report {
                    ExecutionReport::Fill(fill) => {
                        trades.push(fill.trade_id);
                        if trades.len() == 2 {
                            return trades;
                        }
                    }
                    ExecutionReport::Order(_) => {}
                    other => panic!("unexpected recovery report: {other:?}"),
                }
            }
        };

        tokio::pin!(run);
        let trades = tokio::select! {
            () = &mut run => panic!("recovery loop stopped"),
            result = tokio::time::timeout(Duration::from_secs(2), collect) => result.unwrap(),
        };
        assert_eq!(
            trades.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
            vec!["trade-alpha", "trade-beta"]
        );
    }

    #[rstest]
    #[case::zero_total("0", false)]
    #[case::partial_total("0.4", true)]
    #[case::empty_targeted_history("0", true)]
    #[tokio::test]
    async fn test_discovered_trades_survive_stale_totals_and_later_empty_scans(
        #[case] filled_qty: &str,
        #[case] targeted: bool,
    ) {
        let mut spec = RecoverySpec::new().await;
        let complete = spec.state.lock().fills.clone();
        let order = spec.state.lock().order.clone();
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = filled_qty.into();

            if targeted {
                let mut fills = complete.clone();
                fills["fills"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|fill| filled_qty == "0.4" && fill["trade_id"] == "trade-alpha");
                state.fills_targeted = Some(fills);
            }
        }

        spec.recovery.retry().await;
        assert!(spec.rx.try_recv().is_err());
        assert_eq!(
            spec.recovery.observed.lock()[&VenueOrderId::from("venue-1")].len(),
            2
        );
        spec.state.lock().fills["fills"] = serde_json::Value::Array(Vec::new());
        spec.recovery.retry().await;
        assert!(spec.rx.try_recv().is_err());
        assert_eq!(
            spec.recovery.observed.lock()[&VenueOrderId::from("venue-1")].len(),
            2
        );
        {
            let mut state = spec.state.lock();
            state.order = order;
            state.fills = complete;
            state.fills_targeted = None;
        }

        spec.recovery.retry().await;
        let (report, fills) = spec.bundle();
        assert_eq!(report.filled_qty, Quantity::from("1.00000000"));
        assert_eq!(
            fills
                .iter()
                .map(|fill| fill.trade_id.as_str())
                .collect::<Vec<_>>(),
            vec!["trade-alpha", "trade-beta"]
        );
        assert!(spec.recovery.observed.lock().is_empty());
        assert!(spec.recovery.pending.lock().is_empty());
    }

    #[tokio::test]
    async fn test_recovery_preserves_newer_cached_amendment() {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = "0.4".into();
            state.order["order"]["average_filled_price"] = "100.00".into();
            state.fills["fills"].as_array_mut().unwrap().remove(0);
        }

        let node = recovery_node();
        spec.attach(&node);
        spec.recovery.retry().await;
        apply_recovery_reports(&node, &spec.reports());
        let mut amended = spec
            .recovery
            .http_client
            .request_order_status_report(
                AccountId::from("COINBASE-001"),
                None,
                Some(VenueOrderId::from("venue-1")),
            )
            .await
            .unwrap();
        amended.quantity = Quantity::from("2.00000000");
        amended.price = Some(Price::from("10100.00"));
        amended.ts_last = UnixNanos::from(amended.ts_last.as_u64() + 1);
        node.kernel()
            .exec_engine
            .borrow_mut()
            .reconcile_order_status_report(&amended);
        {
            let cache = node.kernel().cache.borrow();
            let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
            assert_eq!(order.quantity(), amended.quantity);
            assert_eq!(order.price(), amended.price);
        }

        spec.recovery.retry().await;
        apply_recovery_reports(&node, &spec.reports());
        let cache = node.kernel().cache.borrow();
        let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
        assert_eq!(order.quantity(), amended.quantity);
        assert_eq!(order.price(), amended.price);
        assert_eq!(order.filled_qty(), Quantity::from("0.40000000"));
    }

    #[tokio::test]
    async fn test_external_recovery_emits_order_status_before_real_fills() {
        let mut spec = RecoverySpec::new().await;
        spec.recovery.retry().await;
        let reports = spec.reports();

        let ExecutionReport::Order(accepted) = &reports[0] else {
            panic!("order status must precede external fills")
        };

        let ExecutionReport::Order(terminal) = &reports[3] else {
            panic!("final order status must follow external fills")
        };

        let fills: Vec<_> = reports[1..3]
            .iter()
            .map(|report| {
                let ExecutionReport::Fill(fill) = report else {
                    panic!("expected canonical fill report")
                };

                assert_eq!(fill.account_id, accepted.account_id);
                assert_eq!(fill.instrument_id, accepted.instrument_id);
                assert_eq!(fill.venue_order_id, accepted.venue_order_id);
                fill.trade_id
            })
            .collect();

        assert_eq!(reports.len(), 4);
        assert_eq!(accepted.order_status, OrderStatus::Accepted);
        assert_eq!(accepted.quantity, terminal.quantity);
        assert_eq!(accepted.price, terminal.price);
        assert_eq!(accepted.filled_qty, Quantity::from("0.00000000"));
        assert_eq!(accepted.avg_px, None);
        assert_eq!(accepted.ts_last, accepted.ts_accepted);
        assert_eq!(terminal.order_status, OrderStatus::Filled);
        assert_eq!(terminal.filled_qty, Quantity::from("1.00000000"));
        assert_eq!(
            fills,
            vec![TradeId::from("trade-alpha"), TradeId::from("trade-beta")]
        );
        let node = recovery_node();
        apply_recovery_reports(&node, &reports);
        let cache = node.kernel().cache.borrow();
        let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), terminal.filled_qty);
        assert_eq!(
            order.trade_ids().into_iter().copied().collect::<Vec<_>>(),
            fills
        );
    }

    #[rstest]
    #[case::partial_cancel("CANCELLED", OrderStatus::Canceled, "0.4")]
    #[case::partial_expire("EXPIRED", OrderStatus::Expired, "0.4")]
    #[case::empty_cancel("CANCELLED", OrderStatus::Canceled, "0")]
    #[case::empty_expire("EXPIRED", OrderStatus::Expired, "0")]
    #[tokio::test]
    async fn test_unknown_recovery_applies_native_terminal_after_fills(
        #[case] status: &str,
        #[case] expected: OrderStatus,
        #[case] filled: &str,
        #[values(false, true)] known: bool,
        #[values(false, true)] missing_client_id: bool,
    ) {
        let mut spec = RecoverySpec::new().await;

        let client_order_id = ClientOrderId::from(if missing_client_id {
            "venue-1"
        } else {
            "client-1"
        });

        if missing_client_id {
            spec.state.lock().order["order"]["client_order_id"] = "".into();
        }

        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = status.into();
            state.order["order"]["filled_size"] = filled.into();
            state.order["order"]["average_filled_price"] = "100".into();
            let fills = state.fills["fills"].as_array_mut().unwrap();
            fills.remove(0);
            if filled == "0" {
                fills.clear();
            }
        }

        let node = recovery_node();

        if known {
            let mut report = spec
                .recovery
                .http_client
                .request_order_status_report(
                    AccountId::from("COINBASE-001"),
                    None,
                    Some(VenueOrderId::from("venue-1")),
                )
                .await
                .unwrap();
            report.quantity = Quantity::from("2.00000000");
            report.price = Some(Price::from("10100.00"));
            report.filled_qty = Quantity::from("0.00000000");
            report.avg_px = None;
            report.order_status = OrderStatus::Accepted;
            node.kernel()
                .exec_engine
                .borrow_mut()
                .reconcile_order_status_report(&report);
        }

        spec.attach(&node);

        let status = if expected == OrderStatus::Canceled {
            CbStatus::Cancelled
        } else {
            CbStatus::Expired
        };

        let mut update = make_user_order_update(filled, "0", "100", "0.04", status);
        if missing_client_id {
            update.client_order_id.clear();
        }

        let carrier = make_carrier(update);
        spec.recovery.handle_update(carrier).await;
        spec.apply(&node);
        let before = node
            .kernel()
            .cache
            .borrow()
            .order(&client_order_id)
            .unwrap()
            .event_count();
        spec.recovery.retry().await;
        spec.apply(&node);
        let cache = node.kernel().cache.borrow();
        let order = cache.order(&client_order_id).unwrap();
        assert_eq!(order.status(), expected);
        assert_eq!(
            order.quantity(),
            Quantity::from(if known { "2.00000000" } else { "1.00000000" })
        );
        assert_eq!(
            order.price(),
            Some(Price::from(if known { "10100.00" } else { "10000.00" }))
        );
        assert_eq!(
            order.filled_qty().as_decimal(),
            filled.parse::<Decimal>().unwrap()
        );
        assert_eq!(order.event_count(), before);
        assert_eq!(order.trade_ids().len(), usize::from(filled != "0"));
        assert!(spec.recovery.lifecycle.lock().pending.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_recovery_accepts_cached_submitted_order_without_replacing_terms(
        #[values(false, true)] fills_lag: bool,
    ) {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = if fills_lag { "0.4" } else { "0" }.into();
            if fills_lag {
                state.fills["fills"].as_array_mut().unwrap().remove(0);
                state.fail_fills = true;
            } else {
                state.fills["fills"].as_array_mut().unwrap().clear();
            }
        }

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("STRATEGY-007"))
            .instrument_id(InstrumentId::from("BTC-USD.COINBASE"))
            .client_order_id(ClientOrderId::from("client-1"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.00000000"))
            .price(Price::from("10100.00"))
            .build();

        let factory = OrderEventFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
            AccountType::Cash,
            None,
        );
        let submitted = factory.generate_order_submitted(&order, UnixNanos::from(10));
        order.apply(submitted).unwrap();
        let node = recovery_node();
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        spec.attach(&node);
        let carrier = make_carrier(make_user_order_update(
            if fills_lag { "0.4" } else { "0" },
            if fills_lag { "0.6" } else { "1" },
            "100",
            "0.04",
            CbStatus::Open,
        ));
        spec.recovery.handle_update(carrier).await;
        spec.apply(&node);
        {
            let cache = node.kernel().cache.borrow();
            let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
            assert_eq!(order.status(), OrderStatus::Accepted);
            assert_eq!(order.quantity(), Quantity::from("2.00000000"));
            assert_eq!(order.price(), Some(Price::from("10100.00")));
            assert_eq!(order.filled_qty(), Quantity::from("0.00000000"));
        }

        if fills_lag {
            assert_eq!(spec.recovery.pending.lock().len(), 1);
        } else {
            assert!(spec.recovery.pending.lock().is_empty());
        }
    }

    #[rstest]
    #[case::cancel(OrderStatus::PendingCancel)]
    #[case::update(OrderStatus::PendingUpdate)]
    #[tokio::test]
    async fn test_recovery_preserves_pending_command_on_unchanged_open_report(
        #[case] expected_status: OrderStatus,
    ) {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = "0".into();
            state.fills["fills"].as_array_mut().unwrap().clear();
        }

        let report = spec
            .recovery
            .http_client
            .request_order_status_report(
                AccountId::from("COINBASE-001"),
                None,
                Some(VenueOrderId::from("venue-1")),
            )
            .await
            .unwrap();
        let node = recovery_node();
        node.kernel()
            .exec_engine
            .borrow_mut()
            .reconcile_order_status_report(&report);
        let order = node
            .kernel()
            .cache
            .borrow()
            .order(&ClientOrderId::from("client-1"))
            .unwrap()
            .clone();
        let ts = UnixNanos::from(report.ts_last.as_u64() + 1);

        let pending = match expected_status {
            OrderStatus::PendingCancel => OrderEventAny::PendingCancel(OrderPendingCancel::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                order.account_id(),
                UUID4::new(),
                ts,
                ts,
                false,
                order.venue_order_id(),
            )),
            OrderStatus::PendingUpdate => OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                order.account_id(),
                UUID4::new(),
                ts,
                ts,
                false,
                order.venue_order_id(),
            )),
            _ => unreachable!(),
        };

        node.kernel().exec_engine.borrow_mut().process(&pending);
        let before = node
            .kernel()
            .cache
            .borrow()
            .order(&order.client_order_id())
            .unwrap()
            .event_count();
        node.kernel()
            .exec_engine
            .borrow_mut()
            .reconcile_order_status_report(&report);
        {
            let cache = node.kernel().cache.borrow();
            let order = cache.order(&order.client_order_id()).unwrap();
            assert_eq!(order.status(), expected_status);
            assert_eq!(order.event_count(), before);
        }

        spec.attach(&node);
        spec.recovery.queue(report);
        spec.recovery.retry().await;
        spec.apply(&node);
        let cache = node.kernel().cache.borrow();
        let order = cache.order(&order.client_order_id()).unwrap();
        assert_eq!(order.status(), expected_status);
        assert_eq!(order.event_count(), before);
        assert_eq!(order.quantity(), Quantity::from("1.00000000"));
        assert_eq!(order.price(), Some(Price::from("10000.00")));
        assert_eq!(order.filled_qty(), Quantity::from("0.00000000"));
        assert!(order.trade_ids().is_empty());
        assert!(
            cache
                .positions_refs(None, None, None, None, None)
                .is_empty()
        );
    }

    #[tokio::test]
    async fn test_partial_user_amendment_survives_snapshot_and_rest_replay() {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = "OPEN".into();
            state.order["order"]["filled_size"] = "0.4".into();
            state.order["order"]["average_filled_price"] = "100".into();
            state.fills["fills"].as_array_mut().unwrap().remove(0);
        }

        let node = recovery_node();
        spec.attach(&node);
        spec.recovery.retry().await;
        spec.apply(&node);

        let context = OrderContext {
            price: Some(Price::from("10100.00")),
            ..Default::default()
        };

        spec.recovery
            .order_contexts
            .lock()
            .insert("client-1".into(), context);
        let update = make_user_order_update("0.4", "1.6", "100", "0.04", CbStatus::Open);
        spec.recovery.handle_update(make_carrier(update)).await;
        spec.apply(&node);
        {
            let cache = node.kernel().cache.borrow();
            let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
            assert_eq!(order.quantity(), Quantity::from("2.00000000"));
            assert_eq!(order.price(), Some(Price::from("10100.00")));
        }

        let stale = make_user_order_update("0.4", "0.6", "100", "0.04", CbStatus::Open);
        spec.recovery
            .handle_update(make_carrier_with_kind(stale, true))
            .await;
        spec.apply(&node);
        let cache = node.kernel().cache.borrow();
        let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
        assert_eq!(order.quantity(), Quantity::from("2.00000000"));
        assert_eq!(order.price(), Some(Price::from("10100.00")));
        assert_eq!(order.filled_qty(), Quantity::from("0.40000000"));
        assert_eq!(
            order.trade_ids().into_iter().copied().collect::<Vec<_>>(),
            vec![TradeId::from("trade-alpha")]
        );
    }

    #[rstest]
    #[case::partial("OPEN", "2", "0.5", OrderStatus::PartiallyFilled)]
    #[case::filled_increase("FILLED", "1", "0.5", OrderStatus::Filled)]
    #[case::filled_decrease("FILLED", "1", "2", OrderStatus::Filled)]
    #[tokio::test]
    async fn test_recovery_updates_missed_quantity_amendment_before_fills(
        #[case] status: &str,
        #[case] quantity: &str,
        #[case] cached_quantity: &str,
        #[case] expected_status: OrderStatus,
    ) {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["status"] = status.into();
            state.order["order"]["order_configuration"]["limit_limit_gtc"]["base_size"] =
                quantity.into();
        }

        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("STRATEGY-007"))
            .instrument_id(InstrumentId::from("BTC-USD.COINBASE"))
            .client_order_id(ClientOrderId::from("client-1"))
            .side(OrderSide::Buy)
            .quantity(cached_quantity.parse().unwrap())
            .price(Price::from("10100.00"))
            .build();

        let factory = OrderEventFactory::new(
            TraderId::from("TRADER-001"),
            AccountId::from("COINBASE-001"),
            AccountType::Cash,
            None,
        );
        let accepted = factory.generate_order_accepted(
            &order,
            VenueOrderId::from("venue-1"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        order.apply(accepted).unwrap();
        let before = order.event_count();
        let node = recovery_node();
        node.kernel()
            .cache
            .borrow_mut()
            .add_order(order, None, None, false)
            .unwrap();
        spec.attach(&node);
        spec.recovery.retry().await;
        spec.apply(&node);
        let cache = node.kernel().cache.borrow();
        let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
        let events = order.events();
        let update_index = events
            .iter()
            .position(|event| matches!(event, OrderEventAny::Updated(_)))
            .unwrap();
        let fill_index = events
            .iter()
            .position(|event| matches!(event, OrderEventAny::Filled(_)))
            .unwrap();
        let positions = cache.positions_open_refs(None, None, None, None, None);
        assert!(update_index < fill_index);
        assert_eq!(order.status(), expected_status);
        assert_eq!(
            order.quantity().as_decimal(),
            quantity.parse::<Decimal>().unwrap()
        );
        assert_eq!(order.price(), Some(Price::from("10100.00")));
        assert_eq!(order.filled_qty(), Quantity::from("1.00000000"));
        assert_eq!(order.event_count(), before + 3);
        assert_eq!(
            order.trade_ids().into_iter().copied().collect::<Vec<_>>(),
            vec![TradeId::from("trade-alpha"), TradeId::from("trade-beta")]
        );
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, Quantity::from("1.00000000"));
        assert_eq!(positions[0].avg_px_open, Decimal::from(112));
        assert_eq!(positions[0].commissions(), vec![Money::from("0.10 USD")]);
    }

    #[rstest]
    #[tokio::test]
    async fn test_quote_market_recovery_normalizes_base_units_after_terminal_totals(
        #[values(false, true)] known: bool,
        #[values(false, true)] snapshot: bool,
        #[values(false, true)] partial: bool,
    ) {
        let mut spec = RecoverySpec::new().await;
        {
            let mut state = spec.state.lock();
            state.order["order"]["order_type"] = "MARKET".into();
            state.order["order"]["time_in_force"] = "IMMEDIATE_OR_CANCEL".into();
            state.order["order"]["order_configuration"] =
                serde_json::json!({"market_market_ioc": {"quote_size": "112"}});
            state.order["order"]["size_in_quote"] = true.into();
            state.fills["fills"][0]["size_in_quote"] = true.into();
            state.fills["fills"][0]["size"] = "72".into();
            state.fills["fills"][1]["size_in_quote"] = true.into();
            state.fills["fills"][1]["size"] = "40".into();
        }

        let node = recovery_node();

        if known {
            let mut order = OrderTestBuilder::new(OrderType::Market)
                .trader_id(TraderId::from("TRADER-001"))
                .strategy_id(StrategyId::from("STRATEGY-007"))
                .instrument_id(InstrumentId::from("BTC-USD.COINBASE"))
                .client_order_id(ClientOrderId::from("client-1"))
                .side(OrderSide::Buy)
                .quantity(Quantity::from("112.00"))
                .quote_quantity(true)
                .time_in_force(TimeInForce::Ioc)
                .build();

            let factory = OrderEventFactory::new(
                TraderId::from("TRADER-001"),
                AccountId::from("COINBASE-001"),
                AccountType::Cash,
                None,
            );
            let accepted = factory.generate_order_accepted(
                &order,
                VenueOrderId::from("venue-1"),
                UnixNanos::from(1),
                UnixNanos::from(1),
            );
            order.apply(accepted).unwrap();
            node.kernel()
                .cache
                .borrow_mut()
                .add_order(order, None, None, false)
                .unwrap();
            spec.recovery
                .order_contexts
                .lock()
                .insert("client-1".into(), OrderContext::default());
        }

        spec.attach(&node);
        let final_order = spec.state.lock().order.clone();
        let final_fills = spec.state.lock().fills.clone();

        if partial {
            {
                let mut state = spec.state.lock();
                state.order["order"]["status"] = "OPEN".into();
                state.order["order"]["filled_size"] = "0.4".into();
                state.order["order"]["average_filled_price"] = "100".into();
                state.fills["fills"].as_array_mut().unwrap().remove(0);
            }

            let mut update = make_user_order_update("0.4", "0", "100", "0.04", CbStatus::Open);
            update.order_type = CbType::Market;
            spec.recovery.handle_update(make_carrier(update)).await;
            assert!(spec.rx.try_recv().is_err());
            assert_eq!(spec.recovery.pending.lock().len(), 1);
        }

        {
            let mut state = spec.state.lock();
            state.order = final_order;
            state.fills = final_fills;
        }

        let mut update = make_user_order_update("1", "0", "112", "0.10", CbStatus::Filled);
        update.order_type = CbType::Market;
        spec.recovery
            .handle_update(make_carrier_with_kind(update, snapshot))
            .await;
        spec.apply(&node);
        let before = node
            .kernel()
            .cache
            .borrow()
            .order(&ClientOrderId::from("client-1"))
            .unwrap()
            .event_count();
        spec.recovery.retry().await;
        spec.apply(&node);
        let cache = node.kernel().cache.borrow();
        let order = cache.order(&ClientOrderId::from("client-1")).unwrap();
        let positions = cache.positions_refs(None, None, None, None, None);
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.quantity(), Quantity::from("1.00000000"));
        assert_eq!(order.filled_qty(), Quantity::from("1.00000000"));
        assert!(!order.is_quote_quantity());
        assert_eq!(
            order.trade_ids().into_iter().copied().collect::<Vec<_>>(),
            vec![TradeId::from("trade-alpha"), TradeId::from("trade-beta")]
        );
        assert_eq!(order.event_count(), before);
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].quantity, Quantity::from("1.00000000"));
        assert_eq!(positions[0].avg_px_open, Decimal::from(112));
        assert_eq!(positions[0].commissions(), vec![Money::from("0.10 USD")]);
    }

    fn apply_recovery_reports(node: &LiveNode, reports: &[ExecutionReport]) {
        for report in reports {
            node.kernel()
                .exec_engine
                .borrow_mut()
                .reconcile_execution_report(report);
        }
    }

    fn recovery_node() -> LiveNode {
        let node = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Live)
            .unwrap()
            .build()
            .unwrap();
        let kernel = node.kernel();
        kernel
            .cache
            .borrow_mut()
            .add_instrument(test_instrument())
            .unwrap();
        let account_id = AccountId::from("COINBASE-001");

        let state = AccountState::new(
            account_id,
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::from("100000 USD"),
                Money::from("0 USD"),
                Money::from("100000 USD"),
            )],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        kernel
            .cache
            .borrow_mut()
            .add_account(AccountAny::Cash(CashAccount::new(state, true, false)))
            .unwrap();
        node
    }
}
