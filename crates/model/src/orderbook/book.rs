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
use nautilus_core::UnixNanos;
use rust_decimal::Decimal;

use super::{
    aggregation::pre_process_order, analysis, display::pprint_book, level::BookLevel,
    own::OwnOrderBook,
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
    /// The current count of updates applied to the order book.
    pub update_count: u64,
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
            "{}(instrument_id={}, book_type={}, update_count={})",
            stringify!(OrderBook),
            self.instrument_id,
            self.book_type,
            self.update_count,
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
            update_count: 0,
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
        self.update_count = 0;
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

    /// Groups bid quantities by price into buckets, limited by depth.
    pub fn group_bids(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
    ) -> IndexMap<Decimal, Decimal> {
        group_levels(self.bids(None), group_size, depth, true)
    }

    /// Groups ask quantities by price into buckets, limited by depth.
    pub fn group_asks(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
    ) -> IndexMap<Decimal, Decimal> {
        group_levels(self.asks(None), group_size, depth, false)
    }

    /// Maps bid prices to total public size per level, excluding own orders up to a depth limit.
    ///
    /// With `own_book`, subtracts own order sizes, filtered by `status` if provided.
    /// Uses `accepted_buffer_ns` to include only orders accepted at least that many
    /// nanoseconds before `now` (defaults to now).
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
            filter_quantities(
                &mut public_map,
                own_book.bid_quantity(status, accepted_buffer_ns, now),
            );
        }

        public_map
    }

    /// Maps ask prices to total public size per level, excluding own orders up to a depth limit.
    ///
    /// With `own_book`, subtracts own order sizes, filtered by `status` if provided.
    /// Uses `accepted_buffer_ns` to include only orders accepted at least that many
    /// nanoseconds before `now` (defaults to now).
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
            filter_quantities(
                &mut public_map,
                own_book.ask_quantity(status, accepted_buffer_ns, now),
            );
        }

        public_map
    }

    /// Groups bid quantities into price buckets, truncating to a maximum depth, excluding own orders.
    ///
    /// With `own_book`, subtracts own order sizes, filtered by `status` if provided.
    /// Uses `accepted_buffer_ns` to include only orders accepted at least that many
    /// nanoseconds before `now` (defaults to now).
    pub fn group_bids_filtered(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let mut public_map = group_levels(self.bids(None), group_size, depth, true);

        if let Some(own_book) = own_book {
            filter_quantities(
                &mut public_map,
                own_book.group_bids(group_size, depth, status, accepted_buffer_ns, now),
            );
        }

        public_map
    }

    /// Groups ask quantities into price buckets, truncating to a maximum depth, excluding own orders.
    ///
    /// With `own_book`, subtracts own order sizes, filtered by `status` if provided.
    /// Uses `accepted_buffer_ns` to include only orders accepted at least that many
    /// nanoseconds before `now` (defaults to now).
    pub fn group_asks_filtered(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let mut public_map = group_levels(self.asks(None), group_size, depth, false);

        if let Some(own_book) = own_book {
            filter_quantities(
                &mut public_map,
                own_book.group_asks(group_size, depth, status, accepted_buffer_ns, now),
            );
        }

        public_map
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
        self.update_count += 1;
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

fn filter_quantities(
    public_map: &mut IndexMap<Decimal, Decimal>,
    own_map: IndexMap<Decimal, Decimal>,
) {
    for (price, own_size) in own_map {
        if let Some(public_size) = public_map.get_mut(&price) {
            *public_size = (*public_size - own_size).max(Decimal::ZERO);

            if *public_size == Decimal::ZERO {
                public_map.shift_remove(&price);
            }
        }
    }
}

fn group_levels<'a>(
    levels_iter: impl Iterator<Item = &'a BookLevel>,
    group_size: Decimal,
    depth: Option<usize>,
    is_bid: bool,
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
