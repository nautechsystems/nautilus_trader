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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::{Cache, CacheConfig, database::CacheDatabaseAdapter},
    clock::{Clock, LiveClock, TestClock},
    enums::Environment,
    logging::{init_logging, init_tracing, logger::LogGuard, writer::FileWriterConfig},
    msgbus::{MessageBus, set_message_bus},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_data::engine::DataEngine;
use nautilus_execution::engine::ExecutionEngine;
use nautilus_model::identifiers::TraderId;
use ustr::Ustr;

use crate::config::NautilusKernelConfig;

/// Core Nautilus system kernel.
///
/// Orchestrates data and execution engines, cache, clock, and messaging across environments.
#[derive(Debug)]
pub struct NautilusKernel {
    /// The kernel name (for logging and identification).
    pub name: Ustr,
    /// The unique instance identifier for this kernel.
    pub instance_id: UUID4,
    /// The machine identifier (hostname or similar).
    pub machine_id: String,
    /// The kernel configuration.
    pub config: NautilusKernelConfig,
    /// The shared in-memory cache.
    pub cache: Rc<RefCell<Cache>>,
    /// The clock driving the kernel.
    pub clock: Rc<RefCell<dyn Clock>>,
    /// Guard for the logging subsystem (keeps logger thread alive).
    pub log_guard: LogGuard,
    /// The data engine instance.
    pub data_engine: DataEngine,
    /// The execution engine instance.
    pub exec_engine: ExecutionEngine,
    /// The UNIX timestamp (nanoseconds) when the kernel was created.
    pub ts_created: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the kernel was last started.
    pub ts_started: Option<UnixNanos>,
    /// The UNIX timestamp (nanoseconds) when the kernel was last shutdown.
    pub ts_shutdown: Option<UnixNanos>,
}

impl NautilusKernel {
    /// Create a new [`NautilusKernel`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the logging subsystem fails to initialize.
    pub fn new(name: Ustr, config: NautilusKernelConfig) -> anyhow::Result<Self> {
        let instance_id = config.instance_id.unwrap_or_default();
        let machine_id = String::new(); // TODO: Implement

        let _ = init_tracing();

        // Initialize logging subsystem
        let file_config = FileWriterConfig::default();
        let log_guard = init_logging(
            config.trader_id,
            instance_id,
            config.logging.clone(),
            file_config,
        )?;

        log::info!("Building system kernel");

        let msgbus = MessageBus::new(
            config.trader_id,
            instance_id,
            Some(name.as_str().to_string()),
            None,
        );
        set_message_bus(Rc::new(RefCell::new(msgbus)));

        let clock = Self::initialize_clock(&config.environment);
        let cache = Self::initialize_cache(config.trader_id, &instance_id, config.cache.clone());

        let data_engine = DataEngine::new(clock.clone(), cache.clone(), config.data_engine.clone());
        let exec_engine =
            ExecutionEngine::new(clock.clone(), cache.clone(), config.exec_engine.clone());

        let ts_created = clock.borrow().timestamp_ns();

        Ok(Self {
            name,
            instance_id,
            machine_id,
            config,
            cache,
            clock,
            log_guard,
            data_engine,
            exec_engine,
            ts_created,
            ts_started: None,
            ts_shutdown: None,
        })
    }

    /// Initialize the shared clock based on the environment.
    ///
    /// Uses a `TestClock` for backtest, or `LiveClock` for live/sandbox.
    fn initialize_clock(environment: &Environment) -> Rc<RefCell<dyn Clock>> {
        match environment {
            Environment::Backtest => {
                let test_clock = TestClock::new();
                Rc::new(RefCell::new(test_clock))
            }
            Environment::Live | Environment::Sandbox => {
                let live_clock = LiveClock::new();
                Rc::new(RefCell::new(live_clock))
            }
        }
    }

    /// Initialize the shared cache.
    ///
    /// Returns an in-memory cache with optional database adapter.
    fn initialize_cache(
        trader_id: TraderId,
        instance_id: &UUID4,
        cache_config: Option<CacheConfig>,
    ) -> Rc<RefCell<Cache>> {
        let cache_config = cache_config.unwrap_or_default();

        // TODO: Placeholder: persistent database adapter can be initialized here (e.g., Redis)
        let cache_database: Option<Box<dyn CacheDatabaseAdapter>> = None;

        let cache = Cache::new(Some(cache_config), cache_database);
        Rc::new(RefCell::new(cache))
    }

    fn generate_timestamp_ns(&self) -> UnixNanos {
        self.clock.borrow().timestamp_ns()
    }

    /// Return the kernel's environment context (Backtest, Sandbox, Live).
    #[must_use]
    pub const fn environment(&self) -> Environment {
        self.config.environment
    }

    /// Return the kernel's name.
    #[must_use]
    pub const fn name(&self) -> Ustr {
        self.name
    }

    /// Return the kernel's trader ID.
    #[must_use]
    pub const fn trader_id(&self) -> TraderId {
        self.config.trader_id
    }

    /// Return the kernel's machine ID.
    #[must_use]
    pub fn machine_id(&self) -> &str {
        &self.machine_id
    }

    /// Return the kernel's instance ID.
    #[must_use]
    pub const fn instance_id(&self) -> UUID4 {
        self.instance_id
    }

    /// Return the UNIX timestamp (ns) when the kernel was created.
    #[must_use]
    pub const fn ts_created(&self) -> UnixNanos {
        self.ts_created
    }

    /// Return the UNIX timestamp (ns) when the kernel was last started.
    #[must_use]
    pub const fn ts_started(&self) -> Option<UnixNanos> {
        self.ts_started
    }

    /// Return the UNIX timestamp (ns) when the kernel was last shutdown.
    #[must_use]
    pub const fn ts_shutdown(&self) -> Option<UnixNanos> {
        self.ts_shutdown
    }

    /// Return whether the kernel has been configured to load state.
    #[must_use]
    pub const fn load_state(&self) -> bool {
        self.config.load_state
    }

    /// Return whether the kernel has been configured to save state.
    #[must_use]
    pub const fn save_state(&self) -> bool {
        self.config.save_state
    }

    /// Return the kernel's clock.
    #[must_use]
    pub fn clock(&self) -> Rc<RefCell<dyn Clock>> {
        self.clock.clone()
    }

    /// Return the kernel's cache.
    #[must_use]
    pub fn cache(&self) -> Rc<RefCell<Cache>> {
        self.cache.clone()
    }

    /// Return the kernel's data engine.
    #[must_use]
    pub const fn data_engine(&self) -> &DataEngine {
        &self.data_engine
    }

    /// Return the kernel's execution engine.
    #[must_use]
    pub const fn exec_engine(&self) -> &ExecutionEngine {
        &self.exec_engine
    }

    /// Start the Nautilus system kernel.
    pub fn start(&self) {
        self.start_engines();
        self.connect_clients();
    }

    /// Stop the Nautilus system kernel.
    pub fn stop(&self) {
        self.stop_clients();
        self.disconnect_clients();
        self.stop_engines();
        self.cancel_timers();
        self.flush_writer();
    }

    /// Dispose of the Nautilus system kernel, releasing resources.
    pub fn dispose(&self) {
        self.stop_engines();

        self.data_engine.dispose();
    }

    /// Cancel all tasks currently running under the kernel.
    ///
    /// Intended for cleanup during shutdown.
    const fn cancel_all_tasks(&self) {
        // TODO: implement task cancellation logic for async contexts
    }

    /// Start all engine components.
    /// Currently only starts the data engine.
    fn start_engines(&self) {
        self.data_engine.start();
    }

    const fn register_executor(&self) {
        // TODO: register executors for actors and strategies when supported
    }

    /// Stop all engine components.
    /// Currently only stops the data engine.
    fn stop_engines(&self) {
        self.data_engine.stop();
    }

    /// Connect engine clients (e.g., data sources).
    fn connect_clients(&self) {
        self.data_engine.connect();
    }

    /// Disconnect all engine clients.
    fn disconnect_clients(&self) {
        self.data_engine.disconnect();
    }

    /// Stop engine clients.
    fn stop_clients(&self) {
        self.data_engine.stop();
    }

    /// Initialize the portfolio (orders & positions).
    const fn initialize_portfolio(&self) {
        // TODO: Placeholder: portfolio initialization to be implemented in next pass
    }

    /// Await engine clients to connect and initialize.
    ///
    /// Blocks until connected or timeout.
    const fn await_engines_connected(&self) {
        // TODO: await engine connections with timeout
    }

    /// Await execution engine state reconciliation.
    ///
    /// Blocks until executions are reconciled or timeout.
    const fn await_execution_reconciliation(&self) {
        // TODO: await execution reconciliation with timeout
    }

    /// Await portfolio initialization (e.g., positions & PnL).
    ///
    /// Blocks until portfolio is initialized or timeout.
    const fn await_portfolio_initialized(&self) {
        // TODO: await portfolio initialization with timeout
    }

    /// Await post-stop trader residual events.
    ///
    /// Allows final cleanup before full shutdown.
    const fn await_trader_residuals(&self) {
        // TODO: await trader residual events after stop
    }

    /// Check if engine clients are connected.
    const fn check_engines_connected(&self) {
        // TODO: check engine connection status
    }

    /// Check if engine clients are disconnected.
    const fn check_engines_disconnected(&self) {
        // TODO: check engine disconnection status
    }

    /// Check if the portfolio has been initialized.
    const fn check_portfolio_initialized(&self) {
        // TODO: check portfolio initialized status
    }

    fn cancel_timers(&self) {
        self.clock.borrow_mut().cancel_timers();
    }

    const fn flush_writer(&self) {
        // TODO: No writer in this kernel version; placeholder for future streaming
    }
}
