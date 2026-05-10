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
    cell::{Cell, Ref, RefCell},
    rc::Rc,
    time::Duration,
};

use nautilus_common::{
    cache::{Cache, CacheConfig, database::CacheDatabaseAdapter},
    clock::{Clock, TestClock},
    component::Component,
    enums::Environment,
    logging::{
        headers, init_logging,
        logger::{LogGuard, LoggerConfig},
        writer::FileWriterConfig,
    },
    messages::system::ShutdownSystem,
    msgbus::{
        self, MessageBus, MessagingSwitchboard, ShareableMessageHandler, get_message_bus,
        set_message_bus,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::DataEngine;
use nautilus_execution::{engine::ExecutionEngine, order_emulator::adapter::OrderEmulatorAdapter};
use nautilus_model::identifiers::{ClientId, TraderId};
use nautilus_portfolio::portfolio::Portfolio;
use nautilus_risk::engine::RiskEngine;
use ustr::Ustr;

use crate::{builder::NautilusKernelBuilder, config::NautilusKernelConfig, trader::Trader};

/// Core Nautilus system kernel.
///
/// Orchestrates data and execution engines, cache, clock, and messaging across environments.
#[derive(Debug)]
pub struct NautilusKernel {
    /// The kernel name (for logging and identification).
    pub name: String,
    /// The unique instance identifier for this kernel.
    pub instance_id: UUID4,
    /// The machine identifier (hostname or similar).
    pub machine_id: String,
    /// The kernel configuration.
    pub config: Box<dyn NautilusKernelConfig>,
    /// The shared in-memory cache.
    pub cache: Rc<RefCell<Cache>>,
    /// The clock driving the kernel.
    pub clock: Rc<RefCell<dyn Clock>>,
    /// The portfolio manager.
    pub portfolio: Rc<RefCell<Portfolio>>,
    /// Guard for the logging subsystem (keeps logger thread alive).
    pub log_guard: LogGuard,
    /// The data engine instance.
    pub data_engine: Rc<RefCell<DataEngine>>,
    /// The risk engine instance.
    pub risk_engine: Rc<RefCell<RiskEngine>>,
    /// The execution engine instance.
    pub exec_engine: Rc<RefCell<ExecutionEngine>>,
    /// The order emulator for handling emulated orders.
    pub order_emulator: OrderEmulatorAdapter,
    /// The trader component (shared for [`Controller`](crate::controller::Controller) access).
    pub trader: Rc<RefCell<Trader>>,
    /// The UNIX timestamp (nanoseconds) when the kernel was created.
    pub ts_created: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the kernel was last started.
    pub ts_started: Option<UnixNanos>,
    /// The UNIX timestamp (nanoseconds) when the kernel was last shutdown.
    pub ts_shutdown: Option<UnixNanos>,
    shutdown_requested: Rc<Cell<bool>>,
}

impl NautilusKernel {
    /// Create a new [`NautilusKernelBuilder`] for fluent configuration.
    #[must_use]
    pub const fn builder(
        name: String,
        trader_id: TraderId,
        environment: Environment,
    ) -> NautilusKernelBuilder {
        NautilusKernelBuilder::new(name, trader_id, environment)
    }

    /// Create a new [`NautilusKernel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the kernel fails to initialize.
    pub fn new<T: NautilusKernelConfig + 'static>(name: String, config: T) -> anyhow::Result<Self> {
        let instance_id = config.instance_id().unwrap_or_default();
        let machine_id = Self::determine_machine_id()?;

        let logger_config = config.logging();
        let log_guard = Self::initialize_logging(config.trader_id(), instance_id, logger_config)?;
        headers::log_header(
            config.trader_id(),
            &machine_id,
            instance_id,
            Ustr::from(&name),
        );

        log::info!("Building system kernel");

        let clock = Self::initialize_clock(&config.environment());
        let cache = Self::initialize_cache(config.cache());

        let msgbus = Rc::new(RefCell::new(MessageBus::new(
            config.trader_id(),
            instance_id,
            Some(name.clone()),
            None,
        )));
        set_message_bus(msgbus);

        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            config.portfolio(),
        )));

        let risk_engine = RiskEngine::new(
            config.risk_engine().unwrap_or_default(),
            portfolio.borrow().clone_shallow(),
            clock.clone(),
            cache.clone(),
        );
        let risk_engine = Rc::new(RefCell::new(risk_engine));

        let exec_engine = ExecutionEngine::new(clock.clone(), cache.clone(), config.exec_engine());
        let exec_engine = Rc::new(RefCell::new(exec_engine));

        let order_emulator =
            OrderEmulatorAdapter::new(config.trader_id(), clock.clone(), cache.clone());

        let data_engine = DataEngine::new(clock.clone(), cache.clone(), config.data_engine());
        let data_engine = Rc::new(RefCell::new(data_engine));

        DataEngine::register_msgbus_handlers(&data_engine);
        RiskEngine::register_msgbus_handlers(&risk_engine);
        ExecutionEngine::register_msgbus_handlers(&exec_engine);

        let shutdown_requested = Rc::new(Cell::new(false));
        Self::register_shutdown_handler(config.trader_id(), shutdown_requested.clone());

        let trader = Rc::new(RefCell::new(Trader::new(
            config.trader_id(),
            instance_id,
            config.environment(),
            clock.clone(),
            cache.clone(),
            portfolio.clone(),
        )));

        let ts_created = clock.borrow().timestamp_ns();

        Ok(Self {
            name,
            instance_id,
            machine_id,
            config: Box::new(config),
            cache,
            clock,
            portfolio,
            log_guard,
            data_engine,
            risk_engine,
            exec_engine,
            order_emulator,
            trader,
            ts_created,
            ts_started: None,
            ts_shutdown: None,
            shutdown_requested,
        })
    }

    fn register_shutdown_handler(trader_id: TraderId, shutdown_requested: Rc<Cell<bool>>) {
        let handler = ShareableMessageHandler::from_typed(move |cmd: &ShutdownSystem| {
            if cmd.trader_id != trader_id {
                log::warn!("Received {cmd} not for this trader {trader_id}, ignoring",);
                return;
            }

            if shutdown_requested.get() {
                log::debug!("Shutdown already requested, ignoring {cmd}");
                return;
            }

            log::info!("Received {cmd}, requesting shutdown");
            shutdown_requested.set(true);
        });
        let topic = MessagingSwitchboard::shutdown_system_topic();
        msgbus::subscribe_any(topic.into(), handler, None);
    }

    fn determine_machine_id() -> anyhow::Result<String> {
        sysinfo::System::host_name().ok_or_else(|| anyhow::anyhow!("Failed to determine hostname"))
    }

    fn initialize_logging(
        trader_id: TraderId,
        instance_id: UUID4,
        config: LoggerConfig,
    ) -> anyhow::Result<LogGuard> {
        #[cfg(feature = "tracing-bridge")]
        let use_tracing = config.use_tracing;

        let log_guard = match init_logging(
            trader_id,
            instance_id,
            config,
            FileWriterConfig::default(), // TODO: Properly incorporate file writer config
        ) {
            Ok(guard) => guard,
            Err(e) => {
                // Only recover from SetLoggerError (logger already registered).
                // This is common in tests where multiple kernels are created and
                // the log crate's global logger persists after LogGuard teardown.
                // Any other error (e.g. thread spawn failure) is propagated.
                if e.downcast_ref::<log::SetLoggerError>().is_some() {
                    if let Some(guard) = LogGuard::new() {
                        guard
                    } else {
                        return Err(e.context(
                            "A non-Nautilus logger is already registered; \
                             cannot initialize Nautilus logging",
                        ));
                    }
                } else {
                    return Err(e);
                }
            }
        };

        // Initialize tracing subscriber if enabled (idempotent)
        #[cfg(feature = "tracing-bridge")]
        if use_tracing && !nautilus_common::logging::bridge::tracing_is_initialized() {
            nautilus_common::logging::bridge::init_tracing()?;
        }

        Ok(log_guard)
    }

    fn initialize_clock(environment: &Environment) -> Rc<RefCell<dyn Clock>> {
        match environment {
            Environment::Backtest => {
                let test_clock = TestClock::new();
                Rc::new(RefCell::new(test_clock))
            }
            #[cfg(feature = "live")]
            Environment::Live | Environment::Sandbox => {
                let live_clock = nautilus_common::live::clock::LiveClock::default(); // nautilus-import-ok
                Rc::new(RefCell::new(live_clock))
            }
            #[cfg(not(feature = "live"))]
            Environment::Live | Environment::Sandbox => {
                panic!(
                    "Live/Sandbox environment requires the 'live' feature to be enabled. \
                     Build with `--features live` or add `features = [\"live\"]` to your dependency."
                );
            }
        }
    }

    fn initialize_cache(cache_config: Option<CacheConfig>) -> Rc<RefCell<Cache>> {
        let cache_config = cache_config.unwrap_or_default();

        // TODO: Placeholder: persistent database adapter can be initialized here (e.g., Redis)
        let cache_database: Option<Box<dyn CacheDatabaseAdapter>> = None;
        let cache = Cache::new(Some(cache_config), cache_database);

        Rc::new(RefCell::new(cache))
    }

    fn cancel_timers(&self) {
        self.clock.borrow_mut().cancel_timers();
    }

    #[must_use]
    pub fn generate_timestamp_ns(&self) -> UnixNanos {
        self.clock.borrow().timestamp_ns()
    }

    /// Returns the kernel's environment context (Backtest, Sandbox, Live).
    #[must_use]
    pub fn environment(&self) -> Environment {
        self.config.environment()
    }

    /// Returns the kernel's name.
    #[must_use]
    pub const fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the kernel's trader ID.
    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        self.config.trader_id()
    }

    /// Returns the kernel's machine ID.
    #[must_use]
    pub fn machine_id(&self) -> &str {
        &self.machine_id
    }

    /// Returns the kernel's instance ID.
    #[must_use]
    pub const fn instance_id(&self) -> UUID4 {
        self.instance_id
    }

    /// Returns the delay after stopping the node to await residual events before final shutdown.
    #[must_use]
    pub fn delay_post_stop(&self) -> Duration {
        self.config.delay_post_stop()
    }

    /// Returns the UNIX timestamp (ns) when the kernel was created.
    #[must_use]
    pub const fn ts_created(&self) -> UnixNanos {
        self.ts_created
    }

    /// Returns the UNIX timestamp (ns) when the kernel was last started.
    #[must_use]
    pub const fn ts_started(&self) -> Option<UnixNanos> {
        self.ts_started
    }

    /// Returns the UNIX timestamp (ns) when the kernel was last shutdown.
    #[must_use]
    pub const fn ts_shutdown(&self) -> Option<UnixNanos> {
        self.ts_shutdown
    }

    /// Returns `true` if a `ShutdownSystem` command has been received.
    #[must_use]
    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested.get()
    }

    /// Clears the shutdown flag.
    ///
    /// Call this before starting a fresh run so a prior `ShutdownSystem`
    /// command does not abort it.
    pub fn reset_shutdown_flag(&self) {
        self.shutdown_requested.set(false);
    }

    /// Returns a shared handle to the shutdown flag for async runtimes
    /// that need to poll it outside the kernel's direct borrow.
    #[must_use]
    pub fn shutdown_flag(&self) -> Rc<Cell<bool>> {
        self.shutdown_requested.clone()
    }

    /// Returns whether the kernel has been configured to load state.
    #[must_use]
    pub fn load_state(&self) -> bool {
        self.config.load_state()
    }

    /// Returns whether the kernel has been configured to save state.
    #[must_use]
    pub fn save_state(&self) -> bool {
        self.config.save_state()
    }

    /// Returns the kernel's clock.
    #[must_use]
    pub fn clock(&self) -> Rc<RefCell<dyn Clock>> {
        self.clock.clone()
    }

    /// Returns the kernel's cache.
    #[must_use]
    pub fn cache(&self) -> Rc<RefCell<Cache>> {
        self.cache.clone()
    }

    /// Returns the kernel's portfolio.
    #[must_use]
    pub fn portfolio(&self) -> Ref<'_, Portfolio> {
        self.portfolio.borrow()
    }

    /// Returns the kernel's data engine.
    #[must_use]
    pub fn data_engine(&self) -> Ref<'_, DataEngine> {
        self.data_engine.borrow()
    }

    /// Returns the kernel's risk engine.
    #[must_use]
    pub const fn risk_engine(&self) -> &Rc<RefCell<RiskEngine>> {
        &self.risk_engine
    }

    /// Returns the kernel's execution engine.
    #[must_use]
    pub const fn exec_engine(&self) -> &Rc<RefCell<ExecutionEngine>> {
        &self.exec_engine
    }

    /// Returns the kernel's trader (shared reference).
    #[must_use]
    pub fn trader(&self) -> &Rc<RefCell<Trader>> {
        &self.trader
    }

    /// Starts the Nautilus system kernel synchronously (for backtest use).
    pub fn start(&mut self) {
        log::info!("Starting");
        self.start_engines();

        log::info!("Initializing trader");
        if let Err(e) = self.trader.borrow_mut().initialize() {
            log::error!("Error initializing trader: {e:?}");
            return;
        }

        log::info!("Starting clients...");

        if let Err(e) = self.start_clients() {
            log::error!("Error starting clients: {e:?}");
        }
        log::info!("Clients started");

        self.ts_started = Some(self.clock.borrow().timestamp_ns());
        log::info!("Started");
    }

    /// Starts the Nautilus system kernel asynchronously.
    pub async fn start_async(&mut self) {
        self.start();
    }

    /// Starts the trader (strategies and actors).
    ///
    /// This should be called after clients are connected and instruments are cached.
    pub fn start_trader(&mut self) {
        log::info!("Starting trader...");
        if let Err(e) = self.trader.borrow_mut().start() {
            log::error!("Error starting trader: {e:?}");
        }
        log::info!("Trader started");
    }

    /// Stops the trader and its registered components.
    ///
    /// This method initiates a graceful shutdown of trading components (strategies, actors)
    /// which may trigger residual events such as order cancellations. The caller should
    /// continue processing events after calling this method to handle these residual events.
    pub fn stop_trader(&mut self) {
        if !self.trader.borrow().is_running() {
            return;
        }

        log::info!("Stopping trader...");

        if let Err(e) = self.trader.borrow_mut().stop() {
            log::error!("Error stopping trader: {e}");
        }
    }

    /// Finalizes the kernel shutdown after the grace period.
    ///
    /// This method should be called after the residual events grace period has elapsed
    /// and all remaining events have been processed. It disconnects clients and stops engines.
    pub async fn finalize_stop(&mut self) {
        // Stop all adapter clients
        if let Err(e) = self.stop_all_clients() {
            log::error!("Error stopping clients: {e:?}");
        }

        self.stop_engines();
        self.cancel_timers();

        self.ts_shutdown = Some(self.clock.borrow().timestamp_ns());
        log::info!("Stopped");
    }

    /// Resets the Nautilus system kernel to its initial state.
    pub fn reset(&mut self) {
        log::info!("Resetting");

        if let Err(e) = self.trader.borrow_mut().reset() {
            log::error!("Error resetting trader: {e:?}");
        }

        self.data_engine.borrow_mut().reset();
        self.exec_engine.borrow_mut().reset();
        self.risk_engine.borrow_mut().reset();

        self.ts_started = None;
        self.ts_shutdown = None;

        log::info!("Reset");
    }

    /// Disposes of the Nautilus system kernel, releasing resources.
    pub fn dispose(&mut self) {
        log::info!("Disposing");

        if let Err(e) = self.trader.borrow_mut().dispose() {
            log::error!("Error disposing trader: {e:?}");
        }

        self.stop_engines();

        self.data_engine.borrow_mut().dispose();
        self.exec_engine.borrow_mut().dispose();
        self.risk_engine.borrow_mut().dispose();
        self.cache.borrow_mut().dispose();
        get_message_bus().borrow_mut().dispose();

        log::info!("Disposed");
    }

    /// Starts all engine components.
    fn start_engines(&self) {
        self.data_engine.borrow_mut().start();
        self.exec_engine.borrow_mut().start();
        self.risk_engine.borrow_mut().start();
    }

    /// Stops all engine components.
    fn stop_engines(&self) {
        self.data_engine.borrow_mut().stop();
        self.exec_engine.borrow_mut().stop();
        self.risk_engine.borrow_mut().stop();
    }

    /// Starts all engine clients.
    ///
    /// Note: Async connection (connect/disconnect) is handled by LiveNode for live clients.
    /// This method only handles synchronous start operations on execution clients.
    fn start_clients(&self) -> Result<(), Vec<anyhow::Error>> {
        let mut errors = Vec::new();

        {
            let mut exec_engine = self.exec_engine.borrow_mut();
            let exec_adapters = exec_engine.get_clients_mut();

            for adapter in exec_adapters {
                if let Err(e) = adapter.start() {
                    log::error!("Error starting execution client {}: {e}", adapter.client_id);
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Stops all engine clients.
    ///
    /// Note: Async disconnection is handled by LiveNode for live clients.
    /// This method only handles synchronous stop operations on execution clients.
    fn stop_all_clients(&self) -> Result<(), Vec<anyhow::Error>> {
        let mut errors = Vec::new();

        {
            let mut exec_engine = self.exec_engine.borrow_mut();
            let exec_adapters = exec_engine.get_clients_mut();

            for adapter in exec_adapters {
                if let Err(e) = adapter.stop() {
                    log::error!("Error stopping execution client {}: {e}", adapter.client_id);
                    errors.push(e);
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Connects data engine clients.
    ///
    /// Data clients are connected first so that instruments are published
    /// and can be drained into the cache before execution clients connect.
    #[expect(clippy::await_holding_refcell_ref)] // Single-threaded runtime, intentional design
    pub async fn connect_data_clients(&mut self) {
        log::info!("Connecting data clients...");
        self.data_engine.borrow_mut().connect().await;
    }

    /// Connects execution engine clients.
    ///
    /// Must be called after data clients are connected and instrument events
    /// have been drained into the cache, so execution clients can load instruments.
    #[expect(clippy::await_holding_refcell_ref)] // Single-threaded runtime, intentional design
    pub async fn connect_exec_clients(&mut self) {
        log::info!("Connecting execution clients...");
        self.exec_engine.borrow_mut().connect().await;
    }

    /// Disconnects all engine clients.
    ///
    /// # Errors
    ///
    /// Returns an error if any client fails to disconnect.
    #[expect(clippy::await_holding_refcell_ref)] // Single-threaded runtime, intentional design
    pub async fn disconnect_clients(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting clients...");
        self.data_engine.borrow_mut().disconnect().await?;
        self.exec_engine.borrow_mut().disconnect().await?;
        Ok(())
    }

    /// Returns `true` if all engine clients are connected.
    #[must_use]
    pub fn check_engines_connected(&self) -> bool {
        self.data_engine.borrow().check_connected() && self.exec_engine.borrow().check_connected()
    }

    /// Returns `true` if all engine clients are disconnected.
    #[must_use]
    pub fn check_engines_disconnected(&self) -> bool {
        self.data_engine.borrow().check_disconnected()
            && self.exec_engine.borrow().check_disconnected()
    }

    /// Returns connection status for all data clients.
    #[must_use]
    pub fn data_client_connection_status(&self) -> Vec<(ClientId, bool)> {
        self.data_engine.borrow().client_connection_status()
    }

    /// Returns connection status for all execution clients.
    #[must_use]
    pub fn exec_client_connection_status(&self) -> Vec<(ClientId, bool)> {
        self.exec_engine.borrow().client_connection_status()
    }
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use nautilus_common::messages::system::ShutdownSystem;
    use nautilus_core::UUID4;
    use rstest::*;
    use ustr::Ustr;

    use super::*;
    use crate::builder::NautilusKernelBuilder;

    #[rstest]
    fn test_shutdown_system_sets_kernel_flag() {
        let kernel = NautilusKernelBuilder::default().build().unwrap();
        assert!(!kernel.is_shutdown_requested());

        let command = ShutdownSystem::new(
            kernel.trader_id(),
            Ustr::from("TestComponent"),
            Some("unit test".to_string()),
            UUID4::new(),
            kernel.generate_timestamp_ns(),
        );

        msgbus::publish_any(
            MessagingSwitchboard::shutdown_system_topic(),
            command.as_any(),
        );
        assert!(kernel.is_shutdown_requested());

        kernel.reset_shutdown_flag();
        assert!(!kernel.is_shutdown_requested());
    }

    #[rstest]
    fn test_shutdown_system_idempotent() {
        let kernel = NautilusKernelBuilder::default().build().unwrap();

        let make_cmd = || {
            ShutdownSystem::new(
                kernel.trader_id(),
                Ustr::from("TestComponent"),
                None,
                UUID4::new(),
                kernel.generate_timestamp_ns(),
            )
        };

        let topic = MessagingSwitchboard::shutdown_system_topic();
        msgbus::publish_any(topic, make_cmd().as_any());
        assert!(kernel.is_shutdown_requested());

        msgbus::publish_any(topic, make_cmd().as_any());
        assert!(kernel.is_shutdown_requested());

        kernel.reset_shutdown_flag();
        assert!(!kernel.is_shutdown_requested());

        msgbus::publish_any(topic, make_cmd().as_any());
        assert!(kernel.is_shutdown_requested());
    }

    #[rstest]
    fn test_shutdown_system_ignores_other_trader() {
        let kernel = NautilusKernelBuilder::default().build().unwrap();

        let command = ShutdownSystem::new(
            TraderId::from("OTHER-TRADER"),
            Ustr::from("TestComponent"),
            None,
            UUID4::new(),
            kernel.generate_timestamp_ns(),
        );

        msgbus::publish_any(
            MessagingSwitchboard::shutdown_system_topic(),
            command.as_any(),
        );
        assert!(!kernel.is_shutdown_requested());
    }
}
