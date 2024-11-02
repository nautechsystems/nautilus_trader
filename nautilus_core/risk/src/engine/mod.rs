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

pub struct RiskEngine<C>
where
    C: Clock,
{
    clock: C,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    // Counters
    command_count: u64,
    event_count: u64,
    order_submit_throttler: Throttler<SubmitOrder, Box<dyn Fn(SubmitOrder)>>,
    order_modify_throttler: Throttler<ModifyOrder, Box<dyn Fn(ModifyOrder)>>,
    max_notional_per_order: HashMap<InstrumentId, Decimal>,
    trading_state: TradingState,
    config: RiskEngineConfig,
}

impl<C> RiskEngine<C>
where
    C: Clock,
{
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

        let ts_now = self.clock.timestamp_ns();
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
            // TradingCommand::ModifyOrder(modify_order) => self.handle_modify_order(modify_order),
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
                if let Some(pos) = self.cache.borrow().position(&position_id) {
                    if !order.would_reduce_only(pos.side, pos.quantity) {
                        self.deny_command(
                            TradingCommand::SubmitOrder(command),
                            &format!("Reduce only order would increase position {position_id}"),
                        );
                        return;
                    }
                } else {
                    self.deny_command(
                        TradingCommand::SubmitOrder(command),
                        &format!("Position {position_id} not found for reduce-only order"),
                    );
                    return;
                };
            }
        }

        let borrowed_cache = self.cache.borrow();

        let instrument = if let Some(instrument) = borrowed_cache.instrument(&order.instrument_id())
        {
            instrument
        } else {
            self.deny_command(
                TradingCommand::SubmitOrder(command.clone()),
                &format!("Instrument for {} not found", command.instrument_id),
            );
            return; // Denied
        };

        // PRE-TRADE ORDER(S) CHECKS
        if !self.check_order(instrument.clone(), order.clone()) {
            return; // Denied
        }

        if !self.check_orders_risk(instrument.clone(), Vec::from([order.clone()])) {
            return; // Denied
        }

        self.execution_gateway(
            instrument.clone(),
            TradingCommand::SubmitOrder(command.clone()),
        );
    }

    fn handle_submit_order_list(&self, command: SubmitOrderList) {
        if self.config.bypass {
            self.send_to_execution(TradingCommand::SubmitOrderList(command));
            return;
        }

        let borrowed_cache = self.cache.borrow();
        let instrument = if let Some(instrument) = borrowed_cache.instrument(&command.instrument_id)
        {
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

        self.execution_gateway(instrument.clone(), TradingCommand::SubmitOrderList(command));
    }

    fn handle_modify_order(&self, command: ModifyOrder) {
        let burrowed_cache = self.cache.borrow();
        let order = if let Some(order) = burrowed_cache.order(&command.client_order_id) {
            order
        } else {
            log::error!(
                "ModifyOrder DENIED: Order with command.client_order_id: {} not found",
                command.client_order_id
            );
            return;
        };

        // if order.is_closed() {
        // } else if order.status() == OrderStatus::PendingCancel {
        // }

        // Get instrument for orders
        let instrument = if let Some(instrument) = burrowed_cache.instrument(&order.instrument_id())
        {
            instrument
        } else {
            self.reject_modify_order(
                order.clone(),
                &format!("no instrument found for {}", command.instrument_id),
            );
            return; // Denied
        };

        // Check Price
        let mut risk_msg = self.check_price(instrument, command.price);
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(order.clone(), &risk_msg);
            return; // Denied
        }

        // Check Trigger
        risk_msg = self.check_price(instrument, command.trigger_price);
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(order.clone(), &risk_msg);
            return; // Denied
        }

        // Check Quantity
        risk_msg = self.check_quantity(instrument, command.quantity);
        if let Some(risk_msg) = risk_msg {
            self.reject_modify_order(order.clone(), &risk_msg);
            return; // Denied
        }

        // Check TradingState
        match self.trading_state {
            TradingState::Halted => {
                self.reject_modify_order(
                    order.clone(),
                    "TradingState::HALTED: Cannot modify order",
                );
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

        self.order_modify_throttler.send(command);
    }

    // -- PRE-TRADE CHECKS ------------------------------------------------------------------------

    fn check_order(&self, instrument: InstrumentAny, order: OrderAny) -> bool {
        ////////////////////////////////////////////////////////////////////////////////
        // VALIDATION CHECKS
        ////////////////////////////////////////////////////////////////////////////////
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

        // Get account for risk checks
        let borrowed_cache = self.cache.borrow();
        let account =
            if let Some(account) = borrowed_cache.account_for_venue(&instrument.id().venue) {
                account
            } else {
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
        // if let Some(max_quantity) = instrument.max_quantity() {
        //     if quantity_val > max_quantity {
        //         return Some(format!(
        //             "quantity {} invalid (> maximum trade size of {})",
        //             quantity_val,
        //             max_quantity
        //         ));
        //     }
        // }

        // // Check minimum quantity
        // if let Some(min_quantity) = instrument.min_quantity() {
        //     if quantity_val < min_quantity {
        //         return Some(format!(
        //             "quantity {} invalid (< minimum trade size of {})",
        //             quantity_val,
        //             min_quantity
        //         ));
        //     }
        // }

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
            self.clock.timestamp_ns(), // TODO: Check if this is correct
            self.clock.timestamp_ns(),
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
        let denied = OrderEventAny::ModifyRejected(OrderModifyRejected::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            self.clock.timestamp_ns(),
            self.clock.timestamp_ns(),
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
                    self.order_submit_throttler.send(submit_order);
                    // return; not allowed by clippy
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    // HANDLE?
                }
                _ => {}
            },
        }
    }

    fn send_to_execution(&self, command: TradingCommand) {
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
