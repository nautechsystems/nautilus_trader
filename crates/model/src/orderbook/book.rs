// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::Display;

use ahash::AHashSet;
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
    orderbook::{
        BookIntegrityError, InvalidBookOperation,
        ladder::{BookLadder, BookPrice},
    },
    types::{
        Price, Quantity,
        price::{PRICE_ERROR, PRICE_UNDEF},
    },
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
            bids: BookLadder::new(OrderSideSpecified::Buy, book_type),
            asks: BookLadder::new(OrderSideSpecified::Sell, book_type),
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
            OrderSideSpecified::Buy => self.bids.add(order, flags),
            OrderSideSpecified::Sell => self.asks.add(order, flags),
        }

        self.increment(sequence, ts_event);
    }

    /// Updates an existing order in the book after preprocessing based on book type.
    pub fn update(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: UnixNanos) {
        let order = pre_process_order(self.book_type, order, flags);
        match order.side.as_specified() {
            OrderSideSpecified::Buy => self.bids.update(order, flags),
            OrderSideSpecified::Sell => self.asks.update(order, flags),
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

    /// Removes overlapped bid/ask levels when the book is strictly crossed (best bid > best ask)
    ///
    /// - Acts only when both sides exist and the book is crossed.
    /// - Deletes by removing whole price levels via the ladder API to preserve invariants.
    /// - `side=None` or `NoOrderSide` clears both overlapped ranges (conservative, may widen spread).
    /// - `side=Buy` clears crossed bids only; side=Sell clears crossed asks only.
    /// - Returns removed price levels (crossed bids first, then crossed asks), or None if nothing removed.
    pub fn clear_stale_levels(&mut self, side: Option<OrderSide>) -> Option<Vec<BookLevel>> {
        if self.book_type == BookType::L1_MBP {
            // L1_MBP maintains a single top-of-book price per side; nothing to do
            return None;
        }

        let (Some(best_bid), Some(best_ask)) = (self.best_bid_price(), self.best_ask_price())
        else {
            return None;
        };

        if best_bid <= best_ask {
            return None;
        }

        let mut removed_levels = Vec::new();
        let mut clear_bids = false;
        let mut clear_asks = false;

        match side {
            Some(OrderSide::Buy) => clear_bids = true,
            Some(OrderSide::Sell) => clear_asks = true,
            _ => {
                clear_bids = true;
                clear_asks = true;
            }
        }

        // Collect prices to remove for asks (prices <= best_bid)
        let mut ask_prices_to_remove = Vec::new();
        if clear_asks {
            for bp in self.asks.levels.keys() {
                if bp.value <= best_bid {
                    ask_prices_to_remove.push(*bp);
                } else {
                    break;
                }
            }
        }

        // Collect prices to remove for bids (prices >= best_ask)
        let mut bid_prices_to_remove = Vec::new();
        if clear_bids {
            for bp in self.bids.levels.keys() {
                if bp.value >= best_ask {
                    bid_prices_to_remove.push(*bp);
                } else {
                    break;
                }
            }
        }

        if ask_prices_to_remove.is_empty() && bid_prices_to_remove.is_empty() {
            return None;
        }

        let bid_count = bid_prices_to_remove.len();
        let ask_count = ask_prices_to_remove.len();

        // Remove and collect bid levels
        for price in bid_prices_to_remove {
            if let Some(level) = self.bids.remove_level(price) {
                removed_levels.push(level);
            }
        }

        // Remove and collect ask levels
        for price in ask_prices_to_remove {
            if let Some(level) = self.asks.remove_level(price) {
                removed_levels.push(level);
            }
        }

        self.increment(self.sequence, self.ts_last);

        if removed_levels.is_empty() {
            None
        } else {
            let total_orders: usize = removed_levels.iter().map(|level| level.orders.len()).sum();

            log::warn!(
                "Removed {} stale/crossed levels (instrument_id={}, bid_levels={}, ask_levels={}, total_orders={}), book was crossed with best_bid={} > best_ask={}",
                removed_levels.len(),
                self.instrument_id,
                bid_count,
                ask_count,
                total_orders,
                best_bid,
                best_ask
            );

            Some(removed_levels)
        }
    }

    /// Applies a single order book delta operation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The delta's instrument ID does not match this book's instrument ID.
    /// - An `Add` is given with `NoOrderSide` (either explicitly or because the cache lookup failed).
    /// - After resolution the delta still has `NoOrderSide` but its action is not `Clear`.
    pub fn apply_delta(&mut self, delta: &OrderBookDelta) -> Result<(), BookIntegrityError> {
        if delta.instrument_id != self.instrument_id {
            return Err(BookIntegrityError::InstrumentMismatch(
                self.instrument_id,
                delta.instrument_id,
            ));
        }
        self.apply_delta_unchecked(delta)
    }

    /// Applies a single order book delta operation without instrument ID validation.
    ///
    /// "Unchecked" refers only to skipping the instrument ID match - other validations
    /// still apply and errors are still returned. This exists because `Ustr` interning
    /// is not shared across FFI boundaries, causing pointer-based equality to fail even
    /// when string values match. This limitation may be resolved in a future version.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - An `Add` is given with `NoOrderSide` (either explicitly or because the cache lookup failed).
    /// - After resolution the delta still has `NoOrderSide` but its action is not `Clear`.
    pub fn apply_delta_unchecked(
        &mut self,
        delta: &OrderBookDelta,
    ) -> Result<(), BookIntegrityError> {
        let mut order = delta.order;

        if order.side == OrderSide::NoOrderSide && order.order_id != 0 {
            match self.resolve_no_side_order(order) {
                Ok(resolved) => order = resolved,
                Err(BookIntegrityError::OrderNotFoundForSideResolution(order_id)) => {
                    match delta.action {
                        BookAction::Add => return Err(BookIntegrityError::NoOrderSide),
                        BookAction::Update | BookAction::Delete => {
                            // Already consistent
                            log::debug!(
                                "Skipping {:?} for unknown order_id={order_id}",
                                delta.action
                            );
                            return Ok(());
                        }
                        BookAction::Clear => {} // Won't hit this (order_id != 0)
                    }
                }
                Err(e) => return Err(e),
            }
        }

        if order.side == OrderSide::NoOrderSide && delta.action != BookAction::Clear {
            return Err(BookIntegrityError::NoOrderSide);
        }

        let flags = delta.flags;
        let sequence = delta.sequence;
        let ts_event = delta.ts_event;

        match delta.action {
            BookAction::Add => self.add(order, flags, sequence, ts_event),
            BookAction::Update => self.update(order, flags, sequence, ts_event),
            BookAction::Delete => self.delete(order, flags, sequence, ts_event),
            BookAction::Clear => self.clear(sequence, ts_event),
        }

        Ok(())
    }

    /// Applies multiple order book delta operations.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The deltas' instrument ID does not match this book's instrument ID.
    /// - Any individual delta application fails (see [`Self::apply_delta`]).
    pub fn apply_deltas(&mut self, deltas: &OrderBookDeltas) -> Result<(), BookIntegrityError> {
        if deltas.instrument_id != self.instrument_id {
            return Err(BookIntegrityError::InstrumentMismatch(
                self.instrument_id,
                deltas.instrument_id,
            ));
        }
        self.apply_deltas_unchecked(deltas)
    }

    /// Applies multiple order book delta operations without instrument ID validation.
    ///
    /// See [`Self::apply_delta_unchecked`] for details on why this function exists.
    ///
    /// # Errors
    ///
    /// Returns an error if any individual delta application fails.
    pub fn apply_deltas_unchecked(
        &mut self,
        deltas: &OrderBookDeltas,
    ) -> Result<(), BookIntegrityError> {
        for delta in &deltas.deltas {
            self.apply_delta_unchecked(delta)?;
        }
        Ok(())
    }

    /// Replaces current book state with a depth snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the depth's instrument ID does not match this book's instrument ID.
    pub fn apply_depth(&mut self, depth: &OrderBookDepth10) -> Result<(), BookIntegrityError> {
        if depth.instrument_id != self.instrument_id {
            return Err(BookIntegrityError::InstrumentMismatch(
                self.instrument_id,
                depth.instrument_id,
            ));
        }
        self.apply_depth_unchecked(depth)
    }

    /// Replaces current book state with a depth snapshot without instrument ID validation.
    ///
    /// See [`Self::apply_delta_unchecked`] for details on why this function exists.
    ///
    /// # Errors
    ///
    /// This function currently does not return errors, but returns `Result` for API consistency.
    pub fn apply_depth_unchecked(
        &mut self,
        depth: &OrderBookDepth10,
    ) -> Result<(), BookIntegrityError> {
        self.bids.clear();
        self.asks.clear();

        for order in depth.bids {
            // Skip padding entries
            if order.side == OrderSide::NoOrderSide || !order.size.is_positive() {
                continue;
            }

            debug_assert_eq!(
                order.side,
                OrderSide::Buy,
                "Bid order must have Buy side, was {:?}",
                order.side
            );

            let order = pre_process_order(self.book_type, order, depth.flags);
            self.bids.add(order, depth.flags);
        }

        for order in depth.asks {
            // Skip padding entries
            if order.side == OrderSide::NoOrderSide || !order.size.is_positive() {
                continue;
            }

            debug_assert_eq!(
                order.side,
                OrderSide::Sell,
                "Ask order must have Sell side, was {:?}",
                order.side
            );

            let order = pre_process_order(self.book_type, order, depth.flags);
            self.asks.add(order, depth.flags);
        }

        self.increment(depth.sequence, depth.ts_event);

        Ok(())
    }

    fn resolve_no_side_order(&self, mut order: BookOrder) -> Result<BookOrder, BookIntegrityError> {
        let resolved_side = self
            .bids
            .cache
            .get(&order.order_id)
            .or_else(|| self.asks.cache.get(&order.order_id))
            .map(|book_price| match book_price.side {
                OrderSideSpecified::Buy => OrderSide::Buy,
                OrderSideSpecified::Sell => OrderSide::Sell,
            })
            .ok_or(BookIntegrityError::OrderNotFoundForSideResolution(
                order.order_id,
            ))?;

        order.side = resolved_side;

        Ok(order)
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
        status: Option<AHashSet<OrderStatus>>,
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
                own_book.bid_quantity(status, None, None, accepted_buffer_ns, now),
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
        status: Option<AHashSet<OrderStatus>>,
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
                own_book.ask_quantity(status, None, None, accepted_buffer_ns, now),
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
        status: Option<AHashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let mut public_map = group_levels(self.bids(None), group_size, depth, true);

        if let Some(own_book) = own_book {
            filter_quantities(
                &mut public_map,
                own_book.bid_quantity(status, depth, Some(group_size), accepted_buffer_ns, now),
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
        status: Option<AHashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let mut public_map = group_levels(self.asks(None), group_size, depth, false);

        if let Some(own_book) = own_book {
            filter_quantities(
                &mut public_map,
                own_book.ask_quantity(status, depth, Some(group_size), accepted_buffer_ns, now),
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

    /// Returns the cumulative quantity available at or better than the specified price.
    ///
    /// For a BUY order, sums ask levels at or below the price.
    /// For a SELL order, sums bid levels at or above the price.
    #[must_use]
    pub fn get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        let side = order_side.as_specified();
        let levels = match side {
            OrderSideSpecified::Buy => &self.asks.levels,
            OrderSideSpecified::Sell => &self.bids.levels,
        };

        analysis::get_quantity_for_price(price, side, levels)
    }

    /// Returns the quantity at a specific price level only, or 0 if no level exists.
    ///
    /// Unlike `get_quantity_for_price` which returns cumulative quantity across
    /// multiple levels, this returns only the quantity at the exact price level.
    #[must_use]
    pub fn get_quantity_at_level(
        &self,
        price: Price,
        order_side: OrderSide,
        size_precision: u8,
    ) -> Quantity {
        let side = order_side.as_specified();

        // For a BUY order, we look in asks (sell side); for SELL order, we look in bids (buy side)
        // BookPrice keys use the side of orders IN the book, not the incoming order side
        let (levels, book_side) = match side {
            OrderSideSpecified::Buy => (&self.asks.levels, OrderSideSpecified::Sell),
            OrderSideSpecified::Sell => (&self.bids.levels, OrderSideSpecified::Buy),
        };

        let book_price = BookPrice::new(price, book_side);

        levels
            .get(&book_price)
            .map_or(Quantity::zero(size_precision), |level| {
                Quantity::from_raw(level.size_raw(), size_precision)
            })
    }

    /// Simulates fills for an order, returning list of (price, quantity) tuples.
    #[must_use]
    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        match order.side.as_specified() {
            OrderSideSpecified::Buy => self.asks.simulate_fills(order),
            OrderSideSpecified::Sell => self.bids.simulate_fills(order),
        }
    }

    /// Returns all price levels crossed by an order at the given price and side.
    ///
    /// Unlike `simulate_fills`, this returns ALL crossed levels regardless of
    /// order quantity. Used when liquidity consumption tracking needs visibility
    /// into all available levels.
    #[must_use]
    pub fn get_all_crossed_levels(
        &self,
        order_side: OrderSide,
        price: Price,
        size_precision: u8,
    ) -> Vec<(Price, Quantity)> {
        let side = order_side.as_specified();
        let levels = match side {
            OrderSideSpecified::Buy => &self.asks.levels,
            OrderSideSpecified::Sell => &self.bids.levels,
        };

        analysis::get_levels_for_price(price, side, levels, size_precision)
    }

    /// Return a formatted string representation of the order book.
    #[must_use]
    pub fn pprint(&self, num_levels: usize, group_size: Option<Decimal>) -> String {
        pprint_book(self, num_levels, group_size)
    }

    fn increment(&mut self, sequence: u64, ts_event: UnixNanos) {
        // Critical invariant checks: panic in debug, warn in release
        if sequence < self.sequence {
            let msg = format!(
                "Sequence number should not go backwards: old={}, new={}",
                self.sequence, sequence
            );
            debug_assert!(sequence >= self.sequence, "{}", msg);
            log::warn!("{msg}");
        }

        if ts_event < self.ts_last {
            let msg = format!(
                "Timestamp should not go backwards: old={}, new={}",
                self.ts_last, ts_event
            );
            debug_assert!(ts_event >= self.ts_last, "{}", msg);
            log::warn!("{msg}");
        }

        if self.update_count == u64::MAX {
            // Debug assert to catch in development
            debug_assert!(
                self.update_count < u64::MAX,
                "Update count at u64::MAX limit (about to overflow): {}",
                self.update_count
            );

            // Spam warnings in production when at/near u64::MAX
            log::warn!(
                "Update count at u64::MAX: {} (instrument_id={})",
                self.update_count,
                self.instrument_id
            );
        }

        self.sequence = sequence;
        self.ts_last = ts_event;
        self.update_count = self.update_count.saturating_add(1);
    }

    /// Updates L1 book state from a quote tick. Only valid for L1_MBP book type.
    ///
    /// # Errors
    ///
    /// Returns an error if the book type is not `L1_MBP`.
    pub fn update_quote_tick(&mut self, quote: &QuoteTick) -> Result<(), InvalidBookOperation> {
        if self.book_type != BookType::L1_MBP {
            return Err(InvalidBookOperation::Update(self.book_type));
        }

        // Crossed quotes (bid > ask) can occur temporarily in volatile markets
        if cfg!(debug_assertions) && quote.bid_price > quote.ask_price {
            log::warn!(
                "Quote has crossed prices: bid={}, ask={} for {}",
                quote.bid_price,
                quote.ask_price,
                self.instrument_id
            );
        }

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

        self.increment(self.sequence.saturating_add(1), quote.ts_event);

        Ok(())
    }

    /// Updates L1 book state from a trade tick. Only valid for L1_MBP book type.
    ///
    /// # Errors
    ///
    /// Returns an error if the book type is not `L1_MBP`.
    pub fn update_trade_tick(&mut self, trade: &TradeTick) -> Result<(), InvalidBookOperation> {
        if self.book_type != BookType::L1_MBP {
            return Err(InvalidBookOperation::Update(self.book_type));
        }

        // Prices can be zero or negative for certain instruments (options, spreads)
        debug_assert!(
            trade.price.raw != PRICE_UNDEF && trade.price.raw != PRICE_ERROR,
            "Trade has invalid/uninitialized price: {}",
            trade.price
        );

        // TradeTick enforces positive size at construction, but assert as sanity check
        debug_assert!(
            trade.size.is_positive(),
            "Trade has non-positive size: {}",
            trade.size
        );

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

        self.increment(self.sequence.saturating_add(1), trade.ts_event);

        Ok(())
    }

    fn update_book_bid(&mut self, order: BookOrder, ts_event: UnixNanos) {
        if let Some(top_bids) = self.bids.top()
            && let Some(top_bid) = top_bids.first()
        {
            self.bids.remove_order(top_bid.order_id, 0, ts_event);
        }
        self.bids.add(order, 0); // Internal replacement, no F_MBP flags
    }

    fn update_book_ask(&mut self, order: BookOrder, ts_event: UnixNanos) {
        if let Some(top_asks) = self.asks.top()
            && let Some(top_ask) = top_asks.first()
        {
            self.asks.remove_order(top_ask.order_id, 0, ts_event);
        }
        self.asks.add(order, 0); // Internal replacement, no F_MBP flags
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
    if group_size <= Decimal::ZERO {
        log::error!("Invalid group_size: {group_size}, must be positive; returning empty map");
        return IndexMap::new();
    }

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
