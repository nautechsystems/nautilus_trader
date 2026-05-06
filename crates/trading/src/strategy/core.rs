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
    cell::RefCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use ahash::AHashMap;
use nautilus_common::{
    actor::{DataActorConfig, DataActorCore},
    cache::Cache,
    clock::Clock,
    factories::OrderFactory,
};
use nautilus_execution::order_manager::manager::OrderManager;
use nautilus_model::identifiers::{
    ActorId, ClientOrderId, StrategyId, TraderId, normalize_order_id_tag,
};
use nautilus_portfolio::portfolio::Portfolio;
use ustr::Ustr;

use super::config::StrategyConfig;

/// The core component of a [`Strategy`](super::Strategy), managing data, orders, and state.
///
/// This struct is intended to be held as a member within a user's custom strategy struct.
/// The user's struct should then `Deref` and `DerefMut` to this `StrategyCore` instance
/// to satisfy the trait bounds of [`Strategy`](super::Strategy) and
/// [`DataActor`](nautilus_common::actor::data_actor::DataActor).
pub struct StrategyCore {
    pub(crate) actor: DataActorCore,
    /// The strategy configuration.
    pub config: StrategyConfig,
    strategy_id: Option<StrategyId>,
    order_id_tag: Option<String>,
    pub(crate) order_manager: Option<OrderManager>,
    pub(crate) order_factory: Option<OrderFactory>,
    pub(crate) portfolio: Option<Rc<RefCell<Portfolio>>>,
    pub(crate) gtd_timers: AHashMap<ClientOrderId, Ustr>,
    pub(crate) is_exiting: bool,
    pub(crate) pending_stop: bool,
    pub(crate) market_exit_attempts: u64,
    pub(crate) market_exit_timer_name: Ustr,
    pub(crate) market_exit_tag: Ustr,
}

impl Debug for StrategyCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(StrategyCore))
            .field("actor", &self.actor)
            .field("config", &self.config)
            .field("strategy_id", &self.strategy_id)
            .field("order_id_tag", &self.order_id_tag)
            .field("order_manager", &self.order_manager)
            .field("order_factory", &self.order_factory)
            .field("is_exiting", &self.is_exiting)
            .field("pending_stop", &self.pending_stop)
            .field("market_exit_attempts", &self.market_exit_attempts)
            .finish()
    }
}

impl StrategyCore {
    /// Creates a new [`StrategyCore`] instance.
    pub fn new(config: StrategyConfig) -> Self {
        let configured_strategy_id = config.strategy_id;
        let configured_order_id_tag = normalize_order_id_tag(config.order_id_tag.as_deref());
        let strategy_id = configured_strategy_id
            .map(|id| strategy_id_with_order_id_tag(id, configured_order_id_tag));
        let order_id_tag = strategy_id
            .map(|id| id.get_tag().to_string())
            .or_else(|| configured_order_id_tag.map(str::to_string));

        let actor_config = DataActorConfig {
            actor_id: strategy_id.map(|id| ActorId::from(id.inner().as_str())),
            log_events: config.log_events,
            log_commands: config.log_commands,
        };

        let strategy_id_str = strategy_id
            .map(|id| id.inner().to_string())
            .unwrap_or_default();
        let market_exit_timer_name = Ustr::from(&format!("MARKET_EXIT_CHECK:{strategy_id_str}"));

        Self {
            actor: DataActorCore::new(actor_config),
            config,
            strategy_id,
            order_id_tag,
            order_manager: None,
            order_factory: None,
            portfolio: None,
            gtd_timers: AHashMap::new(),
            is_exiting: false,
            pending_stop: false,
            market_exit_attempts: 0,
            market_exit_timer_name,
            market_exit_tag: Ustr::from("MARKET_EXIT"),
        }
    }

    /// Changes the strategy ID before registration.
    pub fn change_id(&mut self, strategy_id: StrategyId) {
        let strategy_id = strategy_id_with_order_id_tag(strategy_id, self.order_id_tag());
        self.set_runtime_strategy_id(strategy_id);
    }

    /// Changes the order ID tag before registration.
    pub fn change_order_id_tag(&mut self, order_id_tag: &str) {
        self.order_id_tag = normalize_order_id_tag(Some(order_id_tag)).map(str::to_string);

        if let Some(strategy_id) = self.strategy_id
            && let Some(order_id_tag) = self.order_id_tag()
        {
            let strategy_id = strategy_id_with_order_id_tag(strategy_id, Some(order_id_tag));
            self.set_runtime_strategy_id(strategy_id);
        }
    }

    fn set_runtime_strategy_id(&mut self, strategy_id: StrategyId) {
        let actor_id = ActorId::from(strategy_id.inner().as_str());
        self.actor.actor_id = actor_id;
        self.actor.config.actor_id = Some(actor_id);
        self.strategy_id = Some(strategy_id);
        self.order_id_tag = Some(strategy_id.get_tag().to_string());
        self.market_exit_timer_name = Ustr::from(&format!("MARKET_EXIT_CHECK:{strategy_id}"));
    }

    /// Returns the runtime order ID tag.
    #[must_use]
    pub fn order_id_tag(&self) -> Option<&str> {
        self.order_id_tag.as_deref()
    }

    /// Returns the runtime strategy ID.
    #[must_use]
    pub fn strategy_id(&self) -> Option<StrategyId> {
        self.strategy_id
    }

    /// Registers the strategy with the trading engine components.
    ///
    /// This is typically called by the framework when the strategy is added to an engine.
    ///
    /// # Errors
    ///
    /// Returns an error if registration with the actor core fails.
    pub fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        portfolio: Rc<RefCell<Portfolio>>,
    ) -> anyhow::Result<()> {
        let strategy_id = StrategyId::from(self.actor.actor_id.inner().as_str());

        self.actor
            .register(trader_id, clock.clone(), cache.clone())?;

        // Update market exit timer name with actual strategy ID
        self.market_exit_timer_name = Ustr::from(&format!("MARKET_EXIT_CHECK:{strategy_id}"));

        self.strategy_id = Some(strategy_id);
        self.order_id_tag = Some(strategy_id.get_tag().to_string());

        self.order_factory = Some(OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock.clone(),
            self.config.use_uuid_client_order_ids,
            self.config.use_hyphens_in_client_order_ids,
        ));

        self.order_manager = Some(OrderManager::new(clock, cache, false, None, None, None));

        self.portfolio = Some(portfolio);

        Ok(())
    }

    /// Returns a mutable reference to the [`OrderFactory`].
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    pub fn order_factory(&mut self) -> &mut OrderFactory {
        self.order_factory
            .as_mut()
            .expect("Strategy not registered: OrderFactory not initialized")
    }

    /// Returns a mutable reference to the [`OrderManager`].
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    pub fn order_manager(&mut self) -> &mut OrderManager {
        self.order_manager
            .as_mut()
            .expect("Strategy not registered: OrderManager not initialized")
    }

    /// Returns a reference to the [`Portfolio`].
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    pub fn portfolio(&self) -> &Rc<RefCell<Portfolio>> {
        self.portfolio
            .as_ref()
            .expect("Strategy not registered: Portfolio not initialized")
    }

    /// Resets the market exit state.
    pub fn reset_market_exit_state(&mut self) {
        self.is_exiting = false;
        self.pending_stop = false;
        self.market_exit_attempts = 0;
    }
}

fn strategy_id_with_order_id_tag(
    strategy_id: StrategyId,
    order_id_tag: Option<&str>,
) -> StrategyId {
    let Some(order_id_tag) = normalize_order_id_tag(order_id_tag) else {
        return strategy_id;
    };

    if strategy_id.get_tag() == order_id_tag {
        strategy_id
    } else {
        StrategyId::from(format!("{strategy_id}-{order_id_tag}"))
    }
}

impl Deref for StrategyCore {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.actor
    }
}

impl DerefMut for StrategyCore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.actor
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_model::identifiers::{StrategyId, TraderId};
    use nautilus_portfolio::portfolio::Portfolio;
    use rstest::rstest;

    use super::*;

    fn create_test_config() -> StrategyConfig {
        StrategyConfig {
            strategy_id: Some(StrategyId::from("TEST-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        }
    }

    #[rstest]
    fn test_strategy_core_new() {
        let config = create_test_config();
        let core = StrategyCore::new(config.clone());

        assert_eq!(core.config.strategy_id, config.strategy_id);
        assert_eq!(core.config.order_id_tag, config.order_id_tag);
        assert_eq!(core.strategy_id(), config.strategy_id);
        assert_eq!(core.order_id_tag(), Some("001"));
        assert!(core.order_manager.is_none());
        assert!(core.order_factory.is_none());
        assert!(core.portfolio.is_none());
        assert!(!core.is_exiting);
        assert!(!core.pending_stop);
        assert_eq!(core.market_exit_attempts, 0);
    }

    #[rstest]
    fn test_strategy_core_new_applies_explicit_order_id_tag_to_strategy_id() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("ExampleStrategy-XNAS")),
            order_id_tag: Some("T01".to_string()),
            ..Default::default()
        };

        let core = StrategyCore::new(config.clone());

        assert_eq!(core.actor_id(), ActorId::from("ExampleStrategy-XNAS-T01"));
        assert_eq!(core.config.strategy_id, config.strategy_id);
        assert_eq!(core.config.order_id_tag, config.order_id_tag);
        assert_eq!(
            core.strategy_id(),
            Some(StrategyId::from("ExampleStrategy-XNAS-T01"))
        );
        assert_eq!(core.order_id_tag(), Some("T01"));
    }

    #[rstest]
    fn test_strategy_core_new_uses_strategy_tag_when_order_id_tag_is_omitted() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("ExampleStrategy-XNAS")),
            ..Default::default()
        };

        let core = StrategyCore::new(config.clone());

        assert_eq!(core.actor_id(), ActorId::from("ExampleStrategy-XNAS"));
        assert_eq!(core.config.strategy_id, config.strategy_id);
        assert_eq!(core.config.order_id_tag, None);
        assert_eq!(core.strategy_id(), config.strategy_id);
        assert_eq!(core.order_id_tag(), Some("XNAS"));
    }

    #[rstest]
    fn test_strategy_core_change_id_appends_existing_order_id_tag() {
        let config = StrategyConfig {
            order_id_tag: Some("T01".to_string()),
            ..Default::default()
        };
        let mut core = StrategyCore::new(config);

        core.change_id(StrategyId::from("ExampleStrategy-XNAS"));

        assert_eq!(core.actor_id(), ActorId::from("ExampleStrategy-XNAS-T01"));
        assert_eq!(
            core.strategy_id(),
            Some(StrategyId::from("ExampleStrategy-XNAS-T01"))
        );
        assert_eq!(core.order_id_tag(), Some("T01"));
    }

    #[rstest]
    fn test_strategy_core_change_order_id_tag_appends_to_existing_strategy_id() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("ExampleStrategy-XNAS")),
            ..Default::default()
        };
        let mut core = StrategyCore::new(config);

        core.change_order_id_tag("T01");

        assert_eq!(core.actor_id(), ActorId::from("ExampleStrategy-XNAS-T01"));
        assert_eq!(
            core.strategy_id(),
            Some(StrategyId::from("ExampleStrategy-XNAS-T01"))
        );
        assert_eq!(core.order_id_tag(), Some("T01"));
    }

    #[rstest]
    fn test_strategy_core_change_order_id_tag_does_not_duplicate_matching_tag() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("ExampleStrategy-XNAS-T01")),
            ..Default::default()
        };
        let mut core = StrategyCore::new(config);

        core.change_order_id_tag("T01");

        assert_eq!(core.actor_id(), ActorId::from("ExampleStrategy-XNAS-T01"));
        assert_eq!(
            core.strategy_id(),
            Some(StrategyId::from("ExampleStrategy-XNAS-T01"))
        );
        assert_eq!(core.order_id_tag(), Some("T01"));
    }

    #[rstest]
    fn test_strategy_core_register() {
        let config = create_test_config();
        let mut core = StrategyCore::new(config);

        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));

        let result = core.register(trader_id, clock, cache, portfolio);
        assert!(result.is_ok());

        assert!(core.order_manager.is_some());
        assert!(core.order_factory.is_some());
        assert!(core.portfolio.is_some());
        assert_eq!(core.trader_id(), Some(trader_id));
    }

    #[rstest]
    fn test_strategy_core_register_uses_order_id_tag_for_factory() {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("ExampleStrategy-XNAS")),
            order_id_tag: Some("T01".to_string()),
            ..Default::default()
        };
        let mut core = StrategyCore::new(config);

        let trader_id = TraderId::from("TRADER-001");
        let clock = Rc::new(RefCell::new(TestClock::new()));
        let cache = Rc::new(RefCell::new(Cache::default()));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            cache.clone(),
            clock.clone(),
            None,
        )));

        core.register(trader_id, clock, cache, portfolio).unwrap();

        let order_factory = core.order_factory();
        let client_order_id = order_factory.generate_client_order_id();
        let order_list_id = order_factory.generate_order_list_id();

        assert_eq!(
            core.strategy_id(),
            Some(StrategyId::from("ExampleStrategy-XNAS-T01"))
        );
        assert_eq!(client_order_id.as_str(), "O-19700101-000000-001-T01-1");
        assert_eq!(order_list_id.as_str(), "OL-19700101-000000-001-T01-1");
    }

    #[rstest]
    fn test_strategy_core_deref() {
        let config = create_test_config();
        let core = StrategyCore::new(config);

        assert!(core.trader_id().is_none());
    }

    #[rstest]
    fn test_strategy_core_debug() {
        let config = create_test_config();
        let core = StrategyCore::new(config);

        let debug_str = format!("{core:?}");
        assert!(debug_str.contains("StrategyCore"));
    }
}
