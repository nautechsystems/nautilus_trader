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

use std::{cell::RefCell, rc::Rc};

use nautilus_model::orders::OrderAny;

use crate::{
    matching_engine::engine::OrderMatchingEngine, order_emulator::emulator::OrderEmulator,
};

pub trait FillMarketOrderHandler {
    fn fill_market_order(&mut self, order: &OrderAny);
}

#[derive(Clone)]
pub enum FillMarketOrderHandlerAny {
    OrderMatchingEngine(Rc<RefCell<OrderMatchingEngine>>),
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

impl FillMarketOrderHandler for FillMarketOrderHandlerAny {
    fn fill_market_order(&mut self, order: &OrderAny) {
        match self {
            Self::OrderMatchingEngine(engine) => {
                engine.borrow_mut().fill_market_order(&mut order.clone());
            }
            Self::OrderEmulator(emulator) => {
                emulator.borrow_mut().fill_market_order(&mut order.clone());
            }
        }
    }
}

#[derive(Clone)]
pub struct ShareableFillMarketOrderHandler(pub FillMarketOrderHandlerAny);

pub trait FillLimitOrderHandler {
    fn fill_limit_order(&mut self, order: &mut OrderAny);
}

#[derive(Clone)]
pub enum FillLimitOrderHandlerAny {
    OrderMatchingEngine(Rc<RefCell<OrderMatchingEngine>>),
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

impl FillLimitOrderHandler for FillLimitOrderHandlerAny {
    fn fill_limit_order(&mut self, order: &mut OrderAny) {
        match self {
            Self::OrderMatchingEngine(engine) => {
                engine.borrow_mut().fill_limit_order(order);
            }
            Self::OrderEmulator(emulator) => {
                emulator.borrow_mut().fill_limit_order(order);
            }
        }
    }
}

#[derive(Clone)]
pub struct ShareableFillLimitOrderHandler(pub FillLimitOrderHandlerAny);

pub trait TriggerStopOrderHandler {
    fn trigger_stop_order(&mut self, order: &mut OrderAny);
}

#[derive(Clone)]
pub enum TriggerStopOrderHandlerAny {
    OrderMatchingEngine(Rc<RefCell<OrderMatchingEngine>>),
    OrderEmulator(Rc<RefCell<OrderEmulator>>),
}

impl TriggerStopOrderHandler for TriggerStopOrderHandlerAny {
    fn trigger_stop_order(&mut self, order: &mut OrderAny) {
        match self {
            Self::OrderMatchingEngine(engine) => {
                engine.borrow_mut().trigger_stop_order(order);
            }
            Self::OrderEmulator(emulator) => {
                emulator.borrow_mut().trigger_stop_order(order);
            }
        }
    }
}

#[derive(Clone)]
pub struct ShareableTriggerStopOrderHandler(pub TriggerStopOrderHandlerAny);
