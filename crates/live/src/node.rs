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
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use nautilus_common::{
    actor::{Actor, DataActor},
    cache::database::CacheDatabaseAdapter,
    component::Component,
    enums::{Environment, LogColor},
    log_info,
    messages::{DataEvent, ExecutionEvent, data::DataCommand, execution::TradingCommand},
    timer::TimeEventHandler,
};
use nautilus_core::UUID4;
use nautilus_model::{
    events::OrderEventAny,
    identifiers::{StrategyId, TraderId},
};
use nautilus_system::{config::NautilusKernelConfig, kernel::NautilusKernel};
use nautilus_trading::strategy::Strategy;
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
pub struct LiveNode {
    kernel: NautilusKernel,
    runner: Option<AsyncRunner>,
    config: LiveNodeConfig,
    handle: LiveNodeHandle,
    exec_manager: ExecutionManager,
    shutdown_deadline: Option<tokio::time::Instant>,
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

        let runner = AsyncRunner::new();
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

    /// Starts the live node.
    ///
    /// # Errors
    ///
    /// Returns an error if startup fails.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.state().is_running() {
            anyhow::bail!("Already running");
        }

        self.handle.set_state(NodeState::Starting);

        self.kernel.start_async().await;
        self.kernel.connect_clients().await;

        if !self.await_engines_connected().await {
            log::error!("Cannot start trader: engine client(s) not connected");
            self.handle.set_state(NodeState::Running);
            return Ok(());
        }

        // Process pending data events before reconciliation and starting trader
        if let Some(runner) = self.runner.as_mut() {
            runner.drain_pending_data_events();
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

        tokio::time::sleep(delay).await;
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

        let start = Instant::now();
        let timeout = self.config.timeout_connection;
        let interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.kernel.check_engines_connected() {
                log::info!("All engine clients connected");
                return true;
            }
            tokio::time::sleep(interval).await;
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

        let start = Instant::now();
        let timeout = self.config.timeout_disconnection;
        let interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.kernel.check_engines_disconnected() {
                log::info!("All engine clients disconnected");
                return;
            }
            tokio::time::sleep(interval).await;
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
    #[allow(clippy::await_holding_refcell_ref)] // Single-threaded runtime, intentional design
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
        let start = Instant::now();
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

                    // SAFETY: Do not hold the Rc across an await point
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

        let stop_handle = self.handle.clone();
        let mut pending = PendingEvents::default();

        // Startup phase: process events while completing startup
        // TODO: Add ctrl_c and stop_handle monitoring here to allow aborting a
        // hanging startup. Currently signals during startup are ignored, and
        // any pending stop_flag is cleared when transitioning to Running.
        let engines_connected = {
            let startup_future = self.complete_startup();
            tokio::pin!(startup_future);

            loop {
                tokio::select! {
                    biased;

                    result = &mut startup_future => {
                        break result?;
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
                        // Account and Report events are safe, order events conflict
                        match evt {
                            ExecutionEvent::Account(_) | ExecutionEvent::Report(_) => {
                                AsyncRunner::handle_exec_event(evt);
                            }
                            ExecutionEvent::Order(order_evt) => {
                                pending.order_evts.push(order_evt);
                            }
                        }
                    }
                    Some(cmd) = exec_cmd_rx.recv() => {
                        pending.exec_cmds.push(cmd);
                    }
                }
            }
        };

        pending.drain();

        if engines_connected {
            // Run reconciliation now that instruments are in cache and start trader
            self.perform_startup_reconciliation().await?;
            self.kernel.start_trader();
        } else {
            log::error!("Not starting trader: engine client(s) not connected");
        }

        self.handle.set_state(NodeState::Running);

        // Running phase: runs until shutdown deadline expires
        let mut residual_events = 0usize;

        loop {
            let shutdown_deadline = self.shutdown_deadline;
            let is_shutting_down = self.state() == NodeState::ShuttingDown;

            tokio::select! {
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
                    AsyncRunner::handle_exec_event(evt);
                }
                Some(cmd) = exec_cmd_rx.recv() => {
                    if is_shutting_down {
                        log::debug!("Residual exec command: {cmd:?}");
                        residual_events += 1;
                    }
                    AsyncRunner::handle_exec_command(cmd);
                }
                result = tokio::signal::ctrl_c(), if self.state() == NodeState::Running => {
                    match result {
                        Ok(()) => log::info!("Received SIGINT, shutting down"),
                        Err(e) => log::error!("Failed to listen for SIGINT: {e}"),
                    }
                    self.initiate_shutdown();
                }
                () = async {
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        if stop_handle.should_stop() {
                            log::info!("Received stop signal from handle");
                            return;
                        }
                    }
                }, if self.state() == NodeState::Running => {
                    self.initiate_shutdown();
                }
                () = async {
                    match shutdown_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                }, if self.state() == NodeState::ShuttingDown => {
                    break;
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

    /// Returns `true` if all engines connected successfully, `false` otherwise.
    /// Note: Does NOT run reconciliation - that happens after pending events are drained.
    async fn complete_startup(&mut self) -> anyhow::Result<bool> {
        self.kernel.connect_clients().await;

        if !self.await_engines_connected().await {
            return Ok(false);
        }

        Ok(true)
    }

    fn initiate_shutdown(&mut self) {
        self.kernel.stop_trader();
        let delay = self.kernel.delay_post_stop();
        log::info!("Awaiting residual events ({delay:?})...");

        self.shutdown_deadline = Some(tokio::time::Instant::now() + delay);
        self.handle.set_state(NodeState::ShuttingDown);
    }

    async fn finalize_stop(&mut self) -> anyhow::Result<()> {
        self.kernel.disconnect_clients().await?;
        self.await_engines_disconnected().await;
        self.kernel.finalize_stop().await;

        self.handle.set_state(NodeState::Stopped);

        Ok(())
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

    /// Gets a reference to the execution manager.
    #[must_use]
    pub const fn exec_manager(&self) -> &ExecutionManager {
        &self.exec_manager
    }

    /// Gets an exclusive reference to the execution manager.
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

        self.kernel.trader.add_actor(actor)
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

        self.kernel.trader.add_actor_from_factory(factory)
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

        self.kernel.trader.add_strategy(strategy)
    }
}

/// Events queued during startup to avoid RefCell borrow conflicts.
///
/// During `connect_clients()`, the data_engine and exec_engine are borrowed
/// across awaits. Processing commands/events that trigger msgbus handlers
/// would try to borrow the same engines, causing a panic.
#[derive(Default)]
struct PendingEvents {
    data_cmds: Vec<DataCommand>,
    data_evts: Vec<DataEvent>,
    exec_cmds: Vec<TradingCommand>,
    order_evts: Vec<OrderEventAny>,
}

impl PendingEvents {
    fn drain(&mut self) {
        let total = self.data_evts.len()
            + self.data_cmds.len()
            + self.exec_cmds.len()
            + self.order_evts.len();

        if total > 0 {
            log::debug!(
                "Processing {total} events/commands queued during startup \
                 (data_evts={}, data_cmds={}, exec_cmds={}, order_evts={})",
                self.data_evts.len(),
                self.data_cmds.len(),
                self.exec_cmds.len(),
                self.order_evts.len()
            );
        }

        for evt in self.data_evts.drain(..) {
            AsyncRunner::handle_data_event(evt);
        }
        for cmd in self.data_cmds.drain(..) {
            AsyncRunner::handle_data_command(cmd);
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
}
