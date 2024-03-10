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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::{python::serialization::from_dict_pyo3, time::UnixNanos};
use nautilus_model::{
    enums::OrderSide,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::databento::{
    enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction},
    types::{DatabentoImbalance, DatabentoStatistics},
};

#[pymethods]
impl DatabentoImbalance {
    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(instrument_id={}, ref_price={}, cont_book_clr_price={}, auct_interest_clr_price={}, paired_qty={}, total_imbalance_qty={}, side={}, significant_imbalance={}, ts_event={}, ts_recv={}, ts_init={})",
            stringify!(DatabentoImbalance),
            self.instrument_id,
            self.ref_price,
            self.cont_book_clr_price,
            self.auct_interest_clr_price,
            self.paired_qty,
            self.total_imbalance_qty,
            self.side,
            self.significant_imbalance,
            self.ts_event,
            self.ts_recv,
            self.ts_init,
        )
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "ref_price")]
    fn py_ref_price(&self) -> Price {
        self.ref_price
    }

    #[getter]
    #[pyo3(name = "cont_book_clr_price")]
    fn py_cont_book_clr_price(&self) -> Price {
        self.cont_book_clr_price
    }

    #[getter]
    #[pyo3(name = "auct_interest_clr_price")]
    fn py_auct_interest_clr_price(&self) -> Price {
        self.auct_interest_clr_price
    }

    #[getter]
    #[pyo3(name = "paired_qty")]
    fn py_paired_qty(&self) -> Quantity {
        self.paired_qty
    }

    #[getter]
    #[pyo3(name = "total_imbalance_qty")]
    fn py_total_imbalance_qty(&self) -> Quantity {
        self.total_imbalance_qty
    }

    #[getter]
    #[pyo3(name = "side")]
    fn py_side(&self) -> OrderSide {
        self.side
    }

    #[getter]
    #[pyo3(name = "significant_imbalance")]
    fn py_significant_imbalance(&self) -> String {
        self.significant_imbalance.to_string()
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    #[getter]
    #[pyo3(name = "ts_recv")]
    fn py_ts_recv(&self) -> UnixNanos {
        self.ts_recv
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    // TODO
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(DatabentoImbalance))?;
        Ok(dict.into())
    }
}

#[pymethods]
impl DatabentoStatistics {
    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(instrument_id={}, stat_type={}, update_action={}, price={}, quantity={}, channel_id={}, stat_flags={}, sequence={}, ts_ref={}, ts_in_delta={}, ts_event={}, ts_recv={}, ts_init={})",
            stringify!(DatabentoStatistics),
            self.instrument_id,
            self.stat_type,
            self.update_action,
            self.price.map_or_else(|| "None".to_string(), |p| format!("{p}")),
            self.quantity.map_or_else(|| "None".to_string(), |q| format!("{q}")),
            self.channel_id,
            self.stat_flags,
            self.sequence,
            self.ts_ref,
            self.ts_in_delta,
            self.ts_event,
            self.ts_recv,
            self.ts_init,
        )
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "stat_type")]
    fn py_stat_type(&self) -> DatabentoStatisticType {
        self.stat_type
    }

    #[getter]
    #[pyo3(name = "update_action")]
    fn py_update_action(&self) -> DatabentoStatisticUpdateAction {
        self.update_action
    }

    #[getter]
    #[pyo3(name = "price")]
    fn py_price(&self) -> Option<Price> {
        self.price
    }

    #[getter]
    #[pyo3(name = "quantity")]
    fn py_quantity(&self) -> Option<Quantity> {
        self.quantity
    }

    #[getter]
    #[pyo3(name = "channel_id")]
    fn py_channel_id(&self) -> u16 {
        self.channel_id
    }

    #[getter]
    #[pyo3(name = "stat_flags")]
    fn py_stat_flags(&self) -> u8 {
        self.stat_flags
    }

    #[getter]
    #[pyo3(name = "sequence")]
    fn py_sequence(&self) -> u32 {
        self.sequence
    }

    #[getter]
    #[pyo3(name = "ts_ref")]
    fn py_ts_ref(&self) -> UnixNanos {
        self.ts_ref
    }

    #[getter]
    #[pyo3(name = "ts_in_delta")]
    fn py_ts_in_delta(&self) -> i32 {
        self.ts_in_delta
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    #[pyo3(name = "ts_recv")]
    #[getter]
    fn py_ts_recv(&self) -> UnixNanos {
        self.ts_recv
    }

    #[pyo3(name = "ts_init")]
    #[getter]
    fn py_ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    // TODO
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(DatabentoStatistics))?;
        Ok(dict.into())
    }
}
