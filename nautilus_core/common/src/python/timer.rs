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

use std::str::FromStr;

use nautilus_core::{python::to_pyvalue_err, time::UnixNanos, uuid::UUID4};
use pyo3::{
    basic::CompareOp,
    prelude::*,
    types::{PyLong, PyString, PyTuple},
};
use ustr::Ustr;

use crate::timer::TimeEvent;

#[pymethods]
impl TimeEvent {
    #[new]
    fn py_new(name: &str, event_id: UUID4, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        Self::new(Ustr::from(name), event_id, ts_event, ts_init)
    }

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let tuple: (&PyString, &PyString, &PyLong, &PyLong) = state.extract(py)?;

        self.name = Ustr::from(tuple.0.extract()?);
        self.event_id = UUID4::from_str(tuple.1.extract()?).map_err(to_pyvalue_err)?;
        self.ts_event = tuple.2.extract()?;
        self.ts_init = tuple.3.extract()?;

        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((
            self.name.to_string(),
            self.event_id.to_string(),
            self.ts_event,
            self.ts_init,
        )
            .to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> Self {
        Self::new(Ustr::from("NULL"), UUID4::new(), 0, 0)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{}('{}')", stringify!(TimeEvent), self)
    }

    #[getter]
    #[pyo3(name = "name")]
    fn py_name(&self) -> String {
        self.name.to_string()
    }

    #[getter]
    #[pyo3(name = "event_id")]
    fn py_event_id(&self) -> UUID4 {
        self.event_id
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}
