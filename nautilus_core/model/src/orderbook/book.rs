// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
use tabled::{settings::Style, Table, Tabled};
use thiserror::Error;

use super::{ladder::BookPrice, level::Level};
use crate::{
    data::{delta::OrderBookDelta, order::BookOrder, quote::QuoteTick, trade::TradeTick},
    enums::{BookAction, BookType, OrderSide},
    identifiers::instrument_id::InstrumentId,
    orderbook::ladder::Ladder,
    types::{price::Price, quantity::Quantity},
};

#[derive(thiserror::Error, Debug)]
pub enum InvalidBookOperation {
    #[error("Invalid book operation: cannot pre-process order for {0} book")]
    PreProcessOrder(BookType),
    #[error("Invalid book operation: cannot add for {0} book")]
    Add(BookType),
}

#[derive(Error, Debug)]
pub enum BookIntegrityError {
    #[error("Integrity error: order not found: order_id={0}, ts_event={1}, sequence={2}")]
    OrderNotFound(u64, u64, u64),
    #[error("Integrity error: invalid `NoOrderSide` in book")]
    NoOrderSide,
    #[error("Integrity error: orders in cross [{0} @ {1}]")]
    OrdersCrossed(BookPrice, BookPrice),
    #[error("Integrity error: number of {0} orders at level > 1 for L2_MBP book, was {1}")]
    TooManyOrders(OrderSide, usize),
    #[error("Integrity error: number of {0} levels > 1 for L1_MBP book, was {1}")]
    TooManyLevels(OrderSide, usize),
}

#[derive(Tabled)]
struct OrderLevelDisplay {
    bids: String,
    price: String,
    asks: String,
}

/// Provides an order book which can handle L1/L2/L3 granularity data.
pub struct OrderBook {
    bids: Ladder,
    asks: Ladder,
    pub instrument_id: InstrumentId,
    pub book_type: BookType,
    pub sequence: u64,
    pub ts_last: UnixNanos,
    pub count: u64,
}

impl OrderBook {
    #[must_use]
    pub fn new(instrument_id: InstrumentId, book_type: BookType) -> Self {
        Self {
            bids: Ladder::new(OrderSide::Buy),
            asks: Ladder::new(OrderSide::Sell),
            instrument_id,
            book_type,
            sequence: 0,
            ts_last: 0,
            count: 0,
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
        let order = match self.book_type {
            BookType::L3_MBO => order, // No order pre-processing
            BookType::L2_MBP => self.pre_process_order(order),
            BookType::L1_MBP => panic!("{}", InvalidBookOperation::Add(self.book_type)),
        };

        match order.side {
            OrderSide::Buy => self.bids.add(order),
            OrderSide::Sell => self.asks.add(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn update(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        let order = match self.book_type {
            BookType::L3_MBO => order, // No order pre-processing
            BookType::L2_MBP => self.pre_process_order(order),
            BookType::L1_MBP => {
                self.update_l1(order, ts_event, sequence);
                self.pre_process_order(order)
            }
        };

        match order.side {
            OrderSide::Buy => self.bids.update(order),
            OrderSide::Sell => self.asks.update(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        let order = match self.book_type {
            BookType::L3_MBO => order, // No order pre-processing
            BookType::L2_MBP => self.pre_process_order(order),
            BookType::L1_MBP => self.pre_process_order(order),
        };

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
            OrderSide::Buy => self.asks.levels.iter(),
            OrderSide::Sell => self.bids.levels.iter(),
            _ => panic!("Invalid `OrderSide` {}", order_side),
        };
        let mut cumulative_size_raw = 0u64;
        let mut cumulative_value = 0.0;

        for (book_price, level) in levels {
            let size_this_level = level.size_raw().min(qty.raw - cumulative_size_raw);
            cumulative_size_raw += size_this_level;
            cumulative_value += book_price.value.as_f64() * size_this_level as f64;

            if cumulative_size_raw >= qty.raw {
                break;
            }
        }

        if cumulative_size_raw == 0 {
            0.0
        } else {
            cumulative_value / cumulative_size_raw as f64
        }
    }

    pub fn get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        let levels = match order_side {
            OrderSide::Buy => self.asks.levels.iter(),
            OrderSide::Sell => self.bids.levels.iter(),
            _ => panic!("Invalid `OrderSide` {}", order_side),
        };

        let mut matched_size: f64 = 0.0;

        for (book_price, level) in levels {
            match order_side {
                OrderSide::Buy => {
                    if book_price.value > price {
                        break;
                    }
                }
                OrderSide::Sell => {
                    if book_price.value < price {
                        break;
                    }
                }
                _ => panic!("Invalid `OrderSide` {}", order_side),
            }
            matched_size += level.size();
        }

        matched_size
    }

    pub fn update_quote_tick(&mut self, tick: &QuoteTick) {
        self.update_bid(
            BookOrder::from_quote_tick(tick, OrderSide::Buy),
            tick.ts_event,
            0,
        );
        self.update_ask(
            BookOrder::from_quote_tick(tick, OrderSide::Sell),
            tick.ts_event,
            0,
        );
    }

    pub fn update_trade_tick(&mut self, tick: &TradeTick) {
        self.update_bid(
            BookOrder::from_trade_tick(tick, OrderSide::Buy),
            tick.ts_event,
            0,
        );
        self.update_ask(
            BookOrder::from_trade_tick(tick, OrderSide::Sell),
            tick.ts_event,
            0,
        );
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
        let ask_levels: Vec<(&BookPrice, &Level)> =
            self.asks.levels.iter().take(num_levels).rev().collect();
        let bid_levels: Vec<(&BookPrice, &Level)> =
            self.bids.levels.iter().take(num_levels).collect();
        let levels: Vec<(&BookPrice, &Level)> = ask_levels.into_iter().chain(bid_levels).collect();

        let data: Vec<OrderLevelDisplay> = levels
            .iter()
            .map(|(book_price, level)| {
                let is_bid_level = self.bids.levels.contains_key(book_price);
                let is_ask_level = self.asks.levels.contains_key(book_price);

                let bid_sizes: Vec<String> = level
                    .orders
                    .iter()
                    .filter(|_| is_bid_level)
                    .map(|order| format!("{}", order.1.size))
                    .collect();

                let ask_sizes: Vec<String> = level
                    .orders
                    .iter()
                    .filter(|_| is_ask_level)
                    .map(|order| format!("{}", order.1.size))
                    .collect();

                OrderLevelDisplay {
                    bids: if bid_sizes.is_empty() {
                        String::from("")
                    } else {
                        format!("[{}]", bid_sizes.join(", "))
                    },
                    price: format!("{}", level.price),
                    asks: if ask_sizes.is_empty() {
                        String::from("")
                    } else {
                        format!("[{}]", ask_sizes.join(", "))
                    },
                }
            })
            .collect();

        Table::new(data).with(Style::rounded()).to_string()
    }

    pub fn check_integrity(&self) -> Result<(), BookIntegrityError> {
        match self.book_type {
            BookType::L3_MBO => self.check_integrity_l3(),
            BookType::L2_MBP => self.check_integrity_l2(),
            BookType::L1_MBP => self.check_integrity_l1(),
        }
    }

    fn check_integrity_l3(&self) -> Result<(), BookIntegrityError> {
        let top_bid_level = self.bids.top();
        let top_ask_level = self.asks.top();

        if top_bid_level.is_none() || top_ask_level.is_none() {
            return Ok(());
        }

        // SAFETY: Levels were already checked for None
        let best_bid = top_bid_level.unwrap().price;
        let best_ask = top_ask_level.unwrap().price;

        if best_bid >= best_ask {
            return Err(BookIntegrityError::OrdersCrossed(best_bid, best_ask));
        }

        Ok(())
    }

    fn check_integrity_l2(&self) -> Result<(), BookIntegrityError> {
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

        Ok(())
    }

    fn check_integrity_l1(&self) -> Result<(), BookIntegrityError> {
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

        Ok(())
    }

    fn increment(&mut self, ts_event: u64, sequence: u64) {
        self.ts_last = ts_event;
        self.sequence = sequence;
        self.count += 1;
    }

    fn update_l1(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
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

    fn pre_process_order(&self, mut order: BookOrder) -> BookOrder {
        match self.book_type {
            // Because a L1_MBP only has one level per side, we replace the
            // `order.order_id` with the enum value of the side, which will let us easily process
            // the order.
            BookType::L1_MBP => order.order_id = order.side as u64,
            // Because a L2_MBP only has one order per level, we replace the
            // `order.order_id` with a raw price value, which will let us easily process the order.
            BookType::L2_MBP => order.order_id = order.price.raw as u64,
            BookType::L3_MBO => panic!("{}", InvalidBookOperation::PreProcessOrder(self.book_type)),
        }

        order
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        data::order::BookOrder,
        enums::{AggressorSide, OrderSide},
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
        types::{price::Price, quantity::Quantity},
    };

    fn create_stub_book(book_type: BookType) -> OrderBook {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        OrderBook::new(instrument_id, book_type)
    }

    #[rstest]
    fn test_orderbook_creation() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);

        assert_eq!(book.instrument_id, instrument_id);
        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[rstest]
    fn test_orderbook_reset() {
        let mut book = create_stub_book(BookType::L2_MBP);
        book.sequence = 10;
        book.ts_last = 100;
        book.count = 3;

        book.reset();

        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[rstest]
    fn test_best_bid_and_ask_when_nothing_in_book() {
        let book = create_stub_book(BookType::L2_MBP);

        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_ask_price(), None);
        assert_eq!(book.best_bid_size(), None);
        assert_eq!(book.best_ask_size(), None);
        assert!(!book.has_bid());
        assert!(!book.has_ask());
    }

    #[rstest]
    fn test_bid_side_with_one_order() {
        let mut book = create_stub_book(BookType::L3_MBO);
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        book.add(order1, 100, 1);

        assert_eq!(book.best_bid_price(), Some(Price::from("1.000")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("1.0")));
        assert!(book.has_bid());
    }

    #[rstest]
    fn test_ask_side_with_one_order() {
        let mut book = create_stub_book(BookType::L3_MBO);
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(order, 200, 2);

        assert_eq!(book.best_ask_price(), Some(Price::from("2.000")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("2.0")));
        assert!(book.has_ask());
    }
    #[rstest]
    fn test_spread_with_no_bids_or_asks() {
        let book = create_stub_book(BookType::L3_MBO);
        assert_eq!(book.spread(), None);
    }

    #[rstest]
    fn test_spread_with_bids_and_asks() {
        let mut book = create_stub_book(BookType::L2_MBP);
        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(bid1, 100, 1);
        book.add(ask1, 200, 2);

        assert_eq!(book.spread(), Some(1.0));
    }

    #[rstest]
    fn test_midpoint_with_no_bids_or_asks() {
        let book = create_stub_book(BookType::L2_MBP);
        assert_eq!(book.midpoint(), None);
    }

    #[rstest]
    fn test_midpoint_with_bids_asks() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(bid1, 100, 1);
        book.add(ask1, 200, 2);

        assert_eq!(book.midpoint(), Some(1.5));
    }

    #[rstest]
    fn test_get_price_for_quantity_no_market() {
        let book = create_stub_book(BookType::L2_MBP);
        let qty = Quantity::from(1);

        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Buy), 0.0);
        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Sell), 0.0);
    }

    #[rstest]
    fn test_get_quantity_for_price_no_market() {
        let book = create_stub_book(BookType::L2_MBP);
        let price = Price::from("1.0");

        assert_eq!(book.get_quantity_for_price(price, OrderSide::Buy), 0.0);
        assert_eq!(book.get_quantity_for_price(price, OrderSide::Sell), 0.0);
    }

    #[rstest]
    fn test_get_price_for_quantity() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let ask2 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.010"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("0.990"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        book.add(bid1, 0, 1);
        book.add(bid2, 0, 1);
        book.add(ask1, 0, 1);
        book.add(ask2, 0, 1);

        let qty = Quantity::from("1.5");

        assert_eq!(
            book.get_avg_px_for_quantity(qty, OrderSide::Buy),
            2.0033333333333334
        );
        assert_eq!(
            book.get_avg_px_for_quantity(qty, OrderSide::Sell),
            0.9966666666666667
        );
    }

    #[rstest]
    fn test_get_quantity_for_price() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let ask3 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.011"),
            Quantity::from("3.0"),
            0, // order_id not applicable
        );
        let ask2 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.010"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        let ask1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            0, // order_id not applicable
        );
        let bid2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("0.990"),
            Quantity::from("2.0"),
            0, // order_id not applicable
        );
        let bid3 = BookOrder::new(
            OrderSide::Buy,
            Price::from("0.989"),
            Quantity::from("3.0"),
            0, // order_id not applicable
        );
        book.add(bid1, 0, 1);
        book.add(bid2, 0, 1);
        book.add(bid3, 0, 1);
        book.add(ask1, 0, 1);
        book.add(ask2, 0, 1);
        book.add(ask3, 0, 1);

        assert_eq!(
            book.get_quantity_for_price(Price::from("2.010"), OrderSide::Buy),
            3.0
        );
        assert_eq!(
            book.get_quantity_for_price(Price::from("0.990"), OrderSide::Sell),
            3.0
        );
    }

    #[rstest]
    fn test_update_quote_tick_l1() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);
        let tick = QuoteTick::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Price::from("5000.000"),
            Price::from("5100.000"),
            Quantity::from("100.00000000"),
            Quantity::from("99.00000000"),
            0,
            0,
        )
        .unwrap();

        book.update_quote_tick(&tick);

        // Check if the top bid order in order_book is the same as the one created from tick
        let top_bid_order = book.bids.top().unwrap().first().unwrap();
        let top_ask_order = book.asks.top().unwrap().first().unwrap();
        let expected_bid_order = BookOrder::from_quote_tick(&tick, OrderSide::Buy);
        let expected_ask_order = BookOrder::from_quote_tick(&tick, OrderSide::Sell);
        assert_eq!(*top_bid_order, expected_bid_order);
        assert_eq!(*top_ask_order, expected_ask_order);
    }

    #[rstest]
    fn test_update_trade_tick_l1() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);

        let price = Price::from("15000.000");
        let size = Quantity::from("10.00000000");
        let trade_tick = TradeTick::new(
            instrument_id,
            price,
            size,
            AggressorSide::Buyer,
            TradeId::new("123456789").unwrap(),
            0,
            0,
        );

        book.update_trade_tick(&trade_tick);

        assert_eq!(book.best_bid_price().unwrap(), price);
        assert_eq!(book.best_ask_price().unwrap(), price);
        assert_eq!(book.best_bid_size().unwrap(), size);
        assert_eq!(book.best_ask_size().unwrap(), size);
    }

    #[rstest]
    fn test_pprint() {
        let mut book = create_stub_book(BookType::L3_MBO);
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        let order2 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.500"),
            Quantity::from("2.0"),
            2,
        );
        let order3 = BookOrder::new(
            OrderSide::Buy,
            Price::from("2.000"),
            Quantity::from("3.0"),
            3,
        );
        let order4 = BookOrder::new(
            OrderSide::Sell,
            Price::from("3.000"),
            Quantity::from("3.0"),
            4,
        );
        let order5 = BookOrder::new(
            OrderSide::Sell,
            Price::from("4.000"),
            Quantity::from("4.0"),
            5,
        );
        let order6 = BookOrder::new(
            OrderSide::Sell,
            Price::from("5.000"),
            Quantity::from("8.0"),
            6,
        );

        book.add(order1, 100, 1);
        book.add(order2, 200, 2);
        book.add(order3, 300, 3);
        book.add(order4, 400, 4);
        book.add(order5, 500, 5);
        book.add(order6, 600, 6);

        let pprint_output = book.pprint(3);

        let expected_output = "╭───────┬───────┬───────╮\n\
                               │ bids  │ price │ asks  │\n\
                               ├───────┼───────┼───────┤\n\
                               │       │ 5.000 │ [8.0] │\n\
                               │       │ 4.000 │ [4.0] │\n\
                               │       │ 3.000 │ [3.0] │\n\
                               │ [3.0] │ 2.000 │       │\n\
                               │ [2.0] │ 1.500 │       │\n\
                               │ [1.0] │ 1.000 │       │\n\
                               ╰───────┴───────┴───────╯";

        println!("{}", pprint_output);
        assert_eq!(pprint_output, expected_output);
    }
}
