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
    cell::{Ref, RefCell, RefMut},
    fmt::Debug,
    rc::Rc,
};

use ahash::AHashMap;
use nautilus_common::{
    actor::{DataActorConfig, DataActorCore, DataActorNative},
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

use super::{
    api::{OrderApi, PortfolioApi},
    config::StrategyConfig,
};

/// The core component of a [`Strategy`](crate::strategy::Strategy), managing data, orders,
/// and state.
///
/// This struct is intended to be held as a member within a user's custom strategy struct.
/// Use the `nautilus_strategy!` macro to provide the trait accessors required by
/// [`Strategy`](crate::strategy::Strategy), [`StrategyNative`], and
/// [`DataActor`](nautilus_common::actor::DataActor). It does not deref to
/// [`DataActorCore`]; normal strategy logic should use facade methods on the
/// strategy value.
pub struct StrategyCore {
    pub(crate) actor: DataActorCore,
    /// The strategy configuration.
    pub config: StrategyConfig,
    strategy_id: Option<StrategyId>,
    order_id_tag: Option<String>,
    pub(crate) order_manager: Option<OrderManager>,
    pub(crate) order_factory: Option<Rc<RefCell<OrderFactory>>>,
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

/// Native-only access to internal strategy runtime state.
///
/// Use this trait from engine, runtime, testkit, or opt-in native strategy
/// code when direct access to host runtime objects matters for an explicit
/// latency-sensitive path, or when host integration code needs access below
/// the facade API.
///
/// Do not import this trait in strategy code intended to run through Python or
/// the plug-in authoring surface. Those surfaces should use facade methods such
/// as `order()` and `portfolio()`, because native borrows, `Rc<RefCell<_>>`, and
/// core references do not cross those boundaries.
pub trait StrategyNative {
    /// Returns the strategy core.
    fn strategy_core(&self) -> &StrategyCore;

    /// Returns the mutable strategy core.
    fn strategy_core_mut(&mut self) -> &mut StrategyCore;

    /// Returns a mutable borrow of the order factory.
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    fn order_factory(&mut self) -> RefMut<'_, OrderFactory> {
        self.strategy_core_mut()
            .order_factory
            .as_ref()
            .expect("Strategy not registered: OrderFactory not initialized")
            .borrow_mut()
    }

    /// Returns a clone of the reference-counted order factory.
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    fn order_factory_rc(&self) -> Rc<RefCell<OrderFactory>> {
        self.strategy_core()
            .order_factory
            .as_ref()
            .expect("Strategy not registered: OrderFactory not initialized")
            .clone()
    }

    /// Returns a clone of the reference-counted portfolio.
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    fn portfolio_rc(&self) -> Rc<RefCell<Portfolio>> {
        self.strategy_core()
            .portfolio
            .as_ref()
            .expect("Strategy not registered: Portfolio not initialized")
            .clone()
    }
}

impl StrategyCore {
    /// Creates a new [`StrategyCore`] instance.
    #[must_use]
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

        self.order_factory = Some(Rc::new(RefCell::new(OrderFactory::new(
            trader_id,
            strategy_id,
            None,
            None,
            clock.clone(),
            self.config.use_uuid_client_order_ids,
            self.config.use_hyphens_in_client_order_ids,
        ))));

        self.order_manager = Some(OrderManager::new(clock, cache, false));

        self.portfolio = Some(portfolio);

        Ok(())
    }

    /// Returns the user-facing order creation API.
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    #[must_use]
    pub fn order(&self) -> OrderApi<'_> {
        let order_factory = self
            .order_factory
            .as_ref()
            .expect("Strategy not registered: OrderFactory not initialized");
        OrderApi::new(order_factory.as_ref())
    }

    /// Returns the user-facing portfolio read API.
    ///
    /// # Panics
    ///
    /// Panics if the strategy has not been registered.
    #[must_use]
    pub(crate) fn portfolio_api(&self) -> PortfolioApi<'_> {
        let portfolio = self
            .portfolio
            .as_ref()
            .expect("Strategy not registered: Portfolio not initialized");
        PortfolioApi::new(portfolio.as_ref())
    }

    pub(crate) fn actor_id(&self) -> ActorId {
        self.actor.actor_id()
    }

    pub(crate) fn trader_id(&self) -> Option<TraderId> {
        self.actor.trader_id()
    }

    pub(crate) fn clock_mut(&mut self) -> RefMut<'_, dyn Clock> {
        DataActorNative::clock_mut(self)
    }

    pub(crate) fn cache_ref(&self) -> Ref<'_, Cache> {
        DataActorNative::cache_ref(self)
    }

    pub(crate) fn cache_rc(&self) -> Rc<RefCell<Cache>> {
        DataActorNative::cache_rc(self)
    }

    /// Resets the market exit state.
    pub fn reset_market_exit_state(&mut self) {
        self.is_exiting = false;
        self.pending_stop = false;
        self.market_exit_attempts = 0;
    }
}

impl DataActorNative for StrategyCore {
    fn core(&self) -> &DataActorCore {
        &self.actor
    }

    fn core_mut(&mut self) -> &mut DataActorCore {
        &mut self.actor
    }
}

impl StrategyNative for StrategyCore {
    fn strategy_core(&self) -> &StrategyCore {
        self
    }

    fn strategy_core_mut(&mut self) -> &mut StrategyCore {
        self
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

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce, TrailingOffsetType, TriggerType},
        identifiers::{AccountId, InstrumentId, StrategyId, TraderId},
        orders::Order,
        types::{Price, Quantity},
    };
    use nautilus_portfolio::portfolio::Portfolio;
    use rstest::rstest;
    use rust_decimal::Decimal;

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

        let (client_order_id, order_list_id) = {
            let mut order_factory = core.order_factory();
            (
                order_factory.generate_client_order_id(),
                order_factory.generate_order_list_id(),
            )
        };

        assert_eq!(
            core.strategy_id(),
            Some(StrategyId::from("ExampleStrategy-XNAS-T01"))
        );
        assert_eq!(client_order_id.as_str(), "O-19700101-000000-001-T01-1");
        assert_eq!(order_list_id.as_str(), "OL-19700101-000000-001-T01-1");
    }

    #[rstest]
    fn test_strategy_core_order_api_creates_orders() {
        let core = registered_test_core();
        let orders = core.order();

        let market = orders.market(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Buy,
            Quantity::from("1.0"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let limit = orders.limit(
            InstrumentId::from("BTCUSDT.BINANCE"),
            OrderSide::Sell,
            Quantity::from("2.0"),
            Price::from("100.00"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(market.order_type(), OrderType::Market);
        assert_eq!(
            market.client_order_id().as_str(),
            "O-19700101-000000-001-001-1"
        );
        assert_eq!(limit.order_type(), OrderType::Limit);
        assert_eq!(
            limit.client_order_id().as_str(),
            "O-19700101-000000-001-001-2"
        );
    }

    #[rstest]
    fn test_strategy_core_order_api_creates_remaining_order_types() {
        let core = registered_test_core();
        let orders = core.order();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let trigger_instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
        let expire_time = UnixNanos::from(1_000);
        let display_qty = Quantity::from("0.5");

        let stop_market = orders.stop_market(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("1.0"),
            Price::from("99.00"),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtd),
            Some(expire_time),
            Some(true),
            Some(false),
            Some(display_qty),
            Some(TriggerType::BidAsk),
            Some(trigger_instrument_id),
            None,
            None,
            None,
            None,
        );
        let stop_limit = orders.stop_limit(
            instrument_id,
            OrderSide::Sell,
            Quantity::from("1.1"),
            Price::from("101.00"),
            Price::from("100.50"),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtd),
            Some(expire_time),
            Some(true),
            Some(false),
            Some(false),
            Some(display_qty),
            Some(TriggerType::BidAsk),
            Some(trigger_instrument_id),
            None,
            None,
            None,
            None,
        );
        let market_to_limit = orders.market_to_limit(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("1.2"),
            Some(TimeInForce::Gtd),
            Some(expire_time),
            Some(true),
            Some(false),
            Some(display_qty),
            None,
            None,
            None,
            None,
        );
        let market_if_touched = orders.market_if_touched(
            instrument_id,
            OrderSide::Sell,
            Quantity::from("1.3"),
            Price::from("98.50"),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtd),
            Some(expire_time),
            Some(false),
            Some(false),
            Some(TriggerType::BidAsk),
            Some(trigger_instrument_id),
            None,
            None,
            None,
            None,
        );
        let limit_if_touched = orders.limit_if_touched(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("1.4"),
            Price::from("97.50"),
            Price::from("97.00"),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtd),
            Some(expire_time),
            Some(true),
            Some(false),
            Some(false),
            Some(display_qty),
            Some(TriggerType::BidAsk),
            Some(trigger_instrument_id),
            None,
            None,
            None,
            None,
        );
        let trailing_stop_market = orders.trailing_stop_market(
            instrument_id,
            OrderSide::Sell,
            Quantity::from("1.5"),
            Decimal::new(25, 2),
            Some(TrailingOffsetType::Price),
            Some(Price::from("105.00")),
            Some(Price::from("104.50")),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtd),
            Some(expire_time),
            Some(false),
            Some(false),
            Some(display_qty),
            Some(TriggerType::BidAsk),
            Some(trigger_instrument_id),
            None,
            None,
            None,
            None,
        );
        let trailing_stop_limit = orders.trailing_stop_limit(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("1.6"),
            Price::from("96.00"),
            Decimal::new(10, 2),
            Decimal::new(50, 2),
            Some(TrailingOffsetType::Price),
            Some(Price::from("97.00")),
            Some(Price::from("96.50")),
            Some(TriggerType::LastPrice),
            Some(TimeInForce::Gtd),
            Some(expire_time),
            Some(true),
            Some(false),
            Some(false),
            Some(display_qty),
            Some(TriggerType::BidAsk),
            Some(trigger_instrument_id),
            None,
            None,
            None,
            None,
        );
        let mut list_orders = vec![market_to_limit.clone(), stop_limit.clone()];
        let order_list = orders.create_list(&mut list_orders, expire_time);

        assert_eq!(stop_market.order_type(), OrderType::StopMarket);
        assert_eq!(stop_market.trigger_price(), Some(Price::from("99.00")));
        assert_eq!(stop_market.trigger_type(), Some(TriggerType::LastPrice));
        assert_eq!(stop_market.time_in_force(), TimeInForce::Gtd);
        assert_eq!(stop_market.expire_time(), Some(expire_time));
        assert!(stop_market.is_reduce_only());
        assert_eq!(stop_market.display_qty(), Some(display_qty));
        assert_eq!(stop_market.emulation_trigger(), Some(TriggerType::BidAsk));
        assert_eq!(
            stop_market.trigger_instrument_id(),
            Some(trigger_instrument_id)
        );

        assert_eq!(stop_limit.order_type(), OrderType::StopLimit);
        assert_eq!(stop_limit.price(), Some(Price::from("101.00")));
        assert_eq!(stop_limit.trigger_price(), Some(Price::from("100.50")));
        assert!(stop_limit.is_post_only());

        assert_eq!(market_to_limit.order_type(), OrderType::MarketToLimit);
        assert_eq!(market_to_limit.time_in_force(), TimeInForce::Gtd);
        assert_eq!(market_to_limit.expire_time(), Some(expire_time));
        assert!(market_to_limit.is_reduce_only());
        assert_eq!(market_to_limit.display_qty(), Some(display_qty));

        assert_eq!(market_if_touched.order_type(), OrderType::MarketIfTouched);
        assert_eq!(
            market_if_touched.trigger_price(),
            Some(Price::from("98.50"))
        );
        assert_eq!(
            market_if_touched.trigger_type(),
            Some(TriggerType::LastPrice)
        );

        assert_eq!(limit_if_touched.order_type(), OrderType::LimitIfTouched);
        assert_eq!(limit_if_touched.price(), Some(Price::from("97.50")));
        assert_eq!(limit_if_touched.trigger_price(), Some(Price::from("97.00")));
        assert!(limit_if_touched.is_post_only());

        assert_eq!(
            trailing_stop_market.order_type(),
            OrderType::TrailingStopMarket
        );
        assert_eq!(
            trailing_stop_market.trailing_offset(),
            Some(Decimal::new(25, 2))
        );
        assert_eq!(
            trailing_stop_market.trailing_offset_type(),
            Some(TrailingOffsetType::Price)
        );
        assert_eq!(
            trailing_stop_market.activation_price(),
            Some(Price::from("105.00"))
        );
        assert_eq!(
            trailing_stop_market.trigger_price(),
            Some(Price::from("104.50"))
        );

        assert_eq!(
            trailing_stop_limit.order_type(),
            OrderType::TrailingStopLimit
        );
        assert_eq!(trailing_stop_limit.price(), Some(Price::from("96.00")));
        assert_eq!(
            trailing_stop_limit.limit_offset(),
            Some(Decimal::new(10, 2))
        );
        assert_eq!(
            trailing_stop_limit.trailing_offset(),
            Some(Decimal::new(50, 2))
        );
        assert_eq!(
            trailing_stop_limit.activation_price(),
            Some(Price::from("97.00"))
        );
        assert!(trailing_stop_limit.is_post_only());

        assert_eq!(order_list.id, list_orders[0].order_list_id().unwrap());
        assert_eq!(order_list.id, list_orders[1].order_list_id().unwrap());
        assert_eq!(order_list.instrument_id, instrument_id);
        assert_eq!(
            order_list.client_order_ids,
            list_orders
                .iter()
                .map(Order::client_order_id)
                .collect::<Vec<_>>()
        );
    }

    #[rstest]
    fn test_strategy_core_order_api_generates_ids() {
        let mut core = registered_test_core();
        let (client_order_id, order_list_id) = {
            let orders = core.order();
            (
                orders.generate_client_order_id(),
                orders.generate_order_list_id(),
            )
        };

        let _native_order_factory = core.order_factory().generate_client_order_id();

        assert_eq!(client_order_id.as_str(), "O-19700101-000000-001-001-1");
        assert_eq!(order_list_id.as_str(), "OL-19700101-000000-001-001-1");
    }

    #[rstest]
    fn test_strategy_core_order_api_creates_bracket_orders() {
        let core = registered_test_core();

        let orders = core
            .order()
            .bracket()
            .instrument_id(InstrumentId::from("BTCUSDT.BINANCE"))
            .order_side(OrderSide::Buy)
            .quantity(Quantity::from("1.0"))
            .tp_price(Price::from("110.00"))
            .sl_trigger_price(Price::from("90.00"))
            .call();
        let order_list_id = orders[0].order_list_id();

        assert_eq!(orders.len(), 3);
        assert_eq!(orders[0].order_type(), OrderType::Market);
        assert_eq!(orders[1].order_type(), OrderType::StopMarket);
        assert_eq!(orders[2].order_type(), OrderType::Limit);
        assert!(order_list_id.is_some());
        assert!(
            orders
                .iter()
                .all(|order| order.order_list_id() == order_list_id)
        );
    }

    #[rstest]
    fn test_strategy_core_portfolio_api_returns_owned_reads() {
        let core = registered_test_core();
        let portfolio = core.portfolio_api();
        let instrument_id = InstrumentId::from("BTCUSDT.BINANCE");
        let venue = instrument_id.venue;

        let is_initialized = portfolio.is_initialized();
        let balances_locked = portfolio.balances_locked(&venue);
        let margins_init = portfolio.margins_init(&venue);
        let margins_maint = portfolio.margins_maint(&venue);
        let unrealized_pnls = portfolio.unrealized_pnls(&venue, None);
        let realized_pnls = portfolio.realized_pnls(&venue, None);
        let net_exposures = portfolio.net_exposures(&venue, None);
        let unrealized_pnl = portfolio.unrealized_pnl(&instrument_id);
        let realized_pnl = portfolio.realized_pnl(&instrument_id);
        let total_pnl = portfolio.total_pnl(&instrument_id);
        let total_pnls = portfolio.total_pnls(&venue, None);
        let mark_values = portfolio.mark_values(&venue, None);
        let equity = portfolio.equity(&venue, None);
        let net_exposure = portfolio.net_exposure(&instrument_id, None);
        let is_flat = portfolio.is_flat(&instrument_id);
        let net_position = portfolio.net_position(&instrument_id);
        let missing_prices = portfolio.missing_price_instruments(&venue);
        let snapshots = portfolio.snapshots(&AccountId::from("SIM-001"));
        let recorded_realized_pnls = portfolio.recorded_realized_pnls();

        let native_portfolio = core.portfolio_rc();
        let _native_portfolio = native_portfolio.borrow_mut();

        assert!(!is_initialized);
        assert!(balances_locked.is_empty());
        assert!(margins_init.is_empty());
        assert!(margins_maint.is_empty());
        assert!(unrealized_pnls.is_empty());
        assert!(realized_pnls.is_empty());
        assert_eq!(net_exposures, None);
        assert_eq!(unrealized_pnl, None);
        assert_eq!(realized_pnl, None);
        assert_eq!(total_pnl, None);
        assert!(total_pnls.is_empty());
        assert!(mark_values.is_empty());
        assert!(equity.is_empty());
        assert_eq!(net_exposure, None);
        assert!(is_flat);
        assert_eq!(net_position, Decimal::ZERO);
        assert!(missing_prices.is_empty());
        assert!(snapshots.is_empty());
        assert!(recorded_realized_pnls.is_empty());
    }

    #[rstest]
    fn test_strategy_core_actor_state_starts_unregistered() {
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

    fn registered_test_core() -> StrategyCore {
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

        core.register(trader_id, clock, cache, portfolio).unwrap();
        core
    }
}
