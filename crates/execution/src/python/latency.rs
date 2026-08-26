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

//! Python bindings for latency model types.

use nautilus_core::{UnixNanos, python::to_pytype_err};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::models::latency::{LatencyModelAny, StaticLatencyModel};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl StaticLatencyModel {
    /// Static latency model with fixed latency values.
    ///
    /// Models the latency for different order operations including base network latency
    /// and specific operation latencies for insert, update, and delete operations.
    ///
    /// The base latency is automatically added to each operation latency, matching
    /// Python's behavior. For example, if `base_latency_nanos = 100ms` and
    /// `insert_latency_nanos = 200ms`, the effective insert latency will be 300ms.
    #[new]
    #[pyo3(signature = (
        base_latency_nanos = 0,
        insert_latency_nanos = 0,
        update_latency_nanos = 0,
        cancel_latency_nanos = 0,
    ))]
    fn py_new(
        base_latency_nanos: u64,
        insert_latency_nanos: u64,
        update_latency_nanos: u64,
        cancel_latency_nanos: u64,
    ) -> Self {
        Self::new(
            UnixNanos::from(base_latency_nanos),
            UnixNanos::from(insert_latency_nanos),
            UnixNanos::from(update_latency_nanos),
            UnixNanos::from(cancel_latency_nanos),
        )
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Extracts a Python latency model object into a Rust [`LatencyModelAny`].
///
/// # Errors
///
/// Returns an error if `obj` is not a supported latency model binding.
pub fn pyobject_to_latency_model_any(obj: &Bound<'_, PyAny>) -> PyResult<LatencyModelAny> {
    if let Ok(m) = obj.extract::<StaticLatencyModel>() {
        return Ok(LatencyModelAny::Static(m));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to LatencyModel"
    )))
}

/// Converts a Rust [`LatencyModelAny`] into its Python binding object.
///
/// # Errors
///
/// Returns an error if conversion to a Python object fails.
pub fn latency_model_any_to_pyobject(
    py: Python<'_>,
    model: &LatencyModelAny,
) -> PyResult<Py<PyAny>> {
    match model {
        LatencyModelAny::Static(model) => model.clone().into_py_any(py),
    }
}
