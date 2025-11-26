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
    cell::RefCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use nautilus_common::{
    actor::{DataActorConfig, DataActorCore},
    cache::Cache,
    clock::Clock,
    factories::OrderFactory,
};
use nautilus_core::time::get_atomic_clock_static;
use nautilus_execution::order_manager::manager::OrderManager;
use nautilus_model::identifiers::{ActorId, StrategyId, TraderId};
use nautilus_portfolio::portfolio::Portfolio;

use super::config::StrategyConfig;

/// The core component of a [`Strategy`], managing data, orders, and state.
///
/// This struct is intended to be held as a member within a user's custom strategy struct.
/// The user's struct should then `Deref` and `DerefMut` to this `StrategyCore` instance
/// to satisfy the trait bounds of [`Strategy`] and [`DataActor`].
pub struct StrategyCore {
    /// The underlying data actor core.
    pub actor: DataActorCore,
    /// The strategy configuration.
    pub config: StrategyConfig,
    /// The order manager.
    pub order_manager: Option<OrderManager>,
    /// The order factory.
    pub order_factory: Option<OrderFactory>,
    /// The portfolio.
    pub portfolio: Option<Rc<RefCell<Portfolio>>>,
}

impl Debug for StrategyCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StrategyCore")
            .field("actor", &self.actor)
            .field("config", &self.config)
            .field("order_manager", &self.order_manager)
            .field("order_factory", &self.order_factory)
            .finish()
    }
}

impl StrategyCore {
    /// Creates a new [`StrategyCore`] instance.
    pub fn new(config: StrategyConfig) -> Self {
        let actor_config = DataActorConfig {
            actor_id: config
                .strategy_id
                .map(|id| ActorId::from(id.inner().as_str())),
            log_events: config.log_events,
            log_commands: config.log_commands,
        };

        Self {
            actor: DataActorCore::new(actor_config),
            config,
            order_manager: None,
            order_factory: None,
            portfolio: None,
        }
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
        self.actor
            .register(trader_id, clock.clone(), cache.clone())?;

        let strategy_id = StrategyId::from(self.actor.actor_id.inner().as_str());

        self.order_factory = Some(OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            get_atomic_clock_static(),
            self.config.use_uuid_client_order_ids,
            self.config.use_hyphens_in_client_order_ids,
        ));

        self.order_manager = Some(OrderManager::new(
            clock, cache, false, // active_local
        ));

        self.portfolio = Some(portfolio);

        Ok(())
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
        assert!(core.order_manager.is_none());
        assert!(core.order_factory.is_none());
        assert!(core.portfolio.is_none());
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
