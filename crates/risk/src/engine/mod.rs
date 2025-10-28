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

//! Risk management engine implementation.

pub mod config;

#[cfg(test)]
mod tests;

use std::{cell::RefCell, collections::HashMap, fmt::Debug, rc::Rc};

use config::RiskEngineConfig;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    messages::execution::{ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand},
    msgbus,
    throttler::Throttler,
};
use nautilus_core::UUID4;
use nautilus_model::{
    accounts::{Account, AccountAny},
    enums::{InstrumentClass, OrderSide, OrderStatus, TimeInForce, TradingState},
    events::{OrderDenied, OrderEventAny, OrderModifyRejected},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny, OrderList},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_portfolio::Portfolio;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use ustr::Ustr;

type SubmitOrderFn = Box<dyn Fn(SubmitOrder)>;
type ModifyOrderFn = Box<dyn Fn(ModifyOrder)>;

/// Central risk management engine that validates and controls trading operations.
///
/// The `RiskEngine` provides comprehensive pre-trade risk checks including order validation,
/// balance verification, position sizing limits, and trading state management. It acts as
/// a gateway between strategy orders and execution, ensuring all trades comply with
/// defined risk parameters and regulatory constraints.
#[allow(dead_code)]
pub struct RiskEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    portfolio: Portfolio,
    pub throttled_submit_order: Throttler<SubmitOrder, SubmitOrderFn>,
    pub throttled_modify_order: Throttler<ModifyOrder, ModifyOrderFn>,
    max_notional_per_order: HashMap<InstrumentId, Decimal>,
    trading_state: TradingState,
    config: RiskEngineConfig,
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
        let throttled_submit_order =
            Self::create_submit_order_throttler(&config, clock.clone(), cache.clone());

        let throttled_modify_order =
            Self::create_modify_order_throttler(&config, clock.clone(), cache.clone());

        Self {
            clock,
            cache,
            portfolio,
            throttled_submit_order,
            throttled_modify_order,
            max_notional_per_order: HashMap::new(),
            trading_state: TradingState::Active,
            config,
        }
    }

    fn create_submit_order_throttler(
        config: &RiskEngineConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Throttler<SubmitOrder, SubmitOrderFn> {
        let success_handler = {
            Box::new(move |submit_order: SubmitOrder| {
                msgbus::send_any(
                    "ExecEngine.execute".into(),
                    &TradingCommand::SubmitOrder(submit_order),
                );
            }) as Box<dyn Fn(SubmitOrder)>
        };

        let failure_handler = {
            let cache = cache;
            let clock = clock.clone();
            Box::new(move |submit_order: SubmitOrder| {
                let reason = "REJECTED BY THROTTLER";
                log::warn!(
                    "SubmitOrder for {} DENIED: {}",
                    submit_order.client_order_id,
                    reason
                );

                Self::handle_submit_order_cache(&cache, &submit_order);

                let denied = Self::create_order_denied(&submit_order, reason, &clock);

                msgbus::send_any("ExecEngine.process".into(), &denied);
            }) as Box<dyn Fn(SubmitOrder)>
        };

        Throttler::new(
            config.max_order_submit.limit,
            config.max_order_submit.interval_ns,
            clock,
            "ORDER_SUBMIT_THROTTLER".to_string(),
            success_handler,
            Some(failure_handler),
            Ustr::from(&UUID4::new().to_string()),
        )
    }

    fn create_modify_order_throttler(
        config: &RiskEngineConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Throttler<ModifyOrder, ModifyOrderFn> {
        let success_handler = {
            Box::new(move |order: ModifyOrder| {
                msgbus::send_any(
                    "ExecEngine.execute".into(),
                    &TradingCommand::ModifyOrder(order),
                );
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

                let order = match Self::get_existing_order(&cache, &order) {
                    Some(order) => order,
                    None => return,
                };

                let rejected = Self::create_modify_rejected(&order, reason, &clock);

                msgbus::send_any("ExecEngine.process".into(), &rejected);
            }) as Box<dyn Fn(ModifyOrder)>
        };

        Throttler::new(
            config.max_order_modify.limit,
            config.max_order_modify.interval_ns,
            clock,
            "ORDER_MODIFY_THROTTLER".to_string(),
            success_handler,
            Some(failure_handler),
            Ustr::from(&UUID4::new().to_string()),
        )
    }

    fn handle_submit_order_cache(cache: &Rc<RefCell<Cache>>, submit_order: &SubmitOrder) {
        let mut cache = cache.borrow_mut();
        if !cache.order_exists(&submit_order.client_order_id) {
            cache
                .add_order(submit_order.order.clone(), None, None, false)
                .map_err(|e| {
                    log::error!("Cannot add order to cache: {e}");
                })
                .unwrap();
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

    // -- COMMANDS --------------------------------------------------------------------------------

    /// Executes a trading command through the risk management pipeline.
    pub fn execute(&mut self, command: TradingCommand) {
        // This will extend to other commands such as `RiskCommand`
        self.handle_command(command);
    }

    /// Processes an order event for risk monitoring and state updates.
    pub fn process(&mut self, event: OrderEventAny) {
        // This will extend to other events such as `RiskEvent`
        self.handle_event(event);
    }

    /// Sets the trading state for risk control enforcement.
    pub fn set_trading_state(&mut self, state: TradingState) {
        if state == self.trading_state {
            log::warn!("No change to trading state: already set to {state:?}");
            return;
        }

        self.trading_state = state;

        let _ts_now = self.clock.borrow().timestamp_ns();

        // TODO: Create a new Event "TradingStateChanged" in OrderEventAny enum.
        // let event = OrderEventAny::TradingStateChanged(TradingStateChanged::new(..,self.trading_state,..));

        msgbus::publish("events.risk".into(), &"message"); // TODO: Send the new Event here

        log::info!("Trading state set to {state:?}");
    }

    /// Sets the maximum notional value per order for the specified instrument.
    pub fn set_max_notional_per_order(&mut self, instrument_id: InstrumentId, new_value: Decimal) {
        self.max_notional_per_order.insert(instrument_id, new_value);

        let new_value_str = new_value.to_string();
        log::info!("Set MAX_NOTIONAL_PER_ORDER: {instrument_id} {new_value_str}");
    }

    // -- COMMAND HANDLERS ------------------------------------------------------------------------

    // Renamed from `execute_command`
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
            TradingCommand::QueryAccount(query_account) => {
                self.send_to_execution(TradingCommand::QueryAccount(query_account));
            }
            _ => {
                log::error!("Cannot handle command: {command}");
            }
        }
    }

    fn handle_submit_order(&mut self, command: SubmitOrder) {
        if self.config.bypass {
            self.send_to_execution(TradingCommand::SubmitOrder(command));
            return;
        }

        let order = &command.order;
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
                        &format!("Reduce only order would increase position {position_id}"),
                    );
                    return; // Denied
                }
            } else {
                self.deny_command(
                    TradingCommand::SubmitOrder(command),
                    &format!("Position {position_id} not found for reduce-only order"),
                );
                return;
            }
        }

        let instrument_exists = {
            let cache = self.cache.borrow();
            cache.instrument(&command.instrument_id).cloned()
        };

        let instrument = if let Some(instrument) = instrument_exists {
            instrument
        } else {
            self.deny_command(
                TradingCommand::SubmitOrder(command.clone()),
                &format!("Instrument for {} not found", command.instrument_id),
            );
            return; // Denied
        };

        ////////////////////////////////////////////////////////////////////////////////
        // PRE-TRADE ORDER(S) CHECKS
        ////////////////////////////////////////////////////////////////////////////////
        if !self.check_order(instrument.clone(), order.clone()) {
            return; // Denied
        }

        if !self.check_orders_risk(instrument.clone(), Vec::from([order.clone()])) {
            return; // Denied
        }

        // Route through execution gateway for TradingState checks & throttling
        self.execution_gateway(instrument, TradingCommand::SubmitOrder(command));
    }

    fn handle_submit_order_list(&mut self, command: SubmitOrderList) {
        if self.config.bypass {
            self.send_to_execution(TradingCommand::SubmitOrderList(command));
            return;
        }

        let instrument_exists = {
            let cache = self.cache.borrow();
            cache.instrument(&command.instrument_id).cloned()
        };

        let instrument = if let Some(instrument) = instrument_exists {
            instrument
        } else {
            self.deny_command(
                TradingCommand::SubmitOrderList(command.clone()),
                &format!("no instrument found for {}", command.instrument_id),
            );
            return; // Denied
        };

        ////////////////////////////////////////////////////////////////////////////////
        // PRE-TRADE ORDER(S) CHECKS
        ////////////////////////////////////////////////////////////////////////////////
        for order in command.order_list.orders.clone() {
            if !self.check_order(instrument.clone(), order) {
                return; // Denied
            }
        }

        if !self.check_orders_risk(instrument.clone(), command.order_list.clone().orders) {
            self.deny_order_list(
                command.order_list.clone(),
                &format!("OrderList {} DENIED", command.order_list.id),
            );
            return; // Denied
        }

        self.execution_gateway(instrument, TradingCommand::SubmitOrderList(command));
    }

    fn handle_modify_order(&mut self, command: ModifyOrder) {
        ////////////////////////////////////////////////////////////////////////////////
        // VALIDATE COMMAND
        ////////////////////////////////////////////////////////////////////////////////
        let order_exists = {
            let cache = self.cache.borrow();
            cache.order(&command.client_order_id).cloned()
        };

        let order = if let Some(order) = order_exists {
            order
        } else {
            log::error!(
                "ModifyOrder DENIED: Order with command.client_order_id: {} not found",
                command.client_order_id
            );
            return;
        };

        if order.is_closed() {
            self.reject_modify_order(
                order,
                &format!(
                    "Order with command.client_order_id: {} already closed",
                    command.client_order_id
                ),
            );
            return;
        } else if order.status() == OrderStatus::PendingCancel {
            self.reject_modify_order(
                order,
                &format!(
                    "Order with command.client_order_id: {} is already pending cancel",
                    command.client_order_id
                ),
            );
            return;
        }

        let maybe_instrument = {
            let cache = self.cache.borrow();
            cache.instrument(&command.instrument_id).cloned()
        };

        let instrument = if let Some(instrument) = maybe_instrument {
            instrument
        } else {
            self.reject_modify_order(
                order,
                &format!("no instrument found for {:?}", command.instrument_id),
            );
            return; // Denied
        };

        // Check Price
        let mut risk_msg = self.check_price(&instrument, command.price);
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(order, &risk_msg);
            return; // Denied
        }

        // Check Trigger
        risk_msg = self.check_price(&instrument, command.trigger_price);
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(order, &risk_msg);
            return; // Denied
        }

        // Check Quantity
        risk_msg = self.check_quantity(&instrument, command.quantity, order.is_quote_quantity());
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(order, &risk_msg);
            return; // Denied
        }

        // Check TradingState
        match self.trading_state {
            TradingState::Halted => {
                self.reject_modify_order(order, "TradingState is HALTED: Cannot modify order");
            }
            TradingState::Reducing => {
                if let Some(quantity) = command.quantity
                    && quantity > order.quantity()
                    && ((order.is_buy() && self.portfolio.is_net_long(&instrument.id()))
                        || (order.is_sell() && self.portfolio.is_net_short(&instrument.id())))
                {
                    self.reject_modify_order(
                        order,
                        &format!(
                            "TradingState is REDUCING and update will increase exposure {}",
                            instrument.id()
                        ),
                    );
                }
            }
            _ => {}
        }

        self.throttled_modify_order.send(command);
    }

    // -- PRE-TRADE CHECKS ------------------------------------------------------------------------

    fn check_order(&self, instrument: InstrumentAny, order: OrderAny) -> bool {
        ////////////////////////////////////////////////////////////////////////////////
        // VALIDATION CHECKS
        ////////////////////////////////////////////////////////////////////////////////
        if order.time_in_force() == TimeInForce::Gtd {
            // SAFETY: GTD guarantees an expire time
            let expire_time = order.expire_time().unwrap();
            if expire_time <= self.clock.borrow().timestamp_ns() {
                self.deny_order(
                    order,
                    &format!("GTD {} already past", expire_time.to_rfc3339()),
                );
                return false; // Denied
            }
        }

        if !self.check_order_price(instrument.clone(), order.clone())
            || !self.check_order_quantity(instrument, order)
        {
            return false; // Denied
        }

        true
    }

    fn check_order_price(&self, instrument: InstrumentAny, order: OrderAny) -> bool {
        ////////////////////////////////////////////////////////////////////////////////
        // CHECK PRICE
        ////////////////////////////////////////////////////////////////////////////////
        if order.price().is_some() {
            let risk_msg = self.check_price(&instrument, order.price());
            if let Some(risk_msg) = risk_msg {
                self.deny_order(order, &risk_msg);
                return false; // Denied
            }
        }

        ////////////////////////////////////////////////////////////////////////////////
        // CHECK TRIGGER
        ////////////////////////////////////////////////////////////////////////////////
        if order.trigger_price().is_some() {
            let risk_msg = self.check_price(&instrument, order.trigger_price());
            if let Some(risk_msg) = risk_msg {
                self.deny_order(order, &risk_msg);
                return false; // Denied
            }
        }

        true
    }

    fn check_order_quantity(&self, instrument: InstrumentAny, order: OrderAny) -> bool {
        let risk_msg = self.check_quantity(
            &instrument,
            Some(order.quantity()),
            order.is_quote_quantity(),
        );
        if let Some(risk_msg) = risk_msg {
            self.deny_order(order, &risk_msg);
            return false; // Denied
        }

        true
    }

    fn check_orders_risk(&self, instrument: InstrumentAny, orders: Vec<OrderAny>) -> bool {
        ////////////////////////////////////////////////////////////////////////////////
        // CHECK TRIGGER
        ////////////////////////////////////////////////////////////////////////////////
        let mut last_px: Option<Price> = None;
        let mut max_notional: Option<Money> = None;

        // Determine max notional
        let max_notional_setting = self.max_notional_per_order.get(&instrument.id());
        if let Some(max_notional_setting_val) = max_notional_setting.copied() {
            max_notional = Some(Money::new(
                max_notional_setting_val
                    .to_f64()
                    .expect("Invalid decimal conversion"),
                instrument.quote_currency(),
            ));
        }

        // Get account for risk checks
        let account_exists = {
            let cache = self.cache.borrow();
            cache.account_for_venue(&instrument.id().venue).cloned()
        };

        let account = if let Some(account) = account_exists {
            account
        } else {
            log::debug!("Cannot find account for venue {}", instrument.id().venue);
            return true; // TODO: Temporary early return until handling routing/multiple venues
        };
        let cash_account = match account {
            AccountAny::Cash(cash_account) => cash_account,
            AccountAny::Margin(_) => return true, // TODO: Determine risk controls for margin
        };
        let free = cash_account.balance_free(Some(instrument.quote_currency()));
        if self.config.debug {
            log::debug!("Free cash: {free:?}");
        }

        let mut cum_notional_buy: Option<Money> = None;
        let mut cum_notional_sell: Option<Money> = None;
        let mut base_currency: Option<Currency> = None;
        for order in &orders {
            // Determine last price based on order type
            last_px = match order {
                OrderAny::Market(_) | OrderAny::MarketToLimit(_) => {
                    if last_px.is_none() {
                        let cache = self.cache.borrow();
                        if let Some(last_quote) = cache.quote(&instrument.id()) {
                            match order.order_side() {
                                OrderSide::Buy => Some(last_quote.ask_price),
                                OrderSide::Sell => Some(last_quote.bid_price),
                                _ => panic!("Invalid order side"),
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
                        log::warn!(
                            "Cannot check {} order risk: no trigger price was set", // TODO: Use last_trade += offset
                            order.order_type()
                        );
                        continue;
                    }
                }
                _ => order.price(),
            };

            let last_px = if let Some(px) = last_px {
                px
            } else {
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
                        _ => last_px,
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

            // Check min/max quantity against effective quantity
            if let Some(max_quantity) = instrument.max_quantity()
                && effective_quantity > max_quantity
            {
                self.deny_order(
                    order.clone(),
                    &format!(
                        "QUANTITY_EXCEEDS_MAXIMUM: effective_quantity={effective_quantity}, max_quantity={max_quantity}"
                    ),
                );
                return false; // Denied
            }

            if let Some(min_quantity) = instrument.min_quantity()
                && effective_quantity < min_quantity
            {
                self.deny_order(
                    order.clone(),
                    &format!(
                        "QUANTITY_BELOW_MINIMUM: effective_quantity={effective_quantity}, min_quantity={min_quantity}"
                    ),
                );
                return false; // Denied
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
                        order.clone(),
                        &format!(
                            "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional={max_notional_value:?}, notional={notional:?}"
                        ),
                    );
                return false; // Denied
            }

            // Check MIN notional instrument limit
            if let Some(min_notional) = instrument.min_notional()
                && notional.currency == min_notional.currency
                && notional < min_notional
            {
                self.deny_order(
                        order.clone(),
                        &format!(
                            "NOTIONAL_LESS_THAN_MIN_FOR_INSTRUMENT: min_notional={min_notional:?}, notional={notional:?}"
                        ),
                    );
                return false; // Denied
            }

            // // Check MAX notional instrument limit
            if let Some(max_notional) = instrument.max_notional()
                && notional.currency == max_notional.currency
                && notional > max_notional
            {
                self.deny_order(
                        order.clone(),
                        &format!(
                            "NOTIONAL_GREATER_THAN_MAX_FOR_INSTRUMENT: max_notional={max_notional:?}, notional={notional:?}"
                        ),
                    );
                return false; // Denied
            }

            // Calculate OrderBalanceImpact (valid for CashAccount only)
            let notional = instrument.calculate_notional_value(effective_quantity, last_px, None);
            let order_balance_impact = match order.order_side() {
                OrderSide::Buy => Money::from_raw(-notional.raw, notional.currency),
                OrderSide::Sell => Money::from_raw(notional.raw, notional.currency),
                OrderSide::NoOrderSide => {
                    panic!("invalid `OrderSide`, was {}", order.order_side());
                }
            };

            if self.config.debug {
                log::debug!("Balance impact: {order_balance_impact}");
            }

            if let Some(free_val) = free
                && (free_val.as_decimal() + order_balance_impact.as_decimal()) < Decimal::ZERO
            {
                self.deny_order(
                    order.clone(),
                    &format!(
                        "NOTIONAL_EXCEEDS_FREE_BALANCE: free={free_val:?}, notional={notional:?}"
                    ),
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

                if let (Some(free), Some(cum_notional_buy)) = (free, cum_notional_buy)
                    && cum_notional_buy > free
                {
                    self.deny_order(order.clone(), &format!("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_buy}"));
                    return false; // Denied
                }
            } else if order.is_sell() {
                if cash_account.base_currency.is_some() {
                    if order.is_reduce_only() {
                        if self.config.debug {
                            log::debug!(
                                "Reduce-only SELL skips cumulative notional free-balance check"
                            );
                        }
                    } else {
                        match cum_notional_sell.as_mut() {
                            Some(cum_notional_buy_val) => {
                                cum_notional_buy_val.raw += order_balance_impact.raw;
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

                        if let (Some(free), Some(cum_notional_sell)) = (free, cum_notional_sell)
                            && cum_notional_sell > free
                        {
                            self.deny_order(order.clone(), &format!("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_sell}"));
                            return false; // Denied
                        }
                    }
                }
                // Account is already of type Cash, so no check
                else if let Some(base_currency) = base_currency {
                    if order.is_reduce_only() {
                        if self.config.debug {
                            log::debug!(
                                "Reduce-only SELL skips base-currency cumulative free check"
                            );
                        }
                        continue;
                    }

                    let cash_value = Money::from_raw(
                        effective_quantity
                            .raw
                            .try_into()
                            .map_err(|e| log::error!("Unable to convert Quantity to f64: {e}"))
                            .unwrap(),
                        base_currency,
                    );

                    if self.config.debug {
                        log::debug!("Cash value: {cash_value:?}");
                        log::debug!(
                            "Total: {:?}",
                            cash_account.balance_total(Some(base_currency))
                        );
                        log::debug!(
                            "Locked: {:?}",
                            cash_account.balance_locked(Some(base_currency))
                        );
                        log::debug!("Free: {:?}", cash_account.balance_free(Some(base_currency)));
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
                    if let (Some(free), Some(cum_notional_sell)) = (free, cum_notional_sell)
                        && cum_notional_sell.raw > free.raw
                    {
                        self.deny_order(order.clone(), &format!("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_sell}"));
                        return false; // Denied
                    }
                }
            }
        }

        // Finally
        true // Passed
    }

    fn check_price(&self, instrument: &InstrumentAny, price: Option<Price>) -> Option<String> {
        let price_val = price?;

        if price_val.precision > instrument.price_precision() {
            return Some(format!(
                "price {} invalid (precision {} > {})",
                price_val,
                price_val.precision,
                instrument.price_precision()
            ));
        }

        if instrument.instrument_class() != InstrumentClass::Option && price_val.raw <= 0 {
            return Some(format!("price {price_val} invalid (<= 0)"));
        }

        None
    }

    fn check_quantity(
        &self,
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

    // -- DENIALS ---------------------------------------------------------------------------------

    fn deny_command(&self, command: TradingCommand, reason: &str) {
        match command {
            TradingCommand::SubmitOrder(command) => {
                self.deny_order(command.order, reason);
            }
            TradingCommand::SubmitOrderList(command) => {
                self.deny_order_list(command.order_list, reason);
            }
            _ => {
                panic!("Cannot deny command {command}");
            }
        }
    }

    fn deny_order(&self, order: OrderAny, reason: &str) {
        log::warn!(
            "SubmitOrder for {} DENIED: {}",
            order.client_order_id(),
            reason
        );

        if order.status() != OrderStatus::Initialized {
            return;
        }

        let mut cache = self.cache.borrow_mut();
        if !cache.order_exists(&order.client_order_id()) {
            cache
                .add_order(order.clone(), None, None, false)
                .map_err(|e| {
                    log::error!("Cannot add order to cache: {e}");
                })
                .unwrap();
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

        msgbus::send_any("ExecEngine.process".into(), &denied);
    }

    fn deny_order_list(&self, order_list: OrderList, reason: &str) {
        for order in order_list.orders {
            if !order.is_closed() {
                self.deny_order(order, reason);
            }
        }
    }

    fn reject_modify_order(&self, order: OrderAny, reason: &str) {
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

        msgbus::send_any("ExecEngine.process".into(), &denied);
    }

    // -- EGRESS ----------------------------------------------------------------------------------

    fn execution_gateway(&mut self, instrument: InstrumentAny, command: TradingCommand) {
        match self.trading_state {
            TradingState::Halted => match command {
                TradingCommand::SubmitOrder(submit_order) => {
                    self.deny_order(submit_order.order, "TradingState::HALTED");
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    self.deny_order_list(submit_order_list.order_list, "TradingState::HALTED");
                }
                _ => {}
            },
            TradingState::Reducing => match command {
                TradingCommand::SubmitOrder(submit_order) => {
                    let order = submit_order.order;
                    if order.is_buy() && self.portfolio.is_net_long(&instrument.id()) {
                        self.deny_order(
                            order,
                            &format!(
                                "BUY when TradingState::REDUCING and LONG {}",
                                instrument.id()
                            ),
                        );
                    } else if order.is_sell() && self.portfolio.is_net_short(&instrument.id()) {
                        self.deny_order(
                            order,
                            &format!(
                                "SELL when TradingState::REDUCING and SHORT {}",
                                instrument.id()
                            ),
                        );
                    }
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    let order_list = submit_order_list.order_list;
                    for order in &order_list.orders {
                        if order.is_buy() && self.portfolio.is_net_long(&instrument.id()) {
                            self.deny_order_list(
                                order_list,
                                &format!(
                                    "BUY when TradingState::REDUCING and LONG {}",
                                    instrument.id()
                                ),
                            );
                            return;
                        } else if order.is_sell() && self.portfolio.is_net_short(&instrument.id()) {
                            self.deny_order_list(
                                order_list,
                                &format!(
                                    "SELL when TradingState::REDUCING and SHORT {}",
                                    instrument.id()
                                ),
                            );
                            return;
                        }
                    }
                }
                _ => {}
            },
            TradingState::Active => match command {
                TradingCommand::SubmitOrder(submit_order) => {
                    self.throttled_submit_order.send(submit_order);
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    // TODO: implement throttler for order lists
                    self.send_to_execution(TradingCommand::SubmitOrderList(submit_order_list));
                }
                _ => {}
            },
        }
    }

    fn send_to_execution(&self, command: TradingCommand) {
        msgbus::send_any("ExecEngine.execute".into(), &command);
    }

    fn handle_event(&mut self, event: OrderEventAny) {
        // We intend to extend the risk engine to be able to handle additional events.
        // For now we just log.
        if self.config.debug {
            log::debug!("{RECV}{EVT} {event:?}");
        }
    }
}
