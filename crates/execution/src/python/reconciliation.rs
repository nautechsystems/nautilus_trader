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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    python::instruments::pyobject_to_instrument_any, reports::ExecutionMassStatus,
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyTuple},
};
use rust_decimal::Decimal;

use crate::reconciliation::{
    calculate_reconciliation_price, process_mass_status_for_reconciliation,
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
#[pyfunction(name = "calculate_reconciliation_price")]
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
