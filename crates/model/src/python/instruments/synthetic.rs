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

use std::collections::HashMap;

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::{
    identifiers::{InstrumentId, Symbol},
    instruments::SyntheticInstrument,
    types::Price,
};

#[pymethods]
impl SyntheticInstrument {
    #[new]
    #[pyo3(signature = (symbol, price_precision, components, formula, ts_event, ts_init))]
    fn py_new(
        symbol: Symbol,
        price_precision: u8,
        components: Vec<InstrumentId>,
        formula: String,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        Self::new_checked(
            symbol,
            price_precision,
            components,
            formula,
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> InstrumentId {
        self.id
    }

    #[getter]
    #[pyo3(name = "price_precision")]
    fn py_price_precision(&self) -> u8 {
        self.price_precision
    }

    #[getter]
    #[pyo3(name = "price_increment")]
    fn py_price_increment(&self) -> Price {
        self.price_increment
    }

    #[getter]
    #[pyo3(name = "components")]
    fn py_components(&self) -> Vec<InstrumentId> {
        self.components.clone()
    }

    #[getter]
    #[pyo3(name = "formula")]
    fn py_formula(&self) -> &str {
        self.formula.as_str()
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[pyo3(name = "is_valid_formula")]
    fn py_is_valid_formula(&self, formula: &str) -> bool {
        self.is_valid_formula(formula)
    }

    #[pyo3(name = "change_formula")]
    fn py_change_formula(&mut self, formula: String) -> PyResult<()> {
        self.change_formula(formula).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "calculate")]
    fn py_calculate(&mut self, inputs: Vec<f64>) -> PyResult<Price> {
        self.calculate(&inputs).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "calculate_from_map")]
    fn py_calculate_from_map(
        &mut self,
        _py: Python<'_>,
        inputs: &Bound<'_, PyDict>,
    ) -> PyResult<Price> {
        let mut map: HashMap<String, f64> = HashMap::new();
        for (key, value) in inputs.iter() {
            let k: String = key.extract()?;
            let v: f64 = value.extract()?;
            map.insert(k, v);
        }
        self.calculate_from_map(&map).map_err(to_pyvalue_err)
    }
}
