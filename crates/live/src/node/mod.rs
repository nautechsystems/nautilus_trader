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

//! Live trading node built on a single-threaded tokio event loop.
//!
//! `LiveNode::run()` drives the system through a `tokio::select!` loop that
//! multiplexes data events, execution events, trading commands, timers, and
//! periodic maintenance tasks (reconciliation, purge, prune, audit).
//!
//! # Threading model
//!
//! The core types (`ExecutionManager`, `ExecutionEngine`, `Cache`) use
//! `Rc<RefCell<..>>` and are `!Send`. All access happens on the same thread.
//! The `select!` macro runs one branch to completion (including inner awaits)
//! before polling the next, so `RefCell` borrows held across `.await` points
//! within a single branch cannot conflict with borrows in other branches.
//!
//! # Startup sequencing
//!
//! Startup connects clients in two phases so that instruments are in the
//! cache before execution clients read them:
//!
//! 1. Connect data clients (instruments arrive as buffered `DataEvent`s).
//! 2. Flush all pending data events and commands into the cache via
//!    `flush_pending_data`, which loops `try_recv` on the channel receivers
//!    until no items remain.
//! 3. Connect execution clients (`load_instruments_from_cache` now finds
//!    populated instruments).
//! 4. Drain remaining events, then run reconciliation.
//!
//! Both `run()` (integrated event loop) and `start()` (manual lifecycle)
//! follow this sequence.
//!
//! # Reconciliation
//!
//! Continuous inflight and open-order checks run on independent intervals. The
//! shared maintenance timer in the select loop dispatches reconciliation at
//! the minimum enabled interval. Each dispatch the handler checks which
//! sub-checks are due based on elapsed nanoseconds and schedules their work.
//! Continuous checks do not await venue HTTP in the select loop: open-order
//! and position checks poll bulk venue report futures from the loop.
//!
//! # Maintenance dispatcher
//!
//! Six periodic tasks share a single coarse `maintenance_timer`:
//!
//! - reconciliation (inflight, open, position sub-checks)
//! - purge closed orders
//! - purge closed positions
//! - purge account events
//! - own-books audit
//! - recent-fills cache prune
//!
//! The runner wakes one timer per loop iteration regardless of how many
//! maintenance tasks are configured. Each task tracks its own
//! `next_fire: Instant` and the dispatcher fires the bodies whose deadline
//! has passed, rescheduling `next = now + interval` (equivalent to
//! `MissedTickBehavior::Delay`). Disabled tasks anchor on a far-future
//! `next` that never trips.
//!
//! The 100ms timer cadence is the effective floor for any maintenance
//! interval. Configured intervals below 100ms (the config types allow
//! `inflight_check_interval_ms` and `own_books_audit_interval_secs` smaller)
//! get rounded up to the next tick. Real workloads do not run venue or cache
//! maintenance below 100ms (defaults are seconds to minutes). Cadence drifts
//! by at most one body duration per fire.

use std::{fmt::Debug, future::Future, pin::Pin, time::Duration};

use indexmap::IndexSet;
use nautilus_common::{
    actor::{Actor, DataActor, DataActorNative},
    cache::database::CacheDatabaseAdapter,
    component::Component,
    enums::{Environment, LogColor},
    live::dst,
    log_info,
    messages::{
        DataEvent, ExecutionEvent, ExecutionReport,
        data::DataCommand,
        execution::{GenerateOrderStatusReports, GeneratePositionStatusReports, TradingCommand},
    },
    msgbus::{self, BusMessage},
    timer::TimeEventHandler,
};
use nautilus_core::{
    UUID4, UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, mins_to_secs, secs_to_nanos_unchecked},
};
use nautilus_model::{
    events::OrderEventAny,
    identifiers::{ClientOrderId, TraderId, Venue},
    orders::Order,
    reports::{OrderStatusReport, PositionStatusReport},
};
use nautilus_system::{config::NautilusKernelConfig, kernel::NautilusKernel};
use nautilus_trading::{
    ExecutionAlgorithm, ExecutionAlgorithmNative,
    strategy::{Strategy, StrategyNative},
};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    execution::{
        client::LiveExecutionClient,
        manager::{
            ExecutionManager, ExecutionManagerConfig, OpenOrderReportCheck, PositionReportCheck,
        },
    },
    runner::{AsyncRunner, AsyncRunnerChannels},
};

pub mod builder;
pub mod config;
#[cfg(feature = "plugin")]
pub mod plugin;
mod state;

use builder::ExternalMessageBusIngress;
pub use builder::LiveNodeBuilder;
use config::{LiveNodeConfig, PluginConfig};
use state::EngineConnectionStatus;
pub use state::{LiveNodeHandle, NodeState};

/// High-level abstraction for a live Nautilus system node.
///
/// Provides a simplified interface for running live systems
/// with automatic client management and lifecycle handling.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", unsendable)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")
)]
pub struct LiveNode {
    kernel: NautilusKernel,
    runner: Option<AsyncRunner>,
    config: LiveNodeConfig,
    handle: LiveNodeHandle,
    exec_manager: ExecutionManager,
    exec_clients: Vec<LiveExecutionClient>,
    external_msgbus: Option<ExternalMessageBusIngress>,
    shutdown_deadline: Option<dst::time::Instant>,
    #[cfg(feature = "plugin")]
    plugins: plugin::NodePlugins,
    #[cfg(feature = "python")]
    #[allow(dead_code)] // TODO: Under development
    python_actors: Vec<pyo3::Py<pyo3::PyAny>>,
}

impl LiveNode {
    /// Creates a new `LiveNode` from builder components.
    ///
    /// This is an internal constructor used by `LiveNodeBuilder`.
    #[must_use]
    pub(crate) fn new_from_builder(
        kernel: NautilusKernel,
        runner: AsyncRunner,
        config: LiveNodeConfig,
        exec_manager: ExecutionManager,
        exec_clients: Vec<LiveExecutionClient>,
        external_msgbus: Option<ExternalMessageBusIngress>,
    ) -> Self {
        Self {
            kernel,
            runner: Some(runner),
            config,
            handle: LiveNodeHandle::new(),
            exec_manager,
            exec_clients,
            external_msgbus,
            shutdown_deadline: None,
            #[cfg(feature = "plugin")]
            plugins: plugin::NodePlugins,
            #[cfg(feature = "python")]
            python_actors: Vec::new(),
        }
    }

    /// Creates a new [`LiveNodeBuilder`] for fluent configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment is invalid for live trading.
    pub fn builder(
        trader_id: TraderId,
        environment: Environment,
    ) -> anyhow::Result<LiveNodeBuilder> {
        LiveNodeBuilder::new(trader_id, environment)
    }

    /// Creates a new [`LiveNode`] directly from a kernel name and optional configuration.
    ///
    /// This is a convenience method for creating a live node with a pre-configured
    /// kernel configuration, bypassing the builder pattern. If no config is provided,
    /// a default configuration will be used.
    ///
    /// # Errors
    ///
    /// Returns an error if kernel construction fails.
    pub fn build(name: String, config: Option<LiveNodeConfig>) -> anyhow::Result<Self> {
        let mut config = config.unwrap_or_default();
        config.environment = Environment::Live;

        match config.environment() {
            Environment::Sandbox | Environment::Live => {}
            Environment::Backtest => {
                anyhow::bail!("LiveNode cannot be used with Backtest environment");
            }
        }

        config.validate_runtime_support()?;

        if config.event_store.is_some() {
            anyhow::bail!(
                "LiveNodeConfig.event_store is set but LiveNode::build cannot install a factory; \
                 use LiveNodeBuilder::with_event_store(...) instead"
            );
        }

        let runner = AsyncRunner::new();
        runner.bind_senders();

        let kernel = NautilusKernel::new(name, config.clone())?;

        let exec_manager_config =
            ExecutionManagerConfig::from(&config.exec_engine).with_trader_id(config.trader_id);
        let exec_manager = ExecutionManager::new(
            kernel.clock.clone(),
            kernel.cache.clone(),
            exec_manager_config,
        );

        let node = Self {
            kernel,
            runner: Some(runner),
            config,
            handle: LiveNodeHandle::new(),
            exec_manager,
            exec_clients: Vec::new(),
            external_msgbus: None,
            shutdown_deadline: None,
            #[cfg(feature = "plugin")]
            plugins: plugin::NodePlugins,
            #[cfg(feature = "python")]
            python_actors: Vec::new(),
        };
        node.load_configured_plugins()?;

        log::info!("LiveNode built successfully with kernel config");

        Ok(node)
    }

    /// Loads and registers plug-ins declared on the node config.
    ///
    /// # Errors
    ///
    /// Returns an error when plug-ins are configured without `nautilus-plugin-host`.
    #[cfg(feature = "plugin")]
    pub(crate) fn load_configured_plugins(&self) -> anyhow::Result<()> {
        if self.config.plugins.is_empty() {
            return Ok(());
        }

        anyhow::bail!(
            "LiveNodeConfig.plugins requires nautilus-plugin-host; nautilus-plugin is the guest SDK only"
        )
    }

    /// Loads and registers plug-ins declared on the node config.
    ///
    /// # Errors
    ///
    /// Returns an error when plug-ins are configured without `nautilus-plugin-host`.
    #[cfg(not(feature = "plugin"))]
    pub(crate) fn load_configured_plugins(&self) -> anyhow::Result<()> {
        if self.config.plugins.is_empty() {
            return Ok(());
        }

        anyhow::bail!(
            "LiveNodeConfig.plugins requires nautilus-plugin-host; nautilus-plugin is the guest SDK only"
        )
    }

    /// Loads and registers one plug-in instance.
    ///
    /// # Errors
    ///
    /// Returns an error because dynamic plug-in hosting lives in `nautilus-plugin-host`.
    #[cfg(feature = "plugin")]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "signature mirrors the host-enabled API"
    )]
    pub fn add_plugin(&mut self, config: PluginConfig) -> anyhow::Result<()> {
        config.validate_runtime_support(self.config.plugins.len())?;

        anyhow::bail!(
            "LiveNode::add_plugin requires nautilus-plugin-host; nautilus-plugin is the guest SDK only"
        )
    }

    /// Rejects plug-in registration when host support is not linked.
    ///
    /// # Errors
    ///
    /// Always returns an error explaining that `nautilus-plugin-host` is required.
    #[cfg(not(feature = "plugin"))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "signature mirrors the plugin-enabled API"
    )]
    pub fn add_plugin(&mut self, config: PluginConfig) -> anyhow::Result<()> {
        let _ = config;
        anyhow::bail!(
            "LiveNode::add_plugin requires nautilus-plugin-host; nautilus-plugin is the guest SDK only"
        )
    }

    /// Returns a thread-safe handle to control this node.
    #[must_use]
    pub fn handle(&self) -> LiveNodeHandle {
        self.handle.clone()
    }

    /// Starts the live node without entering a select loop.
    ///
    /// Connects clients, runs reconciliation, and starts the trader, but does
    /// not consume the runner or drive channel receivers. Channel traffic that
    /// arrives after startup is not serviced until the caller provides a loop.
    ///
    /// For a self-contained entry point that owns the event loop, use [`run`](Self::run).
    ///
    /// # Errors
    ///
    /// Returns an error if startup fails.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.state().is_running() {
            anyhow::bail!("Already running");
        }

        if let Some(runner) = self.runner.as_ref() {
            runner.bind_senders();
        }

        self.handle.set_state(NodeState::Starting);

        self.kernel.reset_shutdown_flag();
        self.kernel.start_async().await;

        if self.kernel.is_event_store_replay() {
            log::info!(
                "Event-store replay loaded; skipping live client connection and reconciliation",
            );
            self.handle.set_state(NodeState::Running);
            return Ok(());
        }

        if self.kernel.is_event_store_replay_configured() {
            self.abort_startup("Event-store replay did not start")
                .await?;
            return Ok(());
        }

        // Connect data clients first and flush instrument events into cache
        self.kernel.connect_data_clients().await;

        if let Some(runner) = self.runner.as_mut() {
            runner.flush_pending_data();
        }

        self.kernel.connect_exec_clients().await;

        if let Some(reason) = self.startup_abort_reason() {
            self.abort_startup(reason).await?;
            return Ok(());
        }

        match self.await_engines_connected().await {
            EngineConnectionStatus::Connected => {}
            EngineConnectionStatus::TimedOut => {
                log::error!("Cannot start trader: engine client(s) not connected");
                self.handle.set_state(NodeState::Running);
                return Ok(());
            }
            EngineConnectionStatus::StopRequested => {
                self.abort_startup("Stop signal received during startup")
                    .await?;
                return Ok(());
            }
            EngineConnectionStatus::ShutdownRequested => {
                self.abort_startup("Shutdown signal received during startup")
                    .await?;
                return Ok(());
            }
        }

        self.perform_startup_reconciliation().await?;

        self.kernel.start_trader();
        #[cfg(feature = "plugin")]
        if let Err(e) = self.plugins.start_controllers() {
            return self.abort_after_trader_start_failure(e).await;
        }

        self.handle.set_state(NodeState::Running);

        Ok(())
    }

    /// Stop the live node.
    ///
    /// This method stops the trader, waits for the configured grace period to allow
    /// residual events to be processed, then finalizes the shutdown sequence.
    ///
    /// # Errors
    ///
    /// Returns an error if shutdown fails.
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        if !self.state().is_running() {
            anyhow::bail!("Not running");
        }

        self.handle.set_state(NodeState::ShuttingDown);

        #[cfg(feature = "plugin")]
        let controller_stop_result = self.plugins.stop_controllers();
        #[cfg(not(feature = "plugin"))]
        let controller_stop_result: anyhow::Result<()> = Ok(());

        self.kernel.stop_trader();
        let delay = self.kernel.delay_post_stop();
        log::info!("Awaiting residual events ({delay:?})...");

        dst::time::sleep(delay).await;
        let stop_result = self.finalize_stop().await;
        match (controller_stop_result, stop_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(controller_err), Ok(())) => Err(controller_err),
            (Ok(()), Err(stop_err)) => Err(stop_err),
            (Err(controller_err), Err(stop_err)) => {
                log::error!("Error stopping plug-in controllers: {controller_err}");
                Err(stop_err)
            }
        }
    }

    /// Awaits engine clients to connect with timeout.
    ///
    /// Returns the final connection wait status.
    async fn await_engines_connected(&self) -> EngineConnectionStatus {
        log::info!(
            "Awaiting engine connections ({:?} timeout)...",
            self.config.timeout_connection
        );

        let start = dst::time::Instant::now();
        let timeout = self.config.timeout_connection;
        let interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.handle.should_stop() {
                log::warn!("Stop signal received, aborting connection wait");
                return EngineConnectionStatus::StopRequested;
            }

            if self.kernel.is_shutdown_requested() {
                log::warn!("Shutdown signal received, aborting connection wait");
                return EngineConnectionStatus::ShutdownRequested;
            }

            if self.kernel.check_engines_connected() {
                log::info!("All engine clients connected");
                return EngineConnectionStatus::Connected;
            }
            dst::time::sleep(interval).await;
        }

        self.log_connection_status();
        EngineConnectionStatus::TimedOut
    }

    /// Awaits engine clients to disconnect with timeout.
    ///
    /// Logs an error with client status on timeout but does not fail.
    async fn await_engines_disconnected(&self) {
        log::info!(
            "Awaiting engine disconnections ({:?} timeout)...",
            self.config.timeout_disconnection
        );

        let start = dst::time::Instant::now();
        let timeout = self.config.timeout_disconnection;
        let interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.kernel.check_engines_disconnected() {
                log::info!("All engine clients disconnected");
                return;
            }
            dst::time::sleep(interval).await;
        }

        log::error!(
            "Timed out ({:?}) waiting for engines to disconnect\n\
             DataEngine.check_disconnected() == {}\n\
             ExecEngine.check_disconnected() == {}",
            timeout,
            self.kernel.data_engine().check_disconnected(),
            self.kernel.exec_engine().borrow().check_disconnected(),
        );
    }

    fn log_connection_status(&self) {
        #[derive(Tabled)]
        struct ClientStatus {
            #[tabled(rename = "Client")]
            client: String,
            #[tabled(rename = "Type")]
            client_type: &'static str,
            #[tabled(rename = "Connected")]
            connected: bool,
        }

        let data_status = self.kernel.data_client_connection_status();
        let exec_status = self.kernel.exec_client_connection_status();

        let mut rows: Vec<ClientStatus> = Vec::new();

        for (client_id, connected) in data_status {
            rows.push(ClientStatus {
                client: client_id.to_string(),
                client_type: "Data",
                connected,
            });
        }

        for (client_id, connected) in exec_status {
            rows.push(ClientStatus {
                client: client_id.to_string(),
                client_type: "Execution",
                connected,
            });
        }

        let table = Table::new(&rows).with(Style::rounded()).to_string();

        log::warn!(
            "Timed out ({:?}) waiting for engines to connect\n\n{table}\n\n\
             DataEngine.check_connected() == {}\n\
             ExecEngine.check_connected() == {}",
            self.config.timeout_connection,
            self.kernel.data_engine().check_connected(),
            self.kernel.exec_engine().borrow().check_connected(),
        );
    }

    /// Performs startup reconciliation to align internal state with venue state.
    ///
    /// This method queries each execution client for mass status (orders, fills, positions)
    /// and reconciles any discrepancies with the local cache state.
    ///
    /// # Errors
    ///
    /// Returns an error if reconciliation fails or times out.
    #[expect(clippy::await_holding_refcell_ref)] // Single-threaded runtime, intentional design
    async fn perform_startup_reconciliation(&mut self) -> anyhow::Result<()> {
        if !self.config.exec_engine.reconciliation {
            log::info!("Startup reconciliation disabled");
            return Ok(());
        }

        log_info!(
            "Starting execution state reconciliation...",
            color = LogColor::Blue
        );

        let lookback_mins = self
            .config
            .exec_engine
            .reconciliation_lookback_mins
            .map(u64::from);

        let timeout = self.config.timeout_reconciliation;
        let start = dst::time::Instant::now();
        let client_ids = self.kernel.exec_engine.borrow().client_ids();

        for client_id in client_ids {
            if start.elapsed() > timeout {
                log::warn!("Reconciliation timeout reached, stopping early");
                break;
            }

            log_info!(
                "Requesting mass status from {}...",
                client_id,
                color = LogColor::Blue
            );

            let mass_status_result = self
                .kernel
                .exec_engine
                .borrow_mut()
                .generate_mass_status(&client_id, lookback_mins)
                .await;

            match mass_status_result {
                Ok(Some(mass_status)) => {
                    log_info!(
                        "Reconciling ExecutionMassStatus for {}",
                        client_id,
                        color = LogColor::Blue
                    );

                    let exec_engine_rc = self.kernel.exec_engine.clone();

                    let result = self
                        .exec_manager
                        .reconcile_execution_mass_status(mass_status, exec_engine_rc)
                        .await;

                    if result.events.is_empty() {
                        log_info!(
                            "Reconciliation for {} succeeded",
                            client_id,
                            color = LogColor::Blue
                        );
                    } else {
                        log::info!(
                            color = LogColor::Blue as u8;
                            "Reconciliation for {} processed {} events",
                            client_id,
                            result.events.len()
                        );
                    }

                    // Register external orders with execution clients for tracking
                    if !result.external_orders.is_empty() {
                        let exec_engine = self.kernel.exec_engine.borrow();
                        for external in result.external_orders {
                            exec_engine.register_external_order(
                                external.client_order_id,
                                external.venue_order_id,
                                external.instrument_id,
                                external.strategy_id,
                                external.ts_init,
                            );
                        }
                    }
                }
                Ok(None) => {
                    log::warn!(
                        "No mass status available from {client_id} \
                         (likely adapter error when generating reports)"
                    );
                }
                Err(e) => {
                    log::warn!("Failed to get mass status from {client_id}: {e}");
                }
            }
        }

        self.kernel.portfolio.borrow_mut().initialize_orders();
        self.kernel.portfolio.borrow_mut().initialize_positions();

        let elapsed_secs = start.elapsed().as_secs_f64();
        log_info!(
            "Startup reconciliation completed in {:.2}s",
            elapsed_secs,
            color = LogColor::Blue
        );

        Ok(())
    }

    /// Run the live node with automatic shutdown handling.
    ///
    /// This method starts the node, runs indefinitely, and handles graceful shutdown
    /// on interrupt signals.
    ///
    /// # Thread Safety
    ///
    /// The event loop runs directly on the current thread (not spawned) because the
    /// msgbus uses thread-local storage. Endpoints registered by the kernel are only
    /// accessible from the same thread.
    ///
    /// # Shutdown Sequence
    ///
    /// 1. Signal received (SIGINT or handle stop).
    /// 2. Trader components stopped (triggers order cancellations, etc.).
    /// 3. Event loop continues processing residual events for the configured grace period.
    /// 4. Kernel finalized, clients disconnected, remaining events drained.
    ///
    /// # Errors
    ///
    /// Returns an error if the node fails to start or encounters a runtime error.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        if self.state().is_running() {
            anyhow::bail!("Already running");
        }

        let Some(runner) = self.runner.take() else {
            anyhow::bail!("Runner already consumed - run() called twice");
        };
        runner.bind_senders();

        let AsyncRunnerChannels {
            mut time_evt_rx,
            mut exec_evt_rx,
            mut exec_cmd_rx,
            mut data_evt_rx,
            mut data_cmd_rx,
        } = runner.take_channels();

        log::info!("Event loop starting");

        self.handle.set_state(NodeState::Starting);
        self.kernel.reset_shutdown_flag();
        self.kernel.start_async().await;

        if self.kernel.is_event_store_replay() {
            log::info!(
                "Event-store replay loaded; skipping live client connection and reconciliation",
            );
            self.handle.set_state(NodeState::Running);
            return Ok(());
        }

        if self.kernel.is_event_store_replay_configured() {
            self.abort_startup("Event-store replay did not start")
                .await?;
            return Ok(());
        }

        let mut external_msgbus_rx = match self.take_external_ingress_receiver() {
            Ok(rx) => rx,
            Err(e) => {
                let result = self
                    .abort_startup("External message bus ingress failed to start")
                    .await;
                Self::drain_channels(
                    &mut time_evt_rx,
                    &mut data_evt_rx,
                    &mut data_cmd_rx,
                    &mut exec_evt_rx,
                    &mut exec_cmd_rx,
                );
                log::info!("Event loop stopped");

                if let Err(finalize_err) = result {
                    anyhow::bail!(
                        "failed to start external message bus ingress: {e}; failed to finalize startup abort: {finalize_err}"
                    );
                }

                return Err(e);
            }
        };

        let stop_handle = self.handle.clone();
        let mut pending = PendingEvents::default();

        // Startup phase 1: Connect data clients and drain instrument events into cache.
        // This ensures the cache is populated before execution clients connect.
        // TODO: Add ctrl_c, stop_handle, and shutdown monitoring here to
        // allow aborting a hanging connect future.
        drive_with_event_buffering(
            self.kernel.connect_data_clients(),
            &mut pending,
            &mut time_evt_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        )
        .await;

        // Flush any data events still queued in the channel receivers that the
        // select loop did not capture before the connect future resolved, then
        // drain everything into cache.
        flush_pending_data(&mut pending, &mut data_evt_rx, &mut data_cmd_rx);
        debug_assert!(
            pending.data_evts.is_empty() && pending.data_cmds.is_empty(),
            "data must be drained into cache before exec clients connect",
        );

        // Startup phase 2: Connect execution clients (instruments now in cache)
        let engine_connection_status = drive_with_event_buffering(
            self.connect_exec_phase(),
            &mut pending,
            &mut time_evt_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        )
        .await?;

        // Flush channel receivers and drain all remaining pending events
        flush_all_pending(
            &mut pending,
            &mut time_evt_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );
        debug_assert!(
            pending.is_empty(),
            "all startup events must be processed before reconciliation",
        );

        if let Some(reason) = engine_connection_status
            .abort_reason()
            .or_else(|| self.startup_abort_reason())
        {
            self.abort_startup(reason).await?;
            Self::drain_channels(
                &mut time_evt_rx,
                &mut data_evt_rx,
                &mut data_cmd_rx,
                &mut exec_evt_rx,
                &mut exec_cmd_rx,
            );
            log::info!("Event loop stopped");
            return Ok(());
        }

        if engine_connection_status == EngineConnectionStatus::Connected {
            // Run reconciliation now that instruments are in cache and start trader
            self.perform_startup_reconciliation().await?;
            self.kernel.start_trader();
            #[cfg(feature = "plugin")]
            if let Err(e) = self.plugins.start_controllers() {
                let result = self.abort_after_trader_start_failure(e).await;
                Self::drain_channels(
                    &mut time_evt_rx,
                    &mut data_evt_rx,
                    &mut data_cmd_rx,
                    &mut exec_evt_rx,
                    &mut exec_cmd_rx,
                );
                log::info!("Event loop stopped");
                return result;
            }
        } else {
            log::error!("Not starting trader: engine client(s) not connected");
        }

        self.handle.set_state(NodeState::Running);

        let exec_config = &self.config.exec_engine;
        let inflight_interval_ns =
            u64::from(exec_config.inflight_check_interval_ms) * NANOSECONDS_IN_MILLISECOND;
        let open_interval_ns = exec_config
            .open_check_interval_secs
            .filter(|&s| s > 0.0)
            .map_or(0, secs_to_nanos_unchecked);
        let position_interval_ns = exec_config
            .position_check_interval_secs
            .filter(|&s| s > 0.0)
            .map_or(0, secs_to_nanos_unchecked);
        let has_clients = !self
            .kernel
            .exec_engine
            .borrow()
            .get_all_clients()
            .is_empty();
        let recon_enabled = has_clients
            && (inflight_interval_ns > 0 || open_interval_ns > 0 || position_interval_ns > 0);

        let recon_min_interval = if recon_enabled {
            let mut intervals = Vec::new();

            if exec_config.inflight_check_interval_ms > 0 {
                intervals.push(Duration::from_millis(u64::from(
                    exec_config.inflight_check_interval_ms,
                )));
            }

            if let Some(s) = exec_config.open_check_interval_secs.filter(|&s| s > 0.0) {
                intervals.push(Duration::from_secs_f64(s));
            }

            if let Some(s) = exec_config
                .position_check_interval_secs
                .filter(|&s| s > 0.0)
            {
                intervals.push(Duration::from_secs_f64(s));
            }

            intervals
                .into_iter()
                .min()
                .unwrap_or(Duration::from_secs(1))
        } else {
            Duration::from_secs(1) // Unused, timer won't fire
        };

        // `reconciliation_startup_delay_secs` is a post-reconciliation grace period:
        // startup reconciliation has already completed above, and this delay offsets
        // the first periodic tick to let the system stabilize before continuous checks
        // begin.
        let startup_delay = if self.config.exec_engine.reconciliation {
            Duration::from_secs_f64(exec_config.reconciliation_startup_delay_secs)
        } else {
            Duration::ZERO
        };

        let recon_start = dst::time::Instant::now() + startup_delay;

        let mut ts_last_inflight = self.exec_manager.generate_timestamp_ns();
        let mut ts_last_open = ts_last_inflight;
        let mut ts_last_position = ts_last_inflight;

        // Per-task `(interval, next_fire)` schedules dispatched by the
        // shared `maintenance_timer` below. See module docs for rationale.
        let far_future = Duration::from_hours(24 * 365 * 100);

        let make_schedule = |opt_dur: Option<Duration>| -> (Duration, dst::time::Instant) {
            let dur = opt_dur.unwrap_or(far_future);
            (dur, recon_start + dur)
        };

        let (recon_interval, mut recon_next) = make_schedule(if recon_enabled {
            Some(recon_min_interval)
        } else {
            None
        });

        let (purge_orders_interval, mut purge_orders_next) = make_schedule(
            exec_config
                .purge_closed_orders_interval_mins
                .filter(|&m| m > 0)
                .map(|m| Duration::from_secs(mins_to_secs(u64::from(m)))),
        );

        let (purge_positions_interval, mut purge_positions_next) = make_schedule(
            exec_config
                .purge_closed_positions_interval_mins
                .filter(|&m| m > 0)
                .map(|m| Duration::from_secs(mins_to_secs(u64::from(m)))),
        );

        let (purge_account_interval, mut purge_account_next) = make_schedule(
            exec_config
                .purge_account_events_interval_mins
                .filter(|&m| m > 0)
                .map(|m| Duration::from_secs(mins_to_secs(u64::from(m)))),
        );

        let (own_books_interval, mut own_books_next) = make_schedule(
            exec_config
                .own_books_audit_interval_secs
                .filter(|&s| s > 0.0)
                .map(Duration::from_secs_f64),
        );

        let (prune_fills_interval, mut prune_fills_next) =
            make_schedule(Some(Duration::from_mins(1)));

        let mut maintenance_timer = dst::time::interval(Duration::from_millis(100));
        maintenance_timer.set_missed_tick_behavior(dst::time::MissedTickBehavior::Skip);

        // Stop-check timer is not subject to the reconciliation startup delay,
        // so shutdown signals remain responsive from the moment the node reaches
        // `Running`. Set `MissedTickBehavior::Skip` so backlog ticks do not fire
        // a burst after the select arm was suspended by other branches.
        let mut stop_check_timer = dst::time::interval(Duration::from_millis(100));
        stop_check_timer.set_missed_tick_behavior(dst::time::MissedTickBehavior::Skip);

        // Running phase: runs until shutdown deadline expires
        let mut residual_events = 0usize;
        let mut open_order_report_task: Option<OpenOrderReportTask> = None;
        let mut position_report_task: Option<PositionReportTask> = None;
        let ctrl_c = dst::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            let shutdown_deadline = self.shutdown_deadline;
            let is_shutting_down = self.state() == NodeState::ShuttingDown;
            let is_running = self.state() == NodeState::Running;

            tokio::select! {
                biased;

                // Signal branches first so they are always checked
                result = &mut ctrl_c, if is_running => {
                    match result {
                        Ok(()) => log::info!("Received SIGINT, shutting down"),
                        Err(e) => log::error!("Failed to listen for SIGINT: {e}"),
                    }
                    self.initiate_shutdown();
                }
                _ = stop_check_timer.tick(), if is_running => {
                    if stop_handle.should_stop() {
                        log::info!("Received stop signal from handle");
                        self.initiate_shutdown();
                    } else if self.kernel.is_shutdown_requested() {
                        log::info!("Received ShutdownSystem command, shutting down");
                        self.initiate_shutdown();
                    }
                }
                () = async {
                    match shutdown_deadline {
                        Some(deadline) => dst::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                }, if self.state() == NodeState::ShuttingDown => {
                    break;
                }
                result = async {
                    match open_order_report_task.as_mut() {
                        Some(task) => task.future.as_mut().await,
                        None => std::future::pending::<OpenOrderReportResult>().await,
                    }
                }, if open_order_report_task.is_some() => {
                    open_order_report_task = None;
                    let events = self
                        .exec_manager
                        .reconcile_open_order_reports(&result.check, result.reports);
                    self.process_reconciliation_events(&events);
                }
                result = async {
                    match position_report_task.as_mut() {
                        Some(task) => task.future.as_mut().await,
                        None => std::future::pending::<PositionReportResult>().await,
                    }
                }, if position_report_task.is_some() => {
                    position_report_task = None;
                    let events = self.exec_manager.reconcile_position_reports(
                        &result.check,
                        result.reports,
                        &result.failed_venues,
                    );
                    self.process_reconciliation_events(&events);
                }

                // Maintenance dispatcher (before event processing to avoid
                // starvation). See module docs for design rationale.
                _ = maintenance_timer.tick(), if is_running => {
                    let mut now = dst::time::Instant::now();

                    if recon_enabled && now >= recon_next {
                        let recon_intervals = ReconciliationCheckIntervals {
                            inflight: inflight_interval_ns,
                            open: open_interval_ns,
                            position: position_interval_ns,
                        };
                        let mut recon_state = ReconciliationCheckState {
                            ts_last_inflight: &mut ts_last_inflight,
                            ts_last_open: &mut ts_last_open,
                            ts_last_position: &mut ts_last_position,
                            open_order_report_task: &mut open_order_report_task,
                            position_report_task: &mut position_report_task,
                        };

                        self.run_reconciliation_checks(
                            recon_intervals,
                            &mut recon_state,
                        );

                        now = dst::time::Instant::now();
                        recon_next = now + recon_interval;
                    }

                    if now >= purge_orders_next {
                        self.exec_manager.purge_closed_orders();
                        purge_orders_next = now + purge_orders_interval;
                    }

                    if now >= purge_positions_next {
                        self.exec_manager.purge_closed_positions();
                        purge_positions_next = now + purge_positions_interval;
                    }

                    if now >= purge_account_next {
                        self.exec_manager.purge_account_events();
                        purge_account_next = now + purge_account_interval;
                    }

                    if now >= own_books_next {
                        self.kernel.cache().borrow_mut().audit_own_order_books();
                        own_books_next = now + own_books_interval;
                    }

                    if now >= prune_fills_next {
                        self.exec_manager.prune_recent_fills_cache(60.0);
                        prune_fills_next = now + prune_fills_interval;
                    }
                }

                // Event processing branches. Exec commands and events are
                // ordered ahead of data events so a strategy action (cancel,
                // submit, etc.) is not delayed behind a market data backlog
                // when the biased select polls receivers each iteration.
                Some(handler) = time_evt_rx.recv() => {
                    AsyncRunner::handle_time_event(handler);

                    if is_shutting_down {
                        log::debug!("Residual time event");
                        residual_events += 1;
                    }
                }
                Some(evt) = exec_evt_rx.recv() => {
                    if is_shutting_down {
                        log::debug!("Residual exec event: {evt:?}");
                        residual_events += 1;
                    }

                    let mut close_ids: Vec<ClientOrderId> = Vec::new();

                    match &evt {
                        ExecutionEvent::Order(order_evt) => {
                            self.exec_manager.record_local_activity(order_evt.client_order_id());
                            match order_evt {
                                OrderEventAny::Filled(fill) => {
                                    self.exec_manager.record_position_activity(
                                        fill.instrument_id,
                                        fill.account_id,
                                        fill.ts_event,
                                    );
                                    self.exec_manager.mark_fill_processed(fill.trade_id);
                                }
                                OrderEventAny::Accepted(_)
                                | OrderEventAny::Rejected(_)
                                | OrderEventAny::Canceled(_)
                                | OrderEventAny::Expired(_)
                                | OrderEventAny::Denied(_)
                                | OrderEventAny::Updated(_)
                                | OrderEventAny::ModifyRejected(_)
                                | OrderEventAny::CancelRejected(_) => {
                                    self.exec_manager.clear_recon_tracking(
                                        &order_evt.client_order_id(), true,
                                    );
                                }
                                _ => {}
                            }
                            close_ids.push(order_evt.client_order_id());
                        }
                        ExecutionEvent::OrderSubmittedBatch(batch) => {
                            for submitted in &batch.events {
                                self.exec_manager.record_local_activity(submitted.client_order_id);
                            }
                        }
                        ExecutionEvent::OrderAcceptedBatch(batch) => {
                            for accepted in &batch.events {
                                self.exec_manager.record_local_activity(accepted.client_order_id);
                                self.exec_manager.clear_recon_tracking(
                                    &accepted.client_order_id, true,
                                );
                            }
                        }
                        ExecutionEvent::OrderCanceledBatch(batch) => {
                            for canceled in &batch.events {
                                self.exec_manager.record_local_activity(canceled.client_order_id);
                                self.exec_manager.clear_recon_tracking(
                                    &canceled.client_order_id, true,
                                );
                                close_ids.push(canceled.client_order_id);
                            }
                        }
                        ExecutionEvent::Report(report) => {
                            if let ExecutionReport::Fill(fill_report) = report
                                && self.exec_manager.is_fill_recently_processed(&fill_report.trade_id) {
                                    log::debug!(
                                        "Skipping recently processed fill report: {}",
                                        fill_report.trade_id,
                                    );
                                    continue;
                            }
                            self.exec_manager.observe_execution_report(report);
                        }
                        ExecutionEvent::Account(_) => {}
                    }

                    AsyncRunner::handle_exec_event(evt);

                    // Post-dispatch: clear tracking when order closes
                    for coid in &close_ids {
                        let is_closed = self.kernel.cache().borrow()
                            .order(coid).is_some_and(|o| o.is_closed());
                        if is_closed {
                            self.exec_manager.clear_recon_tracking(coid, true);
                        }
                    }
                }
                Some(cmd) = exec_cmd_rx.recv() => {
                    if is_shutting_down {
                        log::debug!("Residual exec command: {cmd:?}");
                        residual_events += 1;
                    }

                    match &cmd {
                        TradingCommand::SubmitOrder(submit) => {
                            self.exec_manager.register_inflight(submit.client_order_id);
                        }
                        TradingCommand::SubmitOrderList(submit) => {
                            for order_init in &submit.order_inits {
                                self.exec_manager.register_inflight(order_init.client_order_id);
                            }
                        }
                        TradingCommand::ModifyOrder(modify) => {
                            self.exec_manager.register_inflight(modify.client_order_id);
                        }
                        TradingCommand::ModifyOrders(modify) => {
                            for child in &modify.modifies {
                                self.exec_manager.register_inflight(child.client_order_id);
                            }
                        }
                        TradingCommand::CancelOrder(cancel) => {
                            self.exec_manager.register_inflight(cancel.client_order_id);
                        }
                        _ => {}
                    }
                    AsyncRunner::handle_exec_command(cmd);
                }
                message = recv_external_msgbus_message(&mut external_msgbus_rx) => {
                    match message {
                        Some(message) => {
                            if is_shutting_down {
                                log::debug!("Residual external message bus message: {message}");
                                residual_events += 1;
                            }
                            Self::republish_external_msgbus_message(&message);
                        }
                        None => {
                            log::info!("External message bus ingress closed");
                            external_msgbus_rx = None;
                            self.close_external_ingress();
                        }
                    }
                }
                Some(evt) = data_evt_rx.recv() => {
                    if is_shutting_down {
                        log::debug!("Residual data event: {evt:?}");
                        residual_events += 1;
                    }
                    AsyncRunner::handle_data_event(evt);
                }
                Some(cmd) = data_cmd_rx.recv() => {
                    if is_shutting_down {
                        log::debug!("Residual data command: {cmd:?}");
                        residual_events += 1;
                    }
                    AsyncRunner::handle_data_command(cmd);
                }
            }
        }

        if residual_events > 0 {
            log::debug!("Processed {residual_events} residual events during shutdown");
        }

        drop(open_order_report_task.take());
        drop(position_report_task.take());
        drop(external_msgbus_rx.take());
        let _ = self.kernel.cache().borrow().check_residuals();

        self.finalize_stop().await?;

        // Handle events that arrived during finalize_stop
        Self::drain_channels(
            &mut time_evt_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );

        log::info!("Event loop stopped");

        Ok(())
    }

    fn take_external_ingress_receiver(
        &mut self,
    ) -> anyhow::Result<Option<tokio::sync::mpsc::Receiver<BusMessage>>> {
        let Some(external_ingress) = self.external_msgbus.as_mut() else {
            return Ok(None);
        };

        let receiver = external_ingress.take_receiver()?;
        log::info!("External message bus ingress started");
        Ok(Some(receiver))
    }

    fn republish_external_msgbus_message(message: &BusMessage) {
        if let Err(e) = msgbus::republish_external_message(message) {
            log::error!("Failed to republish external message bus message: {e}");
        }
    }

    fn close_external_ingress(&mut self) {
        if let Some(external_ingress) = self.external_msgbus.as_mut()
            && !external_ingress.is_closed()
        {
            external_ingress.close();
        }
    }

    fn process_reconciliation_events(&mut self, events: &[OrderEventAny]) {
        if events.is_empty() {
            return;
        }

        log::info!(
            "Processing {} reconciliation event{}",
            events.len(),
            if events.len() == 1 { "" } else { "s" }
        );

        for event in events {
            self.exec_manager
                .record_local_activity(event.client_order_id());
            if let OrderEventAny::Filled(fill) = event {
                self.exec_manager.record_position_activity(
                    fill.instrument_id,
                    fill.account_id,
                    fill.ts_event,
                );
                self.exec_manager.mark_fill_processed(fill.trade_id);
            }
            self.kernel.exec_engine.borrow_mut().process(event);
        }
    }

    /// Connects execution clients and checks all engines are connected.
    ///
    /// Returns the final connection wait status.
    /// Must be called after data clients are connected and instrument events drained.
    async fn connect_exec_phase(&mut self) -> anyhow::Result<EngineConnectionStatus> {
        self.kernel.connect_exec_clients().await;
        Ok(self.await_engines_connected().await)
    }

    fn startup_abort_reason(&self) -> Option<&'static str> {
        if self.handle.should_stop() {
            Some("Stop signal received during startup")
        } else if self.kernel.is_shutdown_requested() {
            Some("Shutdown signal received during startup")
        } else {
            None
        }
    }

    async fn abort_startup(&mut self, reason: &str) -> anyhow::Result<()> {
        log::info!("{reason}, aborting startup");
        self.handle.set_state(NodeState::ShuttingDown);
        self.finalize_stop().await
    }

    #[cfg(feature = "plugin")]
    async fn abort_after_trader_start_failure(
        &mut self,
        start_err: anyhow::Error,
    ) -> anyhow::Result<()> {
        log::info!("Plug-in controller startup failed, aborting startup");
        self.handle.set_state(NodeState::ShuttingDown);
        self.kernel.stop_trader();

        if let Err(finalize_err) = self.finalize_stop().await {
            anyhow::bail!(
                "failed to start plug-in controller: {start_err}; failed to finalize startup abort: {finalize_err}"
            );
        }
        Err(start_err)
    }

    fn initiate_shutdown(&mut self) {
        #[cfg(feature = "plugin")]
        if let Err(e) = self.plugins.stop_controllers() {
            log::error!("Error stopping plug-in controllers: {e}");
        }
        self.kernel.stop_trader();
        let delay = self.kernel.delay_post_stop();
        log::info!("Awaiting residual events ({delay:?})...");

        self.shutdown_deadline = Some(dst::time::Instant::now() + delay);
        self.handle.set_state(NodeState::ShuttingDown);
    }

    async fn finalize_stop(&mut self) -> anyhow::Result<()> {
        self.close_external_ingress();

        let disconnect_result = self.kernel.disconnect_clients().await;
        if let Err(ref e) = disconnect_result {
            log::error!("Error disconnecting clients: {e}");
        }

        self.await_engines_disconnected().await;
        self.kernel.finalize_stop().await;

        self.handle.set_state(NodeState::Stopped);

        disconnect_result
    }

    fn drain_channels(
        time_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<TimeEventHandler>,
        data_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        data_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
        exec_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        exec_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<TradingCommand>,
    ) {
        let mut drained = 0;

        while let Ok(handler) = time_evt_rx.try_recv() {
            AsyncRunner::handle_time_event(handler);
            drained += 1;
        }

        while let Ok(cmd) = data_cmd_rx.try_recv() {
            AsyncRunner::handle_data_command(cmd);
            drained += 1;
        }

        while let Ok(evt) = data_evt_rx.try_recv() {
            AsyncRunner::handle_data_event(evt);
            drained += 1;
        }

        while let Ok(cmd) = exec_cmd_rx.try_recv() {
            AsyncRunner::handle_exec_command(cmd);
            drained += 1;
        }

        while let Ok(evt) = exec_evt_rx.try_recv() {
            AsyncRunner::handle_exec_event(evt);
            drained += 1;
        }

        if drained > 0 {
            log::info!("Drained {drained} remaining events during shutdown");
        }
    }

    /// Gets the node's environment.
    #[must_use]
    pub fn environment(&self) -> Environment {
        self.kernel.environment()
    }

    /// Gets a reference to the underlying kernel.
    #[must_use]
    pub const fn kernel(&self) -> &NautilusKernel {
        &self.kernel
    }

    /// Gets an exclusive reference to the underlying kernel.
    #[must_use]
    pub const fn kernel_mut(&mut self) -> &mut NautilusKernel {
        &mut self.kernel
    }

    /// Gets the node's trader ID.
    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        self.kernel.trader_id()
    }

    /// Gets the node's instance ID.
    #[must_use]
    pub const fn instance_id(&self) -> UUID4 {
        self.kernel.instance_id()
    }

    /// Returns the current node state.
    #[must_use]
    pub fn state(&self) -> NodeState {
        self.handle.state()
    }

    /// Checks if the live node is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Sets the cache database adapter for persistence.
    ///
    /// This allows setting a database adapter (e.g., PostgreSQL, Redis) after the node
    /// is built but before it starts running. The database adapter is used to persist
    /// cache data for recovery and state management.
    ///
    /// # Errors
    ///
    /// Returns an error if the node is already running.
    pub fn set_cache_database(
        &mut self,
        database: Box<dyn CacheDatabaseAdapter>,
    ) -> anyhow::Result<()> {
        if self.state() != NodeState::Idle {
            anyhow::bail!(
                "Cannot set cache database while node is running, set it before calling start()"
            );
        }

        self.kernel.cache().borrow_mut().set_database(database);
        Ok(())
    }

    /// Returns the execution manager.
    #[must_use]
    pub fn exec_manager(&self) -> &ExecutionManager {
        &self.exec_manager
    }

    /// Returns a mutable reference to the execution manager.
    #[must_use]
    pub fn exec_manager_mut(&mut self) -> &mut ExecutionManager {
        &mut self.exec_manager
    }

    /// Adds an actor to the trader.
    ///
    /// This method provides a high-level interface for adding actors to the underlying
    /// trader without requiring direct access to the kernel. Actors should be added
    /// after the node is built but before starting the node.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The trader is not in a valid state for adding components.
    /// - An actor with the same ID is already registered.
    /// - The node is currently running.
    pub fn add_actor<T>(&mut self, actor: T) -> anyhow::Result<()>
    where
        T: DataActor + DataActorNative + Component + Actor + 'static,
    {
        if self.state() != NodeState::Idle {
            anyhow::bail!(
                "Cannot add actor while node is running, add actors before calling start()"
            );
        }

        self.kernel.trader.borrow_mut().add_actor(actor)
    }

    /// Adds an actor to the live node using a factory function.
    ///
    /// The factory function is called at registration time to create the actor,
    /// avoiding cloning issues with non-cloneable actor types.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The node is currently running.
    /// - The factory function fails to create the actor.
    /// - The underlying trader registration fails.
    pub fn add_actor_from_factory<F, T>(&mut self, factory: F) -> anyhow::Result<()>
    where
        F: FnOnce() -> anyhow::Result<T>,
        T: DataActor + DataActorNative + Component + Actor + 'static,
    {
        if self.state() != NodeState::Idle {
            anyhow::bail!(
                "Cannot add actor while node is running, add actors before calling start()"
            );
        }

        self.kernel
            .trader
            .borrow_mut()
            .add_actor_from_factory(factory)
    }

    /// Adds a strategy to the trader.
    ///
    /// Strategies are registered in both the component registry (for lifecycle management)
    /// and the actor registry (for data callbacks via msgbus).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The node is currently running.
    /// - A strategy with the same ID is already registered.
    pub fn add_strategy<T>(&mut self, mut strategy: T) -> anyhow::Result<()>
    where
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        if self.state() != NodeState::Idle {
            anyhow::bail!(
                "Cannot add strategy while node is running, add strategies before calling start()"
            );
        }

        // Register external order claims before adding strategy (which moves it)
        let strategy_id = self
            .kernel
            .trader
            .borrow()
            .prepare_strategy_for_registration(&mut strategy)?;
        if let Some(claims) = strategy.external_order_claims() {
            for instrument_id in &claims {
                self.exec_manager
                    .claim_external_orders(*instrument_id, strategy_id)?;
            }
            log_info!(
                "Registered external order claims for {}: {:?}",
                strategy_id,
                claims,
                color = LogColor::Blue
            );
        }

        self.kernel.trader.borrow_mut().add_strategy(strategy)
    }

    /// Adds an execution algorithm to the trader.
    ///
    /// Execution algorithms are registered in both the component registry (for lifecycle
    /// management) and the actor registry (for data callbacks via msgbus).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The node is currently running.
    /// - An execution algorithm with the same ID is already registered.
    pub fn add_exec_algorithm<T>(&mut self, exec_algorithm: T) -> anyhow::Result<()>
    where
        T: ExecutionAlgorithm + ExecutionAlgorithmNative + Component + Debug + 'static,
    {
        if self.state() != NodeState::Idle {
            anyhow::bail!(
                "Cannot add exec algorithm while node is running, add exec algorithms before calling start()"
            );
        }

        self.kernel
            .trader
            .borrow_mut()
            .add_exec_algorithm(exec_algorithm)
    }

    // Runs reconciliation sub-checks, each gated by its own interval.
    // Continuous checks only schedule venue work; they do not await venue I/O
    // in the event loop.
    fn run_reconciliation_checks(
        &mut self,
        intervals: ReconciliationCheckIntervals,
        state: &mut ReconciliationCheckState<'_>,
    ) {
        let ts_now = self.exec_manager.generate_timestamp_ns();

        if reconciliation_check_due(ts_now, *state.ts_last_inflight, intervals.inflight) {
            if self.state() == NodeState::ShuttingDown {
                return;
            }
            let result = self.exec_manager.check_inflight_orders();
            self.process_reconciliation_events(&result.events);
            for cmd in result.queries {
                AsyncRunner::handle_exec_command(cmd);
            }
            *state.ts_last_inflight = ts_now;
        }

        let open_due = reconciliation_check_due(ts_now, *state.ts_last_open, intervals.open);
        let position_due =
            reconciliation_check_due(ts_now, *state.ts_last_position, intervals.position);

        if (open_due || position_due) && self.state() == NodeState::ShuttingDown {
            return;
        }

        if state.open_order_report_task.is_some() {
            if open_due {
                log::debug!("Open-order reconciliation already in progress");
                *state.ts_last_open = ts_now;
            }

            if position_due {
                log::debug!(
                    "Position reconciliation delayed: open-order reconciliation in progress"
                );
            }

            return;
        }

        if state.position_report_task.is_some() {
            if position_due {
                log::debug!("Position reconciliation already in progress");
                *state.ts_last_position = ts_now;
            }

            if open_due {
                log::debug!(
                    "Open-order reconciliation delayed: position reconciliation in progress"
                );
            }

            return;
        }

        if position_due && (!open_due || *state.ts_last_position < *state.ts_last_open) {
            *state.position_report_task = self.start_position_report_check();
            *state.ts_last_position = ts_now;
        } else if open_due {
            *state.open_order_report_task = self.start_open_order_report_check();
            *state.ts_last_open = ts_now;
        }
    }

    fn start_open_order_report_check(&self) -> Option<OpenOrderReportTask> {
        if self.exec_clients.is_empty() {
            log::debug!("No execution clients to check orders consistency");
            return None;
        }

        let check = self
            .exec_manager
            .prepare_open_order_report_check(UUID4::new());
        let command = check.command.clone();
        let clients = self.exec_clients.clone();

        Some(OpenOrderReportTask {
            future: Box::pin(async move {
                let reports = request_open_order_reports(clients, command).await;
                OpenOrderReportResult { check, reports }
            }),
        })
    }

    fn start_position_report_check(&self) -> Option<PositionReportTask> {
        if self.exec_clients.is_empty() {
            log::debug!("No execution clients to check positions consistency");
            return None;
        }

        let check = self
            .exec_manager
            .prepare_position_report_check(UUID4::new());
        let command = check.command.clone();
        let clients = self.exec_clients.clone();

        Some(PositionReportTask {
            future: Box::pin(async move {
                let result = request_position_reports(clients, command).await;
                PositionReportResult {
                    check,
                    reports: result.reports,
                    failed_venues: result.failed_venues,
                }
            }),
        })
    }
}

async fn recv_external_msgbus_message(
    rx: &mut Option<tokio::sync::mpsc::Receiver<BusMessage>>,
) -> Option<BusMessage> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending::<Option<BusMessage>>().await,
    }
}

async fn request_open_order_reports(
    clients: Vec<LiveExecutionClient>,
    command: GenerateOrderStatusReports,
) -> Vec<OrderStatusReport> {
    let mut all_reports = Vec::new();

    for client in clients {
        match client.generate_order_status_reports(&command).await {
            Ok(reports) => {
                all_reports.extend(reports);
            }
            Err(e) => {
                log::warn!(
                    "Failed to generate order status reports from {}: {e}",
                    client.client_id()
                );
            }
        }
    }

    all_reports
}

async fn request_position_reports(
    clients: Vec<LiveExecutionClient>,
    command: GeneratePositionStatusReports,
) -> PositionReportQueryResult {
    let mut all_reports = Vec::new();
    let mut failed_venues = IndexSet::new();

    for client in clients {
        let venue = client.venue();
        match client.generate_position_status_reports(&command).await {
            Ok(reports) => {
                all_reports.extend(reports);
            }
            Err(e) => {
                failed_venues.insert(venue);
                log::warn!(
                    "Failed to generate position status reports from {}: {e}",
                    client.client_id()
                );
            }
        }
    }

    PositionReportQueryResult {
        reports: all_reports,
        failed_venues,
    }
}

fn reconciliation_check_due(ts_now: UnixNanos, ts_last: UnixNanos, interval_ns: u64) -> bool {
    interval_ns > 0
        && ts_now
            .duration_since(&ts_last)
            .is_some_and(|elapsed_ns| elapsed_ns >= interval_ns)
}

#[derive(Clone, Copy)]
struct ReconciliationCheckIntervals {
    inflight: u64,
    open: u64,
    position: u64,
}

struct ReconciliationCheckState<'a> {
    ts_last_inflight: &'a mut UnixNanos,
    ts_last_open: &'a mut UnixNanos,
    ts_last_position: &'a mut UnixNanos,
    open_order_report_task: &'a mut Option<OpenOrderReportTask>,
    position_report_task: &'a mut Option<PositionReportTask>,
}

type OpenOrderReportFuture = Pin<Box<dyn Future<Output = OpenOrderReportResult>>>;

struct OpenOrderReportTask {
    future: OpenOrderReportFuture,
}

struct OpenOrderReportResult {
    check: OpenOrderReportCheck,
    reports: Vec<OrderStatusReport>,
}

type PositionReportFuture = Pin<Box<dyn Future<Output = PositionReportResult>>>;

struct PositionReportTask {
    future: PositionReportFuture,
}

struct PositionReportResult {
    check: PositionReportCheck,
    reports: Vec<PositionStatusReport>,
    failed_venues: IndexSet<Venue>,
}

struct PositionReportQueryResult {
    reports: Vec<PositionStatusReport>,
    failed_venues: IndexSet<Venue>,
}

/// Flushes data events and commands from both `pending` and the channel receivers
/// into the cache, looping until no progress is made.
///
/// This closes the gap where `drive_with_event_buffering` exits as soon as its
/// driven future resolves (biased select), leaving items in the channel receivers
/// that were not captured into `pending`.
fn flush_pending_data(
    pending: &mut PendingEvents,
    data_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    data_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
) {
    loop {
        let mut progressed = pending.drain_data();

        while let Ok(evt) = data_evt_rx.try_recv() {
            AsyncRunner::handle_data_event(evt);
            progressed = true;
        }

        while let Ok(cmd) = data_cmd_rx.try_recv() {
            AsyncRunner::handle_data_command(cmd);
            progressed = true;
        }

        if !progressed {
            break;
        }
    }
}

/// Flushes all channel receivers into `pending`, then drains everything.
///
/// Unlike [`flush_pending_data`] this is a single pass, not a drain-until-quiet
/// loop. Sufficient for phase 2 where the goal is to capture items the biased
/// select did not poll before the connect future resolved.
fn flush_all_pending(
    pending: &mut PendingEvents,
    time_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<TimeEventHandler>,
    data_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    data_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
    exec_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    exec_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<TradingCommand>,
) {
    // Flush channel receivers into pending
    while let Ok(handler) = time_evt_rx.try_recv() {
        AsyncRunner::handle_time_event(handler);
    }

    while let Ok(evt) = data_evt_rx.try_recv() {
        pending.data_evts.push(evt);
    }

    while let Ok(cmd) = data_cmd_rx.try_recv() {
        pending.data_cmds.push(cmd);
    }

    while let Ok(evt) = exec_evt_rx.try_recv() {
        match evt {
            ExecutionEvent::Account(_) => {
                AsyncRunner::handle_exec_event(evt);
            }
            ExecutionEvent::Report(report) => {
                pending.exec_reports.push(report);
            }
            ExecutionEvent::Order(order_evt) => {
                pending.order_evts.push(order_evt);
            }
            ExecutionEvent::OrderSubmittedBatch(batch) => {
                for submitted in batch {
                    pending.order_evts.push(OrderEventAny::Submitted(submitted));
                }
            }
            ExecutionEvent::OrderAcceptedBatch(batch) => {
                for accepted in batch {
                    pending.order_evts.push(OrderEventAny::Accepted(accepted));
                }
            }
            ExecutionEvent::OrderCanceledBatch(batch) => {
                for canceled in batch {
                    pending.order_evts.push(OrderEventAny::Canceled(canceled));
                }
            }
        }
    }

    while let Ok(cmd) = exec_cmd_rx.try_recv() {
        pending.exec_cmds.push(cmd);
    }

    pending.drain();
}

/// Drives a future to completion while buffering channel events.
///
/// Time events are handled immediately. Account events are forwarded directly.
/// All other events are buffered in `pending` for later processing.
async fn drive_with_event_buffering<F: std::future::Future>(
    future: F,
    pending: &mut PendingEvents,
    time_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<TimeEventHandler>,
    data_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    data_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataCommand>,
    exec_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    exec_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<TradingCommand>,
) -> F::Output {
    tokio::pin!(future);

    loop {
        tokio::select! {
            biased;

            result = &mut future => {
                break result;
            }
            Some(handler) = time_evt_rx.recv() => {
                AsyncRunner::handle_time_event(handler);
            }
            Some(evt) = exec_evt_rx.recv() => {
                // Account events are safe to process immediately. Report and
                // Order events need ExecEngine borrow_mut which may conflict
                // with the borrow held by the driven future.
                match evt {
                    ExecutionEvent::Account(_) => {
                        AsyncRunner::handle_exec_event(evt);
                    }
                    ExecutionEvent::Report(report) => {
                        pending.exec_reports.push(report);
                    }
                    ExecutionEvent::Order(order_evt) => {
                        pending.order_evts.push(order_evt);
                    }
                    ExecutionEvent::OrderSubmittedBatch(batch) => {
                        for submitted in batch {
                            pending.order_evts.push(OrderEventAny::Submitted(submitted));
                        }
                    }
                    ExecutionEvent::OrderAcceptedBatch(batch) => {
                        for accepted in batch {
                            pending.order_evts.push(OrderEventAny::Accepted(accepted));
                        }
                    }
                    ExecutionEvent::OrderCanceledBatch(batch) => {
                        for canceled in batch {
                            pending.order_evts.push(OrderEventAny::Canceled(canceled));
                        }
                    }
                }
            }
            Some(cmd) = exec_cmd_rx.recv() => {
                pending.exec_cmds.push(cmd);
            }
            Some(evt) = data_evt_rx.recv() => {
                pending.data_evts.push(evt);
            }
            Some(cmd) = data_cmd_rx.recv() => {
                pending.data_cmds.push(cmd);
            }
        }
    }
}

#[derive(Default)]
struct PendingEvents {
    data_cmds: Vec<DataCommand>,
    data_evts: Vec<DataEvent>,
    exec_cmds: Vec<TradingCommand>,
    exec_reports: Vec<ExecutionReport>,
    order_evts: Vec<OrderEventAny>,
}

impl PendingEvents {
    fn is_empty(&self) -> bool {
        self.data_evts.is_empty()
            && self.data_cmds.is_empty()
            && self.exec_cmds.is_empty()
            && self.exec_reports.is_empty()
            && self.order_evts.is_empty()
    }

    /// Drains only data events and commands into the cache.
    ///
    /// Returns `true` if any events or commands were drained.
    fn drain_data(&mut self) -> bool {
        let total = self.data_evts.len() + self.data_cmds.len();

        if total > 0 {
            log::debug!(
                "Draining {total} data events/commands into cache \
                 (data_evts={}, data_cmds={})",
                self.data_evts.len(),
                self.data_cmds.len(),
            );
        }

        for evt in self.data_evts.drain(..) {
            AsyncRunner::handle_data_event(evt);
        }

        for cmd in self.data_cmds.drain(..) {
            AsyncRunner::handle_data_command(cmd);
        }

        total > 0
    }

    /// Drains all remaining pending events.
    fn drain(&mut self) {
        let total = self.data_evts.len()
            + self.data_cmds.len()
            + self.exec_cmds.len()
            + self.exec_reports.len()
            + self.order_evts.len();

        if total > 0 {
            log::debug!(
                "Processing {total} events/commands queued during startup \
                 (data_evts={}, data_cmds={}, exec_cmds={}, exec_reports={}, order_evts={})",
                self.data_evts.len(),
                self.data_cmds.len(),
                self.exec_cmds.len(),
                self.exec_reports.len(),
                self.order_evts.len()
            );
        }

        for evt in self.data_evts.drain(..) {
            AsyncRunner::handle_data_event(evt);
        }

        for cmd in self.data_cmds.drain(..) {
            AsyncRunner::handle_data_command(cmd);
        }

        for report in self.exec_reports.drain(..) {
            AsyncRunner::handle_exec_event(ExecutionEvent::Report(report));
        }

        for cmd in self.exec_cmds.drain(..) {
            AsyncRunner::handle_exec_command(cmd);
        }

        for evt in self.order_evts.drain(..) {
            AsyncRunner::handle_exec_event(ExecutionEvent::Order(evt));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        fmt::Debug,
        rc::Rc,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use bytes::Bytes;
    #[cfg(feature = "python")]
    use nautilus_common::runner::{
        SyncDataCommandSender, SyncTradingCommandSender, replace_data_cmd_sender,
        replace_exec_cmd_sender,
    };
    use nautilus_common::{
        cache::Cache,
        clock::Clock,
        enums::SerializationEncoding,
        msgbus::{
            self, BusMessage, BusPayloadType, MessageBusBacking, MessageBusBackingFactory,
            MessageBusConfig, MessageBusExternalEgress, MessageBusExternalIngress,
            MessagingSwitchboard, TypedHandler, TypedIntoHandler,
        },
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::engine::{ExecutionEngine, SnapshotAnchorer};
    use nautilus_model::{
        data::QuoteTick,
        enums::OrderType,
        identifiers::{AccountId, ClientId, InstrumentId, TraderId, VenueOrderId},
        instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
        orders::{OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{Price, Quantity},
    };
    use nautilus_system::{KernelEventStore, RegisteredComponents, event_store::EventStoreConfig};
    use rstest::*;

    use super::*;

    #[derive(Debug)]
    struct ReplayKernelEventStore {
        fail_restore: bool,
    }

    impl KernelEventStore for ReplayKernelEventStore {
        fn restore_parent_cache(
            &mut self,
            _instance_id: UUID4,
            _cache: &mut Cache,
        ) -> anyhow::Result<()> {
            if self.fail_restore {
                anyhow::bail!("replay restore failed");
            }

            Ok(())
        }

        fn open(
            &mut self,
            _instance_id: UUID4,
            _components: &RegisteredComponents,
            _environment: Environment,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_anchorer(&self) -> Option<SnapshotAnchorer> {
            None
        }

        fn seal(&mut self, _ts_init: UnixNanos) {}

        fn run_id(&self) -> Option<&str> {
            Some("replay-child")
        }

        fn parent_run_id(&self) -> Option<&str> {
            Some("seed-run")
        }

        fn is_event_store_replay_configured(&self) -> bool {
            true
        }

        fn is_halted(&self) -> bool {
            false
        }
    }

    fn live_node_with_replay_store(fail_restore: bool) -> LiveNode {
        // load_state must be true: the kernel rejects event-store replay otherwise,
        // and LiveNodeConfig defaults it to false.
        let builder = LiveNodeBuilder::new(TraderId::default(), Environment::Live)
            .unwrap()
            .with_exec_engine_config(crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            })
            .with_load_state(true)
            .with_name("TestKernel")
            .with_event_store(move |_instance_id: UUID4, _clock: Rc<RefCell<dyn Clock>>| {
                Ok(Box::new(ReplayKernelEventStore { fail_restore }) as Box<dyn KernelEventStore>)
            });

        builder.build().unwrap()
    }

    #[rstest]
    fn test_run_reconciliation_checks_does_not_publish_open_order_queries() {
        let config = LiveNodeConfig {
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: true,
                open_check_interval_secs: Some(1.0),
                position_check_interval_secs: Some(1.0),
                max_single_order_queries_per_cycle: 5,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut node =
            LiveNode::build("ReconciliationFallbackNode".to_string(), Some(config)).unwrap();
        let client_id = ClientId::from("TEST-QUERY");
        let account_id = AccountId::from("TEST-QUERY-001");

        let trading_commands = Rc::new(RefCell::new(Vec::new()));
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from({
                let trading_commands = trading_commands.clone();
                move |command: TradingCommand| {
                    trading_commands.borrow_mut().push(command);
                }
            }),
        );

        let venue_order_id = VenueOrderId::from("V-NODE-QUERY-001");
        let instrument = crypto_perpetual_ethusdt();
        let instrument_id = instrument.id();
        let client_order_id = ClientOrderId::from("O-NODE-QUERY-001");

        node.kernel
            .cache
            .borrow_mut()
            .add_instrument(InstrumentAny::CryptoPerpetual(instrument))
            .unwrap();
        insert_accepted_limit_order_in_node(
            &node,
            account_id,
            client_id,
            instrument_id,
            client_order_id,
            venue_order_id,
        );

        let mut ts_last_inflight = UnixNanos::default();
        let mut ts_last_open = UnixNanos::default();
        let mut ts_last_position = UnixNanos::default();
        let mut open_order_report_task = None;
        let mut position_report_task = None;

        node.run_reconciliation_checks(
            ReconciliationCheckIntervals {
                inflight: 0,
                open: 1,
                position: 0,
            },
            &mut ReconciliationCheckState {
                ts_last_inflight: &mut ts_last_inflight,
                ts_last_open: &mut ts_last_open,
                ts_last_position: &mut ts_last_position,
                open_order_report_task: &mut open_order_report_task,
                position_report_task: &mut position_report_task,
            },
        );

        let commands = trading_commands.borrow();

        assert!(commands.is_empty());
        assert!(open_order_report_task.is_none());
        assert!(position_report_task.is_none());

        ExecutionEngine::register_msgbus_handlers(&node.kernel.exec_engine);
    }

    fn insert_accepted_limit_order_in_node(
        node: &LiveNode,
        account_id: AccountId,
        client_id: ClientId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .client_order_id(client_order_id)
            .instrument_id(instrument_id)
            .quantity(Quantity::from("10.0"))
            .price(Price::from("100.0"))
            .build();
        let submitted = TestOrderEventStubs::submitted(&order, account_id);
        node.kernel
            .cache
            .borrow_mut()
            .add_order(order, None, Some(client_id), false)
            .unwrap();
        let order = node
            .kernel
            .cache
            .borrow_mut()
            .update_order(&submitted)
            .unwrap();
        let accepted = TestOrderEventStubs::accepted(&order, account_id, venue_order_id);
        node.kernel
            .cache
            .borrow_mut()
            .update_order(&accepted)
            .unwrap();
    }

    #[rstest]
    #[case(0, NodeState::Idle)]
    #[case(1, NodeState::Starting)]
    #[case(2, NodeState::Running)]
    #[case(3, NodeState::ShuttingDown)]
    #[case(4, NodeState::Stopped)]
    fn test_node_state_from_u8_valid(#[case] value: u8, #[case] expected: NodeState) {
        assert_eq!(NodeState::from_u8(value), expected);
    }

    #[rstest]
    #[case(5)]
    #[case(255)]
    #[should_panic(expected = "Invalid NodeState value")]
    fn test_node_state_from_u8_invalid_panics(#[case] value: u8) {
        let _ = NodeState::from_u8(value);
    }

    #[rstest]
    fn test_node_state_roundtrip() {
        for state in [
            NodeState::Idle,
            NodeState::Starting,
            NodeState::Running,
            NodeState::ShuttingDown,
            NodeState::Stopped,
        ] {
            assert_eq!(NodeState::from_u8(state.as_u8()), state);
        }
    }

    #[rstest]
    fn test_node_state_is_running_only_for_running() {
        assert!(!NodeState::Idle.is_running());
        assert!(!NodeState::Starting.is_running());
        assert!(NodeState::Running.is_running());
        assert!(!NodeState::ShuttingDown.is_running());
        assert!(!NodeState::Stopped.is_running());
    }

    #[rstest]
    #[tokio::test]
    async fn test_await_engines_connected_returns_stop_requested() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();
        let handle = node.handle();

        handle.stop();

        let status = node.await_engines_connected().await;

        assert_eq!(status, EngineConnectionStatus::StopRequested);
        assert!(handle.should_stop());
    }

    #[rstest]
    #[tokio::test]
    async fn test_await_engines_connected_returns_shutdown_requested() {
        let node = LiveNode::build("TestNode".to_string(), None).unwrap();

        node.kernel().shutdown_flag().set(true);

        let status = node.await_engines_connected().await;

        assert_eq!(status, EngineConnectionStatus::ShutdownRequested);
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_stop_request_aborts_startup_without_running() {
        let config = LiveNodeConfig {
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            timeout_disconnection: Duration::from_millis(50),
            ..Default::default()
        };
        let mut node = LiveNode::build("TestNode".to_string(), Some(config)).unwrap();
        let handle = node.handle();

        handle.stop();
        node.start().await.unwrap();

        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(handle.should_stop());
        assert!(!handle.is_running());
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_event_store_replay_skips_live_connections() {
        let mut node = live_node_with_replay_store(false);
        let handle = node.handle();

        node.start().await.unwrap();

        assert_eq!(handle.state(), NodeState::Running);
        assert!(handle.is_running());
        assert!(node.kernel.is_event_store_replay());
        assert!(node.runner.is_some());
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_event_store_replay_config_failure_aborts_startup() {
        let mut node = live_node_with_replay_store(true);
        let handle = node.handle();

        node.start().await.unwrap();

        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(node.kernel.is_event_store_replay_configured());
        assert!(!node.kernel.is_event_store_replay());
        assert!(node.runner.is_some());
    }

    #[rstest]
    #[tokio::test]
    async fn test_run_event_store_replay_consumes_runner_and_stops_before_connections() {
        let mut node = live_node_with_replay_store(false);
        let handle = node.handle();

        node.run().await.unwrap();

        assert_eq!(handle.state(), NodeState::Running);
        assert!(handle.is_running());
        assert!(node.kernel.is_event_store_replay());
        assert!(node.runner.is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn test_run_event_store_replay_config_failure_aborts_startup() {
        let mut node = live_node_with_replay_store(true);
        let handle = node.handle();

        node.run().await.unwrap();

        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
        assert!(node.kernel.is_event_store_replay_configured());
        assert!(!node.kernel.is_event_store_replay());
        assert!(node.runner.is_none());
    }

    #[rstest]
    fn test_build_rejects_event_store_config_without_factory() {
        let config = LiveNodeConfig {
            event_store: Some(EventStoreConfig::default()),
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let err = LiveNodeBuilder::from_config(config)
            .expect("builder")
            .build()
            .expect_err("should reject event_store config without factory");

        assert!(
            err.to_string().contains("with_event_store"),
            "error message should mention with_event_store, was: {err}"
        );
    }

    #[rstest]
    fn test_direct_build_rejects_event_store_config() {
        let config = LiveNodeConfig {
            event_store: Some(EventStoreConfig::default()),
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let err = LiveNode::build("TestNode".to_string(), Some(config))
            .expect_err("LiveNode::build should reject event_store config");

        assert!(
            err.to_string().contains("with_event_store"),
            "error message should mention with_event_store, was: {err}"
        );
    }

    #[rstest]
    fn test_handle_initial_state() {
        let handle = LiveNodeHandle::new();

        assert_eq!(handle.state(), NodeState::Idle);
        assert!(!handle.should_stop());
        assert!(!handle.is_running());
    }

    #[rstest]
    fn test_handle_stop_sets_flag() {
        let handle = LiveNodeHandle::new();

        handle.stop();

        assert!(handle.should_stop());
    }

    #[rstest]
    fn test_handle_set_state_running_clears_stop_flag() {
        let handle = LiveNodeHandle::new();
        handle.stop();
        assert!(handle.should_stop());

        handle.set_state(NodeState::Running);

        assert!(!handle.should_stop());
        assert!(handle.is_running());
        assert_eq!(handle.state(), NodeState::Running);
    }

    #[rstest]
    fn test_handle_node_state_transitions() {
        let handle = LiveNodeHandle::new();
        assert_eq!(handle.state(), NodeState::Idle);

        handle.set_state(NodeState::Starting);
        assert_eq!(handle.state(), NodeState::Starting);
        assert!(!handle.is_running());

        handle.set_state(NodeState::Running);
        assert_eq!(handle.state(), NodeState::Running);
        assert!(handle.is_running());

        handle.set_state(NodeState::ShuttingDown);
        assert_eq!(handle.state(), NodeState::ShuttingDown);
        assert!(!handle.is_running());

        handle.set_state(NodeState::Stopped);
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(!handle.is_running());
    }

    #[rstest]
    fn test_handle_clone_shares_state_bidirectionally() {
        let handle1 = LiveNodeHandle::new();
        let handle2 = handle1.clone();

        // Mutation from handle1 visible in handle2
        handle1.stop();
        assert!(handle2.should_stop());

        // Mutation from handle2 visible in handle1
        handle2.set_state(NodeState::Running);
        assert_eq!(handle1.state(), NodeState::Running);
    }

    #[rstest]
    fn test_handle_stop_flag_independent_of_state() {
        let handle = LiveNodeHandle::new();

        // Stop flag can be set regardless of state
        handle.set_state(NodeState::Starting);
        handle.stop();
        assert!(handle.should_stop());
        assert_eq!(handle.state(), NodeState::Starting);

        // Only Running state clears the stop flag
        handle.set_state(NodeState::ShuttingDown);
        assert!(handle.should_stop()); // Still set

        handle.set_state(NodeState::Running);
        assert!(!handle.should_stop()); // Cleared
    }

    #[rstest]
    fn test_builder_creation() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_builder_rejects_backtest() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Backtest);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Backtest"));
    }

    #[rstest]
    fn test_builder_accepts_live_environment() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Live);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_builder_accepts_sandbox_environment() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_builder_fluent_api_chaining() {
        let builder = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Live)
            .unwrap()
            .with_name("TestNode")
            .with_instance_id(UUID4::new())
            .with_load_state(false)
            .with_save_state(true)
            .with_timeout_connection(30)
            .with_timeout_reconciliation(60)
            .with_reconciliation(true)
            .with_reconciliation_lookback_mins(120)
            .with_timeout_portfolio(10)
            .with_timeout_disconnection_secs(5)
            .with_delay_post_stop_secs(3)
            .with_delay_shutdown_secs(10);

        assert_eq!(builder.name(), "TestNode");
    }

    #[rstest]
    fn test_builder_with_external_msgbus_egress_uses_configured_encoding() {
        let (external_egress, publications, closed) = CapturingExternalEgress::new();
        let msgbus_config = MessageBusConfig {
            encoding: SerializationEncoding::Json,
            ..Default::default()
        };
        let node = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
            .unwrap()
            .with_msgbus_config(msgbus_config)
            .with_external_msgbus_egress(Box::new(external_egress))
            .build()
            .expect("node builds with external message bus egress");
        let quote = QuoteTick::default();

        msgbus::publish_quote("data.quotes.TEST".into(), &quote);

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic, "data.quotes.TEST");
        assert_eq!(
            serde_json::from_slice::<QuoteTick>(&publications[0].payload)
                .expect("JSON payload must decode as QuoteTick"),
            quote
        );
        drop(publications);

        msgbus::get_message_bus().borrow_mut().dispose();
        assert!(closed.get());
        drop(node);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_builder_with_external_msgbus_factory_installs_egress_and_ingress() {
        let quote = QuoteTick::default();
        let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let publications = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let factory = CapturingBackingFactory::new(publications.clone(), closed.clone(), Some(rx));
        let msgbus_config = MessageBusConfig {
            external_streams: Some(vec!["stream".to_string()]),
            ..Default::default()
        };
        let config = LiveNodeConfig {
            environment: Environment::Sandbox,
            msgbus: Some(msgbus_config),
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_connection: Duration::from_millis(500),
            timeout_disconnection: Duration::from_millis(500),
            ..Default::default()
        };
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_external_msgbus_factory(Box::new(factory))
            .build()
            .expect("node builds with external message bus factory");

        msgbus::publish_quote("data.quotes.TEST".into(), &quote);
        {
            let publications = publications.lock().unwrap();
            assert_eq!(publications.len(), 1);
            assert_eq!(publications[0].topic, "data.quotes.TEST");
            assert_eq!(
                serde_json::from_slice::<QuoteTick>(&publications[0].payload)
                    .expect("JSON payload must decode as QuoteTick"),
                quote
            );
        }

        let received = Rc::new(RefCell::new(Vec::<QuoteTick>::new()));
        let handle = node.handle();
        let handler = TypedHandler::from({
            let received = received.clone();
            let handle = handle.clone();
            move |quote: &QuoteTick| {
                received.borrow_mut().push(*quote);
                handle.stop();
            }
        });
        msgbus::subscribe_quotes("data.quotes.*".into(), handler, None);
        msgbus::get_message_bus()
            .borrow_mut()
            .add_streaming_type(BusPayloadType::QuoteTick);

        let payload =
            Bytes::from(serde_json::to_vec(&quote).expect("QuoteTick should serialize as JSON"));
        let message = BusMessage::with_str_topic(
            "data.quotes.TEST",
            BusPayloadType::QuoteTick,
            payload,
            SerializationEncoding::Json,
        );

        tokio::time::timeout(Duration::from_secs(5), async {
            let run = node.run();
            tokio::pin!(run);

            let drive = async {
                for _ in 0..100 {
                    if handle.is_running() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert!(handle.is_running(), "node should reach running state");

                tx.send(message)
                    .await
                    .expect("external ingress receiver should be open");

                for _ in 0..100 {
                    if received.borrow().len() == 1 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert_eq!(*received.borrow(), vec![quote]);
            };

            tokio::select! {
                biased;

                () = drive => {}
                result = &mut run => {
                    panic!("node stopped before factory ingress was republished: {result:?}");
                }
            }

            run.await.expect("node should stop cleanly");
        })
        .await
        .expect("live node should republish factory ingress and stop before timeout");

        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(closed.load(Ordering::Relaxed));
        msgbus::get_message_bus().borrow_mut().dispose();
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_builder_with_external_msgbus_factory_without_streams_runs_without_ingress() {
        let quote = QuoteTick::default();
        let publications = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let factory = CapturingBackingFactory::new(publications.clone(), closed.clone(), None);
        let config = LiveNodeConfig {
            environment: Environment::Sandbox,
            msgbus: Some(MessageBusConfig::default()),
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_connection: Duration::from_millis(500),
            timeout_disconnection: Duration::from_millis(500),
            ..Default::default()
        };
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_external_msgbus_factory(Box::new(factory))
            .build()
            .expect("node builds with egress-only message bus factory");
        let handle = node.handle();

        msgbus::publish_quote("data.quotes.TEST".into(), &quote);
        {
            let publications = publications.lock().unwrap();
            assert_eq!(publications.len(), 1);
            assert_eq!(publications[0].topic, "data.quotes.TEST");
        }

        tokio::time::timeout(Duration::from_secs(5), async {
            let run = node.run();
            tokio::pin!(run);

            let drive = async {
                for _ in 0..100 {
                    if handle.is_running() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert!(handle.is_running(), "node should reach running state");
                handle.stop();
            };

            tokio::select! {
                biased;

                () = drive => {}
                result = &mut run => {
                    panic!("node stopped before egress-only factory run was observed: {result:?}");
                }
            }

            run.await.expect("node should stop cleanly");
        })
        .await
        .expect("live node should run without external ingress before timeout");

        assert_eq!(handle.state(), NodeState::Stopped);
        msgbus::get_message_bus().borrow_mut().dispose();
        assert!(closed.load(Ordering::Relaxed));
    }

    #[rstest]
    fn test_builder_with_external_msgbus_factory_rejects_injected_surfaces() {
        let (external_egress, _publications, _closed) = CapturingExternalEgress::new();
        let egress_factory = CapturingBackingFactory::new(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(AtomicBool::new(false)),
            None,
        );
        let egress_error = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
            .unwrap()
            .with_external_msgbus_factory(Box::new(egress_factory))
            .with_external_msgbus_egress(Box::new(external_egress))
            .build()
            .expect_err("builder should reject factory plus injected egress");

        assert!(
            egress_error
                .to_string()
                .contains("cannot be combined with injected egress or ingress")
        );

        let (_tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let ingress_factory = CapturingBackingFactory::new(
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(AtomicBool::new(false)),
            None,
        );
        let ingress = CapturingExternalIngress::new(rx, Rc::new(Cell::new(false)));
        let ingress_error = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
            .unwrap()
            .with_external_msgbus_factory(Box::new(ingress_factory))
            .with_external_ingress(Box::new(ingress))
            .build()
            .expect_err("builder should reject factory plus injected ingress");

        assert!(
            ingress_error
                .to_string()
                .contains("cannot be combined with injected egress or ingress")
        );
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_run_republishes_external_ingress_on_local_msgbus() {
        let quote = QuoteTick::default();
        let received = Rc::new(RefCell::new(Vec::<QuoteTick>::new()));
        let payload =
            Bytes::from(serde_json::to_vec(&quote).expect("QuoteTick should serialize as JSON"));
        let message = BusMessage::with_str_topic(
            "data.quotes.TEST",
            BusPayloadType::QuoteTick,
            payload,
            SerializationEncoding::Json,
        );
        let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let closed = Rc::new(Cell::new(false));
        let ingress = CapturingExternalIngress::new(rx, closed.clone());
        let config = LiveNodeConfig {
            environment: Environment::Sandbox,
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_connection: Duration::from_millis(500),
            timeout_disconnection: Duration::from_millis(500),
            ..Default::default()
        };
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_external_ingress(Box::new(ingress))
            .build()
            .expect("node builds with external message bus ingress");
        let handle = node.handle();
        let handler = TypedHandler::from({
            let received = received.clone();
            move |quote: &QuoteTick| {
                received.borrow_mut().push(*quote);
            }
        });
        msgbus::subscribe_quotes("data.quotes.*".into(), handler, None);
        msgbus::get_message_bus()
            .borrow_mut()
            .add_streaming_type(BusPayloadType::QuoteTick);

        tokio::time::timeout(Duration::from_secs(5), async {
            let run = node.run();
            tokio::pin!(run);

            let drive = async {
                for _ in 0..100 {
                    if handle.is_running() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert!(handle.is_running(), "node should reach running state");

                tx.send(message)
                    .await
                    .expect("external ingress receiver should be open");

                for _ in 0..100 {
                    if received.borrow().len() == 1 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert_eq!(*received.borrow(), vec![quote]);
                handle.stop();
            };

            tokio::select! {
                biased;

                () = drive => {}
                result = &mut run => {
                    panic!("node stopped before external message was republished: {result:?}");
                }
            }

            run.await.expect("node should stop cleanly");
        })
        .await
        .expect("live node should republish ingress and stop before timeout");

        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(closed.get());
        msgbus::get_message_bus().borrow_mut().dispose();
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_run_closes_external_ingress_when_receiver_closes() {
        let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let closed = Rc::new(Cell::new(false));
        let ingress = CapturingExternalIngress::new(rx, closed.clone());
        let config = LiveNodeConfig {
            environment: Environment::Sandbox,
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_connection: Duration::from_millis(500),
            timeout_disconnection: Duration::from_millis(500),
            ..Default::default()
        };
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_external_ingress(Box::new(ingress))
            .build()
            .expect("node builds with external message bus ingress");
        let handle = node.handle();

        tokio::time::timeout(Duration::from_secs(5), async {
            let run = node.run();
            tokio::pin!(run);

            let drive = async {
                for _ in 0..100 {
                    if handle.is_running() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert!(handle.is_running(), "node should reach running state");

                drop(tx);

                for _ in 0..100 {
                    if closed.get() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                assert!(closed.get(), "external ingress should close");
                assert!(
                    handle.is_running(),
                    "node should keep running after ingress closes"
                );
                handle.stop();
            };

            tokio::select! {
                biased;

                () = drive => {}
                result = &mut run => {
                    panic!("node stopped before ingress close was observed: {result:?}");
                }
            }

            run.await.expect("node should stop cleanly");
        })
        .await
        .expect("live node should close ingress and stop before timeout");

        assert_eq!(handle.state(), NodeState::Stopped);
    }

    #[rstest]
    #[tokio::test(flavor = "current_thread")]
    async fn test_run_aborts_startup_when_external_ingress_receiver_unavailable() {
        let closed = Rc::new(Cell::new(false));
        let ingress = FailingExternalIngress::new(closed.clone());
        let config = LiveNodeConfig {
            environment: Environment::Sandbox,
            exec_engine: crate::config::LiveExecEngineConfig {
                reconciliation: false,
                ..Default::default()
            },
            delay_post_stop: Duration::ZERO,
            timeout_connection: Duration::from_millis(500),
            timeout_disconnection: Duration::from_millis(500),
            ..Default::default()
        };
        let mut node = LiveNodeBuilder::from_config(config)
            .unwrap()
            .with_external_ingress(Box::new(ingress))
            .build()
            .expect("node builds with external message bus ingress");
        let handle = node.handle();

        let err = node.run().await.expect_err("run should fail");

        assert!(
            err.to_string()
                .contains("external ingress receiver unavailable")
        );
        assert_eq!(handle.state(), NodeState::Stopped);
        assert!(closed.get());
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_node_build_and_initial_state() {
        let node = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
            .unwrap()
            .with_name("TestNode")
            .build()
            .unwrap();

        assert_eq!(node.state(), NodeState::Idle);
        assert!(!node.is_running());
        assert_eq!(node.environment(), Environment::Sandbox);
        assert_eq!(node.trader_id(), TraderId::from("TRADER-001"));
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_node_build_replaces_stale_runner_senders() {
        replace_data_cmd_sender(Arc::new(SyncDataCommandSender));
        replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));

        let first = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
            .unwrap()
            .with_name("FirstNode")
            .build()
            .unwrap();

        assert_eq!(first.state(), NodeState::Idle);
        drop(first);

        let second = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
            .unwrap()
            .with_name("SecondNode")
            .build()
            .unwrap();

        assert_eq!(second.state(), NodeState::Idle);
        assert!(!second.is_running());
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_node_handle_reflects_node_state() {
        let node = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
            .unwrap()
            .with_name("TestNode")
            .build()
            .unwrap();

        let handle = node.handle();

        assert_eq!(handle.state(), NodeState::Idle);
        assert!(!handle.is_running());
    }

    #[rstest]
    fn test_pending_drain_data_returns_false_when_empty() {
        let mut pending = PendingEvents::default();

        assert!(!pending.drain_data());
    }

    #[rstest]
    fn test_pending_drain_data_returns_true_when_non_empty() {
        use nautilus_model::instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt};

        let mut pending = PendingEvents::default();
        pending
            .data_evts
            .push(DataEvent::Instrument(InstrumentAny::CryptoPerpetual(
                crypto_perpetual_ethusdt(),
            )));

        assert!(pending.drain_data());
        assert!(pending.data_evts.is_empty());
    }

    fn stub_data_event() -> DataEvent {
        use nautilus_model::instruments::{InstrumentAny, stubs::crypto_perpetual_ethusdt};

        DataEvent::Instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt()))
    }

    fn stub_data_command() -> DataCommand {
        use nautilus_common::messages::data::{SubscribeCommand, subscribe::SubscribeInstruments};
        use nautilus_core::{UUID4, UnixNanos};
        use nautilus_model::identifiers::Venue;

        DataCommand::Subscribe(SubscribeCommand::Instruments(SubscribeInstruments::new(
            None,
            Venue::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        )))
    }

    #[rstest]
    fn test_flush_pending_data_drains_events_and_commands() {
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();

        let mut pending = PendingEvents::default();

        // Pre-load pending (items captured by the select loop)
        pending.data_evts.push(stub_data_event());
        pending.data_cmds.push(stub_data_command());

        // Pre-load channels (items missed by the select loop)
        evt_tx.send(stub_data_event()).unwrap();
        cmd_tx.send(stub_data_command()).unwrap();

        flush_pending_data(&mut pending, &mut evt_rx, &mut cmd_rx);

        assert!(pending.data_evts.is_empty());
        assert!(pending.data_cmds.is_empty());
        assert!(evt_rx.try_recv().is_err());
        assert!(cmd_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_flush_pending_data_drains_mixed_sources() {
        let (evt_tx, mut evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();

        let mut pending = PendingEvents::default();

        // First pass: pending has an event, channel has a command
        pending.data_evts.push(stub_data_event());
        cmd_tx.send(stub_data_command()).unwrap();

        // Second pass: channel has items that simulate arrival during first drain
        evt_tx.send(stub_data_event()).unwrap();
        evt_tx.send(stub_data_event()).unwrap();
        cmd_tx.send(stub_data_command()).unwrap();

        flush_pending_data(&mut pending, &mut evt_rx, &mut cmd_rx);

        assert!(pending.data_evts.is_empty());
        assert!(pending.data_cmds.is_empty());
        assert!(evt_rx.try_recv().is_err());
        assert!(cmd_rx.try_recv().is_err());
    }

    fn stub_time_event_handler() -> TimeEventHandler {
        use std::rc::Rc;

        use nautilus_common::timer::{TimeEvent, TimeEventCallback, TimeEventHandler};
        use nautilus_core::{UUID4, UnixNanos};
        use ustr::Ustr;

        TimeEventHandler::new(
            TimeEvent::new(
                Ustr::from("test-timer"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
            TimeEventCallback::RustLocal(Rc::new(|_| {})),
        )
    }

    fn stub_trading_command() -> TradingCommand {
        use nautilus_common::messages::execution::query::QueryAccount;
        use nautilus_core::{UUID4, UnixNanos};
        use nautilus_model::identifiers::AccountId;

        TradingCommand::QueryAccount(QueryAccount::new(
            TraderId::from("TESTER-001"),
            None,
            AccountId::from("TEST-001"),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None, // correlation_id
        ))
    }

    fn stub_exec_event() -> ExecutionEvent {
        use nautilus_model::{
            enums::{LiquiditySide, OrderSide},
            identifiers::{AccountId, InstrumentId, TradeId, VenueOrderId},
            reports::FillReport,
            types::{Money, Price, Quantity},
        };

        ExecutionEvent::Report(ExecutionReport::Fill(Box::new(FillReport::new(
            AccountId::from("TEST-001"),
            InstrumentId::from("TEST.VENUE"),
            VenueOrderId::from("V-001"),
            TradeId::from("T-001"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("100.0"),
            Money::from("0.01 USD"),
            LiquiditySide::Maker,
            None,
            None,
            nautilus_core::UnixNanos::default(),
            nautilus_core::UnixNanos::default(),
            None,
        ))))
    }

    #[rstest]
    fn test_flush_all_pending_drains_buffered_channels() {
        let (time_tx, mut time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (data_evt_tx, mut data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (data_cmd_tx, mut data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (exec_evt_tx, mut exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (exec_cmd_tx, mut exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommand>();

        let mut pending = PendingEvents::default();

        // Pre-load pending with data items
        pending.data_evts.push(stub_data_event());
        pending.data_cmds.push(stub_data_command());

        // Pre-load all channel types
        time_tx.send(stub_time_event_handler()).unwrap();
        data_evt_tx.send(stub_data_event()).unwrap();
        data_cmd_tx.send(stub_data_command()).unwrap();
        exec_evt_tx.send(stub_exec_event()).unwrap();
        exec_cmd_tx.send(stub_trading_command()).unwrap();

        flush_all_pending(
            &mut pending,
            &mut time_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );

        assert!(pending.data_evts.is_empty());
        assert!(pending.data_cmds.is_empty());
        assert!(pending.exec_reports.is_empty());
        assert!(pending.exec_cmds.is_empty());
        assert!(pending.order_evts.is_empty());
        assert!(time_rx.try_recv().is_err());
        assert!(data_evt_rx.try_recv().is_err());
        assert!(data_cmd_rx.try_recv().is_err());
        assert!(exec_evt_rx.try_recv().is_err());
        assert!(exec_cmd_rx.try_recv().is_err());
    }

    fn stub_order_event() -> ExecutionEvent {
        use nautilus_model::events::order::spec::OrderSubmittedSpec;

        ExecutionEvent::Order(OrderEventAny::Submitted(
            OrderSubmittedSpec::builder().build(),
        ))
    }

    fn stub_account_event() -> ExecutionEvent {
        use nautilus_core::{UUID4, UnixNanos};
        use nautilus_model::{
            enums::AccountType, events::account::state::AccountState, identifiers::AccountId,
        };

        ExecutionEvent::Account(AccountState::new(
            AccountId::from("TEST-001"),
            AccountType::Cash,
            vec![],
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        ))
    }

    #[rstest]
    fn test_flush_all_pending_routes_order_event_to_order_evts() {
        let (_time_tx, mut time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_data_evt_tx, mut data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, mut data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (exec_evt_tx, mut exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, mut exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommand>();

        let mut pending = PendingEvents::default();

        exec_evt_tx.send(stub_order_event()).unwrap();
        exec_evt_tx.send(stub_exec_event()).unwrap();

        flush_all_pending(
            &mut pending,
            &mut time_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );

        // Both order and report events are drained by pending.drain()
        assert!(pending.order_evts.is_empty());
        assert!(pending.exec_reports.is_empty());
        assert!(exec_evt_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_flush_all_pending_routes_account_event_immediately() {
        let (_time_tx, mut time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_data_evt_tx, mut data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, mut data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (exec_evt_tx, mut exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, mut exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommand>();

        let mut pending = PendingEvents::default();

        exec_evt_tx.send(stub_account_event()).unwrap();

        flush_all_pending(
            &mut pending,
            &mut time_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );

        // Account events are forwarded immediately, never buffered in pending
        assert!(pending.exec_reports.is_empty());
        assert!(pending.order_evts.is_empty());
        assert!(pending.exec_cmds.is_empty());
        assert!(exec_evt_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_pending_is_empty_when_default() {
        let pending = PendingEvents::default();

        assert!(pending.is_empty());
    }

    #[rstest]
    fn test_pending_is_empty_false_with_data_evt() {
        let mut pending = PendingEvents::default();
        pending.data_evts.push(stub_data_event());

        assert!(!pending.is_empty());
    }

    #[rstest]
    fn test_pending_is_empty_false_with_data_cmd() {
        let mut pending = PendingEvents::default();
        pending.data_cmds.push(stub_data_command());

        assert!(!pending.is_empty());
    }

    #[rstest]
    fn test_pending_is_empty_false_with_exec_cmd() {
        let mut pending = PendingEvents::default();
        pending.exec_cmds.push(stub_trading_command());

        assert!(!pending.is_empty());
    }

    #[rstest]
    fn test_pending_is_empty_false_with_exec_report() {
        let mut pending = PendingEvents::default();

        if let ExecutionEvent::Report(report) = stub_exec_event() {
            pending.exec_reports.push(report);
        }

        assert!(!pending.is_empty());
    }

    #[rstest]
    fn test_pending_is_empty_false_with_order_evt() {
        let mut pending = PendingEvents::default();

        if let ExecutionEvent::Order(order_evt) = stub_order_event() {
            pending.order_evts.push(order_evt);
        }

        assert!(!pending.is_empty());
    }

    fn stub_submitted_batch_event() -> ExecutionEvent {
        use nautilus_model::{
            events::{OrderSubmittedBatch, order::spec::OrderSubmittedSpec},
            identifiers::ClientOrderId,
        };

        let events = vec![
            OrderSubmittedSpec::builder()
                .client_order_id(ClientOrderId::from("O-001"))
                .build(),
            OrderSubmittedSpec::builder()
                .client_order_id(ClientOrderId::from("O-002"))
                .build(),
        ];

        ExecutionEvent::OrderSubmittedBatch(OrderSubmittedBatch::new(events))
    }

    fn stub_canceled_batch_event() -> ExecutionEvent {
        use nautilus_model::{
            events::{OrderCanceledBatch, order::spec::OrderCanceledSpec},
            identifiers::ClientOrderId,
        };

        let events = vec![
            OrderCanceledSpec::builder()
                .client_order_id(ClientOrderId::from("O-001"))
                .build(),
            OrderCanceledSpec::builder()
                .client_order_id(ClientOrderId::from("O-002"))
                .build(),
        ];

        ExecutionEvent::OrderCanceledBatch(OrderCanceledBatch::new(events))
    }

    #[rstest]
    fn test_flush_all_pending_buffers_submitted_batch_as_individual_events() {
        let (_time_tx, mut time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_data_evt_tx, mut data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, mut data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (exec_evt_tx, mut exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, mut exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommand>();

        let mut pending = PendingEvents::default();

        exec_evt_tx.send(stub_submitted_batch_event()).unwrap();

        flush_all_pending(
            &mut pending,
            &mut time_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );

        // Batch should be unpacked into individual Submitted events then drained
        assert!(pending.order_evts.is_empty());
        assert!(exec_evt_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_flush_all_pending_buffers_canceled_batch_as_individual_events() {
        let (_time_tx, mut time_rx) = tokio::sync::mpsc::unbounded_channel::<TimeEventHandler>();
        let (_data_evt_tx, mut data_evt_rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let (_data_cmd_tx, mut data_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<DataCommand>();
        let (exec_evt_tx, mut exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        let (_exec_cmd_tx, mut exec_cmd_rx) =
            tokio::sync::mpsc::unbounded_channel::<TradingCommand>();

        let mut pending = PendingEvents::default();

        exec_evt_tx.send(stub_canceled_batch_event()).unwrap();

        flush_all_pending(
            &mut pending,
            &mut time_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );

        // Batch should be unpacked into individual Canceled events then drained
        assert!(pending.order_evts.is_empty());
        assert!(exec_evt_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_flush_all_pending_expands_batch_into_order_evts_before_drain() {
        use nautilus_model::identifiers::ClientOrderId;

        let (exec_evt_tx, mut exec_evt_rx) =
            tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();

        exec_evt_tx.send(stub_canceled_batch_event()).unwrap();

        let mut pending = PendingEvents::default();

        // Manually replicate what flush_all_pending does before drain
        while let Ok(evt) = exec_evt_rx.try_recv() {
            match evt {
                ExecutionEvent::Account(_) => {
                    AsyncRunner::handle_exec_event(evt);
                }
                ExecutionEvent::Report(report) => {
                    pending.exec_reports.push(report);
                }
                ExecutionEvent::Order(order_evt) => {
                    pending.order_evts.push(order_evt);
                }
                ExecutionEvent::OrderSubmittedBatch(batch) => {
                    for submitted in batch {
                        pending.order_evts.push(OrderEventAny::Submitted(submitted));
                    }
                }
                ExecutionEvent::OrderAcceptedBatch(batch) => {
                    for accepted in batch {
                        pending.order_evts.push(OrderEventAny::Accepted(accepted));
                    }
                }
                ExecutionEvent::OrderCanceledBatch(batch) => {
                    for canceled in batch {
                        pending.order_evts.push(OrderEventAny::Canceled(canceled));
                    }
                }
            }
        }

        assert_eq!(pending.order_evts.len(), 2);
        assert!(
            matches!(&pending.order_evts[0], OrderEventAny::Canceled(c) if c.client_order_id == ClientOrderId::from("O-001"))
        );
        assert!(
            matches!(&pending.order_evts[1], OrderEventAny::Canceled(c) if c.client_order_id == ClientOrderId::from("O-002"))
        );
    }

    #[derive(Debug)]
    struct CapturedEgressMessage {
        topic: String,
        payload: Bytes,
    }

    type CapturedEgressMessages = Rc<RefCell<Vec<CapturedEgressMessage>>>;
    type SharedClosed = Rc<Cell<bool>>;

    #[derive(Debug)]
    struct CapturingExternalIngress {
        rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
        closed: SharedClosed,
    }

    impl CapturingExternalIngress {
        fn new(rx: tokio::sync::mpsc::Receiver<BusMessage>, closed: SharedClosed) -> Self {
            Self {
                rx: Some(rx),
                closed,
            }
        }
    }

    impl MessageBusExternalIngress for CapturingExternalIngress {
        fn is_closed(&self) -> bool {
            self.closed.get()
        }

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            self.rx
                .take()
                .ok_or_else(|| anyhow::anyhow!("external ingress receiver already taken"))
        }

        fn close(&mut self) {
            self.closed.set(true);
        }
    }

    #[derive(Debug)]
    struct FailingExternalIngress {
        closed: SharedClosed,
    }

    impl FailingExternalIngress {
        fn new(closed: SharedClosed) -> Self {
            Self { closed }
        }
    }

    impl MessageBusExternalIngress for FailingExternalIngress {
        fn is_closed(&self) -> bool {
            self.closed.get()
        }

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            anyhow::bail!("external ingress receiver unavailable")
        }

        fn close(&mut self) {
            self.closed.set(true);
        }
    }

    struct CapturingExternalEgress {
        publications: CapturedEgressMessages,
        closed: SharedClosed,
    }

    impl CapturingExternalEgress {
        fn new() -> (Self, CapturedEgressMessages, SharedClosed) {
            let publications = Rc::new(RefCell::new(Vec::new()));
            let closed = Rc::new(Cell::new(false));
            (
                Self {
                    publications: publications.clone(),
                    closed: closed.clone(),
                },
                publications,
                closed,
            )
        }
    }

    impl MessageBusExternalEgress for CapturingExternalEgress {
        fn is_closed(&self) -> bool {
            self.closed.get()
        }

        fn publish(&self, message: BusMessage) {
            self.publications.borrow_mut().push(CapturedEgressMessage {
                topic: message.topic.to_string(),
                payload: message.payload,
            });
        }

        fn close(&mut self) {
            self.closed.set(true);
        }
    }

    struct CapturingBackingFactory {
        publications: Arc<Mutex<Vec<CapturedEgressMessage>>>,
        closed: Arc<AtomicBool>,
        rx: Mutex<Option<tokio::sync::mpsc::Receiver<BusMessage>>>,
    }

    impl CapturingBackingFactory {
        fn new(
            publications: Arc<Mutex<Vec<CapturedEgressMessage>>>,
            closed: Arc<AtomicBool>,
            rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
        ) -> Self {
            Self {
                publications,
                closed,
                rx: Mutex::new(rx),
            }
        }
    }

    impl Debug for CapturingBackingFactory {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct(stringify!(CapturingBackingFactory))
                .finish_non_exhaustive()
        }
    }

    impl MessageBusBackingFactory for CapturingBackingFactory {
        fn create(
            &self,
            _trader_id: TraderId,
            _instance_id: UUID4,
            _config: MessageBusConfig,
        ) -> anyhow::Result<Box<dyn MessageBusBacking>> {
            let rx = self.rx.lock().unwrap().take();
            Ok(Box::new(CapturingBacking {
                publications: self.publications.clone(),
                closed: self.closed.clone(),
                rx,
            }))
        }
    }

    struct CapturingBacking {
        publications: Arc<Mutex<Vec<CapturedEgressMessage>>>,
        closed: Arc<AtomicBool>,
        rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
    }

    impl MessageBusBacking for CapturingBacking {
        fn is_closed(&self) -> bool {
            self.closed.load(Ordering::Relaxed)
        }

        fn publish(&self, message: BusMessage) {
            self.publications
                .lock()
                .unwrap()
                .push(CapturedEgressMessage {
                    topic: message.topic.to_string(),
                    payload: message.payload,
                });
        }

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            self.rx
                .take()
                .ok_or_else(|| anyhow::anyhow!("external ingress receiver unavailable"))
        }

        fn close(&mut self) {
            self.closed.store(true, Ordering::Relaxed);
        }
    }
}
