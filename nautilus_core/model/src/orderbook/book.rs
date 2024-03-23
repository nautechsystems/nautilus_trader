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

use super::{aggregation::pre_process_order, analysis, display::pprint_book, level::Level};
use crate::{
    data::{
        delta::OrderBookDelta, deltas::OrderBookDeltas, depth::OrderBookDepth10, order::BookOrder,
    },
    enums::{BookAction, BookType, OrderSide},
    identifiers::instrument_id::InstrumentId,
    orderbook::{error::BookIntegrityError, ladder::Ladder},
    types::{price::Price, quantity::Quantity},
};

/// Provides an order book which can handle both MBO (market by order / L3)
/// and MBP (market by price / L2) granularity data.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBook {
    /// The order book type (MBP types will aggregate orders).
    pub book_type: BookType,
    /// The instrument ID for the order book.
    pub instrument_id: InstrumentId,
    /// The last event sequence number for the order book.
    pub sequence: u64,
    /// The timestamp of the last event applied to the order book.
    pub ts_last: UnixNanos,
    /// The current count of events applied to the order book.
    pub count: u64,
    pub(crate) bids: Ladder,
    pub(crate) asks: Ladder,
}

impl OrderBook {
    #[must_use]
    pub fn new(book_type: BookType, instrument_id: InstrumentId) -> Self {
        Self {
            book_type,
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
        let order = pre_process_order(self.book_type, order);
        match order.side {
            OrderSide::Buy => self.bids.add(order),
            OrderSide::Sell => self.asks.add(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn update(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        let order = pre_process_order(self.book_type, order);
        match order.side {
            OrderSide::Buy => self.bids.update(order),
            OrderSide::Sell => self.asks.update(order),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }

        self.increment(ts_event, sequence);
    }

    pub fn delete(&mut self, order: BookOrder, ts_event: u64, sequence: u64) {
        let order = pre_process_order(self.book_type, order);
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
        self.bids.top().map_or(false, |top| !top.orders.is_empty())
    }

    #[must_use]
    pub fn has_ask(&self) -> bool {
        self.asks.top().map_or(false, |top| !top.orders.is_empty())
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
        self.bids
            .top()
            .and_then(|top| top.first().map(|order| order.size))
    }

    #[must_use]
    pub fn best_ask_size(&self) -> Option<Quantity> {
        self.asks
            .top()
            .and_then(|top| top.first().map(|order| order.size))
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

        analysis::get_avg_px_for_quantity(qty, levels)
    }

    #[must_use]
    pub fn get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        let levels = match order_side {
            OrderSide::Buy => &self.asks.levels,
            OrderSide::Sell => &self.bids.levels,
            _ => panic!("Invalid `OrderSide` {order_side}"),
        };

        analysis::get_quantity_for_price(price, order_side, levels)
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

    use crate::{
        data::{
            depth::{stubs::stub_depth10, OrderBookDepth10},
            order::BookOrder,
            quote::QuoteTick,
            trade::TradeTick,
        },
        enums::{AggressorSide, BookType, OrderSide},
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
        orderbook::{
            aggregation::{book_update_quote_tick, book_update_trade_tick},
            analysis::book_check_integrity,
            book::OrderBook,
        },
        types::{price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_best_bid_and_ask_when_nothing_in_book() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(BookType::L2_MBP, instrument_id);

        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_ask_price(), None);
        assert_eq!(book.best_bid_size(), None);
        assert_eq!(book.best_ask_size(), None);
        assert!(!book.has_bid());
        assert!(!book.has_ask());
    }

    #[rstest]
    fn test_bid_side_with_one_order() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L3_MBO, instrument_id);
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
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L3_MBO, instrument_id);
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
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(BookType::L3_MBO, instrument_id);
        assert_eq!(book.spread(), None);
    }

    #[rstest]
    fn test_spread_with_bids_and_asks() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L3_MBO, instrument_id);
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
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(BookType::L2_MBP, instrument_id);
        assert_eq!(book.midpoint(), None);
    }

    #[rstest]
    fn test_midpoint_with_bids_asks() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L2_MBP, instrument_id);

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
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(BookType::L2_MBP, instrument_id);

        let qty = Quantity::from(1);

        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Buy), 0.0);
        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Sell), 0.0);
    }

    #[rstest]
    fn test_get_quantity_for_price_no_market() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(BookType::L2_MBP, instrument_id);

        let price = Price::from("1.0");

        assert_eq!(book.get_quantity_for_price(price, OrderSide::Buy), 0.0);
        assert_eq!(book.get_quantity_for_price(price, OrderSide::Sell), 0.0);
    }

    #[rstest]
    fn test_get_price_for_quantity() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L2_MBP, instrument_id);

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
            2.003_333_333_333_333_4
        );
        assert_eq!(
            book.get_avg_px_for_quantity(qty, OrderSide::Sell),
            0.996_666_666_666_666_7
        );
    }

    #[rstest]
    fn test_get_quantity_for_price() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L2_MBP, instrument_id);

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
    fn test_apply_depth(stub_depth10: OrderBookDepth10) {
        let depth = stub_depth10;
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(BookType::L2_MBP, instrument_id);

        book.apply_depth(depth);

        assert_eq!(book.best_bid_price().unwrap().as_f64(), 99.00);
        assert_eq!(book.best_ask_price().unwrap().as_f64(), 100.00);
        assert_eq!(book.best_bid_size().unwrap().as_f64(), 100.0);
        assert_eq!(book.best_ask_size().unwrap().as_f64(), 100.0);
    }

    #[rstest]
    fn test_orderbook_creation() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let book = OrderBook::new(BookType::L2_MBP, instrument_id);

        assert_eq!(book.instrument_id, instrument_id);
        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[rstest]
    fn test_orderbook_reset() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(BookType::L1_MBP, instrument_id);
        book.sequence = 10;
        book.ts_last = 100;
        book.count = 3;

        book.reset();

        assert_eq!(book.book_type, BookType::L1_MBP);
        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[rstest]
    fn test_update_quote_tick_l1() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L1_MBP, instrument_id);
        let quote = QuoteTick::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Price::from("5000.000"),
            Price::from("5100.000"),
            Quantity::from("100.00000000"),
            Quantity::from("99.00000000"),
            0,
            0,
        )
        .unwrap();

        book_update_quote_tick(&mut book, &quote);

        assert_eq!(book.best_bid_price().unwrap(), quote.bid_price);
        assert_eq!(book.best_ask_price().unwrap(), quote.ask_price);
        assert_eq!(book.best_bid_size().unwrap(), quote.bid_size);
        assert_eq!(book.best_ask_size().unwrap(), quote.ask_size);
    }

    #[rstest]
    fn test_update_trade_tick_l1() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L1_MBP, instrument_id);

        let price = Price::from("15000.000");
        let size = Quantity::from("10.00000000");
        let trade = TradeTick::new(
            instrument_id,
            price,
            size,
            AggressorSide::Buyer,
            TradeId::new("123456789").unwrap(),
            0,
            0,
        );

        book_update_trade_tick(&mut book, &trade);

        assert_eq!(book.best_bid_price().unwrap(), price);
        assert_eq!(book.best_ask_price().unwrap(), price);
        assert_eq!(book.best_bid_size().unwrap(), size);
        assert_eq!(book.best_ask_size().unwrap(), size);
    }

    #[rstest]
    fn test_check_integrity_when_crossed() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L2_MBP, instrument_id);

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

        assert!(book_check_integrity(&book).is_err());
    }

    #[rstest]
    fn test_pprint() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(BookType::L3_MBO, instrument_id);

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

        println!("{pprint_output}");
        assert_eq!(pprint_output, expected_output);
    }
}
