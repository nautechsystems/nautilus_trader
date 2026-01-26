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

//! Core component for execution algorithms.

use std::{
    cell::RefCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use nautilus_common::{
    actor::{DataActorConfig, DataActorCore},
    cache::Cache,
    clock::Clock,
    msgbus::TypedHandler,
};
use nautilus_model::{
    events::{OrderEventAny, PositionEvent},
    identifiers::{ActorId, ClientOrderId, ExecAlgorithmId, StrategyId, TraderId},
    orders::OrderAny,
    types::Quantity,
};

use super::config::ExecutionAlgorithmConfig;

/// Holds event handlers for strategy event subscriptions.
#[derive(Clone, Debug)]
pub struct StrategyEventHandlers {
    /// The topic string for order events.
    pub order_topic: String,
    /// The handler for order events.
    pub order_handler: TypedHandler<OrderEventAny>,
    /// The topic string for position events.
    pub position_topic: String,
    /// The handler for position events.
    pub position_handler: TypedHandler<PositionEvent>,
}

/// The core component of an [`ExecutionAlgorithm`](super::ExecutionAlgorithm).
///
/// This struct manages the internal state for execution algorithms including
/// spawn ID tracking and strategy subscriptions. It wraps a [`DataActorCore`]
/// to provide data actor capabilities.
///
/// User algorithms should hold this as a member and implement `Deref`/`DerefMut`
/// to satisfy the trait bounds of [`ExecutionAlgorithm`](super::ExecutionAlgorithm).
pub struct ExecutionAlgorithmCore {
    /// The underlying data actor core.
    pub actor: DataActorCore,
    /// The execution algorithm configuration.
    pub config: ExecutionAlgorithmConfig,
    /// The execution algorithm ID.
    pub exec_algorithm_id: ExecAlgorithmId,
    /// Maps primary order client IDs to their spawn sequence counter.
    exec_spawn_ids: AHashMap<ClientOrderId, u32>,
    /// Tracks strategies that have been subscribed to for events.
    subscribed_strategies: AHashSet<StrategyId>,
    /// Tracks pending spawn reductions for quantity restoration on denial/rejection.
    pending_spawn_reductions: AHashMap<ClientOrderId, Quantity>,
    /// Maps strategies to their event handlers for cleanup on reset.
    strategy_event_handlers: AHashMap<StrategyId, StrategyEventHandlers>,
}

impl Debug for ExecutionAlgorithmCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionAlgorithmCore))
            .field("actor", &self.actor)
            .field("config", &self.config)
            .field("exec_algorithm_id", &self.exec_algorithm_id)
            .field("exec_spawn_ids", &self.exec_spawn_ids.len())
            .field("subscribed_strategies", &self.subscribed_strategies.len())
            .field(
                "pending_spawn_reductions",
                &self.pending_spawn_reductions.len(),
            )
            .field(
                "strategy_event_handlers",
                &self.strategy_event_handlers.len(),
            )
            .finish()
    }
}

impl ExecutionAlgorithmCore {
    /// Creates a new [`ExecutionAlgorithmCore`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `config.exec_algorithm_id` is `None`.
    #[must_use]
    pub fn new(config: ExecutionAlgorithmConfig) -> Self {
        let exec_algorithm_id = config
            .exec_algorithm_id
            .expect("ExecutionAlgorithmConfig must have exec_algorithm_id set");

        let actor_config = DataActorConfig {
            actor_id: Some(ActorId::from(exec_algorithm_id.inner().as_str())),
            log_events: config.log_events,
            log_commands: config.log_commands,
        };

        Self {
            actor: DataActorCore::new(actor_config),
            config,
            exec_algorithm_id,
            exec_spawn_ids: AHashMap::new(),
            subscribed_strategies: AHashSet::new(),
            pending_spawn_reductions: AHashMap::new(),
            strategy_event_handlers: AHashMap::new(),
        }
    }

    /// Registers the execution algorithm with the trading engine components.
    ///
    /// # Errors
    ///
    /// Returns an error if registration with the actor core fails.
    pub fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()> {
        self.actor.register(trader_id, clock, cache)
    }

    /// Returns the execution algorithm ID.
    #[must_use]
    pub fn id(&self) -> ExecAlgorithmId {
        self.exec_algorithm_id
    }

    /// Generates the next spawn client order ID for a primary order.
    ///
    /// The generated ID follows the pattern: `{primary_id}-E{sequence}`.
    #[must_use]
    pub fn spawn_client_order_id(&mut self, primary_id: &ClientOrderId) -> ClientOrderId {
        let sequence = self
            .exec_spawn_ids
            .entry(*primary_id)
            .and_modify(|s| *s += 1)
            .or_insert(1);

        ClientOrderId::new(format!("{primary_id}-E{sequence}"))
    }

    /// Returns the current spawn sequence for a primary order, if any.
    #[must_use]
    pub fn spawn_sequence(&self, primary_id: &ClientOrderId) -> Option<u32> {
        self.exec_spawn_ids.get(primary_id).copied()
    }

    /// Checks if a strategy has been subscribed to for events.
    #[must_use]
    pub fn is_strategy_subscribed(&self, strategy_id: &StrategyId) -> bool {
        self.subscribed_strategies.contains(strategy_id)
    }

    /// Marks a strategy as subscribed for events.
    pub fn add_subscribed_strategy(&mut self, strategy_id: StrategyId) {
        self.subscribed_strategies.insert(strategy_id);
    }

    /// Stores the event handlers for a strategy subscription.
    pub fn store_strategy_event_handlers(
        &mut self,
        strategy_id: StrategyId,
        handlers: StrategyEventHandlers,
    ) {
        self.strategy_event_handlers.insert(strategy_id, handlers);
    }

    /// Takes and returns all stored strategy event handlers, clearing the internal map.
    pub fn take_strategy_event_handlers(&mut self) -> AHashMap<StrategyId, StrategyEventHandlers> {
        std::mem::take(&mut self.strategy_event_handlers)
    }

    /// Clears all spawn tracking state.
    pub fn clear_spawn_ids(&mut self) {
        self.exec_spawn_ids.clear();
    }

    /// Clears all strategy subscriptions.
    pub fn clear_subscribed_strategies(&mut self) {
        self.subscribed_strategies.clear();
    }

    /// Tracks a pending spawn reduction for potential restoration.
    pub fn track_pending_spawn_reduction(&mut self, spawn_id: ClientOrderId, quantity: Quantity) {
        self.pending_spawn_reductions.insert(spawn_id, quantity);
    }

    /// Removes and returns the pending spawn reduction for an order, if any.
    pub fn take_pending_spawn_reduction(&mut self, spawn_id: &ClientOrderId) -> Option<Quantity> {
        self.pending_spawn_reductions.remove(spawn_id)
    }

    /// Clears all pending spawn reductions.
    pub fn clear_pending_spawn_reductions(&mut self) {
        self.pending_spawn_reductions.clear();
    }

    /// Resets the core to its initial state.
    ///
    /// Note: This clears handler storage but does NOT unsubscribe from msgbus.
    /// Call `unsubscribe_all_strategy_events` first to properly unsubscribe.
    pub fn reset(&mut self) {
        self.exec_spawn_ids.clear();
        self.subscribed_strategies.clear();
        self.pending_spawn_reductions.clear();
        self.strategy_event_handlers.clear();
    }

    /// Returns the order for the given client order ID from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the order is not found in the cache.
    pub fn get_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<OrderAny> {
        self.cache()
            .order(client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found in cache for {client_order_id}"))
    }
}

impl Deref for ExecutionAlgorithmCore {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.actor
    }
}

impl DerefMut for ExecutionAlgorithmCore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.actor
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn create_test_config() -> ExecutionAlgorithmConfig {
        ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::new("TWAP")),
            ..Default::default()
        }
    }

    #[rstest]
    fn test_core_new() {
        let config = create_test_config();
        let core = ExecutionAlgorithmCore::new(config.clone());

        assert_eq!(core.exec_algorithm_id, ExecAlgorithmId::new("TWAP"));
        assert_eq!(core.config.log_events, config.log_events);
        assert!(core.exec_spawn_ids.is_empty());
        assert!(core.subscribed_strategies.is_empty());
    }

    #[rstest]
    fn test_spawn_client_order_id_sequence() {
        let config = create_test_config();
        let mut core = ExecutionAlgorithmCore::new(config);

        let primary_id = ClientOrderId::new("O-001");

        let spawn1 = core.spawn_client_order_id(&primary_id);
        assert_eq!(spawn1.as_str(), "O-001-E1");

        let spawn2 = core.spawn_client_order_id(&primary_id);
        assert_eq!(spawn2.as_str(), "O-001-E2");

        let spawn3 = core.spawn_client_order_id(&primary_id);
        assert_eq!(spawn3.as_str(), "O-001-E3");
    }

    #[rstest]
    fn test_spawn_client_order_id_different_primaries() {
        let config = create_test_config();
        let mut core = ExecutionAlgorithmCore::new(config);

        let primary1 = ClientOrderId::new("O-001");
        let primary2 = ClientOrderId::new("O-002");

        let spawn1_1 = core.spawn_client_order_id(&primary1);
        let spawn2_1 = core.spawn_client_order_id(&primary2);
        let spawn1_2 = core.spawn_client_order_id(&primary1);

        assert_eq!(spawn1_1.as_str(), "O-001-E1");
        assert_eq!(spawn2_1.as_str(), "O-002-E1");
        assert_eq!(spawn1_2.as_str(), "O-001-E2");
    }

    #[rstest]
    fn test_spawn_sequence() {
        let config = create_test_config();
        let mut core = ExecutionAlgorithmCore::new(config);

        let primary_id = ClientOrderId::new("O-001");

        assert_eq!(core.spawn_sequence(&primary_id), None);

        let _ = core.spawn_client_order_id(&primary_id);
        assert_eq!(core.spawn_sequence(&primary_id), Some(1));

        let _ = core.spawn_client_order_id(&primary_id);
        assert_eq!(core.spawn_sequence(&primary_id), Some(2));
    }

    #[rstest]
    fn test_strategy_subscription_tracking() {
        let config = create_test_config();
        let mut core = ExecutionAlgorithmCore::new(config);

        let strategy_id = StrategyId::new("TEST-001");

        assert!(!core.is_strategy_subscribed(&strategy_id));

        core.add_subscribed_strategy(strategy_id);
        assert!(core.is_strategy_subscribed(&strategy_id));
    }

    #[rstest]
    fn test_clear_spawn_ids() {
        let config = create_test_config();
        let mut core = ExecutionAlgorithmCore::new(config);

        let primary_id = ClientOrderId::new("O-001");
        let _ = core.spawn_client_order_id(&primary_id);

        assert!(core.spawn_sequence(&primary_id).is_some());

        core.clear_spawn_ids();
        assert!(core.spawn_sequence(&primary_id).is_none());
    }

    #[rstest]
    fn test_reset() {
        let config = create_test_config();
        let mut core = ExecutionAlgorithmCore::new(config);

        let primary_id = ClientOrderId::new("O-001");
        let strategy_id = StrategyId::new("TEST-001");

        let _ = core.spawn_client_order_id(&primary_id);
        core.add_subscribed_strategy(strategy_id);

        core.reset();

        assert!(core.spawn_sequence(&primary_id).is_none());
        assert!(!core.is_strategy_subscribed(&strategy_id));
    }

    #[rstest]
    fn test_deref_to_data_actor_core() {
        let config = create_test_config();
        let core = ExecutionAlgorithmCore::new(config);

        // Should be able to access DataActorCore methods via Deref
        assert!(core.trader_id().is_none());
    }
}
