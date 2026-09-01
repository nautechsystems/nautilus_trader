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
    throttler::{RateLimit, Throttler},
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
        PositionEvent,
    },
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    types::{Currency, Money, Price, Quantity, money::MoneyRaw, quantity::QuantityRaw},
};
use nautilus_portfolio::Portfolio;
use rust_decimal::Decimal;
use ustr::Ustr;

// Returns cash and wallet accounts for sell-balance checks; margin and betting accounts
// follow their own sell paths.
fn cash_or_wallet_account(account: &AccountAny) -> Option<&dyn Account> {
    match account {
        AccountAny::Cash(cash) => Some(cash),
        AccountAny::Wallet(wallet) => Some(wallet),
        AccountAny::Margin(_) | AccountAny::Betting(_) => None,
    }
}

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

        let full_position_exit = self.is_full_position_exit(&command, &instrument, &order);

        if !self.check_order(&instrument, &order, full_position_exit) {
            return; // Denied
        }

        if !self.check_orders_risk(&instrument, &[order], full_position_exit) {
            return; // Denied
        }

        // Route through execution gateway for TradingState checks & throttling
        self.execution_gateway(&instrument, TradingCommand::SubmitOrder(command));
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

        self.full_position_exit_reduces(command, order)
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
            && !order.is_reduce_only()
            && order.quantity().is_positive()
    }

    fn full_position_exit_reduces(&self, command: &SubmitOrder, order: &OrderAny) -> bool {
        let Some(position_id) = command.position_id else {
            return false;
        };
        let position = {
            let cache = self.cache.borrow();
            if cache.position_id(&order.client_order_id()).copied() != Some(position_id) {
                return false;
            }
            cache.position(&position_id).map(|position| {
                (
                    position.is_open(),
                    position.instrument_id,
                    position.side,
                    position.quantity,
                )
            })
        };
        let Some((is_open, position_instrument_id, position_side, position_quantity)) = position
        else {
            return false;
        };

        is_open
            && position_instrument_id == order.instrument_id()
            && matches!(
                (order.order_side(), position_side),
                (OrderSide::Buy, PositionSide::Short) | (OrderSide::Sell, PositionSide::Long)
            )
            && order.would_reduce_only(position_side, position_quantity)
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

        if !self.check_orders_risk(&representative, &orders, false) {
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
    ) -> bool {
        let mut orders_by_account: AHashMap<Option<AccountId>, Vec<&OrderAny>> = AHashMap::new();
        for order in orders {
            orders_by_account
                .entry(order.account_id())
                .or_default()
                .push(order);
        }

        for (account_id, account_orders) in &orders_by_account {
            if !self.check_orders_risk_for_account(
                instrument,
                account_orders,
                *account_id,
                full_position_exit,
            ) {
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
        full_position_exit: bool,
    ) -> bool {
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

        // Get account for risk checks: use explicit account_id if provided, otherwise venue lookup
        let resolved_account = {
            let cache = self.cache.borrow();

            if let Some(account_id) = account_id {
                cache
                    .account(&account_id)
                    .map(|account| account.clone_without_events())
            } else {
                cache
                    .account_for_venue(&instrument.id().venue)
                    .map(|account| account.clone_without_events())
            }
        };

        let Some(mut account) = resolved_account else {
            log::debug!(
                "Cannot find account for venue {} (account_id={account_id:?})",
                instrument.id().venue
            );

            for (&order, price) in orders.iter().zip(&market_prices) {
                if matches!(order, OrderAny::Market(_) | OrderAny::MarketToLimit(_))
                    && price.is_none()
                {
                    self.deny_no_market_price(instrument.id(), order);
                    return false;
                }
            }

            return true;
        };

        let is_margin = matches!(account, AccountAny::Margin(_));
        let is_betting = matches!(account, AccountAny::Betting(_));
        let is_wallet = matches!(account, AccountAny::Wallet(_));
        let free = match &account {
            AccountAny::Margin(margin) => margin.balance_free(Some(instrument.quote_currency())),
            AccountAny::Cash(cash) => cash.balance_free(Some(instrument.quote_currency())),
            AccountAny::Betting(betting) => betting.balance_free(Some(instrument.quote_currency())),
            AccountAny::Wallet(wallet) => Some(
                wallet
                    .balance_free(Some(instrument.quote_currency()))
                    .unwrap_or_else(|| Money::zero(instrument.quote_currency())),
            ),
        };
        let allow_borrowing = match &account {
            AccountAny::Cash(cash) => cash.allow_borrowing,
            AccountAny::Margin(_) | AccountAny::Betting(_) | AccountAny::Wallet(_) => false,
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

        for (&order, market_price) in orders.iter().zip(market_prices) {
            // Determine last price based on order type
            let last_px = match order {
                OrderAny::Market(_) | OrderAny::MarketToLimit(_) => {
                    let Some(price) = market_price else {
                        let is_reducing = !is_wallet
                            && (order.is_reduce_only()
                                || (order.is_sell()
                                    && (cum_sell_qty_raw + order.quantity().raw)
                                        <= available_long_qty_raw));

                        if !order.is_quote_quantity()
                            && order.is_sell()
                            && !is_reducing
                            && let Some(unleveraged) = cash_or_wallet_account(&account)
                            && unleveraged.base_currency().is_none()
                            && let Some(base_currency) = instrument.base_currency()
                            && !self.check_cash_sell_balance(
                                unleveraged,
                                allow_borrowing,
                                order,
                                order.quantity(),
                                base_currency,
                                &mut cum_notional_sell,
                            )
                        {
                            return false;
                        }

                        self.deny_no_market_price(instrument.id(), order);
                        return false;
                    };

                    Some(price)
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
                                        order.order_side(),
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
                                    order.order_side(),
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
                                        order.order_side(),
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
                                    &OrderDeniedReason::TrailingStopCalculationFailed { detail: e }
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
            if !order.is_quote_quantity() && !full_position_exit {
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

            let notional = match instrument.try_calculate_notional_value(
                effective_quantity,
                last_px,
                Some(true),
            ) {
                Ok(notional) => notional,
                Err(e) => {
                    self.deny_order(
                        order,
                        &OrderDeniedReason::NotionalCalculationFailed {
                            detail: e.to_string(),
                        }
                        .to_string(),
                    );
                    return false;
                }
            };

            if self.config.debug {
                log::debug!("Notional: {notional:?}");
            }

            // Check MAX notional per order limit
            if !full_position_exit
                && let Some(max_notional_value) = max_notional
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

            // Whole-position and reduce-only orders may close residual positions below the
            // venue minimum
            if !order.is_reduce_only()
                && !full_position_exit
                && let Some(min_notional) = instrument.min_notional()
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
            if !full_position_exit
                && let Some(max_notional) = instrument.max_notional()
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
                    AccountAny::Margin(margin) => match margin.calculate_initial_margin(
                        instrument,
                        effective_quantity,
                        last_px,
                        None,
                    ) {
                        Ok(margin) => margin,
                        Err(e) => {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::InitialMarginCalculationFailed {
                                    detail: e.to_string(),
                                }
                                .to_string(),
                            );
                            return false;
                        }
                    },
                    _ => unreachable!(),
                };

                if self.config.debug {
                    log::debug!("Initial margin required: {margin_req}");
                }

                // Determine if order is position-reducing
                let is_reducing = order.is_reduce_only()
                    || full_position_exit
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
                        &OrderDeniedReason::InitialMarginExceedsFreeBalance {
                            free_balance: margin_free_val,
                            initial_margin: margin_req,
                        }
                        .to_string(),
                    );
                    return false;
                }

                // Cumulative margin check
                match cum_margin_required.as_mut() {
                    Some(cum) => {
                        let Some(total) = cum.checked_add(margin_req) else {
                            self.deny_order(
                                order,
                                &OrderDeniedReason::CumulativeInitialMarginCalculationFailed {
                                    detail: "total exceeds Money bounds".to_string(),
                                }
                                .to_string(),
                            );
                            return false;
                        };
                        *cum = total;
                    }
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
                        &OrderDeniedReason::CumulativeInitialMarginExceedsFreeBalance {
                            free_balance: margin_free_val,
                            cumulative_initial_margin: cum_margin,
                        }
                        .to_string(),
                    );
                    return false;
                }
            } else {
                // Cash account: check full notional value
                let notional = match instrument.try_calculate_notional_value(
                    effective_quantity,
                    last_px,
                    None,
                ) {
                    Ok(notional) => notional,
                    Err(e) => {
                        self.deny_order(
                            order,
                            &OrderDeniedReason::NotionalCalculationFailed {
                                detail: e.to_string(),
                            }
                            .to_string(),
                        );
                        return false;
                    }
                };
                let order_balance_impact = if is_betting {
                    match &mut account {
                        AccountAny::Betting(betting) => {
                            match betting.calculate_balance_locked(
                                instrument,
                                order.order_side(),
                                effective_quantity,
                                last_px,
                                None,
                            ) {
                                Ok(locked) => {
                                    Money::from_raw(-locked.raw, instrument.quote_currency())
                                }
                                Err(e) => {
                                    self.deny_order(
                                        order,
                                        &OrderDeniedReason::BettingBalanceLockedCalculationFailed {
                                            detail: e.to_string(),
                                        }
                                        .to_string(),
                                    );
                                    return false;
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                } else {
                    match order.order_side() {
                        OrderSide::Buy => Money::from_raw(-notional.raw, notional.currency),
                        OrderSide::Sell => Money::from_raw(notional.raw, notional.currency),
                    }
                };

                if self.config.debug {
                    log::debug!("Balance impact: {order_balance_impact}");
                }

                // Check if order reduces an existing position
                let is_position_reducing = if order.is_buy() {
                    let reducing = full_position_exit
                        || (cum_buy_qty_raw + effective_quantity.raw) <= available_short_qty_raw;
                    cum_buy_qty_raw += effective_quantity.raw;
                    reducing
                } else if order.is_sell() {
                    let reducing = order.is_reduce_only()
                        || full_position_exit
                        || (cum_sell_qty_raw + effective_quantity.raw) <= available_long_qty_raw;
                    cum_sell_qty_raw += effective_quantity.raw;
                    reducing
                } else {
                    false
                };

                if is_position_reducing && !is_wallet {
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
                            free_balance: free_val,
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
                            &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                                free_balance: free,
                                cumulative_notional: cum_notional_buy,
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
                                &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                                    free_balance: free,
                                    cumulative_notional: cum_notional_sell,
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
                        AccountAny::Wallet(wallet) => wallet.base_currency.is_some(),
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
                                &OrderDeniedReason::CumulativeNotionalExceedsFreeBalance {
                                    free_balance: free,
                                    cumulative_notional: cum_notional_sell,
                                }
                                .to_string(),
                            );
                            return false; // Denied
                        }
                    } else if let Some(base_currency) = base_currency {
                        let Some(unleveraged) = cash_or_wallet_account(&account) else {
                            unreachable!()
                        };

                        if !self.check_cash_sell_balance(
                            unleveraged,
                            allow_borrowing,
                            order,
                            effective_quantity,
                            base_currency,
                            &mut cum_notional_sell,
                        ) {
                            return false;
                        }
                    }
                }
            }
        }

        // Finally
        true // Passed
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

    fn check_cash_sell_balance(
        &self,
        account: &dyn Account,
        allow_borrowing: bool,
        order: &OrderAny,
        quantity: Quantity,
        base_currency: Currency,
        cum_notional_sell: &mut Option<Money>,
    ) -> bool {
        let cash_value_raw: MoneyRaw = match quantity.raw.try_into() {
            Ok(value) => value,
            Err(e) => {
                self.deny_order(
                    order,
                    &OrderDeniedReason::QuantityConversionFailed {
                        detail: e.to_string(),
                    }
                    .to_string(),
                );
                return false;
            }
        };

        let cash_value = Money::from_raw(cash_value_raw, base_currency);
        let base_free = account
            .balance_free(Some(base_currency))
            .unwrap_or_else(|| Money::zero(base_currency));

        if self.config.debug {
            log::debug!("Cash value: {cash_value:?}");
            log::debug!("Total: {:?}", account.balance_total(Some(base_currency)));
            log::debug!("Locked: {:?}", account.balance_locked(Some(base_currency)));
            log::debug!("Free: {base_free:?}");
        }

        match cum_notional_sell {
            Some(value) => value.raw += cash_value.raw,
            None => *cum_notional_sell = Some(cash_value),
        }

        if self.config.debug {
            log::debug!("Cumulative notional SELL: {cum_notional_sell:?}");
        }

        if !allow_borrowing
            && let Some(cum_notional_sell) = *cum_notional_sell
            && cum_notional_sell.raw > base_free.raw
        {
            self.deny_order(
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

    fn deny_no_market_price(&self, instrument_id: InstrumentId, order: &OrderAny) {
        self.deny_order(
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

        if !instrument.allows_negative_price() && price_val.raw <= 0 {
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
