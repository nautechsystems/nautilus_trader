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

//! Central orchestrator for managing actors, strategies, and execution algorithms.
//!
//! The `Trader` component serves as the primary coordination layer between the kernel
//! and individual trading components. It manages component lifecycles, provides
//! unique identification, and coordinates with system engines.

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use ahash::AHashMap;
use indexmap::IndexMap;
#[cfg(feature = "python")]
use nautilus_common::python::wrappers::release_python_wrapper;
use nautilus_common::{
    actor::{
        DataActor, DataActorNative,
        registry::{deregister_actor, try_get_actor_unchecked},
    },
    cache::Cache,
    clock::Clock,
    component::{
        Component, component_state, deregister_component, dispose_component,
        register_component_actor, reset_component, start_component, stop_component,
    },
    enums::{ComponentState, ComponentTrigger, Environment},
    messages::execution::TradingCommand,
    msgbus,
    msgbus::{
        ShareableMessageHandler, TypedHandler, get_message_bus,
        switchboard::{get_event_order_topic, get_event_position_topic},
    },
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    events::{OrderEventAny, PositionEvent},
    identifiers::{
        ActorId, ComponentId, ExecAlgorithmId, StrategyId, TraderId, check_order_id_tag,
        normalize_order_id_tag,
    },
    orders::Order,
};
use nautilus_portfolio::portfolio::Portfolio;
use nautilus_trading::{
    ExecutionAlgorithm, ExecutionAlgorithmNative,
    strategy::{Strategy, StrategyNative},
};
use ustr::Ustr;

use crate::{
    clock_factory::ClockFactory,
    registration::{
        base_strategy_id, ensure_unique_order_id_tag, strategy_control_endpoint,
        strategy_registration_id,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrategyCommand {
    ExitMarket,
}

type ExecAlgorithmSubscriptionFn = Box<dyn FnMut() -> anyhow::Result<()>>;
type PersistedComponentState = IndexMap<String, Vec<u8>>;
type ComponentStateLoadFn = fn(Ustr, PersistedComponentState) -> anyhow::Result<()>;
type ComponentStateSaveFn = fn(Ustr) -> anyhow::Result<PersistedComponentState>;

#[derive(Clone, Copy)]
struct ComponentStateCallbacks {
    load: ComponentStateLoadFn,
    save: ComponentStateSaveFn,
}

/// Central orchestrator for managing trading components.
///
/// The `Trader` manages the lifecycle and coordination of actors, strategies,
/// and execution algorithms within the trading system. It provides component
/// registration, state management, and integration with system engines.
///
/// # Notes
///
/// Strategies implement `Strategy::stop() -> bool` which returns whether to proceed
/// with the component stop. This enables `manage_stop` behavior where the strategy
/// can defer stopping until a market exit completes.
///
/// We store type-erased closures because the component registry stores trait objects
/// and we need to call `Strategy::stop()` which requires the concrete type. The
/// closure is created during `add_strategy` when the concrete type `T` is known.
pub struct Trader {
    /// The unique trader identifier.
    pub trader_id: TraderId,
    /// The unique instance identifier.
    pub instance_id: UUID4,
    /// The trading environment context.
    pub environment: Environment,
    /// Component state for lifecycle management.
    state: ComponentState,
    /// Clock source for trader timestamps and component clocks.
    clock_factory: ClockFactory,
    /// System cache for data storage.
    pub(crate) cache: Rc<RefCell<Cache>>,
    /// Portfolio reference for strategy registration.
    pub(crate) portfolio: Rc<RefCell<Portfolio>>,
    /// Registered actor IDs (actors stored in global registry).
    pub(crate) actor_ids: Vec<ActorId>,
    /// Type-erased state callbacks for registered actors.
    actor_state_callbacks: AHashMap<ActorId, ComponentStateCallbacks>,
    /// Registered strategy IDs (strategies stored in global registry).
    pub(crate) strategy_ids: Vec<StrategyId>,
    /// Type-erased state callbacks for registered strategies.
    strategy_state_callbacks: AHashMap<StrategyId, ComponentStateCallbacks>,
    /// Strategy stop functions for managed stop behavior.
    strategy_stop_fns: AHashMap<StrategyId, Box<dyn FnMut() -> bool>>,
    /// Msgbus handler IDs for strategy event subscriptions (order, position).
    strategy_handler_ids: AHashMap<StrategyId, (Ustr, Ustr)>,
    /// Registered exec algorithm IDs (algorithms stored in global registry).
    pub(crate) exec_algorithm_ids: Vec<ExecAlgorithmId>,
    /// Restores strategy event subscriptions for concrete execution algorithms.
    exec_algorithm_restore_fns: AHashMap<ExecAlgorithmId, ExecAlgorithmSubscriptionFn>,
    /// Removes strategy event subscriptions for concrete execution algorithms.
    exec_algorithm_cleanup_fns: AHashMap<ExecAlgorithmId, ExecAlgorithmSubscriptionFn>,
    /// Component clocks for individual components.
    pub(crate) clocks: IndexMap<ComponentId, Rc<RefCell<dyn Clock>>>,
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
        clock_factory: ClockFactory,
        cache: Rc<RefCell<Cache>>,
        portfolio: Rc<RefCell<Portfolio>>,
    ) -> Self {
        let clock = clock_factory.clock();
        let ts_created = clock.borrow().timestamp_ns();

        Self {
            trader_id,
            instance_id,
            environment,
            state: ComponentState::PreInitialized,
            clock_factory,
            cache,
            portfolio,
            actor_ids: Vec::new(),
            actor_state_callbacks: AHashMap::new(),
            strategy_ids: Vec::new(),
            strategy_state_callbacks: AHashMap::new(),
            strategy_stop_fns: AHashMap::new(),
            strategy_handler_ids: AHashMap::new(),
            exec_algorithm_ids: Vec::new(),
            exec_algorithm_restore_fns: AHashMap::new(),
            exec_algorithm_cleanup_fns: AHashMap::new(),
            clocks: IndexMap::new(),
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
    pub const fn strategy_count(&self) -> usize {
        self.strategy_ids.len()
    }

    /// Returns the number of registered execution algorithms.
    #[must_use]
    pub const fn exec_algorithm_count(&self) -> usize {
        self.exec_algorithm_ids.len()
    }

    /// Returns references to all component clocks for backtest time advancement.
    #[must_use]
    pub fn get_component_clocks(&self) -> Vec<Rc<RefCell<dyn Clock>>> {
        self.clocks.values().cloned().collect()
    }

    /// Returns the total number of registered components.
    #[must_use]
    pub const fn component_count(&self) -> usize {
        self.actor_ids.len() + self.strategy_ids.len() + self.exec_algorithm_ids.len()
    }

    /// Returns a list of all registered actor IDs.
    #[must_use]
    pub fn actor_ids(&self) -> Vec<ActorId> {
        self.actor_ids.clone()
    }

    /// Returns a list of all registered strategy IDs.
    #[must_use]
    pub fn strategy_ids(&self) -> Vec<StrategyId> {
        self.strategy_ids.clone()
    }

    /// Returns a list of all registered execution algorithm IDs.
    #[must_use]
    pub fn exec_algorithm_ids(&self) -> Vec<ExecAlgorithmId> {
        self.exec_algorithm_ids.clone()
    }

    /// Creates a clock for a component and registers it for time advancement.
    ///
    /// Each component gets its own clock instance so that the default time event
    /// callback registered on each clock is independent. In backtest mode, the
    /// clocks are also used for deterministic time advancement by the engine.
    pub fn create_component_clock(&mut self, component_id: ComponentId) -> Rc<RefCell<dyn Clock>> {
        let clock = self.clock_factory.create_component_clock();
        self.clocks.insert(component_id, clock.clone());
        clock
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
        T: DataActor + DataActorNative + Component + Debug + 'static,
    {
        self.validate_actor_or_strategy_registration()?;

        let actor_id = actor.actor_id();

        // Check for duplicate registration
        if self.actor_ids.contains(&actor_id) {
            anyhow::bail!("Actor {actor_id} is already registered");
        }

        let component_id = ComponentId::from(actor_id);
        let clock = self.create_component_clock(component_id);

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
        T: DataActor + DataActorNative + Component + Debug + 'static,
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
        T: DataActor + DataActorNative + Component + Debug + 'static,
    {
        let actor_id = actor.actor_id();

        // Register in both component and actor registries (this consumes the actor)
        register_component_actor(actor);

        // Store actor ID for lifecycle management
        self.actor_ids.push(actor_id);
        self.actor_state_callbacks.insert(
            actor_id,
            ComponentStateCallbacks {
                load: Self::load_component_state::<T>,
                save: Self::save_component_state::<T>,
            },
        );

        log::info!("Registered actor {actor_id} with trader {}", self.trader_id);

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
    pub fn add_actor_id_for_lifecycle<T>(&mut self, actor_id: ActorId) -> anyhow::Result<()>
    where
        T: DataActor + DataActorNative + Debug + 'static,
    {
        // Check for duplicate registration
        if self.actor_ids.contains(&actor_id) {
            anyhow::bail!("Actor '{actor_id}' is already tracked by trader");
        }

        // Store actor ID for lifecycle management
        self.actor_ids.push(actor_id);
        self.actor_state_callbacks.insert(
            actor_id,
            ComponentStateCallbacks {
                load: Self::load_component_state::<T>,
                save: Self::save_component_state::<T>,
            },
        );

        log::debug!(
            "Added actor ID '{actor_id}' to trader {} for lifecycle management",
            self.trader_id
        );

        Ok(())
    }

    /// Adds an externally-registered execution algorithm ID to the trader for lifecycle management.
    ///
    /// The execution algorithm must already be registered in the global component and actor
    /// registries. This method only tracks the ID so the trader can manage the algorithm's
    /// lifecycle (start/stop/dispose).
    ///
    /// # Errors
    ///
    /// Returns an error if an execution algorithm with the same ID is already tracked.
    pub fn add_exec_algorithm_id_for_lifecycle(
        &mut self,
        exec_algorithm_id: ExecAlgorithmId,
    ) -> anyhow::Result<()> {
        if self.exec_algorithm_ids.contains(&exec_algorithm_id) {
            anyhow::bail!("Execution algorithm '{exec_algorithm_id}' is already tracked by trader");
        }

        self.exec_algorithm_ids.push(exec_algorithm_id);

        log::debug!(
            "Added exec algorithm ID '{exec_algorithm_id}' to trader {} for lifecycle management",
            self.trader_id
        );

        Ok(())
    }

    /// Adds an externally-registered strategy to the trader for lifecycle management
    /// and installs its order/position event subscriptions, stop hook, and control endpoint.
    ///
    /// The strategy must already be registered in the global component and actor
    /// registries. The generic parameter `T` must match the concrete type stored
    /// in those registries so that the typed event handlers can retrieve it.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy ID is already tracked by this trader.
    pub fn add_strategy_id_with_subscriptions<T>(
        &mut self,
        strategy_id: StrategyId,
    ) -> anyhow::Result<()>
    where
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        if self.strategy_ids.contains(&strategy_id) {
            anyhow::bail!("Strategy '{strategy_id}' is already tracked by trader");
        }

        let existing_order_id_tags: Vec<&str> =
            self.strategy_ids.iter().map(StrategyId::get_tag).collect();
        ensure_unique_order_id_tag(&existing_order_id_tags, strategy_id.get_tag())?;

        let actor_id = strategy_id.inner();

        // Subscribe to order events for this strategy
        let order_topic = get_event_order_topic(strategy_id);
        let order_actor_id = actor_id;
        let order_handler = TypedHandler::from(move |event: &OrderEventAny| {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&order_actor_id) {
                strategy.handle_order_event(event.clone());
            } else {
                log::error!("Strategy {order_actor_id} not found for order event handling");
            }
        });
        let order_handler_id = order_handler.id();
        msgbus::subscribe_order_events(order_topic.into(), order_handler, None);

        // Subscribe to position events for this strategy
        let position_topic = get_event_position_topic(strategy_id);
        let position_handler = TypedHandler::from(move |event: &PositionEvent| {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&actor_id) {
                strategy.handle_position_event(event.clone());
            } else {
                log::error!("Strategy {actor_id} not found for position event handling");
            }
        });
        let position_handler_id = position_handler.id();
        msgbus::subscribe_position_events(position_topic.into(), position_handler, None);

        let control_actor_id = actor_id;
        let control_handler = TypedHandler::from(move |command: &StrategyCommand| {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&control_actor_id) {
                match command {
                    StrategyCommand::ExitMarket => {
                        if let Err(e) = strategy.market_exit() {
                            log::error!(
                                "Error handling strategy command for {control_actor_id}: {e}"
                            );
                        }
                    }
                }
            } else {
                log::error!("Strategy {control_actor_id} not found for control handling");
            }
        });
        get_message_bus()
            .borrow_mut()
            .endpoint_map::<StrategyCommand>()
            .register(strategy_control_endpoint(strategy_id), control_handler);

        self.strategy_ids.push(strategy_id);
        self.strategy_state_callbacks.insert(
            strategy_id,
            ComponentStateCallbacks {
                load: Self::load_component_state::<T>,
                save: Self::save_component_state::<T>,
            },
        );
        self.strategy_handler_ids
            .insert(strategy_id, (order_handler_id, position_handler_id));

        // Register stop hook
        let stop_actor_id = actor_id;
        let stop_fn = Box::new(move || -> bool {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&stop_actor_id) {
                Strategy::stop(&mut *strategy)
            } else {
                log::error!("Strategy {stop_actor_id} not found for stop");
                true
            }
        });
        self.strategy_stop_fns.insert(strategy_id, stop_fn);

        log::debug!(
            "Added strategy '{strategy_id}' to trader {} with event subscriptions",
            self.trader_id
        );

        Ok(())
    }

    /// Prepares a strategy ID and order ID tag before registration.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured order ID tag contains the '-' strategy ID separator,
    /// or if the strategy ID or order ID tag is already registered.
    pub fn prepare_strategy_for_registration<T>(
        &self,
        strategy: &mut T,
    ) -> anyhow::Result<StrategyId>
    where
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        // Guards a config built without `StrategyConfig::validate`, such as a struct literal
        if let Some(order_id_tag) = StrategyNative::strategy_core(strategy)
            .config
            .order_id_tag
            .as_deref()
        {
            check_order_id_tag(order_id_tag)?;
        }

        let existing_order_id_tags: Vec<&str> =
            self.strategy_ids.iter().map(StrategyId::get_tag).collect();

        let configured_strategy_id = StrategyNative::strategy_core(strategy).strategy_id();
        let runtime_order_id_tag =
            normalize_order_id_tag(StrategyNative::strategy_core(strategy).order_id_tag());

        let strategy_id = if let Some(strategy_id) = configured_strategy_id {
            ensure_unique_order_id_tag(&existing_order_id_tags, strategy_id.get_tag())?;
            StrategyNative::strategy_core_mut(strategy).change_id(strategy_id);
            strategy_id
        } else {
            let order_id_tag = runtime_order_id_tag.map_or_else(
                || format!("{:03}", existing_order_id_tags.len()),
                str::to_string,
            );
            ensure_unique_order_id_tag(&existing_order_id_tags, &order_id_tag)?;

            let base_id = strategy_registration_id::<T>(strategy);
            let strategy_id =
                StrategyId::from(format!("{}-{order_id_tag}", base_strategy_id(&base_id)));
            StrategyNative::strategy_core_mut(strategy).change_id(strategy_id);
            strategy_id
        };

        if self.strategy_ids.contains(&strategy_id) {
            anyhow::bail!("Strategy {strategy_id} is already registered");
        }

        Ok(strategy_id)
    }

    /// Adds a strategy to the trader.
    ///
    /// Strategies are registered in both the component registry (for lifecycle management)
    /// and the actor registry (for data callbacks via msgbus). The strategy's `StrategyCore`
    /// is also registered with the portfolio for order management.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The trader is not in a valid state for adding components.
    /// - A strategy with the same ID is already registered.
    pub fn add_strategy<T>(&mut self, mut strategy: T) -> anyhow::Result<()>
    where
        T: Strategy + StrategyNative + DataActorNative + Component + Debug + 'static,
    {
        self.validate_actor_or_strategy_registration()?;

        let strategy_id = self.prepare_strategy_for_registration(&mut strategy)?;

        let component_id = strategy.component_id();
        let clock = self.create_component_clock(component_id);

        // Register strategy core with portfolio for order management
        StrategyNative::strategy_core_mut(&mut strategy).register(
            self.trader_id,
            clock.clone(),
            self.cache.clone(),
            self.portfolio.clone(),
        )?;

        // Register default time event handler for this strategy
        let actor_id = strategy.actor_id().inner();
        let callback = TimeEventCallback::from(move |event: TimeEvent| {
            if let Some(mut actor) = try_get_actor_unchecked::<T>(&actor_id) {
                actor.handle_time_event(&event);
            } else {
                log::error!("Strategy {actor_id} not found for time event handling");
            }
        });
        clock.borrow_mut().register_default_handler(callback);

        // Transition to Ready state
        strategy.initialize()?;

        // Register in both component and actor registries
        register_component_actor(strategy);

        let order_topic = get_event_order_topic(strategy_id);
        let order_actor_id = actor_id;
        let order_handler = TypedHandler::from(move |event: &OrderEventAny| {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&order_actor_id) {
                strategy.handle_order_event(event.clone());
            } else {
                log::error!("Strategy {order_actor_id} not found for order event handling");
            }
        });
        let order_handler_id = order_handler.id();
        msgbus::subscribe_order_events(order_topic.into(), order_handler, None);

        let position_topic = get_event_position_topic(strategy_id);
        let position_handler = TypedHandler::from(move |event: &PositionEvent| {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&actor_id) {
                strategy.handle_position_event(event.clone());
            } else {
                log::error!("Strategy {actor_id} not found for position event handling");
            }
        });
        let position_handler_id = position_handler.id();
        msgbus::subscribe_position_events(position_topic.into(), position_handler, None);

        let control_actor_id = actor_id;
        let control_handler = TypedHandler::from(move |command: &StrategyCommand| {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&control_actor_id) {
                match command {
                    StrategyCommand::ExitMarket => {
                        if let Err(e) = strategy.market_exit() {
                            log::error!(
                                "Error handling strategy command for {control_actor_id}: {e}"
                            );
                        }
                    }
                }
            } else {
                log::error!("Strategy {control_actor_id} not found for control handling");
            }
        });
        get_message_bus()
            .borrow_mut()
            .endpoint_map::<StrategyCommand>()
            .register(strategy_control_endpoint(strategy_id), control_handler);

        self.strategy_ids.push(strategy_id);
        self.strategy_state_callbacks.insert(
            strategy_id,
            ComponentStateCallbacks {
                load: Self::load_component_state::<T>,
                save: Self::save_component_state::<T>,
            },
        );
        self.strategy_handler_ids
            .insert(strategy_id, (order_handler_id, position_handler_id));

        let stop_actor_id = actor_id;
        let stop_fn = Box::new(move || -> bool {
            if let Some(mut strategy) = try_get_actor_unchecked::<T>(&stop_actor_id) {
                Strategy::stop(&mut *strategy)
            } else {
                log::error!("Strategy {stop_actor_id} not found for stop");
                true // Proceed with component stop anyway
            }
        });
        self.strategy_stop_fns.insert(strategy_id, stop_fn);

        log::info!(
            "Registered strategy {strategy_id} with trader {}",
            self.trader_id
        );

        Ok(())
    }

    /// Adds an execution algorithm to the trader.
    ///
    /// Execution algorithms are registered in both the component registry (for lifecycle
    /// management) and the actor registry (for data callbacks via msgbus).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The trader is not in a valid state for adding components.
    /// - An execution algorithm with the same ID is already registered.
    pub fn add_exec_algorithm<T>(&mut self, mut exec_algorithm: T) -> anyhow::Result<()>
    where
        T: ExecutionAlgorithm + ExecutionAlgorithmNative + Component + Debug + 'static,
    {
        self.validate_exec_algorithm_registration()?;

        let exec_algorithm_id =
            ExecAlgorithmId::from(exec_algorithm.component_id().inner().as_str());

        if self.exec_algorithm_ids.contains(&exec_algorithm_id) {
            anyhow::bail!("Execution algorithm '{exec_algorithm_id}' is already registered");
        }

        let component_id = exec_algorithm.component_id();
        let clock = self.create_component_clock(component_id);

        exec_algorithm.register(self.trader_id, clock, self.cache.clone())?;
        exec_algorithm
            .exec_algorithm_core_mut()
            .set_portfolio(self.portfolio.clone());

        register_component_actor(exec_algorithm);

        // Register the {id}.execute endpoint so the order manager can
        // route TradingCommands to this algorithm via msgbus::send_any
        let actor_id = exec_algorithm_id.inner();
        let restore_actor_id = actor_id;
        let restore_fn: ExecAlgorithmSubscriptionFn = Box::new(move || {
            let Some(mut algo) = try_get_actor_unchecked::<T>(&restore_actor_id) else {
                anyhow::bail!(
                    "Execution algorithm {restore_actor_id} not found while restoring subscriptions"
                );
            };

            let mut strategy_ids = {
                let cache = algo.exec_algorithm_core_mut().cache_ref();
                cache
                    .orders_for_exec_algorithm(&exec_algorithm_id, None, None, None, None, None)
                    .into_iter()
                    .filter(|order| {
                        !order.is_closed() && order.exec_algorithm_id() == Some(exec_algorithm_id)
                    })
                    .map(|order| order.strategy_id())
                    .collect::<Vec<_>>()
            };
            strategy_ids.sort_unstable();
            strategy_ids.dedup();

            for strategy_id in strategy_ids {
                algo.subscribe_to_strategy_events(strategy_id);
            }

            Ok(())
        });
        let cleanup_actor_id = actor_id;
        let cleanup_fn: ExecAlgorithmSubscriptionFn = Box::new(move || {
            let Some(mut algo) = try_get_actor_unchecked::<T>(&cleanup_actor_id) else {
                anyhow::bail!(
                    "Execution algorithm {cleanup_actor_id} not found while cleaning subscriptions"
                );
            };
            algo.unsubscribe_all_strategy_events();
            Ok(())
        });
        let endpoint: Ustr = format!("{exec_algorithm_id}.execute").into();
        let handler = ShareableMessageHandler::from_typed(move |command: &TradingCommand| {
            if let Some(mut algo) = try_get_actor_unchecked::<T>(&actor_id) {
                if let Err(e) = algo.execute(command.clone()) {
                    log::error!("Error executing command on algorithm {actor_id}: {e}");
                }
            } else {
                log::error!("Execution algorithm {actor_id} not found in registry");
            }
        });
        msgbus::register_any(endpoint.into(), handler);

        self.exec_algorithm_ids.push(exec_algorithm_id);
        self.exec_algorithm_restore_fns
            .insert(exec_algorithm_id, restore_fn);
        self.exec_algorithm_cleanup_fns
            .insert(exec_algorithm_id, cleanup_fn);

        log::info!(
            "Registered execution algorithm {exec_algorithm_id} with trader {}",
            self.trader_id
        );

        Ok(())
    }

    /// Validates that the trader is in a valid state for actor and strategy registration.
    ///
    /// Actors and strategies can be added while the trader is `PreInitialized`, `Ready`,
    /// `Stopped`, or `Running`. This enables the [`Controller`](crate::controller::Controller)
    /// to add them at runtime.
    pub(crate) fn validate_actor_or_strategy_registration(&self) -> anyhow::Result<()> {
        match self.state {
            ComponentState::PreInitialized
            | ComponentState::Ready
            | ComponentState::Starting
            | ComponentState::Stopped
            | ComponentState::Running => Ok(()),
            ComponentState::Disposed => {
                anyhow::bail!("Cannot add components to disposed trader")
            }
            _ => anyhow::bail!("Cannot add components in current state: {}", self.state),
        }
    }

    /// Validates that the trader is in a valid state for execution algorithm registration.
    pub(crate) fn validate_exec_algorithm_registration(&self) -> anyhow::Result<()> {
        match self.state {
            ComponentState::PreInitialized | ComponentState::Ready | ComponentState::Stopped => {
                Ok(())
            }
            ComponentState::Running => {
                anyhow::bail!("Cannot add execution algorithms to running trader")
            }
            ComponentState::Disposed => {
                anyhow::bail!("Cannot add components to disposed trader")
            }
            _ => anyhow::bail!(
                "Cannot add execution algorithms in current state: {}",
                self.state
            ),
        }
    }

    /// Starts all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to start.
    pub fn start_components(&mut self) -> anyhow::Result<()> {
        let actor_ids = self.actor_ids.clone();
        let strategy_ids = self.strategy_ids.clone();
        let exec_algorithm_ids = self.exec_algorithm_ids.clone();

        for actor_id in actor_ids {
            log::debug!("Starting actor {actor_id}");
            Self::start_component_if_not_running(actor_id.inner())?;
        }

        for strategy_id in strategy_ids {
            log::debug!("Starting strategy {strategy_id}");
            Self::start_component_if_not_running(strategy_id.inner())?;
        }

        let mut restored_exec_algorithm_ids = Vec::new();

        for exec_algorithm_id in exec_algorithm_ids {
            log::debug!("Starting execution algorithm {exec_algorithm_id}");
            match self.start_exec_algorithm_if_not_running(exec_algorithm_id) {
                Ok(true) => restored_exec_algorithm_ids.push(exec_algorithm_id),
                Ok(false) => {}
                Err(start_err) => {
                    return Err(self.exec_algorithm_start_error_with_rollback(
                        exec_algorithm_id,
                        &restored_exec_algorithm_ids,
                        start_err,
                    ));
                }
            }
        }

        Ok(())
    }

    /// Starts the trader while releasing the trader borrow before component callbacks run.
    ///
    /// # Errors
    ///
    /// Returns an error if the trader state transition or any component startup fails.
    pub fn start_with_component_callbacks(trader: &Rc<RefCell<Self>>) -> anyhow::Result<()> {
        trader
            .borrow_mut()
            .transition_state(ComponentTrigger::Start)?;

        let (actor_ids, strategy_ids, exec_algorithm_ids) = {
            let trader_ref = trader.borrow();
            (
                trader_ref.actor_ids.clone(),
                trader_ref.strategy_ids.clone(),
                trader_ref.exec_algorithm_ids.clone(),
            )
        };

        for actor_id in actor_ids {
            log::debug!("Starting actor {actor_id}");
            Self::start_component_if_not_running(actor_id.inner())?;
        }

        for strategy_id in strategy_ids {
            log::debug!("Starting strategy {strategy_id}");
            Self::start_component_if_not_running(strategy_id.inner())?;
        }

        let mut restored_exec_algorithm_ids = Vec::new();

        for exec_algorithm_id in exec_algorithm_ids {
            log::debug!("Starting execution algorithm {exec_algorithm_id}");
            let component_state = match component_state(&exec_algorithm_id.inner()) {
                Ok(state) => state,
                Err(start_err) => {
                    let e = trader
                        .borrow_mut()
                        .exec_algorithm_start_error_with_rollback(
                            exec_algorithm_id,
                            &restored_exec_algorithm_ids,
                            start_err,
                        );
                    return Err(e);
                }
            };

            if component_state == ComponentState::Running {
                continue;
            }

            if let Err(start_err) = trader
                .borrow_mut()
                .restore_exec_algorithm_subscriptions(exec_algorithm_id)
            {
                let e = trader
                    .borrow_mut()
                    .exec_algorithm_start_error_with_rollback(
                        exec_algorithm_id,
                        &restored_exec_algorithm_ids,
                        start_err,
                    );
                return Err(e);
            }
            restored_exec_algorithm_ids.push(exec_algorithm_id);

            if let Err(start_err) = start_component(&exec_algorithm_id.inner()) {
                let e = trader
                    .borrow_mut()
                    .exec_algorithm_start_error_with_rollback(
                        exec_algorithm_id,
                        &restored_exec_algorithm_ids,
                        start_err,
                    );
                return Err(e);
            }
        }

        let mut trader_ref = trader.borrow_mut();
        let clock = trader_ref.clock_factory.clock();
        trader_ref.ts_started = Some(clock.borrow().timestamp_ns());
        trader_ref.transition_state(ComponentTrigger::StartCompleted)?;

        Ok(())
    }

    fn start_component_if_not_running(component_id: Ustr) -> anyhow::Result<()> {
        if component_state(&component_id)? == ComponentState::Running {
            return Ok(());
        }

        start_component(&component_id)
    }

    fn start_exec_algorithm_if_not_running(
        &mut self,
        exec_algorithm_id: ExecAlgorithmId,
    ) -> anyhow::Result<bool> {
        if component_state(&exec_algorithm_id.inner())? == ComponentState::Running {
            return Ok(false);
        }

        self.restore_exec_algorithm_subscriptions(exec_algorithm_id)?;
        if let Err(start_err) = start_component(&exec_algorithm_id.inner()) {
            return match self.cleanup_exec_algorithm_subscriptions(exec_algorithm_id) {
                Ok(()) => Err(start_err),
                Err(cleanup_err) => anyhow::bail!(
                    "Failed to start execution algorithm {exec_algorithm_id}: {start_err:#}; \
                     failed to roll back subscriptions: {cleanup_err:#}"
                ),
            };
        }

        Ok(true)
    }

    fn restore_exec_algorithm_subscriptions(
        &mut self,
        exec_algorithm_id: ExecAlgorithmId,
    ) -> anyhow::Result<()> {
        if let Some(restore_fn) = self.exec_algorithm_restore_fns.get_mut(&exec_algorithm_id) {
            restore_fn()?;
        }
        Ok(())
    }

    fn cleanup_exec_algorithm_subscriptions(
        &mut self,
        exec_algorithm_id: ExecAlgorithmId,
    ) -> anyhow::Result<()> {
        if let Some(cleanup_fn) = self.exec_algorithm_cleanup_fns.get_mut(&exec_algorithm_id) {
            cleanup_fn()?;
        }
        Ok(())
    }

    fn cleanup_exec_algorithm_subscriptions_for(
        &mut self,
        exec_algorithm_ids: &[ExecAlgorithmId],
    ) -> anyhow::Result<()> {
        let mut errors = Vec::new();

        for exec_algorithm_id in exec_algorithm_ids {
            if let Err(e) = self.cleanup_exec_algorithm_subscriptions(*exec_algorithm_id) {
                errors.push(format!("{exec_algorithm_id}: {e:#}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("{}", errors.join("; "))
        }
    }

    fn exec_algorithm_start_error_with_rollback(
        &mut self,
        exec_algorithm_id: ExecAlgorithmId,
        restored_exec_algorithm_ids: &[ExecAlgorithmId],
        start_err: anyhow::Error,
    ) -> anyhow::Error {
        match self.cleanup_exec_algorithm_subscriptions_for(restored_exec_algorithm_ids) {
            Ok(()) => start_err,
            Err(cleanup_err) => anyhow::anyhow!(
                "Failed while starting execution algorithm {exec_algorithm_id}: {start_err:#}; \
                 failed to roll back restored subscriptions: {cleanup_err:#}"
            ),
        }
    }

    /// Stops all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to stop.
    pub fn stop_components(&mut self) -> anyhow::Result<()> {
        for actor_id in &self.actor_ids {
            log::debug!("Stopping actor {actor_id}");
            Self::stop_component_if_active(actor_id.inner())?;
        }

        for exec_algorithm_id in &self.exec_algorithm_ids {
            log::debug!("Stopping execution algorithm {exec_algorithm_id}");
            Self::stop_component_if_active(exec_algorithm_id.inner())?;
        }

        for strategy_id in self.strategy_ids.clone() {
            log::debug!("Stopping strategy {strategy_id}");
            let should_proceed = self
                .strategy_stop_fns
                .get_mut(&strategy_id)
                .is_none_or(|stop_fn| stop_fn());

            if should_proceed {
                Self::stop_component_if_active(strategy_id.inner())?;
            }
        }

        Ok(())
    }

    /// Stops a partially started trader without deferring managed strategy shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if the trader transition or any component stop fails. All registered
    /// components still receive a stop attempt before the error is returned.
    pub fn stop_after_start_failure(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Stop)?;

        let stop_result = self.stop_components_after_start_failure();
        let clock = self.clock_factory.clock();
        self.ts_stopped = Some(clock.borrow().timestamp_ns());
        let transition_result = self.transition_state(ComponentTrigger::StopCompleted);

        match (stop_result, transition_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(stop_err), Ok(())) => Err(stop_err),
            (Ok(()), Err(transition_err)) => Err(transition_err),
            (Err(stop_err), Err(transition_err)) => anyhow::bail!(
                "Failed to stop trader components: {stop_err}; failed to complete trader stop: \
                 {transition_err}"
            ),
        }
    }

    fn stop_components_after_start_failure(&mut self) -> anyhow::Result<()> {
        let mut errors = Vec::new();

        for actor_id in &self.actor_ids {
            log::debug!("Stopping actor {actor_id} after startup failure");
            if let Err(e) = Self::stop_component_if_active(actor_id.inner()) {
                errors.push(format!("actor {actor_id}: {e:#}"));
            }
        }

        for exec_algorithm_id in self.exec_algorithm_ids.clone() {
            log::debug!("Stopping execution algorithm {exec_algorithm_id} after startup failure");
            if let Err(e) = Self::stop_component_if_active(exec_algorithm_id.inner()) {
                errors.push(format!("execution algorithm {exec_algorithm_id}: {e:#}"));
            }

            if let Err(e) = self.cleanup_exec_algorithm_subscriptions(exec_algorithm_id) {
                errors.push(format!(
                    "execution algorithm {exec_algorithm_id} subscription cleanup: {e:#}"
                ));
            }
        }

        for strategy_id in &self.strategy_ids {
            log::debug!("Stopping strategy {strategy_id} after startup failure");
            if let Err(e) = Self::stop_component_if_active(strategy_id.inner()) {
                errors.push(format!("strategy {strategy_id}: {e:#}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "Failed to stop one or more trader components after startup failure: {}",
                errors.join("; ")
            )
        }
    }

    fn stop_component_if_active(component_id: Ustr) -> anyhow::Result<()> {
        if !matches!(
            component_state(&component_id)?,
            ComponentState::Starting | ComponentState::Running
        ) {
            return Ok(());
        }

        stop_component(&component_id)
    }

    /// Resets all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to reset.
    pub fn reset_components(&mut self) -> anyhow::Result<()> {
        for actor_id in &self.actor_ids {
            log::debug!("Resetting actor {actor_id}");
            reset_component(&actor_id.inner())?;
        }

        for strategy_id in &self.strategy_ids {
            log::debug!("Resetting strategy {strategy_id}");
            reset_component(&strategy_id.inner())?;
        }

        for exec_algorithm_id in self.exec_algorithm_ids.clone() {
            log::debug!("Resetting execution algorithm {exec_algorithm_id}");
            self.cleanup_exec_algorithm_subscriptions(exec_algorithm_id)?;
            reset_component(&exec_algorithm_id.inner())?;
        }

        Ok(())
    }

    /// Disposes of all registered components.
    ///
    /// # Errors
    ///
    /// Returns an error if any component fails to dispose.
    pub fn dispose_components(&mut self) -> anyhow::Result<()> {
        for actor_id in self.actor_ids.clone() {
            log::debug!("Disposing actor {actor_id}");
            self.retire_actor(actor_id)?;
        }

        for strategy_id in self.strategy_ids.clone() {
            log::debug!("Disposing strategy {strategy_id}");
            self.retire_strategy(strategy_id)?;
        }

        for exec_algorithm_id in self.exec_algorithm_ids.clone() {
            log::debug!("Disposing execution algorithm {exec_algorithm_id}");
            self.retire_exec_algorithm(exec_algorithm_id)?;
        }

        // Clocks created for components which never completed registration
        for clock in self.clocks.values() {
            clock.borrow_mut().cancel_timers();
        }
        self.clocks.clear();

        Ok(())
    }

    /// Clears all registered strategies, disposing each and removing their clocks.
    ///
    /// # Errors
    ///
    /// Returns an error if any strategy fails to dispose.
    pub fn clear_strategies(&mut self) -> anyhow::Result<()> {
        for strategy_id in self.strategy_ids.clone() {
            log::debug!("Disposing strategy {strategy_id}");
            self.retire_strategy(strategy_id)?;
        }

        Ok(())
    }

    /// Clears all registered actors, disposing each and removing their clocks.
    ///
    /// # Errors
    ///
    /// Returns an error if any actor fails to dispose.
    pub fn clear_actors(&mut self) -> anyhow::Result<()> {
        for actor_id in self.actor_ids.clone() {
            log::debug!("Disposing actor {actor_id}");
            // Stop if running before disposal; ignore stop failures so a single
            // misbehaving actor does not leave the rest in a half-cleared state.
            let _ = stop_component(&actor_id.inner());
            self.retire_actor(actor_id)?;
        }

        Ok(())
    }

    /// Clears all registered execution algorithms, disposing each and removing their clocks.
    ///
    /// # Errors
    ///
    /// Returns an error if any execution algorithm fails to dispose.
    pub fn clear_exec_algorithms(&mut self) -> anyhow::Result<()> {
        for exec_algorithm_id in self.exec_algorithm_ids.clone() {
            log::debug!("Disposing execution algorithm {exec_algorithm_id}");
            self.retire_exec_algorithm(exec_algorithm_id)?;
        }

        Ok(())
    }

    // -- Individual component management ----------------------------------------

    /// Starts the actor with the given `actor_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is not registered or cannot be started.
    pub fn start_actor(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        if !self.actor_ids.contains(actor_id) {
            anyhow::bail!("Cannot start actor, {actor_id} not found");
        }
        start_component(&actor_id.inner())
    }

    /// Stops the actor with the given `actor_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is not registered or cannot be stopped.
    pub fn stop_actor(&self, actor_id: &ActorId) -> anyhow::Result<()> {
        if !self.actor_ids.contains(actor_id) {
            anyhow::bail!("Cannot stop actor, {actor_id} not found");
        }
        stop_component(&actor_id.inner())
    }

    /// Removes the actor with the given `actor_id`.
    ///
    /// Will stop the actor first if it is currently running. Disposes the actor
    /// and removes it from the trader's tracking.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is not registered, or if disposal fails. A failed disposal
    /// keeps the actor registered and tracked, and leaves it `Faulted`; see [`Component::dispose`].
    /// Calling this again retires the actor.
    pub fn remove_actor(&mut self, actor_id: &ActorId) -> anyhow::Result<()> {
        if !self.actor_ids.contains(actor_id) {
            anyhow::bail!("Cannot remove actor, {actor_id} not found");
        }

        // Stop if running, then dispose
        let _ = stop_component(&actor_id.inner());
        self.retire_actor(*actor_id)?;

        log::info!("Removed actor {actor_id} from trader {}", self.trader_id);
        Ok(())
    }

    /// Starts the strategy with the given `strategy_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or cannot be started.
    pub fn start_strategy(&self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        if !self.strategy_ids.contains(strategy_id) {
            anyhow::bail!("Cannot start strategy, {strategy_id} not found");
        }
        start_component(&strategy_id.inner())
    }

    /// Stops the strategy with the given `strategy_id`.
    ///
    /// Respects the `manage_stop` behavior - if the strategy's stop function
    /// returns `false`, the component stop is deferred until market exit completes.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or cannot be stopped.
    pub fn stop_strategy(&mut self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        if !self.strategy_ids.contains(strategy_id) {
            anyhow::bail!("Cannot stop strategy, {strategy_id} not found");
        }

        let should_proceed = self
            .strategy_stop_fns
            .get_mut(strategy_id)
            .is_none_or(|stop_fn| stop_fn());

        if should_proceed {
            stop_component(&strategy_id.inner())?;
        }

        Ok(())
    }

    /// Exits the market for the strategy with the given `strategy_id`.
    ///
    /// Sends a strategy command to the strategy's control endpoint. The strategy
    /// then performs its own managed market exit.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered or its control endpoint is missing.
    pub fn market_exit_strategy(
        trader: &Rc<RefCell<Self>>,
        strategy_id: &StrategyId,
    ) -> anyhow::Result<()> {
        let handler = trader.borrow().strategy_command_handler(*strategy_id)?;
        handler.handle(&StrategyCommand::ExitMarket);
        Ok(())
    }

    fn strategy_command_handler(
        &self,
        strategy_id: StrategyId,
    ) -> anyhow::Result<TypedHandler<StrategyCommand>> {
        if !self.strategy_ids.contains(&strategy_id) {
            anyhow::bail!("Cannot market exit strategy, {strategy_id} not found");
        }

        let endpoint = strategy_control_endpoint(strategy_id);
        let handler = {
            let msgbus = get_message_bus();
            msgbus
                .borrow_mut()
                .endpoint_map::<StrategyCommand>()
                .get(endpoint)
                .cloned()
        };

        let Some(handler) = handler else {
            anyhow::bail!(
                "Cannot exit market for strategy {strategy_id}: control endpoint '{}' not registered",
                endpoint.as_str()
            );
        };

        Ok(handler)
    }

    /// Removes the strategy with the given `strategy_id`.
    ///
    /// Will stop the strategy first if it is currently running. Disposes the strategy
    /// and removes it from the trader's tracking along with its event subscriptions.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy is not registered, or if disposal fails. A failed disposal
    /// keeps the strategy registered and tracked, and leaves it `Faulted`; see
    /// [`Component::dispose`]. Calling this again retires the strategy.
    pub fn remove_strategy(&mut self, strategy_id: &StrategyId) -> anyhow::Result<()> {
        if !self.strategy_ids.contains(strategy_id) {
            anyhow::bail!("Cannot remove strategy, {strategy_id} not found");
        }

        // Stop if running, then dispose
        let _ = stop_component(&strategy_id.inner());
        self.retire_strategy(*strategy_id)?;

        log::info!(
            "Removed strategy {strategy_id} from trader {}",
            self.trader_id
        );
        Ok(())
    }

    /// Disposes an actor, then releases everything its registration created.
    ///
    /// Each component is retired completely before the next one starts, so a failure part way
    /// through a bulk operation leaves the trader's bookkeeping consistent with the registries.
    fn retire_actor(&mut self, actor_id: ActorId) -> anyhow::Result<()> {
        Self::dispose_registered_component(actor_id.inner())?;

        self.release_component(ComponentId::from(actor_id));
        self.actor_ids.retain(|id| id != &actor_id);
        self.actor_state_callbacks.remove(&actor_id);

        Ok(())
    }

    /// Disposes a strategy, then releases everything its registration created.
    fn retire_strategy(&mut self, strategy_id: StrategyId) -> anyhow::Result<()> {
        Self::dispose_registered_component(strategy_id.inner())?;

        self.remove_strategy_subscriptions(strategy_id);
        self.release_component(ComponentId::from(strategy_id));
        self.strategy_ids.retain(|id| id != &strategy_id);
        self.strategy_state_callbacks.remove(&strategy_id);
        self.strategy_stop_fns.remove(&strategy_id);

        Ok(())
    }

    /// Disposes an execution algorithm, then releases everything its registration created.
    fn retire_exec_algorithm(&mut self, exec_algorithm_id: ExecAlgorithmId) -> anyhow::Result<()> {
        Self::dispose_registered_component(exec_algorithm_id.inner())?;
        self.cleanup_exec_algorithm_subscriptions(exec_algorithm_id)?;

        let endpoint: Ustr = format!("{exec_algorithm_id}.execute").into();
        msgbus::deregister_any(endpoint.into());
        self.release_component(ComponentId::from(exec_algorithm_id));
        self.exec_algorithm_ids
            .retain(|id| id != &exec_algorithm_id);
        self.exec_algorithm_restore_fns.remove(&exec_algorithm_id);
        self.exec_algorithm_cleanup_fns.remove(&exec_algorithm_id);

        Ok(())
    }

    /// Disposes the component `id` unless it has already reached a terminal state.
    ///
    /// A component disposed from Python has already run `on_dispose`, so a second disposal
    /// transition would fail and strand the trader's bookkeeping. A `Faulted` component has
    /// released its subscriptions on every route into that state, through either
    /// [`Component::dispose`] or [`Component::fault`], so it is retirable without a further
    /// transition.
    fn dispose_registered_component(id: Ustr) -> anyhow::Result<()> {
        let state = component_state(&id)?;

        if matches!(state, ComponentState::Disposed | ComponentState::Faulted) {
            log::debug!("Component {id} already {state}, skipping disposal transition");
            return Ok(());
        }

        dispose_component(&id)
    }

    /// Removes the msgbus registrations the trader installed for `strategy_id`.
    fn remove_strategy_subscriptions(&mut self, strategy_id: StrategyId) {
        if let Some((order_handler_id, position_handler_id)) =
            self.strategy_handler_ids.remove(&strategy_id)
        {
            let order_topic = get_event_order_topic(strategy_id);
            let position_topic = get_event_position_topic(strategy_id);
            msgbus::remove_order_event_handler(order_topic.into(), order_handler_id);
            msgbus::remove_position_event_handler(position_topic.into(), position_handler_id);
        }

        get_message_bus()
            .borrow_mut()
            .endpoint_map::<StrategyCommand>()
            .deregister(strategy_control_endpoint(strategy_id));
    }

    /// Releases the clock, registry entries, and Python wrapper registered for a component.
    ///
    /// Called once a component has disposed successfully, to retire a `Faulted` component, or to
    /// roll back a failed registration. A failed disposal does not reach here on the attempt that
    /// failed, so the component stays registered and reachable for inspection or retry; a later
    /// attempt retires it through the `Faulted` route.
    ///
    /// A rollback only removes what the failed attempt created, because the Python registration
    /// path rejects a component ID this trader already tracks before it mutates anything.
    pub(crate) fn release_component(&mut self, component_id: ComponentId) {
        if let Some(clock) = self.clocks.shift_remove(&component_id) {
            let mut clock = clock.borrow_mut();
            clock.cancel_timers();
            clock.cancel_default_handler();
            clock.cancel_callbacks();
        }

        let id = component_id.inner();
        deregister_component(&id);
        deregister_actor(&id);

        // Runs last because dropping the wrapper can trigger Python finalization which re-enters
        // Rust, and by then nothing is registered
        #[cfg(feature = "python")]
        release_python_wrapper(component_id);
    }

    // -- Lifecycle management ---------------------------------------------------

    /// Loads persisted actor and strategy state in registration order.
    ///
    /// Empty state and a cache without database backing do not invoke component callbacks.
    ///
    /// # Errors
    ///
    /// Returns an error if state cannot be loaded or a component callback fails.
    pub(crate) fn load_state(trader: &Rc<RefCell<Self>>) -> anyhow::Result<()> {
        let (cache, actor_callbacks, strategy_callbacks) = {
            let trader = trader.borrow();
            let actor_callbacks = trader.actor_state_callbacks()?;
            let strategy_callbacks = trader.strategy_state_callbacks()?;

            (trader.cache.clone(), actor_callbacks, strategy_callbacks)
        };

        if !cache.borrow().has_backing() {
            return Ok(());
        }

        for (actor_id, callbacks) in actor_callbacks {
            let state = cache
                .borrow()
                .load_actor_state(&actor_id)
                .map_err(|e| anyhow::anyhow!("Failed to load actor {actor_id} state: {e:#}"))?;
            let Some(state) = state.filter(|state| !state.is_empty()) else {
                continue;
            };

            (callbacks.load)(actor_id.inner(), state)
                .map_err(|e| anyhow::anyhow!("Failed to restore actor {actor_id} state: {e:#}"))?;
        }

        for (strategy_id, callbacks) in strategy_callbacks {
            let state = cache
                .borrow()
                .load_strategy_state(&strategy_id)
                .map_err(|e| {
                    anyhow::anyhow!("Failed to load strategy {strategy_id} state: {e:#}")
                })?;
            let Some(state) = state.filter(|state| !state.is_empty()) else {
                continue;
            };

            (callbacks.load)(strategy_id.inner(), state).map_err(|e| {
                anyhow::anyhow!("Failed to restore strategy {strategy_id} state: {e:#}")
            })?;
        }

        Ok(())
    }

    /// Saves actor and strategy state in registration order.
    ///
    /// Empty state is persisted, while a cache without database backing does not invoke
    /// component callbacks. All callbacks and updates receive an attempt before errors return.
    ///
    /// # Errors
    ///
    /// Returns an error containing every component callback or persistence failure.
    pub(crate) fn save_state(trader: &Rc<RefCell<Self>>) -> anyhow::Result<()> {
        let (cache, actor_callbacks, strategy_callbacks) = {
            let trader = trader.borrow();
            let actor_callbacks = trader.actor_state_callbacks()?;
            let strategy_callbacks = trader.strategy_state_callbacks()?;

            (trader.cache.clone(), actor_callbacks, strategy_callbacks)
        };

        if !cache.borrow().has_backing() {
            return Ok(());
        }

        let mut errors = Vec::new();

        for (actor_id, callbacks) in actor_callbacks {
            match (callbacks.save)(actor_id.inner()) {
                Ok(state) => {
                    if let Err(e) = cache.borrow().update_actor_state(&actor_id, &state) {
                        errors.push(format!("actor {actor_id} persistence: {e:#}"));
                    }
                }
                Err(e) => errors.push(format!("actor {actor_id} callback: {e:#}")),
            }
        }

        for (strategy_id, callbacks) in strategy_callbacks {
            match (callbacks.save)(strategy_id.inner()) {
                Ok(state) => {
                    if let Err(e) = cache.borrow().update_strategy_state(&strategy_id, &state) {
                        errors.push(format!("strategy {strategy_id} persistence: {e:#}"));
                    }
                }
                Err(e) => errors.push(format!("strategy {strategy_id} callback: {e:#}")),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Failed to save component state: {}", errors.join("; "))
        }
    }

    fn actor_state_callbacks(&self) -> anyhow::Result<Vec<(ActorId, ComponentStateCallbacks)>> {
        self.actor_ids
            .iter()
            .map(|actor_id| {
                self.actor_state_callbacks
                    .get(actor_id)
                    .copied()
                    .map(|callbacks| (*actor_id, callbacks))
                    .ok_or_else(|| anyhow::anyhow!("Actor {actor_id} state callback not found"))
            })
            .collect()
    }

    fn strategy_state_callbacks(
        &self,
    ) -> anyhow::Result<Vec<(StrategyId, ComponentStateCallbacks)>> {
        self.strategy_ids
            .iter()
            .map(|strategy_id| {
                self.strategy_state_callbacks
                    .get(strategy_id)
                    .copied()
                    .map(|callbacks| (*strategy_id, callbacks))
                    .ok_or_else(|| {
                        anyhow::anyhow!("Strategy {strategy_id} state callback not found")
                    })
            })
            .collect()
    }

    fn load_component_state<T>(
        component_id: Ustr,
        state: PersistedComponentState,
    ) -> anyhow::Result<()>
    where
        T: DataActor + DataActorNative + Debug + 'static,
    {
        let mut component = try_get_actor_unchecked::<T>(&component_id).ok_or_else(|| {
            anyhow::anyhow!("Component {component_id} not found in actor registry")
        })?;
        component.on_load(state)
    }

    fn save_component_state<T>(component_id: Ustr) -> anyhow::Result<PersistedComponentState>
    where
        T: DataActor + DataActorNative + Debug + 'static,
    {
        let component = try_get_actor_unchecked::<T>(&component_id).ok_or_else(|| {
            anyhow::anyhow!("Component {component_id} not found in actor registry")
        })?;
        component.on_save()
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
        let clock = self.clock_factory.clock();
        self.ts_started = Some(clock.borrow().timestamp_ns());

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.stop_components()?;

        let clock = self.clock_factory.clock();
        self.ts_stopped = Some(clock.borrow().timestamp_ns());

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

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
        sync::Arc,
    };

    #[cfg(feature = "python")]
    use nautilus_common::{
        actor::data_actor::ImportableActorConfig,
        python::{
            actor::{PyDataActor, PyDataActorInner},
            wrappers::get_python_wrapper,
        },
    };
    use nautilus_common::{
        actor::{
            DataActorCore,
            data_actor::DataActorConfig,
            registry::{actor_exists, get_actor_unchecked, try_get_actor_unchecked},
        },
        cache::Cache,
        clock::TestClock,
        component::get_component,
        enums::{ComponentState, Environment},
        messages::execution::SubmitOrder,
        msgbus,
        msgbus::{
            MessageBus, MessagingSwitchboard, TypedHandler, set_message_bus,
            switchboard::{
                get_bars_topic, get_book_deltas_topic, get_book_depth10_topic, get_custom_topic,
                get_event_order_topic,
            },
        },
        nautilus_actor,
        runner::{
            SyncTradingCommandSender, drain_trading_cmd_queue, replace_exec_cmd_sender,
            trading_cmd_queue_is_empty,
        },
    };
    use nautilus_core::UUID4;
    use nautilus_data::engine::{DataEngine, config::DataEngineConfig};
    use nautilus_execution::engine::{ExecutionEngine, config::ExecutionEngineConfig};
    use nautilus_model::{
        data::{Bar, DataType, stubs::stub_bar},
        enums::{BookType, OrderSide, OrderStatus, OrderType, PositionAdjustmentType},
        events::{
            OrderAccepted, OrderDenied, OrderFilled, OrderRejected, OrderUpdated, PositionAdjusted,
            order::spec::{
                OrderAcceptedSpec, OrderFilledSpec, OrderRejectedSpec, OrderSubmittedSpec,
                OrderUpdatedSpec,
            },
        },
        identifiers::{
            AccountId, ActorId, ClientOrderId, ComponentId, InstrumentId, PositionId, TraderId,
            VenueOrderId,
        },
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::{OrderAny, OrderTestBuilder},
        stubs::TestDefault,
        types::Quantity,
    };
    use nautilus_portfolio::portfolio::Portfolio;
    use nautilus_risk::engine::{RiskEngine, config::RiskEngineConfig};
    #[cfg(feature = "python")]
    use nautilus_testkit::cache::TestCacheDatabaseControl;
    #[cfg(feature = "python")]
    use nautilus_trading::python::strategy::{PyStrategy, PyStrategyInner};
    use nautilus_trading::{
        ExecutionAlgorithmConfig, ExecutionAlgorithmCore, StrategyNative,
        nautilus_execution_algorithm, nautilus_strategy,
        strategy::{config::StrategyConfig, core::StrategyCore},
    };
    #[cfg(feature = "python")]
    use pyo3::{
        ffi::c_str,
        prelude::*,
        types::{PyDict, PyModule},
    };
    use rstest::rstest;

    use super::*;
    use crate::clock_factory::ClockFactory;

    // Simple DataActor wrapper for testing
    #[derive(Debug)]
    struct TestDataActor {
        core: DataActorCore,
        fail_dispose: bool,
        bars_received: usize,
    }

    impl TestDataActor {
        fn new(config: DataActorConfig) -> Self {
            Self {
                core: DataActorCore::new(config),
                fail_dispose: false,
                bars_received: 0,
            }
        }
    }

    impl DataActor for TestDataActor {
        fn on_dispose(&mut self) -> anyhow::Result<()> {
            if self.fail_dispose {
                anyhow::bail!("test actor dispose failure");
            }
            Ok(())
        }

        fn on_bar(&mut self, _bar: &Bar) -> anyhow::Result<()> {
            self.bars_received += 1;
            Ok(())
        }
    }

    nautilus_actor!(TestDataActor);

    // Simple ExecutionAlgorithm wrapper for testing
    #[derive(Debug)]
    struct TestExecAlgorithm {
        core: ExecutionAlgorithmCore,
        fail_start: bool,
        submit_on_accept: Option<OrderAny>,
        accepted_events: usize,
        denied_events: usize,
        rejected_events: usize,
        updated_events: usize,
        filled_events: usize,
        position_events: usize,
    }

    impl TestExecAlgorithm {
        fn new(config: ExecutionAlgorithmConfig) -> Self {
            Self {
                core: ExecutionAlgorithmCore::new(config),
                fail_start: false,
                submit_on_accept: None,
                accepted_events: 0,
                denied_events: 0,
                rejected_events: 0,
                updated_events: 0,
                filled_events: 0,
                position_events: 0,
            }
        }
    }

    impl DataActor for TestExecAlgorithm {
        fn on_start(&mut self) -> anyhow::Result<()> {
            if self.fail_start {
                anyhow::bail!("test execution algorithm start failure");
            }
            Ok(())
        }
    }

    nautilus_execution_algorithm!(TestExecAlgorithm, {
        fn on_order(&mut self, _order: OrderAny) -> anyhow::Result<()> {
            Ok(())
        }

        fn on_order_rejected(&mut self, _event: OrderRejected) {
            self.rejected_events += 1;
        }

        fn on_order_accepted(&mut self, _event: OrderAccepted) {
            self.accepted_events += 1;

            if let Some(order) = self.submit_on_accept.take() {
                self.submit_order(order, None, None).unwrap();
            }
        }

        fn on_order_denied(&mut self, _event: OrderDenied) {
            self.denied_events += 1;
        }

        fn on_order_updated(&mut self, _event: OrderUpdated) {
            self.updated_events += 1;
        }

        fn on_algo_order_filled(&mut self, _event: OrderFilled) {
            self.filled_events += 1;
        }

        fn on_position_event(&mut self, _event: PositionEvent) {
            self.position_events += 1;
        }
    });

    fn add_cached_exec_order(
        cache: &Rc<RefCell<Cache>>,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        exec_algorithm_id: Option<ExecAlgorithmId>,
        is_terminal: bool,
    ) -> OrderAny {
        let mut builder = OrderTestBuilder::new(OrderType::Market);
        builder
            .client_order_id(client_order_id)
            .strategy_id(strategy_id)
            .instrument_id(InstrumentId::test_default())
            .quantity(Quantity::from(1));

        if let Some(exec_algorithm_id) = exec_algorithm_id {
            builder
                .exec_algorithm_id(exec_algorithm_id)
                .exec_spawn_id(client_order_id);
        }

        let order = builder.build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, None, false)
            .unwrap();

        if is_terminal {
            let event = OrderEventAny::Rejected(
                OrderRejectedSpec::builder()
                    .trader_id(order.trader_id())
                    .strategy_id(order.strategy_id())
                    .instrument_id(order.instrument_id())
                    .client_order_id(order.client_order_id())
                    .account_id(AccountId::test_default())
                    .reason("TEST_TERMINAL".into())
                    .build(),
            );
            cache.borrow_mut().update_order(&event).unwrap();
        }

        order
    }

    // Simple Strategy wrapper for testing
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

    #[expect(clippy::type_complexity)]
    fn create_trader_components() -> (
        Rc<RefCell<MessageBus>>,
        Rc<RefCell<Cache>>,
        Rc<RefCell<Portfolio>>,
        Rc<RefCell<DataEngine>>,
        Rc<RefCell<RiskEngine>>,
        Rc<RefCell<ExecutionEngine>>,
        ClockFactory,
    ) {
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
        let cache = Rc::new(RefCell::new(Cache::new(None, None)));
        let portfolio = Rc::new(RefCell::new(Portfolio::new(
            clock.clone(),
            cache.clone(),
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
            risk_clock.clone() as Rc<RefCell<dyn Clock>>,
            risk_cache.clone(),
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
            clock_factory,
        )
    }

    #[rstest]
    fn test_trader_creation() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

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
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::from("TRADER-001");
        let instance_id = UUID4::new();

        let trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        assert_eq!(
            trader.component_id(),
            ComponentId::from("Trader-TRADER-001")
        );
    }

    #[rstest]
    fn test_add_actor_success() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

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
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

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
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("Test-Strategy")),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);

        let result = trader.add_strategy(strategy);
        assert!(result.is_ok());
        assert_eq!(trader.strategy_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(
            trader
                .strategy_ids()
                .contains(&StrategyId::from("Test-Strategy"))
        );
    }

    #[rstest]
    fn test_add_strategy_rejects_order_id_tag_with_separator() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("HyphenTagStrategy-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let mut strategy = TestStrategy::new(config);
        StrategyNative::strategy_core_mut(&mut strategy)
            .config
            .order_id_tag = Some("A-B".to_string());

        let error = trader.add_strategy(strategy).unwrap_err();

        assert_eq!(
            error.to_string(),
            "`order_id_tag` cannot contain the '-' strategy ID separator, was 'A-B'"
        );
        assert_eq!(trader.strategy_count(), 0);
        assert_eq!(trader.component_count(), 0);
    }

    #[rstest]
    fn test_add_strategy_preserves_explicit_instrument_strategy_id() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let strategy_id = StrategyId::from("ExampleStrategy-XNAS");
        let config = StrategyConfig {
            strategy_id: Some(strategy_id),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);

        trader.add_strategy(strategy).unwrap();

        let mut registered = get_actor_unchecked::<TestStrategy>(&strategy_id.inner());
        let (client_order_id, order_list_id) = {
            let mut order_factory = registered.order_factory();
            (
                order_factory.generate_client_order_id(),
                order_factory.generate_order_list_id(),
            )
        };

        assert_eq!(trader.strategy_ids(), vec![strategy_id]);
        assert_eq!(registered.strategy_id(), Some(strategy_id));
        assert!(client_order_id.as_str().ends_with("-001-XNAS-1"));
        assert!(order_list_id.as_str().ends_with("-001-XNAS-1"));
    }

    #[rstest]
    fn test_add_strategy_appends_configured_order_id_tag_to_explicit_strategy_id() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let strategy_id = StrategyId::from("ExampleStrategy-XNAS");
        let runtime_strategy_id = StrategyId::from("ExampleStrategy-XNAS-T01");
        let config = StrategyConfig {
            strategy_id: Some(strategy_id),
            order_id_tag: Some("T01".to_string()),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);

        trader.add_strategy(strategy).unwrap();

        assert!(try_get_actor_unchecked::<TestStrategy>(&strategy_id.inner()).is_none());

        let mut registered = get_actor_unchecked::<TestStrategy>(&runtime_strategy_id.inner());
        let (client_order_id, order_list_id) = {
            let mut order_factory = registered.order_factory();
            (
                order_factory.generate_client_order_id(),
                order_factory.generate_order_list_id(),
            )
        };

        assert_eq!(trader.strategy_ids(), vec![runtime_strategy_id]);
        assert_eq!(registered.strategy_id(), Some(runtime_strategy_id));
        assert!(client_order_id.as_str().ends_with("-001-T01-1"));
        assert!(order_list_id.as_str().ends_with("-001-T01-1"));
    }

    #[rstest]
    fn test_add_strategies_with_no_order_id_tags_assigns_unique_tags() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let strategy1 = TestStrategy::new(StrategyConfig::default());
        let strategy2 = TestStrategy::new(StrategyConfig::default());

        assert!(trader.add_strategy(strategy1).is_ok());
        assert!(trader.add_strategy(strategy2).is_ok());
        assert_eq!(
            trader.strategy_ids(),
            vec![
                StrategyId::from("TestStrategy-000"),
                StrategyId::from("TestStrategy-001")
            ]
        );
    }

    #[rstest]
    fn test_prepare_strategy_for_registration_is_idempotent() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let mut strategy = TestStrategy::new(StrategyConfig::default());

        let prepared_id = trader
            .prepare_strategy_for_registration(&mut strategy)
            .unwrap();
        assert_eq!(prepared_id, StrategyId::from("TestStrategy-000"));
        let core = StrategyNative::strategy_core(&strategy);
        assert_eq!(core.config.strategy_id, None);
        assert_eq!(core.config.order_id_tag, None);
        assert_eq!(core.strategy_id(), Some(prepared_id));
        assert_eq!(core.order_id_tag(), Some("000"));

        assert!(trader.add_strategy(strategy).is_ok());
        assert_eq!(trader.strategy_ids(), vec![prepared_id]);
    }

    #[rstest]
    fn test_add_strategy_with_duplicate_order_id_tag_fails() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = StrategyConfig {
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        let strategy1 = TestStrategy::new(config.clone());
        let strategy2 = TestStrategy::new(config);

        assert!(trader.add_strategy(strategy1).is_ok());
        assert_eq!(
            trader.strategy_ids(),
            vec![StrategyId::from("TestStrategy-001")]
        );

        let result = trader.add_strategy(strategy2);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("order_id_tag conflict")
        );
    }

    #[rstest]
    fn test_add_strategy_id_with_subscriptions_duplicate_order_id_tag_fails() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        assert!(
            trader
                .add_strategy_id_with_subscriptions::<TestStrategy>(StrategyId::from("Foo-001"))
                .is_ok()
        );

        let result =
            trader.add_strategy_id_with_subscriptions::<TestStrategy>(StrategyId::from("Bar-001"));

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("order_id_tag conflict")
        );
        assert_eq!(trader.strategy_ids(), vec![StrategyId::from("Foo-001")]);
    }

    #[rstest]
    fn test_add_strategy_with_mismatched_strategy_id_and_order_id_tag_appends_tag() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("TestStrategy-001")),
            order_id_tag: Some("002".to_string()),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);

        assert!(trader.add_strategy(strategy).is_ok());
        assert_eq!(
            trader.strategy_ids(),
            vec![StrategyId::from("TestStrategy-001-002")]
        );
    }

    #[rstest]
    fn test_add_exec_algorithm_success() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::from("TestExecAlgorithm")),
            ..Default::default()
        };
        let exec_algorithm = TestExecAlgorithm::new(config);
        let exec_algorithm_id = exec_algorithm.id();

        let result = trader.add_exec_algorithm(exec_algorithm);
        assert!(result.is_ok());
        assert_eq!(trader.exec_algorithm_count(), 1);
        assert_eq!(trader.component_count(), 1);
        assert!(trader.exec_algorithm_ids().contains(&exec_algorithm_id));
    }

    #[rstest]
    fn test_exec_algorithm_submit_from_order_event_defers_risk_denial() {
        std::thread::spawn(|| {
            msgbus::get_message_bus().borrow_mut().dispose();
            replace_exec_cmd_sender(Arc::new(SyncTradingCommandSender));

            let trader_id = TraderId::test_default();
            let instance_id = UUID4::new();
            let strategy_id = StrategyId::from("Callback-001");
            let exec_algorithm_id = ExecAlgorithmId::from("CALLBACK");
            let account_id = AccountId::from("SIM-001");
            let venue_order_id = VenueOrderId::from("V-PRIMARY-001");
            let parent_order_id = ClientOrderId::from("O-PRIMARY-001");
            let child_order_id = ClientOrderId::from("O-CHILD-001");
            let clock_factory = ClockFactory::test_default();
            let clock = clock_factory.clock();
            let msgbus = Rc::new(RefCell::new(MessageBus::new(
                trader_id,
                instance_id,
                Some("test".to_string()),
                None,
            )));
            set_message_bus(msgbus);

            let cache = Rc::new(RefCell::new(Cache::default()));
            let instrument = InstrumentAny::CurrencyPair(audusd_sim());
            let instrument_id = instrument.id();
            cache.borrow_mut().add_instrument(instrument).unwrap();
            let portfolio = Rc::new(RefCell::new(Portfolio::new(
                clock.clone(),
                cache.clone(),
                None,
            )));
            let risk_engine = Rc::new(RefCell::new(RiskEngine::new(
                RiskEngineConfig::default(),
                portfolio.borrow().clone_shallow(),
                clock.clone(),
                cache.clone(),
            )));
            let exec_engine = Rc::new(RefCell::new(ExecutionEngine::new(
                clock.clone(),
                cache.clone(),
                Some(ExecutionEngineConfig::default()),
            )));
            RiskEngine::register_msgbus_handlers(&risk_engine);
            ExecutionEngine::register_msgbus_handlers(&exec_engine);

            let parent = OrderTestBuilder::new(OrderType::Market)
                .trader_id(trader_id)
                .strategy_id(strategy_id)
                .instrument_id(instrument_id)
                .client_order_id(parent_order_id)
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1000"))
                .exec_algorithm_id(exec_algorithm_id)
                .exec_spawn_id(parent_order_id)
                .build();
            let child = OrderTestBuilder::new(OrderType::Market)
                .trader_id(trader_id)
                .strategy_id(strategy_id)
                .instrument_id(instrument_id)
                .client_order_id(child_order_id)
                .side(OrderSide::NoOrderSide)
                .quantity(Quantity::from("100"))
                .exec_algorithm_id(exec_algorithm_id)
                .exec_spawn_id(child_order_id)
                .build();

            let config = ExecutionAlgorithmConfig {
                exec_algorithm_id: Some(exec_algorithm_id),
                ..Default::default()
            };
            let mut exec_algorithm = TestExecAlgorithm::new(config);
            exec_algorithm.submit_on_accept = Some(child);
            let mut trader = Trader::new(
                trader_id,
                instance_id,
                Environment::Backtest,
                clock_factory,
                cache.clone(),
                portfolio,
            );
            trader.add_exec_algorithm(exec_algorithm).unwrap();
            trader.start_components().unwrap();

            cache
                .borrow_mut()
                .add_order(parent.clone(), None, None, false)
                .unwrap();
            let submit = SubmitOrder::new(
                trader_id,
                None,
                strategy_id,
                instrument_id,
                parent.client_order_id(),
                parent.init_event().clone(),
                Some(exec_algorithm_id),
                None,
                None,
                UUID4::new(),
                clock.borrow().timestamp_ns(),
                None,
            );
            get_actor_unchecked::<TestExecAlgorithm>(&exec_algorithm_id.inner())
                .execute(TradingCommand::SubmitOrder(submit))
                .unwrap();

            let submitted = OrderEventAny::Submitted(
                OrderSubmittedSpec::builder()
                    .trader_id(trader_id)
                    .strategy_id(strategy_id)
                    .instrument_id(instrument_id)
                    .client_order_id(parent.client_order_id())
                    .account_id(account_id)
                    .build(),
            );
            let accepted = OrderEventAny::Accepted(
                OrderAcceptedSpec::builder()
                    .trader_id(trader_id)
                    .strategy_id(strategy_id)
                    .instrument_id(instrument_id)
                    .client_order_id(parent.client_order_id())
                    .venue_order_id(venue_order_id)
                    .account_id(account_id)
                    .build(),
            );
            msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), submitted);
            msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), accepted);

            {
                let cache = cache.borrow();
                let parent = cache.order(&parent.client_order_id()).unwrap();
                let child = cache.order(&child_order_id).unwrap();
                let exec_algorithm =
                    get_actor_unchecked::<TestExecAlgorithm>(&exec_algorithm_id.inner());

                assert!(!trading_cmd_queue_is_empty());
                assert_eq!(risk_engine.borrow().command_count(), 0);
                assert_eq!(exec_engine.borrow().event_count(), 2);
                assert_eq!(parent.status(), OrderStatus::Accepted);
                assert_eq!(parent.event_count(), 3);
                assert_eq!(child.status(), OrderStatus::Initialized);
                assert_eq!(child.event_count(), 1);
                assert_eq!(exec_algorithm.accepted_events, 1);
                assert_eq!(exec_algorithm.denied_events, 0);
                assert!(exec_algorithm.submit_on_accept.is_none());
            }

            drain_trading_cmd_queue();

            {
                let cache = cache.borrow();
                let parent = cache.order(&parent.client_order_id()).unwrap();
                let child = cache.order(&child_order_id).unwrap();
                let exec_algorithm =
                    get_actor_unchecked::<TestExecAlgorithm>(&exec_algorithm_id.inner());

                assert!(trading_cmd_queue_is_empty());
                assert_eq!(risk_engine.borrow().command_count(), 1);
                assert_eq!(exec_engine.borrow().event_count(), 3);
                assert_eq!(parent.status(), OrderStatus::Accepted);
                assert_eq!(parent.event_count(), 3);
                assert_eq!(child.status(), OrderStatus::Denied);
                assert_eq!(child.event_count(), 2);
                assert_eq!(
                    child.last_event().message(),
                    Some("INVALID_ORDER_SIDE: NO_ORDER_SIDE".into())
                );
                assert_eq!(exec_algorithm.accepted_events, 1);
                assert_eq!(exec_algorithm.denied_events, 1);
                assert!(exec_algorithm.submit_on_accept.is_none());
            }
        })
        .join()
        .unwrap();
    }

    #[rstest]
    fn test_exec_algorithm_restores_cached_strategy_subscriptions_on_start_and_restart() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();
        let unique = UUID4::new();
        let exec_algorithm_id = ExecAlgorithmId::from(format!("RECOVERY-{unique}"));
        let other_algorithm_id = ExecAlgorithmId::from(format!("OTHER-{unique}"));
        let strategy_a = StrategyId::from(format!("RecoveryA-{unique}"));
        let strategy_b = StrategyId::from(format!("RecoveryB-{unique}"));
        let terminal_strategy = StrategyId::from(format!("Terminal-{unique}"));
        let external_strategy = StrategyId::external();

        let order_a = add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-A1-{unique}")),
            strategy_a,
            Some(exec_algorithm_id),
            false,
        );
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-A2-{unique}")),
            strategy_a,
            Some(exec_algorithm_id),
            false,
        );
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-B-{unique}")),
            strategy_b,
            Some(exec_algorithm_id),
            false,
        );
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-TERMINAL-{unique}")),
            terminal_strategy,
            Some(exec_algorithm_id),
            true,
        );
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-OTHER-{unique}")),
            StrategyId::from(format!("Other-{unique}")),
            Some(other_algorithm_id),
            false,
        );
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-EXTERNAL-{unique}")),
            external_strategy,
            None,
            false,
        );

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );
        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(exec_algorithm_id),
            ..Default::default()
        };
        trader
            .add_exec_algorithm(TestExecAlgorithm::new(config))
            .unwrap();

        trader.start_components().unwrap();

        assert_eq!(order_a.exec_spawn_id(), Some(order_a.client_order_id()));
        {
            let registered = get_actor_unchecked::<TestExecAlgorithm>(&exec_algorithm_id.inner());
            assert!(registered.core.is_strategy_subscribed(&strategy_a));
            assert!(registered.core.is_strategy_subscribed(&strategy_b));
            assert!(!registered.core.is_strategy_subscribed(&terminal_strategy));
            assert!(!registered.core.is_strategy_subscribed(&external_strategy));
        }

        let rejected = OrderEventAny::Rejected(
            OrderRejectedSpec::builder()
                .trader_id(order_a.trader_id())
                .strategy_id(strategy_a)
                .instrument_id(order_a.instrument_id())
                .client_order_id(order_a.client_order_id())
                .account_id(AccountId::test_default())
                .reason("TEST_REJECTED".into())
                .build(),
        );
        let updated = OrderEventAny::Updated(
            OrderUpdatedSpec::builder()
                .trader_id(order_a.trader_id())
                .strategy_id(strategy_a)
                .instrument_id(order_a.instrument_id())
                .client_order_id(order_a.client_order_id())
                .build(),
        );
        let filled = OrderEventAny::Filled(
            OrderFilledSpec::builder()
                .trader_id(order_a.trader_id())
                .strategy_id(strategy_a)
                .instrument_id(order_a.instrument_id())
                .client_order_id(order_a.client_order_id())
                .build(),
        );
        let position = PositionEvent::PositionAdjusted(PositionAdjusted::new(
            trader_id,
            strategy_a,
            InstrumentId::test_default(),
            PositionId::from(format!("P-{unique}")),
            AccountId::test_default(),
            PositionAdjustmentType::Funding,
            None,
            None,
            None,
            UUID4::new(),
            0.into(),
            0.into(),
        ));

        let order_topic = format!("events.order.{strategy_a}");
        msgbus::publish_order_event(order_topic.clone().into(), &rejected);
        msgbus::publish_order_event(order_topic.clone().into(), &updated);
        msgbus::publish_order_event(order_topic.into(), &filled);
        msgbus::publish_position_event(format!("events.position.{strategy_a}").into(), &position);

        {
            let registered = get_actor_unchecked::<TestExecAlgorithm>(&exec_algorithm_id.inner());
            assert_eq!(registered.rejected_events, 1);
            assert_eq!(registered.updated_events, 1);
            assert_eq!(registered.filled_events, 1);
            assert_eq!(registered.position_events, 1);
        }

        trader.stop_components().unwrap();
        trader.reset_components().unwrap();
        {
            let registered = get_actor_unchecked::<TestExecAlgorithm>(&exec_algorithm_id.inner());
            assert!(!registered.core.is_strategy_subscribed(&strategy_a));
            assert!(!registered.core.is_strategy_subscribed(&strategy_b));
        }

        trader.start_components().unwrap();
        {
            let registered = get_actor_unchecked::<TestExecAlgorithm>(&exec_algorithm_id.inner());
            assert!(registered.core.is_strategy_subscribed(&strategy_a));
            assert!(registered.core.is_strategy_subscribed(&strategy_b));
        }

        trader.stop_components().unwrap();
        trader.clear_exec_algorithms().unwrap();
        assert!(trader.exec_algorithm_restore_fns.is_empty());
        assert!(trader.exec_algorithm_cleanup_fns.is_empty());
    }

    #[rstest]
    fn test_exec_algorithm_start_failure_cleans_all_restored_subscriptions() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();
        let unique = UUID4::new();
        let running_algorithm_id = ExecAlgorithmId::from(format!("RUNNING-{unique}"));
        let failing_algorithm_id = ExecAlgorithmId::from(format!("FAIL-{unique}"));
        let running_strategy_id = StrategyId::from(format!("Running-{unique}"));
        let failing_strategy_id = StrategyId::from(format!("Failing-{unique}"));
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-RUNNING-{unique}")),
            running_strategy_id,
            Some(running_algorithm_id),
            false,
        );
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-FAILING-{unique}")),
            failing_strategy_id,
            Some(failing_algorithm_id),
            false,
        );

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );
        let running_config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(running_algorithm_id),
            ..Default::default()
        };
        trader
            .add_exec_algorithm(TestExecAlgorithm::new(running_config))
            .unwrap();
        let failing_config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(failing_algorithm_id),
            ..Default::default()
        };
        let mut failing_algorithm = TestExecAlgorithm::new(failing_config);
        failing_algorithm.fail_start = true;
        trader.add_exec_algorithm(failing_algorithm).unwrap();
        trader.initialize().unwrap();
        let trader = Rc::new(RefCell::new(trader));

        let error = Trader::start_with_component_callbacks(&trader).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("test execution algorithm start failure")
        );
        {
            let running = get_actor_unchecked::<TestExecAlgorithm>(&running_algorithm_id.inner());
            let failing = get_actor_unchecked::<TestExecAlgorithm>(&failing_algorithm_id.inner());
            assert!(!running.core.is_strategy_subscribed(&running_strategy_id));
            assert!(!failing.core.is_strategy_subscribed(&failing_strategy_id));
        }

        trader.borrow_mut().stop_after_start_failure().unwrap();

        let running = get_actor_unchecked::<TestExecAlgorithm>(&running_algorithm_id.inner());
        assert!(!running.core.is_strategy_subscribed(&running_strategy_id));
    }

    #[rstest]
    fn test_start_components_failure_cleans_previously_restored_subscriptions() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();
        let unique = UUID4::new();
        let running_algorithm_id = ExecAlgorithmId::from(format!("DIRECT-RUNNING-{unique}"));
        let failing_algorithm_id = ExecAlgorithmId::from(format!("DIRECT-FAIL-{unique}"));
        let running_strategy_id = StrategyId::from(format!("DirectRunning-{unique}"));
        let failing_strategy_id = StrategyId::from(format!("DirectFailing-{unique}"));
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-DIRECT-RUNNING-{unique}")),
            running_strategy_id,
            Some(running_algorithm_id),
            false,
        );
        add_cached_exec_order(
            &cache,
            ClientOrderId::from(format!("O-DIRECT-FAILING-{unique}")),
            failing_strategy_id,
            Some(failing_algorithm_id),
            false,
        );

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );
        trader
            .add_exec_algorithm(TestExecAlgorithm::new(ExecutionAlgorithmConfig {
                exec_algorithm_id: Some(running_algorithm_id),
                ..Default::default()
            }))
            .unwrap();
        let mut failing_algorithm = TestExecAlgorithm::new(ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(failing_algorithm_id),
            ..Default::default()
        });
        failing_algorithm.fail_start = true;
        trader.add_exec_algorithm(failing_algorithm).unwrap();

        let error = trader.start_components().unwrap_err();

        assert!(
            error
                .to_string()
                .contains("test execution algorithm start failure")
        );
        let running = get_actor_unchecked::<TestExecAlgorithm>(&running_algorithm_id.inner());
        let failing = get_actor_unchecked::<TestExecAlgorithm>(&failing_algorithm_id.inner());
        assert!(!running.core.is_strategy_subscribed(&running_strategy_id));
        assert!(!failing.core.is_strategy_subscribed(&failing_strategy_id));
    }

    #[rstest]
    fn test_cannot_add_exec_algorithm_while_running() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );
        trader.state = ComponentState::Running;

        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::from("TestExecAlgorithm")),
            ..Default::default()
        };
        let exec_algorithm = TestExecAlgorithm::new(config);

        let result = trader.add_exec_algorithm(exec_algorithm);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Cannot add execution algorithms to running trader"
        );
        assert_eq!(trader.exec_algorithm_count(), 0);
    }

    #[rstest]
    fn test_component_lifecycle() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        // Add components
        let actor = TestDataActor::new(DataActorConfig::default());

        let strategy_config = StrategyConfig {
            strategy_id: Some(StrategyId::from("Test-Strategy")),
            ..Default::default()
        };
        let strategy = TestStrategy::new(strategy_config);

        let exec_algorithm_config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::from("TestExecAlgorithm")),
            ..Default::default()
        };
        let exec_algorithm = TestExecAlgorithm::new(exec_algorithm_config);

        assert!(trader.add_actor(actor).is_ok());
        assert!(trader.add_strategy(strategy).is_ok());
        assert!(trader.add_exec_algorithm(exec_algorithm).is_ok());
        assert_eq!(trader.component_count(), 3);

        // Test start components
        let start_result = trader.start_components();
        assert!(start_result.is_ok(), "{:?}", start_result.unwrap_err());

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
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

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
    fn test_market_exit_strategy_fails_when_control_endpoint_missing() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("Test-Strategy")),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);
        trader.add_strategy(strategy).unwrap();

        let strategy_id = StrategyId::from("Test-Strategy");
        let endpoint = strategy_control_endpoint(strategy_id);
        assert!(
            get_message_bus()
                .borrow_mut()
                .endpoint_map::<StrategyCommand>()
                .is_registered(endpoint)
        );
        get_message_bus()
            .borrow_mut()
            .endpoint_map::<StrategyCommand>()
            .deregister(endpoint);

        let trader = Rc::new(RefCell::new(trader));
        let result = Trader::market_exit_strategy(&trader, &strategy_id);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Cannot exit market for strategy {strategy_id}: control endpoint '{}' not registered",
                endpoint.as_str()
            )
        );
    }

    #[rstest]
    fn test_remove_strategy_deregisters_strategy_endpoint() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("Test-Strategy")),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);
        trader.add_strategy(strategy).unwrap();

        let strategy_id = StrategyId::from("Test-Strategy");
        let endpoint = strategy_control_endpoint(strategy_id);
        assert!(
            get_message_bus()
                .borrow_mut()
                .endpoint_map::<StrategyCommand>()
                .is_registered(endpoint)
        );

        trader.remove_strategy(&strategy_id).unwrap();

        assert!(
            !get_message_bus()
                .borrow_mut()
                .endpoint_map::<StrategyCommand>()
                .is_registered(endpoint)
        );
    }

    #[rstest]
    fn test_can_add_components_while_running() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        // Simulate running state
        trader.state = ComponentState::Running;

        let actor = TestDataActor::new(DataActorConfig::default());
        let result = trader.add_actor(actor);
        assert!(result.is_ok());
        assert_eq!(trader.actor_count(), 1);
    }

    #[rstest]
    fn test_cannot_add_components_while_disposed() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        // Simulate disposed state
        trader.state = ComponentState::Disposed;

        let actor = TestDataActor::new(DataActorConfig::default());
        let result = trader.add_actor(actor);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("disposed trader"));
    }

    #[rstest]
    fn test_create_component_clock_backtest_creates_individual_clocks() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory.clone(),
            cache,
            portfolio,
        );

        let component_a = ComponentId::new("ACTOR-A");
        let component_b = ComponentId::new("ACTOR-B");
        let clock_a = trader.create_component_clock(component_a);
        let clock_b = trader.create_component_clock(component_b);
        let primary_clock = clock_factory.clock();

        // Each component gets its own clock instance
        assert_ne!(
            clock_a.as_ptr() as *const _,
            primary_clock.as_ptr() as *const _
        );
        assert_ne!(clock_a.as_ptr() as *const _, clock_b.as_ptr() as *const _);
    }

    #[rstest]
    fn test_get_component_clocks_returns_registration_order() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );
        let mut registered = Vec::new();

        for index in 0..32 {
            let component_id = ComponentId::new(format!("ACTOR-{index:02}").as_str());
            registered.push(trader.create_component_clock(component_id));
        }

        let returned = trader.get_component_clocks();
        assert_eq!(returned.len(), registered.len());
        for (actual, expected) in returned.iter().zip(&registered) {
            assert!(Rc::ptr_eq(actual, expected));
        }
    }

    #[rstest]
    fn test_create_component_clock_live_uses_factory_with_distinct_instances() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, _clock_factory) =
            create_trader_components();
        let calls = Rc::new(Cell::new(0usize));
        let calls_in_closure = calls.clone();
        let clock_factory = ClockFactory::new(move || {
            calls_in_closure.set(calls_in_closure.get() + 1);
            Rc::new(RefCell::new(TestClock::new())) as Rc<RefCell<dyn Clock>>
        });

        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Sandbox,
            clock_factory,
            cache,
            portfolio,
        );

        let a = trader.create_component_clock(ComponentId::new("ACTOR-A"));
        let b = trader.create_component_clock(ComponentId::new("ACTOR-B"));

        assert_eq!(
            calls.get(),
            3,
            "factory invoked for primary clock and each component",
        );
        assert!(
            !Rc::ptr_eq(&a, &b),
            "each component must get its own clock instance"
        );
    }

    #[rstest]
    fn test_clear_strategies_preserves_other_handlers() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("Test-Strategy")),
            ..Default::default()
        };
        let strategy = TestStrategy::new(config);
        trader.add_strategy(strategy).unwrap();

        let strategy_id = StrategyId::from("Test-Strategy");
        let endpoint = strategy_control_endpoint(strategy_id);
        assert!(
            get_message_bus()
                .borrow_mut()
                .endpoint_map::<StrategyCommand>()
                .is_registered(endpoint)
        );

        // Simulate an exec algorithm subscribing to the same strategy topic
        let ext_received = Rc::new(RefCell::new(0));
        let ext_clone = ext_received.clone();
        let ext_handler =
            TypedHandler::from_with_id("exec-algo-handler", move |_: &OrderEventAny| {
                *ext_clone.borrow_mut() += 1;
            });
        let order_topic = get_event_order_topic(strategy_id);
        msgbus::subscribe_order_events(order_topic.into(), ext_handler, None);

        trader.clear_strategies().unwrap();
        assert_eq!(trader.strategy_count(), 0);
        assert!(
            !get_message_bus()
                .borrow_mut()
                .endpoint_map::<StrategyCommand>()
                .is_registered(endpoint)
        );

        let event = OrderEventAny::Accepted(OrderAccepted::test_default());
        msgbus::publish_order_event(order_topic, &event);
        assert_eq!(*ext_received.borrow(), 1);
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_python_actor_and_strategy_state_callbacks_use_registered_types() {
        pyo3::Python::initialize();

        Python::attach(|py| {
            py.run(
                c_str!(
                    r#"
class StateComponent:
    def __init__(self, state):
        self.state = state
        self.loaded = None
        self.calls = []

    def on_load(self, state):
        self.calls.append("on_load")
        self.loaded = dict(state)

    def on_save(self):
        self.calls.append("on_save")
        return self.state
"#
                ),
                None,
                None,
            )
            .unwrap();

            let component_class = py.eval(c_str!("StateComponent"), None, None).unwrap();
            let actor_save =
                IndexMap::from([("actor-save".to_string(), b"python-actor-saved".to_vec())]);
            let strategy_save = IndexMap::from([(
                "strategy-save".to_string(),
                b"python-strategy-saved".to_vec(),
            )]);
            let py_actor_state = PyDict::new(py);
            py_actor_state
                .set_item("actor-save", b"python-actor-saved")
                .unwrap();
            let py_strategy_state = PyDict::new(py);
            py_strategy_state
                .set_item("strategy-save", b"python-strategy-saved")
                .unwrap();
            let py_actor = component_class.call1((py_actor_state,)).unwrap().unbind();
            let py_strategy = component_class
                .call1((py_strategy_state,))
                .unwrap()
                .unbind();

            let actor_id = ActorId::from("PYTHON-STATE-ACTOR");
            let strategy_id = StrategyId::from("PYTHON-STATE-STRATEGY-001");
            let actor_load =
                IndexMap::from([("actor-load".to_string(), b"python-actor-loaded".to_vec())]);
            let strategy_load = IndexMap::from([(
                "strategy-load".to_string(),
                b"python-strategy-loaded".to_vec(),
            )]);
            let (database, control) = TestCacheDatabaseControl::create();
            control.set_actor_state(actor_id, &actor_load);
            control.set_strategy_state(strategy_id, &strategy_load);

            let (
                _msgbus,
                cache,
                portfolio,
                _data_engine,
                _risk_engine,
                _exec_engine,
                clock_factory,
            ) = create_trader_components();
            cache.borrow_mut().set_database(Box::new(database));
            let trader_id = TraderId::test_default();
            let mut trader = Trader::new(
                trader_id,
                UUID4::new(),
                Environment::Backtest,
                clock_factory,
                cache.clone(),
                portfolio.clone(),
            );

            let mut actor = PyDataActor::new(Some(DataActorConfig {
                actor_id: Some(actor_id),
                ..Default::default()
            }));
            actor.set_python_instance(py_actor.bind(py)).unwrap();
            let actor_clock = trader.create_component_clock(ComponentId::from(actor_id));
            actor
                .register(trader_id, actor_clock, cache.clone())
                .unwrap();
            actor.register_in_global_registries().unwrap();
            trader
                .add_actor_id_for_lifecycle::<PyDataActorInner>(actor_id)
                .unwrap();

            let mut strategy = PyStrategy::new(Some(StrategyConfig {
                strategy_id: Some(strategy_id),
                ..Default::default()
            }));
            strategy.set_python_instance(py_strategy.bind(py)).unwrap();
            let strategy_clock = trader.create_component_clock(ComponentId::from(strategy_id));
            strategy
                .register(trader_id, strategy_clock, cache, portfolio)
                .unwrap();
            strategy.register_in_global_registries().unwrap();
            trader
                .add_strategy_id_with_subscriptions::<PyStrategyInner>(strategy_id)
                .unwrap();

            let trader = Rc::new(RefCell::new(trader));
            Trader::load_state(&trader).unwrap();
            Trader::save_state(&trader).unwrap();

            let actor_loaded = py_actor
                .getattr(py, "loaded")
                .unwrap()
                .extract::<std::collections::HashMap<String, Vec<u8>>>(py)
                .unwrap();
            let strategy_loaded = py_strategy
                .getattr(py, "loaded")
                .unwrap()
                .extract::<std::collections::HashMap<String, Vec<u8>>>(py)
                .unwrap();
            let actor_calls = py_actor
                .getattr(py, "calls")
                .unwrap()
                .extract::<Vec<String>>(py)
                .unwrap();
            let strategy_calls = py_strategy
                .getattr(py, "calls")
                .unwrap()
                .extract::<Vec<String>>(py)
                .unwrap();

            assert_eq!(
                actor_loaded,
                std::collections::HashMap::from([(
                    "actor-load".to_string(),
                    b"python-actor-loaded".to_vec(),
                )])
            );
            assert_eq!(
                strategy_loaded,
                std::collections::HashMap::from([(
                    "strategy-load".to_string(),
                    b"python-strategy-loaded".to_vec(),
                )])
            );
            assert_eq!(actor_calls, vec!["on_load", "on_save"]);
            assert_eq!(strategy_calls, vec!["on_load", "on_save"]);
            assert_eq!(control.actor_state(&actor_id), Some(actor_save));
            assert_eq!(control.strategy_state(&strategy_id), Some(strategy_save));
        });
    }

    #[rstest]
    fn test_clear_actors_disposes_and_clears_state() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let trader_id = TraderId::test_default();
        let instance_id = UUID4::new();

        let mut trader = Trader::new(
            trader_id,
            instance_id,
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_a = TestDataActor::new(DataActorConfig {
            actor_id: Some(ActorId::from("Actor-A")),
            ..Default::default()
        });
        let actor_b = TestDataActor::new(DataActorConfig {
            actor_id: Some(ActorId::from("Actor-B")),
            ..Default::default()
        });
        trader.add_actor(actor_a).unwrap();
        trader.add_actor(actor_b).unwrap();
        assert_eq!(trader.actor_count(), 2);
        assert_eq!(
            trader.get_component_clocks().len(),
            2,
            "each registered actor must have a component clock",
        );

        trader.clear_actors().unwrap();

        assert_eq!(trader.actor_count(), 0);
        assert!(trader.actor_ids().is_empty());
        assert_eq!(
            trader.get_component_clocks().len(),
            0,
            "actor clocks must be dropped after clear_actors",
        );
    }

    #[rstest]
    fn test_remove_actor_deregisters_component() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let retired_id = ActorId::from("Retired-Actor");
        let retained_id = ActorId::from("Retained-Actor");
        trader
            .add_actor(TestDataActor::new(DataActorConfig {
                actor_id: Some(retired_id),
                ..Default::default()
            }))
            .unwrap();
        trader
            .add_actor(TestDataActor::new(DataActorConfig {
                actor_id: Some(retained_id),
                ..Default::default()
            }))
            .unwrap();

        trader.remove_actor(&retired_id).unwrap();

        assert!(get_component(&retired_id.inner()).is_none());
        assert!(!actor_exists(&retired_id.inner()));
        assert_eq!(trader.actor_ids(), vec![retained_id]);
        assert_eq!(trader.get_component_clocks().len(), 1);

        // Deregistration is exact: an unrelated component sharing the registry survives
        assert!(get_component(&retained_id.inner()).is_some());
        assert!(actor_exists(&retained_id.inner()));
    }

    #[rstest]
    fn test_remove_strategy_deregisters_component() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        trader
            .add_strategy(TestStrategy::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("Retired-001")),
                ..Default::default()
            }))
            .unwrap();
        trader
            .add_strategy(TestStrategy::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("Retained-002")),
                ..Default::default()
            }))
            .unwrap();

        let retired_id = StrategyId::from("Retired-001");
        let retained_id = StrategyId::from("Retained-002");

        // The control endpoint lives in the typed endpoint map, not the `register_any` endpoints
        let control_endpoint_registered = |strategy_id| {
            get_message_bus()
                .borrow_mut()
                .endpoint_map::<StrategyCommand>()
                .get(strategy_control_endpoint(strategy_id))
                .is_some()
        };
        assert!(control_endpoint_registered(retired_id));
        assert!(control_endpoint_registered(retained_id));

        trader.remove_strategy(&retired_id).unwrap();

        assert!(get_component(&retired_id.inner()).is_none());
        assert!(!actor_exists(&retired_id.inner()));
        assert!(!trader.strategy_handler_ids.contains_key(&retired_id));
        assert!(
            !control_endpoint_registered(retired_id),
            "the retired strategy control endpoint must be deregistered",
        );
        assert_eq!(trader.strategy_ids(), vec![retained_id]);

        assert!(get_component(&retained_id.inner()).is_some());
        assert!(actor_exists(&retained_id.inner()));
        assert!(trader.strategy_handler_ids.contains_key(&retained_id));
        assert!(
            control_endpoint_registered(retained_id),
            "deregistration must not remove an unrelated strategy's control endpoint",
        );
    }

    #[rstest]
    fn test_clear_exec_algorithms_deregisters_components() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let first_id = ExecAlgorithmId::from("EXEC-ALGO-1");
        let second_id = ExecAlgorithmId::from("EXEC-ALGO-2");
        for exec_algorithm_id in [first_id, second_id] {
            trader
                .add_exec_algorithm(TestExecAlgorithm::new(ExecutionAlgorithmConfig {
                    exec_algorithm_id: Some(exec_algorithm_id),
                    ..Default::default()
                }))
                .unwrap();
        }

        for exec_algorithm_id in [first_id, second_id] {
            assert!(
                msgbus::has_endpoint(&format!("{exec_algorithm_id}.execute")),
                "the execute endpoint must be registered before clearing",
            );
        }

        trader.clear_exec_algorithms().unwrap();

        for exec_algorithm_id in [first_id, second_id] {
            assert!(get_component(&exec_algorithm_id.inner()).is_none());
            assert!(!actor_exists(&exec_algorithm_id.inner()));
            assert!(!msgbus::has_endpoint(&format!(
                "{exec_algorithm_id}.execute"
            )));
        }
        assert!(trader.exec_algorithm_ids().is_empty());
        assert!(trader.get_component_clocks().is_empty());
    }

    #[rstest]
    fn test_failed_dispose_preserves_registration() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = ActorId::from("Failing-Dispose-Actor");
        let mut actor = TestDataActor::new(DataActorConfig {
            actor_id: Some(actor_id),
            ..Default::default()
        });
        actor.fail_dispose = true;
        trader.add_actor(actor).unwrap();

        let error = trader.remove_actor(&actor_id).unwrap_err();

        assert_eq!(error.to_string(), "test actor dispose failure");
        assert_eq!(trader.actor_ids(), vec![actor_id]);
        assert_eq!(trader.get_component_clocks().len(), 1);
        assert!(get_component(&actor_id.inner()).is_some());
        assert!(actor_exists(&actor_id.inner()));
        assert_eq!(
            component_state(&actor_id.inner()).unwrap(),
            ComponentState::Faulted,
            "a failed disposal faults the component rather than reaching Disposed",
        );
    }

    #[rstest]
    fn test_failed_dispose_component_can_be_retired() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = ActorId::from("Retired-After-Failed-Dispose-Actor");
        let mut actor = TestDataActor::new(DataActorConfig {
            actor_id: Some(actor_id),
            ..Default::default()
        });
        actor.fail_dispose = true;
        trader.add_actor(actor).unwrap();

        trader.remove_actor(&actor_id).unwrap_err();
        assert_eq!(
            component_state(&actor_id.inner()).unwrap(),
            ComponentState::Faulted
        );

        // The dead end this closes: retirement previously failed for the life of the process
        trader.remove_actor(&actor_id).unwrap();

        assert!(get_component(&actor_id.inner()).is_none());
        assert!(!actor_exists(&actor_id.inner()));
        assert!(trader.actor_ids().is_empty());
        assert!(trader.get_component_clocks().is_empty());
    }

    #[rstest]
    fn test_failed_dispose_releases_subscriptions() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = ActorId::from("Subscribed-Failing-Dispose-Actor");
        let mut actor = TestDataActor::new(DataActorConfig {
            actor_id: Some(actor_id),
            ..Default::default()
        });
        actor.fail_dispose = true;
        trader.add_actor(actor).unwrap();
        trader.start_actor(&actor_id).unwrap();

        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let deltas_topic = get_book_deltas_topic(instrument_id);
        get_actor_unchecked::<TestDataActor>(&actor_id.inner()).subscribe_book_deltas(
            instrument_id,
            BookType::L3_MBO,
            None,
            None,
            false,
            None,
        );

        // Positive control: without this the check after the failed disposal would be vacuous
        assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);

        trader.remove_actor(&actor_id).unwrap_err();

        // A failed disposal releases subscriptions even though it retains the registration
        assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
    }

    #[rstest]
    fn test_runtime_faulted_component_retires_without_leaking_subscriptions() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = ActorId::from("Runtime-Faulted-Actor");
        trader
            .add_actor(TestDataActor::new(DataActorConfig {
                actor_id: Some(actor_id),
                ..Default::default()
            }))
            .unwrap();
        trader.start_actor(&actor_id).unwrap();

        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let deltas_topic = get_book_deltas_topic(instrument_id);
        get_actor_unchecked::<TestDataActor>(&actor_id.inner()).subscribe_book_deltas(
            instrument_id,
            BookType::L3_MBO,
            None,
            None,
            false,
            None,
        );

        // Positive control: without this the check after retirement would be vacuous
        assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);

        // Faulting at runtime is a separate route to Faulted from a failed disposal, and
        // retirement skips disposal for a faulted component
        get_actor_unchecked::<TestDataActor>(&actor_id.inner())
            .fault()
            .unwrap();
        assert_eq!(
            component_state(&actor_id.inner()).unwrap(),
            ComponentState::Faulted
        );

        trader.remove_actor(&actor_id).unwrap();

        assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
        assert!(get_component(&actor_id.inner()).is_none());
        assert!(!actor_exists(&actor_id.inner()));
    }

    #[rstest]
    fn test_already_disposed_component_can_be_removed() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = ActorId::from("Directly-Disposed-Actor");
        trader
            .add_actor(TestDataActor::new(DataActorConfig {
                actor_id: Some(actor_id),
                ..Default::default()
            }))
            .unwrap();

        // Mirrors a Python caller invoking `dispose()` on its own component
        dispose_component(&actor_id.inner()).unwrap();
        assert_eq!(
            component_state(&actor_id.inner()).unwrap(),
            ComponentState::Disposed
        );

        trader.remove_actor(&actor_id).unwrap();

        assert!(get_component(&actor_id.inner()).is_none());
        assert!(!actor_exists(&actor_id.inner()));
        assert!(trader.actor_ids().is_empty());
        assert!(trader.get_component_clocks().is_empty());
    }

    #[cfg(feature = "python")]
    fn install_owned_actor_module(py: Python<'_>, module_name: &str) {
        let module = PyModule::new(py, module_name).expect("test module should create");
        module
            .setattr("DataActor", py.get_type::<PyDataActor>())
            .expect("DataActor type should bind");
        module
            .setattr("INSTANCES", PyDict::new(py))
            .expect("INSTANCES should bind");

        let code = std::ffi::CString::new(
            r#"
import weakref


class OwnedActor(DataActor):
    def __init__(self):
        super().__init__()
        INSTANCES["actor"] = weakref.ref(self)
"#,
        )
        .expect("python test code should be valid CString");

        py.run(code.as_c_str(), Some(&module.dict()), None)
            .expect("test actor code should execute");

        py.import("sys")
            .expect("sys should import")
            .getattr("modules")
            .expect("sys.modules should exist")
            .set_item(module_name, module)
            .expect("test actor module should register");
    }

    #[cfg(feature = "python")]
    fn owned_actor_is_alive(py: Python<'_>, module_name: &str) -> bool {
        !py.import(module_name)
            .expect("test actor module should import")
            .getattr("INSTANCES")
            .expect("INSTANCES should exist")
            .get_item("actor")
            .expect("the actor weak reference should be recorded")
            .call0()
            .expect("a weak reference should be callable")
            .is_none()
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_trader_owns_python_actor_wrapper_until_removal() {
        Python::initialize();

        let module_name = "test_trader_owned_actor";
        Python::attach(|py| install_owned_actor_module(py, module_name));

        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = trader
            .add_actor_from_importable_config(&ImportableActorConfig {
                actor_path: format!("{module_name}:OwnedActor"),
                config_path: String::new(),
                config: std::collections::HashMap::new(),
            })
            .unwrap();

        assert_eq!(actor_id, ActorId::from("OwnedActor"));
        assert!(
            Python::attach(|py| owned_actor_is_alive(py, module_name)),
            "the trader must own the registered wrapper after the caller drops its reference",
        );

        trader.remove_actor(&actor_id).unwrap();

        assert!(
            !Python::attach(|py| owned_actor_is_alive(py, module_name)),
            "removal must release the trader's strong owner and let the wrapper be collected",
        );
        assert!(get_component(&actor_id.inner()).is_none());
        assert!(!actor_exists(&actor_id.inner()));
    }

    #[cfg(feature = "python")]
    fn install_python_component_module(py: Python<'_>, module_name: &str) {
        let module = PyModule::new(py, module_name).expect("test module should create");
        module
            .setattr("DataActor", py.get_type::<PyDataActor>())
            .expect("DataActor type should bind");
        module
            .setattr("Strategy", py.get_type::<PyStrategy>())
            .expect("Strategy type should bind");

        let code = c_str!(
            r#"
class ModuleActor(DataActor):
    pass


class ModuleStrategy(Strategy):
    pass
"#
        );

        py.run(code, Some(&module.dict()), None)
            .expect("test component code should execute");

        py.import("sys")
            .expect("sys should import")
            .getattr("modules")
            .expect("sys.modules should exist")
            .set_item(module_name, module)
            .expect("test component module should register");
    }

    #[cfg(feature = "python")]
    fn create_python_component(py: Python<'_>, module_name: &str, class_name: &str) -> Py<PyAny> {
        py.import(module_name)
            .expect("test component module should import")
            .getattr(class_name)
            .expect("test component class should exist")
            .call0()
            .expect("test component should construct")
            .unbind()
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_colliding_python_registration_leaves_the_live_component_registered() {
        Python::initialize();

        let module_name = "test_trader_colliding_components";
        let strategy_id = StrategyId::from("Colliding-001");
        let actor_id = ActorId::from("Colliding-001");
        let component_id = ComponentId::from(strategy_id);

        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        Python::attach(|py| {
            install_python_component_module(py, module_name);

            let py_strategy = create_python_component(py, module_name, "ModuleStrategy");
            py_strategy
                .bind(py)
                .extract::<PyRefMut<PyStrategy>>()
                .unwrap()
                .set_strategy_id(strategy_id)
                .unwrap();

            trader
                .commit_python_strategy_instance(&py_strategy)
                .unwrap();

            // Positive control: without these the checks after the failed attempt would be vacuous
            assert!(get_component(&component_id.inner()).is_some());
            assert!(actor_exists(&component_id.inner()));
            assert!(
                get_python_wrapper(component_id)
                    .unwrap()
                    .bind(py)
                    .is(py_strategy.bind(py))
            );

            let py_actor = create_python_component(py, module_name, "ModuleActor");
            py_actor
                .bind(py)
                .extract::<PyRefMut<PyDataActor>>()
                .unwrap()
                .set_actor_id(actor_id);

            let error = trader
                .add_python_actor_instance(&py_actor, actor_id)
                .expect_err("an actor colliding with a live strategy must not register");
            assert!(error.to_string().contains("already registered"));

            // The strategy keeps every registration its own attempt created
            assert!(try_get_actor_unchecked::<PyStrategyInner>(&component_id.inner()).is_some());
            assert!(get_component(&component_id.inner()).is_some());
            assert!(actor_exists(&component_id.inner()));
            assert!(
                get_python_wrapper(component_id)
                    .expect("the strategy must still hold its wrapper")
                    .bind(py)
                    .is(py_strategy.bind(py))
            );
            assert_eq!(trader.strategy_ids(), vec![strategy_id]);
            assert!(trader.actor_ids().is_empty());
            assert_eq!(trader.get_component_clocks().len(), 1);
        });
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_failed_python_actor_registration_rolls_back_only_its_own_state() {
        Python::initialize();

        let module_name = "test_trader_rollback_components";
        let registered_id = ActorId::from("Rollback-Registered");
        let attempted_id = ActorId::from("Rollback-Attempted");
        let registered_component_id = ComponentId::from(registered_id);
        let attempted_component_id = ComponentId::from(attempted_id);

        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        Python::attach(|py| {
            install_python_component_module(py, module_name);

            let py_actor = create_python_component(py, module_name, "ModuleActor");
            py_actor
                .bind(py)
                .extract::<PyRefMut<PyDataActor>>()
                .unwrap()
                .set_actor_id(registered_id);

            trader
                .add_python_actor_instance(&py_actor, registered_id)
                .unwrap();

            // The same instance cannot register twice, so this attempt fails after it has already
            // created a component clock
            let error = trader
                .add_python_actor_instance(&py_actor, attempted_id)
                .expect_err("registering an already registered actor must fail");
            assert!(error.to_string().contains("already registered"));

            assert!(get_component(&attempted_component_id.inner()).is_none());
            assert!(!actor_exists(&attempted_component_id.inner()));
            assert!(get_python_wrapper(attempted_component_id).is_none());
            assert_eq!(trader.get_component_clocks().len(), 1);

            assert!(get_component(&registered_component_id.inner()).is_some());
            assert!(actor_exists(&registered_component_id.inner()));
            assert!(
                get_python_wrapper(registered_component_id)
                    .expect("the registered actor must still hold its wrapper")
                    .bind(py)
                    .is(py_actor.bind(py))
            );
            assert_eq!(trader.actor_ids(), vec![registered_id]);
        });
    }

    #[rstest]
    fn test_retirement_removes_component_subscriptions() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = ActorId::from("Subscribing-Actor");
        trader
            .add_actor(TestDataActor::new(DataActorConfig {
                actor_id: Some(actor_id),
                ..Default::default()
            }))
            .unwrap();
        trader.start_actor(&actor_id).unwrap();

        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let data_type = DataType::new(stringify!(TestRetirementData), None, None);
        let deltas_topic = get_book_deltas_topic(instrument_id);
        let depth_topic = get_book_depth10_topic(instrument_id);
        let data_topic = get_custom_topic(&data_type);

        {
            let mut actor = get_actor_unchecked::<TestDataActor>(&actor_id.inner());
            actor.subscribe_data(data_type, None, None);
            actor.subscribe_book_deltas(instrument_id, BookType::L3_MBO, None, None, false, None);
            actor.subscribe_book_depth10(instrument_id, BookType::L2_MBP, None, false, None);
        }

        // Positive control: without these the checks after retirement would be vacuous
        assert_eq!(msgbus::subscriptions_count_any(data_topic).unwrap(), 1);
        assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 1);
        assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 1);

        trader.remove_actor(&actor_id).unwrap();

        // Retirement must leave no handler behind for any of the component's subscription kinds
        assert_eq!(msgbus::subscriptions_count_any(data_topic).unwrap(), 0);
        assert_eq!(msgbus::subscriber_count_deltas(deltas_topic), 0);
        assert_eq!(msgbus::subscriber_count_depth10(depth_topic), 0);
    }

    #[rstest]
    fn test_failed_bulk_disposal_keeps_bookkeeping_consistent() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let retired_id = ActorId::from("Bulk-Retired-Actor");
        let failing_id = ActorId::from("Bulk-Failing-Actor");
        trader
            .add_actor(TestDataActor::new(DataActorConfig {
                actor_id: Some(retired_id),
                ..Default::default()
            }))
            .unwrap();
        let mut failing = TestDataActor::new(DataActorConfig {
            actor_id: Some(failing_id),
            ..Default::default()
        });
        failing.fail_dispose = true;
        trader.add_actor(failing).unwrap();

        let error = trader.clear_actors().unwrap_err();

        assert_eq!(error.to_string(), "test actor dispose failure");
        assert_eq!(trader.actor_ids(), vec![failing_id]);
        assert_eq!(trader.get_component_clocks().len(), 1);
        assert!(get_component(&retired_id.inner()).is_none());
        assert!(!actor_exists(&retired_id.inner()));
        assert!(get_component(&failing_id.inner()).is_some());
        assert!(actor_exists(&failing_id.inner()));
    }

    #[rstest]
    fn test_subscription_handler_tolerates_deregistered_actor() {
        let (_msgbus, cache, portfolio, _data_engine, _risk_engine, _exec_engine, clock_factory) =
            create_trader_components();
        let mut trader = Trader::new(
            TraderId::test_default(),
            UUID4::new(),
            Environment::Backtest,
            clock_factory,
            cache,
            portfolio,
        );

        let actor_id = ActorId::from("Snapshotted-Actor");
        trader
            .add_actor(TestDataActor::new(DataActorConfig {
                actor_id: Some(actor_id),
                ..Default::default()
            }))
            .unwrap();
        trader.start_actor(&actor_id).unwrap();

        let bar = stub_bar();
        let topic = get_bars_topic(bar.bar_type.standard());
        get_actor_unchecked::<TestDataActor>(&actor_id.inner()).subscribe_bars(
            bar.bar_type,
            None,
            None,
        );

        msgbus::publish_bar(topic, &bar);
        assert_eq!(
            get_actor_unchecked::<TestDataActor>(&actor_id.inner()).bars_received,
            1,
        );

        // Mirrors the dispatch window where a handler snapshotted for publication outlives
        // deregistration, which no unsubscribe can close
        deregister_actor(&actor_id.inner());

        // The delivery asserted above proves the handler is installed, and deregistering the
        // actor does not touch the message bus, so it is still installed here. The assertion is
        // that this does not panic resolving the actor which is now gone.
        msgbus::publish_bar(topic, &bar);
    }

    /// One trader operation applied by the retirement property test.
    #[derive(Debug, Clone, Copy)]
    enum TraderOp {
        AddActor(u8),
        RemoveActor(u8),
        ClearActors,
        AddStrategy(u8),
        RemoveStrategy(u8),
        ClearStrategies,
        AddExecAlgorithm(u8),
        ClearExecAlgorithms,
        DisposeComponents,
    }

    fn prop_actor_id(slot: u8) -> ActorId {
        ActorId::from(format!("PropActor-{slot}").as_str())
    }

    fn prop_strategy_id(slot: u8) -> StrategyId {
        StrategyId::from(format!("PropStrategy-{slot:03}").as_str())
    }

    fn prop_exec_algorithm_id(slot: u8) -> ExecAlgorithmId {
        ExecAlgorithmId::from(format!("PropExecAlgo-{slot}").as_str())
    }

    fn apply_trader_op(trader: &mut Trader, op: TraderOp) {
        // Every operation may legitimately fail (duplicate add, removing an absent component),
        // so the property is about the resulting state rather than the return value.
        match op {
            TraderOp::AddActor(slot) => {
                let _ = trader.add_actor(TestDataActor::new(DataActorConfig {
                    actor_id: Some(prop_actor_id(slot)),
                    ..Default::default()
                }));
            }
            TraderOp::RemoveActor(slot) => {
                let _ = trader.remove_actor(&prop_actor_id(slot));
            }
            TraderOp::ClearActors => {
                let _ = trader.clear_actors();
            }
            TraderOp::AddStrategy(slot) => {
                let _ = trader.add_strategy(TestStrategy::new(StrategyConfig {
                    strategy_id: Some(prop_strategy_id(slot)),
                    ..Default::default()
                }));
            }
            TraderOp::RemoveStrategy(slot) => {
                let _ = trader.remove_strategy(&prop_strategy_id(slot));
            }
            TraderOp::ClearStrategies => {
                let _ = trader.clear_strategies();
            }
            TraderOp::AddExecAlgorithm(slot) => {
                let _ =
                    trader.add_exec_algorithm(TestExecAlgorithm::new(ExecutionAlgorithmConfig {
                        exec_algorithm_id: Some(prop_exec_algorithm_id(slot)),
                        ..Default::default()
                    }));
            }
            TraderOp::ClearExecAlgorithms => {
                let _ = trader.clear_exec_algorithms();
            }
            TraderOp::DisposeComponents => {
                let _ = trader.dispose_components();
            }
        }
    }

    /// Asserts the trader's bookkeeping agrees with the global registries.
    ///
    /// Tracked components must resolve in both registries, untracked slots must be absent from
    /// both, and every tracked component must still own exactly one clock.
    fn assert_registry_consistency(trader: &Trader, slots: &[u8]) {
        for &slot in slots {
            let actor_id = prop_actor_id(slot);
            let tracked = trader.actor_ids().contains(&actor_id);
            assert_eq!(
                get_component(&actor_id.inner()).is_some(),
                tracked,
                "actor {actor_id} component registry entry must match trader tracking",
            );
            assert_eq!(
                actor_exists(&actor_id.inner()),
                tracked,
                "actor {actor_id} actor registry entry must match trader tracking",
            );

            let strategy_id = prop_strategy_id(slot);
            let tracked = trader.strategy_ids().contains(&strategy_id);
            assert_eq!(
                get_component(&strategy_id.inner()).is_some(),
                tracked,
                "strategy {strategy_id} component registry entry must match trader tracking",
            );
            assert_eq!(
                actor_exists(&strategy_id.inner()),
                tracked,
                "strategy {strategy_id} actor registry entry must match trader tracking",
            );

            let exec_algorithm_id = prop_exec_algorithm_id(slot);
            let tracked = trader.exec_algorithm_ids().contains(&exec_algorithm_id);
            assert_eq!(
                get_component(&exec_algorithm_id.inner()).is_some(),
                tracked,
                "exec algorithm {exec_algorithm_id} component entry must match trader tracking",
            );
            assert_eq!(
                actor_exists(&exec_algorithm_id.inner()),
                tracked,
                "exec algorithm {exec_algorithm_id} actor entry must match trader tracking",
            );
        }

        assert_eq!(
            trader.get_component_clocks().len(),
            trader.component_count(),
            "every tracked component owns exactly one clock",
        );
    }

    // Anonymous so proptest's `Strategy` does not collide with the trading `Strategy` trait
    use proptest::strategy::Strategy as _;

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        /// Whatever order add, remove, clear, and dispose arrive in, the trader's bookkeeping
        /// never disagrees with the global registries, and nothing it stopped tracking is left
        /// behind in them.
        #[rstest]
        fn prop_trader_bookkeeping_matches_registries(
            ops in proptest::collection::vec(
                proptest::prop_oneof![
                    (0u8..3).prop_map(TraderOp::AddActor),
                    (0u8..3).prop_map(TraderOp::RemoveActor),
                    proptest::prelude::Just(TraderOp::ClearActors),
                    (0u8..3).prop_map(TraderOp::AddStrategy),
                    (0u8..3).prop_map(TraderOp::RemoveStrategy),
                    proptest::prelude::Just(TraderOp::ClearStrategies),
                    (0u8..3).prop_map(TraderOp::AddExecAlgorithm),
                    proptest::prelude::Just(TraderOp::ClearExecAlgorithms),
                    proptest::prelude::Just(TraderOp::DisposeComponents),
                ],
                1..12usize,
            ),
        ) {
            let slots: Vec<u8> = (0u8..3).collect();

            // Reset up front: a case which fails mid-way never reaches its teardown, and stale
            // registry entries would otherwise corrupt every later shrink iteration
            for &slot in &slots {
                deregister_component(&prop_actor_id(slot).inner());
                deregister_actor(&prop_actor_id(slot).inner());
                deregister_component(&prop_strategy_id(slot).inner());
                deregister_actor(&prop_strategy_id(slot).inner());
                deregister_component(&prop_exec_algorithm_id(slot).inner());
                deregister_actor(&prop_exec_algorithm_id(slot).inner());
            }

            let (
                _msgbus,
                cache,
                portfolio,
                _data_engine,
                _risk_engine,
                _exec_engine,
                clock_factory,
            ) = create_trader_components();
            let mut trader = Trader::new(
                TraderId::test_default(),
                UUID4::new(),
                Environment::Backtest,
                clock_factory,
                cache,
                portfolio,
            );

            for op in ops {
                apply_trader_op(&mut trader, op);
                assert_registry_consistency(&trader, &slots);
            }

            // Retire everything so the thread-local registries stay isolated between cases,
            // then prove retirement left nothing behind
            let _ = trader.dispose_components();
            assert_registry_consistency(&trader, &slots);
            assert_eq!(trader.component_count(), 0);
            assert!(trader.get_component_clocks().is_empty());
        }
    }
}
