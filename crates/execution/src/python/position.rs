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

use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, serialization::from_dict_pyo3},
};
use nautilus_model::{
    enums::PositionSide,
    identifiers::{AccountId, InstrumentId, PositionId},
    types::Quantity,
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::reports::position::PositionStatusReport;

#[pymethods]
impl PositionStatusReport {
    #[new]
    #[pyo3(signature = (account_id, instrument_id, position_side, quantity, ts_last, ts_init, venue_position_id=None, report_id=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        account_id: AccountId,
        instrument_id: InstrumentId,
        position_side: PositionSide,
        quantity: Quantity,
        ts_last: u64,
        ts_init: u64,
        venue_position_id: Option<PositionId>,
        report_id: Option<UUID4>,
    ) -> PyResult<Self> {
        Ok(Self::new(
            account_id,
            instrument_id,
            position_side,
            quantity,
            venue_position_id,
            ts_last.into(),
            ts_init.into(),
            report_id,
        ))
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
    #[pyo3(name = "account_id")]
    const fn py_account_id(&self) -> AccountId {
        self.account_id
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    const fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "strategy_id")]
    const fn py_strategy_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "venue_position_id")]
    const fn py_venue_position_id(&self) -> Option<PositionId> {
        self.venue_position_id
    }

    #[getter]
    #[pyo3(name = "position_side")]
    const fn py_position_side(&self) -> PositionSide {
        self.position_side
    }

    #[getter]
    #[pyo3(name = "quantity")]
    const fn py_quantity(&self) -> Quantity {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "report_id")]
    const fn py_report_id(&self) -> UUID4 {
        self.report_id
    }

    #[getter]
    #[pyo3(name = "ts_last")]
    const fn py_ts_last(&self) -> u64 {
        self.ts_last.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[getter]
    #[pyo3(name = "is_flat")]
    const fn py_is_flat(&self) -> bool {
        self.is_flat()
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

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(PositionStatusReport))?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        match self.venue_position_id {
            Some(venue_position_id) => {
                dict.set_item("venue_position_id", venue_position_id.to_string())?;
            }
            None => dict.set_item("venue_position_id", py.None())?,
        }
        dict.set_item("position_side", self.position_side.to_string())?;
        dict.set_item("quantity", self.quantity.to_string())?;
        dict.set_item("signed_decimal_qty", self.signed_decimal_qty.to_string())?;
        dict.set_item("report_id", self.report_id.to_string())?;
        dict.set_item("ts_last", self.ts_last.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        Ok(dict.into())
    }
}
