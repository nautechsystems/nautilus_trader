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

use nautilus_core::python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3};
use pyo3::{
    basic::CompareOp,
    prelude::*,
    types::{PyDict, PyList},
};
use rust_decimal::prelude::ToPrimitive;

use super::common::commissions_from_vec;
use crate::{
    enums::{OrderSide, PositionSide},
    events::OrderFilled,
    identifiers::{
        ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    position::Position,
    python::instruments::pyobject_to_instrument_any,
    types::{Currency, Money, Price, Quantity},
};

#[pymethods]
impl Position {
    #[new]
    fn py_new(py: Python, instrument: Py<PyAny>, fill: OrderFilled) -> PyResult<Self> {
        let instrument_any = pyobject_to_instrument_any(py, instrument)?;
        Ok(Self::new(&instrument_any, fill))
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        self.to_string()
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    fn py_strategy_id(&self) -> StrategyId {
        self.strategy_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> PositionId {
        self.id
    }

    #[getter]
    #[pyo3(name = "symbol")]
    fn py_symbol(&self) -> Symbol {
        self.symbol()
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue()
    }

    #[getter]
    #[pyo3(name = "opening_order_id")]
    fn py_opening_order_id(&self) -> ClientOrderId {
        self.opening_order_id
    }

    #[getter]
    #[pyo3(name = "closing_order_id")]
    fn py_closing_order_id(&self) -> Option<ClientOrderId> {
        self.closing_order_id
    }

    #[getter]
    #[pyo3(name = "entry")]
    fn py_entry(&self) -> OrderSide {
        self.entry
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> PositionSide {
        self.side
    }

    #[getter]
    #[pyo3(name = "signed_qty")]
    fn py_signed_qty(&self) -> f64 {
        self.signed_qty
    }

    #[getter]
    #[pyo3(name = "quantity")]
    fn py_quantity(&self) -> Quantity {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "peak_qty")]
    fn py_peak_qty(&self) -> Quantity {
        self.peak_qty
    }

    #[getter]
    #[pyo3(name = "price_precision")]
    fn py_price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    #[pyo3(name = "size_precision")]
    fn py_size_precision(&self) -> u8 {
        self.size_precision
    }

    #[getter]
    #[pyo3(name = "multiplier")]
    fn py_multiplier(&self) -> Quantity {
        self.multiplier
    }

    #[getter]
    #[pyo3(name = "is_inverse")]
    fn py_is_inverse(&self) -> bool {
        self.is_inverse
    }

    #[getter]
    #[pyo3(name = "base_currency")]
    fn py_base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[getter]
    #[pyo3(name = "quote_currency")]
    fn py_quote_currency(&self) -> Currency {
        self.quote_currency
    }

    #[getter]
    #[pyo3(name = "settlement_currency")]
    fn py_settlement_currency(&self) -> Currency {
        self.settlement_currency
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_opened")]
    fn py_ts_opened(&self) -> u64 {
        self.ts_opened.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_closed")]
    fn py_ts_closed(&self) -> Option<u64> {
        self.ts_closed.map(std::convert::Into::into)
    }

    #[getter]
    #[pyo3(name = "duration_ns")]
    fn py_duration_ns(&self) -> u64 {
        self.duration_ns
    }

    #[getter]
    #[pyo3(name = "avg_px_open")]
    fn py_avg_px_open(&self) -> f64 {
        self.avg_px_open
    }

    #[getter]
    #[pyo3(name = "avg_px_close")]
    fn py_avg_px_close(&self) -> Option<f64> {
        self.avg_px_close
    }

    #[getter]
    #[pyo3(name = "realized_return")]
    fn py_realized_return(&self) -> f64 {
        self.realized_return
    }

    #[getter]
    #[pyo3(name = "realized_pnl")]
    fn py_realized_pnl(&self) -> Option<Money> {
        self.realized_pnl
    }

    #[getter]
    #[pyo3(name = "events")]
    fn py_events(&self) -> Vec<OrderFilled> {
        self.events.clone()
    }

    #[getter]
    #[pyo3(name = "client_order_ids")]
    fn py_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.client_order_ids()
    }

    #[getter]
    #[pyo3(name = "venue_order_ids")]
    fn py_venue_order_ids(&self) -> Vec<VenueOrderId> {
        self.venue_order_ids()
    }

    #[getter]
    #[pyo3(name = "trade_ids")]
    fn py_trade_ids(&self) -> Vec<TradeId> {
        self.trade_ids()
    }

    #[getter]
    #[pyo3(name = "last_event")]
    fn py_last_event(&self) -> Option<OrderFilled> {
        self.last_event()
    }

    #[getter]
    #[pyo3(name = "last_trade_id")]
    fn py_last_trade_id(&self) -> Option<TradeId> {
        self.last_trade_id()
    }

    #[getter]
    #[pyo3(name = "event_count")]
    fn py_event_count(&self) -> usize {
        self.events.len()
    }

    #[getter]
    #[pyo3(name = "is_open")]
    fn py_is_open(&self) -> bool {
        self.is_open()
    }

    #[getter]
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[getter]
    #[pyo3(name = "is_long")]
    fn py_is_long(&self) -> bool {
        self.is_long()
    }

    #[getter]
    #[pyo3(name = "is_short")]
    fn py_is_short(&self) -> bool {
        self.is_short()
    }

    #[pyo3(name = "unrealized_pnl")]
    fn py_unrealized_pnl(&self, last: Price) -> Money {
        self.unrealized_pnl(last)
    }

    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(&self, last: Price) -> Money {
        self.total_pnl(last)
    }

    #[pyo3(name = "commissions")]
    fn py_commissions(&self) -> Vec<Money> {
        self.commissions()
    }

    #[pyo3(name = "apply")]
    fn py_apply(&mut self, fill: &OrderFilled) {
        self.apply(fill);
    }

    #[pyo3(name = "is_opposite_side")]
    fn py_is_opposite_side(&self, side: OrderSide) -> bool {
        self.is_opposite_side(side)
    }

    #[pyo3(name = "calculate_pnl")]
    fn py_calculate_pnl(&self, avg_px_open: f64, avg_px_close: f64, quantity: Quantity) -> Money {
        self.calculate_pnl(avg_px_open, avg_px_close, quantity)
    }

    #[pyo3(name = "notional_value")]
    fn py_notional_value(&self, price: Price) -> Money {
        self.notional_value(price)
    }

    /// Constructs a [`Position`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if deserialization from the Python dict fails.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    /// Converts this [`Position`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if serialization into a Python dict fails.
    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(Position))?;
        let events_dict: PyResult<Vec<_>> = self.events.iter().map(|e| e.py_to_dict(py)).collect();
        dict.set_item("events", events_dict?)?;
        dict.set_item("trader_id", self.trader_id.to_string())?;
        dict.set_item("strategy_id", self.strategy_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("position_id", self.id.to_string())?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("opening_order_id", self.opening_order_id.to_string())?;
        match self.closing_order_id {
            Some(closing_order_id) => {
                dict.set_item("closing_order_id", closing_order_id.to_string())?;
            }
            None => dict.set_item("closing_order_id", py.None())?,
        }
        dict.set_item("entry", self.entry.to_string())?;
        dict.set_item("side", self.side.to_string())?;
        dict.set_item("signed_qty", self.signed_qty.to_f64())?;
        dict.set_item("quantity", self.quantity.to_string())?;
        dict.set_item("peak_qty", self.peak_qty.to_string())?;
        dict.set_item("price_precision", self.price_precision.to_u8())?;
        dict.set_item("size_precision", self.size_precision.to_u8())?;
        dict.set_item("multiplier", self.multiplier.to_string())?;
        dict.set_item("is_inverse", self.is_inverse)?;
        match self.base_currency {
            Some(base_currency) => {
                dict.set_item("base_currency", base_currency.code.to_string())?;
            }
            None => dict.set_item("base_currency", py.None())?,
        }
        dict.set_item("quote_currency", self.quote_currency.code.to_string())?;
        dict.set_item(
            "settlement_currency",
            self.settlement_currency.code.to_string(),
        )?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        dict.set_item("ts_opened", self.ts_opened.as_u64())?;
        dict.set_item("ts_last", self.ts_last.as_u64())?;
        match self.ts_closed {
            Some(ts_closed) => dict.set_item("ts_closed", ts_closed.as_u64())?,
            None => dict.set_item("ts_closed", py.None())?,
        }
        dict.set_item("duration_ns", self.duration_ns.to_u64())?;
        dict.set_item("avg_px_open", self.avg_px_open)?;
        match self.avg_px_close {
            Some(avg_px_close) => dict.set_item("avg_px_close", avg_px_close)?,
            None => dict.set_item("avg_px_close", py.None())?,
        }
        dict.set_item("realized_return", self.realized_return)?;
        match self.realized_pnl {
            Some(realized_pnl) => dict.set_item("realized_pnl", realized_pnl.to_string())?,
            None => dict.set_item("realized_pnl", py.None())?,
        }
        let venue_order_ids_list =
            PyList::new(py, self.venue_order_ids().iter().map(ToString::to_string))
                .expect("Invalid `ExactSizeIterator`");
        dict.set_item("venue_order_ids", venue_order_ids_list)?;
        let trade_ids_list = PyList::new(py, self.trade_ids.iter().map(ToString::to_string))
            .expect("Invalid `ExactSizeIterator`");
        dict.set_item("trade_ids", trade_ids_list)?;
        dict.set_item("buy_qty", self.buy_qty.to_string())?;
        dict.set_item("sell_qty", self.sell_qty.to_string())?;
        dict.set_item("commissions", commissions_from_vec(py, self.commissions())?)?;
        Ok(dict.into())
    }
}
