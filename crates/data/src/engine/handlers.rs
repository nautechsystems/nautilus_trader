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

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::msgbus::handler::MessageHandler;
use nautilus_model::data::{Bar, BarType, QuoteTick, TradeTick};
use ustr::Ustr;

use crate::aggregation::BarAggregator;

// Quote ticks -> bar aggregator
#[derive(Debug)]
pub struct BarQuoteHandler {
    aggregator: Rc<RefCell<Box<dyn BarAggregator>>>,
    bar_type: BarType,
}

impl BarQuoteHandler {
    pub(crate) fn new(aggregator: Rc<RefCell<Box<dyn BarAggregator>>>, bar_type: BarType) -> Self {
        Self {
            aggregator,
            bar_type,
        }
    }
}

impl MessageHandler for BarQuoteHandler {
    fn id(&self) -> Ustr {
        Ustr::from(&format!("BarQuoteHandler|{}", self.bar_type))
    }

    fn handle(&self, msg: &dyn Any) {
        if let Some(quote) = msg.downcast_ref::<QuoteTick>() {
            self.aggregator.borrow_mut().handle_quote(*quote);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Trade ticks -> bar aggregator
#[derive(Debug)]
pub struct BarTradeHandler {
    aggregator: Rc<RefCell<Box<dyn BarAggregator>>>,
    bar_type: BarType,
}

impl BarTradeHandler {
    pub(crate) fn new(aggregator: Rc<RefCell<Box<dyn BarAggregator>>>, bar_type: BarType) -> Self {
        Self {
            aggregator,
            bar_type,
        }
    }
}

impl MessageHandler for BarTradeHandler {
    fn id(&self) -> Ustr {
        Ustr::from(&format!("BarTradeHandler|{}", self.bar_type))
    }

    fn handle(&self, msg: &dyn Any) {
        if let Some(trade) = msg.downcast_ref::<TradeTick>() {
            self.aggregator.borrow_mut().handle_trade(*trade);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// Composite bars -> bar aggregator
#[derive(Debug)]
pub struct BarBarHandler {
    aggregator: Rc<RefCell<Box<dyn BarAggregator>>>,
    bar_type: BarType,
}

impl BarBarHandler {
    pub(crate) fn new(aggregator: Rc<RefCell<Box<dyn BarAggregator>>>, bar_type: BarType) -> Self {
        Self {
            aggregator,
            bar_type,
        }
    }
}

impl MessageHandler for BarBarHandler {
    fn id(&self) -> Ustr {
        Ustr::from(&format!("BarBarHandler|{}", self.bar_type))
    }

    fn handle(&self, msg: &dyn Any) {
        if let Some(bar) = msg.downcast_ref::<Bar>() {
            self.aggregator.borrow_mut().handle_bar(*bar);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
