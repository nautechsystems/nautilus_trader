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

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use nautilus_common::{
    actor::{Actor, DataActor},
    component::Component,
    enums::Environment,
    messages::{DataEvent, ExecutionEvent, data::DataCommand, execution::TradingCommand},
    timer::TimeEventHandlerV2,
};
use nautilus_core::UUID4;
use nautilus_data::client::DataClientAdapter;
use nautilus_model::identifiers::TraderId;
use nautilus_system::{
    config::NautilusKernelConfig,
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    kernel::NautilusKernel,
};
use nautilus_trading::strategy::Strategy;

use crate::{
    config::LiveNodeConfig,
    runner::{AsyncRunner, AsyncRunnerChannels},
};

/// A thread-safe handle to control a `LiveNode` from other threads.
/// This allows starting, stopping, and querying the node's state
/// without requiring the node itself to be Send + Sync.
#[derive(Clone, Debug)]
pub struct LiveNodeHandle {
    /// Atomic flag indicating if the node should stop.
    pub(crate) stop_flag: Arc<AtomicBool>,
    /// Atomic flag indicating if the node is currently running.
    pub(crate) running_flag: Arc<AtomicBool>,
}

impl Default for LiveNodeHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveNodeHandle {
    /// Creates a new handle with default (stopped) state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            running_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Returns whether the node should stop.
    #[must_use]
    pub fn should_stop(&self) -> bool {
        self.stop_flag.load(Ordering::Relaxed)
    }

    /// Returns whether the node is currently running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running_flag.load(Ordering::Relaxed)
    }

    /// Signals the node to stop.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    /// Marks the node as running (internal use).
    pub(crate) fn set_running(&self, running: bool) {
        self.running_flag.store(running, Ordering::Relaxed);
        if running {
            // Clear stop flag when starting
            self.stop_flag.store(false, Ordering::Relaxed);
        }
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
    is_running: bool,
    shutdown_deadline: Option<tokio::time::Instant>,
    #[cfg(feature = "python")]
    #[allow(dead_code)] // TODO: Under development
    python_actors: Vec<pyo3::Py<pyo3::PyAny>>,
}

impl LiveNode {
    /// Returns a thread-safe handle to control this node.
    #[must_use]
    pub fn handle(&self) -> LiveNodeHandle {
        self.handle.clone()
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

        log::info!("LiveNode built successfully with kernel config");

        Ok(Self {
            kernel,
            runner: Some(runner),
            config,
            handle: LiveNodeHandle::new(),
            is_running: false,
            shutdown_deadline: None,
            #[cfg(feature = "python")]
            python_actors: Vec::new(),
        })
    }

    /// Starts the live node.
    ///
    /// # Errors
    ///
    /// Returns an error if startup fails.
    pub async fn start(&mut self) -> anyhow::Result<()> {
        if self.is_running {
            anyhow::bail!("Already running");
        }

        self.kernel.start_async().await;
        self.kernel.connect_clients().await?;
        self.await_engines_connected().await?;

        self.is_running = true;
        self.handle.set_running(true);

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
        if !self.is_running {
            anyhow::bail!("Not running");
        }

        self.kernel.stop_trader();
        let delay = self.kernel.delay_post_stop();
        log::info!("Awaiting residual events ({delay:?})...");
        tokio::time::sleep(delay).await;
        self.finalize_stop().await
    }

    /// Finalizes the shutdown after the residual events grace period.
    ///
    /// This completes the shutdown sequence by finalizing the kernel shutdown,
    /// disconnecting clients, and updating the node state. Should be called after
    /// the grace period for processing residual events has elapsed.
    async fn finalize_stop(&mut self) -> anyhow::Result<()> {
        self.kernel.finalize_stop().await;
        self.kernel.disconnect_clients().await?;
        self.await_engines_disconnected().await?;

        self.is_running = false;
        self.handle.set_running(false);

        Ok(())
    }

    /// Initiates the shutdown sequence by stopping the trader and setting the grace period deadline.
    fn initiate_shutdown(&mut self) {
        self.kernel.stop_trader();
        let delay = self.kernel.delay_post_stop();
        log::info!("Awaiting residual events ({delay:?})...");
        self.shutdown_deadline = Some(tokio::time::Instant::now() + delay);
    }

    /// Returns whether the node is currently shutting down.
    const fn is_shutting_down(&self) -> bool {
        self.shutdown_deadline.is_some()
    }

    /// Awaits engine clients to connect with timeout.
    async fn await_engines_connected(&self) -> anyhow::Result<()> {
        let start = Instant::now();
        let timeout = self.config.timeout_connection;
        let interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.kernel.check_engines_connected() {
                log::info!("All engine clients connected");
                return Ok(());
            }
            tokio::time::sleep(interval).await;
        }

        anyhow::bail!("Timeout waiting for engine clients to connect after {timeout:?}")
    }

    /// Awaits engine clients to disconnect with timeout.
    async fn await_engines_disconnected(&self) -> anyhow::Result<()> {
        let start = Instant::now();
        let timeout = self.config.timeout_disconnection;
        let interval = Duration::from_millis(100);

        while start.elapsed() < timeout {
            if self.kernel.check_engines_disconnected() {
                log::info!("All engine clients disconnected");
                return Ok(());
            }
            tokio::time::sleep(interval).await;
        }

        anyhow::bail!("Timeout waiting for engine clients to disconnect after {timeout:?}")
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
        if self.runner.is_none() {
            anyhow::bail!("Runner already consumed - run() called twice");
        }

        self.start().await?;

        // SAFETY: We checked is_none() above and start() doesn't consume the runner
        let Some(runner) = self.runner.take() else {
            unreachable!("Runner was verified to exist before start()")
        };

        let AsyncRunnerChannels {
            mut time_evt_rx,
            mut data_evt_rx,
            mut data_cmd_rx,
            mut exec_evt_rx,
            mut exec_cmd_rx,
        } = runner.take_channels();

        log::info!("Event loop starting");

        loop {
            tokio::select! {
                Some(handler) = time_evt_rx.recv() => {
                    AsyncRunner::handle_time_event(handler);
                }
                Some(evt) = data_evt_rx.recv() => {
                    AsyncRunner::handle_data_event(evt);
                }
                Some(cmd) = data_cmd_rx.recv() => {
                    AsyncRunner::handle_data_command(cmd);
                }
                Some(evt) = exec_evt_rx.recv() => {
                    AsyncRunner::handle_exec_event(evt);
                }
                Some(cmd) = exec_cmd_rx.recv() => {
                    AsyncRunner::handle_exec_command(cmd);
                }
                result = tokio::signal::ctrl_c(), if !self.is_shutting_down() => {
                    match result {
                        Ok(()) => log::info!("Received SIGINT, shutting down"),
                        Err(e) => log::error!("Failed to listen for SIGINT: {e}"),
                    }
                    self.initiate_shutdown();
                }
                () = async {
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        if self.handle.should_stop() {
                            log::info!("Received stop signal from handle");
                            return;
                        }
                    }
                }, if !self.is_shutting_down() => {
                    self.initiate_shutdown();
                }
                () = async {
                    match self.shutdown_deadline {
                        Some(deadline) => tokio::time::sleep_until(deadline).await,
                        None => std::future::pending::<()>().await,
                    }
                } => {
                    break;
                }
            }
        }

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

    fn drain_channels(
        &self,
        time_evt_rx: &mut tokio::sync::mpsc::UnboundedReceiver<TimeEventHandlerV2>,
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

    /// Checks if the live node is currently running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        self.is_running
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
        if self.is_running {
            anyhow::bail!(
                "Cannot add actor while node is running. Add actors before calling start()."
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
        if self.is_running {
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
        if self.is_running {
            anyhow::bail!(
                "Cannot add strategy while node is running, add strategies before calling start()"
            );
        }

        self.kernel.trader.add_strategy(strategy)
    }
}

/// Builder for constructing a [`LiveNode`] with a fluent API.
///
/// Provides configuration options specific to live nodes,
/// including client factory registration and timeout settings.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.live", unsendable)
)]
pub struct LiveNodeBuilder {
    name: String,
    config: LiveNodeConfig,
    data_client_factories: HashMap<String, Box<dyn DataClientFactory>>,
    exec_client_factories: HashMap<String, Box<dyn ExecutionClientFactory>>,
    data_client_configs: HashMap<String, Box<dyn ClientConfig>>,
    exec_client_configs: HashMap<String, Box<dyn ClientConfig>>,
}

impl LiveNodeBuilder {
    /// Creates a new [`LiveNodeBuilder`] with required parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if `environment` is invalid (BACKTEST).
    pub fn new(trader_id: TraderId, environment: Environment) -> anyhow::Result<Self> {
        match environment {
            Environment::Sandbox | Environment::Live => {}
            Environment::Backtest => {
                anyhow::bail!("LiveNode cannot be used with Backtest environment");
            }
        }

        let config = LiveNodeConfig {
            environment,
            trader_id,
            ..Default::default()
        };

        Ok(Self {
            name: "LiveNode".to_string(),
            config,
            data_client_factories: HashMap::new(),
            exec_client_factories: HashMap::new(),
            data_client_configs: HashMap::new(),
            exec_client_configs: HashMap::new(),
        })
    }

    /// Returns the name for the node.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Set the name for the node.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the instance ID for the node.
    #[must_use]
    pub const fn with_instance_id(mut self, instance_id: UUID4) -> Self {
        self.config.instance_id = Some(instance_id);
        self
    }

    /// Configure whether to load state on startup.
    #[must_use]
    pub const fn with_load_state(mut self, load_state: bool) -> Self {
        self.config.load_state = load_state;
        self
    }

    /// Configure whether to save state on shutdown.
    #[must_use]
    pub const fn with_save_state(mut self, save_state: bool) -> Self {
        self.config.save_state = save_state;
        self
    }

    /// Set the connection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_connection(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_connection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the reconciliation timeout in seconds.
    #[must_use]
    pub const fn with_timeout_reconciliation(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_reconciliation = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the portfolio initialization timeout in seconds.
    #[must_use]
    pub const fn with_timeout_portfolio(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_portfolio = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the disconnection timeout in seconds.
    #[must_use]
    pub const fn with_timeout_disconnection_secs(mut self, timeout_secs: u64) -> Self {
        self.config.timeout_disconnection = Duration::from_secs(timeout_secs);
        self
    }

    /// Set the post-stop delay in seconds.
    #[must_use]
    pub const fn with_delay_post_stop_secs(mut self, delay_secs: u64) -> Self {
        self.config.delay_post_stop = Duration::from_secs(delay_secs);
        self
    }

    /// Set the shutdown timeout in seconds.
    #[must_use]
    pub const fn with_delay_shutdown_secs(mut self, delay_secs: u64) -> Self {
        self.config.timeout_shutdown = Duration::from_secs(delay_secs);
        self
    }

    /// Adds a data client factory with configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_data_client(
        mut self,
        name: Option<String>,
        factory: Box<dyn DataClientFactory>,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| factory.name().to_string());

        if self.data_client_factories.contains_key(&name) {
            anyhow::bail!("Data client '{name}' is already registered");
        }

        self.data_client_factories.insert(name.clone(), factory);
        self.data_client_configs.insert(name, config);
        Ok(self)
    }

    /// Adds an execution client factory with configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same name is already registered.
    pub fn add_exec_client(
        mut self,
        name: Option<String>,
        factory: Box<dyn ExecutionClientFactory>,
        config: Box<dyn ClientConfig>,
    ) -> anyhow::Result<Self> {
        let name = name.unwrap_or_else(|| factory.name().to_string());

        if self.exec_client_factories.contains_key(&name) {
            anyhow::bail!("Execution client '{name}' is already registered");
        }

        self.exec_client_factories.insert(name.clone(), factory);
        self.exec_client_configs.insert(name, config);
        Ok(self)
    }

    /// Build the [`LiveNode`] with the configured settings.
    ///
    /// This will:
    /// 1. Build the underlying kernel.
    /// 2. Create clients using factories.
    /// 3. Register clients with engines.
    ///
    /// # Errors
    ///
    /// Returns an error if node construction fails.
    pub fn build(mut self) -> anyhow::Result<LiveNode> {
        log::info!(
            "Building LiveNode with {} data clients and {} execution clients",
            self.data_client_factories.len(),
            self.exec_client_factories.len()
        );

        // Create runner first to set up global event channels
        let runner = AsyncRunner::new();
        let kernel = NautilusKernel::new(self.name.clone(), self.config.clone())?;

        // Create and register data clients
        for (name, factory) in self.data_client_factories {
            if let Some(config) = self.data_client_configs.remove(&name) {
                log::info!("Creating data client '{name}'");

                let client =
                    factory.create(&name, config.as_ref(), kernel.cache(), kernel.clock())?;
                let client_id = client.client_id();
                let venue = client.venue();

                let adapter = DataClientAdapter::new(
                    client_id, venue, true, // handles_order_book_deltas
                    true, // handles_order_book_snapshots
                    client,
                );

                kernel
                    .data_engine
                    .borrow_mut()
                    .register_client(adapter, venue);

                log::info!("Registered data client '{name}' ({client_id})");
            } else {
                log::warn!("No config found for data client factory '{name}'");
            }
        }

        // Create and register execution clients
        for (name, factory) in self.exec_client_factories {
            if let Some(config) = self.exec_client_configs.remove(&name) {
                log::info!("Creating execution client '{name}'");

                let client =
                    factory.create(&name, config.as_ref(), kernel.cache(), kernel.clock())?;
                let client_id = client.client_id();

                kernel.exec_engine.borrow_mut().register_client(client)?;

                log::info!("Registered execution client '{name}' ({client_id})");
            } else {
                log::warn!("No config found for execution client factory '{name}'");
            }
        }

        log::info!("Built successfully");

        Ok(LiveNode {
            kernel,
            runner: Some(runner),
            config: self.config,
            handle: LiveNodeHandle::new(),
            is_running: false,
            shutdown_deadline: None,
            #[cfg(feature = "python")]
            python_actors: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::identifiers::TraderId;
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_handle_initial_state() {
        let handle = LiveNodeHandle::new();

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
    fn test_handle_set_running_clears_stop_flag() {
        let handle = LiveNodeHandle::new();
        handle.stop();
        assert!(handle.should_stop());

        handle.set_running(true);

        assert!(!handle.should_stop());
        assert!(handle.is_running());
    }

    #[rstest]
    fn test_handle_clone_shares_state() {
        let handle1 = LiveNodeHandle::new();
        let handle2 = handle1.clone();

        handle1.stop();

        assert!(handle2.should_stop());
    }

    #[rstest]
    fn test_trading_node_builder_creation() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox);

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_trading_node_builder_rejects_backtest() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Backtest);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Backtest environment")
        );
    }

    #[rstest]
    fn test_trading_node_builder_fluent_api() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Live);

        assert!(result.is_ok());
        let _builder = result
            .unwrap()
            .with_name("TestNode")
            .with_timeout_connection(30)
            .with_load_state(false);

        // Should not panic and methods should chain
    }

    #[rstest]
    fn test_builder_rejects_backtest_environment() {
        let result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Backtest);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Backtest environment")
        );
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_trading_node_build() {
        let builder_result = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox);

        assert!(builder_result.is_ok());
        let build_result = builder_result.unwrap().with_name("TestNode").build();

        assert!(build_result.is_ok());
        let node = build_result.unwrap();
        assert!(!node.is_running());
        assert_eq!(node.environment(), Environment::Sandbox);
    }
}
