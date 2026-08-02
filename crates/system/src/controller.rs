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

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_common::{
    actor::{
        DataActor, DataActorCore, DataActorNative,
        data_actor::{DataActorConfig, ImportableActorConfig},
        registry::try_get_actor_unchecked,
    },
    component::Component,
    msgbus::{Endpoint, MStr, TypedHandler, get_message_bus},
    nautilus_actor,
};
use nautilus_model::identifiers::{ActorId, StrategyId};
use nautilus_trading::{ImportableStrategyConfig, Strategy, StrategyNative};

use crate::{messages::ControllerCommand, trader::Trader};

#[derive(Debug)]
pub struct Controller {
    core: DataActorCore,
    trader: Rc<RefCell<Trader>>,
}

impl Controller {
    pub const EXECUTE_ENDPOINT: &str = "Controller.execute";

    #[must_use]
    pub fn new(trader: Rc<RefCell<Trader>>, config: Option<DataActorConfig>) -> Self {
        Self {
            core: DataActorCore::new(config.unwrap_or_default()),
            trader,
        }
    }

    /// Sends a controller command to the registered controller endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the controller execute endpoint is not registered.
    pub fn send(command: &ControllerCommand) -> anyhow::Result<()> {
        let endpoint = Self::execute_endpoint();
        let handler = {
            let msgbus = get_message_bus();
            msgbus
                .borrow_mut()
                .endpoint_map::<ControllerCommand>()
                .get(endpoint)
                .cloned()
        };

        let Some(handler) = handler else {
            anyhow::bail!(
                "Controller execute endpoint '{}' not registered",
                endpoint.as_str()
            );
        };

        handler.handle(command);
        Ok(())
    }

    /// Executes a controller command against the underlying trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested lifecycle operation fails.
    pub fn execute(&mut self, command: ControllerCommand) -> anyhow::Result<()> {
        match command {
            ControllerCommand::CreateActor(command) => self
                .create_actor_from_config(&command.actor_config, command.start)
                .map(|_| ()),
            ControllerCommand::StartActor(command) => self.start_actor(&command.actor_id),
            ControllerCommand::StopActor(command) => self.stop_actor(&command.actor_id),
            ControllerCommand::RemoveActor(command) => self.remove_actor(&command.actor_id),
            ControllerCommand::CreateStrategy(command) => self
                .create_strategy_from_config(&command.strategy_config, command.start)
                .map(|_| ()),
            ControllerCommand::StartStrategy(command) => self.start_strategy(&command.strategy_id),
            ControllerCommand::StopStrategy(command) => self.stop_strategy(&command.strategy_id),
            ControllerCommand::ExitMarket(strategy_id) => self.exit_market(&strategy_id),
            ControllerCommand::RemoveStrategy(command) => {
                self.remove_strategy(&command.strategy_id)
            }
        }
    }

    /// Creates a new actor and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if actor registration or startup fails.
    pub fn create_actor<T>(&self, actor: T, start: bool) -> anyhow::Result<ActorId>
    where
        T: DataActor + DataActorNative + Component + Debug + 'static,
    {
        let actor_id = actor.actor_id();
        self.trader.borrow_mut().add_actor(actor)?;

        self.start_created_actor(actor_id, start)?;

        Ok(actor_id)
    }

    /// Creates a new actor from a factory and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if the factory, actor registration, or startup fails.
    pub fn create_actor_from_factory<F, T>(
        &self,
        factory: F,
        start: bool,
    ) -> anyhow::Result<ActorId>
    where
        F: FnOnce() -> anyhow::Result<T>,
        T: DataActor + DataActorNative + Component + Debug + 'static,
    {
        let actor = factory()?;
        self.create_actor(actor, start)
    }

    /// Creates a new actor from an importable config and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if the import, actor registration, or startup fails.
    #[cfg(feature = "python")]
    pub fn create_actor_from_config(
        &self,
        actor_config: &ImportableActorConfig,
        start: bool,
    ) -> anyhow::Result<ActorId> {
        let actor_id = self
            .trader
            .borrow_mut()
            .add_actor_from_importable_config(actor_config)?;

        self.start_created_actor(actor_id, start)?;

        Ok(actor_id)
    }

    /// Rejects importable actor creation when Python support is not compiled in.
    ///
    /// # Errors
    ///
    /// Always returns an unsupported command error.
    #[cfg(not(feature = "python"))]
    pub fn create_actor_from_config(
        &self,
        actor_config: &ImportableActorConfig,
        _start: bool,
    ) -> anyhow::Result<ActorId> {
        Self::unsupported_create_actor_config(actor_config)
    }

    /// Creates a new strategy and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if strategy registration or startup fails.
    pub fn create_strategy<T>(&self, mut strategy: T, start: bool) -> anyhow::Result<StrategyId>
    where
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        let strategy_id = self
            .trader
            .borrow()
            .prepare_strategy_for_registration(&mut strategy)?;
        self.trader.borrow_mut().add_strategy(strategy)?;

        self.start_created_strategy(strategy_id, start)?;

        Ok(strategy_id)
    }

    /// Creates a new strategy from a factory and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if the factory, strategy registration, or startup fails.
    pub fn create_strategy_from_factory<F, T>(
        &self,
        factory: F,
        start: bool,
    ) -> anyhow::Result<StrategyId>
    where
        F: FnOnce() -> anyhow::Result<T>,
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        let strategy = factory()?;
        self.create_strategy(strategy, start)
    }

    /// Creates a new strategy from an importable config and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if the import, strategy registration, or startup fails.
    #[cfg(feature = "python")]
    pub fn create_strategy_from_config(
        &self,
        strategy_config: &ImportableStrategyConfig,
        start: bool,
    ) -> anyhow::Result<StrategyId> {
        let strategy_id = self
            .trader
            .borrow_mut()
            .add_strategy_from_importable_config(strategy_config)?;

        self.start_created_strategy(strategy_id, start)?;

        Ok(strategy_id)
    }

    /// Rejects importable strategy creation when Python support is not compiled in.
    ///
    /// # Errors
    ///
    /// Always returns an unsupported command error.
    #[cfg(not(feature = "python"))]
    pub fn create_strategy_from_config(
        &self,
        strategy_config: &ImportableStrategyConfig,
        _start: bool,
    ) -> anyhow::Result<StrategyId> {
        Self::unsupported_create_strategy_config(strategy_config)
    }

    /// Starts the registered actor with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is not registered or cannot be started.
    pub fn start_actor(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        self.trader.borrow().start_actor(actor_id)
    }

    /// Stops the registered actor with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is not registered or cannot be stopped.
    pub fn stop_actor(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        self.trader.borrow().stop_actor(actor_id)
    }

    /// Removes the registered actor with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor cannot be removed.
    pub fn remove_actor(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        if actor_id.inner() == self.core.actor_id().inner() {
            return Ok(());
        }

        self.trader.borrow_mut().remove_actor(actor_id)
    }

    /// Starts the registered strategy with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or cannot be started.
    pub fn start_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        self.trader.borrow().start_strategy(strategy_id)
    }

    /// Stops the registered strategy with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or cannot be stopped.
    pub fn stop_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        self.trader.borrow_mut().stop_strategy(strategy_id)
    }

    /// Sends an exit-market command to the registered strategy.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or its control endpoint is missing.
    pub fn exit_market(&self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        Trader::market_exit_strategy(&self.trader, strategy_id)
    }

    /// Removes the registered strategy with the given identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy cannot be removed.
    pub fn remove_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        self.trader.borrow_mut().remove_strategy(strategy_id)
    }

    fn start_created_actor(&self, actor_id: ActorId, start: bool) -> anyhow::Result<()> {
        if !start {
            return Ok(());
        }

        if let Err(start_err) = self.start_actor(&actor_id) {
            return Err(self.rollback_actor_start_failure(actor_id, start_err));
        }

        Ok(())
    }

    fn start_created_strategy(&self, strategy_id: StrategyId, start: bool) -> anyhow::Result<()> {
        if !start {
            return Ok(());
        }

        if let Err(start_err) = self.start_strategy(&strategy_id) {
            return Err(self.rollback_strategy_start_failure(strategy_id, start_err));
        }

        Ok(())
    }

    fn rollback_actor_start_failure(
        &self,
        actor_id: ActorId,
        start_err: anyhow::Error,
    ) -> anyhow::Error {
        match self.remove_actor(&actor_id) {
            Ok(()) => start_err,
            Err(rollback_err) => anyhow::anyhow!(
                "Failed to start actor {actor_id}: {start_err}; rollback failed: {rollback_err}"
            ),
        }
    }

    fn rollback_strategy_start_failure(
        &self,
        strategy_id: StrategyId,
        start_err: anyhow::Error,
    ) -> anyhow::Error {
        match self.remove_strategy(&strategy_id) {
            Ok(()) => start_err,
            Err(rollback_err) => anyhow::anyhow!(
                "Failed to start strategy {strategy_id}: {start_err}; rollback failed: {rollback_err}"
            ),
        }
    }

    fn register_execute_endpoint(&self) {
        let controller_id = self.core.actor_id().inner();
        let handler = TypedHandler::from(move |command: &ControllerCommand| {
            if let Some(mut controller) = try_get_actor_unchecked::<Self>(&controller_id) {
                if let Err(e) = controller.execute(command.clone()) {
                    log::error!("Controller command failed for {controller_id}: {e}");
                }
            } else {
                log::error!("Controller {controller_id} not found for command handling");
            }
        });

        get_message_bus()
            .borrow_mut()
            .endpoint_map::<ControllerCommand>()
            .register(Self::execute_endpoint(), handler);
    }

    fn deregister_execute_endpoint() {
        get_message_bus()
            .borrow_mut()
            .endpoint_map::<ControllerCommand>()
            .deregister(Self::execute_endpoint());
    }

    fn execute_endpoint() -> MStr<Endpoint> {
        Self::EXECUTE_ENDPOINT.into()
    }

    #[cfg(not(feature = "python"))]
    fn unsupported_create_actor_config(
        actor_config: &ImportableActorConfig,
    ) -> anyhow::Result<ActorId> {
        anyhow::bail!(
            "CreateActor command for importable actor '{}' is not supported by the Rust controller",
            actor_config.actor_path
        );
    }

    #[cfg(not(feature = "python"))]
    fn unsupported_create_strategy_config(
        strategy_config: &ImportableStrategyConfig,
    ) -> anyhow::Result<StrategyId> {
        anyhow::bail!(
            "CreateStrategy command for importable strategy '{}' is not supported by the Rust controller",
            strategy_config.strategy_path
        );
    }
}

impl DataActor for Controller {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.register_execute_endpoint();
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        Self::deregister_execute_endpoint();
        Ok(())
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        self.register_execute_endpoint();
        Ok(())
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Self::deregister_execute_endpoint();
        Ok(())
    }
}

nautilus_actor!(Controller);

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    #[cfg(feature = "python")]
    use std::ffi::CString;

    #[cfg(feature = "python")]
    use nautilus_common::python::actor::{PyDataActor, PyDataActorInner};
    use nautilus_common::{
        actor::data_actor::ImportableActorConfig,
        cache::Cache,
        clock::TestClock,
        enums::{ComponentState, Environment},
        msgbus::{MessageBus, set_message_bus},
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{identifiers::TraderId, stubs::TestDefault};
    use nautilus_portfolio::portfolio::Portfolio;
    #[cfg(feature = "python")]
    use nautilus_trading::python::strategy::{PyStrategy, PyStrategyInner};
    use nautilus_trading::{
        ImportableStrategyConfig, nautilus_strategy,
        strategy::{StrategyConfig, StrategyCore},
    };
    #[cfg(feature = "python")]
    use pyo3::{
        prelude::*,
        types::{PyDict, PyModule},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        clock_factory::ClockFactory,
        messages::{
            CreateActor, CreateStrategy, RemoveActor, RemoveStrategy, StartActor, StartStrategy,
            StopActor, StopStrategy,
        },
    };

    fn start_actor_command(actor_id: ActorId) -> ControllerCommand {
        StartActor::new(actor_id, UUID4::new(), UnixNanos::default()).into()
    }

    fn stop_actor_command(actor_id: ActorId) -> ControllerCommand {
        StopActor::new(actor_id, UUID4::new(), UnixNanos::default()).into()
    }

    fn remove_actor_command(actor_id: ActorId) -> ControllerCommand {
        RemoveActor::new(actor_id, UUID4::new(), UnixNanos::default()).into()
    }

    fn start_strategy_command(strategy_id: StrategyId) -> ControllerCommand {
        StartStrategy::new(strategy_id, UUID4::new(), UnixNanos::default()).into()
    }

    fn stop_strategy_command(strategy_id: StrategyId) -> ControllerCommand {
        StopStrategy::new(strategy_id, UUID4::new(), UnixNanos::default()).into()
    }

    fn remove_strategy_command(strategy_id: StrategyId) -> ControllerCommand {
        RemoveStrategy::new(strategy_id, UUID4::new(), UnixNanos::default()).into()
    }

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

    nautilus_actor!(TestDataActor);

    #[derive(Debug)]
    struct TestStrategy {
        core: StrategyCore,
    }

    impl TestStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
            }
        }
    }

    impl DataActor for TestStrategy {}

    nautilus_strategy!(TestStrategy);

    #[derive(Debug)]
    struct FailingStartActor {
        core: DataActorCore,
    }

    impl FailingStartActor {
        fn new(config: DataActorConfig) -> Self {
            Self {
                core: DataActorCore::new(config),
            }
        }
    }

    impl DataActor for FailingStartActor {
        fn on_start(&mut self) -> anyhow::Result<()> {
            anyhow::bail!("Simulated actor start failure")
        }
    }

    nautilus_actor!(FailingStartActor);

    #[derive(Debug)]
    struct FailingStartStrategy {
        core: StrategyCore,
    }

    impl FailingStartStrategy {
        fn new(config: StrategyConfig) -> Self {
            Self {
                core: StrategyCore::new(config),
            }
        }
    }

    impl DataActor for FailingStartStrategy {
        fn on_start(&mut self) -> anyhow::Result<()> {
            anyhow::bail!("Simulated strategy start failure")
        }
    }

    nautilus_strategy!(FailingStartStrategy);

    #[derive(Debug)]
    struct ReentrantExitStrategy {
        core: StrategyCore,
        actor_to_stop: ActorId,
    }

    impl ReentrantExitStrategy {
        fn new(config: StrategyConfig, actor_to_stop: ActorId) -> Self {
            Self {
                core: StrategyCore::new(config),
                actor_to_stop,
            }
        }
    }

    impl DataActor for ReentrantExitStrategy {}

    nautilus_strategy!(ReentrantExitStrategy, {
        fn on_market_exit(&mut self) {
            Controller::send(&stop_actor_command(self.actor_to_stop)).unwrap();
        }
    });

    fn create_running_controller() -> (Rc<RefCell<Trader>>, ActorId) {
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();
        let clock_factory = ClockFactory::test_default();
        let clock = clock_factory.clock();
        let mut clock_ref = clock.borrow_mut();
        let test_clock = clock_ref
            .as_any_mut()
            .downcast_mut::<TestClock>()
            .expect("test default clock must be TestClock");
        test_clock.set_time(1_000_000_000u64.into());
        drop(clock_ref);

        let msgbus = Rc::new(RefCell::new(MessageBus::new(
            trader_id,
            instance_id,
            Some("test".to_string()),
            None,
        )));
        set_message_bus(msgbus);

        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            clock.clone(),
            cache.clone(),
            None,
        )));

        let trader = Rc::new(RefCell::new(Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        )));
        trader.borrow_mut().initialize().unwrap();

        let controller = Controller::new(
            trader.clone(),
            Some(DataActorConfig {
                actor_id: Some(ActorId::from("Controller-001")),
                ..Default::default()
            }),
        );
        let controller_id = controller.core.actor_id();

        trader.borrow_mut().add_actor(controller).unwrap();
        trader.borrow_mut().start().unwrap();

        (trader, controller_id)
    }

    #[cfg(feature = "python")]
    fn install_controller_importables_module(py: Python<'_>, module_name: &str) {
        let module = PyModule::new(py, module_name).expect("test module should create");
        module
            .setattr("DataActor", py.get_type::<PyDataActor>())
            .expect("DataActor type should bind");
        module
            .setattr("Strategy", py.get_type::<PyStrategy>())
            .expect("Strategy type should bind");
        module
            .setattr("RESULTS", PyDict::new(py))
            .expect("RESULTS should bind");

        let code = CString::new(
            r#"
RESULTS["actor_start"] = 0
RESULTS["strategy_start"] = 0
RESULTS["fallback_post_init"] = 0
RESULTS["fallback_post_init_seen"] = False
RESULTS["fallback_actor_id"] = ""

class CommandActorConfig:
    def __init__(self, actor_id=None, log_events=True, log_commands=True):
        self.actor_id = actor_id
        self.log_events = log_events
        self.log_commands = log_commands

class CommandActor(DataActor):
    def __init__(self, config):
        super().__init__(config)

    def on_start(self):
        RESULTS["actor_start"] += 1

class FailingActor(CommandActor):
    def on_start(self):
        raise RuntimeError("simulated actor start failure")

class FallbackActorConfig:
    def __init__(self):
        self.actor_id = None
        self.log_events = True
        self.log_commands = True
        self.post_init_called = False

    def __post_init__(self):
        self.post_init_called = True
        RESULTS["fallback_post_init"] += 1

class FallbackActor(DataActor):
    def __init__(self, config):
        super().__init__(config)
        RESULTS["fallback_post_init_seen"] = config.post_init_called
        RESULTS["fallback_actor_id"] = str(config.actor_id)

class CommandStrategyConfig:
    def __init__(self, strategy_id=None, log_events=True, log_commands=True):
        self.strategy_id = strategy_id
        self.log_events = log_events
        self.log_commands = log_commands

class CommandStrategy(Strategy):
    def __init__(self, config):
        super().__init__(config)

    def on_start(self):
        RESULTS["strategy_start"] += 1

class FailingStrategy(CommandStrategy):
    def on_start(self):
        raise RuntimeError("simulated strategy start failure")
"#,
        )
        .expect("python test code should be valid CString");

        py.run(code.as_c_str(), Some(&module.dict()), None)
            .expect("test importables module should load");

        let sys_modules = py
            .import("sys")
            .expect("sys should import")
            .getattr("modules")
            .expect("sys.modules should exist");
        sys_modules
            .set_item(module_name, module)
            .expect("test module should register");
    }

    #[rstest]
    #[cfg(not(feature = "python"))]
    fn test_controller_rejects_importable_create_commands() {
        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();

        let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id).unwrap();
        let actor_config = ImportableActorConfig {
            actor_path: "tests.actors:Actor".to_string(),
            config_path: "tests.actors:ActorConfig".to_string(),
            config: HashMap::new(),
        };
        let strategy_config = ImportableStrategyConfig {
            strategy_path: "tests.strategies:Strategy".to_string(),
            config_path: "tests.strategies:StrategyConfig".to_string(),
            config: HashMap::new(),
        };

        let actor_result = controller.execute(
            CreateActor::new(actor_config, true, UUID4::new(), UnixNanos::default()).into(),
        );
        let strategy_result = controller.execute(
            CreateStrategy::new(strategy_config, true, UUID4::new(), UnixNanos::default()).into(),
        );

        assert_eq!(
            actor_result.unwrap_err().to_string(),
            "CreateActor command for importable actor 'tests.actors:Actor' is not supported by the Rust controller"
        );
        assert_eq!(
            strategy_result.unwrap_err().to_string(),
            "CreateStrategy command for importable strategy 'tests.strategies:Strategy' is not supported by the Rust controller"
        );

        drop(controller);
        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn test_controller_creates_importable_actor_and_strategy_commands() {
        Python::initialize();

        let module_name = "test_system_controller_importables";
        Python::attach(|py| install_controller_importables_module(py, module_name));

        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let actor_id = ActorId::from("CommandActor-001");
        let strategy_id = StrategyId::from("CommandStrategy-001");

        {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let actor_config = ImportableActorConfig {
                actor_path: format!("{module_name}:CommandActor"),
                config_path: format!("{module_name}:CommandActorConfig"),
                config: HashMap::from([(
                    "actor_id".to_string(),
                    serde_json::Value::String("CommandActor-001".to_string()),
                )]),
            };
            let strategy_config = ImportableStrategyConfig {
                strategy_path: format!("{module_name}:CommandStrategy"),
                config_path: format!("{module_name}:CommandStrategyConfig"),
                config: HashMap::from([(
                    "strategy_id".to_string(),
                    serde_json::Value::String("CommandStrategy-001".to_string()),
                )]),
            };

            controller
                .execute(
                    CreateActor::new(actor_config, false, UUID4::new(), UnixNanos::default())
                        .into(),
                )
                .unwrap();
            controller
                .execute(
                    CreateStrategy::new(strategy_config, true, UUID4::new(), UnixNanos::default())
                        .into(),
                )
                .unwrap();
        }

        assert!(trader.borrow().actor_ids().contains(&actor_id));
        assert!(trader.borrow().strategy_ids().contains(&strategy_id));

        assert_eq!(
            try_get_actor_unchecked::<PyDataActorInner>(&actor_id.inner())
                .unwrap()
                .state(),
            ComponentState::Ready
        );
        assert_eq!(
            try_get_actor_unchecked::<PyStrategyInner>(&strategy_id.inner())
                .unwrap()
                .state(),
            ComponentState::Running
        );

        Python::attach(|py| {
            let module = py.import(module_name).expect("test module should import");
            let results_obj = module.getattr("RESULTS").expect("RESULTS should exist");
            let results = results_obj
                .cast::<PyDict>()
                .expect("RESULTS should be a dict");
            assert_eq!(
                results
                    .get_item("actor_start")
                    .expect("actor_start lookup should not error")
                    .expect("actor_start should exist")
                    .extract::<usize>()
                    .expect("actor_start should extract"),
                0
            );
            assert_eq!(
                results
                    .get_item("strategy_start")
                    .expect("strategy_start lookup should not error")
                    .expect("strategy_start should exist")
                    .extract::<usize>()
                    .expect("strategy_start should extract"),
                1
            );
        });

        trader.borrow_mut().remove_actor(&actor_id).unwrap();
        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn test_controller_stop_skips_unstarted_importable_components() {
        Python::initialize();

        let module_name = "test_system_controller_unstarted_components";
        Python::attach(|py| install_controller_importables_module(py, module_name));

        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let actor_id = ActorId::from("CommandActor-001");
        let strategy_id = StrategyId::from("CommandStrategy-001");

        {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let actor_config = ImportableActorConfig {
                actor_path: format!("{module_name}:CommandActor"),
                config_path: format!("{module_name}:CommandActorConfig"),
                config: HashMap::from([(
                    "actor_id".to_string(),
                    serde_json::Value::String("CommandActor-001".to_string()),
                )]),
            };
            let strategy_config = ImportableStrategyConfig {
                strategy_path: format!("{module_name}:CommandStrategy"),
                config_path: format!("{module_name}:CommandStrategyConfig"),
                config: HashMap::from([(
                    "strategy_id".to_string(),
                    serde_json::Value::String("CommandStrategy-001".to_string()),
                )]),
            };

            controller
                .execute(
                    CreateActor::new(actor_config, false, UUID4::new(), UnixNanos::default())
                        .into(),
                )
                .unwrap();
            controller
                .execute(
                    CreateStrategy::new(strategy_config, false, UUID4::new(), UnixNanos::default())
                        .into(),
                )
                .unwrap();
        }

        assert_eq!(
            try_get_actor_unchecked::<PyDataActorInner>(&actor_id.inner())
                .unwrap()
                .state(),
            ComponentState::Ready
        );
        assert_eq!(
            try_get_actor_unchecked::<PyStrategyInner>(&strategy_id.inner())
                .unwrap()
                .state(),
            ComponentState::Ready
        );

        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn test_controller_importable_strategy_tag_collision_does_not_register_orphan() {
        Python::initialize();

        let module_name = "test_system_controller_tag_collision";
        Python::attach(|py| install_controller_importables_module(py, module_name));

        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let existing_strategy_id = StrategyId::from("ExistingStrategy-001");
        let colliding_strategy_id = StrategyId::from("CommandStrategy-001");

        {
            let controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            controller
                .create_strategy(
                    TestStrategy::new(StrategyConfig {
                        strategy_id: Some(existing_strategy_id),
                        order_id_tag: Some("001".to_string()),
                        ..Default::default()
                    }),
                    false,
                )
                .unwrap();
        }

        let clock_count_before = trader.borrow().get_component_clocks().len();

        let result = {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let strategy_config = ImportableStrategyConfig {
                strategy_path: format!("{module_name}:CommandStrategy"),
                config_path: format!("{module_name}:CommandStrategyConfig"),
                config: HashMap::from([(
                    "strategy_id".to_string(),
                    serde_json::Value::String("CommandStrategy-001".to_string()),
                )]),
            };

            controller.execute(
                CreateStrategy::new(strategy_config, false, UUID4::new(), UnixNanos::default())
                    .into(),
            )
        };

        assert_eq!(
            result.unwrap_err().to_string(),
            "Strategy order_id_tag conflict for '001', explicitly define unique order_id_tag values"
        );
        assert!(
            !trader
                .borrow()
                .strategy_ids()
                .contains(&colliding_strategy_id)
        );
        assert!(
            try_get_actor_unchecked::<PyStrategyInner>(&colliding_strategy_id.inner()).is_none()
        );
        assert_eq!(
            trader.borrow().get_component_clocks().len(),
            clock_count_before
        );

        trader
            .borrow_mut()
            .remove_strategy(&existing_strategy_id)
            .unwrap();
        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn test_controller_importable_start_failure_rolls_back_registration() {
        Python::initialize();

        let module_name = "test_system_controller_start_failure";
        Python::attach(|py| install_controller_importables_module(py, module_name));

        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let actor_id = ActorId::from("FailingPyActor-001");
        let strategy_id = StrategyId::from("FailingPyStrategy-001");
        let clock_count_before = trader.borrow().get_component_clocks().len();

        let actor_result = {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let actor_config = ImportableActorConfig {
                actor_path: format!("{module_name}:FailingActor"),
                config_path: format!("{module_name}:CommandActorConfig"),
                config: HashMap::from([(
                    "actor_id".to_string(),
                    serde_json::Value::String("FailingPyActor-001".to_string()),
                )]),
            };

            controller.execute(
                CreateActor::new(actor_config, true, UUID4::new(), UnixNanos::default()).into(),
            )
        };

        assert!(
            actor_result
                .unwrap_err()
                .to_string()
                .contains("simulated actor start failure")
        );
        assert!(!trader.borrow().actor_ids().contains(&actor_id));
        if let Some(actor) = try_get_actor_unchecked::<PyDataActorInner>(&actor_id.inner()) {
            assert_eq!(actor.state(), ComponentState::Disposed);
        }
        assert_eq!(
            trader.borrow().get_component_clocks().len(),
            clock_count_before
        );

        let strategy_result = {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let strategy_config = ImportableStrategyConfig {
                strategy_path: format!("{module_name}:FailingStrategy"),
                config_path: format!("{module_name}:CommandStrategyConfig"),
                config: HashMap::from([(
                    "strategy_id".to_string(),
                    serde_json::Value::String("FailingPyStrategy-001".to_string()),
                )]),
            };

            controller.execute(
                CreateStrategy::new(strategy_config, true, UUID4::new(), UnixNanos::default())
                    .into(),
            )
        };

        assert!(
            strategy_result
                .unwrap_err()
                .to_string()
                .contains("simulated strategy start failure")
        );
        assert!(!trader.borrow().strategy_ids().contains(&strategy_id));
        if let Some(strategy) = try_get_actor_unchecked::<PyStrategyInner>(&strategy_id.inner()) {
            assert_eq!(strategy.state(), ComponentState::Disposed);
        }
        assert_eq!(
            trader.borrow().get_component_clocks().len(),
            clock_count_before
        );

        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn test_controller_importable_malformed_paths_fail_without_mutation() {
        Python::initialize();

        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let clock_count_before = trader.borrow().get_component_clocks().len();

        let actor_result = {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let actor_config = ImportableActorConfig {
                actor_path: "no_colon_here".to_string(),
                config_path: String::new(),
                config: HashMap::new(),
            };

            controller.execute(
                CreateActor::new(actor_config, false, UUID4::new(), UnixNanos::default()).into(),
            )
        };
        let strategy_result = {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let strategy_config = ImportableStrategyConfig {
                strategy_path: "module:Class:Extra".to_string(),
                config_path: String::new(),
                config: HashMap::new(),
            };

            controller.execute(
                CreateStrategy::new(strategy_config, false, UUID4::new(), UnixNanos::default())
                    .into(),
            )
        };

        assert_eq!(
            actor_result.unwrap_err().to_string(),
            "actor_path must be in format 'module.path:ClassName'"
        );
        assert_eq!(
            strategy_result.unwrap_err().to_string(),
            "strategy_path must be in format 'module.path:ClassName'"
        );
        assert_eq!(trader.borrow().actor_ids(), vec![controller_id]);
        assert!(trader.borrow().strategy_ids().is_empty());
        assert_eq!(
            trader.borrow().get_component_clocks().len(),
            clock_count_before
        );

        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    #[cfg(feature = "python")]
    fn test_controller_importable_config_fallback_registers_actor() {
        Python::initialize();

        let module_name = "test_system_controller_config_fallback";
        Python::attach(|py| install_controller_importables_module(py, module_name));

        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let actor_id = ActorId::from("FallbackActor-001");

        {
            let mut controller = try_get_actor_unchecked::<Controller>(&controller_actor_id)
                .expect("controller should be registered");
            let actor_config = ImportableActorConfig {
                actor_path: format!("{module_name}:FallbackActor"),
                config_path: format!("{module_name}:FallbackActorConfig"),
                config: HashMap::from([(
                    "actor_id".to_string(),
                    serde_json::Value::String("FallbackActor-001".to_string()),
                )]),
            };

            controller
                .execute(
                    CreateActor::new(actor_config, false, UUID4::new(), UnixNanos::default())
                        .into(),
                )
                .unwrap();
        }

        assert!(trader.borrow().actor_ids().contains(&actor_id));
        assert_eq!(
            try_get_actor_unchecked::<PyDataActorInner>(&actor_id.inner())
                .unwrap()
                .state(),
            ComponentState::Ready
        );

        Python::attach(|py| {
            let module = py.import(module_name).expect("test module should import");
            let results_obj = module.getattr("RESULTS").expect("RESULTS should exist");
            let results = results_obj
                .cast::<PyDict>()
                .expect("RESULTS should be a dict");
            assert_eq!(
                results
                    .get_item("fallback_post_init")
                    .expect("fallback_post_init lookup should not error")
                    .expect("fallback_post_init should exist")
                    .extract::<usize>()
                    .expect("fallback_post_init should extract"),
                1
            );
            assert!(
                results
                    .get_item("fallback_post_init_seen")
                    .expect("fallback_post_init_seen lookup should not error")
                    .expect("fallback_post_init_seen should exist")
                    .extract::<bool>()
                    .expect("fallback_post_init_seen should extract")
            );
            assert_eq!(
                results
                    .get_item("fallback_actor_id")
                    .expect("fallback_actor_id lookup should not error")
                    .expect("fallback_actor_id should exist")
                    .extract::<String>()
                    .expect("fallback_actor_id should extract"),
                "FallbackActor-001"
            );
        });

        trader.borrow_mut().remove_actor(&actor_id).unwrap();
        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    fn test_controller_manages_actor_lifecycle_by_message() {
        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();

        let actor_id = {
            let controller = try_get_actor_unchecked::<Controller>(&controller_actor_id).unwrap();
            controller
                .create_actor(
                    TestDataActor::new(DataActorConfig {
                        actor_id: Some(ActorId::from("TestActor-001")),
                        ..Default::default()
                    }),
                    false,
                )
                .unwrap()
        };

        assert!(trader.borrow().actor_ids().contains(&actor_id));

        Controller::send(&start_actor_command(actor_id)).unwrap();
        let actor_registry_id = actor_id.inner();
        assert_eq!(
            try_get_actor_unchecked::<TestDataActor>(&actor_registry_id)
                .unwrap()
                .state(),
            ComponentState::Running
        );

        Controller::send(&stop_actor_command(actor_id)).unwrap();
        assert_eq!(
            try_get_actor_unchecked::<TestDataActor>(&actor_registry_id)
                .unwrap()
                .state(),
            ComponentState::Stopped
        );

        Controller::send(&remove_actor_command(actor_id)).unwrap();
        assert!(!trader.borrow().actor_ids().contains(&actor_id));

        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    fn test_controller_manages_strategy_lifecycle_and_exit_market() {
        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();

        let strategy_id = {
            let controller = try_get_actor_unchecked::<Controller>(&controller_actor_id).unwrap();
            controller
                .create_strategy(
                    TestStrategy::new(StrategyConfig {
                        strategy_id: Some(StrategyId::from("TestStrategy-001")),
                        order_id_tag: Some("001".to_string()),
                        ..Default::default()
                    }),
                    false,
                )
                .unwrap()
        };

        assert!(trader.borrow().strategy_ids().contains(&strategy_id));

        Controller::send(&start_strategy_command(strategy_id)).unwrap();
        let strategy_registry_id = strategy_id.inner();
        assert_eq!(
            try_get_actor_unchecked::<TestStrategy>(&strategy_registry_id)
                .unwrap()
                .state(),
            ComponentState::Running
        );

        Controller::send(&ControllerCommand::ExitMarket(strategy_id)).unwrap();
        assert!(
            try_get_actor_unchecked::<TestStrategy>(&strategy_registry_id)
                .unwrap()
                .is_exiting()
        );

        Controller::send(&stop_strategy_command(strategy_id)).unwrap();
        let strategy = try_get_actor_unchecked::<TestStrategy>(&strategy_registry_id).unwrap();
        assert_eq!(strategy.state(), ComponentState::Stopped);
        assert!(!strategy.is_exiting());
        drop(strategy);

        Controller::send(&remove_strategy_command(strategy_id)).unwrap();
        assert!(!trader.borrow().strategy_ids().contains(&strategy_id));

        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    fn test_controller_create_actor_rolls_back_on_start_failure() {
        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let actor_id = ActorId::from("FailingActor-001");

        let result = {
            let controller = try_get_actor_unchecked::<Controller>(&controller_actor_id).unwrap();
            controller.create_actor(
                FailingStartActor::new(DataActorConfig {
                    actor_id: Some(actor_id),
                    ..Default::default()
                }),
                true,
            )
        };

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Simulated actor start failure")
        );
        assert!(!trader.borrow().actor_ids().contains(&actor_id));
        if let Some(actor) = try_get_actor_unchecked::<FailingStartActor>(&actor_id.inner()) {
            assert_eq!(actor.state(), ComponentState::Disposed);
        }

        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    fn test_controller_create_strategy_rolls_back_on_start_failure() {
        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();
        let strategy_id = StrategyId::from("FailingStrategy-001");

        let result = {
            let controller = try_get_actor_unchecked::<Controller>(&controller_actor_id).unwrap();
            controller.create_strategy(
                FailingStartStrategy::new(StrategyConfig {
                    strategy_id: Some(strategy_id),
                    order_id_tag: Some("001".to_string()),
                    ..Default::default()
                }),
                true,
            )
        };

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Simulated strategy start failure")
        );
        assert!(!trader.borrow().strategy_ids().contains(&strategy_id));

        if let Some(strategy) =
            try_get_actor_unchecked::<FailingStartStrategy>(&strategy_id.inner())
        {
            assert_eq!(strategy.state(), ComponentState::Disposed);
        }

        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    fn test_controller_exit_market_allows_reentrant_controller_commands() {
        let (trader, controller_id) = create_running_controller();
        let controller_actor_id = controller_id.inner();

        let helper_actor_id = {
            let controller = try_get_actor_unchecked::<Controller>(&controller_actor_id).unwrap();
            controller
                .create_actor(
                    TestDataActor::new(DataActorConfig {
                        actor_id: Some(ActorId::from("HelperActor-001")),
                        ..Default::default()
                    }),
                    true,
                )
                .unwrap()
        };

        let strategy_id = {
            let controller = try_get_actor_unchecked::<Controller>(&controller_actor_id).unwrap();
            controller
                .create_strategy(
                    ReentrantExitStrategy::new(
                        StrategyConfig {
                            strategy_id: Some(StrategyId::from("ReentrantStrategy-001")),
                            order_id_tag: Some("001".to_string()),
                            ..Default::default()
                        },
                        helper_actor_id,
                    ),
                    false,
                )
                .unwrap()
        };

        Controller::send(&start_strategy_command(strategy_id)).unwrap();
        Controller::send(&ControllerCommand::ExitMarket(strategy_id)).unwrap();

        let helper_actor =
            try_get_actor_unchecked::<TestDataActor>(&helper_actor_id.inner()).unwrap();
        assert_eq!(helper_actor.state(), ComponentState::Stopped);
        drop(helper_actor);
        assert!(
            try_get_actor_unchecked::<ReentrantExitStrategy>(&strategy_id.inner())
                .unwrap()
                .is_exiting()
        );

        Controller::send(&stop_strategy_command(strategy_id)).unwrap();
        Controller::send(&remove_strategy_command(strategy_id)).unwrap();
        Controller::send(&remove_actor_command(helper_actor_id)).unwrap();
        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    fn test_controller_send_fails_after_controller_stop() {
        let (trader, _) = create_running_controller();

        trader.borrow_mut().stop().unwrap();

        let result = Controller::send(&stop_actor_command(ActorId::from("AnyActor-001")));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Controller execute endpoint 'Controller.execute' not registered"
        );

        trader.borrow_mut().dispose_components().unwrap();
    }
}
