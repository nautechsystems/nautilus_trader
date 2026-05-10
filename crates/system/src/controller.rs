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
        DataActor, DataActorCore, data_actor::DataActorConfig, registry::try_get_actor_unchecked,
    },
    component::Component,
    msgbus::{Endpoint, MStr, TypedHandler, get_message_bus},
    nautilus_actor,
};
use nautilus_model::identifiers::{ActorId, StrategyId};
use nautilus_trading::Strategy;

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
    pub fn send(command: ControllerCommand) -> anyhow::Result<()> {
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

        handler.handle(&command);
        Ok(())
    }

    /// Executes a controller command against the underlying trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested lifecycle operation fails.
    pub fn execute(&mut self, command: ControllerCommand) -> anyhow::Result<()> {
        match command {
            ControllerCommand::StartActor(actor_id) => self.start_actor(&actor_id),
            ControllerCommand::StopActor(actor_id) => self.stop_actor(&actor_id),
            ControllerCommand::RemoveActor(actor_id) => self.remove_actor(&actor_id),
            ControllerCommand::StartStrategy(strategy_id) => self.start_strategy(&strategy_id),
            ControllerCommand::StopStrategy(strategy_id) => self.stop_strategy(&strategy_id),
            ControllerCommand::ExitMarket(strategy_id) => self.exit_market(&strategy_id),
            ControllerCommand::RemoveStrategy(strategy_id) => self.remove_strategy(&strategy_id),
        }
    }

    /// Creates a new actor and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if actor registration or startup fails.
    pub fn create_actor<T>(&self, actor: T, start: bool) -> anyhow::Result<ActorId>
    where
        T: DataActor + Component + Debug + 'static,
    {
        let actor_id = actor.actor_id();
        self.trader.borrow_mut().add_actor(actor)?;

        self.start_created_actor(&actor_id, start)?;

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
        T: DataActor + Component + Debug + 'static,
    {
        let actor = factory()?;
        self.create_actor(actor, start)
    }

    /// Creates a new strategy and optionally starts it.
    ///
    /// # Errors
    ///
    /// Returns an error if strategy registration or startup fails.
    pub fn create_strategy<T>(&self, strategy: T, start: bool) -> anyhow::Result<StrategyId>
    where
        T: Strategy + Component + Debug + 'static,
    {
        let strategy_id = StrategyId::from(strategy.component_id().inner().as_str());
        self.trader.borrow_mut().add_strategy(strategy)?;

        self.start_created_strategy(&strategy_id, start)?;

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
        T: Strategy + Component + Debug + 'static,
    {
        let strategy = factory()?;
        self.create_strategy(strategy, start)
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
        if actor_id.inner() == self.actor_id().inner() {
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

    fn start_created_actor(&self, actor_id: &ActorId, start: bool) -> anyhow::Result<()> {
        if !start {
            return Ok(());
        }

        if let Err(start_err) = self.start_actor(actor_id) {
            return Err(self.rollback_actor_start_failure(actor_id, start_err));
        }

        Ok(())
    }

    fn start_created_strategy(&self, strategy_id: &StrategyId, start: bool) -> anyhow::Result<()> {
        if !start {
            return Ok(());
        }

        if let Err(start_err) = self.start_strategy(strategy_id) {
            return Err(self.rollback_strategy_start_failure(strategy_id, start_err));
        }

        Ok(())
    }

    fn rollback_actor_start_failure(
        &self,
        actor_id: &ActorId,
        start_err: anyhow::Error,
    ) -> anyhow::Error {
        match self.remove_actor(actor_id) {
            Ok(()) => start_err,
            Err(rollback_err) => anyhow::anyhow!(
                "Failed to start actor {actor_id}: {start_err}; rollback failed: {rollback_err}"
            ),
        }
    }

    fn rollback_strategy_start_failure(
        &self,
        strategy_id: &StrategyId,
        start_err: anyhow::Error,
    ) -> anyhow::Error {
        match self.remove_strategy(strategy_id) {
            Ok(()) => start_err,
            Err(rollback_err) => anyhow::anyhow!(
                "Failed to start strategy {strategy_id}: {start_err}; rollback failed: {rollback_err}"
            ),
        }
    }

    fn register_execute_endpoint(&self) {
        let controller_id = self.actor_id().inner();
        let handler = TypedHandler::from(move |command: &ControllerCommand| {
            if let Some(mut controller) = try_get_actor_unchecked::<Self>(&controller_id) {
                if let Err(e) = controller.execute(*command) {
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

    fn deregister_execute_endpoint(&self) {
        get_message_bus()
            .borrow_mut()
            .endpoint_map::<ControllerCommand>()
            .deregister(Self::execute_endpoint());
    }

    fn execute_endpoint() -> MStr<Endpoint> {
        Self::EXECUTE_ENDPOINT.into()
    }
}

impl DataActor for Controller {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.register_execute_endpoint();
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.deregister_execute_endpoint();
        Ok(())
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        self.register_execute_endpoint();
        Ok(())
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        self.deregister_execute_endpoint();
        Ok(())
    }
}

nautilus_actor!(Controller);

#[cfg(test)]
mod tests {
    use nautilus_common::{
        cache::Cache,
        clock::{Clock, TestClock},
        enums::{ComponentState, Environment},
        msgbus::{MessageBus, set_message_bus},
    };
    use nautilus_core::UUID4;
    use nautilus_model::{identifiers::TraderId, stubs::TestDefault};
    use nautilus_portfolio::portfolio::Portfolio;
    use nautilus_trading::{
        nautilus_strategy,
        strategy::{StrategyConfig, StrategyCore},
    };
    use rstest::rstest;

    use super::*;

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
            Controller::send(ControllerCommand::StopActor(self.actor_to_stop)).unwrap();
        }
    });

    fn create_running_controller() -> (Rc<RefCell<Trader>>, ActorId) {
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();
        let clock = Rc::new(RefCell::new(TestClock::new()));
        clock.borrow_mut().set_time(1_000_000_000u64.into());

        let msgbus = Rc::new(RefCell::new(MessageBus::new(
            trader_id,
            instance_id,
            Some("test".to_string()),
            None,
        )));
        set_message_bus(msgbus);

        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone() as Rc<RefCell<dyn Clock>>,
            None,
        )));

        let trader = Rc::new(RefCell::new(Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock as Rc<RefCell<dyn Clock>>,
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
        let controller_id = controller.actor_id();

        trader.borrow_mut().add_actor(controller).unwrap();
        trader.borrow_mut().start().unwrap();

        (trader, controller_id)
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

        Controller::send(ControllerCommand::StartActor(actor_id)).unwrap();
        let actor_registry_id = actor_id.inner();
        assert_eq!(
            try_get_actor_unchecked::<TestDataActor>(&actor_registry_id)
                .unwrap()
                .state(),
            ComponentState::Running
        );

        Controller::send(ControllerCommand::StopActor(actor_id)).unwrap();
        assert_eq!(
            try_get_actor_unchecked::<TestDataActor>(&actor_registry_id)
                .unwrap()
                .state(),
            ComponentState::Stopped
        );

        Controller::send(ControllerCommand::RemoveActor(actor_id)).unwrap();
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

        Controller::send(ControllerCommand::StartStrategy(strategy_id)).unwrap();
        let strategy_registry_id = strategy_id.inner();
        assert_eq!(
            try_get_actor_unchecked::<TestStrategy>(&strategy_registry_id)
                .unwrap()
                .state(),
            ComponentState::Running
        );

        Controller::send(ControllerCommand::ExitMarket(strategy_id)).unwrap();
        assert!(
            try_get_actor_unchecked::<TestStrategy>(&strategy_registry_id)
                .unwrap()
                .is_exiting()
        );

        Controller::send(ControllerCommand::StopStrategy(strategy_id)).unwrap();
        let strategy = try_get_actor_unchecked::<TestStrategy>(&strategy_registry_id).unwrap();
        assert_eq!(strategy.state(), ComponentState::Stopped);
        assert!(!strategy.is_exiting());
        drop(strategy);

        Controller::send(ControllerCommand::RemoveStrategy(strategy_id)).unwrap();
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

        Controller::send(ControllerCommand::StartStrategy(strategy_id)).unwrap();
        Controller::send(ControllerCommand::ExitMarket(strategy_id)).unwrap();

        let helper_actor =
            try_get_actor_unchecked::<TestDataActor>(&helper_actor_id.inner()).unwrap();
        assert_eq!(helper_actor.state(), ComponentState::Stopped);
        drop(helper_actor);
        assert!(
            try_get_actor_unchecked::<ReentrantExitStrategy>(&strategy_id.inner())
                .unwrap()
                .is_exiting()
        );

        Controller::send(ControllerCommand::StopStrategy(strategy_id)).unwrap();
        Controller::send(ControllerCommand::RemoveStrategy(strategy_id)).unwrap();
        Controller::send(ControllerCommand::RemoveActor(helper_actor_id)).unwrap();
        trader.borrow_mut().stop().unwrap();
        trader.borrow_mut().dispose_components().unwrap();
    }

    #[rstest]
    fn test_controller_send_fails_after_controller_stop() {
        let (trader, _) = create_running_controller();

        trader.borrow_mut().stop().unwrap();

        let result = Controller::send(ControllerCommand::StopActor(ActorId::from("AnyActor-001")));
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Controller execute endpoint 'Controller.execute' not registered"
        );

        trader.borrow_mut().dispose_components().unwrap();
    }
}
