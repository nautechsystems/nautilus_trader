// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Live execution engine for managing execution state and reconciliation.
//!
//! This module provides the orchestration layer for live trading execution,
//! coordinating between the core execution engine and venue-specific clients
//! while managing state reconciliation.

use std::{
    cell::{Ref, RefCell},
    collections::HashSet,
    fmt::{Debug, Display},
    rc::Rc,
    time::Duration,
};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    messages::{ExecutionEvent, ExecutionReport as ExecReportEnum, execution::TradingCommand},
    msgbus::{self, MessageBus, switchboard},
};
use nautilus_execution::client::ExecutionClient;
use nautilus_model::{
    events::OrderEventAny,
    identifiers::{ClientId, ClientOrderId, InstrumentId},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};

use crate::{
    config::LiveExecEngineConfig,
    reconciliation::{ExecutionReport, ReconciliationConfig, ReconciliationManager},
};

/// Live execution engine that manages execution state and reconciliation.
///
/// The `LiveExecutionEngine` orchestrates:
/// - Startup reconciliation with all venues.
/// - Continuous reconciliation of execution reports.
/// - Inflight order checking and resolution.
/// - Message routing between venues and the core execution engine.
#[allow(dead_code)]
pub struct LiveExecutionEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    reconciliation: ReconciliationManager,
    config: LiveExecEngineConfig,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<TradingCommand>,
    cmd_rx: Option<tokio::sync::mpsc::UnboundedReceiver<TradingCommand>>,
    evt_tx: tokio::sync::mpsc::UnboundedSender<OrderEventAny>,
    evt_rx: Option<tokio::sync::mpsc::UnboundedReceiver<OrderEventAny>>,
    reconciliation_task: Option<tokio::task::JoinHandle<()>>,
    inflight_check_task: Option<tokio::task::JoinHandle<()>>,
    open_check_task: Option<tokio::task::JoinHandle<()>>,
    is_running: bool,
    shutdown_initiated: bool,
}

impl Debug for LiveExecutionEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(LiveExecutionEngine))
            .field("config", &self.config)
            .field("is_running", &self.is_running)
            .field("shutdown_initiated", &self.shutdown_initiated)
            .finish()
    }
}

impl LiveExecutionEngine {
    /// Creates a new [`LiveExecutionEngine`] instance.
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        config: LiveExecEngineConfig,
    ) -> Self {
        let filtered_client_order_ids: HashSet<ClientOrderId> = config
            .filtered_client_order_ids
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| ClientOrderId::from(value.as_str()))
            .collect();

        let reconciliation_instrument_ids: HashSet<InstrumentId> = config
            .reconciliation_instrument_ids
            .clone()
            .unwrap_or_default()
            .into_iter()
            .map(|value| InstrumentId::from(value.as_str()))
            .collect();

        let reconciliation_config = ReconciliationConfig {
            lookback_mins: config.reconciliation_lookback_mins.map(|m| m as u64),
            inflight_threshold_ms: config.inflight_check_threshold_ms as u64,
            inflight_max_retries: config.inflight_check_retries,
            filter_unclaimed_external: config.filter_unclaimed_external_orders,
            generate_missing_orders: config.generate_missing_orders,
            filtered_client_order_ids,
            open_check_threshold_ns: (config.open_check_threshold_ms as u64) * 1_000_000,
            open_check_missing_retries: config.open_check_missing_retries,
            open_check_open_only: config.open_check_open_only,
            open_check_lookback_mins: config.open_check_lookback_mins.map(|m| m as u64),
            filter_position_reports: config.filter_position_reports,
            reconciliation_instrument_ids,
        };

        let reconciliation =
            ReconciliationManager::new(clock.clone(), cache.clone(), reconciliation_config);

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (evt_tx, evt_rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            clock,
            cache,
            msgbus,
            reconciliation,
            config,
            cmd_tx,
            cmd_rx: Some(cmd_rx),
            evt_tx,
            evt_rx: Some(evt_rx),
            reconciliation_task: None,
            inflight_check_task: None,
            open_check_task: None,
            is_running: false,
            shutdown_initiated: false,
        }
    }

    /// Starts the live execution engine.
    ///
    /// This initiates:
    /// - Startup reconciliation with all venues.
    /// - Continuous reconciliation tasks.
    /// - Message processing loops.
    ///
    /// # Errors
    ///
    /// Returns an error if startup reconciliation fails.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.is_running {
            return Ok(());
        }

        log::info!("Starting LiveExecutionEngine");

        if self.config.reconciliation {
            self.reconcile_execution_state().await?;
        }

        self.start_continuous_tasks();

        self.is_running = true;
        log::info!("LiveExecutionEngine started");

        Ok(())
    }

    /// Stops the live execution engine.
    ///
    /// # Errors
    ///
    /// Returns an error if stopping tasks fails.
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if !self.is_running {
            return Ok(());
        }

        log::info!("Stopping LiveExecutionEngine");
        self.shutdown_initiated = true;

        // Cancel all tasks
        if let Some(task) = self.reconciliation_task.take() {
            task.abort();
        }
        if let Some(task) = self.inflight_check_task.take() {
            task.abort();
        }
        if let Some(task) = self.open_check_task.take() {
            task.abort();
        }

        self.is_running = false;
        log::info!("LiveExecutionEngine stopped");

        Ok(())
    }

    /// Reconciles execution state at startup.
    #[allow(dead_code)]
    async fn reconcile_execution_state(&mut self) -> anyhow::Result<()> {
        log::info!("Running startup reconciliation");

        // Add startup delay to let connections stabilize
        if self.config.reconciliation_startup_delay_secs > 0.0 {
            let delay_secs = self.config.reconciliation_startup_delay_secs;
            log::info!("Waiting {}s before reconciliation", delay_secs);
            tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;
        }

        // TODO: Get all registered clients from ExecutionEngine
        // For now, this is a placeholder
        // let clients = self.get_execution_clients();
        // for client in clients {
        //     let lookback_mins = self.config.reconciliation_lookback_mins;
        //     let mass_status = client.generate_mass_status(lookback_mins).await?;
        //     if let Some(mass_status) = mass_status {
        //         self.reconcile_execution_mass_status(mass_status).await?;
        //     }
        // }

        log::info!("Startup reconciliation complete");
        Ok(())
    }

    /// Reconciles execution mass status report.
    #[allow(dead_code)]
    async fn reconcile_execution_mass_status(
        &mut self,
        mass_status: ExecutionMassStatus,
    ) -> anyhow::Result<()> {
        log::info!(
            "Processing mass status for {}: {} orders, {} fills",
            mass_status.venue,
            mass_status.order_reports().len(),
            mass_status.fill_reports().len()
        );

        let events = self
            .reconciliation
            .reconcile_execution_mass_status(mass_status)
            .await;

        // Publish events to execution engine
        for event in events {
            self.publish_event(event);
        }

        Ok(())
    }

    /// Reconciles an execution report.
    pub fn reconcile_execution_report(&mut self, report: ExecutionReport) {
        log::debug!("{RECV} {report:?}");

        let events = self.reconciliation.reconcile_report(report);

        // Publish events to execution engine
        for event in events {
            self.publish_event(event);
        }
    }

    /// Handles a trading command.
    pub fn handle_command(&mut self, command: TradingCommand) {
        log::debug!("{CMD} {command:?}");

        // Commands would be forwarded to appropriate execution client
    }

    /// Publishes an event to the execution engine.
    fn publish_event(&mut self, event: OrderEventAny) {
        log::debug!("{EVT} {event:?}");

        let topic = switchboard::get_event_orders_topic(event.strategy_id());
        msgbus::publish(topic, &event);
    }

    /// Records local order activity for reconciliation tracking.
    pub fn record_local_activity(&mut self, event: &OrderEventAny) {
        let client_order_id = event.client_order_id();
        let mut ts_event = event.ts_event();
        if ts_event.is_zero() {
            ts_event = self.clock.borrow().timestamp_ns();
        }
        self.reconciliation
            .record_local_activity(client_order_id, ts_event);
    }

    /// Clears reconciliation tracking for an order.
    pub fn clear_reconciliation_tracking(
        &mut self,
        client_order_id: &ClientOrderId,
        drop_last_query: bool,
    ) {
        self.reconciliation
            .clear_recon_tracking(client_order_id, drop_last_query);
    }

    /// Starts continuous reconciliation tasks.
    fn start_continuous_tasks(&mut self) {
        if self.config.inflight_check_interval_ms > 0 {
            let interval_ms = self.config.inflight_check_interval_ms as u64;
            self.start_inflight_check_task(interval_ms);
        }

        if let Some(interval_secs) = self.config.open_check_interval_secs
            && interval_secs > 0.0
        {
            self.start_open_check_task(interval_secs);
        }
    }

    fn start_inflight_check_task(&mut self, _interval_ms: u64) {
        log::warn!("Inflight check task not yet implemented due to Send constraints");
    }

    fn start_open_check_task(&mut self, _interval_secs: f64) {
        log::warn!("Open check task not yet implemented due to Send constraints");
    }

    // TODO: Implement when LiveExecutionClient is available
    // fn get_execution_clients(&self) -> Vec<Rc<dyn LiveExecutionClient>> {
    //     Vec::new()
    // }

    /// Returns whether the engine is currently running.
    pub fn is_running(&self) -> bool {
        self.is_running
    }
}

/// Extension trait for live execution clients with message channel support.
pub trait LiveExecutionClientExt: ExecutionClient {
    /// Gets the message channel for sending execution events.
    fn get_message_channel(&self) -> tokio::sync::mpsc::UnboundedSender<ExecutionEvent>;

    /// Gets the clock for timestamp generation.
    fn get_clock(&self) -> Ref<'_, dyn Clock>;

    /// Sends an order event to the execution engine.
    fn send_order_event(&self, event: OrderEventAny) {
        if let Err(e) = self
            .get_message_channel()
            .send(ExecutionEvent::Order(event))
        {
            log_send_error(&self.client_id(), &e);
        }
    }

    /// Sends an order status report to the execution engine.
    fn send_order_status_report(&self, report: OrderStatusReport) {
        let exec_report = ExecReportEnum::OrderStatus(Box::new(report));
        if let Err(e) = self
            .get_message_channel()
            .send(ExecutionEvent::Report(exec_report))
        {
            log_send_error(&self.client_id(), &e);
        }
    }

    /// Sends a fill report to the execution engine.
    fn send_fill_report(&self, report: FillReport) {
        let exec_report = ExecReportEnum::Fill(Box::new(report));
        if let Err(e) = self
            .get_message_channel()
            .send(ExecutionEvent::Report(exec_report))
        {
            log_send_error(&self.client_id(), &e);
        }
    }

    /// Sends a position status report to the execution engine.
    fn send_position_status_report(&self, report: PositionStatusReport) {
        let exec_report = ExecReportEnum::Position(Box::new(report));
        if let Err(e) = self
            .get_message_channel()
            .send(ExecutionEvent::Report(exec_report))
        {
            log_send_error(&self.client_id(), &e);
        }
    }

    /// Sends a mass status report to the execution engine.
    fn send_mass_status(&self, mass_status: ExecutionMassStatus) {
        let exec_report = ExecReportEnum::Mass(Box::new(mass_status));
        if let Err(e) = self
            .get_message_channel()
            .send(ExecutionEvent::Report(exec_report))
        {
            log_send_error(&self.client_id(), &e);
        }
    }
}

#[inline(always)]
fn log_send_error<E: Display>(client_id: &ClientId, e: &E) {
    log::error!("ExecutionClient-{client_id} failed to send message: {e}");
}
