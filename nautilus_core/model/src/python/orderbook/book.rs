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

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::prelude::*;

use crate::{
    data::{
        delta::OrderBookDelta, deltas::OrderBookDeltas, depth::OrderBookDepth10, order::BookOrder,
        quote::QuoteTick, trade::TradeTick,
    },
    enums::{BookType, OrderSide},
    identifiers::instrument_id::InstrumentId,
    orderbook::{
        aggregation::{update_book_with_quote_tick, update_book_with_trade_tick},
        analysis::book_check_integrity,
        book::OrderBook,
        level::Level,
    },
    types::{price::Price, quantity::Quantity},
};

#[pymethods]
impl OrderBook {
    #[new]
    fn py_new(book_type: BookType, instrument_id: InstrumentId) -> Self {
        Self::new(book_type, instrument_id)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        // TODO: Return debug string for now
        format!("{self:?}")
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
    #[pyo3(name = "count")]
    fn py_count(&self) -> u64 {
        self.count
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.reset();
    }

    #[pyo3(signature = (order, flags, sequence, ts_event))]
    #[pyo3(name = "add")]
    fn py_add(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: u64) {
        self.add(order, flags, sequence, ts_event.into());
    }

    #[pyo3(signature = (order, flags, sequence, ts_event))]
    #[pyo3(name = "update")]
    fn py_update(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: u64) {
        self.update(order, flags, sequence, ts_event.into());
    }

    #[pyo3(signature = (order, flags, sequence, ts_event))]
    #[pyo3(name = "delete")]
    fn py_delete(&mut self, order: BookOrder, flags: u8, sequence: u64, ts_event: u64) {
        self.delete(order, flags, sequence, ts_event.into());
    }

    #[pyo3(signature = (sequence, ts_event))]
    #[pyo3(name = "clear")]
    fn py_clear(&mut self, sequence: u64, ts_event: u64) {
        self.clear(sequence, ts_event.into());
    }

    #[pyo3(signature = (sequence, ts_event))]
    #[pyo3(name = "clear_bids")]
    fn py_clear_bids(&mut self, sequence: u64, ts_event: u64) {
        self.clear_bids(sequence, ts_event.into());
    }

    #[pyo3(signature = (sequence, ts_event))]
    #[pyo3(name = "clear_asks")]
    fn py_clear_asks(&mut self, sequence: u64, ts_event: u64) {
        self.clear_asks(sequence, ts_event.into());
    }

    #[pyo3(name = "apply_delta")]
    fn py_apply_delta(&mut self, delta: OrderBookDelta) {
        self.apply_delta(delta);
    }

    #[pyo3(name = "apply_deltas")]
    fn py_apply_deltas(&mut self, deltas: OrderBookDeltas) {
        self.apply_deltas(deltas);
    }

    #[pyo3(name = "apply_depth")]
    fn py_apply_depth(&mut self, depth: OrderBookDepth10) {
        self.apply_depth(depth);
    }

    #[pyo3(name = "check_integrity")]
    fn py_check_integrity(&mut self) -> PyResult<()> {
        book_check_integrity(self).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "bids")]
    fn py_bids(&self) -> Vec<Level> {
        // TODO: Improve efficiency
        self.bids().map(|level_ref| (*level_ref).clone()).collect()
    }

    #[pyo3(name = "asks")]
    fn py_asks(&self) -> Vec<Level> {
        // TODO: Improve efficiency
        self.asks().map(|level_ref| (*level_ref).clone()).collect()
    }

    #[pyo3(name = "best_bid_price")]
    fn py_best_bid_price(&self) -> Option<Price> {
        self.best_bid_price()
    }

    #[pyo3(name = "best_ask_price")]
    fn py_best_ask_price(&self) -> Option<Price> {
        self.best_ask_price()
    }

    #[pyo3(name = "best_bid_size")]
    fn py_best_bid_size(&self) -> Option<Quantity> {
        self.best_bid_size()
    }

    #[pyo3(name = "best_ask_size")]
    fn py_best_ask_size(&self) -> Option<Quantity> {
        self.best_ask_size()
    }

    #[pyo3(name = "spread")]
    fn py_spread(&self) -> Option<f64> {
        self.spread()
    }

    #[pyo3(name = "midpoint")]
    fn py_midpoint(&self) -> Option<f64> {
        self.midpoint()
    }

    #[pyo3(name = "get_avg_px_for_quantity")]
    fn py_get_avg_px_for_quantity(&self, qty: Quantity, order_side: OrderSide) -> f64 {
        self.get_avg_px_for_quantity(qty, order_side)
    }

    #[pyo3(name = "get_quantity_for_price")]
    fn py_get_quantity_for_price(&self, price: Price, order_side: OrderSide) -> f64 {
        self.get_quantity_for_price(price, order_side)
    }

    #[pyo3(name = "simulate_fills")]
    fn py_simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        self.simulate_fills(order)
    }

    #[pyo3(name = "pprint")]
    fn py_pprint(&self, num_levels: usize) -> String {
        self.pprint(num_levels)
    }
}

#[pyfunction()]
#[pyo3(name = "update_book_with_quote_tick")]
pub fn py_update_book_with_quote_tick(book: &mut OrderBook, quote: &QuoteTick) -> PyResult<()> {
    update_book_with_quote_tick(book, quote).map_err(to_pyvalue_err)
}

#[pyfunction()]
#[pyo3(name = "update_book_with_trade_tick")]
pub fn py_update_book_with_trade_tick(book: &mut OrderBook, trade: &TradeTick) -> PyResult<()> {
    update_book_with_trade_tick(book, trade).map_err(to_pyvalue_err)
}
