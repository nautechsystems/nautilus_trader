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

use std::{
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ahash::AHashMap;
use anyhow::Context;
use nautilus_common::{
    live::runner::get_exec_event_sender,
    msgbus::{self, TypedHandler},
};
use nautilus_core::{collections::AtomicMap, string::secret::SecretString, time::AtomicTime};
use nautilus_live::{ExecutionClientCore, execution::context::OrderContext, task::TaskGroupGuard};
use nautilus_model::{
    enums::OrderSide,
    events::{OrderEventAny, PositionEvent},
    identifiers::{ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    types::Money,
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::PolymarketExecutionClient;
use crate::{
    execution::{
        context::OrderContextRegistry,
        reconciliation::venue_leg_filled_before_and_quantity,
        reports::fetch_and_emit_account_state,
        settlement::{SettlementRegistry, UncertainOrder, UncertainOrderKind},
    },
    http::{
        clob::{HeartbeatResponse, PolymarketClobHttpClient},
        error::Error as HttpError,
        models::PolymarketTradeReport,
        query::GetTradesParams,
    },
    websocket::{
        dispatch::{
            WsDispatchContext, WsDispatchState, apply_rest_trade_evidence,
            apply_stream_gap_order_evidence, apply_uncertain_order_evidence, dispatch_user_message,
            emit_void_for_applied_fill,
        },
        messages::PolymarketWsMessage,
    },
};

const SUPPORTED_CLOB_VERSION: u8 = 2;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const HEARTBEAT_REQUEST_TIMEOUT: Duration = Duration::from_secs(4);
const HEARTBEAT_SAFETY_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT_HEALTH_MARGIN: Duration = Duration::from_secs(1);
const HEARTBEAT_REQUEST_FAILURE_LIMIT: u32 = 2;
const TASK_SESSION_GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const TASK_ABORT_TIMEOUT: Duration = Duration::from_secs(2);

/// Sweep interval for retrying targeted terminal REST resolution.
const RESOLUTION_SWEEP_INTERVAL: Duration = Duration::from_millis(500);
/// Delay before the first retry of a trade's targeted REST read.
const RESOLUTION_BACKOFF_INITIAL: Duration = Duration::from_millis(500);
/// Maximum delay between a trade's targeted REST reads.
const RESOLUTION_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// How long targeted REST resolution continues for an order whose venue state is unknown.
const UNCERTAIN_ORDER_RESOLUTION_WINDOW: Duration = Duration::from_secs(600);
/// Maximum targeted REST order reads per sweep, so a reconnect with many open orders cannot cause
/// a read storm.
const UNCERTAIN_ORDER_READS_PER_SWEEP: usize = 10;
/// How far before an uncertain order's venue `created_at` its trades are read, because the venue
/// can stamp an order that matches on submit after its trades' `match_time`.
const UNCERTAIN_ORDER_TRADE_LOOKBACK: Duration = Duration::from_secs(60);

impl PolymarketExecutionClient {
    fn start_heartbeat_task(&self) -> anyhow::Result<()> {
        if !self.config.heartbeat_enabled {
            return Ok(());
        }

        self.heartbeat_healthy.store(false, Ordering::Release);
        let cancellation = self.session_tasks.cancellation_token();

        self.session_tasks.spawn(run_heartbeats(
            self.http_client.clone(),
            cancellation,
            Arc::clone(&self.heartbeat_healthy),
        ))?;
        Ok(())
    }

    fn ensure_order_event_subscription(&mut self) {
        if self.order_event_handler.is_some() {
            return;
        }

        let core = self.core.clone();
        let clock = self.clock;
        let shared_token_instruments = self.shared_token_instruments.clone();
        let neg_risk_index = self.neg_risk_index.clone();
        let order_reservations = self.order_reservations.clone();
        let order_contexts = self.order_contexts.clone();
        let settlement = self.settlement.clone();

        let handler = TypedHandler::from(move |event: &OrderEventAny| {
            if event.instrument_id().venue != core.venue {
                return;
            }

            update_order_reservation(&core, &order_reservations, event.client_order_id());
            update_open_order(&core, &order_contexts, &settlement, event.client_order_id());
            if !is_terminal_order_event(event) {
                return;
            }

            sync_execution_lookup_for_instrument(
                &core,
                clock,
                &shared_token_instruments,
                &neg_risk_index,
                event.instrument_id(),
            );
        });

        msgbus::subscribe_order_events("events.order.*".into(), handler.clone(), Some(10));
        self.order_event_handler = Some(handler);
    }

    fn clear_order_event_subscription(&mut self) {
        if let Some(handler) = self.order_event_handler.take() {
            msgbus::unsubscribe_order_events("events.order.*".into(), &handler);
        }
    }

    fn ensure_position_event_subscription(&mut self) {
        if self.position_event_handler.is_some() {
            return;
        }

        let core = self.core.clone();
        let clock = self.clock;
        let shared_token_instruments = self.shared_token_instruments.clone();
        let neg_risk_index = self.neg_risk_index.clone();
        let handler = TypedHandler::from(move |event: &PositionEvent| {
            if !matches!(event, PositionEvent::PositionClosed(_)) {
                return;
            }

            if event.instrument_id().venue != core.venue {
                return;
            }

            sync_execution_lookup_for_instrument(
                &core,
                clock,
                &shared_token_instruments,
                &neg_risk_index,
                event.instrument_id(),
            );
        });

        msgbus::subscribe_position_events("events.position.*".into(), handler.clone(), Some(10));
        self.position_event_handler = Some(handler);
    }

    fn clear_position_event_subscription(&mut self) {
        if let Some(handler) = self.position_event_handler.take() {
            msgbus::unsubscribe_position_events("events.position.*".into(), &handler);
        }
    }

    /// Subscribes the settlement registry to applied and declined fill events for this venue
    /// and account, so application observation is active before buffered stream messages
    /// drain.
    fn ensure_settlement_observers(&mut self) {
        if self.fill_observer.is_some() {
            return;
        }

        let venue = self.core.venue;
        let account_id = self.core.account_id;
        let settlement = self.settlement.clone();
        let fill_tracker = self.fill_tracker.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let fill_handler = TypedHandler::from(move |event: &OrderEventAny| {
            if let OrderEventAny::Filled(fill) = event
                && fill.instrument_id.venue == venue
                && fill.account_id == account_id
                && let Some((venue_trade_id, fill)) = settlement.observe_fill_applied(fill)
            {
                emit_void_for_applied_fill(&venue_trade_id, &fill, &fill_tracker, &emitter, clock);
            }
        });

        msgbus::subscribe_order_events("events.order_filled.*".into(), fill_handler.clone(), None);
        self.fill_observer = Some(fill_handler);

        let settlement = self.settlement.clone();

        let void_handler = TypedHandler::from(move |event: &OrderEventAny| {
            if let OrderEventAny::FillVoided(voided) = event
                && voided.instrument_id.venue == venue
                && voided.account_id == account_id
            {
                settlement.observe_void_applied(voided);
            }
        });

        msgbus::subscribe_order_events(
            "events.order_fill_voided.*".into(),
            void_handler.clone(),
            None,
        );
        self.void_observer = Some(void_handler);

        let settlement = self.settlement.clone();

        let decline_handler = TypedHandler::from(move |event: &OrderEventAny| {
            if event.instrument_id().venue == venue && event.account_id() == Some(account_id) {
                settlement.observe_fill_declined(event);
            }
        });

        msgbus::subscribe_order_events(
            "events.order_fill_declined.*".into(),
            decline_handler.clone(),
            None,
        );
        self.decline_observer = Some(decline_handler);
    }

    fn clear_settlement_observers(&mut self) {
        if let Some(handler) = self.fill_observer.take() {
            msgbus::unsubscribe_order_events("events.order_filled.*".into(), &handler);
        }

        if let Some(handler) = self.void_observer.take() {
            msgbus::unsubscribe_order_events("events.order_fill_voided.*".into(), &handler);
        }

        if let Some(handler) = self.decline_observer.take() {
            msgbus::unsubscribe_order_events("events.order_fill_declined.*".into(), &handler);
        }
    }

    pub(super) fn spawn_task<F>(&self, description: &'static str, fut: F) -> bool
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let future = async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        };

        match self.pending_tasks.spawn(future) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("Skipping Polymarket {description} after shutdown began: {e}");
                false
            }
        }
    }

    pub(super) fn abort_pending_tasks(&self) {
        self.pending_tasks.begin_shutdown();
    }

    pub(super) fn abort_session_tasks(&self) {
        self.session_tasks.begin_shutdown();
    }

    pub(super) async fn await_pending_tasks(&self) -> anyhow::Result<()> {
        self.pending_tasks.begin_shutdown();
        let result = self
            .pending_tasks
            .finish_shutdown(
                Duration::from_secs(self.config.http_timeout_secs),
                TASK_ABORT_TIMEOUT,
            )
            .await;

        if self.pending_tasks.all_finished() {
            let unfinished = self.ws_dispatch_state.lock().finish_unsubmitted_modifies();
            let ts_event = self.clock.get_time_ns();

            for (client_order_id, venue_order_id, cancel_ts) in unfinished {
                let Some(order) = self.core.cache().order_owned(&client_order_id) else {
                    log::error!(
                        "Cannot finish interrupted Polymarket modification for {client_order_id}: order not found in cache"
                    );
                    continue;
                };

                self.emitter.emit_order_modify_rejected(
                    &order,
                    Some(venue_order_id),
                    "Polymarket modification was interrupted during shutdown",
                    ts_event,
                );

                if let Some(cancel_ts) = cancel_ts
                    && !self.fill_tracker.is_fully_filled(&venue_order_id)
                {
                    self.emitter
                        .emit_order_canceled(&order, Some(venue_order_id), cancel_ts);
                }
            }
        }

        result
            .map_err(|e| anyhow::anyhow!("Failed to terminate Polymarket execution tasks: {e}"))?;
        Ok(())
    }

    pub(super) async fn await_session_tasks(&self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.session_tasks
            .finish_shutdown(TASK_SESSION_GRACEFUL_SHUTDOWN_TIMEOUT, TASK_ABORT_TIMEOUT)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Polymarket session tasks: {e}"))?;
        Ok(())
    }

    pub(super) async fn refresh_account_state(&self) -> anyhow::Result<()> {
        fetch_and_emit_account_state(
            &self.http_client,
            &self.emitter,
            self.clock,
            self.config.signature_type,
            &self.order_reservations,
        )
        .await
    }

    pub(super) async fn await_account_registered(&self, timeout_secs: f64) -> anyhow::Result<()> {
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

    pub(super) async fn start_ws_stream(&mut self) -> anyhow::Result<()> {
        self.ws_client
            .connect()
            .await
            .context("failed to connect user WebSocket")?;

        if let Err(e) = self
            .ws_client
            .subscribe_user()
            .await
            .context("failed to subscribe to user channel")
        {
            if let Err(shutdown_error) = self.ws_client.disconnect().await {
                return Err(e.context(format!(
                    "Polymarket WebSocket startup rollback failed: {shutdown_error}"
                )));
            }
            return Err(e);
        }

        let Some(mut rx) = self.ws_client.take_message_receiver() else {
            let receiver_error = anyhow::anyhow!("WebSocket message receiver not available");
            if let Err(shutdown_error) = self.ws_client.disconnect().await {
                return Err(receiver_error.context(format!(
                    "Polymarket WebSocket startup rollback failed: {shutdown_error}"
                )));
            }
            return Err(receiver_error);
        };

        let emitter = self.emitter.clone();
        let order_reservations = self.order_reservations.clone();
        let token_instruments = self.shared_token_instruments.clone();
        let account_id = self.core.account_id;
        let http_client = self.http_client.clone();
        let clock = self.clock;
        let signature_type = self.config.signature_type;
        let stopping = self.stopping.clone();
        let signer_type = self.config.signer_type;
        let user_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let user_api_key = SecretString::from(self.secrets.credential.api_key_str().to_string());

        let fill_tracker = self.fill_tracker.clone();
        let settlement = self.settlement.clone();
        let pending_submits = self.pending_submits.clone();
        let order_contexts = self.order_contexts.clone();
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let session_spawner = self
            .session_tasks
            .spawner()
            .map_err(|e| anyhow::anyhow!("Polymarket session task admission is closed: {e}"))?;

        if let Err(e) = self.session_tasks.spawn(async move {
            let ctx = WsDispatchContext {
                signer_type,
                token_instruments: &token_instruments,
                fill_tracker: &fill_tracker,
                settlement: &settlement,
                pending_submits: &pending_submits,
                order_contexts: &order_contexts,
                emitter: &emitter,
                account_id,
                clock,
                user_address: &user_address,
                user_api_key: user_api_key.expose_secret(),
            };

            loop {
                match rx.recv().await {
                    Some(PolymarketWsMessage::User(user_msg)) => {
                        let refresh = {
                            let mut state = ws_dispatch_state.lock();
                            dispatch_user_message(&user_msg, &ctx, &mut state)
                        };

                        if refresh.is_some() {
                            let http = http_client.clone();
                            let emit = emitter.clone();
                            let reservations = order_reservations.clone();
                            let session_spawner = session_spawner.clone();

                            let future = async move {
                                match fetch_and_emit_account_state(
                                    &http, &emit, clock, signature_type, &reservations,
                                )
                                .await
                                {
                                    Ok(()) => log::debug!(
                                        "Account state refreshed after finalized trade for {account_id}"
                                    ),
                                    Err(e) => log::warn!(
                                        "Failed to refresh account after finalized trade: {e}"
                                    ),
                                }
                            };

                            if let Err(e) = session_spawner.spawn(future) {
                                log::debug!("Skipping finalized trade refresh during shutdown: {e}");
                            }
                        }
                    }
                    Some(PolymarketWsMessage::Market(_)) => {}
                    Some(PolymarketWsMessage::Reconnected { .. }) => {
                        log::info!("User WebSocket reconnected");
                        // A disconnect ends provisional-application eligibility for trades and
                        // orders admitted under the previous uninterrupted session
                        settlement.begin_session();
                        settlement.note_stream_gap(clock.get_time_ns());

                        if stopping.load(Ordering::Acquire) {
                            log::debug!("Skipping account refresh because execution client is stopping");
                            continue;
                        }

                        let http = http_client.clone();
                        let emit = emitter.clone();
                        let reservations = order_reservations.clone();
                        let future = async move {
                            match fetch_and_emit_account_state(&http, &emit, clock, signature_type, &reservations)
                                .await
                            {
                                Ok(()) => {
                                    log::info!("Account state refreshed after WebSocket reconnect");
                                }
                                Err(e) => {
                                    log::warn!("Failed to refresh account after reconnect: {e}");
                                }
                            }
                        };

                        if let Err(e) = session_spawner.spawn(future) {
                            log::debug!("Skipping reconnect refresh during shutdown: {e}");
                        }
                    }
                    None => {
                        log::debug!("User WebSocket stream ended");
                        break;
                    }
                }
            }

            log::debug!("User WebSocket handler task completed");
        }) {
            if let Err(shutdown_error) = self.ws_client.disconnect().await {
                return Err(anyhow::Error::new(e).context(format!(
                    "Polymarket WebSocket startup rollback failed: {shutdown_error}"
                )));
            }
            return Err(e.into());
        }

        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.stopping.store(true, Ordering::Release);
        self.clear_order_event_subscription();
        self.clear_position_event_subscription();
        self.clear_settlement_observers();
        self.abort_session_tasks();
        self.abort_pending_tasks();
        self.ws_client.begin_shutdown();

        if let Err(e) = self.ws_client.disconnect().await {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = self.await_session_tasks().await {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = self.await_pending_tasks().await {
            self.shutdown_errors.push(e.to_string());
        }
        self.core.set_disconnected();

        if self.shutdown_errors.is_empty() {
            Ok(())
        } else {
            let errors = std::mem::take(&mut self.shutdown_errors);
            anyhow::bail!(
                "Polymarket execution shutdown failed: {}",
                errors.join("; ")
            )
        }
    }

    pub(super) fn get_neg_risk(&self, instrument_id: &InstrumentId) -> bool {
        self.neg_risk_index
            .get_cloned(instrument_id)
            .unwrap_or(false)
    }

    /// Spawns the task that performs targeted terminal REST reads for quarantined and
    /// refresh-requested trades: immediately when woken, then with capped backoff per trade.
    ///
    /// The task also refreshes account state when a terminal outcome or hard fault requests it.
    fn spawn_settlement_resolution_sweeper(&self) {
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let fill_tracker = self.fill_tracker.clone();
        let settlement = self.settlement.clone();
        let pending_submits = self.pending_submits.clone();
        let order_contexts = self.order_contexts.clone();
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let token_instruments = self.shared_token_instruments.clone();
        let order_reservations = self.order_reservations.clone();
        let clock = self.clock;
        let signer_type = self.config.signer_type;
        let signature_type = self.config.signature_type;
        let user_address = self
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| self.secrets.address.clone());
        let user_api_key = SecretString::from(self.secrets.credential.api_key_str().to_string());
        let account_id = self.core.account_id;
        let cancellation = self.session_tasks.cancellation_token();

        let spawned = self.session_tasks.spawn(async move {
            let ctx = WsDispatchContext {
                signer_type,
                token_instruments: &token_instruments,
                fill_tracker: &fill_tracker,
                settlement: &settlement,
                pending_submits: &pending_submits,
                order_contexts: &order_contexts,
                emitter: &emitter,
                account_id,
                clock,
                user_address: &user_address,
                user_api_key: user_api_key.expose_secret(),
            };

            let mut schedule = ResolutionSchedule::default();
            let mut order_schedule = ResolutionSchedule::default();

            loop {
                tokio::select! {
                    () = cancellation.cancelled() => break,
                    () = settlement.resolution_requested() => {}
                    () = tokio::time::sleep(RESOLUTION_SWEEP_INTERVAL) => {}
                }

                for venue_trade_id in
                    schedule.due(settlement.pending_resolutions(), Instant::now(), usize::MAX)
                {
                    resolve_settlement_trade(
                        &http_client,
                        &ctx,
                        &ws_dispatch_state,
                        &venue_trade_id,
                    )
                    .await;
                }

                resolve_due_uncertain_orders(
                    &mut order_schedule,
                    &http_client,
                    &ctx,
                    &ws_dispatch_state,
                )
                .await;

                if settlement.take_account_refresh()
                    && let Err(e) = fetch_and_emit_account_state(
                        &http_client,
                        &emitter,
                        clock,
                        signature_type,
                        &order_reservations,
                    )
                    .await
                {
                    log::warn!("Failed to refresh account after settlement resolution: {e}");
                }

                if settlement.client_faulted() {
                    break;
                }
            }
        });

        if let Err(e) = spawned {
            log::warn!("Cannot start Polymarket settlement resolution sweeper: {e}");
        }
    }

    pub(super) fn get_neg_risk_from_snapshot(
        neg_risk_index: &AHashMap<InstrumentId, bool>,
        instrument_id: &InstrumentId,
    ) -> bool {
        neg_risk_index.get(instrument_id).copied().unwrap_or(false)
    }

    fn upsert_execution_lookup(&self, instrument: &InstrumentAny) {
        upsert_execution_lookup(
            &self.shared_token_instruments,
            &self.neg_risk_index,
            instrument,
        );
    }

    pub(super) fn load_instruments_from_cache(&self) {
        let cache = self.core.cache();
        let instruments: Vec<InstrumentAny> = cache
            .instruments(&self.core.venue, None)
            .into_iter()
            .cloned()
            .collect();

        for inst in &instruments {
            self.upsert_execution_lookup(inst);
        }

        log::debug!("Loaded {} instruments from cache", instruments.len());
    }

    pub(super) fn load_orders_from_cache(&self) {
        let cache = self.core.cache();
        let orders: Vec<_> = cache
            .orders(
                Some(&self.core.venue),
                None,
                None,
                Some(&self.core.account_id),
                None,
            )
            .into_iter()
            .map(|order| order.cloned())
            .collect();
        drop(cache);

        self.order_reservations.lock().clear();

        for order in &orders {
            update_order_reservation(
                &self.core,
                &self.order_reservations,
                order.client_order_id(),
            );
        }

        for order in &orders {
            let Some(venue_order_id) = order.venue_order_id() else {
                continue;
            };

            let replaced_venue_order_id = self
                .ws_dispatch_state
                .lock()
                .replaced_venue_order_id(venue_order_id);
            if replaced_venue_order_id {
                log::debug!(
                    "Skipping stale cache restore for replaced Polymarket venue order ID {venue_order_id}"
                );
            } else {
                self.order_contexts
                    .register_context(venue_order_id, OrderContext::from(order));
                self.order_contexts.mark_accepted(venue_order_id);
                let (prior_filled, current_leg_quantity) =
                    match venue_leg_filled_before_and_quantity(
                        order,
                        venue_order_id,
                        order.quantity().precision,
                    ) {
                        Ok(quantities) => quantities,
                        Err(e) => {
                            log::error!(
                                "Cannot restore Polymarket order {} venue-leg quantities: {e}",
                                order.client_order_id()
                            );
                            continue;
                        }
                    };

                let current_leg_filled = order
                    .filled_qty()
                    .checked_sub(prior_filled)
                    .expect("venue-leg quantity calculation validated cumulative fills");
                self.fill_tracker.restore_order(
                    venue_order_id,
                    current_leg_quantity,
                    current_leg_filled,
                    order.order_side(),
                );
            }

            // Retained core events reconstruct observed fills and voids; their presence is
            // authoritative for the settlement registry.
            for event in order.events() {
                match event {
                    OrderEventAny::Filled(fill) => self.settlement.hydrate_fill(fill),
                    OrderEventAny::FillVoided(voided) => self.settlement.hydrate_void(voided),
                    _ => {}
                }
            }
        }

        // Runs after contexts are restored, since only orders with a context are tracked
        for order in &orders {
            update_open_order(
                &self.core,
                &self.order_contexts,
                &self.settlement,
                order.client_order_id(),
            );
        }

        log::debug!(
            "Loaded {} order lifecycles from cache into {} settlement record(s)",
            orders.len(),
            self.settlement.record_count(),
        );
    }

    pub(super) fn start_client(&mut self) {
        if self.core.is_started() {
            return;
        }

        self.stopping.store(false, Ordering::Release);
        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}",
            self.core.client_id,
            self.core.account_id,
        );
    }

    pub(super) fn stop_client(&mut self) {
        if self.core.is_stopped() {
            return;
        }

        log::info!("Stopping Polymarket execution client");

        self.stopping.store(true, Ordering::Release);
        self.session_tasks.begin_shutdown();
        self.pending_tasks.begin_shutdown();
        self.clear_order_event_subscription();
        self.clear_position_event_subscription();
        self.clear_settlement_observers();

        self.ws_client.begin_shutdown();

        self.core.set_stopped();
        self.core.set_disconnected();

        log::info!("Polymarket execution client stopped");
    }

    pub(super) fn reset_client(&mut self) {
        log::debug!("Resetting Polymarket execution client");

        self.stopping.store(true, Ordering::Release);
        self.session_tasks.begin_shutdown();
        self.pending_tasks.begin_shutdown();
        self.ws_client.begin_shutdown();
        self.core.set_disconnected();
        self.clear_order_event_subscription();
        self.clear_position_event_subscription();
        self.clear_settlement_observers();
        self.shared_token_instruments.store(AHashMap::new());
        self.neg_risk_index.store(AHashMap::new());
        self.order_reservations.lock().clear();
        self.ws_dispatch_state.lock().reset_session();
        self.settlement.clear();
    }

    pub(super) async fn connect_client(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() && self.pending_tasks.is_open() && self.session_tasks.is_open()
        {
            return Ok(());
        }

        log::info!("Connecting Polymarket execution client");

        if !self.pending_tasks.is_open() || !self.session_tasks.is_open() {
            self.teardown_partial_connect().await?;
        }

        if !self.pending_tasks.is_open() {
            self.await_pending_tasks().await?;
            self.pending_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Polymarket task generation: {e}"))?;
        }

        if !self.session_tasks.is_open() {
            self.await_session_tasks().await?;
            self.session_tasks.start_generation().map_err(|e| {
                anyhow::anyhow!("Failed to start Polymarket session generation: {e}")
            })?;
        }
        self.stopping.store(false, Ordering::Release);
        let ws_shutdown = self.ws_client.shutdown_handle();
        let stopping = Arc::clone(&self.stopping);
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {
                stopping.store(true, Ordering::Release);
                ws_shutdown.begin_shutdown();
            });

        let version = self
            .http_client
            .get_version()
            .await
            .context("failed to query Polymarket CLOB protocol version")?
            .version;

        if version != SUPPORTED_CLOB_VERSION {
            anyhow::bail!(
                "Polymarket CLOB protocol version {version} is unsupported; adapter supports V2 only"
            );
        }

        self.settlement.mark_hydrating();
        self.ensure_order_event_subscription();
        // Application observation must be active before buffered user messages drain.
        self.ensure_settlement_observers();
        self.load_instruments_from_cache();
        self.load_orders_from_cache();
        self.core.set_instruments_initialized();
        self.settlement.begin_session();
        self.settlement.mark_live();
        self.spawn_settlement_resolution_sweeper();

        if let Err(e) = self.start_ws_stream().await {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Polymarket startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }
        self.ensure_position_event_subscription();

        let post_ws = async {
            self.refresh_account_state().await?;
            self.await_account_registered(30.0).await?;
            self.start_heartbeat_task()?;
            Ok::<(), anyhow::Error>(())
        };

        if let Err(e) = post_ws.await {
            log::warn!("Connect failed after WS started, tearing down: {e}");
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Polymarket startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        setup_guard.disarm();
        self.core.set_connected();

        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    pub(super) async fn disconnect_client(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting Polymarket execution client");

        self.teardown_partial_connect().await?;

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    pub(super) fn on_instrument_update(&self, instrument: &InstrumentAny) {
        self.upsert_execution_lookup(instrument);
    }
}

async fn run_heartbeats(
    http_client: crate::http::clob::PolymarketClobHttpClient,
    cancellation: CancellationToken,
    healthy: Arc<AtomicBool>,
) {
    let heartbeat_health_timeout = HEARTBEAT_SAFETY_TIMEOUT
        .checked_sub(HEARTBEAT_HEALTH_MARGIN)
        .expect("heartbeat health margin should be shorter than the safety timeout");
    let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat_id = String::new();
    let mut request_failures = 0;
    let mut last_acknowledged = None;

    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            _ = interval.tick() => {}
        }

        let mut resynchronized = false;

        loop {
            let now = tokio::time::Instant::now();
            let request_timeout = now
                .checked_add(HEARTBEAT_REQUEST_TIMEOUT)
                .expect("heartbeat request timeout should fit in an instant");
            let health_deadline = last_acknowledged.map(|acknowledged: tokio::time::Instant| {
                acknowledged
                    .checked_add(heartbeat_health_timeout)
                    .expect("heartbeat health timeout should fit in an instant")
            });

            if health_deadline.is_some_and(|deadline| deadline <= now) {
                log::error!("Polymarket heartbeat health deadline elapsed");
                healthy.store(false, Ordering::Release);
                return;
            }

            let request_deadline =
                health_deadline.map_or(request_timeout, |deadline| deadline.min(request_timeout));
            let response = tokio::select! {
                () = cancellation.cancelled() => return,
                response = tokio::time::timeout_at(
                    request_deadline,
                    http_client.post_heartbeat(&heartbeat_id),
                ) => response.unwrap_or(Err(HttpError::Timeout)),
            };

            match response {
                Ok(HeartbeatResponse::Acknowledged(next_id)) => {
                    let acknowledged = tokio::time::Instant::now();
                    if health_deadline.is_some_and(|deadline| acknowledged >= deadline) {
                        log::error!(
                            "Polymarket heartbeat was acknowledged after the health deadline"
                        );
                        healthy.store(false, Ordering::Release);
                        return;
                    }

                    heartbeat_id = next_id;
                    request_failures = 0;
                    last_acknowledged = Some(acknowledged);
                    healthy.store(true, Ordering::Release);
                    interval.reset_after(HEARTBEAT_INTERVAL);
                    break;
                }
                Ok(HeartbeatResponse::Resynchronize(next_id)) if !resynchronized => {
                    heartbeat_id = next_id;
                    resynchronized = true;
                }
                Ok(HeartbeatResponse::Resynchronize(_)) => {
                    log::error!("Polymarket heartbeat rejected after ID resynchronization");
                    healthy.store(false, Ordering::Release);
                    return;
                }
                Err(e) if e.is_retryable() => {
                    request_failures += 1;
                    if request_failures >= HEARTBEAT_REQUEST_FAILURE_LIMIT {
                        log::error!(
                            "Polymarket heartbeat failed after {request_failures} consecutive request attempts"
                        );
                        healthy.store(false, Ordering::Release);
                        return;
                    }

                    let Some(retry_after) = e.retry_after() else {
                        log::warn!(
                            "Polymarket heartbeat request attempt {request_failures} failed"
                        );
                        continue;
                    };
                    let now = tokio::time::Instant::now();
                    let Some(retry_at) = now.checked_add(retry_after) else {
                        log::error!(
                            "Polymarket heartbeat retry delay exceeded the safety deadline"
                        );
                        healthy.store(false, Ordering::Release);
                        return;
                    };

                    if health_deadline.is_some_and(|deadline| retry_at >= deadline) {
                        log::error!(
                            "Polymarket heartbeat retry delay exceeded the health deadline"
                        );
                        healthy.store(false, Ordering::Release);
                        return;
                    }

                    log::warn!(
                        "Polymarket heartbeat request attempt {request_failures} was rate limited; retrying after {retry_after:?}"
                    );
                    tokio::select! {
                        () = cancellation.cancelled() => return,
                        () = tokio::time::sleep_until(retry_at) => {}
                    }

                    if cancellation.is_cancelled() {
                        return;
                    }
                }
                Err(e) if e.is_auth_error() => {
                    log::error!("Polymarket heartbeat authentication failed");
                    healthy.store(false, Ordering::Release);
                    return;
                }
                Err(_) => {
                    log::error!("Polymarket heartbeat was rejected by the venue");
                    healthy.store(false, Ordering::Release);
                    return;
                }
            }
        }
    }
}

/// Performs a fresh, authenticated, account-scoped targeted REST read for one trade and feeds
/// the result to the settlement registry.
///
/// Missing, duplicated, non-terminal, or non-admissible results leave the trade pending; the
/// sweeper retries with capped backoff.
async fn resolve_settlement_trade(
    http_client: &PolymarketClobHttpClient,
    ctx: &WsDispatchContext<'_>,
    ws_dispatch_state: &Mutex<WsDispatchState>,
    venue_trade_id: &str,
) {
    let params = GetTradesParams {
        id: Some(venue_trade_id.to_string()),
        ..Default::default()
    };

    let trades = match http_client.get_trades(params).await {
        Ok(trades) => trades,
        Err(e) => {
            log::warn!("Targeted REST read for Polymarket trade {venue_trade_id} failed: {e}");
            return;
        }
    };

    let Some(trade) = targeted_trade_row(&trades, venue_trade_id) else {
        return;
    };

    apply_rest_trade_evidence(trade, ctx, &mut ws_dispatch_state.lock());
}

/// Returns the one row matching `venue_trade_id`; a missing or duplicated row is not an
/// authoritative result.
fn targeted_trade_row<'a>(
    trades: &'a [PolymarketTradeReport],
    venue_trade_id: &str,
) -> Option<&'a PolymarketTradeReport> {
    let matches: Vec<&PolymarketTradeReport> = trades
        .iter()
        .filter(|trade| trade.id == venue_trade_id)
        .collect();

    match matches.as_slice() {
        [trade] => Some(*trade),
        [] => {
            log::debug!(
                "Targeted REST read for Polymarket trade {venue_trade_id} returned no matching \
                 row; retrying"
            );
            None
        }
        _ => {
            log::warn!(
                "Targeted REST read for Polymarket trade {venue_trade_id} returned {} rows; \
                 retrying",
                matches.len()
            );
            None
        }
    }
}

/// Runs the due targeted REST reads for orders whose venue state is unknown, at most
/// [`UNCERTAIN_ORDER_READS_PER_SWEEP`] per sweep.
async fn resolve_due_uncertain_orders(
    schedule: &mut ResolutionSchedule<VenueOrderId>,
    http_client: &PolymarketClobHttpClient,
    ctx: &WsDispatchContext<'_>,
    ws_dispatch_state: &Mutex<WsDispatchState>,
) {
    let uncertain_orders = ctx.settlement.uncertain_orders();
    let due = schedule.due(
        uncertain_orders
            .iter()
            .map(|(venue_order_id, _)| *venue_order_id)
            .collect(),
        Instant::now(),
        UNCERTAIN_ORDER_READS_PER_SWEEP,
    );

    for (venue_order_id, uncertain) in uncertain_orders {
        if due.contains(&venue_order_id)
            && resolve_uncertain_order(
                http_client,
                ctx,
                ws_dispatch_state,
                venue_order_id,
                uncertain,
            )
            .await
        {
            ctx.settlement.clear_uncertain_order(&venue_order_id);
        }
    }
}

/// Reads the venue state of an order whose submit outcome is unknown, or the trades of an order
/// that was live during a stream gap, and applies it.
///
/// Returns `true` once the state is applied, or once the resolution window passes without
/// venue evidence so reconciliation resumes for the order's instrument.
async fn resolve_uncertain_order(
    http_client: &PolymarketClobHttpClient,
    ctx: &WsDispatchContext<'_>,
    ws_dispatch_state: &Mutex<WsDispatchState>,
    venue_order_id: VenueOrderId,
    uncertain: UncertainOrder,
) -> bool {
    let apply_evidence = match uncertain.kind {
        UncertainOrderKind::Submit => apply_uncertain_order_evidence,
        UncertainOrderKind::StreamGap => apply_stream_gap_order_evidence,
    };

    let applied = match http_client
        .get_order_optional(venue_order_id.as_str())
        .await
    {
        Ok(Some(order)) => {
            let params = GetTradesParams {
                market: Some(order.market.to_string()),
                after: Some(
                    order
                        .created_at
                        .saturating_sub(UNCERTAIN_ORDER_TRADE_LOOKBACK.as_secs()),
                ),
                ..Default::default()
            };

            match http_client.get_trades(params).await {
                Ok(trades) => apply_evidence(
                    venue_order_id,
                    &order,
                    &trades,
                    ctx,
                    &mut ws_dispatch_state.lock(),
                ),
                Err(e) => {
                    log::warn!(
                        "Targeted REST trade read for Polymarket order {venue_order_id} failed: {e}"
                    );
                    false
                }
            }
        }
        Ok(None) => false,
        Err(e) => {
            log::warn!("Targeted REST read for Polymarket order {venue_order_id} failed: {e}");
            false
        }
    };

    if applied {
        if uncertain.kind == UncertainOrderKind::Submit {
            log::info!(
                "Resolved Polymarket order {venue_order_id} with an unknown submit outcome from \
                 REST evidence"
            );
        }

        return true;
    }

    let elapsed = ctx
        .clock
        .get_time_ns()
        .as_u64()
        .saturating_sub(uncertain.noted_at.as_u64());

    if u128::from(elapsed) < UNCERTAIN_ORDER_RESOLUTION_WINDOW.as_nanos() {
        return false;
    }

    log::warn!(
        "Ending targeted REST resolution of Polymarket order {venue_order_id} without venue \
         evidence after {UNCERTAIN_ORDER_RESOLUTION_WINDOW:?}"
    );
    true
}

/// Targeted REST attempt schedule for pending trades or orders, with capped exponential backoff
/// per key.
#[derive(Debug)]
struct ResolutionSchedule<K> {
    // Completed attempts and the earliest next attempt per pending key
    attempts: AHashMap<K, (u32, Instant)>,
}

impl<K> Default for ResolutionSchedule<K> {
    fn default() -> Self {
        Self {
            attempts: AHashMap::new(),
        }
    }
}

impl<K: Clone + Eq + Hash> ResolutionSchedule<K> {
    /// Returns at most `limit` pending keys due for an attempt at `now` and records those attempts.
    ///
    /// Keys never attempted come first, then the longest overdue, so a capped sweep reaches every
    /// pending key even when sweeps outlast the backoff. Keys no longer pending are forgotten, so a
    /// later request starts a fresh schedule.
    fn due(&mut self, pending: Vec<K>, now: Instant, limit: usize) -> Vec<K> {
        self.attempts.retain(|key, _| pending.contains(key));

        let mut due: Vec<(Option<Instant>, K)> = pending
            .into_iter()
            .filter_map(|key| match self.attempts.get(&key) {
                Some((_, next_attempt_at)) if now < *next_attempt_at => None,
                Some((_, next_attempt_at)) => Some((Some(*next_attempt_at), key)),
                None => Some((None, key)),
            })
            .collect();

        due.sort_by_key(|(next_attempt_at, _)| *next_attempt_at);
        due.truncate(limit);

        due.into_iter()
            .map(|(_, key)| {
                let completed = self
                    .attempts
                    .get(&key)
                    .map_or(0, |(completed, _)| *completed);
                self.attempts.insert(
                    key.clone(),
                    (completed + 1, now + resolution_backoff(completed)),
                );
                key
            })
            .collect()
    }
}

fn resolution_backoff(completed_attempts: u32) -> Duration {
    RESOLUTION_BACKOFF_INITIAL
        .checked_mul(1_u32 << completed_attempts.min(16))
        .map_or(RESOLUTION_BACKOFF_MAX, |backoff| {
            backoff.min(RESOLUTION_BACKOFF_MAX)
        })
}

fn update_order_reservation(
    core: &ExecutionClientCore,
    reservations: &Mutex<AHashMap<ClientOrderId, Money>>,
    client_order_id: ClientOrderId,
) {
    let cache = core.cache();
    let order = cache.order(&client_order_id);
    let Some(order) = order.filter(|order| {
        order.account_id() == Some(core.account_id)
            && order.instrument_id().venue == core.venue
            && order.is_open()
            && order.ts_accepted().is_some()
            && order.order_side() == OrderSide::Buy
    }) else {
        reservations.lock().remove(&client_order_id);
        return;
    };
    let Some(price) = order.price() else {
        reservations.lock().remove(&client_order_id);
        return;
    };
    let Some(instrument) = cache.instrument(&order.instrument_id()) else {
        log::error!("Cannot calculate Polymarket reservation: no instrument for {client_order_id}");
        return;
    };

    match instrument.try_calculate_notional_value(order.leaves_qty(), price, None) {
        Ok(locked) => {
            reservations.lock().insert(client_order_id, locked);
        }
        Err(e) => log::error!("Cannot calculate Polymarket reservation for {client_order_id}: {e}"),
    }
}

/// Tracks an open order for stream-gap reads only when the adapter holds its context, because an
/// order adopted from the venue has no context and its fills reach the engine through
/// reconciliation.
fn update_open_order(
    core: &ExecutionClientCore,
    order_contexts: &OrderContextRegistry,
    settlement: &SettlementRegistry,
    client_order_id: ClientOrderId,
) {
    let cache = core.cache();

    let open = cache
        .order(&client_order_id)
        .filter(|order| {
            order.account_id() == Some(core.account_id)
                && order.instrument_id().venue == core.venue
                && order.is_open()
        })
        .and_then(|order| Some((order.venue_order_id()?, order.instrument_id())))
        .filter(|(venue_order_id, _)| order_contexts.get(venue_order_id).is_some());

    drop(cache);

    match open {
        Some((venue_order_id, instrument_id)) => {
            settlement.note_order_open(client_order_id, venue_order_id, instrument_id);
        }
        None => settlement.note_order_closed(&client_order_id),
    }
}

fn upsert_execution_lookup(
    shared_token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    neg_risk_index: &AtomicMap<InstrumentId, bool>,
    instrument: &InstrumentAny,
) {
    let token_id = Ustr::from(instrument.raw_symbol().as_str());
    shared_token_instruments.insert(token_id, instrument.clone());

    if let InstrumentAny::BinaryOption(bo) = instrument {
        let neg_risk = bo
            .info
            .as_ref()
            .and_then(|i| i.get_bool("neg_risk"))
            .unwrap_or(false);
        neg_risk_index.insert(bo.id, neg_risk);
    }
}

fn remove_execution_lookup(
    shared_token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    neg_risk_index: &AtomicMap<InstrumentId, bool>,
    instrument: &InstrumentAny,
) {
    shared_token_instruments.remove(&Ustr::from(instrument.raw_symbol().as_str()));
    neg_risk_index.remove(&instrument.id());
}

fn sync_execution_lookup_for_instrument(
    core: &ExecutionClientCore,
    clock: &'static AtomicTime,
    shared_token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    neg_risk_index: &AtomicMap<InstrumentId, bool>,
    instrument_id: InstrumentId,
) {
    let now_ns = clock.get_time_ns();
    let account_id = core.account_id;
    let cache = core.cache();

    let instrument = cache.instrument(&instrument_id).cloned();
    let retain = instrument.as_ref().is_some_and(|instrument| {
        if !crate::filters::is_expired(instrument, now_ns) {
            return true;
        }

        cache.has_orders_open(
            Some(&core.venue),
            Some(&instrument_id),
            None,
            Some(&account_id),
            None,
        ) || cache.has_positions_open(
            Some(&core.venue),
            Some(&instrument_id),
            None,
            Some(&account_id),
            None,
        )
    });

    drop(cache);

    match instrument {
        Some(instrument) if retain => {
            upsert_execution_lookup(shared_token_instruments, neg_risk_index, &instrument);
        }
        Some(instrument) => {
            remove_execution_lookup(shared_token_instruments, neg_risk_index, &instrument);
        }
        // Instrument not in cache: token key cannot be derived, so drop only the neg-risk entry
        None => neg_risk_index.remove(&instrument_id),
    }
}

fn is_terminal_order_event(event: &OrderEventAny) -> bool {
    matches!(
        event,
        OrderEventAny::Canceled(_)
            | OrderEventAny::Expired(_)
            | OrderEventAny::Rejected(_)
            | OrderEventAny::Filled(_)
            | OrderEventAny::FillVoided(_)
    )
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use indexmap::IndexMap;
    use nautilus_common::{
        cache::Cache,
        clients::ExecutionClient,
        live::runner::set_exec_event_sender,
        messages::{
            ExecutionEvent,
            execution::{CancelAllOrders, GenerateFillReportsBuilder},
        },
        msgbus::{publish_order_event, publish_position_event, switchboard},
    };
    use nautilus_core::{UUID4, UnixNanos, nanos::DurationNanos};
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        enums::{
            AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, PositionSide, TimeInForce,
        },
        events::{
            OrderEventAny, PositionClosed, PositionEvent,
            order::spec::{
                OrderFillVoidedSpec, OrderFilledSpec, OrderPendingCancelSpec,
                OrderPendingUpdateSpec, OrderUpdatedSpec,
            },
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId,
            TraderId, VenueOrderId,
        },
        instruments::stubs::binary_option,
        orders::{LimitOrder, Order, OrderAny, stubs::TestOrderEventStubs},
        position::Position,
        types::{Currency, Money, Price, Price as ModelPrice, Quantity, Quantity as ModelQuantity},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::{
        common::enums::PolymarketTradeStatus,
        execution::settlement::{
            AdmittedLeg,
            admission::AdmittedTrade,
            registry::tests::{force_client_fault, leg_application, trade_hard_fault},
            state::LegApplication,
        },
        factories::spawn_rejecting_proxy,
    };

    const TEST_PRIVATE_KEY: &str =
        "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
    const TEST_API_SECRET_B64: &str = "dGVzdF9zZWNyZXRfa2V5XzMyYnl0ZXNfcGFkMTIzNDU=";

    fn test_client() -> (PolymarketExecutionClient, Rc<RefCell<Cache>>) {
        test_client_with_proxy(None)
    }

    fn test_client_with_proxy(
        proxy_url: Option<String>,
    ) -> (PolymarketExecutionClient, Rc<RefCell<Cache>>) {
        test_client_with_proxy_and_http_urls(
            proxy_url,
            "http://127.0.0.1:3000",
            "http://127.0.0.1:3000",
        )
    }

    fn test_client_with_proxy_and_http_urls(
        proxy_url: Option<String>,
        base_url_http: &str,
        base_url_data_api: &str,
    ) -> (PolymarketExecutionClient, Rc<RefCell<Cache>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("POLYMARKET"),
            *crate::common::consts::POLYMARKET_VENUE,
            OmsType::Netting,
            AccountId::from("POLYMARKET-001"),
            AccountType::Cash,
            None,
            cache.clone(),
        );
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        set_exec_event_sender(tx);
        let client = PolymarketExecutionClient::new(
            core,
            crate::config::PolymarketExecutionClientConfig {
                private_key: Some(TEST_PRIVATE_KEY.into()),
                api_key: Some("test_api_key".into()),
                api_secret: Some(TEST_API_SECRET_B64.into()),
                passphrase: Some("test_pass".into()),
                funder: None,
                base_url_http: Some(base_url_http.to_string()),
                base_url_ws: Some("ws://127.0.0.1:3000/ws".to_string()),
                base_url_data_api: Some(base_url_data_api.to_string()),
                proxy_url: proxy_url.map(SecretString::from),
                ..crate::config::PolymarketExecutionClientConfig::default()
            },
        )
        .expect("test client should construct");

        (client, cache)
    }

    #[rstest]
    #[tokio::test]
    async fn execution_client_propagates_proxy_without_debug_exposure() {
        const USERNAME: &str = "exec-user";
        const SECRET: &str = "exec-client-proxy-secret";
        let (proxy_addr, requests) = spawn_rejecting_proxy(2).await;
        let proxy_url = format!("http://{USERNAME}:{SECRET}@{proxy_addr}");
        let (client, _cache) = test_client_with_proxy_and_http_urls(
            Some(proxy_url.clone()),
            "https://clob-auth.fixture",
            "https://data-auth.fixture",
        );
        let debug = format!("{client:?}");
        let errors = [
            client
                .http_client
                .get_book("auth-token")
                .await
                .unwrap_err()
                .to_string(),
            client
                .data_api_client
                .get_positions("0x0000000000000000000000000000000000000002")
                .await
                .unwrap_err()
                .to_string(),
        ];
        let requests = requests.lock().await;
        let request_lines = requests
            .iter()
            .map(|request| request.lines().next().unwrap().to_string())
            .collect::<Vec<_>>();
        let expected_auth = format!("Basic {}", BASE64.encode(format!("{USERNAME}:{SECRET}")));

        assert_eq!(
            client
                .config
                .proxy_url
                .as_ref()
                .map(SecretString::expose_secret),
            Some(proxy_url.as_str())
        );
        assert_eq!(client.ws_client.proxy_url().unwrap().expose(), proxy_url);
        assert_eq!(
            request_lines,
            [
                "CONNECT clob-auth.fixture:443 HTTP/1.1",
                "CONNECT data-auth.fixture:443 HTTP/1.1",
            ]
        );

        for request in requests.iter() {
            let auth = request
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("proxy-authorization")
                        .then_some(value.trim())
                })
                .expect("Proxy-Authorization header");
            assert_eq!(auth, expected_auth);
        }

        for error in errors {
            assert!(!error.contains(SECRET));
            assert!(!error.contains(&expected_auth));
        }
        assert!(!debug.contains(SECRET));
    }

    fn test_binary_option(raw_symbol: &str, expired: bool, neg_risk: bool) -> InstrumentAny {
        let clock = nautilus_core::time::get_atomic_clock_realtime();
        let mut binary = binary_option();
        binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
        binary.raw_symbol = Symbol::new(raw_symbol);
        binary.currency = Currency::pUSD();
        binary.expiration_ns = if expired {
            UnixNanos::from(clock.get_time_ns().as_u64().saturating_sub(1_000_000_000))
        } else {
            UnixNanos::from(
                clock
                    .get_time_ns()
                    .as_u64()
                    .saturating_add(86_400_000_000_000),
            )
        };

        let mut info = nautilus_core::Params::new();
        info.insert("neg_risk".to_string(), Value::Bool(neg_risk));
        binary.info = Some(info);

        InstrumentAny::BinaryOption(binary)
    }

    fn open_limit_order(instrument_id: InstrumentId) -> OrderAny {
        OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("O-RETAIN"),
            OrderSide::Buy,
            ModelQuantity::new(10.0, 0),
            ModelPrice::from("0.5000"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            nautilus_core::UUID4::new(),
            UnixNanos::default(),
        ))
    }

    fn cache_accepted_open_order(cache: &mut Cache, instrument_id: InstrumentId) -> OrderAny {
        let mut order = open_limit_order(instrument_id);
        cache.add_order(order.clone(), None, None, false).unwrap();

        let submitted = TestOrderEventStubs::submitted(&order, AccountId::from("POLYMARKET-001"));
        order = cache.update_order(&submitted).unwrap();

        let accepted = TestOrderEventStubs::accepted(
            &order,
            AccountId::from("POLYMARKET-001"),
            VenueOrderId::from("V-001"),
        );
        cache.update_order(&accepted).unwrap()
    }

    fn open_position(instrument: &InstrumentAny) -> Position {
        let order = open_limit_order(instrument.id());
        let filled = match TestOrderEventStubs::filled(
            &order,
            instrument,
            None,
            None,
            Some(ModelPrice::from("0.5000")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("POLYMARKET-001")),
        ) {
            OrderEventAny::Filled(filled) => filled,
            other => panic!("expected filled event, was {other:?}"),
        };

        Position::new(instrument, filled)
    }

    fn closed_position(position: &Position) -> Position {
        let mut closed = position.clone();
        closed.side = PositionSide::Flat;
        closed.signed_qty = 0.0;
        closed.quantity = Quantity::zero(position.size_precision);
        closed.ts_closed = Some(position.ts_last);
        closed.duration_ns = DurationNanos::new(1);
        closed
    }

    fn position_closed_event(position: &Position) -> PositionEvent {
        PositionEvent::PositionClosed(PositionClosed {
            trader_id: position.trader_id,
            strategy_id: position.strategy_id,
            instrument_id: position.instrument_id,
            position_id: position.id,
            account_id: position.account_id,
            opening_order_id: position.opening_order_id,
            closing_order_id: position.closing_order_id,
            entry: position.entry,
            side: PositionSide::Flat,
            signed_qty: 0.0,
            quantity: Quantity::zero(position.size_precision),
            peak_quantity: position.peak_qty,
            last_qty: Quantity::zero(position.size_precision),
            last_px: Price::zero(position.price_precision),
            currency: position.quote_currency,
            avg_px_open: position.avg_px_open,
            avg_px_close: position.avg_px_close,
            realized_return: position.realized_return,
            realized_pnl: position.realized_pnl,
            unrealized_pnl: Money::zero(position.quote_currency),
            duration: DurationNanos::new(1),
            event_id: UUID4::new(),
            ts_opened: position.ts_opened,
            ts_closed: position.ts_closed.or(Some(position.ts_last)),
            ts_event: position.ts_last,
            ts_init: position.ts_last,
        })
    }

    #[rstest]
    fn load_instruments_from_cache_preloads_expired_execution_lookup_state() {
        let (client, cache) = test_client();
        let active = test_binary_option("0xACTIVE", false, true);
        let expired = test_binary_option("0xEXPIRED", true, true);

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(active.clone()).unwrap();
            cache.add_instrument(expired.clone()).unwrap();
        }

        client.load_instruments_from_cache();

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(active.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&active.id()));
        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn load_orders_from_cache_restores_failed_trade_correction_state() {
        let (client, cache) = test_client();
        let instrument = test_binary_option("0xRESTART", false, false);
        let venue_order_id = VenueOrderId::from("V-001");

        let order = {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(instrument.clone()).unwrap();
            let order = cache_accepted_open_order(&mut cache, instrument.id());
            let mut filled = TestOrderEventStubs::filled(
                &order,
                &instrument,
                None,
                None,
                Some(ModelPrice::from("0.5000")),
                None,
                None,
                None,
                None,
                Some(AccountId::from("POLYMARKET-001")),
            );

            if let OrderEventAny::Filled(ref mut fill) = filled {
                fill.trade_id = TradeId::from("trade-restart");
                fill.info = Some(IndexMap::from([
                    (Ustr::from("id"), Ustr::from("trade-restart")),
                    (Ustr::from("taker_order_id"), Ustr::from("V-001")),
                ]));
            }

            let filled = match filled {
                OrderEventAny::Filled(filled) => filled,
                other => panic!("expected filled event, was {other:?}"),
            };
            cache
                .update_order(&OrderEventAny::Filled(filled.clone()))
                .unwrap();
            let voided = OrderFillVoidedSpec::builder()
                .trader_id(filled.trader_id)
                .strategy_id(filled.strategy_id)
                .instrument_id(filled.instrument_id)
                .client_order_id(filled.client_order_id)
                .venue_order_id(filled.venue_order_id)
                .account_id(filled.account_id)
                .trade_id(filled.trade_id)
                .voided_qty(filled.last_qty)
                .maybe_commission_voided(filled.commission)
                .order_side(filled.order_side)
                .order_type(filled.order_type)
                .last_px(filled.last_px)
                .currency(filled.currency)
                .liquidity_side(filled.liquidity_side)
                .maybe_position_id(filled.position_id)
                .maybe_info(filled.info)
                .build();
            cache
                .update_order(&OrderEventAny::FillVoided(voided))
                .unwrap()
        };

        client.load_orders_from_cache();

        let context = client
            .order_contexts
            .get(&venue_order_id)
            .expect("order context restored");

        assert_eq!(context, OrderContext::from(&order));
        assert!(!client.order_contexts.mark_accepted(venue_order_id));
        assert_eq!(
            client.fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(order.filled_qty())
        );
        assert_eq!(order.status(), OrderStatus::Voided);
        assert_eq!(
            leg_application(&client.settlement, &TradeId::from("trade-restart")),
            Some(LegApplication::VoidObserved)
        );
    }

    #[rstest]
    fn load_orders_from_cache_preserves_promoted_replacement_identity() {
        let (client, cache) = test_client();
        let instrument = test_binary_option("0xMODIFY-RESTART", false, false);
        let old_venue_order_id = VenueOrderId::from("V-001");
        let new_venue_order_id = VenueOrderId::from("V-002");

        let order = {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(instrument.clone()).unwrap();
            cache_accepted_open_order(&mut cache, instrument.id())
        };

        client.load_orders_from_cache();

        let context = client
            .order_contexts
            .get(&old_venue_order_id)
            .expect("old venue context loaded");
        {
            let mut state = client.ws_dispatch_state.lock();
            assert!(state.begin_modify(
                order.client_order_id(),
                old_venue_order_id,
                instrument.id(),
            ));
            assert!(state.set_modify_replacement(
                order.client_order_id(),
                new_venue_order_id,
                ModelQuantity::new(12.0, 0),
                ModelQuantity::new(12.0, 0),
                ModelPrice::from("0.6000"),
            ));
            assert!(state.claim_modify_replacement(new_venue_order_id).is_some());
        }

        let replacement_context = OrderContext {
            quantity: ModelQuantity::from("12"),
            price: Some(ModelPrice::from("0.6000")),
            ..context
        };
        client
            .order_contexts
            .register_context(new_venue_order_id, replacement_context);
        client.fill_tracker.restore_order(
            new_venue_order_id,
            ModelQuantity::new(12.0, 0),
            ModelQuantity::zero(0),
            OrderSide::Buy,
        );

        client.ws_dispatch_state.lock().reset_session();
        client.load_orders_from_cache();

        assert_eq!(
            client.order_contexts.get(&new_venue_order_id),
            Some(replacement_context)
        );
        assert_eq!(
            client.order_contexts.get(&old_venue_order_id),
            Some(context)
        );
        assert_eq!(
            client
                .order_contexts
                .venue_order_id(&order.client_order_id()),
            Some(new_venue_order_id)
        );
        assert_eq!(
            client
                .fill_tracker
                .get_cumulative_filled(&new_venue_order_id),
            Some(ModelQuantity::zero(0))
        );
        assert!(
            client
                .ws_dispatch_state
                .lock()
                .replaced_venue_order_id(old_venue_order_id)
        );
    }

    #[rstest]
    fn on_instrument_update_upserts_expired_execution_lookup_state() {
        let (client, _cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_ONLY", true, true);

        client.on_instrument_update(&expired);

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn sync_execution_lookup_keeps_expired_lookup_state_with_open_position() {
        let (client, cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_POSITION", true, true);
        let position = open_position(&expired);

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(expired.clone()).unwrap();
            cache.add_position(&position, OmsType::Netting).unwrap();
        }

        sync_execution_lookup_for_instrument(
            &client.core,
            client.clock,
            &client.shared_token_instruments,
            &client.neg_risk_index,
            expired.id(),
        );

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn sync_execution_lookup_keeps_expired_lookup_state_with_open_order() {
        let (client, cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_ORDER", true, true);

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(expired.clone()).unwrap();
            let _order = cache_accepted_open_order(&mut cache, expired.id());
        }

        sync_execution_lookup_for_instrument(
            &client.core,
            client.clock,
            &client.shared_token_instruments,
            &client.neg_risk_index,
            expired.id(),
        );

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn position_event_subscription_prunes_expired_lookup_after_position_closes() {
        let (client, cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_CLOSED", true, true);
        let position = open_position(&expired);
        let closed = closed_position(&position);

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(expired.clone()).unwrap();
            cache.add_position(&position, OmsType::Netting).unwrap();
        }

        sync_execution_lookup_for_instrument(
            &client.core,
            client.clock,
            &client.shared_token_instruments,
            &client.neg_risk_index,
            expired.id(),
        );
        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));

        {
            let mut cache = cache.borrow_mut();
            cache.update_position(&closed).unwrap();
        }

        let mut client = client;
        client.ensure_position_event_subscription();
        let event = position_closed_event(&closed);
        assert!(matches!(event, PositionEvent::PositionClosed(_)));
        publish_position_event("events.position.TEST".into(), &event);

        assert!(
            !client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(!client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn order_event_subscription_prunes_expired_lookup_after_terminal_order() {
        let (client, cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_ORDER_CLOSED", true, true);
        let mut order;

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(expired.clone()).unwrap();
            order = cache_accepted_open_order(&mut cache, expired.id());
        }

        sync_execution_lookup_for_instrument(
            &client.core,
            client.clock,
            &client.shared_token_instruments,
            &client.neg_risk_index,
            expired.id(),
        );

        let canceled = TestOrderEventStubs::canceled(
            &order,
            AccountId::from("POLYMARKET-001"),
            order.venue_order_id(),
        );
        order.apply(canceled.clone()).unwrap();

        {
            let mut cache = cache.borrow_mut();
            cache.update_order(&canceled).unwrap();
        }

        let mut client = client;
        client.ensure_order_event_subscription();
        publish_order_event("events.order.TEST".into(), &canceled);

        assert!(
            !client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(!client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn order_event_subscription_keeps_expired_lookup_after_filled_when_position_remains_open() {
        let (client, cache) = test_client();
        let expired = test_binary_option("0xEXPIRED_FILLED_OPEN", true, true);
        let order;
        let position;

        {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(expired.clone()).unwrap();
            order = cache_accepted_open_order(&mut cache, expired.id());
        }

        sync_execution_lookup_for_instrument(
            &client.core,
            client.clock,
            &client.shared_token_instruments,
            &client.neg_risk_index,
            expired.id(),
        );

        let filled = TestOrderEventStubs::filled(
            &order,
            &expired,
            None,
            None,
            Some(ModelPrice::from("0.5000")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("POLYMARKET-001")),
        );

        position = match filled.clone() {
            OrderEventAny::Filled(filled) => Position::new(&expired, filled),
            other => panic!("expected filled event, was {other:?}"),
        };

        {
            let mut cache = cache.borrow_mut();
            cache.update_order(&filled).unwrap();
            cache.add_position(&position, OmsType::Netting).unwrap();
        }

        let mut client = client;
        client.ensure_order_event_subscription();
        publish_order_event("events.order.TEST".into(), &filled);

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn position_event_subscription_ignores_other_venue_events() {
        let (mut client, _cache) = test_client();
        let expired = test_binary_option("0xOTHER_VENUE", true, true);
        client.upsert_execution_lookup(&expired);
        client.ensure_position_event_subscription();

        let mut event = position_closed_event(&closed_position(&open_position(&expired)));
        if let PositionEvent::PositionClosed(ref mut closed) = event {
            closed.instrument_id = InstrumentId::from("0xOTHER.OTHER");
        }

        publish_position_event("events.position.TEST".into(), &event);

        assert!(
            client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(client.neg_risk_index.contains_key(&expired.id()));
    }

    #[rstest]
    fn event_subscriptions_can_be_reinstalled_after_disconnect_cleanup() {
        let (mut client, _cache) = test_client();

        client.start_client();
        assert!(client.order_event_handler.is_none());
        assert!(client.position_event_handler.is_none());

        client.ensure_order_event_subscription();
        client.ensure_position_event_subscription();
        assert!(client.order_event_handler.is_some());
        assert!(client.position_event_handler.is_some());

        client.clear_order_event_subscription();
        client.clear_position_event_subscription();
        assert!(client.order_event_handler.is_none());
        assert!(client.position_event_handler.is_none());

        client.ensure_order_event_subscription();
        client.ensure_position_event_subscription();
        assert!(client.order_event_handler.is_some());
        assert!(client.position_event_handler.is_some());
    }

    #[rstest]
    fn order_reservations_follow_acceptance_and_reconciled_updates() {
        let (mut client, cache) = test_client();
        let instrument = test_binary_option("0xRESERVATION", false, false);
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        client.ensure_order_event_subscription();
        let order = cache_accepted_open_order(&mut cache.borrow_mut(), instrument.id());
        let accepted = TestOrderEventStubs::accepted(
            &order,
            client.core.account_id,
            order.venue_order_id().unwrap(),
        );
        publish_order_event(
            msgbus::switchboard::get_event_order_topic(order.strategy_id()),
            &accepted,
        );
        assert_eq!(
            *client.order_reservations.lock(),
            AHashMap::from([(order.client_order_id(), Money::from("5 pUSD"))])
        );

        let updated = OrderEventAny::Updated(
            OrderUpdatedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(order.client_order_id())
                .account_id(client.core.account_id)
                .venue_order_id(order.venue_order_id().unwrap())
                .quantity(Quantity::from("12"))
                .price(Price::from("0.7000"))
                .reconciliation(true)
                .build(),
        );
        cache.borrow_mut().update_order(&updated).unwrap();

        for _ in 0..2 {
            publish_order_event(
                msgbus::switchboard::get_event_order_topic(order.strategy_id()),
                &updated,
            );
        }
        assert_eq!(
            *client.order_reservations.lock(),
            AHashMap::from([(order.client_order_id(), Money::from("8.4 pUSD"))])
        );
    }

    #[rstest]
    fn open_orders_follow_order_events_for_stream_gap_reads() {
        let (mut client, cache) = test_client();
        let instrument = test_binary_option("0xSTREAM_GAP", false, false);
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        client.ensure_order_event_subscription();
        let order = cache_accepted_open_order(&mut cache.borrow_mut(), instrument.id());
        let topic = msgbus::switchboard::get_event_order_topic(order.strategy_id());

        let stream_gap_reads = |client: &PolymarketExecutionClient| {
            client
                .settlement
                .uncertain_orders()
                .into_iter()
                .map(|(venue_order_id, uncertain)| {
                    (
                        venue_order_id,
                        uncertain.instrument_id,
                        uncertain.noted_at,
                        uncertain.kind,
                    )
                })
                .collect::<Vec<_>>()
        };

        let accepted = TestOrderEventStubs::accepted(
            &order,
            client.core.account_id,
            VenueOrderId::from("V-001"),
        );

        // An order adopted from the venue has no adapter context
        publish_order_event(topic, &accepted);
        client.settlement.note_stream_gap(UnixNanos::from(1_u64));
        let without_context = stream_gap_reads(&client);

        client
            .order_contexts
            .register_context(VenueOrderId::from("V-001"), OrderContext::from(&order));
        publish_order_event(topic, &accepted);
        client.settlement.note_stream_gap(UnixNanos::from(1_u64));
        let after_accept = stream_gap_reads(&client);
        client
            .settlement
            .clear_uncertain_order(&VenueOrderId::from("V-001"));

        // A modify replacement moves the order to the venue order ID the read must target
        let updated = OrderEventAny::Updated(
            OrderUpdatedSpec::builder()
                .trader_id(order.trader_id())
                .strategy_id(order.strategy_id())
                .instrument_id(order.instrument_id())
                .client_order_id(order.client_order_id())
                .account_id(client.core.account_id)
                .venue_order_id(VenueOrderId::from("V-002"))
                .quantity(Quantity::from("10"))
                .price(Price::from("0.6000"))
                .build(),
        );
        cache.borrow_mut().update_order(&updated).unwrap();
        client
            .order_contexts
            .register_context(VenueOrderId::from("V-002"), OrderContext::from(&order));
        publish_order_event(topic, &updated);
        client.settlement.note_stream_gap(UnixNanos::from(2_u64));
        let after_replacement = stream_gap_reads(&client);
        client
            .settlement
            .clear_uncertain_order(&VenueOrderId::from("V-002"));

        let canceled = TestOrderEventStubs::canceled(
            &order,
            client.core.account_id,
            Some(VenueOrderId::from("V-002")),
        );
        cache.borrow_mut().update_order(&canceled).unwrap();
        publish_order_event(topic, &canceled);
        client.settlement.note_stream_gap(UnixNanos::from(3_u64));
        let after_cancel = stream_gap_reads(&client);

        assert!(without_context.is_empty());
        assert_eq!(
            after_accept,
            vec![(
                VenueOrderId::from("V-001"),
                instrument.id(),
                UnixNanos::from(1_u64),
                UncertainOrderKind::StreamGap,
            )]
        );
        assert_eq!(
            after_replacement,
            vec![(
                VenueOrderId::from("V-002"),
                instrument.id(),
                UnixNanos::from(2_u64),
                UncertainOrderKind::StreamGap,
            )]
        );
        assert!(after_cancel.is_empty());
    }

    #[rstest]
    #[case::unaccepted_cancel(false, true)]
    #[case::accepted_cancel(true, true)]
    #[case::unaccepted_update(false, false)]
    #[case::accepted_update(true, false)]
    fn order_reservations_require_acceptance_in_pending_states(
        #[case] accepted: bool,
        #[case] cancel: bool,
    ) {
        let (mut client, cache) = test_client();
        let instrument = test_binary_option("0xPENDING_RESERVATION", false, false);
        cache
            .borrow_mut()
            .add_instrument(instrument.clone())
            .unwrap();
        client.ensure_order_event_subscription();
        let order = if accepted {
            cache_accepted_open_order(&mut cache.borrow_mut(), instrument.id())
        } else {
            let order = open_limit_order(instrument.id());
            cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            cache
                .borrow_mut()
                .update_order(&TestOrderEventStubs::submitted(
                    &order,
                    client.core.account_id,
                ))
                .unwrap()
        };
        let event = if cancel {
            OrderEventAny::PendingCancel(
                OrderPendingCancelSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(client.core.account_id)
                    .maybe_venue_order_id(order.venue_order_id())
                    .build(),
            )
        } else {
            OrderEventAny::PendingUpdate(
                OrderPendingUpdateSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(client.core.account_id)
                    .maybe_venue_order_id(order.venue_order_id())
                    .build(),
            )
        };
        cache.borrow_mut().update_order(&event).unwrap();
        publish_order_event(
            msgbus::switchboard::get_event_order_topic(order.strategy_id()),
            &event,
        );
        let expected = if accepted {
            AHashMap::from([(order.client_order_id(), Money::from("5 pUSD"))])
        } else {
            AHashMap::new()
        };
        assert_eq!(*client.order_reservations.lock(), expected);
    }

    #[rstest]
    fn reset_clears_subscriptions_and_lookup_state() {
        let (mut client, _cache) = test_client();
        let expired = test_binary_option("0xRESET", true, true);
        client.upsert_execution_lookup(&expired);
        client.ensure_order_event_subscription();
        client.ensure_position_event_subscription();
        client.settlement.quarantine_invalid_trade("trade-1");

        client
            .order_reservations
            .lock()
            .insert(ClientOrderId::from("RESET-ORDER"), Money::from("5 pUSD"));

        client.reset_client();

        assert!(client.order_reservations.lock().is_empty());
        assert!(client.order_event_handler.is_none());
        assert!(client.position_event_handler.is_none());
        assert!(
            !client
                .shared_token_instruments
                .contains_key(&Ustr::from(expired.raw_symbol().as_str()))
        );
        assert!(!client.neg_risk_index.contains_key(&expired.id()));
        assert_eq!(client.settlement.record_count(), 0);
    }

    #[rstest]
    fn stop_preserves_settlement_records_for_reconnect() {
        let (mut client, _cache) = test_client();
        client.start_client();
        client
            .settlement
            .quarantine_invalid_trade("trade-reconnect");

        client.stop_client();

        assert_eq!(client.settlement.record_count(), 1);
        assert_eq!(
            client.settlement.pending_resolutions(),
            vec!["trade-reconnect".to_string()]
        );
    }

    #[rstest]
    fn decline_observer_hard_faults_this_accounts_pending_fill() {
        let (mut client, _cache) = test_client();
        client.ensure_settlement_observers();
        let instrument_id = InstrumentId::from("TOKEN-A.POLYMARKET");

        let leg = AdmittedLeg {
            venue_order_id: VenueOrderId::from("0xorder"),
            trade_id: TradeId::from("trade-declined"),
            instrument_id,
            order_side: OrderSide::Buy,
            liquidity_side: LiquiditySide::Taker,
            last_qty: Quantity::from("10.00"),
            last_px: Price::from("0.50"),
            commission: Money::from("0 pUSD"),
            ts_event: UnixNanos::from(1_000_u64),
        };

        client.settlement.note_order_submitted(leg.venue_order_id);
        client.settlement.admit_stream_trade(&AdmittedTrade {
            venue_trade_id: "trade-declined".to_string(),
            status: PolymarketTradeStatus::Matched,
            legs: vec![leg.clone()],
        });

        client.settlement.note_leg_enqueued(&leg.trade_id);

        let declined = |account_id: &str| {
            OrderEventAny::Filled(
                OrderFilledSpec::builder()
                    .instrument_id(instrument_id)
                    .venue_order_id(leg.venue_order_id)
                    .account_id(AccountId::from(account_id))
                    .trade_id(leg.trade_id)
                    .last_qty(leg.last_qty)
                    .last_px(leg.last_px)
                    .currency(Currency::pUSD())
                    .build(),
            )
        };

        let topic = switchboard::get_order_fill_declined_topic(instrument_id);

        publish_order_event(topic, &declined("POLYMARKET-OTHER"));
        let fault_after_other_account = trade_hard_fault(&client.settlement, "trade-declined");
        publish_order_event(topic, &declined("POLYMARKET-001"));

        assert!(fault_after_other_account.is_none());
        assert!(trade_hard_fault(&client.settlement, "trade-declined").is_some());
        assert_eq!(
            leg_application(&client.settlement, &leg.trade_id),
            Some(LegApplication::Absent)
        );
    }

    #[rstest]
    fn void_observer_records_applied_correction_void() {
        let (mut client, _cache) = test_client();
        client.ensure_settlement_observers();
        let instrument_id = InstrumentId::from("TOKEN-A.POLYMARKET");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument_id)
            .venue_order_id(VenueOrderId::from("0xorder"))
            .account_id(AccountId::from("POLYMARKET-001"))
            .trade_id(TradeId::from("trade-voided"))
            .last_qty(Quantity::from("10.00"))
            .last_px(Price::from("0.50"))
            .currency(Currency::pUSD())
            .liquidity_side(LiquiditySide::Taker)
            .build();
        client.settlement.hydrate_fill(&fill);
        let voided = OrderEventAny::FillVoided(
            OrderFillVoidedSpec::builder()
                .instrument_id(instrument_id)
                .client_order_id(fill.client_order_id)
                .venue_order_id(fill.venue_order_id)
                .account_id(fill.account_id)
                .trade_id(fill.trade_id)
                .voided_qty(fill.last_qty)
                .last_px(fill.last_px)
                .currency(fill.currency)
                .liquidity_side(fill.liquidity_side)
                .build(),
        );

        publish_order_event(
            switchboard::get_order_fill_voided_topic(instrument_id),
            &voided,
        );

        assert_eq!(
            leg_application(&client.settlement, &fill.trade_id),
            Some(LegApplication::VoidObserved)
        );
    }

    #[rstest]
    fn client_fault_refuses_commands_and_reports_disconnected() {
        let (client, _cache) = test_client();
        client.core.set_connected();

        let cancel_all = || {
            CancelAllOrders::new(
                TraderId::from("TESTER-001"),
                None,
                StrategyId::from("S-001"),
                InstrumentId::from("TOKEN-A.POLYMARKET"),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        };

        let connected_before = client.is_connected();
        let refused_before = client
            .cancel_all_orders(cancel_all())
            .unwrap_err()
            .to_string();

        force_client_fault(&client.settlement, "capacity exhausted");

        assert!(connected_before);
        assert!(!refused_before.contains("faulted closed"));
        assert!(!client.is_connected());
        assert_eq!(
            client
                .cancel_all_orders(cancel_all())
                .unwrap_err()
                .to_string(),
            "Polymarket execution client is faulted closed until restart: capacity exhausted"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn report_generation_fails_closed_while_settlement_evidence_is_unresolved() {
        let (client, _cache) = test_client();
        client
            .settlement
            .quarantine_invalid_trade("trade-unresolved");
        let fill_reports_cmd = GenerateFillReportsBuilder::default()
            .ts_init(UnixNanos::default())
            .build()
            .unwrap();

        let mass_status = client.generate_mass_status(None).await;
        let fill_reports = client.generate_fill_reports(fill_reports_cmd).await;

        assert_eq!(
            mass_status.unwrap_err().to_string(),
            "cannot generate mass status: Polymarket settlement registry holds 1 record(s) \
             with unresolved evidence"
        );
        assert_eq!(
            fill_reports.unwrap_err().to_string(),
            "cannot generate fill reports: Polymarket settlement registry holds 1 record(s) \
             with unresolved evidence"
        );
    }

    #[rstest]
    #[case::missing(&["trade-other"], None)]
    #[case::exact(&["trade-other", "trade-target"], Some(1))]
    #[case::duplicated(&["trade-target", "trade-target"], None)]
    fn targeted_trade_row_requires_exactly_one_match(
        #[case] row_ids: &[&str],
        #[case] expected_index: Option<usize>,
    ) {
        let template: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json")).unwrap();

        let trades: Vec<PolymarketTradeReport> = row_ids
            .iter()
            .map(|id| PolymarketTradeReport {
                id: (*id).to_string(),
                ..template.clone()
            })
            .collect();

        let row = targeted_trade_row(&trades, "trade-target");

        assert_eq!(
            row.map(std::ptr::from_ref),
            expected_index.map(|index| std::ptr::from_ref(&trades[index]))
        );
    }

    #[rstest]
    fn resolution_schedule_retries_pending_trade_with_capped_backoff() {
        let mut schedule = ResolutionSchedule::default();
        let pending = || vec!["trade-1".to_string()];
        let start = Instant::now();

        let first = schedule.due(pending(), start, usize::MAX);
        let within_backoff =
            schedule.due(pending(), start + Duration::from_millis(400), usize::MAX);
        let second = schedule.due(pending(), start + Duration::from_millis(500), usize::MAX);
        let within_doubled =
            schedule.due(pending(), start + Duration::from_millis(1_400), usize::MAX);
        let third = schedule.due(pending(), start + Duration::from_millis(1_500), usize::MAX);

        assert_eq!(first, pending());
        assert!(within_backoff.is_empty());
        assert_eq!(second, pending());
        assert!(within_doubled.is_empty());
        assert_eq!(third, pending());
    }

    #[rstest]
    fn resolution_schedule_restarts_after_trade_leaves_pending() {
        let mut schedule = ResolutionSchedule::default();
        let start = Instant::now();
        schedule.due(vec!["trade-1".to_string()], start, usize::MAX);

        let resolved = schedule.due(Vec::new(), start + Duration::from_millis(100), usize::MAX);
        let quarantined_again = schedule.due(
            vec!["trade-1".to_string()],
            start + Duration::from_millis(200),
            usize::MAX,
        );

        assert!(resolved.is_empty());
        assert_eq!(quarantined_again, vec!["trade-1".to_string()]);
    }

    #[rstest]
    #[tokio::test]
    async fn resolve_due_uncertain_orders_reads_at_most_limit_per_sweep() {
        // Nothing listens on port 1, so each read fails fast and its order stays uncertain
        let (client, _cache) =
            test_client_with_proxy_and_http_urls(None, "http://127.0.0.1:1", "http://127.0.0.1:1");
        let order_count = UNCERTAIN_ORDER_READS_PER_SWEEP + 2;

        for index in 0..order_count {
            client.settlement.note_order_open(
                ClientOrderId::from(format!("O-{index}").as_str()),
                VenueOrderId::from(format!("V-{index}").as_str()),
                InstrumentId::from("TOKEN-A.POLYMARKET"),
            );
        }

        client
            .settlement
            .note_stream_gap(client.clock.get_time_ns());
        let user_address = client
            .secrets
            .funder
            .clone()
            .unwrap_or_else(|| client.secrets.address.clone());

        let ctx = WsDispatchContext {
            signer_type: client.config.signer_type,
            token_instruments: &client.shared_token_instruments,
            fill_tracker: &client.fill_tracker,
            settlement: &client.settlement,
            pending_submits: &client.pending_submits,
            order_contexts: &client.order_contexts,
            emitter: &client.emitter,
            account_id: client.core.account_id,
            clock: client.clock,
            user_address: &user_address,
            user_api_key: client.secrets.credential.api_key_str(),
        };

        let mut schedule = ResolutionSchedule::default();

        resolve_due_uncertain_orders(
            &mut schedule,
            &client.http_client,
            &ctx,
            &client.ws_dispatch_state,
        )
        .await;

        assert_eq!(schedule.attempts.len(), UNCERTAIN_ORDER_READS_PER_SWEEP);
        assert_eq!(client.settlement.uncertain_orders().len(), order_count);
    }

    #[rstest]
    fn resolution_schedule_limit_defers_remaining_keys_without_backoff() {
        let mut schedule = ResolutionSchedule::default();
        let pending = || vec!["order-1", "order-2", "order-3"];
        let start = Instant::now();

        let first = schedule.due(pending(), start, 2);
        let deferred = schedule.due(pending(), start, 2);
        let within_backoff = schedule.due(pending(), start + Duration::from_millis(400), 2);
        let retried = schedule.due(pending(), start + Duration::from_millis(500), 2);

        assert_eq!(first, vec!["order-1", "order-2"]);
        assert_eq!(deferred, vec!["order-3"]);
        assert!(within_backoff.is_empty());
        assert_eq!(retried, vec!["order-1", "order-2"]);
    }

    #[rstest]
    fn resolution_schedule_limit_reaches_every_key_when_sweeps_outlast_backoff() {
        let mut schedule = ResolutionSchedule::default();
        let pending = || vec!["order-1", "order-2", "order-3"];
        let start = Instant::now();

        let first = schedule.due(pending(), start, 2);

        // The sweep reading the first batch outlasts even the maximum backoff
        let second = schedule.due(pending(), start + RESOLUTION_BACKOFF_MAX, 2);

        assert_eq!(first, vec!["order-1", "order-2"]);
        assert_eq!(second, vec!["order-3", "order-1"]);
    }

    #[rstest]
    #[case(0, 500)]
    #[case(1, 1_000)]
    #[case(5, 16_000)]
    #[case(6, 30_000)]
    #[case(40, 30_000)]
    fn resolution_backoff_doubles_to_cap(#[case] completed: u32, #[case] expected_ms: u64) {
        assert_eq!(
            resolution_backoff(completed),
            Duration::from_millis(expected_ms)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn reset_preserves_modifies_until_pending_task_shutdown() {
        let (mut client, cache) = test_client();
        let instrument = test_binary_option("TEST", false, false);
        let instrument_id = instrument.id();
        let abandoned_order = {
            let mut cache = cache.borrow_mut();
            cache.add_instrument(instrument).unwrap();
            cache_accepted_open_order(&mut cache, instrument_id)
        };
        let abandoned_client_order_id = abandoned_order.client_order_id();
        let unresolved_client_order_id = ClientOrderId::from("O-UNRESOLVED-MODIFY");
        let abandoned_venue_order_id = abandoned_order.venue_order_id().unwrap();
        let unresolved_venue_order_id = VenueOrderId::from("V-UNRESOLVED");
        let replacement_venue_order_id = VenueOrderId::from("V-REPLACEMENT");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        {
            let mut state = client.ws_dispatch_state.lock();
            assert!(state.begin_modify(
                abandoned_client_order_id,
                abandoned_venue_order_id,
                instrument_id,
            ));
            assert!(state.confirm_modify_cancel(
                abandoned_client_order_id,
                abandoned_venue_order_id,
                UnixNanos::from(123),
            ));
            assert!(state.begin_modify(
                unresolved_client_order_id,
                unresolved_venue_order_id,
                instrument_id,
            ));
            assert!(state.set_modify_replacement(
                unresolved_client_order_id,
                replacement_venue_order_id,
                Quantity::from("12"),
                Quantity::from("10"),
                Price::from("0.5"),
            ));
        }

        client
            .pending_tasks
            .spawn(std::future::pending::<()>())
            .unwrap();

        client.reset_client();
        {
            let mut state = client.ws_dispatch_state.lock();
            assert!(!state.begin_modify(
                abandoned_client_order_id,
                abandoned_venue_order_id,
                instrument_id,
            ));
            assert_eq!(
                state
                    .pending_modify_promotion(replacement_venue_order_id)
                    .unwrap()
                    .client_order_id,
                unresolved_client_order_id,
            );
        }

        client.pending_tasks.abort();
        client.await_pending_tasks().await.unwrap();

        let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("shutdown should emit a modify outcome")
            .expect("execution event channel should remain open");
        let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event else {
            panic!("expected ModifyRejected, was {event:?}");
        };
        assert_eq!(rejected.client_order_id, abandoned_client_order_id);
        assert_eq!(rejected.venue_order_id, Some(abandoned_venue_order_id));
        assert_eq!(
            rejected.reason,
            "Polymarket modification was interrupted during shutdown"
        );

        let event = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("confirmed cancel should close the old venue leg")
            .expect("execution event channel should remain open");
        let ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) = event else {
            panic!("expected Canceled, was {event:?}");
        };
        assert_eq!(canceled.client_order_id, abandoned_client_order_id);
        assert_eq!(canceled.venue_order_id, Some(abandoned_venue_order_id));
        assert_eq!(canceled.ts_event, UnixNanos::from(123));

        let mut state = client.ws_dispatch_state.lock();
        assert!(state.begin_modify(
            abandoned_client_order_id,
            abandoned_venue_order_id,
            instrument_id,
        ));
        assert!(!state.begin_modify(
            unresolved_client_order_id,
            unresolved_venue_order_id,
            instrument_id,
        ));
        assert_eq!(
            state
                .pending_modify_promotion(replacement_venue_order_id)
                .unwrap()
                .client_order_id,
            unresolved_client_order_id,
        );
    }
}
