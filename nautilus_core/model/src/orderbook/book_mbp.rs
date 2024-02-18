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
        quote::QuoteTick, trade::TradeTick,
    },
    enums::{BookAction, OrderSide},
    identifiers::instrument_id::InstrumentId,
    orderbook::{book::BookIntegrityError, ladder::Ladder},
    types::{price::Price, quantity::Quantity},
};

/// Provides an order book which can handle MBP/L2 or L1 (top only) granularity data.
pub struct OrderBookMbp {
    pub instrument_id: InstrumentId,
    pub top_only: bool,
    pub sequence: u64,
    pub ts_last: UnixNanos,
    pub count: u64,
    bids: Ladder,
    asks: Ladder,
}

impl OrderBookMbp {
    #[must_use]
    pub fn new(instrument_id: InstrumentId, top_only: bool) -> Self {
        Self {
            instrument_id,
            top_only,
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
        let order = self.pre_process_order(order);

        match order.side {
            OrderSide::Buy => self.bids.add(order),
            OrderSide::Sell => self.asks.add(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn update(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        if self.top_only {
            self.update_top(order, ts_event, sequence);
        }
        let order = self.pre_process_order(order);

        match order.side {
            OrderSide::Buy => self.bids.update(order),
            OrderSide::Sell => self.asks.update(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn update_quote_tick(&mut self, quote: &QuoteTick) {
        self.update_bid(
            BookOrder::from_quote_tick(quote, OrderSide::Buy),
            quote.ts_event,
            0,
        );
        self.update_ask(
            BookOrder::from_quote_tick(quote, OrderSide::Sell),
            quote.ts_event,
            0,
        );
    }

    pub fn update_trade_tick(&mut self, trade: &TradeTick) {
        self.update_bid(
            BookOrder::from_trade_tick(trade, OrderSide::Buy),
            trade.ts_event,
            0,
        );
        self.update_ask(
            BookOrder::from_trade_tick(trade, OrderSide::Sell),
            trade.ts_event,
            0,
        );
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        let order = self.pre_process_order(order);

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
            self.apply_delta(delta)
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

    pub fn bids(&self) -> Vec<&Level> {
        self.bids.levels.values().collect()
    }

    pub fn asks(&self) -> Vec<&Level> {
        self.asks.levels.values().collect()
    }

    pub fn has_bid(&self) -> bool {
        match self.bids.top() {
            Some(top) => !top.orders.is_empty(),
            None => false,
        }
    }

    pub fn has_ask(&self) -> bool {
        match self.asks.top() {
            Some(top) => !top.orders.is_empty(),
            None => false,
        }
    }

    pub fn best_bid_price(&self) -> Option<Price> {
        self.bids.top().map(|top| top.price.value)
    }

    pub fn best_ask_price(&self) -> Option<Price> {
        self.asks.top().map(|top| top.price.value)
    }

    pub fn best_bid_size(&self) -> Option<Quantity> {
        match self.bids.top() {
            Some(top) => top.first().map(|order| order.size),
            None => None,
        }
    }

    pub fn best_ask_size(&self) -> Option<Quantity> {
        match self.asks.top() {
            Some(top) => top.first().map(|order| order.size),
            None => None,
        }
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.best_ask_price(), self.best_bid_price()) {
            (Some(ask), Some(bid)) => Some(ask.as_f64() - bid.as_f64()),
            _ => None,
        }
    }

    pub fn midpoint(&self) -> Option<f64> {
        match (self.best_ask_price(), self.best_bid_price()) {
            (Some(ask), Some(bid)) => Some((ask.as_f64() + bid.as_f64()) / 2.0),
            _ => None,
        }
    }

    pub fn get_avg_px_for_quantity(&self, qty: Quantity, order_side: OrderSide) -> f64 {
        let levels = match order_side {
            OrderSide::Buy => &self.asks.levels,
            OrderSide::Sell => &self.bids.levels,
            _ => panic!("Invalid `OrderSide` {}", order_side),
        };

        get_avg_px_for_quantity(qty, levels)
    }

    pub fn get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        let levels = match order_side {
            OrderSide::Buy => &self.asks.levels,
            OrderSide::Sell => &self.bids.levels,
            _ => panic!("Invalid `OrderSide` {}", order_side),
        };

        get_quantity_for_price(price, order_side, levels)
    }

    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        match order.side {
            OrderSide::Buy => self.asks.simulate_fills(order),
            OrderSide::Sell => self.bids.simulate_fills(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }

    /// Return a [`String`] representation of the order book in a human-readable table format.
    pub fn pprint(&self, num_levels: usize) -> String {
        pprint_book(&self.bids, &self.asks, num_levels)
    }

    pub fn check_integrity(&self) -> Result<(), BookIntegrityError> {
        match self.top_only {
            true => {
                if self.bids.len() > 1 {
                    return Err(BookIntegrityError::TooManyLevels(
                        OrderSide::Buy,
                        self.bids.len(),
                    ));
                }
                if self.asks.len() > 1 {
                    return Err(BookIntegrityError::TooManyLevels(
                        OrderSide::Sell,
                        self.asks.len(),
                    ));
                }
            }
            false => {
                for (_, bid_level) in self.bids.levels.iter() {
                    let num_orders = bid_level.orders.len();
                    if num_orders > 1 {
                        return Err(BookIntegrityError::TooManyOrders(
                            OrderSide::Buy,
                            num_orders,
                        ));
                    }
                }

                for (_, ask_level) in self.asks.levels.iter() {
                    let num_orders = ask_level.orders.len();
                    if num_orders > 1 {
                        return Err(BookIntegrityError::TooManyOrders(
                            OrderSide::Sell,
                            num_orders,
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    fn increment(&mut self, ts_event: u64, sequence: u64) {
        self.ts_last = ts_event;
        self.sequence = sequence;
        self.count += 1;
    }

    fn update_bid(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match self.bids.top() {
            Some(top_bids) => match top_bids.first() {
                Some(top_bid) => {
                    let order_id = top_bid.order_id;
                    self.bids.remove(order_id, ts_event, sequence);
                    self.bids.add(order);
                }
                None => {
                    self.bids.add(order);
                }
            },
            None => {
                self.bids.add(order);
            }
        }
    }

    fn update_ask(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        match self.asks.top() {
            Some(top_asks) => match top_asks.first() {
                Some(top_ask) => {
                    let order_id = top_ask.order_id;
                    self.asks.remove(order_id, ts_event, sequence);
                    self.asks.add(order);
                }
                None => {
                    self.asks.add(order);
                }
            },
            None => {
                self.asks.add(order);
            }
        }
    }

    fn update_top(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        // Because of the way we typically get updates from a L1_MBP order book (bid
        // and ask updates at the same time), its quite probable that the last
        // bid is now the ask price we are trying to insert (or vice versa). We
        // just need to add some extra protection against this if we aren't calling
        // `check_integrity()` on each individual update.
        match order.side {
            OrderSide::Buy => {
                if let Some(best_ask_price) = self.best_ask_price() {
                    if order.price > best_ask_price {
                        self.clear_bids(ts_event, sequence);
                    }
                }
            }
            OrderSide::Sell => {
                if let Some(best_bid_price) = self.best_bid_price() {
                    if order.price < best_bid_price {
                        self.clear_asks(ts_event, sequence);
                    }
                }
            }
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }

    fn pre_process_order(&self, mut order: BookOrder) -> BookOrder {
        match self.top_only {
            true => order.order_id = order.side as u64,
            false => order.order_id = order.price.raw as u64,
        };
        order
    }
}
