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

use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc};

use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    clock::{Clock, TestClock},
    component::{
        Component, dispose_component, register_component_actor, reset_component, start_component,
        stop_component,
    },
    enums::{ComponentState, ComponentTrigger, Environment},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{ActorId, ComponentId, ExecAlgorithmId, StrategyId, TraderId};

/// Central orchestrator for managing trading components.
///
/// The `Trader` manages the lifecycle and coordination of actors, strategies,
/// and execution algorithms within the trading system. It provides component
/// registration, state management, and integration with system engines.
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
    /// System cache for data storage.
    cache: Rc<RefCell<Cache>>,
    /// Registered actor IDs (actors stored in global registry).
    actor_ids: Vec<ActorId>,
    /// Registered strategies by strategy ID.
    strategies: HashMap<StrategyId, Box<dyn Component>>,
    /// Registered execution algorithms by algorithm ID.
    exec_algorithms: HashMap<ExecAlgorithmId, Box<dyn Component>>,
    /// Component clocks for individual components.
    clocks: HashMap<ComponentId, Rc<RefCell<dyn Clock>>>, // TODO: TBD global clock?
    /// Timestamp when the trader was created.
    ts_created: UnixNanos,
    /// Timestamp when the trader was last started.
    ts_started: Option<UnixNanos>,
    /// Timestamp when the trader was last stopped.
    ts_stopped: Option<UnixNanos>,
}

impl Debug for Trader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", stringify!(TraderId)) // TODO
    }
}

impl Trader {
    /// Creates a new [`Trader`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        instance_id: UUID4,
        environment: Environment,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        let ts_created = clock.borrow().timestamp_ns();

        Self {
            trader_id,
            instance_id,
            environment,
            state: ComponentState::PreInitialized,
            clock,
            cache,
            actor_ids: Vec::new(),
            strategies: HashMap::new(),
            exec_algorithms: HashMap::new(),
            clocks: HashMap::new(),
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

    /// Returns the number of registered actors.
    #[must_use]
    pub const fn actor_count(&self) -> usize {
        self.actor_ids.len()
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
        self.actor_ids.len() + self.strategies.len() + self.exec_algorithms.len()
    }

    /// Returns a list of all registered actor IDs.
    #[must_use]
    pub fn actor_ids(&self) -> Vec<ActorId> {
        self.actor_ids.clone()
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
    /// - The trader is not in a valid state for adding components.
    /// - An actor with the same ID is already registered.
    pub fn add_actor<T>(&mut self, actor: T) -> anyhow::Result<()>
    where
        T: DataActor + Component + Debug + 'static,
    {
        self.validate_component_registration()?;

        let actor_id = actor.actor_id();

        // Check for duplicate registration
        if self.actor_ids.contains(&actor_id) {
            anyhow::bail!("Actor '{actor_id}' is already registered");
        }

        let clock = self.create_component_clock();
        let component_id = ComponentId::new(actor_id.inner().as_str());
        self.clocks.insert(component_id, clock.clone());

        let mut actor_mut = actor;
        actor_mut.register(self.trader_id, clock, self.cache.clone())?;

        self.add_registered_actor(actor_mut)
    }

    /// Adds an actor to the trader using a factory function.
    ///
    /// The factory function is called at registration time to create the actor,
    /// avoiding cloning issues with non-cloneable actor types.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The factory function fails to create the actor.
    /// - The trader is not in a valid state for adding components.
    /// - An actor with the same ID is already registered.
    pub fn add_actor_from_factory<F, T>(&mut self, factory: F) -> anyhow::Result<()>
    where
        F: FnOnce() -> anyhow::Result<T>,
        T: DataActor + Component + Debug + 'static,
    {
        let actor = factory()?;

        self.add_actor(actor)
    }

    /// Adds an already registered actor to the trader's component registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor cannot be registered in the component registry.
    pub fn add_registered_actor<T>(&mut self, actor: T) -> anyhow::Result<()>
    where
        T: DataActor + Component + Debug + 'static,
    {
        let actor_id = actor.actor_id();
        let mem_addr = actor.mem_address();

        // Register in both component and actor registries (this consumes the actor)
        register_component_actor(actor);

        // Store actor ID for lifecycle management
        self.actor_ids.push(actor_id);

        log::info!(
            "Registered '{actor_id}' at mem_addr {mem_addr} with trader {}",
            self.trader_id
        );

        Ok(())
    }

    /// Adds an actor ID to the trader's lifecycle management without consuming the actor.
    ///
    /// This is useful when the actor is already registered in the global component registry
    /// but the trader needs to track it for lifecycle management. The caller is responsible
    /// for ensuring the actor is properly registered in the global registries.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor ID is already tracked by this trader.
    pub fn add_actor_id_for_lifecycle(&mut self, actor_id: ActorId) -> anyhow::Result<()> {
        // Check for duplicate registration
        if self.actor_ids.contains(&actor_id) {
            anyhow::bail!("Actor '{actor_id}' is already tracked by trader");
        }

        // Store actor ID for lifecycle management
        self.actor_ids.push(actor_id);

        log::debug!(
            "Added actor ID '{actor_id}' to trader {} for lifecycle management",
            self.trader_id
        );

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

        let strategy_id = StrategyId::from(strategy.component_id().inner().as_str());

        // Check for duplicate registration
        if self.strategies.contains_key(&strategy_id) {
            anyhow::bail!("Strategy '{strategy_id}' is already registered");
        }

        let clock = self.create_component_clock();
        let component_id = strategy.component_id();
        self.clocks.insert(component_id, clock.clone());

        strategy.register(self.trader_id, clock, self.cache.clone())?;

        self.strategies.insert(strategy_id, strategy);
        log::info!(
            "Registered strategy '{strategy_id}' with trader {}",
            self.trader_id
        );

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

        let exec_algorithm_id =
            ExecAlgorithmId::from(exec_algorithm.component_id().inner().as_str());

        // Check for duplicate registration
        if self.exec_algorithms.contains_key(&exec_algorithm_id) {
            anyhow::bail!("Execution algorithm '{exec_algorithm_id}' is already registered");
        }

        let clock = self.create_component_clock();
        let component_id = exec_algorithm.component_id();
        self.clocks.insert(component_id, clock.clone());

        exec_algorithm.register(self.trader_id, clock, self.cache.clone())?;

        self.exec_algorithms
            .insert(exec_algorithm_id, exec_algorithm);
        log::info!(
            "Registered execution algorithm '{exec_algorithm_id}' with trader {}",
            self.trader_id
        );

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

    /// Starts all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to start.
    pub fn start_components(&mut self) -> anyhow::Result<()> {
        // Start actors (retrieved from global registry)
        for actor_id in &self.actor_ids {
            log::debug!("Starting actor '{actor_id}'");
            start_component(&actor_id.inner())?;
        }

        for strategy_id in &mut self.strategies.keys() {
            log::debug!("Starting strategy '{strategy_id}'");
            // strategy.start()?; // TODO: TBD
        }

        for exec_algorithm_id in &mut self.exec_algorithms.keys() {
            log::debug!("Starting execution algorithm '{exec_algorithm_id}'");
            // exec_algorithm.start()?;  // TODO: TBD
        }

        Ok(())
    }

    /// Stops all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to stop.
    pub fn stop_components(&mut self) -> anyhow::Result<()> {
        for actor_id in &self.actor_ids {
            log::debug!("Stopping actor '{actor_id}'");
            stop_component(&actor_id.inner())?;
        }

        for exec_algorithm_id in &mut self.exec_algorithms.keys() {
            log::debug!("Stopping execution algorithm '{exec_algorithm_id}'");
            // exec_algorithm.stop()?;  // TODO: TBD
        }

        for strategy_id in &mut self.strategies.keys() {
            log::debug!("Stopping strategy '{strategy_id}'");
            // strategy.stop()?;  // TODO: TBD
        }

        Ok(())
    }

    /// Resets all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to reset.
    pub fn reset_components(&mut self) -> anyhow::Result<()> {
        // Reset actors (retrieved from global registry)
        for actor_id in &self.actor_ids {
            log::debug!("Resetting actor '{actor_id}'");
            reset_component(&actor_id.inner())?;
        }

        for strategy_id in &mut self.strategies.keys() {
            log::debug!("Resetting strategy '{strategy_id}'");
            // strategy.reset()?;  // TODO: TBD
        }

        for exec_algorithm_id in &mut self.exec_algorithms.keys() {
            log::debug!("Resetting execution algorithm '{exec_algorithm_id}'");
            // exec_algorithm.reset()?;  // TODO: TBD
        }

        Ok(())
    }

    /// Disposes of all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to dispose.
    pub fn dispose_components(&mut self) -> anyhow::Result<()> {
        // Dispose actors (retrieved from global registry)
        for actor_id in &self.actor_ids {
            log::debug!("Disposing actor '{actor_id}'");
            dispose_component(&actor_id.inner())?;
        }

        for strategy_id in &mut self.strategies.keys() {
            log::debug!("Disposing strategy '{strategy_id}'");
            // strategy.dispose()?;  // TODO: TBD
        }

        for exec_algorithm_id in &mut self.exec_algorithms.keys() {
            log::debug!("Disposing execution algorithm '{exec_algorithm_id}'");
            // exec_algorithm.dispose()?;  // TODO: TBD
        }

        self.actor_ids.clear();
        self.strategies.clear();
        self.exec_algorithms.clear();
        self.clocks.clear();

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
        let new_state = self.state.transition(&ComponentTrigger::Initialize)?;
        self.state = new_state;

        Ok(())
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        self.start_components()?;

        // Transition to running state
        self.ts_started = Some(self.clock.borrow().timestamp_ns());

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.stop_components()?;

        self.ts_stopped = Some(self.clock.borrow().timestamp_ns());

        Ok(())
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.reset_components()?;

        self.ts_started = None;
        self.ts_stopped = None;

        Ok(())
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        if self.is_running() {
            self.stop()?;
        }

        self.dispose_components()?;

        Ok(())
    }
}

impl Component for Trader {
    fn component_id(&self) -> ComponentId {
        ComponentId::new(format!("Trader-{}", self.trader_id))
    }

    fn state(&self) -> ComponentState {
        self.state
    }

    fn transition_state(&mut self, trigger: ComponentTrigger) -> anyhow::Result<()> {
        self.state = self.state.transition(&trigger)?;
        log::info!("{}", self.state.variant_name());
        Ok(())
    }

    fn register(
        &mut self,
        _trader_id: TraderId,
        _clock: Rc<RefCell<dyn Clock>>,
        _cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("Trader cannot register with itself")
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        Self::on_start(self)
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        Self::on_stop(self)
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        Self::on_reset(self)
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Self::on_dispose(self)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        ops::{Deref, DerefMut},
        rc::Rc,
    };

    use nautilus_common::{
        actor::{DataActorCore, data_actor::DataActorConfig},
        cache::Cache,
        clock::TestClock,
        enums::{ComponentState, Environment},
        msgbus::MessageBus,
    };
    use nautilus_core::UUID4;
    use nautilus_data::engine::{DataEngine, config::DataEngineConfig};
    use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
    use nautilus_model::identifiers::{ActorId, ComponentId, TraderId};
    use nautilus_portfolio::portfolio::Portfolio;
    use nautilus_risk::engine::{RiskEngine, config::RiskEngineConfig};
    use rstest::rstest;

    use super::*;

    // Simple DataActor wrapper for testing
    #[derive(Debug)]
    struct TestDataActor {
        core: DataActorCore,
    }

    impl TestDataActor {
        fn new(config: DataActorConfig) -> Self {
            Self {
                core: DataActorCore::new(config),
            }
        }
    }

    impl DataActor for TestDataActor {}

    impl Deref for TestDataActor {
        type Target = DataActorCore;
        fn deref(&self) -> &Self::Target {
            &self.core
        }
    }

    impl DerefMut for TestDataActor {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.core
        }
    }

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
        fn component_id(&self) -> ComponentId {
            self.id
        }

        fn state(&self) -> ComponentState {
            self.state
        }

        fn transition_state(&mut self, trigger: ComponentTrigger) -> anyhow::Result<()> {
            self.state = self.state.transition(&trigger)?;
            log::info!("{}", self.state.variant_name());
            Ok(())
        }

        fn register(
            &mut self,
            _trader_id: TraderId,
            _clock: Rc<RefCell<dyn Clock>>,
            _cache: Rc<RefCell<Cache>>,
        ) -> anyhow::Result<()> {
            // Mock implementation
            Ok(())
        }

        fn on_start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[allow(clippy::type_complexity)]
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

    #[rstest]
    fn test_trader_creation() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

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

    #[rstest]
    fn test_trader_component_id() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::from("TRADER-001");
        let instance_id = UUID4::new();

        let trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        assert_eq!(
            trader.component_id(),
            ComponentId::from("Trader-TRADER-001")
        );
    }

    #[rstest]
    fn test_add_actor_success() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        let actor = TestDataActor::new(DataActorConfig::default());
        let actor_id = actor.actor_id();

        let result = trader.add_actor(actor);
        assert!(result.is_ok());
        assert_eq!(trader.actor_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(trader.actor_ids().contains(&actor_id));
    }

    #[rstest]
    fn test_add_duplicate_actor_fails() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        let config = DataActorConfig {
            actor_id: Some(ActorId::from("TestActor")),
            ..Default::default()
        };
        let actor1 = TestDataActor::new(config.clone());
        let actor2 = TestDataActor::new(config);

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

    #[rstest]
    fn test_add_strategy_success() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        let strategy = Box::new(MockComponent::new("Test-Strategy"));
        let strategy_id = StrategyId::from(strategy.component_id().inner().as_str());

        let result = trader.add_strategy(strategy);
        assert!(result.is_ok());
        assert_eq!(trader.strategy_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(trader.strategy_ids().contains(&strategy_id));
    }

    #[rstest]
    fn test_add_exec_algorithm_success() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        let exec_algorithm = Box::new(MockComponent::new("TestExecAlgorithm"));
        let exec_algorithm_id =
            ExecAlgorithmId::from(exec_algorithm.component_id().inner().as_str());

        let result = trader.add_exec_algorithm(exec_algorithm);
        assert!(result.is_ok());
        assert_eq!(trader.exec_algorithm_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(trader.exec_algorithm_ids().contains(&exec_algorithm_id));
    }

    #[rstest]
    fn test_component_lifecycle() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        // Add components
        let actor = TestDataActor::new(DataActorConfig::default());
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

    #[rstest]
    fn test_trader_component_lifecycle() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        // Initially pre-initialized
        assert_eq!(trader.state(), ComponentState::PreInitialized);
        assert!(!trader.is_running());
        assert!(!trader.is_stopped());
        assert!(!trader.is_disposed());

        // Cannot start from pre-initialized state
        assert!(trader.start().is_err());

        // Simulate initialization (normally done by kernel)
        trader.initialize().unwrap();

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

    #[rstest]
    fn test_cannot_add_components_while_running() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(trader_id, instance_id, Environment::Backtest, clock, cache);

        // Simulate running state
        trader.state = ComponentState::Running;

        let actor = TestDataActor::new(DataActorConfig::default());
        let result = trader.add_actor(actor);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("while trader is running")
        );
    }

    #[rstest]
    fn test_create_component_clock_backtest_vs_live() {
        let (_msgbus, cache, _portfolio, _data_engine, _risk_engine, _exec_engine, clock) =
            create_trader_components();
        let trader_id = TraderId::default();
        let instance_id = UUID4::new();

        // Test backtest environment - should create individual test clocks
        let trader_backtest = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock.clone(),
            cache.clone(),
        );

        let backtest_clock = trader_backtest.create_component_clock();
        // In backtest, component clock should be different from system clock
        assert_ne!(
            backtest_clock.as_ptr() as *const _,
            clock.as_ptr() as *const _
        );

        // Test live environment - should share system clock
        let trader_live = Trader::new(
            trader_id,
            instance_id,
            Environment::Live,
            clock.clone(),
            cache,
        );

        let live_clock = trader_live.create_component_clock();
        // In live, component clock should be same as system clock
        assert_eq!(live_clock.as_ptr() as *const _, clock.as_ptr() as *const _);
    }
}
