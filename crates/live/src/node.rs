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
//! Three sub-checks run on independent intervals: inflight orders, open order
//! consistency, and position consistency. A single reconciliation timer fires
//! at the minimum enabled interval. Each tick, the handler checks which
//! sub-checks are due based on elapsed nanoseconds and runs them in sequence.
//! The open order and position checks query venues via async HTTP calls,
//! blocking the select loop for the duration of each query.

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use nautilus_common::{
    actor::{Actor, DataActor},
    cache::database::CacheDatabaseAdapter,
    component::Component,
    enums::{Environment, LogColor},
    live::dst,
    log_info,
    messages::{
        DataEvent, ExecutionEvent, ExecutionReport, data::DataCommand, execution::TradingCommand,
    },
    timer::TimeEventHandler,
};
use nautilus_core::{
    UUID4, UnixNanos,
    datetime::{NANOSECONDS_IN_MILLISECOND, mins_to_secs, secs_to_nanos_unchecked},
};
use nautilus_model::{
    events::OrderEventAny,
    identifiers::{ClientOrderId, StrategyId, TraderId},
    orders::Order,
};
use nautilus_system::{config::NautilusKernelConfig, kernel::NautilusKernel};
use nautilus_trading::{ExecutionAlgorithm, strategy::Strategy};
use tabled::{Table, Tabled, settings::Style};

use crate::{
    builder::LiveNodeBuilder,
    config::LiveNodeConfig,
    manager::{ExecutionManager, ExecutionManagerConfig},
    runner::{AsyncRunner, AsyncRunnerChannels},
};

/// Lifecycle state of the `LiveNode` runner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeState {
    #[default]
    Idle = 0,
    Starting = 1,
    Running = 2,
    ShuttingDown = 3,
    Stopped = 4,
}

impl NodeState {
    /// Creates a `NodeState` from its `u8` representation.
    ///
    /// # Panics
    ///
    /// Panics if the value is not a valid `NodeState` discriminant (0-4).
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Idle,
            1 => Self::Starting,
            2 => Self::Running,
            3 => Self::ShuttingDown,
            4 => Self::Stopped,
            _ => panic!("Invalid NodeState value"),
        }
    }

    /// Returns the `u8` representation of this state.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// Returns whether the state is `Running`.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// A thread-safe handle to control a `LiveNode` from other threads.
///
/// This allows stopping and querying the node's state without requiring the
/// node itself to be Send + Sync.
#[derive(Clone, Debug)]
pub struct LiveNodeHandle {
    /// Atomic flag indicating if the node should stop.
    pub(crate) stop_flag: Arc<AtomicBool>,
    /// Atomic state as `NodeState::as_u8()`.
    pub(crate) state: Arc<AtomicU8>,
}

impl Default for LiveNodeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveNodeHandle {
    /// Creates a new handle with default (`Idle`) state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            state: Arc::new(AtomicU8::new(NodeState::Idle.as_u8())),
        }
    }

    /// Sets the node state (internal use).
    pub(crate) fn set_state(&self, state: NodeState) {
        self.state.store(state.as_u8(), Ordering::Relaxed);
        if state == NodeState::Running {
            // Clear stop flag when entering running state
            self.stop_flag.store(false, Ordering::Relaxed);
        }
    }

    /// Returns the current node state.
    #[must_use]
    pub fn state(&self) -> NodeState {
        NodeState::from_u8(self.state.load(Ordering::Relaxed))
    }

    /// Returns whether the node should stop.
    #[must_use]
    pub fn should_stop(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }

    /// Returns whether the node is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.state().is_running()
    }

    /// Signals the node to stop.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

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
    shutdown_deadline: Option<dst::time::Instant>,
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
    ) -> Self {
        Self {
            kernel,
            runner: Some(runner),
            config,
            handle: LiveNodeHandle::new(),
            exec_manager,
            shutdown_deadline: None,
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

        log::info!("LiveNode built successfully with kernel config");

        Ok(Self {
            kernel,
            runner: Some(runner),
            config,
            handle: LiveNodeHandle::new(),
            exec_manager,
            shutdown_deadline: None,
            #[cfg(feature = "python")]
            python_actors: Vec::new(),
        })
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

        self.kernel.start_async().await;

        // Connect data clients first and flush instrument events into cache
        self.kernel.connect_data_clients().await;

        if let Some(runner) = self.runner.as_mut() {
            runner.flush_pending_data();
        }

        self.kernel.connect_exec_clients().await;

        if !self.await_engines_connected().await {
            log::error!("Cannot start trader: engine client(s) not connected");
            self.handle.set_state(NodeState::Running);
            return Ok(());
        }

        self.perform_startup_reconciliation().await?;

        self.kernel.start_trader();

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

        self.kernel.stop_trader();
        let delay = self.kernel.delay_post_stop();
        log::info!("Awaiting residual events ({delay:?})...");

        dst::time::sleep(delay).await;
        self.finalize_stop().await
    }

    /// Awaits engine clients to connect with timeout.
    ///
    /// Returns `true` if all engines connected, `false` if timed out.
    async fn await_engines_connected(&self) -> bool {
        log::info!(
            "Awaiting engine connections ({:?} timeout)...",
            self.config.timeout_connection
        );

        let start = dst::time::Instant::now();
        let timeout = self.config.timeout_connection;
        let interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.kernel.check_engines_connected() {
                log::info!("All engine clients connected");
                return true;
            }
            dst::time::sleep(interval).await;
        }

        self.log_connection_status();
        false
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
            .map(|m| m as u64);

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
            mut data_evt_rx,
            mut data_cmd_rx,
            mut exec_evt_rx,
            mut exec_cmd_rx,
        } = runner.take_channels();

        log::info!("Event loop starting");

        self.handle.set_state(NodeState::Starting);
        self.kernel.start_async().await;
        self.kernel.reset_shutdown_flag();

        let stop_handle = self.handle.clone();
        let shutdown_flag = self.kernel.shutdown_flag();
        let mut pending = PendingEvents::default();

        // Startup phase 1: Connect data clients and drain instrument events into cache.
        // This ensures the cache is populated before execution clients connect.
        // TODO: Add ctrl_c and stop_handle monitoring here to allow aborting a
        // hanging startup. Currently signals during startup are ignored, and
        // any pending stop_flag is cleared when transitioning to Running.
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
        let engines_connected = drive_with_event_buffering(
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

        if engines_connected {
            // Run reconciliation now that instruments are in cache and start trader
            self.perform_startup_reconciliation().await?;
            self.kernel.start_trader();
        } else {
            log::error!("Not starting trader: engine client(s) not connected");
        }

        self.handle.set_state(NodeState::Running);

        let exec_config = &self.config.exec_engine;
        let inflight_interval_ns =
            (exec_config.inflight_check_interval_ms as u64) * NANOSECONDS_IN_MILLISECOND;
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
                intervals.push(Duration::from_millis(
                    exec_config.inflight_check_interval_ms as u64,
                ));
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
        // begin. Matches the legacy Python semantics in `LiveExecutionEngine`.
        let startup_delay = if self.config.exec_engine.reconciliation {
            Duration::from_secs_f64(exec_config.reconciliation_startup_delay_secs)
        } else {
            Duration::ZERO
        };

        let recon_start = dst::time::Instant::now() + startup_delay;

        let mut ts_last_inflight = self.exec_manager.generate_timestamp_ns();
        let mut ts_last_open = ts_last_inflight;
        let mut ts_last_position = ts_last_inflight;

        // Disabled timers use a far-future interval so they never fire.
        // All timers start one full interval after the startup delay
        // so the first tick does not fire immediately.
        let far_future = Duration::from_secs(86400 * 365 * 100);

        let make_timer = |opt_dur: Option<Duration>| {
            let dur = opt_dur.unwrap_or(far_future);
            let mut timer = dst::time::interval_at(recon_start + dur, dur);
            timer.set_missed_tick_behavior(dst::time::MissedTickBehavior::Delay);
            timer
        };

        let mut recon_timer = make_timer(if recon_enabled {
            Some(recon_min_interval)
        } else {
            None
        });

        let mut purge_orders_timer = make_timer(
            exec_config
                .purge_closed_orders_interval_mins
                .filter(|&m| m > 0)
                .map(|m| Duration::from_secs(mins_to_secs(m as u64))),
        );

        let mut purge_positions_timer = make_timer(
            exec_config
                .purge_closed_positions_interval_mins
                .filter(|&m| m > 0)
                .map(|m| Duration::from_secs(mins_to_secs(m as u64))),
        );

        let mut purge_account_timer = make_timer(
            exec_config
                .purge_account_events_interval_mins
                .filter(|&m| m > 0)
                .map(|m| Duration::from_secs(mins_to_secs(m as u64))),
        );

        let mut own_books_timer = make_timer(
            exec_config
                .own_books_audit_interval_secs
                .filter(|&s| s > 0.0)
                .map(Duration::from_secs_f64),
        );

        let mut prune_fills_timer = make_timer(Some(Duration::from_secs(60)));

        // Stop-check timer is not subject to the reconciliation startup delay,
        // so shutdown signals remain responsive from the moment the node reaches
        // `Running`. Set `MissedTickBehavior::Skip` so backlog ticks do not fire
        // a burst after the select arm was suspended by other branches.
        let mut stop_check_timer = dst::time::interval(Duration::from_millis(100));
        stop_check_timer.set_missed_tick_behavior(dst::time::MissedTickBehavior::Skip);

        // Running phase: runs until shutdown deadline expires
        let mut residual_events = 0usize;
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
                    } else if shutdown_flag.get() {
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

                // Housekeeping timers (before event processing to avoid starvation)
                _ = recon_timer.tick(), if is_running && recon_enabled => {
                    if let Err(e) = self.run_reconciliation_checks(
                        inflight_interval_ns,
                        open_interval_ns,
                        position_interval_ns,
                        &mut ts_last_inflight,
                        &mut ts_last_open,
                        &mut ts_last_position,
                    ).await {
                        log::error!("Reconciliation check error: {e}");
                    }
                }
                _ = purge_orders_timer.tick(), if is_running => {
                    self.exec_manager.purge_closed_orders();
                }
                _ = purge_positions_timer.tick(), if is_running => {
                    self.exec_manager.purge_closed_positions();
                }
                _ = purge_account_timer.tick(), if is_running => {
                    self.exec_manager.purge_account_events();
                }
                _ = own_books_timer.tick(), if is_running => {
                    self.kernel.cache().borrow_mut().audit_own_order_books();
                }
                _ = prune_fills_timer.tick(), if is_running => {
                    self.exec_manager.prune_recent_fills_cache(60.0);
                }

                // Event processing branches
                Some(handler) = time_evt_rx.recv() => {
                    AsyncRunner::handle_time_event(handler);

                    if is_shutting_down {
                        log::debug!("Residual time event");
                        residual_events += 1;
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
                                        fill.ts_event,
                                    );
                                    self.exec_manager.mark_fill_processed(fill.trade_id);
                                }
                                OrderEventAny::Accepted(_) => {
                                    self.exec_manager.clear_recon_tracking(
                                        &order_evt.client_order_id(), true,
                                    );
                                }
                                OrderEventAny::Rejected(_)
                                | OrderEventAny::Canceled(_)
                                | OrderEventAny::Expired(_)
                                | OrderEventAny::Denied(_) => {
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
                        TradingCommand::CancelOrder(cancel) => {
                            self.exec_manager.register_inflight(cancel.client_order_id);
                        }
                        _ => {}
                    }
                    AsyncRunner::handle_exec_command(cmd);
                }
            }
        }

        if residual_events > 0 {
            log::debug!("Processed {residual_events} residual events during shutdown");
        }

        let _ = self.kernel.cache().borrow().check_residuals();

        self.finalize_stop().await?;

        // Handle events that arrived during finalize_stop
        self.drain_channels(
            &mut time_evt_rx,
            &mut data_evt_rx,
            &mut data_cmd_rx,
            &mut exec_evt_rx,
            &mut exec_cmd_rx,
        );

        log::info!("Event loop stopped");

        Ok(())
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
                self.exec_manager
                    .record_position_activity(fill.instrument_id, fill.ts_event);
                self.exec_manager.mark_fill_processed(fill.trade_id);
            }
            self.kernel.exec_engine.borrow_mut().process(event);
        }
    }

    /// Connects execution clients and checks all engines are connected.
    ///
    /// Returns `true` if all engines connected successfully, `false` otherwise.
    /// Must be called after data clients are connected and instrument events drained.
    async fn connect_exec_phase(&mut self) -> anyhow::Result<bool> {
        self.kernel.connect_exec_clients().await;

        if !self.await_engines_connected().await {
            return Ok(false);
        }

        Ok(true)
    }

    fn initiate_shutdown(&mut self) {
        self.kernel.stop_trader();
        let delay = self.kernel.delay_post_stop();
        log::info!("Awaiting residual events ({delay:?})...");

        self.shutdown_deadline = Some(dst::time::Instant::now() + delay);
        self.handle.set_state(NodeState::ShuttingDown);
    }

    async fn finalize_stop(&mut self) -> anyhow::Result<()> {
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
        &self,
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
        T: DataActor + Component + Actor + 'static,
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
        T: DataActor + Component + Actor + 'static,
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
    pub fn add_strategy<T>(&mut self, strategy: T) -> anyhow::Result<()>
    where
        T: Strategy + Component + Debug + 'static,
    {
        if self.state() != NodeState::Idle {
            anyhow::bail!(
                "Cannot add strategy while node is running, add strategies before calling start()"
            );
        }

        // Register external order claims before adding strategy (which moves it)
        let strategy_id = StrategyId::from(strategy.component_id().inner().as_str());
        if let Some(claims) = strategy.external_order_claims() {
            for instrument_id in claims {
                self.exec_manager
                    .claim_external_orders(instrument_id, strategy_id);
            }
            log_info!(
                "Registered external order claims for {}: {:?}",
                strategy_id,
                strategy.external_order_claims(),
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
        T: ExecutionAlgorithm + Component + Debug + 'static,
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

    // Runs up to three reconciliation sub-checks (inflight, open orders,
    // positions), each gated by its own interval. A single recon_timer in
    // the select! loop fires at the minimum enabled interval; this method
    // then checks which sub-checks are actually due.
    //
    // The exec_engine borrow is held across the async venue queries because
    // get_all_clients() returns references into the engine's client map.
    // This is safe: select! runs one branch to completion, so no other
    // branch can borrow the same RefCells concurrently.
    #[expect(clippy::await_holding_refcell_ref)]
    async fn run_reconciliation_checks(
        &mut self,
        inflight_interval_ns: u64,
        open_interval_ns: u64,
        position_interval_ns: u64,
        ts_last_inflight: &mut UnixNanos,
        ts_last_open: &mut UnixNanos,
        ts_last_position: &mut UnixNanos,
    ) -> anyhow::Result<()> {
        let ts_now = self.exec_manager.generate_timestamp_ns();

        if inflight_interval_ns > 0 && (ts_now - *ts_last_inflight).as_u64() >= inflight_interval_ns
        {
            if self.state() == NodeState::ShuttingDown {
                return Ok(());
            }
            let result = self.exec_manager.check_inflight_orders();
            self.process_reconciliation_events(&result.events);
            for cmd in result.queries {
                AsyncRunner::handle_exec_command(cmd);
            }
            *ts_last_inflight = ts_now;
        }

        if open_interval_ns > 0 && (ts_now - *ts_last_open).as_u64() >= open_interval_ns {
            if self.state() == NodeState::ShuttingDown {
                return Ok(());
            }
            let eng_ref = self.kernel.exec_engine.borrow();
            let clients = eng_ref.get_all_clients();
            let events = self.exec_manager.check_open_orders(&clients).await;
            drop(clients);
            drop(eng_ref);
            self.process_reconciliation_events(&events);
            *ts_last_open = ts_now;
        }

        if position_interval_ns > 0 && (ts_now - *ts_last_position).as_u64() >= position_interval_ns
        {
            if self.state() == NodeState::ShuttingDown {
                return Ok(());
            }
            let eng_ref = self.kernel.exec_engine.borrow();
            let clients = eng_ref.get_all_clients();
            let events = self
                .exec_manager
                .check_positions_consistency(&clients)
                .await;
            drop(clients);
            drop(eng_ref);
            self.process_reconciliation_events(&events);
            *ts_last_position = ts_now;
        }

        Ok(())
    }
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
            Some(evt) = data_evt_rx.recv() => {
                pending.data_evts.push(evt);
            }
            Some(cmd) = data_cmd_rx.recv() => {
                pending.data_cmds.push(cmd);
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
    #[cfg(feature = "python")]
    use std::sync::Arc;

    #[cfg(feature = "python")]
    use nautilus_common::runner::{
        SyncDataCommandSender, SyncTradingCommandSender, replace_data_cmd_sender,
        replace_exec_cmd_sender,
    };
    use nautilus_model::identifiers::TraderId;
    use rstest::*;

    use super::*;

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
    fn test_flush_all_pending_drains_all_channel_types() {
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
        use nautilus_core::{UUID4, UnixNanos};
        use nautilus_model::{
            events::OrderSubmitted,
            identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId},
        };

        ExecutionEvent::Order(OrderEventAny::Submitted(OrderSubmitted::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("TEST.VENUE"),
            ClientOrderId::from("O-001"),
            AccountId::from("TEST-001"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        )))
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
        use nautilus_core::{UUID4, UnixNanos};
        use nautilus_model::{
            events::{OrderSubmitted, OrderSubmittedBatch},
            identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId},
        };

        let events = vec![
            OrderSubmitted::new(
                TraderId::from("TESTER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("TEST.VENUE"),
                ClientOrderId::from("O-001"),
                AccountId::from("TEST-001"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
            OrderSubmitted::new(
                TraderId::from("TESTER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("TEST.VENUE"),
                ClientOrderId::from("O-002"),
                AccountId::from("TEST-001"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            ),
        ];

        ExecutionEvent::OrderSubmittedBatch(OrderSubmittedBatch::new(events))
    }

    fn stub_canceled_batch_event() -> ExecutionEvent {
        use nautilus_core::{UUID4, UnixNanos};
        use nautilus_model::{
            events::{OrderCanceled, OrderCanceledBatch},
            identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId},
        };

        let events = vec![
            OrderCanceled::new(
                TraderId::from("TESTER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("TEST.VENUE"),
                ClientOrderId::from("O-001"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
                false,
                None,
                Some(AccountId::from("TEST-001")),
            ),
            OrderCanceled::new(
                TraderId::from("TESTER-001"),
                StrategyId::from("S-001"),
                InstrumentId::from("TEST.VENUE"),
                ClientOrderId::from("O-002"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
                false,
                None,
                Some(AccountId::from("TEST-001")),
            ),
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
}
