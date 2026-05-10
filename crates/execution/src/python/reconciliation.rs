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

//! Python bindings for reconciliation functions.

use nautilus_core::{UnixNanos, python::to_pyvalue_err};
use nautilus_model::{
    enums::{OrderSide, OrderType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, TradeId, VenueOrderId},
    python::instruments::pyobject_to_instrument_any,
    reports::ExecutionMassStatus,
    types::{Price, Quantity},
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyTuple},
};
use rust_decimal::Decimal;

use crate::reconciliation::{
    calculate_reconciliation_price, create_inferred_reconciliation_trade_id,
    create_position_reconciliation_venue_order_id, process_mass_status_for_reconciliation,
};

/// Process mass status for position reconciliation.
///
/// Takes ExecutionMassStatus and Instrument, performs all reconciliation logic in Rust,
/// and returns tuple of (order_reports, fill_reports) ready for processing.
///
/// # Returns
///
/// Tuple of `(Dict[str, OrderStatusReport], Dict[str, List[FillReport]])`
///
/// # Errors
///
/// Returns an error if instrument conversion or reconciliation fails.
#[pyfunction(name = "adjust_fills_for_partial_window")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.execution")]
#[pyo3(signature = (mass_status, instrument, tolerance=None))]
pub fn py_adjust_fills_for_partial_window(
    py: Python<'_>,
    mass_status: &Bound<'_, PyAny>,
    instrument: Py<PyAny>,
    tolerance: Option<String>,
) -> PyResult<Py<PyTuple>> {
    let instrument_any = pyobject_to_instrument_any(py, instrument)?;
    let mass_status_obj: ExecutionMassStatus = mass_status.extract()?;

    let tol = tolerance
        .map(|s| Decimal::from_str_exact(&s).map_err(to_pyvalue_err))
        .transpose()?;

    let result = process_mass_status_for_reconciliation(&mass_status_obj, &instrument_any, tol)
        .map_err(to_pyvalue_err)?;

    let orders_dict = PyDict::new(py);
    for (id, order) in result.orders {
        orders_dict.set_item(id.to_string(), order.into_py_any(py)?)?;
    }

    let fills_dict = PyDict::new(py);
    for (id, fills) in result.fills {
        let fills_list: Result<Vec<_>, _> = fills.into_iter().map(|f| f.into_py_any(py)).collect();
        fills_dict.set_item(id.to_string(), fills_list?)?;
    }

    Ok(PyTuple::new(
        py,
        [orders_dict.into_py_any(py)?, fills_dict.into_py_any(py)?],
    )?
    .into())
}

/// Calculate the price needed for a reconciliation order to achieve target position.
///
/// This is a pure function that calculates what price a fill would need to have
/// to move from the current position state to the target position state with the
/// correct average price, accounting for the netting simulation logic.
///
/// # Returns
///
/// Returns `Some(Decimal)` if a valid reconciliation price can be calculated, `None` otherwise.
///
/// # Notes
///
/// The function handles four scenarios:
/// 1. Position to flat: reconciliation_px = current_avg_px (close at current average)
/// 2. Flat to position: reconciliation_px = target_avg_px
/// 3. Position flip (sign change): reconciliation_px = target_avg_px (due to value reset in simulation)
/// 4. Accumulation/reduction: weighted average formula
#[pyfunction(name = "calculate_reconciliation_price")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.execution")]
#[pyo3(signature = (current_position_qty, current_position_avg_px, target_position_qty, target_position_avg_px))]
pub fn py_calculate_reconciliation_price(
    current_position_qty: Decimal,
    current_position_avg_px: Option<Decimal>,
    target_position_qty: Decimal,
    target_position_avg_px: Option<Decimal>,
) -> Option<Decimal> {
    calculate_reconciliation_price(
        current_position_qty,
        current_position_avg_px,
        target_position_qty,
        target_position_avg_px,
    )
}

/// Create a deterministic `TradeId` for an inferred reconciliation fill.
///
/// The `account_id` scopes the ID to the venue account, preventing cross-account
/// collisions on venues where `venue_order_id` is only account-unique. The `ts_last`
/// (venue-provided) differentiates successive reconciliation incidents with the same
/// shape while keeping cross-restart replays deterministic.
#[pyfunction(name = "create_inferred_reconciliation_trade_id")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.execution")]
#[pyo3(signature = (account_id, instrument_id, client_order_id, venue_order_id, order_side, order_type, filled_qty, last_qty, last_px, position_id, ts_last))]
#[expect(clippy::too_many_arguments)]
pub fn py_create_inferred_reconciliation_trade_id(
    account_id: AccountId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
    order_side: OrderSide,
    order_type: OrderType,
    filled_qty: Quantity,
    last_qty: Quantity,
    last_px: Price,
    position_id: PositionId,
    ts_last: u64,
) -> TradeId {
    create_inferred_reconciliation_trade_id(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        order_type,
        filled_qty,
        last_qty,
        last_px,
        position_id,
        UnixNanos::from(ts_last),
    )
}

/// The `account_id` scopes the ID to the venue account, preventing cross-account
/// collisions where the engine would otherwise fall back to `ClientOrderId::from(venue_order_id)`
/// and conflate orders from different accounts. The `ts_last` (venue-provided) ensures that
/// successive reconciliation incidents with the same shape get distinct IDs, while the same
/// logical event replayed after restart still hashes the same (venue re-reports identical ts).
#[pyfunction(name = "create_position_reconciliation_venue_order_id")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.execution")]
#[pyo3(signature = (account_id, instrument_id, order_side, order_type, quantity, price=None, venue_position_id=None, ts_last=0, tag=None))]
#[expect(clippy::needless_pass_by_value, clippy::too_many_arguments)]
pub fn py_create_position_reconciliation_venue_order_id(
    account_id: AccountId,
    instrument_id: InstrumentId,
    order_side: OrderSide,
    order_type: OrderType,
    quantity: Quantity,
    price: Option<Price>,
    venue_position_id: Option<PositionId>,
    ts_last: u64,
    tag: Option<String>,
) -> VenueOrderId {
    create_position_reconciliation_venue_order_id(
        account_id,
        instrument_id,
        order_side,
        order_type,
        quantity,
        price,
        venue_position_id,
        tag.as_deref(),
        UnixNanos::from(ts_last),
    )
}
