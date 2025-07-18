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

//! Python bindings for live node.

use std::{cell::RefCell, rc::Rc};

use nautilus_common::enums::Environment;
use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;
use pyo3::prelude::*;

use crate::node::LiveNode;

/// Python wrapper for `LiveNodeBuilder` that uses interior mutability
/// to work around PyO3's shared ownership model.
#[derive(Debug)]
#[pyclass(
    name = "LiveNodeBuilder",
    module = "nautilus_trader.core.nautilus_pyo3.live",
    unsendable
)]
pub struct LiveNodeBuilderPy {
    inner: Rc<RefCell<Option<crate::node::LiveNodeBuilder>>>,
}

#[pymethods]
impl LiveNode {
    /// Creates a new `LiveNode` builder.
    #[staticmethod]
    fn py_builder(
        name: String,
        trader_id: TraderId,
        environment: Environment,
    ) -> PyResult<LiveNodeBuilderPy> {
        match Self::builder(name, trader_id, environment) {
            Ok(builder) => Ok(LiveNodeBuilderPy {
                inner: Rc::new(RefCell::new(Some(builder))),
            }),
            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                e.to_string(),
            )),
        }
    }

    /// Returns the node's environment.
    #[getter]
    #[pyo3(name = "environment")]
    fn py_environment(&self) -> Environment {
        self.environment()
    }

    /// Returns the node's trader ID.
    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id()
    }

    /// Returns the node's instance ID.
    #[getter]
    #[pyo3(name = "instance_id")]
    const fn py_instance_id(&self) -> UUID4 {
        self.instance_id()
    }

    /// Returns whether the node is running.
    #[getter]
    #[pyo3(name = "is_running")]
    const fn py_is_running(&self) -> bool {
        self.is_running()
    }

    /// Returns a string representation of the node.
    fn __repr__(&self) -> String {
        format!(
            "LiveNode(trader_id={}, environment={:?}, running={})",
            self.trader_id(),
            self.environment(),
            self.is_running()
        )
    }
}

#[pymethods]
impl LiveNodeBuilderPy {
    /// Sets the instance ID for the node.
    fn py_with_instance_id(&self, instance_id: UUID4) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_instance_id(instance_id));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Builder already consumed",
            ))
        }
    }

    /// Sets whether to load state on startup.
    fn py_with_load_state(&self, load_state: bool) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_load_state(load_state));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Builder already consumed",
            ))
        }
    }

    /// Sets whether to save state on shutdown.
    fn py_with_save_state(&self, save_state: bool) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_save_state(save_state));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Builder already consumed",
            ))
        }
    }

    /// Adds a data client with factory and configuration.
    fn add_data_client(
        &self,
        _name: Option<String>,
        _factory: PyObject,
        _config: PyObject,
    ) -> PyResult<Self> {
        // For now, this is a simplified implementation
        // In practice, we'd need to convert PyObject to the appropriate factory and config types
        Err(PyErr::new::<pyo3::exceptions::PyNotImplementedError, _>(
            "add_data_client not yet implemented for Python",
        ))
    }

    /// Builds the node.
    fn build(&self) -> PyResult<LiveNode> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            match builder.build() {
                Ok(node) => Ok(node),
                Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                    e.to_string(),
                )),
            }
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Builder already consumed",
            ))
        }
    }

    /// Returns a string representation of the builder.
    fn __repr__(&self) -> String {
        "LiveNodeBuilder".to_string()
    }
}
