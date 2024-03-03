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

use nautilus_core::python::serialization::from_dict_pyo3;
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::databento::types::{DatabentoImbalance, DatabentoStatistics};

#[pymethods]
impl DatabentoImbalance {
    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
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

    fn __str__(&self) -> String {
        self.__repr__()
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

    fn __repr__(&self) -> String {
        format!(
            "{}(instrument_id={}, stat_type={}, update_action={}, price=TBD, quantity=TBD, channel_id={}, stat_flags={}, sequence={}, ts_ref={}, ts_in_delta={}, ts_event={}, ts_recv={}, ts_init={})",
            stringify!(DatabentoStatistics),
            self.instrument_id,
            self.stat_type,
            self.update_action,
            // self.price,  // TODO: Implement display for Option<Price>
            // self.quantity,  // TODO: Implement display for Option<Quantity>
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

    fn __str__(&self) -> String {
        self.__repr__()
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
