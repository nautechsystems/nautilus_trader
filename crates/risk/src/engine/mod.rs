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

//! Provides a generic `ExecutionEngine` for all environments.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use config::RiskEngineConfig;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    msgbus::{self},
    throttler::Throttler,
};
use nautilus_core::UUID4;
use nautilus_execution::messages::{ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand};
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

pub mod config;

type SubmitOrderFn = Box<dyn Fn(SubmitOrder)>;
type ModifyOrderFn = Box<dyn Fn(ModifyOrder)>;

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

impl RiskEngine {
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
                msgbus::send(
                    &Ustr::from("ExecEngine.execute"),
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

                msgbus::send(&Ustr::from("ExecEngine.process"), &denied);
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
                msgbus::send(
                    &Ustr::from("ExecEngine.execute"),
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

                msgbus::send(&Ustr::from("ExecEngine.process"), &rejected);
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

    pub fn execute(&mut self, command: TradingCommand) {
        // This will extend to other commands such as `RiskCommand`
        self.handle_command(command);
    }

    pub fn process(&mut self, event: OrderEventAny) {
        // This will extend to other events such as `RiskEvent`
        self.handle_event(event);
    }

    pub fn set_trading_state(&mut self, state: TradingState) {
        if state == self.trading_state {
            log::warn!("No change to trading state: already set to {state:?}");
            return;
        }

        self.trading_state = state;

        let _ts_now = self.clock.borrow().timestamp_ns();

        // TODO: Create a new Event "TradingStateChanged" in OrderEventAny enum.
        // let event = OrderEventAny::TradingStateChanged(TradingStateChanged::new(..,self.trading_state,..));

        msgbus::publish(&Ustr::from("events.risk"), &"message"); // TODO: Send the new Event here

        log::info!("Trading state set to {state:?}");
    }

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
            _ => {
                log::error!("Cannot handle command: {command}");
            }
        }
    }

    fn handle_submit_order(&self, command: SubmitOrder) {
        if self.config.bypass {
            self.send_to_execution(TradingCommand::SubmitOrder(command));
            return;
        }

        let order = &command.order;
        if let Some(position_id) = command.position_id {
            if order.is_reduce_only() {
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
        }

        let instrument_exists = {
            let cache = self.cache.borrow();
            cache.instrument(&order.instrument_id()).cloned()
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

        self.execution_gateway(instrument, TradingCommand::SubmitOrder(command.clone()));
    }

    fn handle_submit_order_list(&self, command: SubmitOrderList) {
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

    fn handle_modify_order(&self, command: ModifyOrder) {
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

        // Get instrument for orders
        let maybe_instrument = {
            let cache = self.cache.borrow();
            cache.instrument(&command.instrument_id).cloned()
        };

        let instrument = if let Some(instrument) = maybe_instrument {
            instrument
        } else {
            self.reject_modify_order(
                order,
                &format!("no instrument found for {}", command.instrument_id),
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
        risk_msg = self.check_quantity(&instrument, command.quantity);
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
                if let Some(quantity) = command.quantity {
                    if quantity > order.quantity()
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
            }
            _ => {}
        }

        // TODO: Fix message bus usage
        // self.throttled_modify_order.send(command);
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
        let risk_msg = self.check_quantity(&instrument, Some(order.quantity()));
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

            let notional =
                instrument.calculate_notional_value(order.quantity(), last_px, Some(true));

            if self.config.debug {
                log::debug!("Notional: {notional:?}");
            }

            // Check MAX notional per order limit
            if let Some(max_notional_value) = max_notional {
                if notional > max_notional_value {
                    self.deny_order(
                        order.clone(),
                        &format!(
                            "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional={max_notional_value:?}, notional={notional:?}"
                        ),
                    );
                    return false; // Denied
                }
            }

            // Check MIN notional instrument limit
            if let Some(min_notional) = instrument.min_notional() {
                if notional.currency == min_notional.currency && notional < min_notional {
                    self.deny_order(
                        order.clone(),
                        &format!(
                            "NOTIONAL_LESS_THAN_MIN_FOR_INSTRUMENT: min_notional={min_notional:?}, notional={notional:?}"
                        ),
                    );
                    return false; // Denied
                }
            }

            // // Check MAX notional instrument limit
            if let Some(max_notional) = instrument.max_notional() {
                if notional.currency == max_notional.currency && notional > max_notional {
                    self.deny_order(
                        order.clone(),
                        &format!(
                            "NOTIONAL_GREATER_THAN_MAX_FOR_INSTRUMENT: max_notional={max_notional:?}, notional={notional:?}"
                        ),
                    );
                    return false; // Denied
                }
            }

            // Calculate OrderBalanceImpact (valid for CashAccount only)
            let notional = instrument.calculate_notional_value(order.quantity(), last_px, None);
            let order_balance_impact = match order.order_side() {
                OrderSide::Buy => Money::from_raw(-notional.raw, notional.currency),
                OrderSide::Sell => Money::from_raw(notional.raw, notional.currency),
                OrderSide::NoOrderSide => {
                    panic!("invalid `OrderSide`, was {}", order.order_side());
                }
            };

            if self.config.debug {
                log::debug!("Balance impact: {}", order_balance_impact);
            }

            if let Some(free_val) = free {
                if (free_val.as_decimal() + order_balance_impact.as_decimal()) < Decimal::ZERO {
                    self.deny_order(
                        order.clone(),
                        &format!(
                            "NOTIONAL_EXCEEDS_FREE_BALANCE: free={free_val:?}, notional={notional:?}"
                        ),
                    );
                    return false;
                }
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

                if let (Some(free), Some(cum_notional_buy)) = (free, cum_notional_buy) {
                    if cum_notional_buy > free {
                        self.deny_order(order.clone(), &format!("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_buy}"));
                        return false; // Denied
                    }
                }
            } else if order.is_sell() {
                if cash_account.base_currency.is_some() {
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

                    if let (Some(free), Some(cum_notional_sell)) = (free, cum_notional_sell) {
                        if cum_notional_sell > free {
                            self.deny_order(order.clone(), &format!("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_sell}"));
                            return false; // Denied
                        }
                    }
                }
                // Account is already of type Cash, so no check
                else if let Some(base_currency) = base_currency {
                    let cash_value = Money::from_raw(
                        order
                            .quantity()
                            .raw
                            .try_into()
                            .map_err(|e| log::error!("Unable to convert Quantity to f64: {}", e))
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
                        Some(mut cum_notional_sell) => {
                            cum_notional_sell.raw += cash_value.raw;
                        }
                        None => cum_notional_sell = Some(cash_value),
                    }

                    if self.config.debug {
                        log::debug!("Cumulative notional SELL: {cum_notional_sell:?}");
                    }
                    if let (Some(free), Some(cum_notional_sell)) = (free, cum_notional_sell) {
                        if cum_notional_sell.raw > free.raw {
                            self.deny_order(order.clone(), &format!("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={free}, cum_notional={cum_notional_sell}"));
                            return false; // Denied
                        }
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

        // Check maximum quantity
        if let Some(max_quantity) = instrument.max_quantity() {
            if quantity_val > max_quantity {
                return Some(format!(
                    "quantity {quantity_val} invalid (> maximum trade size of {max_quantity})"
                ));
            }
        }

        // // Check minimum quantity
        if let Some(min_quantity) = instrument.min_quantity() {
            if quantity_val < min_quantity {
                return Some(format!(
                    "quantity {quantity_val} invalid (< minimum trade size of {min_quantity})"
                ));
            }
        }

        None
    }

    // -- DENIALS ---------------------------------------------------------------------------------

    fn deny_command(&self, command: TradingCommand, reason: &str) {
        match command {
            TradingCommand::SubmitOrder(submit_order) => {
                self.deny_order(submit_order.order, reason);
            }
            TradingCommand::SubmitOrderList(submit_order_list) => {
                self.deny_order_list(submit_order_list.order_list, reason);
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

        msgbus::send(&Ustr::from("ExecEngine.process"), &denied);
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

        msgbus::send(&Ustr::from("ExecEngine.process"), &denied);
    }

    // -- EGRESS ----------------------------------------------------------------------------------

    fn execution_gateway(&self, instrument: InstrumentAny, command: TradingCommand) {
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
                        return;
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
                TradingCommand::SubmitOrder(_submit_order) => {
                    // TODO: Fix message bus usage
                    // self.throttled_submit_order.send(submit_order);
                }
                TradingCommand::SubmitOrderList(_submit_order_list) => {
                    todo!("NOT IMPLEMENTED");
                }
                _ => {}
            },
        }
    }

    fn send_to_execution(&self, command: TradingCommand) {
        msgbus::send(&Ustr::from("ExecEngine.execute"), &command);
    }

    fn handle_event(&mut self, event: OrderEventAny) {
        // We intend to extend the risk engine to be able to handle additional events.
        // For now we just log.
        if self.config.debug {
            log::debug!("{}{} {event:?}", RECV, EVT);
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, rc::Rc, str::FromStr};

    use nautilus_common::{
        cache::Cache,
        clock::{Clock, TestClock},
        msgbus::{
            self,
            handler::ShareableMessageHandler,
            stubs::{get_message_saving_handler, get_saved_messages},
            switchboard::MessagingSwitchboard,
        },
        throttler::RateLimit,
    };
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_execution::{
        engine::{ExecutionEngine, config::ExecutionEngineConfig},
        messages::{ModifyOrder, SubmitOrder, SubmitOrderList, TradingCommand},
    };
    use nautilus_model::{
        accounts::{
            AccountAny,
            stubs::{cash_account, margin_account},
        },
        data::{QuoteTick, stubs::quote_audusd},
        enums::{AccountType, LiquiditySide, OrderSide, OrderType, TimeInForce, TradingState},
        events::{
            AccountState, OrderAccepted, OrderDenied, OrderEventAny, OrderEventType, OrderFilled,
            OrderSubmitted, account::stubs::cash_account_state_million_usd,
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId,
            Symbol, TradeId, TraderId, VenueOrderId,
            stubs::{
                account_id, client_id_binance, client_order_id, strategy_id_ema_cross, trader_id,
                uuid4, venue_order_id,
            },
        },
        instruments::{
            CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny,
            stubs::{audusd_sim, crypto_perpetual_ethusdt, xbtusd_bitmex},
        },
        orders::{Order, OrderAny, OrderList, OrderTestBuilder},
        types::{AccountBalance, Currency, Money, Price, Quantity, fixed::FIXED_PRECISION},
    };
    use nautilus_portfolio::Portfolio;
    use rstest::{fixture, rstest};
    use rust_decimal::{Decimal, prelude::FromPrimitive};
    use ustr::Ustr;

    use super::{RiskEngine, config::RiskEngineConfig};

    #[fixture]
    fn process_order_event_handler() -> ShareableMessageHandler {
        get_message_saving_handler::<OrderEventAny>(Some(Ustr::from("ExecEngine.process")))
    }

    #[fixture]
    fn execute_order_event_handler() -> ShareableMessageHandler {
        get_message_saving_handler::<TradingCommand>(Some(Ustr::from("ExecEngine.execute")))
    }

    #[fixture]
    fn simple_cache() -> Cache {
        Cache::new(None, None)
    }

    #[fixture]
    fn clock() -> TestClock {
        TestClock::new()
    }

    #[fixture]
    fn max_order_submit() -> RateLimit {
        RateLimit::new(10, 1)
    }

    #[fixture]
    fn max_order_modify() -> RateLimit {
        RateLimit::new(5, 1)
    }

    #[fixture]
    fn max_notional_per_order() -> HashMap<InstrumentId, Decimal> {
        HashMap::new()
    }

    // Market buy order with corresponding fill
    #[fixture]
    fn market_order_buy(instrument_eth_usdt: InstrumentAny) -> OrderAny {
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .build()
    }

    // Market sell order
    #[fixture]
    fn market_order_sell(instrument_eth_usdt: InstrumentAny) -> OrderAny {
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1"))
            .build()
    }

    #[fixture]
    fn get_stub_submit_order(
        trader_id: TraderId,
        client_id_binance: ClientId,
        strategy_id_ema_cross: StrategyId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_eth_usdt: InstrumentAny,
    ) -> SubmitOrder {
        SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_eth_usdt.id(),
            client_order_id,
            venue_order_id,
            market_order_buy(instrument_eth_usdt),
            None,
            None,
            UUID4::new(),
            UnixNanos::from(10),
        )
        .unwrap()
    }

    #[fixture]
    fn config_fixture(
        max_order_submit: RateLimit,
        max_order_modify: RateLimit,
        max_notional_per_order: HashMap<InstrumentId, Decimal>,
    ) -> RiskEngineConfig {
        RiskEngineConfig {
            debug: true,
            bypass: false,
            max_order_submit,
            max_order_modify,
            max_notional_per_order,
        }
    }

    #[fixture]
    pub fn bitmex_cash_account_state_multi() -> AccountState {
        let btc_account_balance = AccountBalance::new(
            Money::from("10 BTC"),
            Money::from("0 BTC"),
            Money::from("10 BTC"),
        );
        let eth_account_balance = AccountBalance::new(
            Money::from("20 ETH"),
            Money::from("0 ETH"),
            Money::from("20 ETH"),
        );
        AccountState::new(
            AccountId::from("BITMEX-001"),
            AccountType::Cash,
            vec![btc_account_balance, eth_account_balance],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None, // multi cash account
        )
    }

    fn get_process_order_event_handler_messages(
        event_handler: ShareableMessageHandler,
    ) -> Vec<OrderEventAny> {
        get_saved_messages::<OrderEventAny>(event_handler)
    }

    fn get_execute_order_event_handler_messages(
        event_handler: ShareableMessageHandler,
    ) -> Vec<TradingCommand> {
        get_saved_messages::<TradingCommand>(event_handler)
    }

    #[fixture]
    fn instrument_eth_usdt(crypto_perpetual_ethusdt: CryptoPerpetual) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt)
    }

    #[fixture]
    fn instrument_xbtusd_bitmex(xbtusd_bitmex: CryptoPerpetual) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(xbtusd_bitmex)
    }

    #[fixture]
    fn instrument_audusd(audusd_sim: CurrencyPair) -> InstrumentAny {
        InstrumentAny::CurrencyPair(audusd_sim)
    }

    #[fixture]
    pub fn instrument_xbtusd_with_high_size_precision() -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            InstrumentId::from("BTCUSDT.BITMEX"),
            Symbol::from("XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::BTC(),
            true,
            1,
            2,
            Price::from("0.5"),
            Quantity::from("0.01"),
            None,
            None,
            None,
            None,
            Some(Money::from("10000000 USD")),
            Some(Money::from("1 USD")),
            Some(Price::from("10000000")),
            Some(Price::from("0.01")),
            Some(Decimal::from_str("0.01").unwrap()),
            Some(Decimal::from_str("0.0035").unwrap()),
            Some(Decimal::from_str("-0.00025").unwrap()),
            Some(Decimal::from_str("0.00075").unwrap()),
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    // Helpers
    fn get_risk_engine(
        cache: Option<Rc<RefCell<Cache>>>,
        config: Option<RiskEngineConfig>,
        clock: Option<Rc<RefCell<TestClock>>>,
        bypass: bool,
    ) -> RiskEngine {
        let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
        let config = config.unwrap_or(RiskEngineConfig {
            debug: true,
            bypass,
            max_order_submit: RateLimit::new(10, 1000),
            max_order_modify: RateLimit::new(5, 1000),
            max_notional_per_order: HashMap::new(),
        });
        let clock = clock.unwrap_or(Rc::new(RefCell::new(TestClock::new())));
        let portfolio = Portfolio::new(cache.clone(), clock.clone(), None);
        RiskEngine::new(config, portfolio, clock, cache)
    }

    fn get_exec_engine(
        cache: Option<Rc<RefCell<Cache>>>,
        clock: Option<Rc<RefCell<TestClock>>>,
        config: Option<ExecutionEngineConfig>,
    ) -> ExecutionEngine {
        let cache = cache.unwrap_or(Rc::new(RefCell::new(Cache::default())));
        let clock = clock.unwrap_or(Rc::new(RefCell::new(TestClock::new())));
        ExecutionEngine::new(clock, cache, config)
    }

    fn order_submitted(order: &OrderAny) -> OrderSubmitted {
        OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.account_id().unwrap_or(account_id()),
            UUID4::new(),
            0.into(),
            0.into(),
        )
    }

    fn order_accepted(order: &OrderAny, venue_order_id: Option<VenueOrderId>) -> OrderAccepted {
        OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id.unwrap_or_default(),
            order.account_id().unwrap_or_default(),
            UUID4::new(),
            0.into(),
            0.into(),
            false,
        )
    }

    fn order_filled(
        order: &OrderAny,
        instrument: &InstrumentAny,
        strategy_id: Option<StrategyId>,
        account_id: Option<AccountId>,
        venue_order_id: Option<VenueOrderId>,
        trade_id: Option<TradeId>,
        last_qty: Option<Quantity>,
        last_px: Option<Price>,
        liquidity_side: Option<LiquiditySide>,
        account: Option<AccountAny>,
        ts_filled_ns: Option<UnixNanos>,
    ) -> OrderFilled {
        let strategy_id = strategy_id.unwrap_or(order.strategy_id());
        let account_id = account_id.unwrap_or(order.account_id().unwrap_or_default());
        let venue_order_id = venue_order_id.unwrap_or(order.venue_order_id().unwrap_or_default());
        let trade_id =
            trade_id.unwrap_or(order.client_order_id().as_str().replace('O', "E").into());
        let last_qty = last_qty.unwrap_or(order.quantity());
        let last_px = last_px.unwrap_or(order.price().unwrap_or_default());
        let liquidity_side = liquidity_side.unwrap_or(LiquiditySide::Taker);
        let ts_filled_ns = ts_filled_ns.unwrap_or(0.into());
        let account = account.unwrap_or(AccountAny::Cash(cash_account(
            cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"),
        )));

        let commission = account
            .calculate_commission(
                instrument.clone(),
                order.quantity(),
                last_px,
                liquidity_side,
                None,
            )
            .unwrap();

        OrderFilled::new(
            trader_id(),
            strategy_id,
            instrument.id(),
            order.client_order_id(),
            venue_order_id,
            account_id,
            trade_id,
            order.order_side(),
            order.order_type(),
            last_qty,
            last_px,
            instrument.quote_currency(),
            liquidity_side,
            UUID4::new(),
            ts_filled_ns,
            0.into(),
            false,
            None,
            Some(commission),
        )
    }

    // Tests
    #[rstest]
    fn test_bypass_config_risk_engine() {
        let risk_engine = get_risk_engine(
            None, None, None, true, // <-- Bypassing pre-trade risk checks for backtest
        );

        assert!(risk_engine.config.bypass);
    }

    #[rstest]
    fn test_trading_state_after_instantiation_returns_active() {
        let risk_engine = get_risk_engine(None, None, None, false);

        assert_eq!(risk_engine.trading_state, TradingState::Active);
    }

    #[rstest]
    fn test_set_trading_state_when_no_change_logs_warning() {
        let mut risk_engine = get_risk_engine(None, None, None, false);

        risk_engine.set_trading_state(TradingState::Active);

        assert_eq!(risk_engine.trading_state, TradingState::Active);
    }

    #[rstest]
    fn test_set_trading_state_changes_value_and_publishes_event() {
        let mut risk_engine = get_risk_engine(None, None, None, false);

        risk_engine.set_trading_state(TradingState::Halted);

        assert_eq!(risk_engine.trading_state, TradingState::Halted);
    }

    #[rstest]
    fn test_max_order_submit_rate_when_no_risk_config_returns_10_per_second() {
        let risk_engine = get_risk_engine(None, None, None, false);

        assert_eq!(risk_engine.config.max_order_submit.limit, 10);
        assert_eq!(risk_engine.config.max_order_submit.interval_ns, 1000);
    }

    #[rstest]
    fn test_max_order_modify_rate_when_no_risk_config_returns_5_per_second() {
        let risk_engine = get_risk_engine(None, None, None, false);

        assert_eq!(risk_engine.config.max_order_modify.limit, 5);
        assert_eq!(risk_engine.config.max_order_modify.interval_ns, 1000);
    }

    #[rstest]
    fn test_max_notionals_per_order_when_no_risk_config_returns_empty_hashmap() {
        let risk_engine = get_risk_engine(None, None, None, false);

        assert_eq!(risk_engine.max_notional_per_order, HashMap::new());
    }

    #[rstest]
    fn test_set_max_notional_per_order_changes_setting(instrument_audusd: InstrumentAny) {
        let mut risk_engine = get_risk_engine(None, None, None, false);

        risk_engine
            .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());

        let mut expected = HashMap::new();
        expected.insert(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());
        assert_eq!(risk_engine.max_notional_per_order, expected);
    }

    #[rstest]
    fn test_given_random_command_then_logs_and_continues(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let mut risk_engine = get_risk_engine(None, None, None, false);

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(100, 0))
            .quantity(Quantity::from("1000"))
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        let random_command = TradingCommand::SubmitOrder(submit_order);

        risk_engine.execute(random_command);
    }

    #[rstest]
    fn test_given_random_event_then_logs_and_continues(instrument_audusd: InstrumentAny) {
        let mut risk_engine = get_risk_engine(None, None, None, false);

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(100, 0))
            .quantity(Quantity::from("1000"))
            .build();

        let random_event = OrderEventAny::Denied(OrderDenied::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            Ustr::from("DENIED"),
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
            risk_engine.clock.borrow().timestamp_ns(),
        ));

        risk_engine.process(random_event);
    }

    // SUBMIT ORDER TESTS
    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_submit_order_with_default_settings_then_sends_to_client(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(100, 0))
            .quantity(Quantity::from("1000"))
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);
        assert_eq!(
            saved_execute_messages.first().unwrap().instrument_id(),
            instrument_audusd.id()
        );
    }

    #[rstest]
    fn test_submit_order_when_risk_bypassed_sends_to_execution_engine(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );
        let mut risk_engine = get_risk_engine(None, None, None, true);

        // TODO: Limit -> Market
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(100, 0))
            .quantity(Quantity::from("1000"))
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);
        assert_eq!(
            saved_execute_messages.first().unwrap().instrument_id(),
            instrument_audusd.id()
        );
    }

    #[rstest]
    fn test_submit_reduce_only_order_when_position_already_closed_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        clock: TestClock,
        simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );
        let clock = Rc::new(RefCell::new(clock));
        let simple_cache = Rc::new(RefCell::new(simple_cache));

        let mut risk_engine =
            get_risk_engine(Some(simple_cache.clone()), None, Some(clock.clone()), true);
        let mut exec_engine = get_exec_engine(Some(simple_cache), Some(clock), None);

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000"))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1000"))
            .reduce_only(true)
            .build();

        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1000"))
            .reduce_only(true)
            .build();

        let submit_order1 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order1.clone(),
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        let submitted = OrderEventAny::Submitted(order_submitted(&order1));
        let accepted = OrderEventAny::Accepted(order_accepted(&order1, None));
        let filled = OrderEventAny::Filled(order_filled(
            &order1,
            &instrument_audusd,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
        exec_engine.process(&submitted);
        exec_engine.process(&accepted);
        exec_engine.process(&filled);

        let submit_order2 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order2.clone(),
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
        exec_engine.process(&OrderEventAny::Submitted(order_submitted(&order2)));
        exec_engine.process(&OrderEventAny::Filled(order_filled(
            &order2,
            &instrument_audusd,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )));

        let submit_order3 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order3,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        // Act
        risk_engine.execute(TradingCommand::SubmitOrder(submit_order3));

        // Assert: TODO
        // assert_eq!(order1.status(), OrderStatus::Filled);
        // assert_eq!(order2.status(), OrderStatus::Filled);
        // assert_eq!(order3.status(), OrderStatus::Denied);

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 3);
        assert_eq!(
            saved_execute_messages.first().unwrap().instrument_id(),
            instrument_audusd.id()
        );
    }

    #[rstest]
    fn test_submit_reduce_only_order_when_position_would_be_increased_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        clock: TestClock,
        simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );
        let clock = Rc::new(RefCell::new(clock));
        let simple_cache = Rc::new(RefCell::new(simple_cache));

        let mut risk_engine =
            get_risk_engine(Some(simple_cache.clone()), None, Some(clock.clone()), true);
        let mut exec_engine = get_exec_engine(Some(simple_cache), Some(clock), None);

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000"))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("2000"))
            .reduce_only(true)
            .build();

        let submit_order1 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order1.clone(),
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        let submitted = OrderEventAny::Submitted(order_submitted(&order1));
        let accepted = OrderEventAny::Accepted(order_accepted(&order1, None));
        let filled = OrderEventAny::Filled(order_filled(
            &order1,
            &instrument_audusd,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
        exec_engine.process(&submitted);
        exec_engine.process(&accepted);
        exec_engine.process(&filled);

        let submit_order2 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order2.clone(),
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        // Act
        risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
        exec_engine.process(&OrderEventAny::Submitted(order_submitted(&order2)));
        exec_engine.process(&OrderEventAny::Accepted(order_accepted(&order2, None)));
        exec_engine.process(&OrderEventAny::Filled(order_filled(
            &order2,
            &instrument_audusd,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )));

        // Assert: TODO
        // assert_eq!(order1.status(), OrderStatus::Filled);
        // assert_eq!(order2.status(), OrderStatus::Denied);

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 2);
        assert_eq!(
            saved_execute_messages.first().unwrap().instrument_id(),
            instrument_audusd.id()
        );
    }

    #[rstest]
    fn test_submit_order_reduce_only_order_with_custom_position_id_not_open_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(100, 0))
            .quantity(Quantity::from("1000"))
            .reduce_only(true)
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            Some(PositionId::new("CUSTOM-001")), // <-- Custom position ID
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from("Position CUSTOM-001 not found for reduce-only order")
        );
    }

    #[rstest]
    fn test_submit_order_when_instrument_not_in_cache_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(100, 0))
            .quantity(Quantity::from("1000"))
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from("Instrument for AUD/USD.SIM not found")
        );
    }

    #[rstest]
    fn test_submit_order_when_invalid_price_precision_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(1_000_000_000_000, FIXED_PRECISION)) // <- Invalid price
            .quantity(Quantity::from("1000"))
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert!(
            saved_process_messages
                .first()
                .unwrap()
                .message()
                .unwrap()
                .contains(&format!("invalid (precision {FIXED_PRECISION} > 5)"))
        );
    }

    #[rstest]
    fn test_submit_order_when_invalid_negative_price_and_not_option_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(-1, 1)) // <- Invalid price
            .quantity(Quantity::from("1000"))
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from("price -0.0 invalid (<= 0)") // TODO: fix
        );
    }

    #[rstest]
    fn test_submit_order_when_invalid_trigger_price_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("1000").unwrap())
            .price(Price::from_raw(1, 1))
            .trigger_price(Price::from_raw(1_000_000_000_000_000, FIXED_PRECISION)) // <- Invalid price
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        // assert!(saved_process_messages
        //     .first()
        //     .unwrap()
        //     .message()
        //     .unwrap()
        //     .contains(&format!("invalid (precision {PRECISION})")));
    }

    #[rstest]
    fn test_submit_order_when_invalid_quantity_precision_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("0.1").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from("quantity 0.1 invalid (precision 1 > 0)")
        );
    }

    #[rstest]
    fn test_submit_order_when_invalid_quantity_exceeds_maximum_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100000000").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from("quantity 100000000 invalid (> maximum trade size of 1000000)")
        );
    }

    #[rstest]
    fn test_submit_order_when_invalid_quantity_less_than_minimum_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("1").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from("quantity 1 invalid (< minimum trade size of 100)")
        );
    }

    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_submit_order_when_market_order_and_no_market_then_logs_warning(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        execute_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        risk_engine.set_max_notional_per_order(
            instrument_audusd.id(),
            Decimal::from_i32(10000000).unwrap(),
        );

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);
        assert_eq!(
            saved_execute_messages.first().unwrap().instrument_id(),
            instrument_audusd.id()
        );
    }

    #[rstest]
    fn test_submit_order_when_less_than_min_notional_for_instrument_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_with_high_size_precision: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler,
        );

        simple_cache
            .add_instrument(instrument_xbtusd_with_high_size_precision.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_xbtusd_with_high_size_precision.id(),
            Price::from("0.075000"),
            Price::from("0.075005"),
            Quantity::from("50000"),
            Quantity::from("50000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_with_high_size_precision.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("0.9").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_with_high_size_precision.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from(
                "NOTIONAL_LESS_THAN_MIN_FOR_INSTRUMENT: min_notional=Money(1.00, USD), notional=Money(0.90, USD)"
            )
        );
    }

    #[rstest]
    fn test_submit_order_when_greater_than_max_notional_for_instrument_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_xbtusd_bitmex.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_xbtusd_bitmex.id(),
            Price::from("7.5000"),
            Price::from("7.5005"),
            Quantity::from("50000"),
            Quantity::from("50000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        risk_engine.set_max_notional_per_order(
            instrument_xbtusd_bitmex.id(),
            Decimal::from_i64(100000000).unwrap(),
        );

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("10000001").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from(
                "NOTIONAL_GREATER_THAN_MAX_FOR_INSTRUMENT: max_notional=Money(10000000.00, USD), notional=Money(10000001.00, USD)"
            )
        );
    }

    #[rstest]
    fn test_submit_order_when_buy_market_order_and_over_max_notional_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_audusd.id(),
            Price::from("0.75000"),
            Price::from("0.75005"),
            Quantity::from("500000"),
            Quantity::from("500000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        risk_engine
            .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("1000000").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from(
                "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional=Money(100000.00, USD), notional=Money(750050.00, USD)"
            )
        );
    }

    #[rstest]
    fn test_submit_order_when_sell_market_order_and_over_max_notional_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_audusd.id(),
            Price::from("0.75000"),
            Price::from("0.75005"),
            Quantity::from("500000"),
            Quantity::from("500000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        risk_engine
            .set_max_notional_per_order(instrument_audusd.id(), Decimal::from_i64(100000).unwrap());

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("1000000").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from(
                "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional=Money(100000.00, USD), notional=Money(750000.00, USD)"
            )
        );
    }

    #[rstest]
    fn test_submit_order_when_market_order_and_over_free_balance_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100000").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);

        assert_eq!(
            saved_process_messages.first().unwrap().event_type(),
            OrderEventType::Denied
        );
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from(
                "NOTIONAL_EXCEEDS_FREE_BALANCE: free=Money(1000000.00, USD), notional=Money(10100000.00, USD)"
            )
        );
    }

    #[rstest]
    fn test_submit_order_list_buys_when_over_free_balance_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("4920").unwrap())
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("5653").unwrap()) // <--- over free balance
            .build();

        let order_list = OrderList::new(
            OrderListId::new("1"),
            instrument_audusd.id(),
            StrategyId::new("S-001"),
            vec![order1, order2],
            risk_engine.clock.borrow().timestamp_ns(),
        );

        let submit_order = SubmitOrderList::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order_list,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrderList(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);

        assert_eq!(saved_process_messages.len(), 3);

        for event in &saved_process_messages {
            assert_eq!(event.event_type(), OrderEventType::Denied);
        }

        // The actual reason is in the first denial; the rest will show `OrderListID` as Denied.
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from(
                "CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, cum_notional=1067873.00 USD"
            )
        );
    }

    #[rstest]
    fn test_submit_order_list_sells_when_over_free_balance_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("4920").unwrap())
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("5653").unwrap()) // <--- over free balance
            .build();

        let order_list = OrderList::new(
            OrderListId::new("1"),
            instrument_audusd.id(),
            StrategyId::new("S-001"),
            vec![order1, order2],
            risk_engine.clock.borrow().timestamp_ns(),
        );

        let submit_order = SubmitOrderList::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order_list,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrderList(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);

        assert_eq!(saved_process_messages.len(), 3);

        for event in &saved_process_messages {
            assert_eq!(event.event_type(), OrderEventType::Denied);
        }

        // Correct reason is in First deny, rest will show OrderList`ID` Denied.
        assert_eq!(
            saved_process_messages.first().unwrap().message().unwrap(),
            Ustr::from(
                "CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free=1000000.00 USD, cum_notional=1057300.00 USD"
            )
        );
    }

    // TODO: After ExecutionClient
    #[rstest]
    fn test_submit_order_list_sells_when_multi_currency_cash_account_over_cumulative_notional() {}

    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_submit_order_when_reducing_and_buy_order_adds_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_xbtusd_bitmex.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_xbtusd_bitmex.id(),
            Price::from("0.075000"),
            Price::from("0.075005"),
            Quantity::from("50000"),
            Quantity::from("50000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

        risk_engine.set_max_notional_per_order(
            instrument_xbtusd_bitmex.id(),
            Decimal::from_str("10000").unwrap(),
        );

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order1 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            order1,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
        risk_engine.set_trading_state(TradingState::Reducing);

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order2 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            order2,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);

        // TODO: currently, portfolio.is_net_long() is false, because portfolio.net_position() is not updated
        // assert!(risk_engine.portfolio.is_net_long(&instrument_xbtusd_bitmex.id()));
        // let saved_process_messages =
        //     get_process_order_event_handler_messages(process_order_event_handler);
        // assert_eq!(saved_process_messages.len(), 1);

        // assert_eq!(
        //     saved_process_messages.first().unwrap().event_type(),
        //     OrderEventType::Denied
        // );
        // assert_eq!(
        //     saved_process_messages.first().unwrap().message().unwrap(),
        //     "BUY when TradingState.REDUCING and LONG"
        // );
    }

    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_submit_order_when_reducing_and_sell_order_adds_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_xbtusd_bitmex.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_xbtusd_bitmex.id(),
            Price::from("0.075000"),
            Price::from("0.075005"),
            Quantity::from("50000"),
            Quantity::from("50000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

        risk_engine.set_max_notional_per_order(
            instrument_xbtusd_bitmex.id(),
            Decimal::from_str("10000").unwrap(),
        );

        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order1 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            order1,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
        risk_engine.set_trading_state(TradingState::Reducing);

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order2 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            order2,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);

        // TODO: currently, portfolio.is_net_short() is false, because portfolio.net_position() is not updated
        // assert!(risk_engine.portfolio.is_net_short(&instrument_xbtusd_bitmex.id()));
        // let saved_process_messages =
        //     get_process_order_event_handler_messages(process_order_event_handler);
        // assert_eq!(saved_process_messages.len(), 1);

        // assert_eq!(
        //     saved_process_messages.first().unwrap().event_type(),
        //     OrderEventType::Denied
        // );
        // assert_eq!(
        //     saved_process_messages.first().unwrap().message().unwrap(),
        //     "SELL when TradingState.REDUCING and SHORT"
        // );
    }

    #[rstest]
    fn test_submit_order_when_trading_halted_then_denies_order(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_eth_usdt.clone())
            .unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            order.instrument_id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.set_trading_state(TradingState::Halted);

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        // Get messages and test
        let saved_messages = get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_messages.len(), 1);
        let first_message = saved_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Denied);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("TradingState::HALTED")
        );
    }

    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_submit_order_beyond_rate_limit_then_denies_order(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        for _i in 0..11 {
            let order = OrderTestBuilder::new(OrderType::Market)
                .instrument_id(instrument_audusd.id())
                .side(OrderSide::Buy)
                .quantity(Quantity::from_str("100").unwrap())
                .build();

            let submit_order = SubmitOrder::new(
                trader_id,
                client_id_binance,
                strategy_id_ema_cross,
                order.instrument_id(),
                client_order_id,
                venue_order_id,
                order.clone(),
                None,
                None,
                UUID4::new(),
                risk_engine.clock.borrow().timestamp_ns(),
            )
            .unwrap();

            risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        }

        assert_eq!(risk_engine.throttled_submit_order.used(), 1.0);

        // Get messages and test
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 1);
        let first_message = saved_process_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::Denied);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("REJECTED BY THROTTLER")
        );
    }

    #[rstest]
    fn test_submit_order_list_when_trading_halted_then_denies_orders(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let entry = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .trigger_price(Price::from_raw(1, 1))
            .build();

        let take_profit = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .price(Price::from_raw(11, 2))
            .build();

        let bracket = OrderList::new(
            OrderListId::new("1"),
            instrument_audusd.id(),
            StrategyId::new("S-001"),
            vec![entry, stop_loss, take_profit],
            risk_engine.clock.borrow().timestamp_ns(),
        );

        let submit_bracket = SubmitOrderList::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            bracket.instrument_id,
            client_order_id,
            venue_order_id,
            bracket,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.set_trading_state(TradingState::Halted);
        risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

        // Get messages and test
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 3);

        for event in &saved_process_messages {
            assert_eq!(event.event_type(), OrderEventType::Denied);
            assert_eq!(event.message().unwrap(), Ustr::from("TradingState::HALTED"));
        }
    }

    #[ignore = "Revisit after high-precision merged"]
    #[rstest]
    fn test_submit_order_list_buys_when_trading_reducing_then_denies_orders(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_xbtusd_bitmex.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_xbtusd_bitmex.id(),
            Price::from("0.075000"),
            Price::from("0.075005"),
            Quantity::from("50000"),
            Quantity::from("50000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

        risk_engine.set_max_notional_per_order(
            instrument_xbtusd_bitmex.id(),
            Decimal::from_str("10000").unwrap(),
        );

        let long = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            long,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        risk_engine.set_trading_state(TradingState::Reducing);

        let entry = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .trigger_price(Price::from_raw(11, 1))
            .build();

        // TODO: attempt to add with overflow
        // let take_profit = OrderTestBuilder::new(OrderType::Limit)
        //     .instrument_id(instrument_xbtusd_bitmex.id())
        //     .side(OrderSide::Buy)
        //     .quantity(Quantity::from_str("100").unwrap())
        //     .price(Price::from_raw(12, 1))
        //     .build();

        let bracket = OrderList::new(
            OrderListId::new("1"),
            instrument_xbtusd_bitmex.id(),
            StrategyId::new("S-001"),
            vec![entry, stop_loss],
            risk_engine.clock.borrow().timestamp_ns(),
        );

        let submit_order_list = SubmitOrderList::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            bracket,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);
    }

    #[ignore = "Revisit after high-precision merged"]
    #[rstest]
    fn test_submit_order_list_sells_when_trading_reducing_then_denies_orders(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_xbtusd_bitmex.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        let quote = QuoteTick::new(
            instrument_xbtusd_bitmex.id(),
            Price::from("0.075000"),
            Price::from("0.075005"),
            Quantity::from("50000"),
            Quantity::from("50000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);

        risk_engine.set_max_notional_per_order(
            instrument_xbtusd_bitmex.id(),
            Decimal::from_str("10000").unwrap(),
        );

        let short = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            short,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        risk_engine.set_trading_state(TradingState::Reducing);

        let entry = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("100").unwrap())
            .trigger_price(Price::from_raw(11, 1))
            .build();

        let take_profit = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from_str("100").unwrap())
            .price(Price::from_raw(12, 1))
            .build();

        let bracket = OrderList::new(
            OrderListId::new("1"),
            instrument_xbtusd_bitmex.id(),
            StrategyId::new("S-001"),
            vec![entry, stop_loss, take_profit],
            risk_engine.clock.borrow().timestamp_ns(),
        );

        let submit_order_list = SubmitOrderList::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            bracket,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrderList(submit_order_list));

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);
    }

    // SUBMIT BRACKET ORDER TESTS
    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_submit_bracket_with_default_settings_sends_to_client(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let entry = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .trigger_price(Price::from_raw(1, 1))
            .build();

        let take_profit = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .price(Price::from_raw(1001, 4))
            .build();

        let bracket = OrderList::new(
            OrderListId::new("1"),
            instrument_audusd.id(),
            StrategyId::new("S-001"),
            vec![entry, stop_loss, take_profit],
            risk_engine.clock.borrow().timestamp_ns(),
        );

        let _submit_bracket = SubmitOrderList::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            bracket.instrument_id,
            client_order_id,
            venue_order_id,
            bracket,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        // risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

        // Get messages and test
        // TODO: complete fn execution_gateway
        // let saved_process_messages =
        //     get_process_order_event_handler_messages(process_order_event_handler);
        // assert_eq!(saved_process_messages.len(), 0);
    }

    #[rstest]
    fn test_submit_bracket_with_emulated_orders_sends_to_emulator() {}

    #[rstest]
    fn test_submit_bracket_order_when_instrument_not_in_cache_then_denies(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let entry = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .build();

        let stop_loss = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .trigger_price(Price::from_raw(1, 1))
            .build();

        let take_profit = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .price(Price::from_raw(1001, 4))
            .build();

        let bracket = OrderList::new(
            OrderListId::new("1"),
            instrument_audusd.id(),
            StrategyId::new("S-001"),
            vec![entry, stop_loss, take_profit],
            risk_engine.clock.borrow().timestamp_ns(),
        );

        let submit_bracket = SubmitOrderList::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            bracket.instrument_id,
            client_order_id,
            venue_order_id,
            bracket,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrderList(submit_bracket));

        // Get messages and test
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 3);

        for event in &saved_process_messages {
            assert_eq!(event.event_type(), OrderEventType::Denied);
            assert_eq!(
                event.message().unwrap(),
                Ustr::from("no instrument found for AUD/USD.SIM")
            );
        }
    }

    #[rstest]
    fn test_submit_order_for_emulation_sends_command_to_emulator() {}

    // MODIFY ORDER TESTS
    #[rstest]
    fn test_modify_order_when_no_order_found_logs_error(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let modify_order = ModifyOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            None,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 0);
    }

    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_modify_order_beyond_rate_limit_then_rejects(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .trigger_price(Price::from_raw(10001, 4))
            .build();

        simple_cache
            .add_order(order, None, Some(client_id_binance), true)
            .unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        for i in 0..11 {
            let modify_order = ModifyOrder::new(
                trader_id,
                client_id_binance,
                strategy_id_ema_cross,
                instrument_audusd.id(),
                client_order_id,
                venue_order_id,
                Some(Quantity::from_str("100").unwrap()),
                Some(Price::from_raw(100011 + i, 5)),
                None,
                UUID4::new(),
                risk_engine.clock.borrow().timestamp_ns(),
            )
            .unwrap();

            risk_engine.execute(TradingCommand::ModifyOrder(modify_order));
        }

        assert_eq!(risk_engine.throttled_modify_order.used(), 1.0);

        // Get messages and test
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 6);
        let first_message = saved_process_messages.first().unwrap();
        assert_eq!(first_message.event_type(), OrderEventType::ModifyRejected);
        assert_eq!(
            first_message.message().unwrap(),
            Ustr::from("Exceeded MAX_ORDER_MODIFY_RATE")
        );
    }

    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_modify_order_with_default_settings_then_sends_to_client(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler,
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100").unwrap())
            .trigger_price(Price::from_raw(10001, 4))
            .build();

        simple_cache
            .add_order(order.clone(), None, Some(client_id_binance), true)
            .unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        let modify_order = ModifyOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            Some(Quantity::from_str("100").unwrap()),
            Some(Price::from_raw(100011, 5)),
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        risk_engine.execute(TradingCommand::ModifyOrder(modify_order));

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 2);
        assert_eq!(
            saved_execute_messages.first().unwrap().instrument_id(),
            instrument_audusd.id()
        );
    }

    #[rstest]
    fn test_modify_order_for_emulated_order_then_sends_to_emulator() {}

    #[rstest]
    fn test_submit_order_when_market_order_and_over_free_balance_then_denies_with_betting_account(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_audusd: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        cash_account_state_million_usd: AccountState,
        quote_audusd: QuoteTick,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );

        simple_cache
            .add_instrument(instrument_audusd.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Margin(margin_account(
                cash_account_state_million_usd,
            )))
            .unwrap();

        simple_cache.add_quote(quote_audusd).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_audusd.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("100000").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_audusd.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 0); // Currently, it executes because check_orders_risk returns true for margin_account
    }

    #[ignore = "Message bus related changes re-investigate"]
    #[rstest]
    fn test_submit_order_for_less_than_max_cum_transaction_value_adausdt_with_crypto_cash_account(
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        let quote = QuoteTick::new(
            instrument_xbtusd_bitmex.id(),
            Price::from("0.6109"),
            Price::from("0.6110"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache
            .add_instrument(instrument_xbtusd_bitmex.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        simple_cache.add_quote(quote).unwrap();

        let mut risk_engine =
            get_risk_engine(Some(Rc::new(RefCell::new(simple_cache))), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("440").unwrap())
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));
        let saved_process_messages =
            get_process_order_event_handler_messages(process_order_event_handler);
        assert_eq!(saved_process_messages.len(), 0);

        let saved_execute_messages =
            get_execute_order_event_handler_messages(execute_order_event_handler);
        assert_eq!(saved_execute_messages.len(), 1);
        assert_eq!(
            saved_execute_messages.first().unwrap().instrument_id(),
            instrument_xbtusd_bitmex.id()
        );
    }

    #[rstest]
    fn test_partial_fill_and_full_fill_account_balance_correct() {}

    #[rstest]
    fn test_submit_order_with_gtd_expire_time_already_passed(
        clock: TestClock,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
        process_order_event_handler: ShareableMessageHandler,
        execute_order_event_handler: ShareableMessageHandler,
        bitmex_cash_account_state_multi: AccountState,
        mut simple_cache: Cache,
    ) {
        msgbus::register(
            MessagingSwitchboard::exec_engine_process(),
            process_order_event_handler.clone(),
        );
        msgbus::register(
            MessagingSwitchboard::exec_engine_execute(),
            execute_order_event_handler.clone(),
        );

        let quote = QuoteTick::new(
            instrument_xbtusd_bitmex.id(),
            Price::from("0.6109"),
            Price::from("0.6110"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        simple_cache
            .add_instrument(instrument_xbtusd_bitmex.clone())
            .unwrap();

        simple_cache
            .add_account(AccountAny::Cash(cash_account(
                bitmex_cash_account_state_multi,
            )))
            .unwrap();

        simple_cache.add_quote(quote).unwrap();

        let cache = Rc::new(RefCell::new(simple_cache));

        let mut risk_engine = get_risk_engine(Some(cache.clone()), None, None, false);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_xbtusd_bitmex.id())
            .side(OrderSide::Buy)
            .price(Price::from("100_000.0"))
            .quantity(Quantity::from_str("440").unwrap())
            .time_in_force(TimeInForce::Gtd)
            .expire_time(UnixNanos::from(1_000)) // <-- Set expire time in the past
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_xbtusd_bitmex.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            None,
            UUID4::new(),
            clock.timestamp_ns(),
        )
        .unwrap();

        clock.set_time(UnixNanos::from(2_000)); // <-- Set time to 2,000 nanos past epoch

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        // TODO: Change command messages to not require owned orders
    }
}
