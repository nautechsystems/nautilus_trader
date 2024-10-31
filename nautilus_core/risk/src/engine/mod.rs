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
use nautilus_execution::messages::{
    modify::ModifyOrder, submit::SubmitOrder, submit_list::SubmitOrderList, TradingCommand,
};
use nautilus_model::{
    enums::TradingState,
    events::order::OrderEventAny,
    identifiers::InstrumentId,
    instruments::any::InstrumentAny,
    orders::{any::OrderAny, list::OrderList},
    types::quantity::Quantity,
};
use rust_decimal::Decimal;

pub mod config;

pub struct RiskEngine<C>
where
    C: Clock,
{
    clock: C,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    order_submit_throttler: Throttler<SubmitOrder, Box<dyn Fn(SubmitOrder)>>,
    order_modify_throttler: Throttler<ModifyOrder, Box<dyn Fn(ModifyOrder)>>,
    max_notional_per_order: HashMap<InstrumentId, Decimal>,
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

    pub fn set_trading_state(&self, state: TradingState) {
        todo!()
    }

    pub fn set_max_notional_per_order(&self, instrument_id: InstrumentId, new_value: Decimal) {
        todo!()
    }

    // -- COMMAND HANDLERS ------------------------------------------------------------------------

    fn handle_command(&self, command: TradingCommand) {
        // Renamed from `execute_command`
        todo!();
    }

    fn handle_submit_order(&self, command: SubmitOrder) {
        todo!();
    }

    fn handle_submit_order_list(&self, command: SubmitOrderList) {
        todo!();
    }

    // -- PRE-TRADE CHECKS ------------------------------------------------------------------------

    fn check_order(&self, instrument: InstrumentAny, order: OrderAny) -> bool {
        todo!()
    }

    fn check_order_price(&self, instrument: InstrumentAny, order: OrderAny) -> bool {
        todo!()
    }

    fn check_order_quantity(&self, instrument: InstrumentAny, order: OrderAny) -> bool {
        todo!()
    }

    fn check_orders_risk(&self, instrument: InstrumentAny, orders: Vec<OrderAny>) -> bool {
        todo!()
    }

    fn check_price(&self, instrument: InstrumentAny, quantity: Quantity) -> &str {
        todo!()
    }

    fn check_quantity(&self, instrument: InstrumentAny, quantity: Quantity) -> &str {
        todo!()
    }

    // -- DENIALS ---------------------------------------------------------------------------------

    fn deny_command(&self, command: TradingCommand, reason: &str) {
        todo!()
    }

    fn deny_new_order(&self, command: TradingCommand) {
        todo!()
    }

    fn deny_modify_order(&self, command: TradingCommand) {
        todo!()
    }

    fn deny_order(&self, order: OrderAny, reason: &str) {
        todo!()
    }

    fn deny_order_list(&self, order_list: OrderList, reason: &str) {
        todo!()
    }

    fn reject_modify_order(&self, order: OrderAny, reason: &str) {}

    // -- EGRESS ----------------------------------------------------------------------------------

    fn execution_gateway(&self, instrument: InstrumentAny, command: TradingCommand) {
        todo!()
    }

    fn send_to_execution(&self, command: TradingCommand) {
        todo!()
    }

    fn handle_event(&self, event: OrderEventAny) {
        if self.config.debug {
            log::debug!("<--[EVT] {event:?}");
        }

        // We intend to extend the risk engine to be able to handle additional events.
        // For now we just log.
    }
}
