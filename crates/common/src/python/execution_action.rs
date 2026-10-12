// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Python bindings for standard execution actions.

use nautilus_core::{from_pydict, python::to_pyvalue_err};
use nautilus_model::{identifiers::InstrumentId, types::Quantity};
use pyo3::{prelude::*, types::PyDict};

use crate::messages::execution::{BinaryOptionAction, BinaryOptionPair, ExecutionAction};

/// Python wrapper for a standard binary-option action.
#[pyclass(
    frozen,
    name = "BinaryOptionAction",
    module = "nautilus_trader.live",
    from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
#[derive(Clone, Debug)]
pub struct PyBinaryOptionAction {
    inner: BinaryOptionAction,
}

impl PyBinaryOptionAction {
    /// Returns the wrapped standard action.
    #[must_use]
    pub fn inner(&self) -> BinaryOptionAction {
        self.inner.clone()
    }
}

/// Converts a Python execution action to its standard Rust representation.
///
/// # Errors
///
/// Returns an error if `action` is not a supported standard action object.
#[expect(clippy::needless_pass_by_value)]
pub fn pyobject_to_execution_action(
    py: Python<'_>,
    action: Py<PyAny>,
) -> PyResult<ExecutionAction> {
    Ok(action.extract::<PyBinaryOptionAction>(py)?.inner().into())
}

/// Converts a standard Rust execution action to its Python object.
///
/// # Errors
///
/// Returns an error if allocating the wrapper object fails.
pub fn execution_action_to_pyobject(
    py: Python<'_>,
    action: ExecutionAction,
) -> PyResult<Py<PyAny>> {
    match action {
        ExecutionAction::BinaryOption(action) => {
            Ok(Py::new(py, PyBinaryOptionAction { inner: action })?.into_any())
        }
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyBinaryOptionAction {
    /// Creates a binary-option split action.
    #[staticmethod]
    #[pyo3(signature = (yes_instrument_id, no_instrument_id, quantity, params=None))]
    fn split(
        py: Python<'_>,
        yes_instrument_id: InstrumentId,
        no_instrument_id: InstrumentId,
        quantity: Quantity,
        params: Option<Py<PyDict>>,
    ) -> PyResult<Self> {
        if !quantity.is_positive() {
            return Err(to_pyvalue_err(
                "Binary option split quantity must be positive",
            ));
        }
        let pair =
            BinaryOptionPair::new(yes_instrument_id, no_instrument_id).map_err(to_pyvalue_err)?;
        let params = match params {
            Some(params) => from_pydict(py, &params)?,
            None => None,
        };

        Ok(Self {
            inner: BinaryOptionAction::Split {
                pair,
                quantity,
                params,
            },
        })
    }

    /// Creates a binary-option merge action.
    #[staticmethod]
    #[pyo3(signature = (yes_instrument_id, no_instrument_id, quantity, params=None))]
    fn merge(
        py: Python<'_>,
        yes_instrument_id: InstrumentId,
        no_instrument_id: InstrumentId,
        quantity: Quantity,
        params: Option<Py<PyDict>>,
    ) -> PyResult<Self> {
        if !quantity.is_positive() {
            return Err(to_pyvalue_err(
                "Binary option merge quantity must be positive",
            ));
        }
        let pair =
            BinaryOptionPair::new(yes_instrument_id, no_instrument_id).map_err(to_pyvalue_err)?;
        let params = match params {
            Some(params) => from_pydict(py, &params)?,
            None => None,
        };

        Ok(Self {
            inner: BinaryOptionAction::Merge {
                pair,
                quantity,
                params,
            },
        })
    }

    #[getter]
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    #[getter]
    fn yes_instrument_id(&self) -> InstrumentId {
        self.inner.pair().yes_instrument_id
    }

    #[getter]
    fn no_instrument_id(&self) -> InstrumentId {
        self.inner.pair().no_instrument_id
    }

    #[getter]
    fn quantity(&self) -> Quantity {
        match &self.inner {
            BinaryOptionAction::Split { quantity, .. }
            | BinaryOptionAction::Merge { quantity, .. } => *quantity,
        }
    }

    #[getter]
    fn params(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        use nautilus_core::python::params::params_to_pydict;

        self.inner
            .params()
            .map(|params| params_to_pydict(py, params))
            .transpose()
    }
}
