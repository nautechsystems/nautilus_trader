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

//! A common `Indicator` trait.

use std::fmt::Debug;

use nautilus_model::{
    data::{Bar, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick},
    orderbook::OrderBook,
};

const IMPL_ERR: &str = "is not implemented for";

#[allow(unused_variables)]
pub trait Indicator {
    fn name(&self) -> String;

    fn has_inputs(&self) -> bool;

    fn initialized(&self) -> bool;

    fn handle_delta(&mut self, delta: &OrderBookDelta) {
        panic!("`handle_delta` {IMPL_ERR} `{}`", self.name());
    }

    fn handle_deltas(&mut self, deltas: &OrderBookDeltas) {
        panic!("`handle_deltas` {IMPL_ERR} `{}`", self.name());
    }

    fn handle_depth(&mut self, depth: &OrderBookDepth10) {
        panic!("`handle_depth` {IMPL_ERR} `{}`", self.name());
    }

    fn handle_book(&mut self, book: &OrderBook) {
        panic!("`handle_book_mbo` {IMPL_ERR} `{}`", self.name());
    }

    fn handle_quote(&mut self, quote: &QuoteTick) {
        panic!("`handle_quote_tick` {IMPL_ERR} `{}`", self.name());
    }

    fn handle_trade(&mut self, trade: &TradeTick) {
        panic!("`handle_trade_tick` {IMPL_ERR} `{}`", self.name());
    }

    fn handle_bar(&mut self, bar: &Bar) {
        panic!("`handle_bar` {IMPL_ERR} `{}`", self.name());
    }

    fn reset(&mut self);
}

pub trait MovingAverage: Indicator {
    fn value(&self) -> f64;
    fn count(&self) -> usize;
    fn update_raw(&mut self, value: f64);
}

impl Debug for dyn Indicator + Send {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Implement custom formatting for the Indicator trait object
        write!(f, "Indicator {{ ... }}")
    }
}

impl Debug for dyn MovingAverage + Send {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Implement custom formatting for the Indicator trait object
        write!(f, "MovingAverage()")
    }
}
