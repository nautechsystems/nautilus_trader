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

//! Risk management engine implementation.

pub mod config;

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use ahash::AHashMap;
use config::RiskEngineConfig;
use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    messages::{
        execution::{
            BatchModifyOrders, ModifyOrder, PARAMS_CLOSE_POSITION, SubmitOrder, SubmitOrderList,
            TradingCommand,
        },
        system::trading::TradingStateChanged,
    },
    msgbus,
    msgbus::{MessagingSwitchboard, TypedHandler, TypedIntoHandler, get_message_bus},
    runner::{TradingCommandMessage, try_get_trading_cmd_sender},
    throttler::Throttler,
};
use nautilus_core::{UUID4, WeakCell};
use nautilus_execution::trailing::{
    trailing_stop_calculate_with_bid_ask, trailing_stop_calculate_with_last,
};
use nautilus_model::{
    accounts::{Account, AccountAny},
    enums::{
        AggregationSource, OrderSide, OrderStatus, OrderType, PositionSide, PriceType, TimeInForce,
        TradingState, TrailingOffsetType, TriggerType,
    },
    events::{
        OrderDenied, OrderDeniedReason, OrderEventAny, OrderModifyRejected, OrderPriceField,
        OrderUpdated, PositionEvent,
    },
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{LIMIT_ORDER_TYPES, Order, OrderAny, STOP_ORDER_TYPES},
    types::{Currency, Money, Price, Quantity, quantity::QuantityRaw},
};
use nautilus_portfolio::Portfolio;
use rust_decimal::Decimal;
use ustr::Ustr;

type SubmitCommandFn = Box<dyn Fn(TradingCommand)>;
type ModifyOrderFn = Box<dyn Fn(ModifyOrder)>;

/// Central risk management engine that validates and controls trading operations.
///
/// The `RiskEngine` provides pre-trade risk checks including order validation,
/// balance verification, position sizing limits, and trading state management. It acts as
/// a gateway between strategy orders and execution, ensuring all trades comply with
/// defined risk parameters and regulatory constraints.
#[allow(dead_code)]
pub struct RiskEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    portfolio: Portfolio,
    trading_state: TradingState,
    config: RiskEngineConfig,
    max_notional_per_order: AHashMap<InstrumentId, Decimal>,
    throttler_submit: Throttler<TradingCommand, SubmitCommandFn>,
    throttler_modify: Throttler<ModifyOrder, ModifyOrderFn>,
    command_count: u64,
    event_count: u64,
}

impl Debug for RiskEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RiskEngine))
            .field("trading_state", &self.trading_state)
            .field("config", &self.config)
            .field("max_notional_per_order", &self.max_notional_per_order)
            .field("throttler_submit", &self.throttler_submit)
            .field("throttler_modify", &self.throttler_modify)
            .field("command_count", &self.command_count)
            .field("event_count", &self.event_count)
            .finish_non_exhaustive()
    }
}

impl RiskEngine {
    /// Creates a new [`RiskEngine`] instance.
    pub fn new(
        config: RiskEngineConfig,
        portfolio: Portfolio,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        let throttler_submit =
            Self::create_submit_throttler(&config, Rc::clone(&clock), Rc::clone(&cache));
        let throttler_modify =
            Self::create_modify_throttler(&config, Rc::clone(&clock), Rc::clone(&cache));
        let max_notional_per_order = config.max_notional_per_order.clone();

        Self {
            clock,
            cache,
            portfolio,
            trading_state: TradingState::Active,
            config,
            max_notional_per_order,
            throttler_submit,
            throttler_modify,
            command_count: 0,
            event_count: 0,
        }
    }

    /// Registers all message bus handlers for the risk engine.
    pub fn register_msgbus_handlers(engine: &Rc<RefCell<Self>>) {
        let weak = WeakCell::from(Rc::downgrade(engine));

        let weak_execute = weak.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_execute(),
            TypedIntoHandler::from(move |cmd: TradingCommand| {
                if let Some(rc) = weak_execute.upgrade() {
                    rc.borrow_mut().execute(cmd);
                }
            }),
        );

        // Queued endpoint for deferred command execution (re-entrancy safe).
        // When a strategy calls `submit_order()` from within an event handler
        // (e.g., `on_order_filled`), the command is routed through this endpoint.
        // In live mode the `TradingCommandSender` queues the command for the next
        // event-loop iteration, preventing a synchronous `deny_order()` from
        // dispatching an `OrderDenied` back into a strategy that still holds a
        // mutable borrow - which would otherwise panic on `RefCell` re-entrancy.
        // If no sender is installed, the queued endpoint falls back to direct dispatch.
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            TypedIntoHandler::from(move |cmd: TradingCommand| {
                if let Some(sender) = try_get_trading_cmd_sender() {
                    sender.execute(TradingCommandMessage::new(
                        MessagingSwitchboard::risk_engine_execute(),
                        cmd,
                    ));
                } else {
                    let endpoint = MessagingSwitchboard::risk_engine_execute();
                    msgbus::send_trading_command(endpoint, cmd);
                }
            }),
        );

        let weak_process = weak.clone();
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::risk_engine_process(),
            TypedIntoHandler::from(move |event: OrderEventAny| {
                if let Some(rc) = weak_process.upgrade() {
                    rc.borrow_mut().process(event);
                }
            }),
        );

        let weak_order_events = weak.clone();
        msgbus::subscribe_order_events(
            "events.order.*".into(),
            TypedHandler::from(move |event: &OrderEventAny| {
                // Risk-generated events can publish while `execute` still owns the engine,
                // and processing is observational, so skipping reentrant events is safe.
                // TODO: Revisit this if order-event processing gains stateful behavior
                if let Some(rc) = weak_order_events.upgrade()
                    && let Ok(mut engine) = rc.try_borrow_mut()
                {
                    engine.process(event.clone());
                }
            }),
            Some(10),
        );

        let weak_position_events = weak;
        msgbus::subscribe_position_events(
            "events.position.*".into(),
            TypedHandler::from(move |event: &PositionEvent| {
                if let Some(rc) = weak_position_events.upgrade() {
                    rc.borrow_mut().process_position_event(event);
                }
            }),
            Some(10),
        );
    }

    fn create_submit_throttler(
        config: &RiskEngineConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Throttler<TradingCommand, SubmitCommandFn> {
        let success_handler = {
            Box::new(move |command: TradingCommand| {
                let endpoint = MessagingSwitchboard::exec_engine_queue_execute();
                msgbus::send_trading_command(endpoint, command);
            }) as Box<dyn Fn(TradingCommand)>
        };

        let failure_handler = {
            let cache = cache;
            let clock = Rc::clone(&clock);
            Box::new(move |command: TradingCommand| {
                let reason = OrderDeniedReason::RateLimitExceeded.to_string();

                match command {
                    TradingCommand::SubmitOrder(submit_order) => {
                        log::warn!(
                            "SubmitOrder for {} DENIED: {reason}",
                            submit_order.client_order_id,
                        );

                        Self::handle_submit_order_cache(&cache, &submit_order);

                        let denied = Self::create_order_denied(&submit_order, &reason, &clock);

                        let endpoint = MessagingSwitchboard::exec_engine_process();
                        msgbus::send_order_event(endpoint, denied);
                    }
                    TradingCommand::SubmitOrderList(submit_order_list) => {
                        log::warn!(
                            "SubmitOrderList for {} DENIED: {reason}",
                            submit_order_list.order_list.id,
                        );

                        let orders: Vec<OrderAny> = cache.borrow().orders_for_ids(
                            &submit_order_list.order_list.client_order_ids,
                            &submit_order_list,
                        );

                        let timestamp = clock.borrow().timestamp_ns();

                        for order in &orders {
                            if order.status() == OrderStatus::Initialized {
                                let denied = OrderEventAny::Denied(OrderDenied::new(
                                    order.trader_id(),
                                    order.strategy_id(),
                                    order.instrument_id(),
                                    order.client_order_id(),
                                    reason.as_str().into(),
                                    UUID4::new(),
                                    timestamp,
                                    timestamp,
                                ));
                                let endpoint = MessagingSwitchboard::exec_engine_process();
                                msgbus::send_order_event(endpoint, denied);
                            }
                        }
                    }
                    _ => {
                        log::error!("Unexpected command type in submit throttler: {command}");
                    }
                }
            }) as Box<dyn Fn(TradingCommand)>
        };

        Throttler::new(
            config.max_order_submit,
            clock,
            "ORDER_SUBMIT_THROTTLER",
            success_handler,
            Some(failure_handler),
            Ustr::from(UUID4::new().as_str()),
        )
    }

    fn create_modify_throttler(
        config: &RiskEngineConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Throttler<ModifyOrder, ModifyOrderFn> {
        let success_handler = {
            Box::new(move |order: ModifyOrder| {
                let endpoint = MessagingSwitchboard::exec_engine_queue_execute();
                msgbus::send_trading_command(endpoint, TradingCommand::ModifyOrder(order));
            }) as Box<dyn Fn(ModifyOrder)>
        };

        let failure_handler = {
            let cache = cache;
            let clock = Rc::clone(&clock);
            Box::new(move |order: ModifyOrder| {
                let reason = "Exceeded MAX_ORDER_MODIFY_RATE";
                log::warn!(
                    "SubmitOrder for {} DENIED: {}",
                    order.client_order_id,
                    reason
                );

                let Some(order) = Self::get_existing_order(&cache, &order) else {
                    return;
                };

                let rejected = Self::create_modify_rejected(&order, reason, &clock);

                let endpoint = MessagingSwitchboard::exec_engine_process();
                msgbus::send_order_event(endpoint, rejected);
            }) as Box<dyn Fn(ModifyOrder)>
        };

        Throttler::new(
            config.max_order_modify,
            clock,
            "ORDER_MODIFY_THROTTLER",
            success_handler,
            Some(failure_handler),
            Ustr::from(UUID4::new().as_str()),
        )
    }

    fn handle_submit_order_cache(cache: &Rc<RefCell<Cache>>, submit_order: &SubmitOrder) {
        let cache = cache.borrow();
        if !cache.order_exists(&submit_order.client_order_id) {
            log::error!(
                "Order not found in cache for client_order_id: {}",
                submit_order.client_order_id
            );
        }
    }

    fn get_existing_order(cache: &Rc<RefCell<Cache>>, order: &ModifyOrder) -> Option<OrderAny> {
        let cache = cache.borrow();
        if let Some(order) = cache.order(&order.client_order_id) {
            Some(order.clone())
        } else {
            log::error!(
                "Order with command.client_order_id: {} not found",
                order.client_order_id
            );
            None
        }
    }

    fn create_order_denied(
        submit_order: &SubmitOrder,
        reason: &str,
        clock: &Rc<RefCell<dyn Clock>>,
    ) -> OrderEventAny {
        let timestamp = clock.borrow().timestamp_ns();
        OrderEventAny::Denied(OrderDenied::new(
            submit_order.trader_id,
            submit_order.strategy_id,
            submit_order.instrument_id,
            submit_order.client_order_id,
            reason.into(),
            UUID4::new(),
            timestamp,
            timestamp,
        ))
    }

    fn create_modify_rejected(
        order: &OrderAny,
        reason: &str,
        clock: &Rc<RefCell<dyn Clock>>,
    ) -> OrderEventAny {
        let timestamp = clock.borrow().timestamp_ns();
        OrderEventAny::ModifyRejected(OrderModifyRejected::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            timestamp,
            timestamp,
            false,
            order.venue_order_id(),
            order.account_id(),
        ))
    }

    /// Executes a trading command through the risk management pipeline.
    // Required by message bus dispatch
    pub fn execute(&mut self, command: TradingCommand) {
        self.command_count += 1;

        // This will extend to other commands such as `RiskCommand`
        self.handle_command(command);
    }

    /// Processes an order event for risk monitoring and state updates.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "message bus dispatch passes owned order events"
    )]
    pub fn process(&mut self, event: OrderEventAny) {
        self.event_count += 1;

        // This will extend to other events such as `RiskEvent`
        self.handle_event(&event);
    }

    fn process_position_event(&mut self, event: &PositionEvent) {
        self.event_count += 1;

        self.handle_position_event(event);
    }

    /// Sets the trading state for risk control enforcement.
    ///
    /// [`TradingState::Halted`] denies all new submit and modify commands.
    pub fn set_trading_state(&mut self, state: TradingState) {
        if state == self.trading_state {
            log::warn!("No change to trading state: already set to {state:?}");
            return;
        }

        self.trading_state = state;

        let ts_now = self.clock.borrow().timestamp_ns();
        let trader_id = get_message_bus().borrow().trader_id;

        let config = self.config_as_map();
        let event =
            TradingStateChanged::new(trader_id, state, config, UUID4::new(), ts_now, ts_now);

        msgbus::publish_any(MessagingSwitchboard::risk_events_topic(), &event);

        log::info!("Trading state set to {state:?}");
    }

    /// Sets the maximum notional value per order for the specified instrument.
    pub fn set_max_notional_per_order(&mut self, instrument_id: InstrumentId, new_value: Decimal) {
        self.max_notional_per_order.insert(instrument_id, new_value);

        let new_value_str = new_value.to_string();
        log::info!("Set MAX_NOTIONAL_PER_ORDER: {instrument_id} {new_value_str}");
    }

    /// Starts the risk engine.
    pub fn start(&mut self) {
        log::info!("Started");
    }

    /// Stops the risk engine.
    pub fn stop(&mut self) {
        log::info!("Stopped");
    }

    /// Resets the risk engine to its initial state.
    pub fn reset(&mut self) {
        self.throttler_submit.reset();
        self.throttler_modify.reset();
        self.max_notional_per_order = self.config.max_notional_per_order.clone();
        self.command_count = 0;
        self.event_count = 0;

        if self.trading_state != TradingState::Active {
            self.set_trading_state(TradingState::Active);
        }

        log::info!("Reset");
    }

    /// Disposes of the risk engine, releasing resources.
    pub fn dispose(&mut self) {
        log::info!("Disposed");
    }

    /// Returns a reference to the clock.
    #[must_use]
    pub fn clock(&self) -> &Rc<RefCell<dyn Clock>> {
        &self.clock
    }

    /// Returns a reference to the cache.
    #[must_use]
    pub fn cache(&self) -> &Rc<RefCell<Cache>> {
        &self.cache
    }

    /// Returns a mutable reference to the portfolio.
    pub fn portfolio_mut(&mut self) -> &mut Portfolio {
        &mut self.portfolio
    }

    /// Returns a reference to the configuration.
    #[must_use]
    pub const fn config(&self) -> &RiskEngineConfig {
        &self.config
    }

    /// Returns the total count of trading commands received by the engine.
    #[must_use]
    pub const fn command_count(&self) -> u64 {
        self.command_count
    }

    /// Returns the total count of order events received by the engine.
    #[must_use]
    pub const fn event_count(&self) -> u64 {
        self.event_count
    }

    /// Returns the current trading state.
    #[must_use]
    pub const fn trading_state(&self) -> TradingState {
        self.trading_state
    }

    /// Returns a reference to the max notional per order settings.
    #[must_use]
    pub const fn max_notional_per_order(&self) -> &AHashMap<InstrumentId, Decimal> {
        &self.max_notional_per_order
    }

    fn config_as_map(&self) -> IndexMap<String, String> {
        let mut map = IndexMap::new();
        map.insert("bypass".to_string(), self.config.bypass.to_string());
        map.insert(
            "max_order_submit_rate".to_string(),
            self.config.max_order_submit.to_string(),
        );
        map.insert(
            "max_order_modify_rate".to_string(),
            self.config.max_order_modify.to_string(),
        );

        for (instrument_id, value) in &self.max_notional_per_order {
            map.insert(
                format!("max_notional_per_order.{instrument_id}"),
                value.to_string(),
            );
        }

        let mut full_position_exit_venues = self
            .config
            .full_position_exit_venues
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        full_position_exit_venues.sort_unstable();
        map.insert(
            "full_position_exit_venues".to_string(),
            full_position_exit_venues.join(","),
        );

        map.insert("debug".to_string(), self.config.debug.to_string());
        map
    }

    fn handle_command(&mut self, command: TradingCommand) {
        if self.config.debug {
            log::debug!("{CMD}{RECV} {command}");
        }

        match command {
            TradingCommand::SubmitOrder(submit_order) => self.handle_submit_order(submit_order),
            TradingCommand::SubmitOrderList(submit_order_list) => {
                self.handle_submit_order_list(submit_order_list);
            }
            TradingCommand::ModifyOrder(modify_order) => self.handle_modify_order(modify_order),
            TradingCommand::ModifyOrders(modify_orders) => {
                self.handle_batch_modify_orders(modify_orders);
            }
            TradingCommand::QueryAccount(query_account) => {
                Self::send_to_execution(TradingCommand::QueryAccount(query_account));
            }
            _ => {
                log::error!("Cannot handle command: {command}");
            }
        }
    }

    fn handle_submit_order(&mut self, command: SubmitOrder) {
        if self.config.bypass {
            Self::send_to_execution(TradingCommand::SubmitOrder(command));
            return;
        }

        let order = {
            let cache = self.cache.borrow();
            let Some(order) = cache.order(&command.client_order_id) else {
                log::error!(
                    "Cannot handle submit order: order not found in cache for {}",
                    command.client_order_id
                );
                return;
            };
            order.clone()
        };

        if let Some(position_id) = command.position_id
            && order.is_reduce_only()
        {
            let position_exists = {
                let cache = self.cache.borrow();
                cache
                    .position(&position_id)
                    .map(|pos| (pos.side, pos.quantity))
            };

            if let Some((pos_side, pos_quantity)) = position_exists {
                if !order.would_reduce_only(pos_side, pos_quantity) {
                    self.deny_command(
                        TradingCommand::SubmitOrder(command),
                        &OrderDeniedReason::ReduceOnlyWouldIncreasePosition { position_id }
                            .to_string(),
                    );
                    return; // Denied
                }
            } else {
                self.deny_command(
                    TradingCommand::SubmitOrder(command),
                    &OrderDeniedReason::PositionNotFound { position_id }.to_string(),
                );
                return;
            }
        }

        let instrument_exists = {
            let cache = self.cache.borrow();
            cache.instrument(&command.instrument_id).cloned()
        };

        let Some(instrument) = instrument_exists else {
            self.deny_command(
                TradingCommand::SubmitOrder(command.clone()),
                &OrderDeniedReason::InstrumentNotFound {
                    instrument_id: command.instrument_id,
                }
                .to_string(),
            );
            return; // Denied
        };

        let full_position_exit = self.is_full_position_exit(&command, &instrument, &order);
        if !self.check_order(&instrument, &order, full_position_exit) {
            return; // Denied
        }

        if !self.check_orders_risk(
            &instrument,
            &[order],
            full_position_exit,
            RiskCheck::Submit,
            command.client_id,
        ) {
            return; // Denied
        }

        self.execution_gateway(TradingCommand::SubmitOrder(command));
    }

    fn is_full_position_exit(
        &self,
        command: &SubmitOrder,
        instrument: &InstrumentAny,
        order: &OrderAny,
    ) -> bool {
        if !self
            .config
            .full_position_exit_venues
            .contains(&instrument.id().venue)
        {
            return false;
        }

        if !Self::has_full_position_exit_intent(command) {
            return false;
        }

        if command.instrument_id != order.instrument_id() {
            return false;
        }

        if !Self::is_full_position_exit_instrument(instrument)
            || !Self::is_full_position_exit_order(order)
        {
            return false;
        }

        self.reduces_identified_open_position(command, order)
    }

    fn has_full_position_exit_intent(command: &SubmitOrder) -> bool {
        command
            .params
            .as_ref()
            .and_then(|params| params.get_bool(PARAMS_CLOSE_POSITION))
            .unwrap_or(false)
    }

    fn is_full_position_exit_instrument(instrument: &InstrumentAny) -> bool {
        match instrument {
            InstrumentAny::CryptoFuture(_) | InstrumentAny::CryptoPerpetual(_) => true,
            InstrumentAny::PerpetualContract(_) => !instrument.is_inverse(),
            _ => false,
        }
    }

    fn is_full_position_exit_order(order: &OrderAny) -> bool {
        matches!(
            order.order_type(),
            OrderType::StopMarket | OrderType::MarketIfTouched
        ) && order.trigger_price().is_some()
            && order.is_reduce_only()
            && order.quantity().is_positive()
    }

    fn is_reducing_submission(&self, command: &SubmitOrder, order: &OrderAny) -> bool {
        order.is_reduce_only()
            && order.quantity().is_positive()
            && command.instrument_id == order.instrument_id()
            && self
                .identified_open_position(command, order)
                .is_some_and(|(side, quantity)| {
                    order.would_reduce_only(side, quantity) && order.quantity() <= quantity
                })
    }

    fn reduces_identified_open_position(&self, command: &SubmitOrder, order: &OrderAny) -> bool {
        self.identified_open_position(command, order)
            .is_some_and(|(side, quantity)| order.would_reduce_only(side, quantity))
    }

    fn identified_open_position(
        &self,
        command: &SubmitOrder,
        order: &OrderAny,
    ) -> Option<(PositionSide, Quantity)> {
        let position_id = command.position_id?;
        let account_id =
            self.order_account_id(order, command.client_id, command.instrument_id.venue);

        let position = {
            let cache = self.cache.borrow();
            if cache.position_id(&order.client_order_id()).copied() != Some(position_id) {
                return None;
            }

            cache
                .position(&position_id)
                .filter(|position| account_id == Some(position.account_id))
                .map(|position| {
                    (
                        position.is_open(),
                        position.instrument_id,
                        position.side,
                        position.quantity,
                    )
                })
        };
        let (is_open, position_instrument_id, position_side, position_quantity) = position?;

        (is_open
            && position_instrument_id == order.instrument_id()
            && matches!(
                (order.order_side(), position_side),
                (OrderSide::Buy, PositionSide::Short) | (OrderSide::Sell, PositionSide::Long)
            ))
        .then_some((position_side, position_quantity))
    }

    fn handle_submit_order_list(&mut self, command: SubmitOrderList) {
        if self.config.bypass {
            Self::send_to_execution(TradingCommand::SubmitOrderList(command));
            return;
        }

        let orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_for_ids(&command.order_list.client_order_ids, &command);

        if orders.len() != command.order_list.client_order_ids.len() {
            self.deny_order_list(
                &orders,
                &OrderDeniedReason::OrderListIncomplete {
                    order_list_id: command.order_list.id,
                }
                .to_string(),
            );
            return; // Denied
        }

        // Per-order checks use each order's own instrument; the cumulative
        // risk check uses the representative. See docs/concepts/orders.md
        // (Order lists -> Caveats for mixed-instrument lists).
        let mut instruments: AHashMap<InstrumentId, InstrumentAny> = AHashMap::new();

        for order in &orders {
            let instrument_id = order.instrument_id();
            if instruments.contains_key(&instrument_id) {
                continue;
            }
            let resolved = self.cache.borrow().instrument(&instrument_id).cloned();
            let Some(instrument) = resolved else {
                self.deny_command(
                    TradingCommand::SubmitOrderList(command),
                    &OrderDeniedReason::InstrumentNotFound { instrument_id }.to_string(),
                );
                return; // Denied
            };
            instruments.insert(instrument_id, instrument);
        }

        for order in &orders {
            let Some(instrument) = instruments.get(&order.instrument_id()) else {
                self.deny_order(
                    order,
                    &OrderDeniedReason::InstrumentNotFound {
                        instrument_id: order.instrument_id(),
                    }
                    .to_string(),
                );
                return; // Denied
            };

            if !self.check_order(instrument, order, false) {
                return; // Denied
            }
        }

        let representative = if let Some(instrument) = instruments.get(&command.instrument_id) {
            instrument.clone()
        } else {
            self.deny_order_list(
                &orders,
                &OrderDeniedReason::InstrumentNotFound {
                    instrument_id: command.instrument_id,
                }
                .to_string(),
            );
            return; // Denied
        };

        if !self.check_orders_risk(
            &representative,
            &orders,
            false,
            RiskCheck::Submit,
            command.client_id,
        ) {
            self.deny_order_list(
                &orders,
                &OrderDeniedReason::OrderListDenied {
                    order_list_id: command.order_list.id,
                }
                .to_string(),
            );
            return; // Denied
        }

        self.execution_gateway(TradingCommand::SubmitOrderList(command));
    }

    fn handle_modify_order(&mut self, command: ModifyOrder) {
        if self.config.bypass {
            Self::send_to_execution(TradingCommand::ModifyOrder(command));
            return;
        }

        if !self.validate_modify_order(&command)
            || !self.check_modify_orders_risk(std::slice::from_ref(&command), command.client_id)
        {
            return;
        }

        self.throttler_modify.send(command);
    }

    fn handle_batch_modify_orders(&mut self, command: BatchModifyOrders) {
        if self.config.bypass {
            Self::send_to_execution(TradingCommand::ModifyOrders(command));
            return;
        }

        if command.modifies.is_empty() {
            log::warn!("Cannot handle BatchModifyOrders: no modify commands");
            return;
        }

        if !self.validate_batch_modify_orders(&command) {
            return;
        }

        if !self.check_modify_orders_risk(&command.modifies, command.client_id) {
            return;
        }

        if !self.throttler_modify.try_reserve(command.modifies.len()) {
            let reason = "Exceeded MAX_ORDER_MODIFY_RATE";

            for modify in &command.modifies {
                let Some(order) = Self::get_existing_order(&self.cache, modify) else {
                    continue;
                };

                self.reject_modify_order(&order, reason);
            }

            return;
        }

        Self::send_to_execution(TradingCommand::ModifyOrders(command));
    }

    fn validate_batch_modify_orders(&self, command: &BatchModifyOrders) -> bool {
        let mut rejected_client_order_ids = Vec::new();
        let mut valid = true;

        for modify in &command.modifies {
            if !self.validate_batch_modify_order(command, modify) {
                rejected_client_order_ids.push(modify.client_order_id);
                valid = false;
            }
        }

        if !valid {
            let reason = "BatchModifyOrders rejected because one or more child modifications failed validation";

            for modify in &command.modifies {
                if rejected_client_order_ids.contains(&modify.client_order_id) {
                    continue;
                }

                let Some(order) = Self::get_existing_order(&self.cache, modify) else {
                    continue;
                };

                self.reject_modify_order(&order, reason);
            }

            return false;
        }

        true
    }

    fn validate_batch_modify_order(
        &self,
        command: &BatchModifyOrders,
        modify: &ModifyOrder,
    ) -> bool {
        if modify.instrument_id != command.instrument_id {
            let order = self
                .cache
                .borrow()
                .order(&modify.client_order_id)
                .map(|order| order.clone());

            if let Some(order) = order {
                self.reject_modify_order(
                    &order,
                    &format!(
                        "BatchModifyOrders instrument {} does not match child instrument {}",
                        command.instrument_id, modify.instrument_id
                    ),
                );
            }

            return false;
        }

        self.validate_modify_order(modify)
    }

    fn validate_modify_order(&self, command: &ModifyOrder) -> bool {
        let order_exists = {
            let cache = self.cache.borrow();
            cache.order(&command.client_order_id).map(|o| o.clone())
        };

        let Some(order) = order_exists else {
            log::error!(
                "ModifyOrder DENIED: Order with command.client_order_id: {} not found",
                command.client_order_id
            );
            return false;
        };

        if order.is_closed() {
            self.reject_modify_order(
                &order,
                &format!(
                    "Order with command.client_order_id: {} already closed",
                    command.client_order_id
                ),
            );
            return false;
        } else if order.status() == OrderStatus::PendingCancel {
            self.reject_modify_order(
                &order,
                &format!(
                    "Order with command.client_order_id: {} is already pending cancel",
                    command.client_order_id
                ),
            );
            return false;
        }

        let maybe_instrument = {
            let cache = self.cache.borrow();
            cache.instrument(&command.instrument_id).cloned()
        };

        let Some(instrument) = maybe_instrument else {
            self.reject_modify_order(
                &order,
                &format!("no instrument found for {:?}", command.instrument_id),
            );
            return false;
        };

        // Check Price
        let mut reason = Self::check_price(&instrument, command.price, OrderPriceField::Price);
        if let Some(reason) = reason {
            self.reject_modify_order(&order, &reason.to_string());
            return false;
        }

        // Check Trigger
        reason = Self::check_price(
            &instrument,
            command.trigger_price,
            OrderPriceField::TriggerPrice,
        );

        if let Some(reason) = reason {
            self.reject_modify_order(&order, &reason.to_string());
            return false;
        }

        // Check Quantity
        reason = Self::check_quantity(
            &instrument,
            command.quantity,
            order.is_quote_quantity(),
            false,
        );

        if let Some(reason) = reason {
            self.reject_modify_order(&order, &reason.to_string());
            return false;
        }

        let state_reason = match self.trading_state {
            TradingState::Halted => Some(OrderDeniedReason::TradingHalted.to_string()),
            TradingState::Reducing => Some(
                OrderDeniedReason::TradingStateReducing {
                    order_side: order.order_side(),
                    instrument_id: instrument.id(),
                }
                .to_string(),
            ),
            TradingState::Active => None,
        };

        if let Some(reason) = state_reason {
            self.reject_modify_order(&order, &reason);
            return false;
        }

        true
    }

    fn check_modify_orders_risk(
        &self,
        commands: &[ModifyOrder],
        client_id: Option<ClientId>,
    ) -> bool {
        let mut originals = Vec::with_capacity(commands.len());
        let mut orders = Vec::with_capacity(commands.len());
        let cache = self.cache.borrow();
        for command in commands {
            let Some(order) = cache.order(&command.client_order_id) else {
                return false;
            };

            originals.push(order.clone());
            let mut projected = order.clone();

            // Project values without applying a venue event or changing the cached order
            projected.update(&OrderUpdated::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                command.quantity.unwrap_or(order.quantity()),
                command.command_id,
                command.ts_init,
                command.ts_init,
                false,
                order.venue_order_id(),
                order.account_id(),
                command.price.filter(|_| {
                    LIMIT_ORDER_TYPES.contains(&order.order_type())
                        || order.order_type() == OrderType::MarketToLimit
                }),
                command.trigger_price.filter(|_| {
                    STOP_ORDER_TYPES.contains(&order.order_type())
                        || matches!(
                            order.order_type(),
                            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
                        )
                }),
                None,
                order.is_quote_quantity(),
            ));

            orders.push(projected);
        }

        let instrument = cache.instrument(&commands[0].instrument_id).cloned();
        drop(cache);
        let check = RiskCheck::Modify(&originals);

        let Some(instrument) = instrument else {
            return false;
        };

        self.check_orders_risk(&instrument, &orders, false, check, client_id)
    }

    fn check_order(
        &self,
        instrument: &InstrumentAny,
        order: &OrderAny,
        full_position_exit: bool,
    ) -> bool {
        if !self.check_order_price(instrument, order)
            || !self.check_order_quantity(instrument, order, full_position_exit)
        {
            return false; // Denied
        }

        if order.time_in_force() == TimeInForce::Gtd {
            let Some(expire_time) = order.expire_time() else {
                self.deny_order(order, &OrderDeniedReason::MissingExpireTime.to_string());
                return false; // Denied
            };

            if expire_time <= self.clock.borrow().timestamp_ns() {
                self.deny_order(
                    order,
                    &OrderDeniedReason::ExpireTimeInPast {
                        expire_time: expire_time.to_rfc3339(),
                    }
                    .to_string(),
                );
                return false; // Denied
            }
        }

        true
    }

    fn check_order_price(&self, instrument: &InstrumentAny, order: &OrderAny) -> bool {
        if order.price().is_some() {
            let reason = Self::check_price(instrument, order.price(), OrderPriceField::Price);
            if let Some(reason) = reason {
                self.deny_order(order, &reason.to_string());
                return false; // Denied
            }
        }

        if order.trigger_price().is_some() {
            let reason = Self::check_price(
                instrument,
                order.trigger_price(),
                OrderPriceField::TriggerPrice,
            );

            if let Some(reason) = reason {
                self.deny_order(order, &reason.to_string());
                return false; // Denied
            }
        }

        true
    }

    fn check_order_quantity(
        &self,
        instrument: &InstrumentAny,
        order: &OrderAny,
        full_position_exit: bool,
    ) -> bool {
        let reason = Self::check_quantity(
            instrument,
            Some(order.quantity()),
            order.is_quote_quantity(),
            full_position_exit,
        );

        if let Some(reason) = reason {
            self.deny_order(order, &reason.to_string());
            return false; // Denied
        }

        true
    }

    fn check_orders_risk(
        &self,
        instrument: &InstrumentAny,
        orders: &[OrderAny],
        full_position_exit: bool,
        check: RiskCheck<'_>,
        client_id: Option<ClientId>,
    ) -> bool {
        let venue = instrument.id().venue;
        let mut orders_by_account: AHashMap<Option<AccountId>, Vec<&OrderAny>> = AHashMap::new();
        for order in orders {
            orders_by_account
                .entry(self.order_account_id(order, client_id, venue))
                .or_default()
                .push(order);
        }

        for (account_id, account_orders) in &orders_by_account {
            if !self.check_orders_risk_for_account(
                instrument,
                account_orders,
                *account_id,
                client_id,
                full_position_exit,
                check,
            ) {
                return false;
            }
        }

        true
    }

    // An order without an assigned account uses the account of the client that command routing
    // selects: a registered command client, else the venue route or default client. An external
    // client, or a command no registered client handles, uses the single account issued under the
    // venue.
    fn order_account_id(
        &self,
        order: &OrderAny,
        client_id: Option<ClientId>,
        venue: Venue,
    ) -> Option<AccountId> {
        if let Some(account_id) = order.account_id() {
            return Some(account_id);
        }

        let cache = self.cache.borrow();

        let routed_account_id = || {
            cache
                .client_id_for_venue(&venue)
                .and_then(|client_id| cache.account_id_for_client(client_id))
        };

        let account_id = match client_id {
            Some(client_id) if cache.is_external_client(&client_id) => None,
            Some(client_id) => cache
                .account_id_for_client(&client_id)
                .or_else(routed_account_id),
            None => routed_account_id(),
        };

        account_id.or_else(|| cache.account_id(&venue)).copied()
    }

    #[allow(
        clippy::too_many_lines,
        reason = "risk checks keep related denial branches together for auditability"
    )]
    fn check_orders_risk_for_account(
        &self,
        instrument: &InstrumentAny,
        orders: &[&OrderAny],
        account_id: Option<AccountId>,
        client_id: Option<ClientId>,
        full_position_exit: bool,
        check: RiskCheck<'_>,
    ) -> bool {
        let max_notional = match self.order_notional_limit(instrument) {
            Ok(limit) => limit,
            Err(reason) => {
                check.reject_orders(self, orders, &reason.to_string());
                return false;
            }
        };

        let mut market_prices = Vec::with_capacity(orders.len());

        for order in orders {
            let price = match order {
                OrderAny::Market(_) | OrderAny::MarketToLimit(_) => {
                    self.market_order_price(instrument.id(), order.order_side())
                }
                _ => None,
            };

            market_prices.push(price);
        }

        let resolved_account = account_id.and_then(|account_id| {
            self.cache
                .borrow()
                .account(&account_id)
                .map(|account| account.clone_without_events())
        });

        let Some(account) = resolved_account else {
            check.reject_orders(
                self,
                orders,
                &OrderDeniedReason::ValidationFailed {
                    detail: format!(
                        "No account available for risk checks: instrument_id={}, client_id={client_id:?}, account_id={account_id:?}",
                        instrument.id()
                    ),
                }
                .to_string(),
            );

            return false;
        };

        let allow_borrowing = match &account {
            AccountAny::Cash(cash) => cash.allow_borrowing,
            AccountAny::Margin(_) | AccountAny::Betting(_) | AccountAny::Wallet(_) => false,
        };

        let available_long_qty_raw = self.available_position_quantity(
            instrument.id(),
            account.id(),
            PositionSide::Long,
            OrderSide::Sell,
            check,
        );

        let available_short_qty_raw =
            if matches!(account, AccountAny::Margin(_) | AccountAny::Betting(_)) {
                self.available_position_quantity(
                    instrument.id(),
                    account.id(),
                    PositionSide::Short,
                    OrderSide::Buy,
                    check,
                )
            } else {
                0
            };

        let mut risk = AccountRisk {
            engine: self,
            instrument,
            account,
            check,
            full_position_exit,
            max_notional,
            allow_borrowing,
            available_long_qty_raw,
            available_short_qty_raw,
            cum_sell_qty_raw: 0,
            cum_buy_qty_raw: 0,
            cum_original_sell_qty_raw: 0,
            cum_original_buy_qty_raw: 0,
            cum_notional_buy: None,
            cum_notional_sell: None,
            cum_margin_required: None,
        };

        for (&order, market_price) in orders.iter().zip(market_prices) {
            if !risk.check_order(order, market_price) {
                return false;
            }
        }

        true
    }

    fn order_notional_limit(
        &self,
        instrument: &InstrumentAny,
    ) -> Result<Option<Money>, OrderDeniedReason> {
        let Some(value) = self.max_notional_per_order.get(&instrument.id()).copied() else {
            return Ok(None);
        };

        Money::from_decimal(value, instrument.quote_currency())
            .map(Some)
            .map_err(|_| OrderDeniedReason::InvalidMaxNotionalPerOrder {
                instrument_id: instrument.id(),
                value,
            })
    }

    fn available_position_quantity(
        &self,
        instrument_id: InstrumentId,
        account_id: AccountId,
        position_side: PositionSide,
        order_side: OrderSide,
        check: RiskCheck<'_>,
    ) -> QuantityRaw {
        let cache = self.cache.borrow();
        let position_quantity: QuantityRaw = cache
            .positions_open(
                None,
                Some(&instrument_id),
                None,
                Some(&account_id),
                Some(position_side),
            )
            .iter()
            .map(|position| position.quantity.raw())
            .sum();
        let pending_quantity: QuantityRaw = cache
            .orders_open(
                None,
                Some(&instrument_id),
                None,
                Some(&account_id),
                Some(order_side),
            )
            .iter()
            .filter(|order| check.original(order).is_none())
            .map(|order| order.leaves_qty().raw())
            .sum();
        let available = position_quantity.saturating_sub(pending_quantity);

        if self.config.debug && position_quantity > 0 {
            log::debug!(
                "Net {position_side} qty (raw): {position_quantity}, pending {order_side}: {pending_quantity}, available: {available}"
            );
        }

        available
    }

    fn order_risk_quantity(
        &self,
        check: RiskCheck<'_>,
        instrument: &InstrumentAny,
        order: &OrderAny,
        quantity: Quantity,
        price: Price,
    ) -> Result<Quantity, ()> {
        if !order.is_quote_quantity() || instrument.is_inverse() {
            return Ok(quantity);
        }

        let effective_price = if matches!(order, OrderAny::Limit(_) | OrderAny::StopLimit(_)) {
            self.cache
                .borrow()
                .quote(&instrument.id())
                .map_or(price, |quote| match order.order_side() {
                    OrderSide::Buy => price.min(quote.ask_price),
                    OrderSide::Sell => price.max(quote.bid_price),
                })
        } else {
            price
        };

        instrument
            .try_calculate_base_quantity(quantity, effective_price)
            .map_err(|e| {
                check.reject(
                    self,
                    order,
                    &OrderDeniedReason::QuantityConversionFailed {
                        detail: e.to_string(),
                    }
                    .to_string(),
                );
            })
    }

    fn check_risk_increase(
        &self,
        check: RiskCheck<'_>,
        order: &OrderAny,
        current: Money,
        previous: Money,
    ) -> Option<Money> {
        // A reduction cannot fund another amendment before the venue acknowledges it
        let increase = if current.currency == previous.currency {
            Money::from_decimal(
                (current.as_decimal().max(Decimal::ZERO)
                    - previous.as_decimal().max(Decimal::ZERO))
                .max(Decimal::ZERO),
                current.currency,
            )
            .ok()
        } else {
            None
        };

        if increase.is_none() {
            check.reject(
                self,
                order,
                &OrderDeniedReason::NotionalCalculationFailed {
                    detail:
                        "amendment risk increase exceeds Money bounds or has incompatible currency"
                            .to_string(),
                }
                .to_string(),
            );
        }

        increase
    }

    fn market_order_price(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
    ) -> Option<Price> {
        let price_type = match order_side {
            OrderSide::Buy => PriceType::Ask,
            OrderSide::Sell => PriceType::Bid,
        };

        let cache = self.cache.borrow();

        if let Some(price) = cache.price(&instrument_id, price_type) {
            return Some(price);
        }

        if let Some(price) = cache.price(&instrument_id, PriceType::Last) {
            return Some(price);
        }

        let bar_price = |price_type| {
            cache
                .bar_types(
                    Some(&instrument_id),
                    Some(&price_type),
                    AggregationSource::External,
                )
                .into_iter()
                .filter_map(|bar_type| {
                    cache
                        .bar(bar_type)
                        .map(|bar| (bar.ts_init, *bar_type, bar.close))
                })
                .max_by_key(|(ts_init, bar_type, _)| (*ts_init, *bar_type))
                .map(|(_, _, price)| price)
        };

        bar_price(price_type).or_else(|| bar_price(PriceType::Last))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "cash sell validation shares the account, cumulative exposure, and rejection context"
    )]
    fn check_cash_sell_balance(
        &self,
        check: RiskCheck<'_>,
        account: &dyn Account,
        allow_borrowing: bool,
        order: &OrderAny,
        quantity: Quantity,
        base_currency: Currency,
        cum_notional_sell: &mut Option<Money>,
    ) -> bool {
        let base_free = account
            .balance_free(Some(base_currency))
            .unwrap_or_else(|| Money::zero(base_currency));

        let cash_value = match Money::from_quantity(quantity, base_free.currency) {
            Ok(value) => value,
            Err(e) => {
                check.reject(
                    self,
                    order,
                    &OrderDeniedReason::QuantityConversionFailed {
                        detail: e.to_string(),
                    }
                    .to_string(),
                );

                return false;
            }
        };

        if self.config.debug {
            log::debug!("Cash value: {cash_value:?}");
            log::debug!("Total: {:?}", account.balance_total(Some(base_currency)));
            log::debug!("Locked: {:?}", account.balance_locked(Some(base_currency)));
            log::debug!("Free: {base_free:?}");
        }

        if !self.accumulate_notional(check, order, cum_notional_sell, cash_value) {
            return false;
        }

        if self.config.debug {
            log::debug!("Cumulative notional SELL: {cum_notional_sell:?}");
        }

        if !allow_borrowing
            && let Some(cum_notional_sell) = *cum_notional_sell
            && cum_notional_sell > base_free
        {
            check.reject(
                self,
                order,
                &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                    free_balance: base_free,
                    cumulative_notional: cum_notional_sell,
                }
                .to_string(),
            );
            return false;
        }

        true
    }

    fn accumulate_notional(
        &self,
        check: RiskCheck<'_>,
        order: &OrderAny,
        total: &mut Option<Money>,
        value: Money,
    ) -> bool {
        let next = match *total {
            Some(current) if current.currency == value.currency => current.checked_add(value),
            Some(_) => None,
            None => Some(value),
        };

        let Some(next) = next else {
            check.reject(self,
                order,
                &OrderDeniedReason::NotionalCalculationFailed {
                    detail: "cumulative notional exceeds Money bounds or has incompatible currency or scale".to_string(),
                }
                .to_string(),
            );

            return false;
        };

        *total = Some(next);
        true
    }

    fn deny_no_market_price(
        &self,
        instrument_id: InstrumentId,
        order: &OrderAny,
        check: RiskCheck<'_>,
    ) {
        check.reject(
            self,
            order,
            &OrderDeniedReason::MarketPriceUnavailable {
                order_type: order.order_type(),
                instrument_id,
            }
            .to_string(),
        );
    }

    fn check_price(
        instrument: &InstrumentAny,
        price: Option<Price>,
        field: OrderPriceField,
    ) -> Option<OrderDeniedReason> {
        let price_val = price?;

        if price_val.precision > instrument.price_precision() {
            return Some(OrderDeniedReason::PricePrecisionExceedsMaximum {
                field,
                price: price_val,
                price_precision: price_val.precision,
                max_precision: instrument.price_precision(),
            });
        }

        if !instrument.allows_negative_price() && (price_val.is_zero() || price_val.is_negative()) {
            return Some(OrderDeniedReason::PriceNotPositive {
                field,
                price: price_val,
            });
        }

        None
    }

    fn check_quantity(
        instrument: &InstrumentAny,
        quantity: Option<Quantity>,
        is_quote_quantity: bool,
        full_position_exit: bool,
    ) -> Option<OrderDeniedReason> {
        let quantity_val = quantity?;

        // Check precision
        if quantity_val.precision > instrument.size_precision() {
            return Some(OrderDeniedReason::QuantityPrecisionExceedsMaximum {
                quantity: quantity_val,
                quantity_precision: quantity_val.precision,
                max_precision: instrument.size_precision(),
            });
        }

        // Base-quantity bounds do not apply to quote-denominated or validated whole-position
        // exits. Applicable quote-quantity notional limits are checked during account risk.
        if is_quote_quantity || full_position_exit {
            return None;
        }

        // Check maximum quantity
        if let Some(max_quantity) = instrument.max_quantity()
            && quantity_val > max_quantity
        {
            return Some(OrderDeniedReason::QuantityExceedsMaximum {
                effective_quantity: quantity_val,
                max_quantity,
            });
        }

        // Check minimum quantity
        if let Some(min_quantity) = instrument.min_quantity()
            && quantity_val < min_quantity
        {
            return Some(OrderDeniedReason::QuantityBelowMinimum {
                effective_quantity: quantity_val,
                min_quantity,
            });
        }

        None
    }

    fn deny_command(&self, command: TradingCommand, reason: &str) {
        match command {
            TradingCommand::SubmitOrder(command) => {
                let order = {
                    let cache = self.cache.borrow();
                    cache.order(&command.client_order_id).map(|o| o.clone())
                };

                if let Some(ref order) = order {
                    self.deny_order(order, reason);
                } else {
                    log::error!(
                        "Cannot deny order: not found in cache for {}",
                        command.client_order_id
                    );
                }
            }
            TradingCommand::SubmitOrderList(command) => {
                let orders: Vec<OrderAny> = self
                    .cache
                    .borrow()
                    .orders_for_ids(&command.order_list.client_order_ids, &command);
                self.deny_order_list(&orders, reason);
            }
            _ => {
                log::error!("Cannot deny command {command}");
            }
        }
    }

    fn deny_order(&self, order: &OrderAny, reason: &str) {
        log::warn!(
            "SubmitOrder for {} DENIED: {}",
            order.client_order_id(),
            reason
        );

        if order.status() != OrderStatus::Initialized {
            return;
        }

        // Scope the cache borrow to avoid RefCell conflict when sending to ExecEngine
        {
            let mut cache = self.cache.borrow_mut();
            if !cache.order_exists(&order.client_order_id())
                && let Err(e) = cache.add_order(order.clone(), None, None, false)
            {
                log::error!("Cannot add order to cache: {e}");
                return;
            }
        }

        let denied = OrderEventAny::Denied(OrderDenied::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            self.clock.borrow().timestamp_ns(),
        ));

        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, denied);
    }

    fn deny_order_list(&self, orders: &[OrderAny], reason: &str) {
        for order in orders {
            if !order.is_closed() {
                self.deny_order(order, reason);
            }
        }
    }

    fn reject_modify_order(&self, order: &OrderAny, reason: &str) {
        let ts_event = self.clock.borrow().timestamp_ns();
        let denied = OrderEventAny::ModifyRejected(OrderModifyRejected::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_event,
            ts_event,
            false,
            order.venue_order_id(),
            order.account_id(),
        ));

        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, denied);
    }

    fn execution_gateway(&mut self, command: TradingCommand) {
        match self.trading_state {
            TradingState::Halted => match command {
                TradingCommand::SubmitOrder(submit_order) => {
                    let order = {
                        let cache = self.cache.borrow();
                        cache
                            .order(&submit_order.client_order_id)
                            .map(|order| order.clone())
                    };

                    if let Some(order) = order {
                        self.deny_order(&order, &OrderDeniedReason::TradingHalted.to_string());
                    }
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    let orders: Vec<OrderAny> = self.cache.borrow().orders_for_ids(
                        &submit_order_list.order_list.client_order_ids,
                        &submit_order_list,
                    );
                    self.deny_order_list(&orders, &OrderDeniedReason::TradingHalted.to_string());
                }
                _ => {}
            },
            TradingState::Reducing => match command {
                TradingCommand::SubmitOrder(submit_order) => {
                    let order = {
                        let cache = self.cache.borrow();
                        cache
                            .order(&submit_order.client_order_id)
                            .map(|order| order.clone())
                    };
                    let Some(order) = order else {
                        return;
                    };

                    if self.is_reducing_submission(&submit_order, &order) {
                        self.throttler_submit
                            .send(TradingCommand::SubmitOrder(submit_order));
                    } else {
                        self.deny_order(
                            &order,
                            &OrderDeniedReason::TradingStateReducing {
                                order_side: order.order_side(),
                                instrument_id: order.instrument_id(),
                            }
                            .to_string(),
                        );
                    }
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    let orders: Vec<OrderAny> = self.cache.borrow().orders_for_ids(
                        &submit_order_list.order_list.client_order_ids,
                        &submit_order_list,
                    );

                    for order in &orders {
                        self.deny_order(
                            order,
                            &OrderDeniedReason::TradingStateReducing {
                                order_side: order.order_side(),
                                instrument_id: order.instrument_id(),
                            }
                            .to_string(),
                        );
                    }
                }
                _ => {}
            },
            TradingState::Active => match command {
                TradingCommand::SubmitOrder(_) | TradingCommand::SubmitOrderList(_) => {
                    self.throttler_submit.send(command);
                }
                _ => {}
            },
        }
    }

    fn send_to_execution(command: TradingCommand) {
        let endpoint = MessagingSwitchboard::exec_engine_queue_execute();
        msgbus::send_trading_command(endpoint, command);
    }

    fn handle_event(&self, event: &OrderEventAny) {
        // We intend to extend the risk engine to be able to handle additional events.
        // For now we just log.
        if self.config.debug {
            log::debug!("{RECV}{EVT} {event}");
        }
    }

    fn handle_position_event(&self, event: &PositionEvent) {
        if self.config.debug {
            log::debug!("{RECV}{EVT} {event:?}");
        }
    }
}

#[derive(Clone, Copy)]
enum RiskCheck<'a> {
    Submit,
    Modify(&'a [OrderAny]),
}

impl<'a> RiskCheck<'a> {
    fn reject_orders(self, engine: &RiskEngine, orders: &[&OrderAny], reason: &str) {
        for order in orders {
            self.reject(engine, order, reason);

            if matches!(self, Self::Modify(_)) {
                break;
            }
        }
    }

    fn reject(self, engine: &RiskEngine, order: &OrderAny, reason: &str) {
        match self {
            Self::Submit => engine.deny_order(order, reason),
            Self::Modify(originals) => {
                for (index, original) in originals.iter().enumerate() {
                    if originals[..index]
                        .iter()
                        .any(|previous| previous.client_order_id() == original.client_order_id())
                    {
                        continue;
                    }

                    engine.reject_modify_order(original, reason);
                }
            }
        }
    }

    fn original(self, order: &OrderAny) -> Option<&'a OrderAny> {
        match self {
            Self::Submit => None,
            Self::Modify(originals) => originals
                .iter()
                .find(|original| original.client_order_id() == order.client_order_id()),
        }
    }
}

struct AccountRisk<'a> {
    engine: &'a RiskEngine,
    instrument: &'a InstrumentAny,
    account: AccountAny,
    check: RiskCheck<'a>,
    full_position_exit: bool,
    max_notional: Option<Money>,
    allow_borrowing: bool,
    available_long_qty_raw: QuantityRaw,
    available_short_qty_raw: QuantityRaw,
    cum_sell_qty_raw: QuantityRaw,
    cum_buy_qty_raw: QuantityRaw,
    cum_original_sell_qty_raw: QuantityRaw,
    cum_original_buy_qty_raw: QuantityRaw,
    cum_notional_buy: Option<Money>,
    cum_notional_sell: Option<Money>,
    cum_margin_required: Option<Money>,
}

impl AccountRisk<'_> {
    fn check_order(&mut self, order: &OrderAny, market_price: Option<Price>) -> bool {
        let Ok(last_px) = self.order_price(order, market_price) else {
            return false;
        };

        let Some(last_px) = last_px else {
            self.engine
                .deny_no_market_price(self.instrument.id(), order, self.check);
            return false;
        };

        let Ok(effective_quantity) = self.engine.order_risk_quantity(
            self.check,
            self.instrument,
            order,
            order.quantity(),
            last_px,
        ) else {
            return false;
        };

        if !self.check_order_limits(order, effective_quantity, last_px) {
            return false;
        }

        // Caps apply to total size, but only unfilled exposure needs funds on amendment
        let effective_quantity = if matches!(self.check, RiskCheck::Modify(_)) {
            let Ok(quantity) = self.engine.order_risk_quantity(
                self.check,
                self.instrument,
                order,
                order.leaves_qty(),
                last_px,
            ) else {
                return false;
            };

            quantity
        } else {
            effective_quantity
        };

        let Ok(original) = self.original_exposure(order, market_price) else {
            return false;
        };

        // Pending reductions cannot release closing capacity for other amendments
        let reserved_quantity = original.map_or(effective_quantity.raw(), |(_, quantity, _)| {
            quantity.raw().max(effective_quantity.raw())
        });

        if matches!(self.account, AccountAny::Margin(_)) {
            return self.check_margin(
                order,
                effective_quantity,
                last_px,
                original,
                reserved_quantity,
            );
        }

        self.check_balance(
            order,
            effective_quantity,
            last_px,
            original,
            reserved_quantity,
        )
    }

    fn original_exposure(
        &mut self,
        order: &OrderAny,
        market_price: Option<Price>,
    ) -> Result<Option<(Price, Quantity, bool)>, ()> {
        let Some(original) = self.check.original(order).filter(|original| {
            original.is_open()
                || (matches!(self.account, AccountAny::Wallet(_)) && original.is_inflight())
        }) else {
            return Ok(None);
        };

        let original_price = match self.order_price(original, market_price) {
            Ok(Some(price)) => price,
            Ok(None) => {
                self.engine
                    .deny_no_market_price(self.instrument.id(), order, self.check);
                return Err(());
            }
            Err(()) => return Err(()),
        };

        let quantity = self.engine.order_risk_quantity(
            self.check,
            self.instrument,
            original,
            original.leaves_qty(),
            original_price,
        )?;
        let is_reducing = !matches!(self.account, AccountAny::Wallet(_))
            && ((original.is_reduce_only()
                && (matches!(self.account, AccountAny::Margin(_)) || original.is_sell()))
                || (original.is_sell()
                    && self.cum_original_sell_qty_raw + quantity.raw()
                        <= self.available_long_qty_raw)
                || (original.is_buy()
                    && self.cum_original_buy_qty_raw + quantity.raw()
                        <= self.available_short_qty_raw));

        if original.is_sell() {
            self.cum_original_sell_qty_raw += quantity.raw();
        } else {
            self.cum_original_buy_qty_raw += quantity.raw();
        }

        Ok(Some((original_price, quantity, is_reducing)))
    }

    fn order_price(
        &mut self,
        order: &OrderAny,
        market_price: Option<Price>,
    ) -> Result<Option<Price>, ()> {
        match order {
            OrderAny::MarketToLimit(_) if order.price().is_some() => Ok(order.price()),
            OrderAny::Market(_) | OrderAny::MarketToLimit(_) => {
                let Some(price) = market_price else {
                    let is_reducing = !matches!(self.account, AccountAny::Wallet(_))
                        && (order.is_reduce_only()
                            || (order.is_sell()
                                && (self.cum_sell_qty_raw + order.quantity().raw())
                                    <= self.available_long_qty_raw));

                    if !order.is_quote_quantity()
                        && order.is_sell()
                        && !is_reducing
                        && let Some(unleveraged) = cash_or_wallet_account(&self.account)
                        && unleveraged.base_currency().is_none()
                        && let Some(base_currency) = self.instrument.base_currency()
                        && !self.engine.check_cash_sell_balance(
                            self.check,
                            unleveraged,
                            self.allow_borrowing,
                            order,
                            order.quantity(),
                            base_currency,
                            &mut self.cum_notional_sell,
                        )
                    {
                        return Err(());
                    }

                    self.engine
                        .deny_no_market_price(self.instrument.id(), order, self.check);
                    return Err(());
                };

                Ok(Some(price))
            }
            OrderAny::StopMarket(_) | OrderAny::MarketIfTouched(_) => Ok(order.trigger_price()),
            OrderAny::TrailingStopMarket(_) | OrderAny::TrailingStopLimit(_) => {
                self.trailing_order_price(order)
            }
            _ => Ok(order.price()),
        }
    }

    fn trailing_order_price(&self, order: &OrderAny) -> Result<Option<Price>, ()> {
        if let Some(price) = order.trigger_price() {
            return Ok(order.price().or(Some(price)));
        }

        // Validate trailing offset type is supported
        let Some(offset_type) = order.trailing_offset_type() else {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::MissingTrailingOffsetType.to_string(),
            );
            return Err(()); // Denied
        };

        if !matches!(
            offset_type,
            TrailingOffsetType::Price | TrailingOffsetType::BasisPoints | TrailingOffsetType::Ticks
        ) {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::UnsupportedTrailingOffsetType { offset_type }.to_string(),
            );
            return Err(());
        }

        let Some(trigger_type) = order.trigger_type() else {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::MissingTriggerType.to_string(),
            );
            return Err(()); // Denied
        };

        let Some(trailing_offset) = order.trailing_offset() else {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::MissingTrailingOffset.to_string(),
            );
            return Err(()); // Denied
        };

        if let Some(price) = order.price() {
            return Ok(Some(price));
        }

        // Release the cache borrow before publishing a rejection
        self.calculate_trailing_price(order, offset_type, trigger_type, trailing_offset)
            .map_err(|detail| {
                self.check.reject(
                    self.engine,
                    order,
                    &OrderDeniedReason::TrailingStopCalculationFailed { detail }.to_string(),
                );
            })
    }

    fn calculate_trailing_price(
        &self,
        order: &OrderAny,
        offset_type: TrailingOffsetType,
        trigger_type: TriggerType,
        trailing_offset: Decimal,
    ) -> Result<Option<Price>, String> {
        let cache = self.engine.cache.borrow();
        if trigger_type != TriggerType::BidAsk
            && let Some(trade) = cache.trade(&self.instrument.id())
        {
            return trailing_stop_calculate_with_last(
                self.instrument.price_increment(),
                offset_type,
                order.order_side(),
                trailing_offset,
                trade.price,
            )
            .map(Some)
            .map_err(|e| e.to_string());
        }

        if matches!(
            trigger_type,
            TriggerType::BidAsk | TriggerType::LastOrBidAsk
        ) && let Some(quote) = cache.quote(&self.instrument.id())
        {
            return trailing_stop_calculate_with_bid_ask(
                self.instrument.price_increment(),
                offset_type,
                order.order_side(),
                trailing_offset,
                quote.bid_price,
                quote.ask_price,
            )
            .map(Some)
            .map_err(|e| e.to_string());
        }

        if trigger_type == TriggerType::BidAsk {
            log::warn!(
                "Cannot check {} order risk: no trigger price set and no bid/ask quotes available for {}",
                order.order_type(),
                self.instrument.id()
            );
        } else {
            log::warn!(
                "Cannot check {} order risk: no trigger price set and no market data available for {}",
                order.order_type(),
                self.instrument.id()
            );
        }

        Ok(None)
    }

    fn check_order_limits(
        &self,
        order: &OrderAny,
        effective_quantity: Quantity,
        last_px: Price,
    ) -> bool {
        // Base-quantity bounds (`min_quantity`/`max_quantity`) do not apply to
        // quote-denominated orders: the client-side conversion uses an estimated
        // price and may differ from the venue fill, and some venues enforce
        // distinct per-order-type minimums. The venue is authoritative for
        // quote-denominated sizing; rely on `min_notional`/`max_notional` below.
        if !order.is_quote_quantity() && !self.full_position_exit {
            if let Some(max_quantity) = self.instrument.max_quantity()
                && effective_quantity > max_quantity
            {
                self.check.reject(
                    self.engine,
                    order,
                    &OrderDeniedReason::QuantityExceedsMaximum {
                        effective_quantity,
                        max_quantity,
                    }
                    .to_string(),
                );

                return false; // Denied
            }

            if let Some(min_quantity) = self.instrument.min_quantity()
                && effective_quantity < min_quantity
            {
                self.check.reject(
                    self.engine,
                    order,
                    &OrderDeniedReason::QuantityBelowMinimum {
                        effective_quantity,
                        min_quantity,
                    }
                    .to_string(),
                );

                return false; // Denied
            }
        }

        let notional = match self.instrument.try_calculate_notional_value(
            effective_quantity,
            last_px,
            Some(true),
        ) {
            Ok(notional) => notional,
            Err(e) => {
                self.check.reject(
                    self.engine,
                    order,
                    &OrderDeniedReason::NotionalCalculationFailed {
                        detail: e.to_string(),
                    }
                    .to_string(),
                );

                return false;
            }
        };

        if self.engine.config.debug {
            log::debug!("Notional: {notional:?}");
        }

        // Check MAX notional per order limit
        if !self.full_position_exit
            && let Some(max_notional_value) = self.max_notional
            && notional > max_notional_value
        {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::NotionalExceedsMaxPerOrder {
                    max_notional: max_notional_value,
                    notional,
                }
                .to_string(),
            );

            return false; // Denied
        }

        // Whole-position and reduce-only orders may close residual positions below the
        // venue minimum
        if !order.is_reduce_only()
            && !self.full_position_exit
            && let Some(min_notional) = self.instrument.min_notional()
            && notional.currency == min_notional.currency
            && notional < min_notional
        {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::NotionalBelowMinimum {
                    min_notional,
                    notional,
                }
                .to_string(),
            );

            return false; // Denied
        }

        // Check MAX notional instrument limit
        if !self.full_position_exit
            && let Some(max_notional) = self.instrument.max_notional()
            && notional.currency == max_notional.currency
            && notional > max_notional
        {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::NotionalExceedsMaximum {
                    max_notional,
                    notional,
                }
                .to_string(),
            );

            return false; // Denied
        }

        true
    }

    fn check_margin(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
        price: Price,
        original: Option<(Price, Quantity, bool)>,
        reserved_quantity: QuantityRaw,
    ) -> bool {
        let Ok(required) = self.initial_margin(order, quantity, price) else {
            return false;
        };

        if self.engine.config.debug {
            log::debug!("Initial margin required: {required}");
        }

        if self.reserve_position(order, quantity, reserved_quantity) {
            if self.engine.config.debug {
                log::debug!("Position-reducing order skips margin check");
            }

            return true;
        }

        let Ok(required) = self.margin_increase(order, required, original) else {
            return false;
        };

        self.check_margin_balance(order, required)
    }

    fn margin_increase(
        &mut self,
        order: &OrderAny,
        required: Money,
        original: Option<(Price, Quantity, bool)>,
    ) -> Result<Money, ()> {
        let Some((price, quantity, was_reducing)) = original else {
            return Ok(required);
        };

        let previous = if was_reducing {
            Money::zero(required.currency)
        } else {
            self.initial_margin(order, quantity, price)?
        };

        self.engine
            .check_risk_increase(self.check, order, required, previous)
            .ok_or(())
    }

    fn initial_margin(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
        price: Price,
    ) -> Result<Money, ()> {
        let AccountAny::Margin(margin) = &mut self.account else {
            unreachable!()
        };

        margin
            .calculate_initial_margin(self.instrument, quantity, price, None)
            .map_err(|e| {
                self.check.reject(
                    self.engine,
                    order,
                    &OrderDeniedReason::InitialMarginCalculationFailed {
                        detail: e.to_string(),
                    }
                    .to_string(),
                );
            })
    }

    fn check_margin_balance(&mut self, order: &OrderAny, required: Money) -> bool {
        if matches!(self.check, RiskCheck::Modify(_)) && required.is_zero() {
            return true;
        }

        let Ok(required) = self.account_currency_amount(order, required) else {
            return false;
        };

        // Inverse instruments can require collateral in the base currency
        let free = self
            .account
            .balance_free(Some(required.currency))
            .unwrap_or_else(|| Money::zero(required.currency));

        if required > free {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::InitialMarginExceedsFreeBalance {
                    free_balance: free,
                    initial_margin: required,
                }
                .to_string(),
            );

            return false;
        }

        let total = match self.cum_margin_required {
            Some(total) => total.checked_add(required),
            None => Some(required),
        };

        let Some(total) = total else {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::CumulativeInitialMarginCalculationFailed {
                    detail: "total exceeds Money bounds".to_string(),
                }
                .to_string(),
            );

            return false;
        };

        self.cum_margin_required = Some(total);

        if self.engine.config.debug {
            log::debug!("Cumulative margin required: {:?}", self.cum_margin_required);
        }

        if total > free {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::CumulativeInitialMarginExceedsFreeBalance {
                    free_balance: free,
                    cumulative_initial_margin: total,
                }
                .to_string(),
            );

            return false;
        }

        true
    }

    fn check_balance(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
        price: Price,
        original: Option<(Price, Quantity, bool)>,
        reserved_quantity: QuantityRaw,
    ) -> bool {
        let Ok((notional, impact)) = self.balance_impact(order, quantity, price) else {
            return false;
        };

        if self.engine.config.debug {
            log::debug!("Balance impact: {impact}");
        }

        if self.reserve_position(order, quantity, reserved_quantity) {
            if self.engine.config.debug {
                log::debug!("Position-reducing order skips balance check");
            }

            return true;
        }

        let Ok(impact) = self.balance_increase(order, impact, original) else {
            return false;
        };

        let is_debit = order.is_buy() || matches!(self.account, AccountAny::Betting(_));
        if matches!(self.check, RiskCheck::Modify(_))
            && impact.is_zero()
            && (is_debit || self.account.base_currency().is_some())
        {
            return true;
        }

        let Ok(impact) = self.account_currency_amount(order, impact) else {
            return false;
        };

        let free = self
            .account
            .balance_free(Some(impact.currency))
            .unwrap_or_else(|| Money::zero(impact.currency));
        if !self.allow_borrowing && free.as_decimal() + impact.as_decimal() < Decimal::ZERO {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::NotionalExceedsFreeBalance {
                    free_balance: free,
                    notional,
                }
                .to_string(),
            );

            return false;
        }

        if is_debit {
            return self.check_cumulative_balance(order, -impact);
        }

        if self.account.base_currency().is_some() {
            return self.check_cumulative_balance(order, impact);
        }

        self.check_asset_balance(order, quantity, original)
    }

    fn reserve_position(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
        reserved: QuantityRaw,
    ) -> bool {
        let (cumulative, available) = match order.order_side() {
            OrderSide::Buy => (&mut self.cum_buy_qty_raw, self.available_short_qty_raw),
            OrderSide::Sell => (&mut self.cum_sell_qty_raw, self.available_long_qty_raw),
        };

        let reducing = self.full_position_exit
            || (order.is_reduce_only()
                && (matches!(self.account, AccountAny::Margin(_)) || order.is_sell()))
            || *cumulative + quantity.raw() <= available;
        *cumulative += reserved;
        reducing && !matches!(self.account, AccountAny::Wallet(_))
    }

    fn balance_impact(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
        price: Price,
    ) -> Result<(Money, Money), ()> {
        let notional = self
            .instrument
            .try_calculate_notional_value(quantity, price, None)
            .map_err(|e| {
                self.check.reject(
                    self.engine,
                    order,
                    &OrderDeniedReason::NotionalCalculationFailed {
                        detail: e.to_string(),
                    }
                    .to_string(),
                );
            })?;

        let impact = if let AccountAny::Betting(betting) = &mut self.account {
            -betting
                .calculate_balance_locked(
                    self.instrument,
                    order.order_side(),
                    quantity,
                    price,
                    None,
                )
                .map_err(|e| {
                    self.check.reject(
                        self.engine,
                        order,
                        &OrderDeniedReason::BettingBalanceLockedCalculationFailed {
                            detail: e.to_string(),
                        }
                        .to_string(),
                    );
                })?
        } else {
            match order.order_side() {
                OrderSide::Buy => -notional,
                OrderSide::Sell => notional,
            }
        };

        Ok((notional, impact))
    }

    fn balance_increase(
        &mut self,
        order: &OrderAny,
        impact: Money,
        original: Option<(Price, Quantity, bool)>,
    ) -> Result<Money, ()> {
        let Some((price, quantity, was_reducing)) = original else {
            return Ok(impact);
        };

        let previous = if was_reducing {
            Ok(Money::zero(impact.currency))
        } else if let AccountAny::Betting(betting) = &mut self.account {
            betting.calculate_balance_locked(
                self.instrument,
                order.order_side(),
                quantity,
                price,
                None,
            )
        } else {
            self.instrument
                .try_calculate_notional_value(quantity, price, None)
        }
        .map_err(|e| {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::NotionalCalculationFailed {
                    detail: e.to_string(),
                }
                .to_string(),
            );
        })?;

        let is_debit = order.is_buy() || matches!(self.account, AccountAny::Betting(_));
        let current = if is_debit { -impact } else { impact };
        let increase = self
            .engine
            .check_risk_increase(self.check, order, current, previous)
            .ok_or(())?;
        Ok(if is_debit { -increase } else { increase })
    }

    fn check_cumulative_balance(&mut self, order: &OrderAny, required: Money) -> bool {
        let cumulative = match order.order_side() {
            OrderSide::Buy => &mut self.cum_notional_buy,
            OrderSide::Sell => &mut self.cum_notional_sell,
        };

        if !self
            .engine
            .accumulate_notional(self.check, order, cumulative, required)
        {
            return false;
        }

        if self.engine.config.debug {
            log::debug!(
                "Cumulative balance required for {}: {cumulative:?}",
                order.order_side()
            );
        }

        let free = self
            .account
            .balance_free(Some(required.currency))
            .unwrap_or_else(|| Money::zero(required.currency));

        if !self.allow_borrowing
            && let Some(total) = *cumulative
            && total > free
        {
            self.check.reject(
                self.engine,
                order,
                &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                    free_balance: free,
                    cumulative_notional: total,
                }
                .to_string(),
            );

            return false;
        }

        true
    }

    fn account_currency_amount(&self, order: &OrderAny, amount: Money) -> Result<Money, ()> {
        let Some(currency) = self.account.base_currency() else {
            return Ok(amount);
        };

        if amount.currency == currency {
            return Ok(amount);
        }

        if amount.is_zero() {
            return Ok(Money::zero(currency));
        }

        // Match the portfolio's order-funding conversion convention
        let price_type = match order.order_side() {
            OrderSide::Buy => PriceType::Bid,
            OrderSide::Sell => PriceType::Ask,
        };

        let xrate = self.engine.cache.borrow().try_get_xrate(
            self.instrument.id().venue,
            amount.currency,
            currency,
            price_type,
        );
        xrate
            .map_err(|e| e.to_string())
            .and_then(|xrate| {
                let xrate = xrate.ok_or_else(|| {
                    format!("No exchange rate from {} to {currency}", amount.currency)
                })?;

                let value = amount.as_decimal().checked_mul(xrate).ok_or_else(|| {
                    "Account currency conversion exceeds Decimal bounds".to_string()
                })?;

                Money::from_decimal(value, currency).map_err(|e| e.to_string())
            })
            .map_err(|e| {
                self.check.reject(
                    self.engine,
                    order,
                    &OrderDeniedReason::ValidationFailed {
                        detail: format!("Account currency conversion failed: {e}"),
                    }
                    .to_string(),
                );
            })
    }

    fn check_asset_balance(
        &mut self,
        order: &OrderAny,
        quantity: Quantity,
        original: Option<(Price, Quantity, bool)>,
    ) -> bool {
        let Some(base_currency) = self.instrument.base_currency() else {
            return true;
        };

        let Some(account) = cash_or_wallet_account(&self.account) else {
            unreachable!()
        };

        let quantity = match original {
            Some((_, previous, false)) => quantity.saturating_sub(previous),
            _ => quantity,
        };

        self.engine.check_cash_sell_balance(
            self.check,
            account,
            self.allow_borrowing,
            order,
            quantity,
            base_currency,
            &mut self.cum_notional_sell,
        )
    }
}

// Returns cash and wallet accounts for sell-balance checks; margin and betting accounts
// follow their own sell paths.
fn cash_or_wallet_account(account: &AccountAny) -> Option<&dyn Account> {
    match account {
        AccountAny::Cash(cash) => Some(cash),
        AccountAny::Wallet(wallet) => Some(wallet),
        AccountAny::Margin(_) | AccountAny::Betting(_) => None,
    }
}

#[cfg(test)]
mod tests;
