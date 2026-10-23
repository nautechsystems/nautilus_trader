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

use nautilus_core::python::{
    IntoPyObjectNautilusExt, correctness_error_to_pyvalue_err,
    serialization::{from_dict_pyo3, to_dict_pyo3},
    to_pyvalue_err,
};
use pyo3::{
    basic::CompareOp,
    prelude::*,
    types::{PyDict, PyList},
};
use rust_decimal::Decimal;

use crate::{
    enums::{InstrumentClass, OrderSide, PositionSide},
    events::{OrderFilled, PositionAdjusted},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, Symbol, TradeId, TraderId,
        Venue, VenueOrderId,
    },
    position::{self, Position},
    python::{common::commissions_from_vec, instruments::pyobject_to_instrument_any},
    types::{Currency, Money, Price, Quantity},
};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl Position {
    /// An open or closed position in a market.
    ///
    /// # Identity and Replay
    ///
    /// The venue may assign the position ID; the strategy's Order Management System (OMS) settings
    /// control system-generated IDs. Retained replay events and cumulative fill corrections preserve
    /// derived state across close and reopen cycles.
    ///
    /// # Accounting
    ///
    /// Financial calculations use checked domain and `Decimal` arithmetic. Decimal has a 96-bit
    /// coefficient and a maximum scale of 28. Input conversions reject precision loss and overflow;
    /// derived products, cost sums, division, and proportional intermediates use its finite precision.
    ///
    /// - Cost accumulators preserve the cost basis without reusing rounded average prices.
    /// - Partial reductions retain unallocated cost; the final reduction consumes it.
    /// - A sub-Money PnL remainder preserves fractional amounts between updates.
    ///
    /// `Money` results round midpoint to even at their native fixed-point scale. `realized_return`
    /// is a floating-point analytics projection and does not feed accounting.
    #[new]
    fn py_new(py: Python, instrument: Py<PyAny>, fill: OrderFilled) -> PyResult<Self> {
        let instrument_any = pyobject_to_instrument_any(py, instrument)?;
        Self::new_checked(&instrument_any, fill).map_err(correctness_error_to_pyvalue_err)
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
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> AccountId {
        self.account_id
    }

    /// Returns the instrument symbol.
    #[getter]
    #[pyo3(name = "symbol")]
    fn py_symbol(&self) -> Symbol {
        self.symbol()
    }

    /// Returns the trading venue.
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
    fn py_signed_qty(&self) -> Decimal {
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
    #[pyo3(name = "instrument_class")]
    fn py_instrument_class(&self) -> InstrumentClass {
        self.instrument_class
    }

    #[getter]
    #[pyo3(name = "is_spot_currency")]
    fn py_is_spot_currency(&self) -> bool {
        self.is_currency_pair
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
    #[pyo3(name = "ts_last")]
    fn py_ts_last(&self) -> u64 {
        self.ts_last.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_closed")]
    fn py_ts_closed(&self) -> Option<u64> {
        self.ts_closed.map(std::convert::Into::into)
    }

    #[getter]
    #[pyo3(name = "duration_ns")]
    fn py_duration_ns(&self) -> u64 {
        self.duration_ns.as_u64()
    }

    #[getter]
    #[pyo3(name = "avg_px_open")]
    fn py_avg_px_open(&self) -> Decimal {
        self.avg_px_open
    }

    #[getter]
    #[pyo3(name = "avg_px_close")]
    fn py_avg_px_close(&self) -> Option<Decimal> {
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

    #[pyo3(name = "events")]
    fn py_events(&self) -> Vec<OrderFilled> {
        self.events.clone()
    }

    #[pyo3(name = "adjustments")]
    fn py_adjustments(&self) -> Vec<PositionAdjusted> {
        self.adjustments.clone()
    }

    /// Returns unique client order IDs from all fill events, sorted.
    #[pyo3(name = "client_order_ids")]
    fn py_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.client_order_ids()
    }

    /// Returns unique venue order IDs from all fill events, sorted.
    #[pyo3(name = "venue_order_ids")]
    fn py_venue_order_ids(&self) -> Vec<VenueOrderId> {
        self.venue_order_ids()
    }

    /// Returns unique trade IDs from all fill events, sorted.
    #[pyo3(name = "trade_ids")]
    fn py_trade_ids(&self) -> Vec<TradeId> {
        self.trade_ids()
    }

    /// Returns the last `OrderFilled` event for the position (if any after purging).
    #[getter]
    #[pyo3(name = "last_event")]
    fn py_last_event(&self) -> Option<OrderFilled> {
        self.last_event()
    }

    /// Returns the last `TradeId` for the position (if any after purging).
    #[getter]
    #[pyo3(name = "last_trade_id")]
    fn py_last_trade_id(&self) -> Option<TradeId> {
        self.last_trade_id()
    }

    /// Returns the count of order fill events applied to this position.
    #[getter]
    #[pyo3(name = "event_count")]
    fn py_event_count(&self) -> usize {
        self.events.len()
    }

    /// Returns whether the position is currently open (has quantity and no close timestamp).
    #[getter]
    #[pyo3(name = "is_open")]
    fn py_is_open(&self) -> bool {
        self.is_open()
    }

    /// Returns whether the position is closed (flat with a close timestamp).
    #[getter]
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    /// Returns whether the position is long (positive quantity).
    #[getter]
    #[pyo3(name = "is_long")]
    fn py_is_long(&self) -> bool {
        self.is_long()
    }

    /// Returns whether the position is short (negative quantity).
    #[getter]
    #[pyo3(name = "is_short")]
    fn py_is_short(&self) -> bool {
        self.is_short()
    }

    /// Returns unrealized PnL at the given price.
    ///
    /// # Errors
    ///
    /// Returns an error if `Self.try_unrealized_pnl` fails.
    #[pyo3(name = "unrealized_pnl")]
    fn py_unrealized_pnl(&self, last: Price) -> PyResult<Money> {
        self.try_unrealized_pnl(last)
            .map_err(nautilus_core::python::to_pyvalue_err)
    }

    /// Returns total PnL at the given price.
    ///
    /// # Errors
    ///
    /// Returns an error if `Self.try_total_pnl` fails.
    #[pyo3(name = "total_pnl")]
    fn py_total_pnl(&self, last: Price) -> PyResult<Money> {
        self.try_total_pnl(last)
            .map_err(nautilus_core::python::to_pyvalue_err)
    }

    /// Returns the cumulative commissions for the position as a vector.
    #[pyo3(name = "commissions")]
    fn py_commissions(&self) -> Vec<Money> {
        self.commissions()
    }

    /// Applies an `OrderFilled` event to this position.
    ///
    /// A fill with the same `trade_id`, the same `client_order_id`, and a new price restates
    /// the open average when that trade is an opening fill in the current cycle. Quantity is
    /// unchanged. The original fill stays in `events`, so a void still matches its `last_px`.
    /// Applying the same `event_id` again does nothing. A different order that reuses the trade
    /// ID is rejected. A closing fill does not change the open average, and restating an
    /// opening fill does not change realized PnL for quantity already closed.
    #[pyo3(name = "apply")]
    fn py_apply(&mut self, fill: &OrderFilled) -> PyResult<()> {
        self.try_apply(fill)
            .map_err(correctness_error_to_pyvalue_err)
    }

    /// Applies a position adjustment event.
    ///
    /// This method handles adjustments to position quantity or realized PnL that occur
    /// outside of normal order fills, such as:
    /// - Commission adjustments in base currency (crypto spot markets).
    /// - Funding payments (perpetual futures).
    ///
    /// The adjustment event is stored in the position's adjustment history for full audit trail.
    ///
    /// # Errors
    ///
    /// Returns an error if currencies differ, arithmetic overflows, or a quantity change
    /// cannot be represented.
    /// An error leaves the position unchanged.
    #[pyo3(name = "apply_adjustment")]
    fn py_apply_adjustment(&mut self, adjustment: PositionAdjusted) -> PyResult<()> {
        self.apply_adjustment(adjustment)
            .map_err(correctness_error_to_pyvalue_err)
    }

    /// Purges all order fill events for the given client order ID and recalculates derived state.
    ///
    /// # Warning
    ///
    /// This operation recalculates the entire position from scratch after removing the specified
    /// order's fills. This is an expensive operation and should be used sparingly.
    ///
    /// # Errors
    ///
    /// Returns an error if the remaining history cannot be represented. An error leaves the
    /// position unchanged.
    #[pyo3(name = "purge_events_for_order")]
    fn py_purge_events_for_order(&mut self, client_order_id: ClientOrderId) -> PyResult<()> {
        self.purge_events_for_order(client_order_id)
            .map_err(correctness_error_to_pyvalue_err)
    }

    /// Returns whether the given order side is opposite to the position entry side.
    #[pyo3(name = "is_opposite_side")]
    fn py_is_opposite_side(&self, side: OrderSide) -> bool {
        self.is_opposite_side(side)
    }

    /// Calculates PnL using Decimal prices and exact quantity.
    ///
    /// # Errors
    ///
    /// Returns an error if `Self.try_calculate_pnl` fails.
    #[pyo3(name = "calculate_pnl")]
    fn py_calculate_pnl(
        &self,
        avg_px_open: Decimal,
        avg_px_close: Decimal,
        quantity: Quantity,
    ) -> PyResult<Money> {
        self.try_calculate_pnl(avg_px_open, avg_px_close, quantity)
            .map_err(nautilus_core::python::to_pyvalue_err)
    }

    /// Calculates the notional value based on the last price.
    #[pyo3(name = "notional_value")]
    fn py_notional_value(&self, price: Price) -> PyResult<Money> {
        self.try_notional_value(price)
            .map_err(nautilus_core::python::to_pyvalue_err)
    }

    /// Constructs a [`Position`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns an error if deserialization from the Python dict fails.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        let values = values.into_bound(py).copy()?;
        let position_id = values.call_method1("pop", ("position_id",))?;
        values.set_item("id", position_id)?;
        let commissions = values
            .call_method1("pop", ("commissions",))?
            .extract::<Option<Vec<String>>>()?
            .unwrap_or_default();
        let amounts = PyDict::new(py);

        for commission in commissions {
            let money = Money::from_str_exact(&commission).map_err(to_pyvalue_err)?;
            amounts.set_item(money.currency.to_string(), money.to_string())?;
        }

        values.set_item("commissions", amounts)?;
        let position: Self = from_dict_pyo3(py, values.unbind())?;
        position
            .check_accounting()
            .map_err(correctness_error_to_pyvalue_err)?;
        Ok(position)
    }

    /// Converts this [`Position`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization into a Python dict fails.
    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = to_dict_pyo3(py, self)?;
        let values = dict.bind(py);
        values.set_item("type", stringify!(Position))?;
        values.set_item("position_id", self.id.to_string())?;
        values.del_item("id")?;
        let events: PyResult<Vec<_>> = self
            .events
            .iter()
            .map(|event| event.py_to_dict(py))
            .collect();
        values.set_item("events", events?)?;
        let adjustments: PyResult<Vec<_>> = self
            .adjustments
            .iter()
            .map(|event| event.py_to_dict(py))
            .collect();
        values.set_item("adjustments", adjustments?)?;
        let venue_order_ids =
            PyList::new(py, self.venue_order_ids().iter().map(ToString::to_string))?;
        values.set_item("venue_order_ids", venue_order_ids)?;
        values.set_item("commissions", commissions_from_vec(py, self.commissions())?)?;
        Ok(dict.into_any())
    }
}

/// Replays position legs onto a hypothetical NETTING position in `ts_opened`
/// order, returning `(net_signed_qty, net_avg_px_open)`.
///
/// Each leg is `(signed_qty, avg_px_open, ts_opened_ns)`. Rules follow
/// `Position.apply`:
/// - Same-side legs produce a quantity-weighted average open price.
/// - Opposite-side legs partial-close at the existing average.
/// - A leg that crosses zero makes the residual take that leg's price.
///
/// Zero-quantity legs are skipped. Sort is stable on `ts_opened`; the caller
/// orders ties (e.g. by `position_id`).
#[must_use]
#[pyfunction]
#[pyo3(name = "fold_net_position")]
#[allow(
    clippy::needless_pass_by_value,
    reason = "PyO3 cannot extract Python list into &[T]; Vec<T> ownership is required"
)]
pub fn py_fold_net_position(legs: Vec<(Decimal, Decimal, u64)>) -> (Decimal, Decimal) {
    position::fold_net_position(&legs)
}
