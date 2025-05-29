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

//! Central orchestrator for managing actors, strategies, and execution algorithms.
//!
//! The `Trader` component serves as the primary coordination layer between the kernel
//! and individual trading components. It manages component lifecycles, provides
//! unique identification, and coordinates with system engines.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc};

use nautilus_common::{
    clock::{Clock, TestClock},
    component::Component,
    enums::{ComponentState, ComponentTrigger, Environment},
    timer::TimeEvent,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{ComponentId, ExecAlgorithmId, StrategyId, TraderId};

/// Central orchestrator for managing trading components.
///
/// The `Trader` manages the lifecycle and coordination of actors, strategies,
/// and execution algorithms within the trading system. It provides component
/// registration, state management, and integration with system engines.
#[derive(Debug)]
pub struct Trader {
    /// The unique trader identifier.
    pub trader_id: TraderId,
    /// The unique instance identifier.
    pub instance_id: UUID4,
    /// The trading environment context.
    pub environment: Environment,
    /// Component state for lifecycle management.
    state: ComponentState,
    /// System clock for timestamping.
    clock: Rc<RefCell<dyn Clock>>,
    /// Registered actors by component ID.
    actors: HashMap<ComponentId, Box<dyn Component>>, // TODO: TBD
    /// Registered strategies by strategy ID.
    strategies: HashMap<StrategyId, Box<dyn Component>>,
    /// Registered execution algorithms by algorithm ID.
    exec_algorithms: HashMap<ExecAlgorithmId, Box<dyn Component>>, // TODO: TBD
    /// Component clocks for individual components.
    component_clocks: HashMap<ComponentId, Rc<RefCell<dyn Clock>>>, // TODO: TBD
    /// Timestamp when the trader was created.
    ts_created: UnixNanos,
    /// Timestamp when the trader was last started.
    ts_started: Option<UnixNanos>,
    /// Timestamp when the trader was last stopped.
    ts_stopped: Option<UnixNanos>,
}

impl Trader {
    /// Creates a new [`Trader`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        environment: Environment,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        let ts_created = clock.borrow().timestamp_ns();

        Self {
            trader_id,
            instance_id,
            environment,
            state: ComponentState::PreInitialized,
            clock,
            actors: HashMap::new(),
            strategies: HashMap::new(),
            exec_algorithms: HashMap::new(),
            component_clocks: HashMap::new(),
            ts_created,
            ts_started: None,
            ts_stopped: None,
        }
    }

    /// Returns the trader ID.
    #[must_use]
    pub const fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    /// Returns the instance ID.
    #[must_use]
    pub const fn instance_id(&self) -> UUID4 {
        self.instance_id
    }

    /// Returns the trading environment.
    #[must_use]
    pub const fn environment(&self) -> Environment {
        self.environment
    }

    /// Returns the current component state.
    #[must_use]
    pub const fn state(&self) -> ComponentState {
        self.state
    }

    /// Returns the timestamp when the trader was created (UNIX nanoseconds).
    #[must_use]
    pub const fn ts_created(&self) -> UnixNanos {
        self.ts_created
    }

    /// Returns the timestamp when the trader was last started (UNIX nanoseconds).
    #[must_use]
    pub const fn ts_started(&self) -> Option<UnixNanos> {
        self.ts_started
    }

    /// Returns the timestamp when the trader was last stopped (UNIX nanoseconds).
    #[must_use]
    pub const fn ts_stopped(&self) -> Option<UnixNanos> {
        self.ts_stopped
    }

    /// Returns whether the trader is running.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self.state, ComponentState::Running)
    }

    /// Returns whether the trader is stopped.
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        matches!(self.state, ComponentState::Stopped)
    }

    /// Returns whether the trader is disposed.
    #[must_use]
    pub const fn is_disposed(&self) -> bool {
        matches!(self.state, ComponentState::Disposed)
    }

    /// Returns the number of registered actors.
    #[must_use]
    pub fn actor_count(&self) -> usize {
        self.actors.len()
    }

    /// Returns the number of registered strategies.
    #[must_use]
    pub fn strategy_count(&self) -> usize {
        self.strategies.len()
    }

    /// Returns the number of registered execution algorithms.
    #[must_use]
    pub fn exec_algorithm_count(&self) -> usize {
        self.exec_algorithms.len()
    }

    /// Returns the total number of registered components.
    #[must_use]
    pub fn component_count(&self) -> usize {
        self.actors.len() + self.strategies.len() + self.exec_algorithms.len()
    }

    /// Returns a list of all registered actor IDs.
    #[must_use]
    pub fn actor_ids(&self) -> Vec<ComponentId> {
        self.actors.keys().copied().collect()
    }

    /// Returns a list of all registered strategy IDs.
    #[must_use]
    pub fn strategy_ids(&self) -> Vec<StrategyId> {
        self.strategies.keys().copied().collect()
    }

    /// Returns a list of all registered execution algorithm IDs.
    #[must_use]
    pub fn exec_algorithm_ids(&self) -> Vec<ExecAlgorithmId> {
        self.exec_algorithms.keys().copied().collect()
    }

    /// Creates a clock for a component.
    ///
    /// Creates a test clock in backtest environment, otherwise returns a reference
    /// to the system clock.
    fn create_component_clock(&self) -> Rc<RefCell<dyn Clock>> {
        match self.environment {
            Environment::Backtest => {
                // Create individual test clock for component in backtest
                Rc::new(RefCell::new(TestClock::new()))
            }
            Environment::Live | Environment::Sandbox => {
                // Share system clock in live environments
                self.clock.clone()
            }
        }
    }

    /// Adds an actor to the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The trader is not in a valid state for adding components
    /// - An actor with the same ID is already registered
    pub fn add_actor(&mut self, mut actor: Box<dyn Component>) -> anyhow::Result<()> {
        self.validate_component_registration()?;

        let component_id = actor.id();

        // Check for duplicate registration
        if self.actors.contains_key(&component_id) {
            anyhow::bail!("Actor '{component_id}' is already registered");
        }

        let component_clock = self.create_component_clock();
        self.component_clocks
            .insert(component_id, component_clock.clone());

        self.register_component(&mut actor, component_clock)?;

        log::info!("Registered actor '{component_id}'");
        self.actors.insert(component_id, actor);

        Ok(())
    }

    /// Adds a strategy to the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The trader is not in a valid state for adding components
    /// - A strategy with the same ID is already registered
    pub fn add_strategy(&mut self, mut strategy: Box<dyn Component>) -> anyhow::Result<()> {
        self.validate_component_registration()?;

        let strategy_id = StrategyId::from(strategy.id().to_string().as_str());

        // Check for duplicate registration
        if self.strategies.contains_key(&strategy_id) {
            anyhow::bail!("Strategy '{strategy_id}' is already registered");
        }

        let component_clock = self.create_component_clock();
        let component_id = strategy.id();
        self.component_clocks
            .insert(component_id, component_clock.clone());

        self.register_component(&mut strategy, component_clock)?;

        // TODO: Generate unique order ID tag for strategy

        log::info!("Registered strategy '{strategy_id}'");
        self.strategies.insert(strategy_id, strategy);

        Ok(())
    }

    /// Adds an execution algorithm to the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The trader is not in a valid state for adding components
    /// - An execution algorithm with the same ID is already registered
    pub fn add_exec_algorithm(
        &mut self,
        mut exec_algorithm: Box<dyn Component>,
    ) -> anyhow::Result<()> {
        self.validate_component_registration()?;

        let exec_algorithm_id = ExecAlgorithmId::from(exec_algorithm.id().to_string().as_str());

        // Check for duplicate registration
        if self.exec_algorithms.contains_key(&exec_algorithm_id) {
            anyhow::bail!("Execution algorithm '{exec_algorithm_id}' is already registered");
        }

        let component_clock = self.create_component_clock();
        let component_id = exec_algorithm.id();
        self.component_clocks
            .insert(component_id, component_clock.clone());

        self.register_component(&mut exec_algorithm, component_clock)?;

        log::info!("Registered execution algorithm '{exec_algorithm_id}'");
        self.exec_algorithms
            .insert(exec_algorithm_id, exec_algorithm);

        Ok(())
    }

    /// Validates that the trader is in a valid state for component registration.
    fn validate_component_registration(&self) -> anyhow::Result<()> {
        match self.state {
            ComponentState::PreInitialized | ComponentState::Ready | ComponentState::Stopped => {
                Ok(())
            }
            ComponentState::Running => {
                anyhow::bail!("Cannot add components while trader is running")
            }
            ComponentState::Disposed => {
                anyhow::bail!("Cannot add components to disposed trader")
            }
            _ => anyhow::bail!("Cannot add components in current state: {}", self.state),
        }
    }

    /// Registers a component with system resources.
    fn register_component(
        &self,
        component: &mut Box<dyn Component>,
        _component_clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<()> {
        // Register component with system resources
        // Note: In a full implementation, this would register the component
        // with the message bus, cache, portfolio, and engines as needed.
        // For now, we'll just set the component's basic properties.

        log::debug!(
            "Registering component '{}' with system resources",
            component.id()
        );

        // TODO: Complete component registration with:
        // - Message bus subscriptions
        // - Cache integration
        // - Portfolio registration
        // - Engine registration based on component type

        Ok(())
    }

    /// Starts all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to start.
    pub fn start_components(&mut self) -> anyhow::Result<()> {
        log::info!("Starting {} components", self.component_count());

        // Start actors
        for (id, actor) in &mut self.actors {
            log::debug!("Starting actor '{id}'");
            actor.start()?;
        }

        // Start strategies
        for (id, strategy) in &mut self.strategies {
            log::debug!("Starting strategy '{id}'");
            strategy.start()?;
        }

        // Start execution algorithms
        for (id, exec_algorithm) in &mut self.exec_algorithms {
            log::debug!("Starting execution algorithm '{id}'");
            exec_algorithm.start()?;
        }

        log::info!("All components started successfully");
        Ok(())
    }

    /// Stops all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to stop.
    pub fn stop_components(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping {} components", self.component_count());

        // Stop execution algorithms first
        for (id, exec_algorithm) in &mut self.exec_algorithms {
            log::debug!("Stopping execution algorithm '{id}'");
            exec_algorithm.stop()?;
        }

        // Stop strategies
        for (id, strategy) in &mut self.strategies {
            log::debug!("Stopping strategy '{id}'");
            strategy.stop()?;
        }

        // Stop actors last
        for (id, actor) in &mut self.actors {
            log::debug!("Stopping actor '{id}'");
            actor.stop()?;
        }

        log::info!("All components stopped successfully");
        Ok(())
    }

    /// Resets all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to reset.
    pub fn reset_components(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting {} components", self.component_count());

        // Reset all components
        for (id, actor) in &mut self.actors {
            log::debug!("Resetting actor '{id}'");
            actor.reset()?;
        }

        for (id, strategy) in &mut self.strategies {
            log::debug!("Resetting strategy '{id}'");
            strategy.reset()?;
        }

        for (id, exec_algorithm) in &mut self.exec_algorithms {
            log::debug!("Resetting execution algorithm '{id}'");
            exec_algorithm.reset()?;
        }

        log::info!("All components reset successfully");
        Ok(())
    }

    /// Disposes of all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to dispose.
    pub fn dispose_components(&mut self) -> anyhow::Result<()> {
        log::info!("Disposing {} components", self.component_count());

        // Dispose all components
        for (id, actor) in &mut self.actors {
            log::debug!("Disposing actor '{id}'");
            actor.dispose()?;
        }

        for (id, strategy) in &mut self.strategies {
            log::debug!("Disposing strategy '{id}'");
            strategy.dispose()?;
        }

        for (id, exec_algorithm) in &mut self.exec_algorithms {
            log::debug!("Disposing execution algorithm '{id}'");
            exec_algorithm.dispose()?;
        }

        // Clear all collections
        self.actors.clear();
        self.strategies.clear();
        self.exec_algorithms.clear();
        self.component_clocks.clear();

        log::info!("All components disposed successfully");
        Ok(())
    }

    /// Initializes the trader, transitioning from `PreInitialized` to `Ready` state.
    ///
    /// This method must be called before starting the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the trader cannot be initialized from its current state.
    pub fn initialize(&mut self) -> anyhow::Result<()> {
        log::info!("Initializing trader {}", self.trader_id);

        let new_state = self.state.transition(&ComponentTrigger::Initialize)?;
        self.state = new_state;

        log::info!("Trader {} initialized successfully", self.trader_id);
        Ok(())
    }
}

impl Component for Trader {
    fn id(&self) -> ComponentId {
        ComponentId::from(format!("Trader-{}", self.trader_id).as_str())
    }

    fn state(&self) -> ComponentState {
        self.state
    }

    fn trigger(&self) -> ComponentTrigger {
        ComponentTrigger::Initialize
    }

    fn is_running(&self) -> bool {
        self.is_running()
    }

    fn is_stopped(&self) -> bool {
        self.is_stopped()
    }

    fn is_disposed(&self) -> bool {
        self.is_disposed()
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.state == ComponentState::Running {
            log::warn!("Trader is already running");
            return Ok(());
        }

        // Validate that we can start from current state
        if !matches!(self.state, ComponentState::Ready | ComponentState::Stopped) {
            anyhow::bail!("Cannot start trader from {} state", self.state);
        }

        log::info!("Starting trader {}", self.trader_id);

        self.start_components()?;

        // Transition to running state
        self.state = ComponentState::Running;
        self.ts_started = Some(self.clock.borrow().timestamp_ns());

        log::info!("Trader {} started successfully", self.trader_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.is_running() {
            log::warn!("Trader is not running");
            return Ok(());
        }

        log::info!("Stopping trader {}", self.trader_id);

        // Stop all components
        self.stop_components()?;

        self.state = ComponentState::Stopped;
        self.ts_stopped = Some(self.clock.borrow().timestamp_ns());

        log::info!("Trader {} stopped successfully", self.trader_id);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        if self.is_running() {
            anyhow::bail!("Cannot reset trader while running. Stop first.");
        }

        log::info!("Resetting trader {}", self.trader_id);

        // Reset all components
        self.reset_components()?;

        self.state = ComponentState::Ready;
        self.ts_started = None;
        self.ts_stopped = None;

        log::info!("Trader {} reset successfully", self.trader_id);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        if self.is_running() {
            self.stop()?;
        }

        log::info!("Disposing trader {}", self.trader_id);

        // Dispose all components
        self.dispose_components()?;

        self.state = ComponentState::Disposed;

        log::info!("Trader {} disposed successfully", self.trader_id);
        Ok(())
    }

    fn handle_event(&mut self, _event: TimeEvent) {
        // TODO: Implement event handling for the trader
        // This would coordinate timer events with components
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        enums::{ComponentState, ComponentTrigger, Environment},
        msgbus::MessageBus,
        timer::TimeEvent,
    };
    use nautilus_core::UUID4;
    use nautilus_data::engine::{DataEngine, config::DataEngineConfig};
    use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
    use nautilus_model::identifiers::{ComponentId, TraderId};
    use nautilus_portfolio::portfolio::Portfolio;
    use nautilus_risk::engine::{RiskEngine, config::RiskEngineConfig};

    use super::*;

    // Mock component for testing
    #[derive(Debug)]
    struct MockComponent {
        id: ComponentId,
        state: ComponentState,
    }

    impl MockComponent {
        fn new(id: &str) -> Self {
            Self {
                id: ComponentId::from(id),
                state: ComponentState::PreInitialized,
            }
        }
    }

    impl Component for MockComponent {
        fn id(&self) -> ComponentId {
            self.id
        }

        fn state(&self) -> ComponentState {
            self.state
        }

        fn trigger(&self) -> ComponentTrigger {
            ComponentTrigger::Initialize
        }

        fn is_running(&self) -> bool {
            matches!(self.state, ComponentState::Running)
        }

        fn is_stopped(&self) -> bool {
            matches!(self.state, ComponentState::Stopped)
        }

        fn is_disposed(&self) -> bool {
            matches!(self.state, ComponentState::Disposed)
        }

        fn start(&mut self) -> anyhow::Result<()> {
            self.state = ComponentState::Running;
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            self.state = ComponentState::Stopped;
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            self.state = ComponentState::Ready;
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            self.state = ComponentState::Disposed;
            Ok(())
        }

        fn handle_event(&mut self, _event: TimeEvent) {
            // No-op for mock
        }
    }

    fn create_trader_components() -> (
        Rc<RefCell<MessageBus>>,
        Rc<RefCell<Cache>>,
        Rc<RefCell<Portfolio>>,
        Rc<RefCell<DataEngine>>,
        Rc<RefCell<RiskEngine>>,
        Rc<RefCell<ExecutionEngine>>,
        Rc<RefCell<TestClock>>,
    ) {
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        // Set the clock to a non-zero time for test purposes
        clock.borrow_mut().set_time(1_000_000_000u64.into());
        let msgbus = Rc::new(RefCell::new(MessageBus::new(
            trader_id,
            instance_id,
            Some("test".to_string()),
            None,
        )));
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone() as Rc<RefCell<dyn Clock>>,
            None,
        )));
        let data_engine = Rc::new(RefCell::new(DataEngine::new(
            clock.clone(),
            cache.clone(),
            Some(DataEngineConfig::default()),
        )));

        // Create separate cache and clock instances for RiskEngine to avoid borrowing conflicts
        let risk_cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let risk_clock = Rc::new(RefCell::new(TestClock::new()));
        let risk_portfolio = Portfolio::new(
            risk_cache.clone(),
            risk_clock.clone() as Rc<RefCell<dyn Clock>>,
            None,
        );
        let risk_engine = Rc::new(RefCell::new(RiskEngine::new(
            RiskEngineConfig::default(),
            risk_portfolio,
            risk_clock as Rc<RefCell<dyn Clock>>,
            risk_cache,
        )));
        let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(
            clock.clone(),
            cache.clone(),
            Some(ExecutionEngineConfig::default()),
        )));

        (
            msgbus,
            cache,
            portfolio,
            data_engine,
            risk_engine,
            exec_engine,
            clock,
        )
    }

    #[test]
    fn test_trader_creation() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        assert_eq!(trader.trader_id(), trader_id);
        assert_eq!(trader.instance_id(), instance_id);
        assert_eq!(trader.environment(), Environment::Backtest);
        assert_eq!(trader.state(), ComponentState::PreInitialized);
        assert_eq!(trader.actor_count(), 0);
        assert_eq!(trader.strategy_count(), 0);
        assert_eq!(trader.exec_algorithm_count(), 0);
        assert_eq!(trader.component_count(), 0);
        assert!(!trader.is_running());
        assert!(!trader.is_stopped());
        assert!(!trader.is_disposed());
        assert!(trader.ts_created() > 0);
        assert!(trader.ts_started().is_none());
        assert!(trader.ts_stopped().is_none());
    }

    #[test]
    fn test_trader_component_id() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::from("TRADER-001");
        let instance_id = UUID4::new();

        let trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        assert_eq!(trader.id(), ComponentId::from("Trader-TRADER-001"));
    }

    #[test]
    fn test_add_actor_success() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        let actor = Box::new(MockComponent::new("TestActor"));
        let actor_id = actor.id();

        let result = trader.add_actor(actor);
        assert!(result.is_ok());
        assert_eq!(trader.actor_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(trader.actor_ids().contains(&actor_id));
    }

    #[test]
    fn test_add_duplicate_actor_fails() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        let actor1 = Box::new(MockComponent::new("TestActor"));
        let actor2 = Box::new(MockComponent::new("TestActor"));

        // First addition should succeed
        assert!(trader.add_actor(actor1).is_ok());
        assert_eq!(trader.actor_count(), 1);

        // Second addition should fail
        let result = trader.add_actor(actor2);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("already registered")
        );
        assert_eq!(trader.actor_count(), 1);
    }

    #[test]
    fn test_add_strategy_success() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        let strategy = Box::new(MockComponent::new("Test-Strategy"));
        let strategy_id = StrategyId::from(strategy.id().to_string().as_str());

        let result = trader.add_strategy(strategy);
        assert!(result.is_ok());
        assert_eq!(trader.strategy_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(trader.strategy_ids().contains(&strategy_id));
    }

    #[test]
    fn test_add_exec_algorithm_success() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        let exec_algorithm = Box::new(MockComponent::new("TestExecAlgorithm"));
        let exec_algorithm_id = ExecAlgorithmId::from(exec_algorithm.id().to_string().as_str());

        let result = trader.add_exec_algorithm(exec_algorithm);
        assert!(result.is_ok());
        assert_eq!(trader.exec_algorithm_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(trader.exec_algorithm_ids().contains(&exec_algorithm_id));
    }

    #[test]
    fn test_component_lifecycle() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        // Add components
        let actor = Box::new(MockComponent::new("TestActor"));
        let strategy = Box::new(MockComponent::new("Test-Strategy"));
        let exec_algorithm = Box::new(MockComponent::new("TestExecAlgorithm"));

        assert!(trader.add_actor(actor).is_ok());
        assert!(trader.add_strategy(strategy).is_ok());
        assert!(trader.add_exec_algorithm(exec_algorithm).is_ok());
        assert_eq!(trader.component_count(), 3);

        // Test start components
        assert!(trader.start_components().is_ok());

        // Test stop components
        assert!(trader.stop_components().is_ok());

        // Test reset components
        assert!(trader.reset_components().is_ok());

        // Test dispose components
        assert!(trader.dispose_components().is_ok());
        assert_eq!(trader.component_count(), 0);
    }

    #[test]
    fn test_trader_component_lifecycle() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        // Initially pre-initialized
        assert_eq!(trader.state(), ComponentState::PreInitialized);
        assert!(!trader.is_running());
        assert!(!trader.is_stopped());
        assert!(!trader.is_disposed());

        // Cannot start from pre-initialized state
        assert!(trader.start().is_err());

        // Simulate initialization (normally done by kernel)
        trader.state = ComponentState::Ready;

        // Test start
        assert!(trader.start().is_ok());
        assert_eq!(trader.state(), ComponentState::Running);
        assert!(trader.is_running());
        assert!(trader.ts_started().is_some());

        // Test stop
        assert!(trader.stop().is_ok());
        assert_eq!(trader.state(), ComponentState::Stopped);
        assert!(trader.is_stopped());
        assert!(trader.ts_stopped().is_some());

        // Test reset
        assert!(trader.reset().is_ok());
        assert_eq!(trader.state(), ComponentState::Ready);
        assert!(trader.ts_started().is_none());
        assert!(trader.ts_stopped().is_none());

        // Test dispose
        assert!(trader.dispose().is_ok());
        assert_eq!(trader.state(), ComponentState::Disposed);
        assert!(trader.is_disposed());
    }

    #[test]
    fn test_cannot_add_components_while_running() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock);

        // Simulate running state
        trader.state = ComponentState::Running;

        let actor = Box::new(MockComponent::new("TestActor"));
        let result = trader.add_actor(actor);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("while trader is running")
        );
    }

    #[test]
    fn test_create_component_clock_backtest_vs_live() {
        let (msgbus, cache, portfolio, data_engine, risk_engine, exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        // Test backtest environment - should create individual test clocks
        let trader_backtest =
            Trader::new(trader_id, instance_id, Environment::Backtest, clock.clone());

        let backtest_clock = trader_backtest.create_component_clock();
        // In backtest, component clock should be different from system clock
        assert_ne!(
            backtest_clock.as_ptr() as *const _,
            clock.as_ptr() as *const _
        );

        // Test live environment - should share system clock
        let trader_live = Trader::new(trader_id, instance_id, Environment::Live, clock.clone());

        let live_clock = trader_live.create_component_clock();
        // In live, component clock should be same as system clock
        assert_eq!(live_clock.as_ptr() as *const _, clock.as_ptr() as *const _);
    }
}
