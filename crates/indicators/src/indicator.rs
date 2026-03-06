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
