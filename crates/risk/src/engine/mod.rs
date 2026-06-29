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
        execution::{BatchModifyOrders, ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand},
        system::trading::TradingStateChanged,
    },
    msgbus,
    msgbus::{MessagingSwitchboard, TypedHandler, TypedIntoHandler, get_message_bus},
    runner::try_get_trading_cmd_sender,
    throttler::{RateLimit, Throttler},
};
use nautilus_core::{UUID4, WeakCell};
use nautilus_execution::trailing::{
    trailing_stop_calculate_with_bid_ask, trailing_stop_calculate_with_last,
};
use nautilus_model::{
    accounts::{Account, AccountAny},
    enums::{
        OrderSide, OrderStatus, PositionSide, TimeInForce, TradingState, TrailingOffsetType,
        TriggerType,
    },
    events::{OrderDenied, OrderDeniedReason, OrderEventAny, OrderModifyRejected, PositionEvent},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Currency, Money, Price, Quantity, money::MoneyRaw, quantity::QuantityRaw},
};
use nautilus_portfolio::Portfolio;
use rust_decimal::Decimal;
use ustr::Ustr;

fn format_rate_limit(rate_limit: &RateLimit) -> String {
    let interval_ns = rate_limit.interval_ns();
    let limit = rate_limit.limit();
    let total_secs = interval_ns / 1_000_000_000;
    let remainder_ns = interval_ns % 1_000_000_000;
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if remainder_ns == 0 {
        format!("{limit}/{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        let micros = remainder_ns / 1_000;
        format!("{limit}/{hours:02}:{minutes:02}:{seconds:02}.{micros:06}")
    }
}

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
    pub throttled_submit: Throttler<TradingCommand, SubmitCommandFn>,
    pub throttled_modify_order: Throttler<ModifyOrder, ModifyOrderFn>,
    max_notional_per_order: AHashMap<InstrumentId, Decimal>,
    trading_state: TradingState,
    config: RiskEngineConfig,
    command_count: u64,
    event_count: u64,
}

impl Debug for RiskEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RiskEngine)).finish()
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
        let throttled_submit = Self::create_submit_throttler(&config, clock.clone(), cache.clone());

        let throttled_modify_order =
            Self::create_modify_order_throttler(&config, clock.clone(), cache.clone());

        Self {
            clock,
            cache,
            portfolio,
            throttled_submit,
            throttled_modify_order,
            max_notional_per_order: config.max_notional_per_order.clone(),
            trading_state: TradingState::Active,
            config,
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
        // mutable borrow — which would otherwise panic on `RefCell` re-entrancy.
        // In backtest/test mode (no sender), falls back to the direct endpoint.
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::risk_engine_queue_execute(),
            TypedIntoHandler::from(move |cmd: TradingCommand| {
                if let Some(sender) = try_get_trading_cmd_sender() {
                    sender.execute(cmd);
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
                if let Some(rc) = weak_order_events.upgrade() {
                    rc.borrow_mut().process(event.clone());
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
            let clock = clock.clone();
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

    fn create_modify_order_throttler(
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
            let clock = clock.clone();
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
            None,
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

        msgbus::publish_any("events.risk".into(), &event);

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
        self.throttled_submit.reset();
        self.throttled_modify_order.reset();
        self.max_notional_per_order = self.config.max_notional_per_order.clone();
        self.trading_state = TradingState::Active;
        self.command_count = 0;
        self.event_count = 0;

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
            format_rate_limit(&self.config.max_order_submit),
        );
        map.insert(
            "max_order_modify_rate".to_string(),
            format_rate_limit(&self.config.max_order_modify),
        );

        for (instrument_id, value) in &self.max_notional_per_order {
            map.insert(
                format!("max_notional_per_order.{instrument_id}"),
                value.to_string(),
            );
        }

        map.insert("debug".to_string(), self.config.debug.to_string());
        map
    }

    fn handle_command(&mut self, command: TradingCommand) {
        if self.config.debug {
            log::debug!("{CMD}{RECV} {command:?}");
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

        if !self.check_order(&instrument, &order) {
            return; // Denied
        }

        if !self.check_orders_risk(&instrument, &[order]) {
            return; // Denied
        }

        // Route through execution gateway for TradingState checks & throttling
        self.execution_gateway(&instrument, TradingCommand::SubmitOrder(command));
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

            if !self.check_order(instrument, order) {
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

        if !self.check_orders_risk(&representative, &orders) {
            self.deny_order_list(
                &orders,
                &OrderDeniedReason::OrderListDenied {
                    order_list_id: command.order_list.id,
                }
                .to_string(),
            );
            return; // Denied
        }

        self.execution_gateway(&representative, TradingCommand::SubmitOrderList(command));
    }

    fn handle_modify_order(&mut self, command: ModifyOrder) {
        if self.config.bypass {
            Self::send_to_execution(TradingCommand::ModifyOrder(command));
            return;
        }

        if !self.validate_modify_order(&command) {
            return;
        }

        self.throttled_modify_order.send(command);
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

        let mut rejected_client_order_ids = Vec::new();
        let mut valid = true;

        for modify in &command.modifies {
            if modify.instrument_id != command.instrument_id {
                if let Some(order) = self
                    .cache
                    .borrow()
                    .order(&modify.client_order_id)
                    .map(|o| o.clone())
                {
                    self.reject_modify_order(
                        &order,
                        &format!(
                            "BatchModifyOrders instrument {} does not match child instrument {}",
                            command.instrument_id, modify.instrument_id
                        ),
                    );
                }
                rejected_client_order_ids.push(modify.client_order_id);
                valid = false;
                continue;
            }

            if !self.validate_modify_order(modify) {
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
            return;
        }

        if !self
            .throttled_modify_order
            .try_reserve(command.modifies.len())
        {
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
        let mut risk_msg = Self::check_price(&instrument, command.price);
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(&order, &risk_msg);
            return false;
        }

        // Check Trigger
        risk_msg = Self::check_price(&instrument, command.trigger_price);
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(&order, &risk_msg);
            return false;
        }

        // Check Quantity
        risk_msg = Self::check_quantity(&instrument, command.quantity, order.is_quote_quantity());
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(&order, &risk_msg);
            return false;
        }

        // Check TradingState
        match self.trading_state {
            TradingState::Halted => {
                self.reject_modify_order(&order, "TradingState is HALTED: Cannot modify order");
                return false;
            }
            TradingState::Reducing => {
                if let Some(quantity) = command.quantity
                    && quantity > order.quantity()
                    && ((order.is_buy() && self.portfolio.is_net_long(&instrument.id()))
                        || (order.is_sell() && self.portfolio.is_net_short(&instrument.id())))
                {
                    self.reject_modify_order(
                        &order,
                        &format!(
                            "TradingState is REDUCING and update will increase exposure {}",
                            instrument.id()
                        ),
                    );
                    return false;
                }
            }
            TradingState::Active => {}
        }

        true
    }

    fn check_order(&self, instrument: &InstrumentAny, order: &OrderAny) -> bool {
        if !self.check_order_price(instrument, order)
            || !self.check_order_quantity(instrument, order)
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
            let risk_msg = Self::check_price(instrument, order.price());
            if let Some(risk_msg) = risk_msg {
                self.deny_order(order, &risk_msg);
                return false; // Denied
            }
        }

        if order.trigger_price().is_some() {
            let risk_msg = Self::check_price(instrument, order.trigger_price());
            if let Some(risk_msg) = risk_msg {
                self.deny_order(order, &format!("trigger {risk_msg}"));
                return false; // Denied
            }
        }

        true
    }

    fn check_order_quantity(&self, instrument: &InstrumentAny, order: &OrderAny) -> bool {
        let risk_msg = Self::check_quantity(
            instrument,
            Some(order.quantity()),
            order.is_quote_quantity(),
        );

        if let Some(risk_msg) = risk_msg {
            self.deny_order(order, &risk_msg);
            return false; // Denied
        }

        true
    }

    fn check_orders_risk(&self, instrument: &InstrumentAny, orders: &[OrderAny]) -> bool {
        let mut orders_by_account: AHashMap<Option<AccountId>, Vec<&OrderAny>> = AHashMap::new();
        for order in orders {
            orders_by_account
                .entry(order.account_id())
                .or_default()
                .push(order);
        }

        for (account_id, account_orders) in &orders_by_account {
            if !self.check_orders_risk_for_account(instrument, account_orders, *account_id) {
                return false;
            }
        }

        true
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
    ) -> bool {
        let mut last_px: Option<Price> = None;
        let mut max_notional: Option<Money> = None;

        // Determine max notional
        let max_notional_setting = self.max_notional_per_order.get(&instrument.id());
        if let Some(max_notional_setting_val) = max_notional_setting.copied() {
            let Ok(max_notional_value) =
                Money::from_decimal(max_notional_setting_val, instrument.quote_currency())
            else {
                for order in orders {
                    self.deny_order(
                        order,
                        &OrderDeniedReason::InvalidMaxNotionalPerOrder {
                            instrument_id: instrument.id(),
                            value: max_notional_setting_val,
                        }
                        .to_string(),
                    );
                }
                return false; // Denied
            };
            max_notional = Some(max_notional_value);
        }

        // Get account for risk checks: use explicit account_id if provided, otherwise venue lookup
        let resolved_account = {
            let cache = self.cache.borrow();

            if let Some(account_id) = account_id {
                cache.account_owned(&account_id)
            } else {
                cache.account_for_venue_owned(&instrument.id().venue)
            }
        };

        let Some(mut account) = resolved_account else {
            log::debug!(
                "Cannot find account for venue {} (account_id={account_id:?})",
                instrument.id().venue
            );
            return true;
        };

        let is_margin = matches!(account, AccountAny::Margin(_));
        let is_betting = matches!(account, AccountAny::Betting(_));
        let free = match &account {
            AccountAny::Margin(margin) => margin.balance_free(Some(instrument.quote_currency())),
            AccountAny::Cash(cash) => cash.balance_free(Some(instrument.quote_currency())),
            AccountAny::Betting(betting) => betting.balance_free(Some(instrument.quote_currency())),
        };
        let allow_borrowing = match &account {
            AccountAny::Cash(cash) => cash.allow_borrowing,
            AccountAny::Margin(_) | AccountAny::Betting(_) => false,
        };

        if self.config.debug {
            log::debug!("Free balance: {free:?}");
        }

        // Get net LONG position quantity for this instrument (for position-reducing sell checks),
        // accounting for already submitted (but unfilled) SELL orders to prevent overselling.
        let (net_long_qty_raw, pending_sell_qty_raw) = {
            let cache = self.cache.borrow();
            let long_qty: QuantityRaw = cache
                .positions_open(
                    None,
                    Some(&instrument.id()),
                    None,
                    None,
                    Some(PositionSide::Long),
                )
                .iter()
                .map(|pos| pos.quantity.raw)
                .sum();
            let pending_sells: QuantityRaw = cache
                .orders_open(
                    None,
                    Some(&instrument.id()),
                    None,
                    None,
                    Some(OrderSide::Sell),
                )
                .iter()
                .map(|ord| ord.leaves_qty().raw)
                .sum();
            (long_qty, pending_sells)
        };

        // Available quantity is long position minus pending sells
        let available_long_qty_raw = net_long_qty_raw.saturating_sub(pending_sell_qty_raw);

        if self.config.debug && net_long_qty_raw > 0 {
            log::debug!(
                "Net LONG qty (raw): {net_long_qty_raw}, pending sells: {pending_sell_qty_raw}, available: {available_long_qty_raw}"
            );
        }

        // For margin and betting accounts, also track SHORT positions for buy-side reduction
        let available_short_qty_raw = if is_margin || is_betting {
            let cache = self.cache.borrow();
            let short_qty: QuantityRaw = cache
                .positions_open(
                    None,
                    Some(&instrument.id()),
                    None,
                    None,
                    Some(PositionSide::Short),
                )
                .iter()
                .map(|pos| pos.quantity.raw)
                .sum();
            let pending_buys: QuantityRaw = cache
                .orders_open(
                    None,
                    Some(&instrument.id()),
                    None,
                    None,
                    Some(OrderSide::Buy),
                )
                .iter()
                .map(|ord| ord.leaves_qty().raw)
                .sum();

            if self.config.debug && short_qty > 0 {
                log::debug!(
                    "Net SHORT qty (raw): {short_qty}, pending buys: {pending_buys}, available: {}",
                    short_qty.saturating_sub(pending_buys)
                );
            }

            short_qty.saturating_sub(pending_buys)
        } else {
            0
        };

        // Track cumulative quantities to determine position-reducing vs position-opening orders
        let mut cum_sell_qty_raw: QuantityRaw = 0;
        let mut cum_buy_qty_raw: QuantityRaw = 0;

        let mut cum_notional_buy: Option<Money> = None;
        let mut cum_notional_sell: Option<Money> = None;
        let mut cum_margin_required: Option<Money> = None;
        let mut base_currency: Option<Currency> = None;

        for order in orders {
            // Determine last price based on order type
            last_px = match order {
                OrderAny::Market(_) | OrderAny::MarketToLimit(_) => {
                    if last_px.is_none() {
                        let quote_price = {
                            let cache = self.cache.borrow();
                            cache.quote(&instrument.id()).map(|last_quote| {
                                match order.order_side() {
                                    OrderSide::Buy => Ok(last_quote.ask_price),
                                    OrderSide::Sell => Ok(last_quote.bid_price),
                                    OrderSide::NoOrderSide => {
                                        Err(OrderDeniedReason::InvalidOrderSide {
                                            order_side: order.order_side(),
                                        }
                                        .to_string())
                                    }
                                }
                            })
                        };

                        if let Some(quote_price) = quote_price {
                            match quote_price {
                                Ok(price) => Some(price),
                                Err(reason) => {
                                    self.deny_order(order, &reason);
                                    return false; // Denied
                                }
                            }
                        } else {
                            let cache = self.cache.borrow();
                            let last_trade = cache.trade(&instrument.id());

                            if let Some(last_trade) = last_trade {
                                Some(last_trade.price)
                            } else {
                                log::warn!(
                                    "Cannot check MARKET order risk: no prices for {}",
                                    instrument.id()
                                );
                                continue;
                            }
                        }
                    } else {
                        last_px
                    }
                }
                OrderAny::StopMarket(_) | OrderAny::MarketIfTouched(_) => order.trigger_price(),
                OrderAny::TrailingStopMarket(_) | OrderAny::TrailingStopLimit(_) => {
                    if let Some(trigger_price) = order.trigger_price() {
                        Some(trigger_price)
                    } else {
                        // Validate trailing offset type is supported
                        let Some(offset_type) = order.trailing_offset_type() else {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::MissingTrailingOffsetType.to_string(),
                            );
                            return false; // Denied
                        };

                        if !matches!(
                            offset_type,
                            TrailingOffsetType::Price
                                | TrailingOffsetType::BasisPoints
                                | TrailingOffsetType::Ticks
                        ) {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::UnsupportedTrailingOffsetType { offset_type }
                                    .to_string(),
                            );
                            return false;
                        }

                        let Some(trigger_type) = order.trigger_type() else {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::MissingTriggerType.to_string(),
                            );
                            return false; // Denied
                        };
                        let Some(trailing_offset) = order.trailing_offset() else {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::MissingTrailingOffset.to_string(),
                            );
                            return false; // Denied
                        };

                        // Compute trailing stop trigger inside a scoped cache borrow
                        // to avoid RefCell conflict if deny_order is called below
                        let calc_result: Result<Option<Price>, String> = {
                            let cache = self.cache.borrow();

                            if trigger_type == TriggerType::BidAsk {
                                if let Some(quote) = cache.quote(&instrument.id()) {
                                    trailing_stop_calculate_with_bid_ask(
                                        instrument.price_increment(),
                                        offset_type,
                                        order.order_side_specified(),
                                        trailing_offset,
                                        quote.bid_price,
                                        quote.ask_price,
                                    )
                                    .map(Some)
                                    .map_err(|e| e.to_string())
                                } else {
                                    log::warn!(
                                        "Cannot check {} order risk: no trigger price set and no bid/ask quotes available for {}",
                                        order.order_type(),
                                        instrument.id()
                                    );
                                    Ok(None)
                                }
                            } else if let Some(last_trade) = cache.trade(&instrument.id()) {
                                trailing_stop_calculate_with_last(
                                    instrument.price_increment(),
                                    offset_type,
                                    order.order_side_specified(),
                                    trailing_offset,
                                    last_trade.price,
                                )
                                .map(Some)
                                .map_err(|e| e.to_string())
                            } else if trigger_type == TriggerType::LastOrBidAsk {
                                if let Some(quote) = cache.quote(&instrument.id()) {
                                    trailing_stop_calculate_with_bid_ask(
                                        instrument.price_increment(),
                                        offset_type,
                                        order.order_side_specified(),
                                        trailing_offset,
                                        quote.bid_price,
                                        quote.ask_price,
                                    )
                                    .map(Some)
                                    .map_err(|e| e.to_string())
                                } else {
                                    log::warn!(
                                        "Cannot check {} order risk: no trigger price set and no market data available for {}",
                                        order.order_type(),
                                        instrument.id()
                                    );
                                    Ok(None)
                                }
                            } else {
                                log::warn!(
                                    "Cannot check {} order risk: no trigger price set and no market data available for {}",
                                    order.order_type(),
                                    instrument.id()
                                );
                                Ok(None)
                            }
                        };
                        // Cache borrow dropped here

                        match calc_result {
                            Ok(Some(trigger)) => Some(trigger),
                            Ok(None) => {
                                continue;
                            }
                            Err(e) => {
                                self.deny_order(
                                    order,
                                    &OrderDeniedReason::TrailingStopCalcFailed { detail: e }
                                        .to_string(),
                                );
                                return false;
                            }
                        }
                    }
                }
                _ => order.price(),
            };

            let Some(last_px) = last_px else {
                log::error!("Cannot check order risk: no price available");
                continue;
            };

            // For quote quantity limit orders, use worst-case execution price
            let effective_price = if order.is_quote_quantity()
                && !instrument.is_inverse()
                && matches!(order, OrderAny::Limit(_) | OrderAny::StopLimit(_))
            {
                // Get current market price for worst-case execution
                let cache = self.cache.borrow();
                if let Some(quote_tick) = cache.quote(&instrument.id()) {
                    match order.order_side() {
                        // BUY: could execute at best ask if below limit (more quantity)
                        OrderSide::Buy => last_px.min(quote_tick.ask_price),
                        // SELL: could execute at best bid if above limit (but less quantity, so use limit)
                        OrderSide::Sell => last_px.max(quote_tick.bid_price),
                        OrderSide::NoOrderSide => last_px,
                    }
                } else {
                    last_px // No market data, use limit price
                }
            } else {
                last_px
            };

            let effective_quantity = if order.is_quote_quantity() && !instrument.is_inverse() {
                instrument.calculate_base_quantity(order.quantity(), effective_price)
            } else {
                order.quantity()
            };

            // Base-quantity bounds (`min_quantity`/`max_quantity`) do not apply to
            // quote-denominated orders: the client-side conversion uses an estimated
            // price and may differ from the venue fill, and some venues enforce
            // distinct per-order-type minimums. The venue is authoritative for
            // quote-denominated sizing; rely on `min_notional`/`max_notional` below.
            if !order.is_quote_quantity() {
                if let Some(max_quantity) = instrument.max_quantity()
                    && effective_quantity > max_quantity
                {
                    self.deny_order(
                        order,
                        &OrderDeniedReason::QuantityExceedsMaximum {
                            effective_quantity,
                            max_quantity,
                        }
                        .to_string(),
                    );
                    return false; // Denied
                }

                if let Some(min_quantity) = instrument.min_quantity()
                    && effective_quantity < min_quantity
                {
                    self.deny_order(
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

            let notional =
                instrument.calculate_notional_value(effective_quantity, last_px, Some(true));

            if self.config.debug {
                log::debug!("Notional: {notional:?}");
            }

            // Check MAX notional per order limit
            if let Some(max_notional_value) = max_notional
                && notional > max_notional_value
            {
                self.deny_order(
                    order,
                    &OrderDeniedReason::NotionalExceedsMaxPerOrder {
                        max_notional: max_notional_value,
                        notional,
                    }
                    .to_string(),
                );
                return false; // Denied
            }

            // Check MIN notional instrument limit
            if let Some(min_notional) = instrument.min_notional()
                && notional.currency == min_notional.currency
                && notional < min_notional
            {
                self.deny_order(
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
            if let Some(max_notional) = instrument.max_notional()
                && notional.currency == max_notional.currency
                && notional > max_notional
            {
                self.deny_order(
                    order,
                    &OrderDeniedReason::NotionalExceedsMaximum {
                        max_notional,
                        notional,
                    }
                    .to_string(),
                );
                return false; // Denied
            }

            if is_margin {
                // Margin account: check initial margin requirement
                let margin_req = match &mut account {
                    AccountAny::Margin(margin) => margin
                        .calculate_initial_margin(instrument, effective_quantity, last_px, None)
                        .unwrap_or_else(|e| {
                            log::error!("Failed to calculate initial margin: {e}");
                            Money::zero(instrument.quote_currency())
                        }),
                    _ => unreachable!(),
                };

                if self.config.debug {
                    log::debug!("Initial margin required: {margin_req}");
                }

                // Determine if order is position-reducing
                let is_reducing = order.is_reduce_only()
                    || (order.is_sell()
                        && (cum_sell_qty_raw + effective_quantity.raw) <= available_long_qty_raw)
                    || (order.is_buy()
                        && (cum_buy_qty_raw + effective_quantity.raw) <= available_short_qty_raw);

                if order.is_sell() {
                    cum_sell_qty_raw += effective_quantity.raw;
                } else if order.is_buy() {
                    cum_buy_qty_raw += effective_quantity.raw;
                }

                if is_reducing {
                    if self.config.debug {
                        log::debug!("Position-reducing order skips margin check");
                    }
                    continue;
                }

                // Look up free balance in the margin requirement's currency
                // (handles inverse instruments where collateral is base currency)
                let margin_free = match &account {
                    AccountAny::Margin(margin) => margin.balance_free(Some(margin_req.currency)),
                    _ => unreachable!(),
                };

                let Some(margin_free_val) = margin_free else {
                    if self.config.debug {
                        log::debug!(
                            "No balance for margin currency {}, skipping margin check",
                            margin_req.currency
                        );
                    }
                    continue;
                };

                // Per-order margin check
                if margin_req > margin_free_val {
                    self.deny_order(
                        order,
                        &OrderDeniedReason::MarginExceedsFreeBalance {
                            free: margin_free_val,
                            margin_required: margin_req,
                        }
                        .to_string(),
                    );
                    return false;
                }

                // Cumulative margin check
                match cum_margin_required.as_mut() {
                    Some(cum) => cum.raw += margin_req.raw,
                    None => cum_margin_required = Some(margin_req),
                }

                if self.config.debug {
                    log::debug!("Cumulative margin required: {cum_margin_required:?}");
                }

                if let Some(cum_margin) = cum_margin_required
                    && cum_margin > margin_free_val
                {
                    self.deny_order(
                        order,
                        &OrderDeniedReason::CumMarginExceedsFreeBalance {
                            free: margin_free_val,
                            cum_margin,
                        }
                        .to_string(),
                    );
                    return false;
                }
            } else {
                // Cash account: check full notional value
                let notional =
                    instrument.calculate_notional_value(effective_quantity, last_px, None);
                let order_balance_impact = if is_betting {
                    match &mut account {
                        AccountAny::Betting(betting) => Money::from_raw(
                            -betting
                                .calculate_balance_locked(
                                    instrument,
                                    order.order_side(),
                                    effective_quantity,
                                    last_px,
                                    None,
                                )
                                .unwrap_or_else(|e| {
                                    log::error!("Failed to calculate betting balance locked: {e}");
                                    Money::zero(instrument.quote_currency())
                                })
                                .raw,
                            instrument.quote_currency(),
                        ),
                        _ => unreachable!(),
                    }
                } else {
                    match order.order_side() {
                        OrderSide::Buy => Money::from_raw(-notional.raw, notional.currency),
                        OrderSide::Sell => Money::from_raw(notional.raw, notional.currency),
                        OrderSide::NoOrderSide => {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::InvalidOrderSide {
                                    order_side: order.order_side(),
                                }
                                .to_string(),
                            );
                            return false; // Denied
                        }
                    }
                };

                if self.config.debug {
                    log::debug!("Balance impact: {order_balance_impact}");
                }

                // Check if order reduces an existing position
                let is_position_reducing = if order.is_buy() {
                    let reducing = order.is_reduce_only()
                        || (cum_buy_qty_raw + effective_quantity.raw) <= available_short_qty_raw;
                    cum_buy_qty_raw += effective_quantity.raw;
                    reducing
                } else if order.is_sell() {
                    let reducing = order.is_reduce_only()
                        || (cum_sell_qty_raw + effective_quantity.raw) <= available_long_qty_raw;
                    cum_sell_qty_raw += effective_quantity.raw;
                    reducing
                } else {
                    false
                };

                if is_position_reducing {
                    if self.config.debug {
                        log::debug!("Position-reducing order skips balance check");
                    }
                    continue;
                }

                // Deny when order exceeds free balance (unless borrowing is enabled)
                if !allow_borrowing
                    && let Some(free_val) = free
                    && (free_val.as_decimal() + order_balance_impact.as_decimal()) < Decimal::ZERO
                {
                    self.deny_order(
                        order,
                        &OrderDeniedReason::NotionalExceedsFreeBalance {
                            free: free_val,
                            notional,
                        }
                        .to_string(),
                    );
                    return false;
                }

                if base_currency.is_none() {
                    base_currency = instrument.base_currency();
                }

                if order.is_buy() {
                    match cum_notional_buy.as_mut() {
                        Some(cum_notional_buy_val) => {
                            cum_notional_buy_val.raw += -order_balance_impact.raw;
                        }
                        None => {
                            cum_notional_buy = Some(Money::from_raw(
                                -order_balance_impact.raw,
                                order_balance_impact.currency,
                            ));
                        }
                    }

                    if self.config.debug {
                        log::debug!("Cumulative notional BUY: {cum_notional_buy:?}");
                    }

                    if !allow_borrowing
                        && let (Some(free), Some(cum_notional_buy)) = (free, cum_notional_buy)
                        && cum_notional_buy > free
                    {
                        self.deny_order(
                            order,
                            &OrderDeniedReason::CumNotionalExceedsFreeBalance {
                                free,
                                cum_notional: cum_notional_buy,
                            }
                            .to_string(),
                        );
                        return false; // Denied
                    }
                } else if order.is_sell() {
                    if is_betting {
                        match cum_notional_sell.as_mut() {
                            Some(cum_notional_sell_val) => {
                                cum_notional_sell_val.raw += -order_balance_impact.raw;
                            }
                            None => {
                                cum_notional_sell = Some(Money::from_raw(
                                    -order_balance_impact.raw,
                                    order_balance_impact.currency,
                                ));
                            }
                        }

                        if self.config.debug {
                            log::debug!("Cumulative betting SELL liability: {cum_notional_sell:?}");
                        }

                        if !allow_borrowing
                            && let (Some(free), Some(cum_notional_sell)) = (free, cum_notional_sell)
                            && cum_notional_sell > free
                        {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::CumNotionalExceedsFreeBalance {
                                    free,
                                    cum_notional: cum_notional_sell,
                                }
                                .to_string(),
                            );
                            return false;
                        }

                        continue;
                    }

                    let has_base_currency = match &account {
                        AccountAny::Margin(_) => false,
                        AccountAny::Cash(cash) => cash.base_currency.is_some(),
                        AccountAny::Betting(betting) => betting.base_currency.is_some(),
                    };

                    if has_base_currency {
                        match cum_notional_sell.as_mut() {
                            Some(cum_notional_sell_val) => {
                                cum_notional_sell_val.raw += order_balance_impact.raw;
                            }
                            None => {
                                cum_notional_sell = Some(Money::from_raw(
                                    order_balance_impact.raw,
                                    order_balance_impact.currency,
                                ));
                            }
                        }

                        if self.config.debug {
                            log::debug!("Cumulative notional SELL: {cum_notional_sell:?}");
                        }

                        if !allow_borrowing
                            && let (Some(free), Some(cum_notional_sell)) = (free, cum_notional_sell)
                            && cum_notional_sell > free
                        {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::CumNotionalExceedsFreeBalance {
                                    free,
                                    cum_notional: cum_notional_sell,
                                }
                                .to_string(),
                            );
                            return false; // Denied
                        }
                    } else if let Some(base_currency) = base_currency {
                        let cash_value_raw: MoneyRaw = match effective_quantity.raw.try_into() {
                            Ok(value) => value,
                            Err(e) => {
                                self.deny_order(
                                    order,
                                    &OrderDeniedReason::QuantityConversionFailed {
                                        detail: e.to_string(),
                                    }
                                    .to_string(),
                                );
                                return false; // Denied
                            }
                        };
                        let cash_value = Money::from_raw(cash_value_raw, base_currency);

                        // Use base-currency free balance for sell checks
                        let base_free = match &account {
                            AccountAny::Margin(_) => None,
                            AccountAny::Cash(cash) => cash.balance_free(Some(base_currency)),
                            AccountAny::Betting(betting) => {
                                betting.balance_free(Some(base_currency))
                            }
                        };

                        if self.config.debug
                            && let AccountAny::Cash(cash) = &account
                        {
                            log::debug!("Cash value: {cash_value:?}");
                            log::debug!("Total: {:?}", cash.balance_total(Some(base_currency)));
                            log::debug!("Locked: {:?}", cash.balance_locked(Some(base_currency)));
                            log::debug!("Free: {base_free:?}");
                        }

                        if self.config.debug
                            && let AccountAny::Betting(betting) = &account
                        {
                            log::debug!("Cash value: {cash_value:?}");
                            log::debug!("Total: {:?}", betting.balance_total(Some(base_currency)));
                            log::debug!(
                                "Locked: {:?}",
                                betting.balance_locked(Some(base_currency))
                            );
                            log::debug!("Free: {base_free:?}");
                        }

                        match cum_notional_sell {
                            Some(mut value) => {
                                value.raw += cash_value.raw;
                                cum_notional_sell = Some(value);
                            }
                            None => cum_notional_sell = Some(cash_value),
                        }

                        if self.config.debug {
                            log::debug!("Cumulative notional SELL: {cum_notional_sell:?}");
                        }

                        if !allow_borrowing
                            && let (Some(base_free), Some(cum_notional_sell)) =
                                (base_free, cum_notional_sell)
                            && cum_notional_sell.raw > base_free.raw
                        {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::CumNotionalExceedsFreeBalance {
                                    free: base_free,
                                    cum_notional: cum_notional_sell,
                                }
                                .to_string(),
                            );
                            return false; // Denied
                        }
                    }
                }
            }
        }

        // Finally
        true // Passed
    }

    fn check_price(instrument: &InstrumentAny, price: Option<Price>) -> Option<String> {
        let price_val = price?;

        if price_val.precision > instrument.price_precision() {
            return Some(format!(
                "price {} invalid (precision {} > {})",
                price_val,
                price_val.precision,
                instrument.price_precision()
            ));
        }

        if !instrument.allows_negative_price() && price_val.raw <= 0 {
            return Some(format!("price {price_val} invalid (<= 0)"));
        }

        None
    }

    fn check_quantity(
        instrument: &InstrumentAny,
        quantity: Option<Quantity>,
        is_quote_quantity: bool,
    ) -> Option<String> {
        let quantity_val = quantity?;

        // Check precision
        if quantity_val.precision > instrument.size_precision() {
            return Some(format!(
                "quantity {} invalid (precision {} > {})",
                quantity_val,
                quantity_val.precision,
                instrument.size_precision()
            ));
        }

        // Skip min/max checks for quote quantities (they will be checked in check_orders_risk using effective_quantity)
        if is_quote_quantity {
            return None;
        }

        // Check maximum quantity
        if let Some(max_quantity) = instrument.max_quantity()
            && quantity_val > max_quantity
        {
            return Some(format!(
                "quantity {quantity_val} invalid (> maximum trade size of {max_quantity})"
            ));
        }

        // Check minimum quantity
        if let Some(min_quantity) = instrument.min_quantity()
            && quantity_val < min_quantity
        {
            return Some(format!(
                "quantity {quantity_val} invalid (< minimum trade size of {min_quantity})"
            ));
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

    fn execution_gateway(&mut self, instrument: &InstrumentAny, command: TradingCommand) {
        match self.trading_state {
            TradingState::Halted => match command {
                TradingCommand::SubmitOrder(submit_order) => {
                    let order = {
                        let cache = self.cache.borrow();
                        cache
                            .order(&submit_order.client_order_id)
                            .map(|o| o.clone())
                    };

                    if let Some(ref order) = order {
                        self.deny_order(order, &OrderDeniedReason::TradingHalted.to_string());
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
            TradingState::Reducing => {
                match &command {
                    TradingCommand::SubmitOrder(submit_order) => {
                        let order = {
                            let cache = self.cache.borrow();
                            cache
                                .order(&submit_order.client_order_id)
                                .map(|o| o.clone())
                        };

                        if let Some(ref order) = order
                            && ((order.is_buy() && self.portfolio.is_net_long(&instrument.id()))
                                || (order.is_sell()
                                    && self.portfolio.is_net_short(&instrument.id())))
                        {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::TradingStateReducing {
                                    order_side: order.order_side(),
                                    instrument_id: instrument.id(),
                                }
                                .to_string(),
                            );
                            return;
                        }
                    }
                    TradingCommand::SubmitOrderList(submit_order_list) => {
                        let orders: Vec<OrderAny> = self.cache.borrow().orders_for_ids(
                            &submit_order_list.order_list.client_order_ids,
                            &submit_order_list,
                        );

                        for order in &orders {
                            let order_instrument_id = order.instrument_id();
                            if (order.is_buy() && self.portfolio.is_net_long(&order_instrument_id))
                                || (order.is_sell()
                                    && self.portfolio.is_net_short(&order_instrument_id))
                            {
                                self.deny_order_list(
                                    &orders,
                                    &OrderDeniedReason::TradingStateReducing {
                                        order_side: order.order_side(),
                                        instrument_id: order_instrument_id,
                                    }
                                    .to_string(),
                                );
                                return;
                            }
                        }
                    }
                    _ => {}
                }
                // Not denied: forward to throttler
                self.throttled_submit.send(command);
            }
            TradingState::Active => match command {
                TradingCommand::SubmitOrder(_) | TradingCommand::SubmitOrderList(_) => {
                    self.throttled_submit.send(command);
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
            log::debug!("{RECV}{EVT} {event:?}");
        }
    }

    fn handle_position_event(&self, event: &PositionEvent) {
        if self.config.debug {
            log::debug!("{RECV}{EVT} {event:?}");
        }
    }
}
