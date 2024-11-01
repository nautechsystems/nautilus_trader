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
use nautilus_common::{cache::Cache, clock::Clock, msgbus::MessageBus, throttler::Throttler};
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

    pub fn execute(&self, command: TradingCommand) {
        // This will extend to other commands such as `RiskCommand`
        todo!()
    }

    pub fn process(&self, event: OrderEventAny) {
        // This will extend to other events such as `RiskEvent`
        todo!()
    }

    pub fn set_trading_state(&mut self, state: TradingState) {
        if state == self.trading_state {
            log::debug!("Trading state unchanged: {state:?}");
            // Improve this comment
            return;
        }

        self.trading_state = state;

        let ts_now = self.clock.timestamp_ns();
        // let event = OrderEventAny {
        //     timestamp: ts_now,
        //     state,
        // };

        self.msgbus
            .borrow_mut()
            .publish(&Ustr::from("events.risk"), &"message"); // TODO: Fix this
                                                              // self.log_state();
        todo!()
    }

    pub fn set_max_notional_per_order(&mut self, instrument_id: InstrumentId, new_value: Decimal) {
        // if new_value.is
        let old_value = self.max_notional_per_order.get(&instrument_id);
        self.max_notional_per_order.insert(instrument_id, new_value);

        let new_value_str = new_value.to_string();
        // log
        // self._log.info(
        //     f"Set MAX_NOTIONAL_PER_ORDER: {instrument_id} {new_value_str}",
        //     color=LogColor.BLUE,
        // )

        todo!()
    }

    // -- COMMAND HANDLERS ------------------------------------------------------------------------

    fn handle_command(&mut self, command: TradingCommand) {
        if self.config.debug {
            log::debug!("-->[CMD] {command:?}");
            // self._log.debug(f"{RECV}{CMD} {command}", LogColor.MAGENTA)
        }
        self.command_count += 1;

        match command {
            TradingCommand::SubmitOrder(submit_order) => self.handle_submit_order(submit_order),
            TradingCommand::SubmitOrderList(submit_order_list) => {
                self.handle_submit_order_list(submit_order_list)
            }
            // TradingCommand::ModifyOrder(modify_order) => self.handle_modify_order(modify_order),
            _ => {
                // self._log.error(f"Unknown command: {command}", color=LogColor.RED)
                todo!()
            }
        }
        // Renamed from `execute_command`
        // todo!();
    }

    fn handle_submit_order(&self, command: SubmitOrder) {
        if self.config.bypass {
            self.send_to_execution(TradingCommand::SubmitOrder(command));
            return;
        }

        let order = &command.order;
        // let order = OrderAny::;

        // Check reduce only

        // if command.position_id is not None:
        // if order.is_reduce_only:
        //     position = self._cache.position(command.position_id)
        //     if position is None or not order.would_reduce_only(position.side, position.quantity):
        //         self._deny_command(
        //             command=command,
        //             reason=f"Reduce only order would increase position {command.position_id!r}",
        //         )

        if let Some(position_id) = command.position_id {
            if order.is_reduce_only() {
                let position = match self.cache.borrow().position(&position_id) {
                    Some(pos) => pos,
                    None => {
                        self.deny_command(
                            TradingCommand::SubmitOrder(command),
                            &format!("Position {} not found for reduce-only order", position_id),
                        );
                        return;
                    }
                };
            }
        }

        // # Get instrument for order
        // cdef Instrument instrument = self._cache.instrument(order.instrument_id)
        // if instrument is None:
        //     self._deny_command(
        //         command=command,
        //         reason=f"Instrument for {command.instrument_id} not found",
        //     )
        //     return  # Denied

        // Get instrument for order
        let bindings = self.cache.borrow();
        let instrument = bindings.instrument(&order.instrument_id());
        if instrument.is_none() {
            // self._log.error(f"Instrument not found: {order.instrument_id}", color=LogColor.RED)
            self.deny_command(TradingCommand::SubmitOrder(command), "Instrument not found")
        }
        // todo!();
    }

    fn handle_submit_order_list(&self, command: SubmitOrderList) {
        if self.config.bypass {
            self.send_to_execution(TradingCommand::SubmitOrderList(command));
            return;
        }

        let bindings = self.cache.borrow();
        let instrument = bindings.instrument(&command.instrument_id);

        if instrument.is_none() {
            self.deny_command(
                TradingCommand::SubmitOrderList(command),
                "Instrument not found",
            );
            return;
        }

        ////////////////////////////////////////////////////////////////////////////////
        // PRE-TRADE ORDER(S) CHECKS
        ////////////////////////////////////////////////////////////////////////////////
        for order in command.order_list.orders.into_iter() {
            if !self.check_order(instrument.unwrap().clone(), order) {
                return; // Denied
            }
        }

        // if !self.check_orders_risk(instrument.unwrap().clone(), command.order_list.clone().orders) {
        //     self.deny_order_list(command.order_list, "OrderList {command.order_list.id.to_str()} DENIED");
        //     return; // Denied
        // }

        todo!()

        // self.execution_gateway(instrument, command);
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
        if let Some(max_notional_setting_val) = max_notional_setting.cloned() {
            max_notional = Some(Money::new(
                max_notional_setting_val
                    .to_f64()
                    .expect("Invalid decimal conversion"),
                instrument.quote_currency(),
            ));
        }

        // Get account for risk checks
        let binding = self.cache.borrow();
        let account = binding.account_for_venue(&instrument.id().venue);
        if account.is_none() {
            log::debug!("Cannot find account for venue {}", instrument.id().venue);
            return true; // TODO: Temporary early return until handling routing/multiple venues
        }

        let account = account.unwrap();

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

        for order in orders.iter() {
            // Determine last price based on order type
            last_px = match order {
                OrderAny::Market(_) | OrderAny::MarketToLimit(_) => {
                    if last_px.is_none() {
                        let binding = self.cache.borrow();
                        if let Some(quote) = binding.quote(&instrument.id()) {
                            match order.order_side() {
                                OrderSide::Buy => Some(quote.ask_price),
                                OrderSide::Sell => Some(quote.bid_price),
                                _ => {
                                    log::error!("Invalid order side");
                                    None
                                }
                            }
                        } else {
                            let binding = self.cache.borrow();
                            binding.trade(&instrument.id()).map(|trade| trade.price)
                        }
                    } else {
                        last_px
                    }
                }
                OrderAny::StopMarket(_) | OrderAny::MarketIfTouched(_) => order.trigger_price(),
                OrderAny::TrailingStopMarket(_) | OrderAny::TrailingStopLimit(_) => {
                    match order.trigger_price() {
                        Some(trigger_price) => Some(trigger_price),
                        None => {
                            log::warn!(
                                "Cannot check {} order risk: no trigger price was set",
                                order.order_type()
                            );
                            continue;
                        }
                    }
                }
                _ => order.price(),
            };

            let last_px = match last_px {
                Some(px) => px,
                None => {
                    log::error!("Cannot check order risk: no price available");
                    continue;
                }
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
                            "NOTIONAL_EXCEEDS_MAX_PER_ORDER: max_notional={:?}, notional={:?}",
                            max_notional_value, notional
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
            return Some(format!("price {} invalid (<= 0)", price_val));
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
                self.deny_order(submit_order.order, reason)
            }
            TradingCommand::SubmitOrderList(submit_order_list) => {
                self.deny_order_list(submit_order_list.order_list, reason)
            }
            _ => {
                panic!("Cannot deny command {}", command);
            }
        }
    }

    fn deny_new_order(&self, command: TradingCommand) {
        match command {
            TradingCommand::SubmitOrder(submit_order) => {
                self.deny_order(submit_order.order, "Exceeded MAX_ORDER_SUBMIT_RATE ")
            }
            TradingCommand::SubmitOrderList(submit_order_list) => self.deny_order_list(
                submit_order_list.order_list,
                "Exceeded MAX_ORDER_SUBMIT_RATE",
            ),
            _ => {}
        }
    }

    fn deny_modify_order(&self, command: ModifyOrder) {
        let binding = self.cache.borrow();
        let order = binding.order(&command.client_order_id);

        if order.is_none() {
            log::error!("Cannot find order for modify: {}", command.client_order_id);
            return;
        }

        self.reject_modify_order(order.unwrap().clone(), "Exceeded MAX_ORDER_MODIFY_RATE");
    }

    fn deny_order(&self, order: OrderAny, reason: &str) {
        log::warn!(
            "SubmitOrder for {} DENIED: {}",
            order.client_order_id(),
            reason
        );

        // if order.is

        if order.status() != OrderStatus::Initialized {
            return;
        }

        if !self.cache.borrow().order_exists(&order.client_order_id()) {
            // TODO: handle error
            let _ = self
                .cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false);
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
                    self.deny_order(submit_order.order, "TradingState::HALTED")
                }
                TradingCommand::SubmitOrderList(submit_order_list) => {
                    self.deny_order_list(submit_order_list.order_list, "TradingState::HALTED")
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
                    self.order_submit_throttler.send(submit_order)
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
            log::debug!("<--[EVT] {event:?}");
        }
        self.event_count += 1;
    }
}
