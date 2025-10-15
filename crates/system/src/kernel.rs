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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    any::Any,
    cell::{Ref, RefCell},
    rc::Rc,
};

use futures::future::join_all;
use nautilus_common::{
    cache::{Cache, CacheConfig, database::CacheDatabaseAdapter},
    clock::{Clock, LiveClock, TestClock},
    component::Component,
    enums::Environment,
    logging::{
        headers, init_logging, init_tracing,
        logger::{LogGuard, LoggerConfig},
        writer::FileWriterConfig,
    },
    messages::{DataResponse, data::DataCommand},
    msgbus::{
        self, MessageBus, get_message_bus,
        handler::{ShareableMessageHandler, TypedMessageHandler},
        set_message_bus,
        switchboard::MessagingSwitchboard,
    },
    runner::get_data_cmd_sender,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::DataEngine;
use nautilus_execution::engine::ExecutionEngine;
use nautilus_model::identifiers::TraderId;
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
    pub portfolio: Portfolio,
    /// Guard for the logging subsystem (keeps logger thread alive).
    pub log_guard: LogGuard,
    /// The data engine instance.
    pub data_engine: Rc<RefCell<DataEngine>>,
    /// The risk engine instance.
    pub risk_engine: RiskEngine,
    /// The execution engine instance.
    pub exec_engine: ExecutionEngine,
    /// The trader component.
    pub trader: Trader,
    /// The UNIX timestamp (nanoseconds) when the kernel was created.
    pub ts_created: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the kernel was last started.
    pub ts_started: Option<UnixNanos>,
    /// The UNIX timestamp (nanoseconds) when the kernel was last shutdown.
    pub ts_shutdown: Option<UnixNanos>,
}

impl NautilusKernel {
    /// Create a new [`NautilusKernelBuilder`] for fluent configuration.
    #[must_use]
    pub const fn builder(
        name: String,
        trader_id: TraderId,
        environment: nautilus_common::enums::Environment,
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
            Ustr::from(stringify!(LiveNode)),
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

        let portfolio = Portfolio::new(cache.clone(), clock.clone(), config.portfolio());
        let risk_engine = RiskEngine::new(
            config.risk_engine().unwrap_or_default(),
            Portfolio::new(cache.clone(), clock.clone(), config.portfolio()),
            clock.clone(),
            cache.clone(),
        );
        let exec_engine = ExecutionEngine::new(clock.clone(), cache.clone(), config.exec_engine());

        let data_engine = DataEngine::new(clock.clone(), cache.clone(), config.data_engine());
        let data_engine = Rc::new(RefCell::new(data_engine));

        // Register DataEngine command execution
        use nautilus_core::WeakCell;

        let data_engine_weak = WeakCell::from(Rc::downgrade(&data_engine));
        let data_engine_weak_clone1 = data_engine_weak.clone();
        let endpoint = MessagingSwitchboard::data_engine_execute();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |cmd: &DataCommand| {
                if let Some(engine_rc) = data_engine_weak_clone1.upgrade() {
                    engine_rc.borrow_mut().execute(cmd);
                }
            },
        )));
        msgbus::register(endpoint, handler);

        // Register DataEngine command queueing
        let endpoint = MessagingSwitchboard::data_engine_queue_execute();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |cmd: &DataCommand| {
                get_data_cmd_sender().clone().execute(cmd.clone());
            },
        )));
        msgbus::register(endpoint, handler);

        // Register DataEngine process handler
        let endpoint = MessagingSwitchboard::data_engine_process();
        let data_engine_weak2 = data_engine_weak.clone();
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::with_any(
            move |data: &dyn Any| {
                if let Some(engine_rc) = data_engine_weak2.upgrade() {
                    engine_rc.borrow_mut().process(data);
                }
            },
        )));
        msgbus::register(endpoint, handler);

        // Register DataEngine response handler
        let endpoint = MessagingSwitchboard::data_engine_response();
        let data_engine_weak3 = data_engine_weak;
        let handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
            move |resp: &DataResponse| {
                if let Some(engine_rc) = data_engine_weak3.upgrade() {
                    engine_rc.borrow_mut().response(resp.clone());
                }
            },
        )));
        msgbus::register(endpoint, handler);

        let trader = Trader::new(
            config.trader_id(),
            instance_id,
            config.environment(),
            clock.clone(),
            cache.clone(),
        );

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
            trader,
            ts_created,
            ts_started: None,
            ts_shutdown: None,
        })
    }

    fn determine_machine_id() -> anyhow::Result<String> {
        Ok(hostname::get()?.to_string_lossy().into_owned())
    }

    fn initialize_logging(
        trader_id: TraderId,
        instance_id: UUID4,
        config: LoggerConfig,
    ) -> anyhow::Result<LogGuard> {
        init_tracing()?;

        let log_guard = init_logging(
            trader_id,
            instance_id,
            config,
            FileWriterConfig::default(), // TODO: Properly incorporate file writer config
        )?;

        Ok(log_guard)
    }

    fn initialize_clock(environment: &Environment) -> Rc<RefCell<dyn Clock>> {
        match environment {
            Environment::Backtest => {
                let test_clock = TestClock::new();
                Rc::new(RefCell::new(test_clock))
            }
            Environment::Live | Environment::Sandbox => {
                let live_clock = LiveClock::default();
                Rc::new(RefCell::new(live_clock))
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

    /// Returns the kernel's message bus.  // TODO: TBD if this is necessary
    #[must_use]
    pub fn msgbus(&self) -> Rc<RefCell<MessageBus>> {
        get_message_bus()
    }

    /// Returns the kernel's portfolio.
    #[must_use]
    pub const fn portfolio(&self) -> &Portfolio {
        &self.portfolio
    }

    /// Returns the kernel's data engine.
    #[must_use]
    pub fn data_engine(&self) -> Ref<'_, DataEngine> {
        self.data_engine.borrow()
    }

    /// Returns the kernel's risk engine.
    #[must_use]
    pub const fn risk_engine(&self) -> &RiskEngine {
        &self.risk_engine
    }

    /// Returns the kernel's execution engine.
    #[must_use]
    pub const fn exec_engine(&self) -> &ExecutionEngine {
        &self.exec_engine
    }

    /// Returns the kernel's trader.
    #[must_use]
    pub const fn trader(&self) -> &Trader {
        &self.trader
    }

    /// Starts the Nautilus system kernel.
    pub async fn start_async(&mut self) {
        log::info!("Starting");
        self.start_engines();

        log::info!("Initializing trader");
        if let Err(e) = self.trader.initialize() {
            log::error!("Error initializing trader: {e:?}");
            return;
        }

        log::info!("Connecting clients...");
        if let Err(e) = self.connect_clients().await {
            log::error!("Error connecting clients: {e:?}");
        }
        log::info!("Clients connected");

        if let Err(e) = self.trader.start() {
            log::error!("Error starting trader: {e:?}");
        }

        self.ts_started = Some(self.clock.borrow().timestamp_ns());
        log::info!("Started");
    }

    /// Stops the Nautilus system kernel.
    pub async fn stop_async(&mut self) {
        log::info!("Stopping");

        // Stop the trader (it will stop all registered components)
        if let Err(e) = self.trader.stop() {
            log::error!("Error stopping trader: {e:?}");
        }

        // Disconnect all adapter clients
        if let Err(e) = self.disconnect_clients().await {
            log::error!("Error disconnecting clients: {e:?}");
        }

        self.stop_engines();
        self.cancel_timers();

        self.ts_shutdown = Some(self.clock.borrow().timestamp_ns());
        log::info!("Stopped");
    }

    /// Resets the Nautilus system kernel to its initial state.
    pub fn reset(&mut self) {
        log::info!("Resetting");

        if let Err(e) = self.trader.reset() {
            log::error!("Error resetting trader: {e:?}");
        }

        // Reset engines
        self.data_engine.borrow_mut().reset();
        // TODO: Reset other engines when reset methods are available

        self.ts_started = None;
        self.ts_shutdown = None;

        log::info!("Reset");
    }

    /// Disposes of the Nautilus system kernel, releasing resources.
    pub fn dispose(&mut self) {
        log::info!("Disposing");

        if let Err(e) = self.trader.dispose() {
            log::error!("Error disposing trader: {e:?}");
        }

        self.stop_engines();

        self.data_engine.borrow_mut().dispose();
        // TODO: Implement dispose methods for other engines

        log::info!("Disposed");
    }

    /// Cancels all tasks currently running under the kernel.
    ///
    /// Intended for cleanup during shutdown.
    const fn cancel_all_tasks(&self) {
        // TODO: implement task cancellation logic for async contexts
    }

    /// Starts all engine components.
    fn start_engines(&self) {
        self.data_engine.borrow_mut().start();
        // TODO: Start other engines when methods are available
    }

    /// Stops all engine components.
    fn stop_engines(&self) {
        self.data_engine.borrow_mut().stop();
        // TODO: Stop other engines when methods are available
    }

    /// Connects all engine clients.
    #[allow(clippy::await_holding_refcell_ref)]
    async fn connect_clients(&mut self) -> Result<(), Vec<anyhow::Error>> {
        let mut data_engine = self.data_engine.borrow_mut();
        let mut data_adapters = data_engine.get_clients_mut();
        let mut futures = Vec::with_capacity(data_adapters.len());

        for adapter in &mut data_adapters {
            futures.push(adapter.connect());
        }

        let results = join_all(futures).await;
        let errors: Vec<anyhow::Error> = results.into_iter().filter_map(Result::err).collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Disconnects all engine clients.
    #[allow(clippy::await_holding_refcell_ref)]
    async fn disconnect_clients(&mut self) -> Result<(), Vec<anyhow::Error>> {
        let mut data_engine = self.data_engine.borrow_mut();
        let mut data_adapters = data_engine.get_clients_mut();
        let mut futures = Vec::with_capacity(data_adapters.len());

        for adapter in &mut data_adapters {
            futures.push(adapter.disconnect());
        }

        let results = join_all(futures).await;
        let errors: Vec<anyhow::Error> = results.into_iter().filter_map(Result::err).collect();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Stops engine clients.
    fn stop_clients(&self) {
        self.data_engine.borrow_mut().stop();
    }

    /// Initializes the portfolio (orders & positions).
    const fn initialize_portfolio(&self) {
        // TODO: Placeholder: portfolio initialization to be implemented in next pass
    }

    /// Awaits engine clients to connect and initialize.
    ///
    /// Blocks until connected or timeout.
    const fn await_engines_connected(&self) {
        // TODO: await engine connections with timeout
    }

    /// Awaits execution engine state reconciliation.
    ///
    /// Blocks until executions are reconciled or timeout.
    const fn await_execution_reconciliation(&self) {
        // TODO: await execution reconciliation with timeout
    }

    /// Awaits portfolio initialization.
    ///
    /// Blocks until portfolio is initialized or timeout.
    const fn await_portfolio_initialized(&self) {
        // TODO: await portfolio initialization with timeout
    }

    /// Awaits post-stop trader residual events.
    ///
    /// Allows final cleanup before full shutdown.
    const fn await_trader_residuals(&self) {
        // TODO: await trader residual events after stop
    }

    /// Checks if engine clients are connected.
    const fn check_engines_connected(&self) {
        // TODO: check engine connection status
    }

    /// Checks if engine clients are disconnected.
    const fn check_engines_disconnected(&self) {
        // TODO: check engine disconnection status
    }

    /// Checks if the portfolio has been initialized.
    const fn check_portfolio_initialized(&self) {
        // TODO: check portfolio initialized status
    }

    /// Flushes the stream writer.
    const fn flush_writer(&self) {
        // TODO: No writer in this kernel version; placeholder for future streaming
    }
}
