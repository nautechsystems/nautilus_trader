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

use nautilus_core::time::UnixNanos;

use super::{
    book::{get_avg_px_for_quantity, get_quantity_for_price},
    display::pprint_book,
    level::Level,
};
use crate::{
    data::{
        delta::OrderBookDelta, deltas::OrderBookDeltas, depth::OrderBookDepth10, order::BookOrder,
    },
    enums::{BookAction, OrderSide},
    identifiers::instrument_id::InstrumentId,
    orderbook::{book::BookIntegrityError, ladder::Ladder},
    types::{price::Price, quantity::Quantity},
};

/// Provides an order book which can handle MBO (market by order, a.k.a L3)
/// granularity data.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBookMbo {
    /// The instrument ID for the order book.
    pub instrument_id: InstrumentId,
    /// The last event sequence number for the order book.
    pub sequence: u64,
    /// The timestamp of the last event applied to the order book.
    pub ts_last: UnixNanos,
    /// The current count of events applied to the order book.
    pub count: u64,
    bids: Ladder,
    asks: Ladder,
}

impl OrderBookMbo {
    #[must_use]
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self {
            instrument_id,
            sequence: 0,
            ts_last: 0,
            count: 0,
            bids: Ladder::new(OrderSide::Buy),
            asks: Ladder::new(OrderSide::Sell),
        }
    }

    pub fn reset(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.sequence = 0;
        self.ts_last = 0;
        self.count = 0;
    }

    pub fn add(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match order.side {
            OrderSide::Buy => self.bids.add(order),
            OrderSide::Sell => self.asks.add(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn update(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match order.side {
            OrderSide::Buy => self.bids.update(order),
            OrderSide::Sell => self.asks.update(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match order.side {
            OrderSide::Buy => self.bids.delete(order, ts_event, sequence),
            OrderSide::Sell => self.asks.delete(order, ts_event, sequence),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn clear(&mut self, ts_event: u64, sequence: u64) {
        self.bids.clear();
        self.asks.clear();
        self.increment(ts_event, sequence);
    }

    pub fn clear_bids(&mut self, ts_event: u64, sequence: u64) {
        self.bids.clear();
        self.increment(ts_event, sequence);
    }

    pub fn clear_asks(&mut self, ts_event: u64, sequence: u64) {
        self.asks.clear();
        self.increment(ts_event, sequence);
    }

    pub fn apply_delta(&mut self, delta: OrderBookDelta) {
        match delta.action {
            BookAction::Add => self.add(delta.order, delta.ts_event, delta.sequence),
            BookAction::Update => self.update(delta.order, delta.ts_event, delta.sequence),
            BookAction::Delete => self.delete(delta.order, delta.ts_event, delta.sequence),
            BookAction::Clear => self.clear(delta.ts_event, delta.sequence),
        }
    }

    pub fn apply_deltas(&mut self, deltas: OrderBookDeltas) {
        for delta in deltas.deltas {
            self.apply_delta(delta);
        }
    }

    pub fn apply_depth(&mut self, depth: OrderBookDepth10) {
        self.bids.clear();
        self.asks.clear();

        for order in depth.bids {
            self.add(order, depth.ts_event, depth.sequence);
        }

        for order in depth.asks {
            self.add(order, depth.ts_event, depth.sequence);
        }
    }

    pub fn bids(&self) -> impl Iterator<Item = &Level> {
        self.bids.levels.values()
    }

    pub fn asks(&self) -> impl Iterator<Item = &Level> {
        self.asks.levels.values()
    }

    #[must_use]
    pub fn has_bid(&self) -> bool {
        match self.bids.top() {
            Some(top) => !top.orders.is_empty(),
            None => false,
        }
    }

    #[must_use]
    pub fn has_ask(&self) -> bool {
        match self.asks.top() {
            Some(top) => !top.orders.is_empty(),
            None => false,
        }
    }

    #[must_use]
    pub fn best_bid_price(&self) -> Option<Price> {
        self.bids.top().map(|top| top.price.value)
    }

    #[must_use]
    pub fn best_ask_price(&self) -> Option<Price> {
        self.asks.top().map(|top| top.price.value)
    }

    #[must_use]
    pub fn best_bid_size(&self) -> Option<Quantity> {
        match self.bids.top() {
            Some(top) => top.first().map(|order| order.size),
            None => None,
        }
    }

    #[must_use]
    pub fn best_ask_size(&self) -> Option<Quantity> {
        match self.asks.top() {
            Some(top) => top.first().map(|order| order.size),
            None => None,
        }
    }

    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (self.best_ask_price(), self.best_bid_price()) {
            (Some(ask), Some(bid)) => Some(ask.as_f64() - bid.as_f64()),
            _ => None,
        }
    }

    #[must_use]
    pub fn midpoint(&self) -> Option<f64> {
        match (self.best_ask_price(), self.best_bid_price()) {
            (Some(ask), Some(bid)) => Some((ask.as_f64() + bid.as_f64()) / 2.0),
            _ => None,
        }
    }

    #[must_use]
    pub fn get_avg_px_for_quantity(&self, qty: Quantity, order_side: OrderSide) -> f64 {
        let levels = match order_side {
            OrderSide::Buy => &self.asks.levels,
            OrderSide::Sell => &self.bids.levels,
            _ => panic!("Invalid `OrderSide` {order_side}"),
        };

        get_avg_px_for_quantity(qty, levels)
    }

    #[must_use]
    pub fn get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        let levels = match order_side {
            OrderSide::Buy => &self.asks.levels,
            OrderSide::Sell => &self.bids.levels,
            _ => panic!("Invalid `OrderSide` {order_side}"),
        };

        get_quantity_for_price(price, order_side, levels)
    }

    #[must_use]
    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        match order.side {
            OrderSide::Buy => self.asks.simulate_fills(order),
            OrderSide::Sell => self.bids.simulate_fills(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }

    /// Return a [`String`] representation of the order book in a human-readable table format.
    #[must_use]
    pub fn pprint(&self, num_levels: usize) -> String {
        pprint_book(&self.bids, &self.asks, num_levels)
    }

    pub fn check_integrity(&self) -> Result<(), BookIntegrityError> {
        let top_bid_level = self.bids.top();
        let top_ask_level = self.asks.top();

        if top_bid_level.is_none() || top_ask_level.is_none() {
            return Ok(());
        }

        // SAFETY: Levels were already checked for None
        let best_bid = top_bid_level.unwrap().price;
        let best_ask = top_ask_level.unwrap().price;

        if best_bid.value >= best_ask.value {
            return Err(BookIntegrityError::OrdersCrossed(best_bid, best_ask));
        }

        Ok(())
    }

    fn increment(&mut self, ts_event: u64, sequence: u64) {
        self.ts_last = ts_event;
        self.sequence = sequence;
        self.count += 1;
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::identifiers::instrument_id::InstrumentId;

    #[rstest]
    fn test_orderbook_creation() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let book = OrderBookMbo::new(instrument_id);

        assert_eq!(book.instrument_id, instrument_id);
        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[rstest]
    fn test_orderbook_reset() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBookMbo::new(instrument_id);
        book.sequence = 10;
        book.ts_last = 100;
        book.count = 3;

        book.reset();

        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[rstest]
    fn test_check_integrity_when_crossed() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBookMbo::new(instrument_id);

        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("2.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        book.add(bid1, 0, 1);
        book.add(ask1, 0, 1);

        assert!(book.check_integrity().is_err());
    }
}
