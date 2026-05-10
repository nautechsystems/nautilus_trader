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

use ahash::AHashSet;
use indexmap::IndexMap;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::prelude::*;
use rust_decimal::Decimal;

use crate::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick},
    enums::{BookType, OrderSide, OrderStatus},
    identifiers::InstrumentId,
    orderbook::{BookLevel, OrderBook, analysis::book_check_integrity, own::OwnOrderBook},
    types::{Price, Quantity},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OrderBook {
    /// Provides a high-performance, versatile order book.
    ///
    /// Maintains buy (bid) and sell (ask) orders in price-time priority, supporting multiple
    /// market data formats:
    /// - L3 (MBO): Market By Order - tracks individual orders with unique IDs.
    /// - L2 (MBP): Market By Price - aggregates orders at each price level.
    /// - L1 (MBP): Top-of-Book - maintains only the best bid and ask prices.
    #[new]
    fn py_new(instrument_id: InstrumentId, book_type: BookType) -> Self {
        Self::new(instrument_id, book_type)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "book_type")]
    fn py_book_type(&self) -> BookType {
        self.book_type
    }

    #[getter]
    #[pyo3(name = "sequence")]
    fn py_sequence(&self) -> u64 {
        self.sequence
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_last.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_last.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_last")]
    fn py_ts_last(&self) -> u64 {
        self.ts_last.as_u64()
    }

    #[getter]
    #[pyo3(name = "update_count")]
    fn py_update_count(&self) -> u64 {
        self.update_count
    }

    /// Resets the order book to its initial empty state.
    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    /// Adds an order to the book after preprocessing based on book type.
    #[pyo3(name = "add")]
    #[pyo3(signature = (order, flags, sequence, ts_event))]
    fn py_add(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: u64) {
        self.add(order, flags, sequence, ts_event.into());
    }

    /// Updates an existing order in the book after preprocessing based on book type.
    #[pyo3(name = "update")]
    #[pyo3(signature = (order, flags, sequence, ts_event))]
    fn py_update(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: u64) {
        self.update(order, flags, sequence, ts_event.into());
    }

    /// Deletes an order from the book after preprocessing based on book type.
    #[pyo3(name = "delete")]
    #[pyo3(signature = (order, flags, sequence, ts_event))]
    fn py_delete(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: u64) {
        self.delete(order, flags, sequence, ts_event.into());
    }

    /// Clears all orders from both sides of the book.
    #[pyo3(name = "clear")]
    #[pyo3(signature = (sequence, ts_event))]
    fn py_clear(&mut self, sequence: u64, ts_event: u64) {
        self.clear(sequence, ts_event.into());
    }

    /// Clears all bid orders from the book.
    #[pyo3(name = "clear_bids")]
    #[pyo3(signature = (sequence, ts_event))]
    fn py_clear_bids(&mut self, sequence: u64, ts_event: u64) {
        self.clear_bids(sequence, ts_event.into());
    }

    /// Clears all ask orders from the book.
    #[pyo3(name = "clear_asks")]
    #[pyo3(signature = (sequence, ts_event))]
    fn py_clear_asks(&mut self, sequence: u64, ts_event: u64) {
        self.clear_asks(sequence, ts_event.into());
    }

    /// Removes overlapped bid/ask levels when the book is strictly crossed (best bid > best ask)
    ///
    /// - Acts only when both sides exist and the book is crossed.
    /// - Deletes by removing whole price levels via the ladder API to preserve invariants.
    /// - `side=None` or `NoOrderSide` clears both overlapped ranges (conservative, may widen spread).
    /// - `side=Buy` clears crossed bids only; side=Sell clears crossed asks only.
    /// - Returns removed price levels (crossed bids first, then crossed asks), or None if nothing removed.
    #[pyo3(name = "clear_stale_levels")]
    #[pyo3(signature = (side=None))]
    fn py_clear_stale_levels(&mut self, side: Option<OrderSide>) -> Option<Vec<BookLevel>> {
        self.clear_stale_levels(side)
    }

    /// Applies a single order book delta operation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The delta's instrument ID does not match this book's instrument ID.
    /// - An `Add` is given with `NoOrderSide` (either explicitly or because the cache lookup failed).
    /// - After resolution the delta still has `NoOrderSide` but its action is not `Clear`.
    #[pyo3(name = "apply_delta")]
    fn py_apply_delta(&mut self, delta: &OrderBookDelta) -> PyResult<()> {
        self.apply_delta_unchecked(delta).map_err(to_pyruntime_err)
    }

    /// Applies multiple order book delta operations.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The deltas' instrument ID does not match this book's instrument ID.
    /// - Any individual delta application fails (see `Self.apply_delta`).
    #[pyo3(name = "apply_deltas")]
    fn py_apply_deltas(&mut self, deltas: &OrderBookDeltas) -> PyResult<()> {
        self.apply_deltas_unchecked(deltas)
            .map_err(to_pyruntime_err)
    }

    /// Replaces current book state with a depth snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the depth's instrument ID does not match this book's instrument ID.
    #[pyo3(name = "apply_depth")]
    fn py_apply_depth(&mut self, depth: &OrderBookDepth10) -> PyResult<()> {
        self.apply_depth_unchecked(depth).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "check_integrity")]
    fn py_check_integrity(&mut self) -> PyResult<()> {
        book_check_integrity(self).map_err(to_pyruntime_err)
    }

    /// Returns an iterator over bid price levels.
    #[pyo3(name = "bids")]
    #[pyo3(signature = (depth=None))]
    fn py_bids(&self, depth: Option<usize>) -> Vec<BookLevel> {
        self.bids(depth)
            .map(|level_ref| (*level_ref).clone())
            .collect()
    }

    /// Returns an iterator over ask price levels.
    #[pyo3(name = "asks")]
    #[pyo3(signature = (depth=None))]
    fn py_asks(&self, depth: Option<usize>) -> Vec<BookLevel> {
        self.asks(depth)
            .map(|level_ref| (*level_ref).clone())
            .collect()
    }

    #[pyo3(name = "bids_to_dict")]
    #[pyo3(signature = (depth=None))]
    fn py_bids_to_dict(&self, depth: Option<usize>) -> IndexMap<Decimal, Decimal> {
        self.bids_as_map(depth)
    }

    #[pyo3(name = "asks_to_dict")]
    #[pyo3(signature = (depth=None))]
    fn py_asks_to_dict(&self, depth: Option<usize>) -> IndexMap<Decimal, Decimal> {
        self.asks_as_map(depth)
    }

    /// Groups bid quantities by price into buckets, limited by depth.
    #[pyo3(name = "group_bids")]
    #[pyo3(signature = (group_size, depth=None))]
    #[must_use]
    pub fn py_group_bids(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
    ) -> IndexMap<Decimal, Decimal> {
        self.group_bids(group_size, depth)
    }

    /// Groups ask quantities by price into buckets, limited by depth.
    #[pyo3(name = "group_asks")]
    #[pyo3(signature = (group_size, depth=None))]
    #[must_use]
    pub fn py_group_asks(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
    ) -> IndexMap<Decimal, Decimal> {
        self.group_asks(group_size, depth)
    }

    #[pyo3(name = "bids_filtered_to_dict")]
    #[pyo3(signature = (depth=None, own_book=None, status=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_bids_filtered_to_dict(
        &self,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<std::collections::HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let status_set: Option<AHashSet<OrderStatus>> = status.map(|s| s.into_iter().collect());
        self.bids_filtered_as_map(
            depth,
            own_book,
            status_set.as_ref(),
            accepted_buffer_ns,
            ts_now,
        )
    }

    #[pyo3(name = "asks_filtered_to_dict")]
    #[pyo3(signature = (depth=None, own_book=None, status=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_asks_filtered_to_dict(
        &self,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<std::collections::HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let status_set: Option<AHashSet<OrderStatus>> = status.map(|s| s.into_iter().collect());
        self.asks_filtered_as_map(
            depth,
            own_book,
            status_set.as_ref(),
            accepted_buffer_ns,
            ts_now,
        )
    }

    #[pyo3(name = "group_bids_filtered")]
    #[pyo3(signature = (group_size, depth=None, own_book=None, status=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_group_bids_filered(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<std::collections::HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let status_set: Option<AHashSet<OrderStatus>> = status.map(|s| s.into_iter().collect());
        self.group_bids_filtered(
            group_size,
            depth,
            own_book,
            status_set.as_ref(),
            accepted_buffer_ns,
            ts_now,
        )
    }

    /// Groups ask quantities into price buckets, truncating to a maximum depth, excluding own orders.
    ///
    /// With `own_book`, subtracts own order sizes, filtered by `status` if provided.
    /// Uses `accepted_buffer_ns` to include only orders accepted at least that many
    /// nanoseconds before `now` (defaults to now).
    #[pyo3(name = "group_asks_filtered")]
    #[pyo3(signature = (group_size, depth=None, own_book=None, status=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_group_asks_filtered(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
        own_book: Option<&OwnOrderBook>,
        status: Option<std::collections::HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let status_set: Option<AHashSet<OrderStatus>> = status.map(|s| s.into_iter().collect());
        self.group_asks_filtered(
            group_size,
            depth,
            own_book,
            status_set.as_ref(),
            accepted_buffer_ns,
            ts_now,
        )
    }

    /// Returns a filtered `OrderBook` view with own sizes subtracted from public levels.
    #[pyo3(name = "filtered_view")]
    #[pyo3(signature = (own_book=None, depth=None, status=None, accepted_buffer_ns=None, ts_now=None))]
    fn py_filtered_view(
        &self,
        own_book: Option<&OwnOrderBook>,
        depth: Option<usize>,
        status: Option<std::collections::HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> PyResult<Self> {
        let status_set: Option<AHashSet<OrderStatus>> = status.map(|s| s.into_iter().collect());
        self.filtered_view_checked(
            own_book,
            depth,
            status_set.as_ref(),
            accepted_buffer_ns,
            ts_now,
        )
        .map_err(to_pyvalue_err)
    }

    /// Returns the best bid price if available.
    #[pyo3(name = "best_bid_price")]
    fn py_best_bid_price(&self) -> Option<Price> {
        self.best_bid_price()
    }

    /// Returns the best ask price if available.
    #[pyo3(name = "best_ask_price")]
    fn py_best_ask_price(&self) -> Option<Price> {
        self.best_ask_price()
    }

    /// Returns the size at the best bid price if available.
    #[pyo3(name = "best_bid_size")]
    fn py_best_bid_size(&self) -> Option<Quantity> {
        self.best_bid_size()
    }

    /// Returns the size at the best ask price if available.
    #[pyo3(name = "best_ask_size")]
    fn py_best_ask_size(&self) -> Option<Quantity> {
        self.best_ask_size()
    }

    /// Returns the spread between best ask and bid prices if both exist.
    #[pyo3(name = "spread")]
    fn py_spread(&self) -> Option<f64> {
        self.spread()
    }

    /// Returns the midpoint between best ask and bid prices if both exist.
    #[pyo3(name = "midpoint")]
    fn py_midpoint(&self) -> Option<f64> {
        self.midpoint()
    }

    /// Calculates the average price to fill the specified quantity.
    #[pyo3(name = "get_avg_px_for_quantity")]
    fn py_get_avg_px_for_quantity(&self, qty: Quantity, order_side: OrderSide) -> f64 {
        self.get_avg_px_for_quantity(qty, order_side)
    }

    /// Calculates the worst (last-touched) price to fill the specified quantity.
    #[pyo3(name = "get_worst_px_for_quantity")]
    fn py_get_worst_px_for_quantity(&self, qty: Quantity, order_side: OrderSide) -> Option<Price> {
        self.get_worst_px_for_quantity(qty, order_side)
    }

    /// Calculates average price and quantity for target exposure. Returns (price, quantity, `executed_exposure`).
    #[pyo3(name = "get_avg_px_qty_for_exposure")]
    fn py_get_avg_px_qty_for_exposure(
        &self,
        qty: Quantity,
        order_side: OrderSide,
    ) -> (f64, f64, f64) {
        self.get_avg_px_qty_for_exposure(qty, order_side)
    }

    /// Returns the cumulative quantity available at or better than the specified price.
    ///
    /// For a BUY order, sums ask levels at or below the price.
    /// For a SELL order, sums bid levels at or above the price.
    #[pyo3(name = "get_quantity_for_price")]
    fn py_get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        self.get_quantity_for_price(price, order_side)
    }

    /// Returns the quantity at a specific price level only, or 0 if no level exists.
    ///
    /// Unlike `get_quantity_for_price` which returns cumulative quantity across
    /// multiple levels, this returns only the quantity at the exact price level.
    #[pyo3(name = "get_quantity_at_level")]
    fn py_get_quantity_at_level(
        &self,
        price: Price,
        order_side: OrderSide,
        size_precision: u8,
    ) -> Quantity {
        self.get_quantity_at_level(price, order_side, size_precision)
    }

    /// Simulates fills for an order, returning list of (price, quantity) tuples.
    #[pyo3(name = "simulate_fills")]
    fn py_simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        self.simulate_fills(order)
    }

    /// Return a formatted string representation of the order book.
    #[pyo3(name = "pprint")]
    #[pyo3(signature = (num_levels=3, group_size=None))]
    fn py_pprint(&self, num_levels: usize, group_size: Option<Decimal>) -> String {
        self.pprint(num_levels, group_size)
    }
}

/// Updates the `OrderBook` with a [`QuoteTick`].
///
/// # Errors
///
/// Returns a `PyErr` if the update operation fails.
#[pyfunction()]
#[pyo3(name = "update_book_with_quote_tick")]
pub fn py_update_book_with_quote_tick(book: &mut OrderBook, quote: &QuoteTick) -> PyResult<()> {
    book.update_quote_tick(quote).map_err(to_pyvalue_err)
}

/// Updates the `OrderBook` with a [`TradeTick`].
///
/// # Errors
///
/// Returns a `PyErr` if the update operation fails.
#[pyfunction()]
#[pyo3(name = "update_book_with_trade_tick")]
pub fn py_update_book_with_trade_tick(book: &mut OrderBook, trade: &TradeTick) -> PyResult<()> {
    book.update_trade_tick(trade).map_err(to_pyvalue_err)
}
