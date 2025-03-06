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

//! A performant, generic, multi-purpose order book.

use std::{collections::HashSet, fmt::Display};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, time::nanos_since_unix_epoch};
use rust_decimal::Decimal;

use super::{
    aggregation::pre_process_order,
    analysis,
    display::pprint_book,
    level::BookLevel,
    own::{OwnBookLevel, OwnOrderBook},
};
use crate::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick},
    enums::{BookAction, BookType, OrderSide, OrderSideSpecified, OrderStatus},
    identifiers::InstrumentId,
    orderbook::{InvalidBookOperation, ladder::BookLadder},
    types::{Price, Quantity},
};

/// Provides a high-performance, versatile order book.
///
/// Maintains buy (bid) and sell (ask) orders in price-time priority, supporting multiple
/// market data formats:
/// - L3 (MBO): Market By Order - tracks individual orders with unique IDs.
/// - L2 (MBP): Market By Price - aggregates orders at each price level.
/// - L1 (MBP): Top-of-Book - maintains only the best bid and ask prices.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBook {
    /// The instrument ID for the order book.
    pub instrument_id: InstrumentId,
    /// The order book type (MBP types will aggregate orders).
    pub book_type: BookType,
    /// The last event sequence number for the order book.
    pub sequence: u64,
    /// The timestamp of the last event applied to the order book.
    pub ts_last: UnixNanos,
    /// The current count of events applied to the order book.
    pub count: u64,
    pub(crate) bids: BookLadder,
    pub(crate) asks: BookLadder,
}

impl PartialEq for OrderBook {
    fn eq(&self, other: &Self) -> bool {
        self.instrument_id == other.instrument_id && self.book_type == other.book_type
    }
}

impl Eq for OrderBook {}

impl Display for OrderBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, book_type={})",
            stringify!(OrderBook),
            self.instrument_id,
            self.book_type,
        )
    }
}

impl OrderBook {
    /// Creates a new [`OrderBook`] instance.
    #[must_use]
    pub fn new(instrument_id: InstrumentId, book_type: BookType) -> Self {
        Self {
            instrument_id,
            book_type,
            sequence: 0,
            ts_last: UnixNanos::default(),
            count: 0,
            bids: BookLadder::new(OrderSideSpecified::Buy),
            asks: BookLadder::new(OrderSideSpecified::Sell),
        }
    }

    /// Resets the order book to its initial empty state.
    pub fn reset(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.sequence = 0;
        self.ts_last = UnixNanos::default();
        self.count = 0;
    }

    /// Adds an order to the book after preprocessing based on book type.
    pub fn add(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: UnixNanos) {
        let order = pre_process_order(self.book_type, order, flags);
        match order.side.as_specified() {
            OrderSideSpecified::Buy => self.bids.add(order),
            OrderSideSpecified::Sell => self.asks.add(order),
        }

        self.increment(sequence, ts_event);
    }

    /// Updates an existing order in the book after preprocessing based on book type.
    pub fn update(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: UnixNanos) {
        let order = pre_process_order(self.book_type, order, flags);
        match order.side.as_specified() {
            OrderSideSpecified::Buy => self.bids.update(order),
            OrderSideSpecified::Sell => self.asks.update(order),
        }

        self.increment(sequence, ts_event);
    }

    /// Deletes an order from the book after preprocessing based on book type.
    pub fn delete(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: UnixNanos) {
        let order = pre_process_order(self.book_type, order, flags);
        match order.side.as_specified() {
            OrderSideSpecified::Buy => self.bids.delete(order, sequence, ts_event),
            OrderSideSpecified::Sell => self.asks.delete(order, sequence, ts_event),
        }

        self.increment(sequence, ts_event);
    }

    /// Clears all orders from both sides of the book.
    pub fn clear(&mut self, sequence: u64, ts_event: UnixNanos) {
        self.bids.clear();
        self.asks.clear();
        self.increment(sequence, ts_event);
    }

    /// Clears all bid orders from the book.
    pub fn clear_bids(&mut self, sequence: u64, ts_event: UnixNanos) {
        self.bids.clear();
        self.increment(sequence, ts_event);
    }

    /// Clears all ask orders from the book.
    pub fn clear_asks(&mut self, sequence: u64, ts_event: UnixNanos) {
        self.asks.clear();
        self.increment(sequence, ts_event);
    }

    /// Applies a single order book delta operation.
    pub fn apply_delta(&mut self, delta: &OrderBookDelta) {
        let order = delta.order;
        let flags = delta.flags;
        let sequence = delta.sequence;
        let ts_event = delta.ts_event;
        match delta.action {
            BookAction::Add => self.add(order, flags, sequence, ts_event),
            BookAction::Update => self.update(order, flags, sequence, ts_event),
            BookAction::Delete => self.delete(order, flags, sequence, ts_event),
            BookAction::Clear => self.clear(sequence, ts_event),
        }
    }

    /// Applies multiple order book delta operations.
    pub fn apply_deltas(&mut self, deltas: &OrderBookDeltas) {
        for delta in &deltas.deltas {
            self.apply_delta(delta);
        }
    }

    /// Replaces current book state with a depth snapshot.
    pub fn apply_depth(&mut self, depth: &OrderBookDepth10) {
        self.bids.clear();
        self.asks.clear();

        for order in depth.bids {
            self.add(order, depth.flags, depth.sequence, depth.ts_event);
        }

        for order in depth.asks {
            self.add(order, depth.flags, depth.sequence, depth.ts_event);
        }
    }

    /// Returns an iterator over bid price levels.
    pub fn bids(&self, depth: Option<usize>) -> impl Iterator<Item = &BookLevel> {
        self.bids.levels.values().take(depth.unwrap_or(usize::MAX))
    }

    /// Returns an iterator over ask price levels.
    pub fn asks(&self, depth: Option<usize>) -> impl Iterator<Item = &BookLevel> {
        self.asks.levels.values().take(depth.unwrap_or(usize::MAX))
    }

    /// Returns bid price levels as a map of price to size.
    pub fn bids_as_map(&self, depth: Option<usize>) -> IndexMap<Decimal, Decimal> {
        self.bids(depth)
            .map(|level| (level.price.value.as_decimal(), level.size_decimal()))
            .collect()
    }

    /// Returns ask price levels as a map of price to size.
    pub fn asks_as_map(&self, depth: Option<usize>) -> IndexMap<Decimal, Decimal> {
        self.asks(depth)
            .map(|level| (level.price.value.as_decimal(), level.size_decimal()))
            .collect()
    }

    /// Returns bid price levels as a map of price to size with own order size filtered out.
    pub fn bids_filtered_as_map(
        &self,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let mut public_map = self
            .bids(depth)
            .map(|level| (level.price.value.as_decimal(), level.size_decimal()))
            .collect::<IndexMap<Decimal, Decimal>>();

        if let Some(own_book) = own_book {
            self.filter_quantities(
                &mut public_map,
                own_book.bids(),
                status.as_ref(),
                accepted_buffer_ns,
                now,
            );
        }

        public_map
    }

    /// Returns ask price levels as a map of price to size with own order size filtered out.
    pub fn asks_filtered_as_map(
        &self,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let mut public_map = self
            .asks(depth)
            .map(|level| (level.price.value.as_decimal(), level.size_decimal()))
            .collect::<IndexMap<Decimal, Decimal>>();

        if let Some(own_book) = own_book {
            self.filter_quantities(
                &mut public_map,
                own_book.asks(),
                status.as_ref(),
                accepted_buffer_ns,
                now,
            );
        }

        public_map
    }

    fn filter_quantities<'a>(
        &self,
        public_map: &mut IndexMap<Decimal, Decimal>,
        own_orders: impl Iterator<Item = &'a OwnBookLevel>,
        status_filter: Option<&HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) {
        let accepted_buffer_ns = accepted_buffer_ns.unwrap_or(0);
        let ts_now = ts_now.unwrap_or_else(nanos_since_unix_epoch);

        for level in own_orders {
            let price = level.price.value.as_decimal();
            if let Some(public_size) = public_map.get_mut(&price) {
                let own_size = level
                    .orders
                    .values()
                    .filter(|order| status_filter.is_none_or(|f| f.contains(&order.status)))
                    .filter(|order| order.ts_accepted + accepted_buffer_ns <= ts_now)
                    .map(|order| order.size.as_decimal())
                    .sum::<Decimal>();

                *public_size = (*public_size - own_size).max(Decimal::ZERO);

                if *public_size == Decimal::ZERO {
                    public_map.shift_remove(&price);
                }
            }
        }
    }

    /// Groups bid levels by price, up to specified depth.
    pub fn group_bids(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
    ) -> IndexMap<Decimal, Decimal> {
        self.group_levels(self.bids(None), group_size, true, depth)
    }

    /// Groups ask levels by price, up to specified depth.
    pub fn group_asks(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
    ) -> IndexMap<Decimal, Decimal> {
        self.group_levels(self.asks(None), group_size, false, depth)
    }

    fn group_levels<'a>(
        &self,
        levels_iter: impl Iterator<Item = &'a BookLevel>,
        group_size: Decimal,
        is_bid: bool,
        depth: Option<usize>,
    ) -> IndexMap<Decimal, Decimal> {
        let mut levels = IndexMap::new();
        let depth = depth.unwrap_or(usize::MAX);

        for level in levels_iter {
            let price = level.price.value.as_decimal();
            let grouped_price = if is_bid {
                (price / group_size).floor() * group_size
            } else {
                (price / group_size).ceil() * group_size
            };
            let size = level.size_decimal();

            levels
                .entry(grouped_price)
                .and_modify(|total| *total += size)
                .or_insert(size);

            if levels.len() > depth {
                levels.pop();
                break;
            }
        }

        levels
    }

    /// Returns true if the book has any bid orders.
    #[must_use]
    pub fn has_bid(&self) -> bool {
        self.bids.top().is_some_and(|top| !top.orders.is_empty())
    }

    /// Returns true if the book has any ask orders.
    #[must_use]
    pub fn has_ask(&self) -> bool {
        self.asks.top().is_some_and(|top| !top.orders.is_empty())
    }

    /// Returns the best bid price if available.
    #[must_use]
    pub fn best_bid_price(&self) -> Option<Price> {
        self.bids.top().map(|top| top.price.value)
    }

    /// Returns the best ask price if available.
    #[must_use]
    pub fn best_ask_price(&self) -> Option<Price> {
        self.asks.top().map(|top| top.price.value)
    }

    /// Returns the size at the best bid price if available.
    #[must_use]
    pub fn best_bid_size(&self) -> Option<Quantity> {
        self.bids
            .top()
            .and_then(|top| top.first().map(|order| order.size))
    }

    /// Returns the size at the best ask price if available.
    #[must_use]
    pub fn best_ask_size(&self) -> Option<Quantity> {
        self.asks
            .top()
            .and_then(|top| top.first().map(|order| order.size))
    }

    /// Returns the spread between best ask and bid prices if both exist.
    #[must_use]
    pub fn spread(&self) -> Option<f64> {
        match (self.best_ask_price(), self.best_bid_price()) {
            (Some(ask), Some(bid)) => Some(ask.as_f64() - bid.as_f64()),
            _ => None,
        }
    }

    /// Returns the midpoint between best ask and bid prices if both exist.
    #[must_use]
    pub fn midpoint(&self) -> Option<f64> {
        match (self.best_ask_price(), self.best_bid_price()) {
            (Some(ask), Some(bid)) => Some((ask.as_f64() + bid.as_f64()) / 2.0),
            _ => None,
        }
    }

    /// Calculates the average price to fill the specified quantity.
    #[must_use]
    pub fn get_avg_px_for_quantity(&self, qty: Quantity, order_side: OrderSide) -> f64 {
        let levels = match order_side.as_specified() {
            OrderSideSpecified::Buy => &self.asks.levels,
            OrderSideSpecified::Sell => &self.bids.levels,
        };

        analysis::get_avg_px_for_quantity(qty, levels)
    }

    /// Calculates average price and quantity for target exposure. Returns (price, quantity, executed_exposure).
    #[must_use]
    pub fn get_avg_px_qty_for_exposure(
        &self,
        target_exposure: Quantity,
        order_side: OrderSide,
    ) -> (f64, f64, f64) {
        let levels = match order_side.as_specified() {
            OrderSideSpecified::Buy => &self.asks.levels,
            OrderSideSpecified::Sell => &self.bids.levels,
        };

        analysis::get_avg_px_qty_for_exposure(target_exposure, levels)
    }

    /// Returns the total quantity available at specified price level.
    #[must_use]
    pub fn get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        let levels = match order_side.as_specified() {
            OrderSideSpecified::Buy => &self.asks.levels,
            OrderSideSpecified::Sell => &self.bids.levels,
        };

        analysis::get_quantity_for_price(price, order_side, levels)
    }

    /// Simulates fills for an order, returning list of (price, quantity) tuples.
    #[must_use]
    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        match order.side.as_specified() {
            OrderSideSpecified::Buy => self.asks.simulate_fills(order),
            OrderSideSpecified::Sell => self.bids.simulate_fills(order),
        }
    }

    /// Return a formatted string representation of the order book.
    #[must_use]
    pub fn pprint(&self, num_levels: usize) -> String {
        pprint_book(&self.bids, &self.asks, num_levels)
    }

    fn increment(&mut self, sequence: u64, ts_event: UnixNanos) {
        self.sequence = sequence;
        self.ts_last = ts_event;
        self.count += 1;
    }

    /// Updates L1 book state from a quote tick. Only valid for L1_MBP book type.
    pub fn update_quote_tick(&mut self, quote: &QuoteTick) -> Result<(), InvalidBookOperation> {
        if self.book_type != BookType::L1_MBP {
            return Err(InvalidBookOperation::Update(self.book_type));
        };

        let bid = BookOrder::new(
            OrderSide::Buy,
            quote.bid_price,
            quote.bid_size,
            OrderSide::Buy as u64,
        );

        let ask = BookOrder::new(
            OrderSide::Sell,
            quote.ask_price,
            quote.ask_size,
            OrderSide::Sell as u64,
        );

        self.update_book_bid(bid, quote.ts_event);
        self.update_book_ask(ask, quote.ts_event);

        Ok(())
    }

    /// Updates L1 book state from a trade tick. Only valid for L1_MBP book type.
    pub fn update_trade_tick(&mut self, trade: &TradeTick) -> Result<(), InvalidBookOperation> {
        if self.book_type != BookType::L1_MBP {
            return Err(InvalidBookOperation::Update(self.book_type));
        };

        let bid = BookOrder::new(
            OrderSide::Buy,
            trade.price,
            trade.size,
            OrderSide::Buy as u64,
        );

        let ask = BookOrder::new(
            OrderSide::Sell,
            trade.price,
            trade.size,
            OrderSide::Sell as u64,
        );

        self.update_book_bid(bid, trade.ts_event);
        self.update_book_ask(ask, trade.ts_event);

        Ok(())
    }

    fn update_book_bid(&mut self, order: BookOrder, ts_event: UnixNanos) {
        if let Some(top_bids) = self.bids.top() {
            if let Some(top_bid) = top_bids.first() {
                self.bids.remove(top_bid.order_id, 0, ts_event);
            }
        }
        self.bids.add(order);
    }

    fn update_book_ask(&mut self, order: BookOrder, ts_event: UnixNanos) {
        if let Some(top_asks) = self.asks.top() {
            if let Some(top_ask) = top_asks.first() {
                self.asks.remove(top_ask.order_id, 0, ts_event);
            }
        }
        self.asks.add(order);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use crate::{
        data::{QuoteTick, TradeTick, depth::OrderBookDepth10, order::BookOrder, stubs::*},
        enums::{
            AggressorSide, BookType, OrderSide, OrderSideSpecified, OrderStatus, OrderType,
            TimeInForce,
        },
        identifiers::{ClientOrderId, InstrumentId, TradeId},
        orderbook::{
            BookIntegrityError, BookPrice, OrderBook, OwnBookOrder, analysis::book_check_integrity,
            own::OwnOrderBook,
        },
        types::{Price, Quantity},
    };

    #[rstest]
    #[case::valid_book(
    BookType::L2_MBP,
    vec![
        (OrderSide::Buy, "99.00", 100, 1001),
        (OrderSide::Sell, "101.00", 100, 2001),
    ],
    Ok(())
)]
    #[case::crossed_book(
    BookType::L2_MBP,
    vec![
        (OrderSide::Buy, "101.00", 100, 1001),
        (OrderSide::Sell, "99.00", 100, 2001),
    ],
    Err(BookIntegrityError::OrdersCrossed(
        BookPrice::new(Price::from("101.00"), OrderSideSpecified::Buy),
        BookPrice::new(Price::from("99.00"), OrderSideSpecified::Sell),
    ))
)]
    #[case::too_many_levels_l1(
    BookType::L1_MBP,
    vec![
        (OrderSide::Buy, "99.00", 100, 1001),
        (OrderSide::Buy, "98.00", 100, 1002),
    ],
    Err(BookIntegrityError::TooManyLevels(OrderSide::Buy, 2))
)]
    fn test_book_integrity_cases(
        #[case] book_type: BookType,
        #[case] orders: Vec<(OrderSide, &str, i64, u64)>,
        #[case] expected: Result<(), BookIntegrityError>,
    ) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, book_type);

        for (side, price, size, id) in orders {
            let order = BookOrder::new(side, Price::from(price), Quantity::from(size), id);
            book.add(order, 0, id, id.into());
        }

        assert_eq!(book_check_integrity(&book), expected);
    }

    #[rstest]
    fn test_book_integrity_price_boundaries() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let min_bid = BookOrder::new(OrderSide::Buy, Price::min(2), Quantity::from(100), 1);
        let max_ask = BookOrder::new(OrderSide::Sell, Price::max(2), Quantity::from(100), 2);

        book.add(min_bid, 0, 1, 1.into());
        book.add(max_ask, 0, 2, 2.into());

        assert!(book_check_integrity(&book).is_ok());
    }

    #[rstest]
    #[case::small_quantity(100)]
    #[case::medium_quantity(1000)]
    #[case::large_quantity(1000000)]
    fn test_book_integrity_quantity_sizes(#[case] quantity: i64) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(quantity),
            1,
        );
        book.add(bid, 0, 1, 1.into());

        assert!(book_check_integrity(&book).is_ok());
        assert_eq!(book.best_bid_size().unwrap().as_f64() as i64, quantity);
    }

    #[rstest]
    fn test_display() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);
        assert_eq!(
            book.to_string(),
            "OrderBook(instrument_id=ETHUSDT-PERP.BINANCE, book_type=L2_MBP)"
        );
    }

    #[rstest]
    fn test_empty_book_state() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);

        assert_eq!(book.best_bid_price(), None);
        assert_eq!(book.best_ask_price(), None);
        assert_eq!(book.best_bid_size(), None);
        assert_eq!(book.best_ask_size(), None);
        assert!(!book.has_bid());
        assert!(!book.has_ask());
    }

    #[rstest]
    fn test_single_bid_state() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
        let order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.000"),
            Quantity::from("1.0"),
            1,
        );
        book.add(order1, 0, 1, 100.into());

        assert_eq!(book.best_bid_price(), Some(Price::from("1.000")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("1.0")));
        assert!(book.has_bid());
    }

    #[rstest]
    fn test_single_ask_state() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from("2.000"),
            Quantity::from("2.0"),
            2,
        );
        book.add(order, 0, 2, 200.into());

        assert_eq!(book.best_ask_price(), Some(Price::from("2.000")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("2.0")));
        assert!(book.has_ask());
    }

    #[rstest]
    fn test_empty_book_spread() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L3_MBO);
        assert_eq!(book.spread(), None);
    }

    #[rstest]
    fn test_spread_with_orders() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);
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
        book.add(bid1, 0, 1, 100.into());
        book.add(ask1, 0, 2, 200.into());

        assert_eq!(book.spread(), Some(1.0));
    }

    #[rstest]
    fn test_empty_book_midpoint() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);
        assert_eq!(book.midpoint(), None);
    }

    #[rstest]
    fn test_midpoint_with_orders() {
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
        book.add(bid1, 0, 1, 100.into());
        book.add(ask1, 0, 2, 200.into());

        assert_eq!(book.midpoint(), Some(1.5));
    }

    #[rstest]
    fn test_get_price_for_quantity_no_market() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let qty = Quantity::from(1);

        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Buy), 0.0);
        assert_eq!(book.get_avg_px_for_quantity(qty, OrderSide::Sell), 0.0);
    }

    #[rstest]
    fn test_get_quantity_for_price_no_market() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);

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
        book.add(bid1, 0, 1, 2.into());
        book.add(bid2, 0, 1, 2.into());
        book.add(ask1, 0, 1, 2.into());
        book.add(ask2, 0, 1, 2.into());

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
        book.add(bid1, 0, 0, 1.into());
        book.add(bid2, 0, 0, 1.into());
        book.add(bid3, 0, 0, 1.into());
        book.add(ask1, 0, 0, 1.into());
        book.add(ask2, 0, 0, 1.into());
        book.add(ask3, 0, 0, 1.into());

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
    fn test_get_price_for_exposure_no_market() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let qty = Quantity::from(1);

        assert_eq!(
            book.get_avg_px_qty_for_exposure(qty, OrderSide::Buy),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(
            book.get_avg_px_qty_for_exposure(qty, OrderSide::Sell),
            (0.0, 0.0, 0.0)
        );
    }

    #[rstest]
    fn test_get_price_for_exposure(stub_depth10: OrderBookDepth10) {
        let depth = stub_depth10;
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        book.apply_depth(&depth);

        let qty = Quantity::from(1);

        assert_eq!(
            book.get_avg_px_qty_for_exposure(qty, OrderSide::Buy),
            (100.0, 0.01, 100.0)
        );
        // TODO: Revisit calculations
        // assert_eq!(
        //     book.get_avg_px_qty_for_exposure(qty, OrderSide::Sell),
        //     (99.0, 0.01010101, 99.0)
        // );
    }

    #[rstest]
    fn test_apply_depth(stub_depth10: OrderBookDepth10) {
        let depth = stub_depth10;
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        book.apply_depth(&depth);

        assert_eq!(book.best_bid_price().unwrap().as_f64(), 99.00);
        assert_eq!(book.best_ask_price().unwrap().as_f64(), 100.00);
        assert_eq!(book.best_bid_size().unwrap().as_f64(), 100.0);
        assert_eq!(book.best_ask_size().unwrap().as_f64(), 100.0);
    }

    #[rstest]
    fn test_orderbook_creation() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);

        assert_eq!(book.instrument_id, instrument_id);
        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.sequence, 0);
        assert_eq!(book.ts_last, 0);
        assert_eq!(book.count, 0);
    }

    #[rstest]
    fn test_orderbook_reset() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);
        book.sequence = 10;
        book.ts_last = 100.into();
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
        let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);
        let quote = QuoteTick::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Price::from("5000.000"),
            Price::from("5100.000"),
            Quantity::from("100.00000000"),
            Quantity::from("99.00000000"),
            0.into(),
            0.into(),
        );

        book.update_quote_tick(&quote).unwrap();

        assert_eq!(book.best_bid_price().unwrap(), quote.bid_price);
        assert_eq!(book.best_ask_price().unwrap(), quote.ask_price);
        assert_eq!(book.best_bid_size().unwrap(), quote.bid_size);
        assert_eq!(book.best_ask_size().unwrap(), quote.ask_size);
    }

    #[rstest]
    fn test_update_trade_tick_l1() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L1_MBP);

        let price = Price::from("15000.000");
        let size = Quantity::from("10.00000000");
        let trade = TradeTick::new(
            instrument_id,
            price,
            size,
            AggressorSide::Buyer,
            TradeId::new("123456789"),
            0.into(),
            0.into(),
        );

        book.update_trade_tick(&trade).unwrap();

        assert_eq!(book.best_bid_price().unwrap(), price);
        assert_eq!(book.best_ask_price().unwrap(), price);
        assert_eq!(book.best_bid_size().unwrap(), size);
        assert_eq!(book.best_ask_size().unwrap(), size);
    }

    #[rstest]
    fn test_pprint() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L3_MBO);

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

        book.add(order1, 0, 1, 100.into());
        book.add(order2, 0, 2, 200.into());
        book.add(order3, 0, 3, 300.into());
        book.add(order4, 0, 4, 400.into());
        book.add(order5, 0, 5, 500.into());
        book.add(order6, 0, 6, 600.into());

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

    #[rstest]
    fn test_group_empty_book() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let grouped_bids = book.group_bids(dec!(1), None);
        let grouped_asks = book.group_asks(dec!(1), None);

        assert!(grouped_bids.is_empty());
        assert!(grouped_asks.is_empty());
    }

    #[rstest]
    fn test_group_price_levels() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let orders = vec![
            BookOrder::new(OrderSide::Buy, Price::from("1.1"), Quantity::from(1), 1),
            BookOrder::new(OrderSide::Buy, Price::from("1.2"), Quantity::from(2), 2),
            BookOrder::new(OrderSide::Buy, Price::from("1.8"), Quantity::from(3), 3),
            BookOrder::new(OrderSide::Sell, Price::from("2.1"), Quantity::from(1), 4),
            BookOrder::new(OrderSide::Sell, Price::from("2.2"), Quantity::from(2), 5),
            BookOrder::new(OrderSide::Sell, Price::from("2.8"), Quantity::from(3), 6),
        ];
        for (i, order) in orders.into_iter().enumerate() {
            book.add(order, 0, i as u64, 100.into());
        }

        let grouped_bids = book.group_bids(dec!(0.5), Some(10));
        let grouped_asks = book.group_asks(dec!(0.5), Some(10));

        assert_eq!(grouped_bids.len(), 2);
        assert_eq!(grouped_asks.len(), 2);
        assert_eq!(grouped_bids.get(&dec!(1.0)), Some(&dec!(3))); // 1.1, 1.2 group to 1.0
        assert_eq!(grouped_bids.get(&dec!(1.5)), Some(&dec!(3))); // 1.8 groups to 1.5
        assert_eq!(grouped_asks.get(&dec!(2.5)), Some(&dec!(3))); // 2.1, 2.2 group to 2.5
        assert_eq!(grouped_asks.get(&dec!(3.0)), Some(&dec!(3))); // 2.8 groups to 3.0
    }

    #[rstest]
    fn test_group_with_depth_limit() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let orders = vec![
            BookOrder::new(OrderSide::Buy, Price::from("1.0"), Quantity::from(1), 1),
            BookOrder::new(OrderSide::Buy, Price::from("2.0"), Quantity::from(2), 2),
            BookOrder::new(OrderSide::Buy, Price::from("3.0"), Quantity::from(3), 3),
            BookOrder::new(OrderSide::Sell, Price::from("4.0"), Quantity::from(1), 4),
            BookOrder::new(OrderSide::Sell, Price::from("5.0"), Quantity::from(2), 5),
            BookOrder::new(OrderSide::Sell, Price::from("6.0"), Quantity::from(3), 6),
        ];

        for (i, order) in orders.into_iter().enumerate() {
            book.add(order, 0, i as u64, 100.into());
        }

        let grouped_bids = book.group_bids(dec!(1), Some(2));
        let grouped_asks = book.group_asks(dec!(1), Some(2));

        assert_eq!(grouped_bids.len(), 2); // Should only have levels at 2.0 and 3.0
        assert_eq!(grouped_asks.len(), 2); // Should only have levels at 5.0 and 6.0
        assert_eq!(grouped_bids.get(&dec!(3)), Some(&dec!(3)));
        assert_eq!(grouped_bids.get(&dec!(2)), Some(&dec!(2)));
        assert_eq!(grouped_asks.get(&dec!(4)), Some(&dec!(1)));
        assert_eq!(grouped_asks.get(&dec!(5)), Some(&dec!(2)));
    }

    #[rstest]
    fn test_group_price_realistic() {
        let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let orders = vec![
            BookOrder::new(
                OrderSide::Buy,
                Price::from("100.00000"),
                Quantity::from(1000),
                1,
            ),
            BookOrder::new(
                OrderSide::Buy,
                Price::from("99.00000"),
                Quantity::from(2000),
                2,
            ),
            BookOrder::new(
                OrderSide::Buy,
                Price::from("98.00000"),
                Quantity::from(3000),
                3,
            ),
            BookOrder::new(
                OrderSide::Sell,
                Price::from("101.00000"),
                Quantity::from(1000),
                4,
            ),
            BookOrder::new(
                OrderSide::Sell,
                Price::from("102.00000"),
                Quantity::from(2000),
                5,
            ),
            BookOrder::new(
                OrderSide::Sell,
                Price::from("103.00000"),
                Quantity::from(3000),
                6,
            ),
        ];
        for (i, order) in orders.into_iter().enumerate() {
            book.add(order, 0, i as u64, 100.into());
        }

        let grouped_bids = book.group_bids(dec!(2), Some(10));
        let grouped_asks = book.group_asks(dec!(2), Some(10));

        assert_eq!(grouped_bids.len(), 2);
        assert_eq!(grouped_asks.len(), 2);
        assert_eq!(grouped_bids.get(&dec!(100.0)), Some(&dec!(1000)));
        assert_eq!(grouped_bids.get(&dec!(98.0)), Some(&dec!(5000))); // 2000 + 3000 grouped
        assert_eq!(grouped_asks.get(&dec!(102.0)), Some(&dec!(3000))); // 1000 + 2000 grouped
        assert_eq!(grouped_asks.get(&dec!(104.0)), Some(&dec!(3000)));
    }

    #[rstest]
    fn test_filtered_book_empty_own_book() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        // Add some orders to the public book
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            2,
        );
        book.add(bid_order, 0, 1, 1.into());
        book.add(ask_order, 0, 2, 2.into());

        // No own book provided, filtered map should be identical to regular map
        let bids_filtered = book.bids_filtered_as_map(None, None, None, None, None);
        let asks_filtered = book.asks_filtered_as_map(None, None, None, None, None);

        let bids_regular = book.bids_as_map(None);
        let asks_regular = book.asks_as_map(None);

        assert_eq!(bids_filtered, bids_regular);
        assert_eq!(asks_filtered, asks_regular);
    }

    #[rstest]
    fn test_filtered_book_with_own_orders() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book
        let bid_order1 = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        let bid_order2 =
            BookOrder::new(OrderSide::Buy, Price::from("99.00"), Quantity::from(200), 2);
        let ask_order1 = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            3,
        );
        let ask_order2 = BookOrder::new(
            OrderSide::Sell,
            Price::from("102.00"),
            Quantity::from(200),
            4,
        );

        book.add(bid_order1, 0, 1, 1.into());
        book.add(bid_order2, 0, 2, 2.into());
        book.add(ask_order1, 0, 3, 3.into());
        book.add(ask_order2, 0, 4, 4.into());

        // Add own orders - half the size of public orders at the same levels
        let own_bid_order = OwnBookOrder::new(
            ClientOrderId::from("BID-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(50),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            1.into(),
            1.into(),
            1.into(),
        );

        let own_ask_order = OwnBookOrder::new(
            ClientOrderId::from("ASK-1"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(50),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            2.into(),
            2.into(),
            2.into(),
        );

        own_book.add(own_bid_order);
        own_book.add(own_ask_order);

        // Get filtered maps
        let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
        let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

        // Check that own order sizes are subtracted
        assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(50))); // 100 - 50 = 50
        assert_eq!(bids_filtered.get(&dec!(99.00)), Some(&dec!(200))); // unchanged
        assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(50))); // 100 - 50 = 50
        assert_eq!(asks_filtered.get(&dec!(102.00)), Some(&dec!(200))); // unchanged
    }

    #[rstest]
    fn test_filtered_book_with_own_orders_exact_size() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            2,
        );

        book.add(bid_order, 0, 1, 1.into());
        book.add(ask_order, 0, 2, 2.into());

        // Add own orders with exact same size as public orders
        let own_bid_order = OwnBookOrder::new(
            ClientOrderId::from("BID-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            1.into(),
            1.into(),
            1.into(),
        );

        let own_ask_order = OwnBookOrder::new(
            ClientOrderId::from("ASK-1"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            2.into(),
            2.into(),
            2.into(),
        );

        own_book.add(own_bid_order);
        own_book.add(own_ask_order);

        // Get filtered maps
        let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
        let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

        // Price levels should be removed as resulting size is zero
        assert!(!bids_filtered.contains_key(&dec!(100.00)));
        assert!(!asks_filtered.contains_key(&dec!(101.00)));
    }

    #[rstest]
    fn test_filtered_book_with_own_orders_larger_size() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            2,
        );

        book.add(bid_order, 0, 1, 1.into());
        book.add(ask_order, 0, 2, 2.into());

        // Add own orders with larger size than public orders
        let own_bid_order = OwnBookOrder::new(
            ClientOrderId::from("BID-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(150),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            1.into(),
            1.into(),
            1.into(),
        );

        let own_ask_order = OwnBookOrder::new(
            ClientOrderId::from("ASK-1"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(150),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            2.into(),
            2.into(),
            2.into(),
        );

        own_book.add(own_bid_order);
        own_book.add(own_ask_order);

        // Get filtered maps
        let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
        let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

        // Price levels should be removed as resulting size is zero or negative
        assert!(!bids_filtered.contains_key(&dec!(100.00)));
        assert!(!asks_filtered.contains_key(&dec!(101.00)));
    }

    #[rstest]
    fn test_filtered_book_with_own_orders_different_level() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book at certain levels
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            2,
        );

        book.add(bid_order, 0, 1, 1.into());
        book.add(ask_order, 0, 2, 2.into());

        // Add own orders at different price levels
        let own_bid_order = OwnBookOrder::new(
            ClientOrderId::from("BID-1"),
            OrderSideSpecified::Buy,
            Price::from("99.00"),
            Quantity::from(50),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            1.into(),
            1.into(),
            1.into(),
        );

        let own_ask_order = OwnBookOrder::new(
            ClientOrderId::from("ASK-1"),
            OrderSideSpecified::Sell,
            Price::from("102.00"),
            Quantity::from(50),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            2.into(),
            2.into(),
            2.into(),
        );

        own_book.add(own_bid_order);
        own_book.add(own_ask_order);

        // Get filtered maps
        let bids_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
        let asks_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

        // Public book levels should be unchanged as own orders are at different levels
        assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(100)));
        assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(100)));
    }

    #[rstest]
    fn test_filtered_book_with_status_filter() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            2,
        );

        book.add(bid_order, 0, 1, 1.into());
        book.add(ask_order, 0, 2, 2.into());

        // Add multiple own orders with different statuses at same price
        let own_bid_accepted = OwnBookOrder::new(
            ClientOrderId::from("BID-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(30),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            1.into(),
            1.into(),
            1.into(),
        );

        let own_bid_submitted = OwnBookOrder::new(
            ClientOrderId::from("BID-2"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(40),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Submitted,
            2.into(),
            2.into(),
            2.into(),
        );

        let own_ask_accepted = OwnBookOrder::new(
            ClientOrderId::from("ASK-1"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(30),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            3.into(),
            3.into(),
            3.into(),
        );

        let own_ask_submitted = OwnBookOrder::new(
            ClientOrderId::from("ASK-2"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(40),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Submitted,
            4.into(),
            4.into(),
            4.into(),
        );

        own_book.add(own_bid_accepted);
        own_book.add(own_bid_submitted);
        own_book.add(own_ask_accepted);
        own_book.add(own_ask_submitted);

        // Create a status filter for only ACCEPTED orders
        let mut status_filter = HashSet::new();
        status_filter.insert(OrderStatus::Accepted);

        // Get filtered maps with status filter
        let bids_filtered = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter.clone()),
            None,
            None,
        );
        let asks_filtered =
            book.asks_filtered_as_map(None, Some(&own_book), Some(status_filter), None, None);

        // Check that only ACCEPTED own orders are subtracted
        assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(70))); // 100 - 30 = 70
        assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(70))); // 100 - 30 = 70

        // Get filtered maps without status filter (should subtract all own orders)
        let bids_all_filtered = book.bids_filtered_as_map(None, Some(&own_book), None, None, None);
        let asks_all_filtered = book.asks_filtered_as_map(None, Some(&own_book), None, None, None);

        assert_eq!(bids_all_filtered.get(&dec!(100.00)), Some(&dec!(30))); // 100 - 30 - 40 = 30
        assert_eq!(asks_all_filtered.get(&dec!(101.00)), Some(&dec!(30))); // 100 - 30 - 40 = 30
    }

    #[rstest]
    fn test_filtered_book_with_depth_limit() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book at multiple levels
        let bid_orders = vec![
            BookOrder::new(
                OrderSide::Buy,
                Price::from("100.00"),
                Quantity::from(100),
                1,
            ),
            BookOrder::new(OrderSide::Buy, Price::from("99.00"), Quantity::from(200), 2),
            BookOrder::new(OrderSide::Buy, Price::from("98.00"), Quantity::from(300), 3),
        ];

        let ask_orders = vec![
            BookOrder::new(
                OrderSide::Sell,
                Price::from("101.00"),
                Quantity::from(100),
                4,
            ),
            BookOrder::new(
                OrderSide::Sell,
                Price::from("102.00"),
                Quantity::from(200),
                5,
            ),
            BookOrder::new(
                OrderSide::Sell,
                Price::from("103.00"),
                Quantity::from(300),
                6,
            ),
        ];

        for (i, order) in bid_orders.iter().enumerate() {
            book.add(*order, 0, i as u64, (i as u64).into());
        }

        for (i, order) in ask_orders.iter().enumerate() {
            book.add(
                *order,
                0,
                ((i + bid_orders.len()) as u64).into(),
                ((i + bid_orders.len()) as u64).into(),
            );
        }

        // Add own orders at some levels
        let own_bid_order = OwnBookOrder::new(
            ClientOrderId::from("BID-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(50),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            1.into(),
            1.into(),
            1.into(),
        );

        let own_ask_order = OwnBookOrder::new(
            ClientOrderId::from("ASK-1"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(50),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            2.into(),
            2.into(),
            2.into(),
        );

        own_book.add(own_bid_order);
        own_book.add(own_ask_order);

        // Get filtered maps with depth limit
        let bids_filtered = book.bids_filtered_as_map(Some(2), Some(&own_book), None, None, None);
        let asks_filtered = book.asks_filtered_as_map(Some(2), Some(&own_book), None, None, None);

        // Check that depth limit is respected and filtering still works
        assert_eq!(bids_filtered.len(), 2);
        assert_eq!(asks_filtered.len(), 2);

        assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(50))); // 100 - 50 = 50
        assert_eq!(bids_filtered.get(&dec!(99.00)), Some(&dec!(200))); // unchanged
        assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(50))); // 100 - 50 = 50
        assert_eq!(asks_filtered.get(&dec!(102.00)), Some(&dec!(200))); // unchanged

        // Third level should not be present due to depth limit
        assert!(!bids_filtered.contains_key(&dec!(98.00)));
        assert!(!asks_filtered.contains_key(&dec!(103.00)));
    }

    #[rstest]
    fn test_filtered_book_with_accepted_buffer() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        let ask_order = BookOrder::new(
            OrderSide::Sell,
            Price::from("101.00"),
            Quantity::from(100),
            2,
        );

        book.add(bid_order, 0, 1, 1.into());
        book.add(ask_order, 0, 2, 2.into());

        // Current time is 1000 ns
        let now = UnixNanos::from(1000);

        // Add own orders with ACCEPTED status at different times
        // This order was accepted at time 900 ns (100 ns ago)
        let own_bid_recent = OwnBookOrder::new(
            ClientOrderId::from("BID-RECENT"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(30),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            900.into(), // ts_last is 100 ns ago
            900.into(), // ts_last is 100 ns ago
            800.into(),
        );

        // This order was accepted at time 500 ns (500 ns ago)
        let own_bid_older = OwnBookOrder::new(
            ClientOrderId::from("BID-OLDER"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(40),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            500.into(), // ts_last is 500 ns ago
            500.into(), // ts_last is 500 ns ago
            400.into(),
        );

        // This order was accepted at time 900 ns (100 ns ago)
        let own_ask_recent = OwnBookOrder::new(
            ClientOrderId::from("ASK-RECENT"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(30),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            900.into(), // ts_last is 100 ns ago
            900.into(), // ts_last is 100 ns ago
            800.into(),
        );

        // This order was accepted at time 500 ns (500 ns ago)
        let own_ask_older = OwnBookOrder::new(
            ClientOrderId::from("ASK-OLDER"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from(40),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            500.into(), // ts_last is 500 ns ago
            500.into(), // ts_last is 500 ns ago
            400.into(),
        );

        own_book.add(own_bid_recent);
        own_book.add(own_bid_older);
        own_book.add(own_ask_recent);
        own_book.add(own_ask_older);

        // Status filter for ACCEPTED orders only
        let mut status_filter = HashSet::new();
        status_filter.insert(OrderStatus::Accepted);

        // Test with a 200 ns buffer - only orders accepted before 800 ns should be filtered
        let accepted_buffer = 200;

        // Get filtered maps with accepted_buffer
        let bids_filtered = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter.clone()),
            Some(accepted_buffer),
            Some(now.into()),
        );

        let asks_filtered = book.asks_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter.clone()),
            Some(accepted_buffer),
            Some(now.into()),
        );

        // Only older orders should be filtered out, recent orders should still be included
        // 100 - 40 = 60 (only older order subtracted)
        assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(60)));
        assert_eq!(asks_filtered.get(&dec!(101.00)), Some(&dec!(60)));

        // Test with a 50 ns buffer - all orders should be filtered
        let short_buffer = 50;

        let bids_short_buffer = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter.clone()),
            Some(short_buffer),
            Some(now.into()),
        );

        let asks_short_buffer = book.asks_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter.clone()),
            Some(short_buffer),
            Some(now.into()),
        );

        // All orders should be filtered out
        // 100 - 30 - 40 = 30
        assert_eq!(bids_short_buffer.get(&dec!(100.00)), Some(&dec!(30)));
        assert_eq!(asks_short_buffer.get(&dec!(101.00)), Some(&dec!(30)));

        // Test with a 600 ns buffer - no orders should be filtered
        let long_buffer = 600;

        let bids_long_buffer = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter.clone()),
            Some(long_buffer),
            Some(now.into()),
        );

        let asks_long_buffer = book.asks_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter.clone()),
            Some(long_buffer),
            Some(now.into()),
        );

        // No orders should be filtered out (all too recent)
        assert_eq!(bids_long_buffer.get(&dec!(100.00)), Some(&dec!(100)));
        assert_eq!(asks_long_buffer.get(&dec!(101.00)), Some(&dec!(100)));
    }

    #[rstest]
    fn test_filtered_book_with_accepted_buffer_mixed_statuses() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut own_book = OwnOrderBook::new(instrument_id);

        // Add orders to the public book
        let bid_order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(100),
            1,
        );
        book.add(bid_order, 0, 1, 1.into());

        // Current time is 1000 ns
        let now = UnixNanos::from(1000);

        // Add own orders with different statuses
        let own_bid_accepted = OwnBookOrder::new(
            ClientOrderId::from("BID-ACCEPTED"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(20),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            500.into(), // ts_last is 500 ns ago
            500.into(), // ts_last is 500 ns ago
            400.into(),
        );

        let own_bid_submitted = OwnBookOrder::new(
            ClientOrderId::from("BID-SUBMITTED"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from(30),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Submitted,
            500.into(), // ts_last doesn't matter for non-ACCEPTED orders
            500.into(),
            400.into(),
        );

        own_book.add(own_bid_accepted);
        own_book.add(own_bid_submitted);

        // Test with no status filter but with accepted buffer
        // Buffer of 300 ns means orders accepted before 700 ns should be filtered
        let accepted_buffer = 300;

        // Without status filter, buffer applies only to ACCEPTED orders
        let bids_filtered = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            None,
            Some(accepted_buffer),
            Some(now.into()),
        );

        // ACCEPTED order should be filtered (500 + 300 = 800 < 1000)
        // SUBMITTED order is always filtered when no status filter
        // 100 - 20 - 30 = 50
        assert_eq!(bids_filtered.get(&dec!(100.00)), Some(&dec!(50)));

        // Now test with a status filter for SUBMITTED only
        let mut status_filter = HashSet::new();
        status_filter.insert(OrderStatus::Submitted);

        let bids_filtered_submitted = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter),
            Some(accepted_buffer),
            Some(now.into()),
        );

        // Only SUBMITTED orders should be filtered, buffer doesn't apply
        // 100 - 30 = 70
        assert_eq!(bids_filtered_submitted.get(&dec!(100.00)), Some(&dec!(70)));

        // Now test with a status filter for both SUBMITTED and ACCEPTED
        let mut status_filter_both = HashSet::new();
        status_filter_both.insert(OrderStatus::Submitted);
        status_filter_both.insert(OrderStatus::Accepted);

        let bids_filtered_both = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter_both.clone()),
            Some(accepted_buffer),
            Some(now.into()),
        );

        // Both orders should be filtered, buffer applies to ACCEPTED
        // 100 - 20 - 30 = 50
        assert_eq!(bids_filtered_both.get(&dec!(100.00)), Some(&dec!(50)));

        // Test with a longer buffer that excludes the ACCEPTED order
        let long_buffer = 600;

        let bids_filtered_long_buffer = book.bids_filtered_as_map(
            None,
            Some(&own_book),
            Some(status_filter_both),
            Some(long_buffer),
            Some(now.into()),
        );

        // Only SUBMITTED order is filtered, ACCEPTED is too recent
        // 100 - 30 = 70
        assert_eq!(
            bids_filtered_long_buffer.get(&dec!(100.00)),
            Some(&dec!(100))
        );
    }
}
