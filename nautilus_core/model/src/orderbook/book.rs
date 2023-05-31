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

use tabled::settings::Style;
use tabled::{Table, Tabled};
use thiserror::Error;

use crate::data::book::{BookOrder, OrderBookDelta};
use crate::data::tick::{QuoteTick, TradeTick};
use crate::enums::{BookAction, BookType, OrderSide};
use crate::identifiers::instrument_id::InstrumentId;
use crate::orderbook::ladder::Ladder;
use crate::types::price::Price;
use crate::types::quantity::Quantity;

use super::ladder::BookPrice;
use super::level::Level;

pub struct OrderBook {
    bids: Ladder,
    asks: Ladder,
    pub instrument_id: InstrumentId,
    pub book_type: BookType,
    pub sequence: u64,
    pub ts_last: u64,
    pub count: u64,
}

#[derive(Error, Debug)]
pub enum BookIntegrityError {
    #[error("Orders in cross [{0} @ {1}]")]
    OrdersCrossed(BookPrice, BookPrice),
    #[error("Integrity check failed for L2_MBP book: number of {0} levels > 1, was {1}")]
    TooManyLevels(OrderSide, usize),
    #[error("Integrity check failed for L1_TBBO book: number of {0} orders levels > 1, was {1}")]
    TooManyOrders(OrderSide, usize),
    #[error("Invalid `OrderSide::NoOrderSide` in book")]
    NoOrderSide,
}

#[derive(Tabled)]
struct OrderLevelDisplay {
    bids: String,
    price: String,
    asks: String,
}

impl OrderBook {
    #[must_use]
    pub fn new(instrument_id: InstrumentId, book_level: BookType) -> Self {
        Self {
            bids: Ladder::new(OrderSide::Buy),
            asks: Ladder::new(OrderSide::Sell),
            instrument_id,
            book_type: book_level,
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
            BookType::L1_TBBO => panic!("Invalid book operation: call `update` for a L1_TBBO book"),
        };

        match order.side {
            OrderSide::Buy => self.bids.add(order),
            OrderSide::Sell => self.asks.add(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.sequence = sequence;
        self.ts_last = ts_event;
        self.count += 1;
    }

    pub fn update(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        let order = match self.book_type {
            BookType::L3_MBO => order, // No order pre-processing
            BookType::L2_MBP => self.pre_process_order(order),
            BookType::L1_TBBO => {
                self.update_l1(order, ts_event, sequence);
                self.pre_process_order(order)
            }
        };

        match order.side {
            OrderSide::Buy => self.bids.update(order),
            OrderSide::Sell => self.asks.update(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.sequence = sequence;
        self.ts_last = ts_event;
        self.count += 1;
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        let order = match self.book_type {
            BookType::L3_MBO => order, // No order pre-processing
            BookType::L2_MBP => self.pre_process_order(order),
            BookType::L1_TBBO => self.pre_process_order(order),
        };

        match order.side {
            OrderSide::Buy => self.bids.delete(order),
            OrderSide::Sell => self.asks.delete(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.sequence = sequence;
        self.ts_last = ts_event;
        self.count += 1;
    }

    pub fn clear(&mut self, ts_event: u64, sequence: u64) {
        self.bids.clear();
        self.asks.clear();
        self.sequence = sequence;
        self.ts_last = ts_event;
        self.count += 1;
    }

    pub fn clear_bids(&mut self, ts_event: u64, sequence: u64) {
        self.bids.clear();
        self.sequence = sequence;
        self.ts_last = ts_event;
        self.count += 1;
    }

    pub fn clear_asks(&mut self, ts_event: u64, sequence: u64) {
        self.asks.clear();
        self.sequence = sequence;
        self.ts_last = ts_event;
        self.count += 1;
    }

    pub fn apply_delta(&mut self, delta: OrderBookDelta) {
        match delta.action {
            BookAction::Add => self.add(delta.order, delta.ts_event, delta.sequence),
            BookAction::Update => self.update(delta.order, delta.ts_event, delta.sequence),
            BookAction::Delete => self.delete(delta.order, delta.ts_event, delta.sequence),
            BookAction::Clear => self.clear(delta.ts_event, delta.sequence),
        }
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
            Some(top) => top.orders.first().map(|order| order.size),
            None => None,
        }
    }

    pub fn best_ask_size(&self) -> Option<Quantity> {
        match self.asks.top() {
            Some(top) => top.orders.first().map(|order| order.size),
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

    pub fn update_quote_tick(&mut self, tick: &QuoteTick) {
        self.update_bid(BookOrder::from_quote_tick(tick, OrderSide::Buy));
        self.update_ask(BookOrder::from_quote_tick(tick, OrderSide::Sell));
    }

    pub fn update_trade_tick(&mut self, tick: &TradeTick) {
        self.update_bid(BookOrder::new(
            OrderSide::Buy,
            tick.price,
            tick.size,
            tick.price.raw as u64,
        ));
        self.update_ask(BookOrder::new(
            OrderSide::Sell,
            tick.price,
            tick.size,
            tick.price.raw as u64,
        ));
    }

    pub fn check_integrity(&self) -> Result<(), BookIntegrityError> {
        match self.book_type {
            BookType::L1_TBBO => {
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
            BookType::L2_MBP => {
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
            BookType::L3_MBO => {
                let top_bid_level = self.bids.top();
                let top_ask_level = self.asks.top();

                if top_bid_level.is_none() || top_ask_level.is_none() {
                    return Ok(());
                }

                let best_bid = top_bid_level.unwrap().price;
                let best_ask = top_ask_level.unwrap().price;

                if best_bid >= best_ask {
                    return Err(BookIntegrityError::OrdersCrossed(best_bid, best_ask));
                }
            }
        }
        Ok(())
    }

    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        match order.side {
            OrderSide::Buy => self.asks.simulate_fills(order),
            OrderSide::Sell => self.bids.simulate_fills(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }

    pub fn pprint(&self, num_levels: usize) -> String {
        let mut ask_levels: Vec<(&BookPrice, &Level)> =
            self.asks.levels.iter().take(num_levels).collect();

        let bid_levels: Vec<(&BookPrice, &Level)> =
            self.bids.levels.iter().take(num_levels).collect();

        ask_levels.reverse();

        let levels: Vec<(&BookPrice, &Level)> = ask_levels
            .into_iter()
            .chain(bid_levels.into_iter())
            .collect();

        let data: Vec<OrderLevelDisplay> = levels
            .iter()
            .map(|(_, level)| {
                let bid_sizes: Vec<String> = level
                    .orders
                    .iter()
                    .filter(|order| self.bids.levels.contains_key(&order.to_book_price()))
                    .map(|order| format!("{}", order.size))
                    .collect();

                let ask_sizes: Vec<String> = level
                    .orders
                    .iter()
                    .filter(|order| self.asks.levels.contains_key(&order.to_book_price()))
                    .map(|order| format!("{}", order.size))
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

    fn update_l1(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        // Because of the way we typically get updates from a L1 order book (bid
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

    fn update_bid(&mut self, order: BookOrder) {
        match self.bids.top() {
            Some(top_bids) => match top_bids.orders.first() {
                Some(top_bid) => {
                    let order_id = top_bid.order_id;
                    self.bids.remove(order_id);
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

    fn update_ask(&mut self, order: BookOrder) {
        match self.asks.top() {
            Some(top_asks) => match top_asks.orders.first() {
                Some(top_ask) => {
                    let order_id = top_ask.order_id;
                    self.asks.remove(order_id);
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
            // Because a L1_TBBO book only has one level per side, we replace the
            // `order.order_id` with the enum value of the side, which will let us easily process
            // the order.
            BookType::L1_TBBO => order.order_id = order.side as u64,
            // Because a L2OrderBook only has one order per level, we replace the
            // `order.order_id` with a raw price value, which will let us easily process the order.
            BookType::L2_MBP => order.order_id = order.price.raw as u64,
            BookType::L3_MBO => panic!("Invalid to process an order for a L3_MBO book"),
        }

        order
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::data::book::BookOrder;
    use crate::enums::{AggressorSide, OrderSide};
    use crate::identifiers::instrument_id::InstrumentId;
    use crate::identifiers::trade_id::TradeId;
    use crate::types::price::Price;
    use crate::types::quantity::Quantity;

    fn create_stub_book() -> OrderBook {
        let instrument_id = InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap();
        OrderBook::new(instrument_id, BookType::L2_MBP)
    }

    #[test]
    fn test_orderbook_creation() {
        let instrument_id = InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap();
        let book = OrderBook::new(instrument_id.clone(), BookType::L2_MBP);

        assert_eq!(book.instrument_id, instrument_id);
        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[test]
    fn test_orderbook_reset() {
        let mut book = create_stub_book();
        book.sequence = 10;
        book.ts_last = 100;
        book.count = 3;

        book.reset();

        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[test]
    fn test_best_bid_and_ask_when_nothing_in_book() {
        let book = create_stub_book();

        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_ask_price(), None);
        assert_eq!(book.best_bid_size(), None);
        assert_eq!(book.best_ask_size(), None);
        assert_eq!(book.has_bid(), false);
        assert_eq!(book.has_ask(), false);
    }

    #[test]
    fn test_bid_side_with_one_order() {
        let mut book = create_stub_book();

        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        book.add(order1, 100, 1);

        assert_eq!(book.best_bid_price(), Some(Price::from("1.000")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("1.0")));
        assert_eq!(book.has_bid(), true);
    }

    #[test]
    fn test_ask_side_with_one_order() {
        let mut book = create_stub_book();

        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(order, 200, 2);

        assert_eq!(book.best_ask_price(), Some(Price::from("2.000")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("2.0")));
        assert_eq!(book.has_ask(), true);
    }
    #[test]
    fn test_spread_with_no_bids_or_asks() {
        let book = create_stub_book();
        assert_eq!(book.spread(), None);
    }

    #[test]
    fn test_spread_with_bids_and_asks() {
        let mut book = create_stub_book();

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
        book.add(bid1.clone(), 100, 1);
        book.add(ask1.clone(), 200, 2);

        assert_eq!(book.spread(), Some(1.0));
    }

    #[test]
    fn test_midpoint_with_no_bids_or_asks() {
        let book = create_stub_book();
        assert_eq!(book.midpoint(), None);
    }

    #[test]
    fn test_midpoint_with_bids_asks() {
        let instrument_id = InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap();
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
        book.add(bid1.clone(), 100, 1);
        book.add(ask1.clone(), 200, 2);

        assert_eq!(book.midpoint(), Some(1.5));
    }

    #[test]
    fn test_update_quote_tick_bid() {
        let instrument_id = InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap();
        let mut order_book = OrderBook::new(instrument_id.clone(), BookType::L1_TBBO);
        let tick = QuoteTick::new(
            InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            Price::new(5000.0, 3),
            Price::new(5100.0, 3),
            Quantity::new(100.0, 8),
            Quantity::new(99.0, 8),
            0,
            0,
        );

        order_book.update_quote_tick(&tick);

        // Check if the top bid order in order_book is the same as the one created from tick
        let top_bid_order = order_book.bids.top().unwrap().orders.first().unwrap();
        let expected_bid_order = BookOrder::from_quote_tick(&tick, OrderSide::Buy);
        assert_eq!(*top_bid_order, expected_bid_order);
    }

    #[test]
    fn test_update_quote_tick_ask() {
        let instrument_id = InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap();
        let mut book = OrderBook::new(instrument_id.clone(), BookType::L1_TBBO);
        let tick = QuoteTick::new(
            instrument_id,
            Price::new(5000.0, 3),
            Price::new(5100.0, 3),
            Quantity::new(100.0, 8),
            Quantity::new(99.0, 8),
            0,
            0,
        );

        book.update_quote_tick(&tick);

        // Check if the top ask order in order_book is the same as the one created from tick
        let top_ask_order = book.asks.top().unwrap().orders.first().unwrap();
        let expected_ask_order = BookOrder::from_quote_tick(&tick, OrderSide::Sell);
        assert_eq!(*top_ask_order, expected_ask_order);
    }

    #[test]
    fn test_update_trade_tick() {
        let instrument_id = InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap();
        let mut book = OrderBook::new(instrument_id.clone(), BookType::L1_TBBO);

        let price = Price::new(15_000.0, 3);
        let size = Quantity::new(10.0, 8);
        let trade_tick = TradeTick::new(
            instrument_id,
            price,
            size,
            AggressorSide::Buyer,
            TradeId::new("123456789"),
            0,
            0,
        );

        book.update_trade_tick(&trade_tick);

        assert_eq!(book.best_bid_price().unwrap(), price);
        assert_eq!(book.best_ask_price().unwrap(), price);
        assert_eq!(book.best_bid_size().unwrap(), size);
        assert_eq!(book.best_ask_size().unwrap(), size);
    }

    #[test]
    fn test_pprint() {
        let mut book = create_stub_book();

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
