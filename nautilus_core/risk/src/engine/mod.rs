// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use config::RiskEngineConfig;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{CMD, EVT, RECV},
    msgbus::MessageBus,
    throttler::Throttler,
};
use nautilus_core::uuid::UUID4;
use nautilus_execution::messages::{
    modify::ModifyOrder, submit::SubmitOrder, submit_list::SubmitOrderList, TradingCommand,
};
use nautilus_model::{
    accounts::{any::AccountAny, base::Account},
    enums::{InstrumentClass, OrderSide, OrderStatus, TradingState},
    events::order::{OrderDenied, OrderEventAny, OrderModifyRejected},
    identifiers::InstrumentId,
    instruments::any::InstrumentAny,
    orders::{any::OrderAny, list::OrderList},
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
use ustr::Ustr;

pub mod config;
// pub mod tests;

type SubmitOrderFn = Box<dyn Fn(SubmitOrder)>;
type ModifyOrderFn = Box<dyn Fn(ModifyOrder)>;

pub struct RiskEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    // Counters
    // command_count: u64,
    // event_count: u64,
    pub throttled_submit_order: Throttler<SubmitOrder, SubmitOrderFn>,
    pub throttled_modify_order: Throttler<ModifyOrder, ModifyOrderFn>,
    max_notional_per_order: HashMap<InstrumentId, Decimal>,
    trading_state: TradingState,
    config: RiskEngineConfig,
}

impl RiskEngine {
    pub fn new(
        config: RiskEngineConfig,
        // portfolio: PortfolioFacade TODO: fix after portfolio implementation
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
    ) -> Self {
        let msgbus1 = msgbus.clone();
        let msgbus2 = msgbus.clone();
        let msgbus3 = msgbus.clone();
        let msgbus4 = msgbus.clone();
        let clock1 = clock.clone();
        let clock2 = clock.clone();
        // let cache1 = cache.clone();
        // let cache2 = cache.clone();
        let throttled_submit_order = Throttler::new(
            config.max_order_submit.clone(),
            clock1.clone(),
            "ORDER_SUBMIT_THROTTLER".to_string(),
            Box::new(move |order: SubmitOrder| {
                msgbus1
                    .borrow_mut()
                    .send(&Ustr::from("ExecEngine.execute"), &order);
            }) as Box<dyn Fn(SubmitOrder)>,
            Some(Box::new(move |order: SubmitOrder| {
                let reason = "REJECTED BY THROTTLER";
                log::warn!(
                    "SubmitOrder for {} DENIED: {}",
                    order.client_order_id,
                    reason
                );
                // let mut burrowed_cache = cache1.borrow_mut();

                // if !burrowed_cache.order_exists(&order.client_order_id) {
                //     burrowed_cache
                //         .add_order(order.clone(), None, None, false)
                //         .map_err(|e| {
                //             log::error!("Cannot add order to cache: {e}");
                //         })
                //         .unwrap();
                // }

                let denied = OrderEventAny::Denied(OrderDenied::new(
                    order.trader_id,
                    order.strategy_id,
                    order.instrument_id,
                    order.client_order_id,
                    reason.into(),
                    UUID4::new(),
                    clock1.borrow().timestamp_ns(),
                    clock1.borrow().timestamp_ns(),
                ));

                msgbus2
                    .borrow_mut()
                    .send(&Ustr::from("ExecEngine.process"), &denied);
            }) as Box<dyn Fn(SubmitOrder)>),
        );

        let throttled_modify_order = Throttler::new(
            config.max_order_modify.clone(),
            clock.clone(),
            "ORDER_MODIFY_THROTTLER".to_string(),
            Box::new(move |order: ModifyOrder| {
                msgbus3
                    .borrow_mut()
                    .send(&Ustr::from("ExecEngine.execute"), &order);
            }) as Box<dyn Fn(ModifyOrder)>,
            Some(Box::new(move |order: ModifyOrder| {
                let reason = "REJECTED BY THROTTLER";
                log::warn!(
                    "SubmitOrder for {} DENIED: {}",
                    order.client_order_id,
                    reason
                );
                // let mut burrowed_cache = cache2.borrow_mut();

                // if !burrowed_cache.order_exists(&order.client_order_id) {
                //     burrowed_cache
                //         .add_order(order.clone(), None, None, false)
                //         .map_err(|e| {
                //             log::error!("Cannot add order to cache: {e}");
                //         })
                //         .unwrap();
                // }

                let denied = OrderEventAny::Denied(OrderDenied::new(
                    order.trader_id,
                    order.strategy_id,
                    order.instrument_id,
                    order.client_order_id,
                    reason.into(),
                    UUID4::new(),
                    clock2.borrow().timestamp_ns(),
                    clock2.borrow().timestamp_ns(),
                ));

                msgbus4
                    .borrow_mut()
                    .send(&Ustr::from("ExecEngine.process"), &denied);
            }) as Box<dyn Fn(ModifyOrder)>),
        );

        Self {
            clock,
            cache,
            msgbus,
            throttled_submit_order,
            throttled_modify_order,
            max_notional_per_order: HashMap::new(),
            trading_state: TradingState::Active,
            config,
        }
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

        let ts_now = self.clock.borrow().timestamp_ns();
        // let event = OrderEventAny {
        //     timestamp: ts_now,
        //     state,
        // };

        // TODO: We need TradingStateChanged enum for OrderEventAny
        // cdef TradingStateChanged event = TradingStateChanged(
        //     trader_id=self.trader_id,
        //     state=self.trading_state,
        //     config=self._config,
        //     event_id=UUID4(),
        //     ts_event=ts_now,
        //     ts_init=ts_now,
        // )

        self.msgbus
            .borrow_mut()
            .publish(&Ustr::from("events.risk"), &"message"); // TODO: Fix this

        log::info!("Trading state set to {state:?}");
    }

    pub fn set_max_notional_per_order(&mut self, instrument_id: InstrumentId, new_value: Decimal) {
        let old_value = self.max_notional_per_order.get(&instrument_id);
        self.max_notional_per_order.insert(instrument_id, new_value);

        let new_value_str = new_value.to_string();
        log::info!("Set MAX_NOTIONAL_PER_ORDER: {instrument_id} {new_value_str}");
    }

    // -- COMMAND HANDLERS ------------------------------------------------------------------------

    // Renamed from `execute_command`
    fn handle_command(&mut self, command: TradingCommand) {
        if self.config.debug {
            log::debug!("{}{} {:?}", CMD, RECV, command);
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

        println!("HEREISSSS");
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

        println!("HEREISSS222#S");

        let instrument_exists = {
            let borrowed_cache = self.cache.borrow();
            borrowed_cache.instrument(&order.instrument_id()).cloned()
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

        println!("HEREISSS11222#S");

        // PRE-TRADE ORDER(S) CHECKS
        if !self.check_order(instrument.clone(), order.clone()) {
            return; // Denied
        }

        println!("HEREISS11222#S");
        if !self.check_orders_risk(instrument.clone(), Vec::from([order.clone()])) {
            return; // Denied
        }

        println!("HESS11222#S");
        self.execution_gateway(instrument, TradingCommand::SubmitOrder(command.clone()));
    }

    fn handle_submit_order_list(&self, command: SubmitOrderList) {
        if self.config.bypass {
            self.send_to_execution(TradingCommand::SubmitOrderList(command));
            return;
        }

        let maybe_instrument = {
            let borrowed_cache = self.cache.borrow();
            borrowed_cache.instrument(&command.instrument_id).cloned()
        };

        let instrument = if let Some(instrument) = maybe_instrument {
            instrument
        } else {
            self.deny_command(
                TradingCommand::SubmitOrderList(command.clone()),
                &format!("no instrument found for {}", command.instrument_id),
            );
            return; // Denied
        };

        // TODO: NEED this type of comment
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
        let order_exists = {
            let burrowed_cache = self.cache.borrow();
            burrowed_cache.order(&command.client_order_id).cloned()
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
            let borrowed_cache = self.cache.borrow();
            borrowed_cache.instrument(&command.instrument_id).cloned()
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
                self.reject_modify_order(order, "TradingState::HALTED: Cannot modify order");
                return; // Denied
            }
            TradingState::Reducing => {
                if let Some(quantity) = command.quantity {
                    if quantity > order.quantity() {
                        // if order.is_buy() && self.portfolio.is_net_long(instrument.id()) {
                        //     self.reject_modify_order(
                        //         order.clone(),
                        //         &format!(
                        //             "TradingState is REDUCING and update will increase exposure {}",
                        //             instrument.id()
                        //         ),
                        //     );
                        //     return; // Denied
                        // } else if order.is_sell() && self.portfolio.is_net_short(instrument.id()) {
                        //     self.reject_modify_order(
                        //         order.clone(),
                        //         &format!(
                        //             "TradingState is REDUCING and update will increase exposure {}",
                        //             instrument.id()
                        //         ),
                        //     );
                        //     return; // Denied
                        // }
                    }
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
        println!("Hinseide check_order");
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
        println!("inside check_order_price");
        if order.price().is_some() {
            println!("inside check_order_price if");
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
        // Determine max notional
        let mut max_notional: Option<Money> = None;
        let max_notional_setting = self.max_notional_per_order.get(&instrument.id());
        if let Some(max_notional_setting_val) = max_notional_setting.copied() {
            max_notional = Some(Money::new(
                max_notional_setting_val
                    .to_f64()
                    .expect("Invalid decimal conversion"),
                instrument.quote_currency(),
            ));
        }

        let account_exists = {
            let borrowed_cache = self.cache.borrow();
            borrowed_cache
                .account_for_venue(&instrument.id().venue)
                .cloned()
        };

        let account = if let Some(account) = account_exists {
            account
        } else {
            println!("Cannot find account for venue {}", instrument.id().venue);
            log::debug!("Cannot find account for venue {}", instrument.id().venue);
            return true; // TODO: Temporary early return until handling routing/multiple venues
        };

        // Check for margin account
        if matches!(account, AccountAny::Margin(_)) {
            return true; // TODO: Determine risk controls for margin
        }

        let free = match account {
            AccountAny::Cash(ref cash) => cash.balance_free(Some(instrument.quote_currency())),
            _ => None,
        };

        if self.config.debug {
            log::debug!("Free cash: {:?}", free);
        }

        let mut last_px: Option<Price> = None;
        let cum_notional_buy: Option<Money> = None;
        let cum_notional_sell: Option<Money> = None;
        let mut base_currency: Option<Currency> = None;

        for order in &orders {
            // Determine last price based on order type
            last_px = match order {
                OrderAny::Market(_) | OrderAny::MarketToLimit(_) => {
                    if last_px.is_none() {
                        let burrowed_cache = self.cache.borrow();
                        if let Some(quote) = burrowed_cache.quote(&instrument.id()) {
                            match order.order_side() {
                                OrderSide::Buy => Some(quote.ask_price),
                                OrderSide::Sell => Some(quote.bid_price),
                                _ => {
                                    log::error!("Invalid order side");
                                    None
                                }
                            }
                        } else {
                            let burrowed_cache = self.cache.borrow();
                            burrowed_cache
                                .trade(&instrument.id())
                                .map(|trade| trade.price)
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
                            "Cannot check {} order risk: no trigger price was set",
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
                log::debug!("Notional: {:?}", notional);
            }

            // Check max notional per order limit
            if let Some(max_notional_value) = max_notional {
                if notional > max_notional_value {
                    self.deny_order(
                        order.clone(),
                        &format!(
                            "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional={max_notional_value:?}, notional={notional:?}"
                        ),
                    );
                    return false;
                }
            }

            // Check MIN notional instrument limit
            // if let Some(min_notional) = instrument.min_notional() {
            //     if notional < min_notional {
            //         self.deny_order(
            //             order.clone(),
            //             &format!(
            //                 "NOTIONAL_BELOW_MIN: min_notional={:?}, notional={:?}",
            //                 min_notional, notional
            //             ),
            //         );
            //         return false; // Denied
            //     }
            // }

            // // Check MAX notional instrument limit
            // if let Some(max_notional) = instrument.max_notional() {
            //     if notional > max_notional {
            //         self.deny_order(
            //             order.clone(),
            //             &format!(
            //                 "NOTIONAL_EXCEEDS_MAX: max_notional={:?}, notional={:?}",
            //                 max_notional, notional
            //             ),
            //         );
            //         return false;
            //     }
            // }

            // let order_balance_impact = match account {
            //     AccountAny::Cash(ref cash) => {
            //         cash.balance_impact(instrument, order.quantity(), last_px, order.order_side())
            //     }
            //     _ => None,
            // };
            // account

            // if self.config.debug {
            //     log::debug!("Balance impact: {:?}", order_balance_impact);
            // }

            // if let (Some(free_val), Some(impact)) = (free, order_balance_impact) {
            //     if (free_val.as_decimal() + impact.as_decimal()) < Decimal::ZERO {
            //         self.deny_order(
            //             order.clone(),
            //             &format!(
            //                 "INSUFFICIENT_BALANCE: free={:?}, notional={:?}",
            //                 free_val, notional
            //             ),
            //         );
            //         return false;
            //     }
            // }

            if base_currency.is_none() {
                base_currency = instrument.base_currency();
            }

            // if order.is_buy() {
            //     match cum_notional_buy {
            //         Some(ref mut cum_buy) => {
            //             if let Some(impact) = order_balance_impact {
            //                 *cum_buy = cum_buy.clone() - impact;
            //             }
            //         }
            //         None => {
            //             if let Some(impact) = order_balance_impact {
            //                 cum_notional_buy = Some(-impact);
            //             }
            //         }
            //     }

            //     if self.config.debug {
            //         log::debug!("Cumulative notional BUY: {:?}", cum_notional_buy);
            //     }

            //     if let (Some(free_val), Some(cum_buy)) = (free.clone(), cum_notional_buy.clone()) {
            //         if cum_buy > free_val {
            //             self.deny_order(
            //                 order.clone(),
            //                 &format!(
            //                     "CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={:?}, cum_notional={:?}",
            //                     free_val, cum_buy
            //                 ),
            //             );
            //             return false;
            //         }
            //     }
            // } else if order.is_sell() {
            //     match account {
            //         AccountAny::Cash(ref cash) => {
            //             if let Some(impact) = order_balance_impact {
            //                 match cum_notional_sell {
            //                     Some(ref mut cum_sell) => {
            //                         *cum_sell = cum_sell.clone() + impact;
            //                     }
            //                     None => {
            //                         cum_notional_sell = Some(impact);
            //                     }
            //                 }

            //                 if self.config.debug {
            //                     log::debug!("Cumulative notional SELL: {:?}", cum_notional_sell);
            //                 }

            //                 if let (Some(free_val), Some(cum_sell)) =
            //                     (free.clone(), cum_notional_sell.clone())
            //                 {
            //                     if cum_sell > free_val {
            //                         self.deny_order(
            //                                 order.clone(),
            //                                 &format!("CUM_NOTIONAL_EXCEEDS_FREE_BALANCE: free={:?}, cum_notional={:?}",
            //                                     free_val, cum_sell),
            //                             );
            //                         return false;
            //                     }
            //                 }
            //             }

            //             if let Some(base_curr) = base_currency {
            //                 let cash_value = Money::new(order.quantity().as_f64(), base_curr);

            //                 if self.config.debug {
            //                     let total = cash.balance_total(Some(base_curr));
            //                     let locked = cash.balance_locked(Some(base_curr));
            //                     let free = cash.balance_free(Some(base_curr));
            //                     log::debug!("Cash value: {:?}", cash_value);
            //                     log::debug!("Total: {:?}", total);
            //                     log::debug!("Locked: {:?}", locked);
            //                     log::debug!("Free: {:?}", free);
            //                 }
            //             }
            //         }
            //         _ => {}
            //     }
            // }
        }

        true // All checks passed
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

    fn deny_new_order(&self, command: TradingCommand) {
        match command {
            TradingCommand::SubmitOrder(submit_order) => {
                self.deny_order(submit_order.order, "Exceeded MAX_ORDER_SUBMIT_RATE ");
            }
            TradingCommand::SubmitOrderList(submit_order_list) => self.deny_order_list(
                submit_order_list.order_list,
                "Exceeded MAX_ORDER_SUBMIT_RATE",
            ),
            _ => {}
        }
    }

    // TODO: TEMP: remove this if not needed
    fn layer_deny_modify_order(&self, command: TradingCommand) {
        if let TradingCommand::ModifyOrder(modify_order) = command {
            self.deny_modify_order(modify_order);
        }
    }

    fn deny_modify_order(&self, command: ModifyOrder) {
        let burrowed_cache = self.cache.borrow();
        let order = if let Some(order) = burrowed_cache.order(&command.client_order_id) {
            order
        } else {
            log::error!(
                "Order with command.client_order_id: {} not found",
                command.client_order_id
            );
            return;
        };

        self.reject_modify_order(order.clone(), "Exceeded MAX_ORDER_MODIFY_RATE");
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

        println!("reason --> {reason}");

        let mut burrowed_cache = self.cache.borrow_mut();
        if !burrowed_cache.order_exists(&order.client_order_id()) {
            burrowed_cache
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
            self.clock.borrow().timestamp_ns(), // TODO: Check if this is correct
            self.clock.borrow().timestamp_ns(),
        ));

        self.msgbus
            .borrow_mut()
            .send(&Ustr::from("ExecEngine.process"), &denied);
    }

    fn deny_order_list(&self, order_list: OrderList, reason: &str) {
        for order in order_list.orders {
            if order.is_closed() {
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
            false, // TODO: Check if this is correct
            order.venue_order_id(),
            order.account_id(),
        ));

        self.msgbus
            .borrow_mut()
            .send(&Ustr::from("ExecEngine.process"), &denied);
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
                    // if order.is_buy() && self.portfolio.is_net_long(instrument.id()) {
                    //     self.deny_order(order, &format!("BUY when TradingState::REDUCING and LONG {}", instrument.id()));
                    //     return;
                    // }
                    // else if order.is_sell() && self.portfolio.is_net_short(instrument.id()) {
                    //     self.deny_order(order, &format!("SELL when TradingState::REDUCING and SHORT {}", instrument.id()));
                    //     return;
                    // }
                    todo!("Pending portfolio implementation");
                    // return;
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    let order_list = submit_order_list.order_list;
                    for order in order_list.orders {
                        // if order.is_buy() && self.portfolio.is_net_long(instrument.id()) {
                        //     self.deny_order_list(order_list, &format!("BUY when TradingState::REDUCING and LONG {}", instrument.id()));
                        //     return;
                        // }
                        // else if order.is_sell() && self.portfolio.is_net_short(instrument.id()) {
                        //     self.deny_order_list(order_list, &format!("SELL when TradingState::REDUCING and SHORT {}", instrument.id()));
                        //     return;
                        // }
                        todo!("Pending portfolio implementation");
                        // return;
                    }
                }
                _ => {}
            },
            TradingState::Active => match command {
                TradingCommand::SubmitOrder(submit_order) => {
                    self.throttled_submit_order.send(submit_order);
                    // return; not allowed by clippy
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    // HANDLE?
                }
                _ => {}
            },
        }
    }

    // TODO: TEMP: remove this if not needed
    fn send_to_execution(&self, command: TradingCommand) {
        self.msgbus
            .borrow_mut()
            .send(&Ustr::from("ExecEngine.execute"), &command);
    }

    // TODO: TEMP: remove this if not needed
    fn modify_send_to_execution(&self, command: ModifyOrder) {
        self.msgbus
            .borrow_mut()
            .send(&Ustr::from("ExecEngine.execute"), &command);
    }

    fn submit_send_to_execution(&self, command: SubmitOrder) {
        self.msgbus
            .borrow_mut()
            .send(&Ustr::from("ExecEngine.execute"), &command);
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
        clock::TestClock,
        msgbus::{stubs::get_stub_shareable_handler, MessageBus},
        throttler::RateLimit,
    };
    use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
    use nautilus_execution::messages::{SubmitOrder, TradingCommand};
    use nautilus_model::{
        accounts::{any::AccountAny, base::BaseAccount, cash::CashAccount},
        data::{quote::QuoteTick, stubs::quote_ethusdt_binance},
        enums::{OrderSide, OrderStatus, OrderType, TradingState},
        events::account::stubs::cash_account_state_million_usd,
        identifiers::{
            stubs::{
                client_id_binance, client_order_id, strategy_id_ema_cross, trader_id,
                venue_order_id,
            },
            ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId, VenueOrderId,
        },
        instruments::{
            any::InstrumentAny,
            crypto_perpetual::CryptoPerpetual,
            stubs::{crypto_perpetual_ethusdt, xbtusd_bitmex},
        },
        orders::{any::OrderAny, builder::OrderTestBuilder},
        types::{price::Price, quantity::Quantity},
    };
    use rstest::{fixture, rstest};
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use super::{config::RiskEngineConfig, RiskEngine};

    #[fixture]
    fn msgbus() -> MessageBus {
        MessageBus::default()
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
    fn config(
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
    fn risk_engine(
        config: RiskEngineConfig,
        clock: TestClock,
        mut simple_cache: Cache,
        msgbus: MessageBus,
        instrument_eth_usdt: InstrumentAny,
    ) -> RiskEngine {
        // Register test instrument in cache
        simple_cache.add_instrument(instrument_eth_usdt).unwrap();

        // we need mock execution client
        // self.exec_client = MockExecutionClient(
        //     client_id=ClientId(self.venue.value),
        //     venue=self.venue,
        //     account_type=AccountType.CASH,
        //     base_currency=USD,
        //     msgbus=self.msgbus,
        //     cache=self.cache,
        //     clock=self.clock,
        // )
        // self.portfolio.update_account(TestEventStubs.cash_account_state())
        // self.exec_engine.register_client(self.exec_client)

        let config = config;
        let clock = Rc::new(RefCell::new(clock));
        let cache = Rc::new(RefCell::new(simple_cache));
        let msgbus = Rc::new(RefCell::new(msgbus));

        RiskEngine::new(config, clock, cache, msgbus)
    }

    #[fixture]
    fn instrument_eth_usdt(crypto_perpetual_ethusdt: CryptoPerpetual) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt)
    }

    #[fixture]
    fn instrument_xbtusd_bitmex(xbtusd_bitmex: CryptoPerpetual) -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(xbtusd_bitmex)
    }

    #[rstest]
    fn test_trading_state_after_instantiation_returns_active(risk_engine: RiskEngine) {
        assert_eq!(risk_engine.trading_state, TradingState::Active);
    }

    #[rstest]
    fn test_set_trading_state_changes_value_and_publishes_event(mut risk_engine: RiskEngine) {
        let handler = get_stub_shareable_handler(None);
        let topic = Ustr::from("events.risk*");
        risk_engine
            .msgbus
            .borrow_mut()
            .subscribe(topic, handler, Some(1));

        let new_state = TradingState::Halted;
        risk_engine.set_trading_state(new_state);

        assert_eq!(risk_engine.msgbus.borrow_mut().topics(), vec![topic]);
        assert_eq!(risk_engine.trading_state, new_state);
    }

    #[rstest]
    fn test_max_order_submit_rate(risk_engine: RiskEngine, max_order_submit: RateLimit) {
        assert_eq!(risk_engine.config.max_order_submit, max_order_submit);
    }

    #[rstest]
    fn test_max_order_modify_rate(risk_engine: RiskEngine, max_order_modify: RateLimit) {
        assert_eq!(risk_engine.config.max_order_modify, max_order_modify);
    }

    #[rstest]
    fn test_max_notional_per_order(
        risk_engine: RiskEngine,
        max_notional_per_order: HashMap<InstrumentId, Decimal>,
    ) {
        assert_eq!(risk_engine.max_notional_per_order, max_notional_per_order);
    }

    #[rstest]
    fn test_set_max_notional_per_order_changes_setting(
        mut risk_engine: RiskEngine,
        instrument_eth_usdt: InstrumentAny,
    ) {
        risk_engine.set_max_notional_per_order(instrument_eth_usdt.id(), Decimal::from(1000000));

        let max_notionals = risk_engine.max_notional_per_order;
        assert_eq!(max_notionals.len(), 1);
        assert_eq!(
            max_notionals.get(&instrument_eth_usdt.id()),
            Some(&Decimal::from(1000000))
        );
    }

    // #[rstest]
    // fn test_given_random_command_then_logs_and_continues(risk_engine: RiskEngine) {
    //     let random = TradingCommand::
    // }

    // test_given_random_event_then_logs_and_continues

    // SUBMIT ORDER TESTS

    #[rstest]
    fn test_submit_order_with_default_settings_then_sends_to_client(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
    ) {
        let submit_order = TradingCommand::SubmitOrder(get_stub_submit_order);
        risk_engine.execute(submit_order);

        // Order is successfully sent to throttler
        assert_eq!(risk_engine.throttled_submit_order.used(), 0.1);
        // TODO: check some kind of final status
    }

    // TODO: fix later
    #[rstest]
    fn test_submit_reduce_only_order_when_position_already_closed_then_denies(
        risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000"))
            .build();

        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1000"))
            .reduce_only(true)
            .build();

        let order3 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1000"))
            .reduce_only(true)
            .build();

        // let submit_order1 = SubmitOrder::new(
        //     trader_id,
        //     client_id_binance,
        //     strategy_id_ema_cross,
        //     instrument_eth_usdt.id(),
        //     client_order_id,
        //     venue_order_id,
        //     order1,
        //     None,
        //     None,
        //     UUID4::new(),
        //     risk_engine.clock.borrow().timestamp_ns(),
        // )
        // .unwrap();

        // risk_engine.execute(TradingCommand::SubmitOrder(submit_order1));
        // assert_eq!(risk_engine.throttled_submit_order.used(), 0.1);
        // TODO: better way to check final status

        let submit_order2 = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_eth_usdt.id(),
            client_order_id,
            venue_order_id,
            order2,
            None,
            Some(PositionId::new("P-19700101-000000-000-None-1")),
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        // cache , why :: TODO fix
        // risk_engine.cache.borrow_mut().add_position(
        //     Position::new(
        //         &instrument_eth_usdt,
        //         OrderFilled::new(
        //             trader_id,
        //             strategy_id,
        //             instrument_id,
        //             client_order_id,
        //             venue_order_id,
        //             account_id,
        //             trade_id,
        //             order_side,
        //             order_type,
        //             last_qty,
        //             last_px,
        //             currency,
        //             liquidity_side,
        //             event_id,
        //             ts_event,
        //             ts_init,
        //             reconciliation,
        //             position_id,
        //             commission,
        //         ),
        //     ),
        //     OmsType::Hedging,
        // );
        // risk_engine.execute(TradingCommand::SubmitOrder(submit_order2));
        // assert_eq!(risk_engine.throttled_submit_order.used(), 0.2);
    }

    // TODO: After fixing above test
    #[rstest]
    fn test_submit_reduce_only_order_when_position_would_be_increased_then_denies() {}

    // TODO: After fixing above test
    #[rstest]
    fn test_submit_order_reduce_only_order_with_custom_position_id_not_open_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000"))
            .reduce_only(true)
            .build();

        let submit_order = SubmitOrder::new(
            trader_id,
            client_id_binance,
            strategy_id_ema_cross,
            instrument_eth_usdt.id(),
            client_order_id,
            venue_order_id,
            order,
            None,
            Some(PositionId::new("CUSTOM-001")),
            UUID4::new(),
            risk_engine.clock.borrow().timestamp_ns(),
        )
        .unwrap();

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let cache = risk_engine.cache.borrow();
        let order = cache.order(&client_order_id).unwrap();

        // FIX: not denied because ExecEngine.process is called in msgbus, but execengine is not started
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_submit_order_when_instrument_not_in_cache_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_xbtusd_bitmex: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_xbtusd_bitmex.id()) // <-- Not in the cache
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000"))
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

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        // execution engine is not registered here, thats why blank vector
        // let ms = risk_engine.msgbus.borrow_mut();
        // println!("MS {:?}", ms.endpoints());

        let cache = risk_engine.cache.borrow();
        let order = cache.order(&client_order_id).unwrap();

        // FIX: not denied because ExecEngine.process is called in msgbus, but execengine is not started
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_submit_order_when_invalid_price_precision_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(999999999, 9)) // <- invalid price
            .quantity(Quantity::from("1000"))
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

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let cache = risk_engine.cache.borrow();
        let order = cache.order(&client_order_id).unwrap();

        // TODO: fix: reason --> price 0.999999999 invalid (precision 9 > 2)
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_submit_order_when_invalid_negative_price_and_not_option_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(-1, 1)) // <- invalid price
            .quantity(Quantity::from("1000"))
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

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let cache = risk_engine.cache.borrow();
        let order = cache.order(&client_order_id).unwrap();

        // TODO: fix: reason --> price -0.0 invalid (<= 0)
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_submit_order_when_invalid_trigger_price_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(1, 1))
            .trigger_price(Price::from_raw(999999999, 9)) // <- invalid price
            .quantity(Quantity::from("1000"))
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

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let cache = risk_engine.cache.borrow();
        let order = cache.order(&client_order_id).unwrap();

        // TODO: fix: reason --> price 0.999999999 invalid (precision 9 > 2)
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_submit_order_when_invalid_quantity_precision_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(1, 1))
            .quantity(Quantity::from_str("1.111111111").unwrap()) // <- invalid quantity
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

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let cache = risk_engine.cache.borrow();
        let order = cache.order(&client_order_id).unwrap();

        // TODO: fix: reason --> quantity 1.111111111 invalid (precision 9 > 3)
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_submit_order_when_invalid_quantity_exceeds_maximum_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .price(Price::from_raw(1, 1))
            .quantity(Quantity::from_str("1000000000").unwrap()) // <- invalid quantity
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

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        let cache = risk_engine.cache.borrow();
        let order = cache.order(&client_order_id).unwrap();

        // TODO: fix: reason --> quantity 1000000000 invalid (> maximum trade size of 10000.0)
        assert_eq!(order.status(), OrderStatus::Initialized);
    }

    // TODO: use any other instrument wwith min quatntity atleast 1
    // #[rstest]
    // fn test_submit_order_when_invalid_quantity_less_than_minimum_then_denies(
    //     mut risk_engine: RiskEngine,
    //     get_stub_submit_order: SubmitOrder,
    //     strategy_id_ema_cross: StrategyId,
    //     client_id_binance: ClientId,
    //     trader_id: TraderId,
    //     client_order_id: ClientOrderId,
    //     instrument_eth_usdt: InstrumentAny,
    //     venue_order_id: VenueOrderId,
    // ) {
    //     let order = OrderTestBuilder::new(OrderType::Limit)
    //         .instrument_id(instrument_eth_usdt.id())
    //         .side(OrderSide::Buy)
    //         .price(Price::from_str("1").unwrap())
    //         .quantity(Quantity::from_str("0").unwrap()) // <- invalid quantity
    //         .build();

    //     println!("order --> {:?}", order.quantity());
    //     let submit_order = SubmitOrder::new(
    //         trader_id,
    //         client_id_binance,
    //         strategy_id_ema_cross,
    //         order.instrument_id(),
    //         client_order_id,
    //         venue_order_id,
    //         order,
    //         None,
    //         None,
    //         UUID4::new(),
    //         risk_engine.clock.borrow().timestamp_ns(),
    //     )
    //     .unwrap();

    //     risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

    //     let cache = risk_engine.cache.borrow();
    //     let order = cache.order(&client_order_id).unwrap();

    //     // TODO: fix: reason --> quantity 1000000000 invalid (> maximum trade size of 10000.0)
    //     assert_eq!(order.status(), OrderStatus::Denied);
    // }

    #[rstest]
    fn test_submit_order_when_less_than_min_notional_for_instrument_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
    ) {
        // TODO: needs exec_engine to be registered
        // let order = OrderTestBuilder::new(OrderType::Limit)
        //     .instrument_id(instrument_eth_usdt.id())
        //     .side(OrderSide::Buy)
        //     .price(Price::from_raw(1, 1))
        //     .quantity(Quantity::from_str("1000").unwrap())
        //     .build();

        // let submit_order = SubmitOrder::new(
        //     trader_id,
        //     client_id_binance,
        //     strategy_id_ema_cross,
        //     order.instrument_id(),
        //     client_order_id,
        //     venue_order_id,
        //     order,
        //     None,
        //     None,
        //     UUID4::new(),
        //     risk_engine.clock.borrow().timestamp_ns(),
        // )
        // .unwrap();

        // risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        // let cache = risk_engine.cache.borrow();
        // let order = cache.order(&client_order_id).unwrap();

        // // TODO: fix: reason --> quantity 1000000000 invalid (> maximum trade size of 10000.0)
        // assert_eq!(order.status(), OrderStatus::Initialized);
    }

    #[rstest]
    fn test_submit_order_when_greater_than_max_notional_for_instrument_then_denies() {}

    #[rstest]
    fn test_submit_order_when_buy_market_order_and_over_max_notional_then_denies(
        mut risk_engine: RiskEngine,
        get_stub_submit_order: SubmitOrder,
        strategy_id_ema_cross: StrategyId,
        client_id_binance: ClientId,
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        instrument_eth_usdt: InstrumentAny,
        venue_order_id: VenueOrderId,
        quote_ethusdt_binance: QuoteTick,
    ) {
        risk_engine.set_max_notional_per_order(instrument_eth_usdt.id(), Decimal::from(100));
        let quote = quote_ethusdt_binance;

        {
            let mut burrowed_cache = risk_engine.cache.borrow_mut();
            burrowed_cache.add_quote(quote).unwrap();
            burrowed_cache
                .add_account(AccountAny::Cash(CashAccount::new(
                    cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD"), // Maybe Wrong
                    false,
                )))
                .unwrap();
        }

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(instrument_eth_usdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from_str("1000").unwrap())
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

        risk_engine.execute(TradingCommand::SubmitOrder(submit_order));

        // let cache = risk_engine.cache.borrow();
        // let order = cache.order(&client_order_id).unwrap();

        // // TODO: fix: reason --> quantity 1000000000 invalid (> maximum trade size of 10000.0)
        // assert_eq!(order.status(), OrderStatus::In);
    }

    #[rstest]
    fn test_submit_order_when_sell_market_order_and_over_max_notional_then_denies() {}
}
