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

use nautilus_common::{
    actor::data_actor::{DataActorConfig, ImportableActorConfig},
    component::{Component, register_component_actor_by_ref},
    enums::Environment,
    python::actor::PyDataActor,
};
use nautilus_core::{UUID4, python::to_pyruntime_err};
use nautilus_model::identifiers::TraderId;
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::node::{LiveNode, LiveNodeBuilder};

#[pymethods]
impl LiveNode {
    /// Creates a new `LiveNode` builder.
    #[staticmethod]
    #[pyo3(name = "builder")]
    fn py_builder(
        name: String,
        trader_id: TraderId,
        environment: Environment,
    ) -> PyResult<LiveNodeBuilderPy> {
        match LiveNode::builder(name, trader_id, environment) {
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
    fn py_instance_id(&self) -> UUID4 {
        self.instance_id()
    }

    /// Returns whether the node is running.
    #[getter]
    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        self.is_running()
    }

    #[pyo3(name = "start")]
    fn py_start(&mut self) -> PyResult<()> {
        if self.is_running() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "LiveNode is already running",
            ));
        }

        // Non-blocking start - just start the node in the background
        let rt = tokio::runtime::Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create runtime: {e}"))
        })?;

        rt.block_on(async {
            self.start()
                .await
                .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
        })
    }

    #[pyo3(name = "run")]
    fn py_run(&mut self, py: Python) -> PyResult<()> {
        if self.is_running() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "LiveNode is already running",
            ));
        }

        // Get a handle for coordinating with the signal checker
        let handle = self.handle();

        // Import signal module
        let signal_module = py.import("signal")?;
        let original_handler =
            signal_module.call_method1("signal", (2, signal_module.getattr("SIG_DFL")?))?; // Save original SIGINT handler (signal 2)

        // Set up a custom signal handler that uses our handle
        let handle_for_signal = handle.clone();
        let signal_callback = pyo3::types::PyCFunction::new_closure(
            py,
            None,
            None,
            move |_args: &pyo3::Bound<'_, pyo3::types::PyTuple>,
                  _kwargs: Option<&pyo3::Bound<'_, pyo3::types::PyDict>>|
                  -> PyResult<()> {
                log::info!("Python signal handler called");
                handle_for_signal.stop();
                Ok(())
            },
        )?;

        // Install our signal handler
        signal_module.call_method1("signal", (2, signal_callback))?;

        // Run the node and restore signal handler afterward
        let result = {
            let rt = tokio::runtime::Runtime::new().map_err(|e| {
                pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to create runtime: {e}"))
            })?;

            rt.block_on(async {
                self.run()
                    .await
                    .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
            })
        };

        // Restore original signal handler
        signal_module.call_method1("signal", (2, original_handler))?;

        result
    }

    #[pyo3(name = "stop")]
    fn py_stop(&self) -> PyResult<()> {
        if !self.is_running() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "LiveNode is not running",
            ));
        }

        // Use the handle to signal stop - this is thread-safe and doesn't require async
        self.handle().stop();
        Ok(())
    }

    #[allow(unsafe_code)] // Required for Python actor component registration
    #[pyo3(name = "add_actor_from_config")]
    fn py_add_actor_from_config(
        &mut self,
        _py: Python,
        config: ImportableActorConfig,
    ) -> PyResult<()> {
        log::debug!("`add_actor_from_config` with: {config:?}");

        // Extract module and class name from actor_path
        let parts: Vec<&str> = config.actor_path.split(':').collect();
        if parts.len() != 2 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "actor_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing actor from module: {module_name} class: {class_name}");

        // Import the Python class to verify it exists and get it for method dispatch
        let _python_class = Python::with_gil(|py| -> PyResult<PyObject> {
            let actor_module = py.import(module_name)?;
            let actor_class = actor_module.getattr(class_name)?;
            Ok(actor_class.unbind())
        })
        .map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to import Python class: {e}"))
        })?;

        // TODO: Create default DataActorConfig for Rust PyDataActor,
        // the Python class will handle its own configuration for now
        let basic_data_actor_config = DataActorConfig::default();

        log::debug!("Created basic DataActorConfig for Rust: {basic_data_actor_config:?}");

        // Create the Python actor and register the internal PyDataActor
        let python_actor = Python::with_gil(|py| -> anyhow::Result<PyObject> {
            // Import the Python class
            let actor_module = py
                .import(module_name)
                .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
            let actor_class = actor_module
                .getattr(class_name)
                .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

            // Create the Python actor instance with no config for now (uses defaults)
            let python_actor = actor_class.call0()?;

            log::debug!("Created Python actor instance: {python_actor:?}");

            // Get a mutable reference to the internal PyDataActor for registration
            let mut py_data_actor_ref = python_actor.extract::<PyRefMut<PyDataActor>>()?;

            log::debug!(
                "Internal PyDataActor mem_addr: {}, registered: {}",
                &py_data_actor_ref.mem_address(),
                py_data_actor_ref.is_registered()
            );

            // Set the Python instance reference for method dispatch on the original
            py_data_actor_ref.set_python_instance(python_actor.clone().unbind());

            log::debug!("Set Python instance reference for method dispatch");

            // Register the internal PyDataActor
            let trader_id = self.trader_id();
            let clock = self.kernel().clock();
            let cache = self.kernel().cache();

            py_data_actor_ref
                .register(trader_id, clock, cache)
                .map_err(|e| anyhow::anyhow!("Failed to register PyDataActor: {e}"))?;

            log::debug!(
                "Internal PyDataActor registered: {}, state: {:?}",
                py_data_actor_ref.is_registered(),
                py_data_actor_ref.state()
            );

            py_data_actor_ref
                .initialize()
                .map_err(|e| anyhow::anyhow!("Failed to initialize PyDataActor: {e}"))?;

            log::debug!(
                "Internal PyDataActor initialized, state: {:?}",
                py_data_actor_ref.state()
            );

            Ok(python_actor.unbind())
        })
        .map_err(to_pyruntime_err)?;

        // Add the actor to the trader's lifecycle management without consuming it
        let actor_id = Python::with_gil(
            |py| -> anyhow::Result<nautilus_model::identifiers::ActorId> {
                let py_actor = python_actor.bind(py);
                let py_data_actor_ref = py_actor
                    .downcast::<PyDataActor>()
                    .map_err(|e| anyhow::anyhow!("Failed to downcast to PyDataActor: {e}"))?;
                let py_data_actor = py_data_actor_ref.borrow();

                // Register the component in the global registry using the unsafe method
                // SAFETY: The Python instance will remain alive, keeping the PyDataActor valid
                unsafe {
                    register_component_actor_by_ref(&*py_data_actor);
                }

                Ok(py_data_actor.actor_id())
            },
        )
        .map_err(to_pyruntime_err)?;

        // TODO: Add the actor ID to the trader for lifecycle management; clean up approach
        self.kernel_mut()
            .trader
            .add_actor_id_for_lifecycle(actor_id)
            .map_err(to_pyruntime_err)?;

        // Store the Python actor reference to prevent garbage collection
        // TODO: Add to a proper LiveNode registry for Python actors
        std::mem::forget(python_actor); // Prevent dropping - we'll manage lifecycle manually

        log::info!("Registered Python actor {actor_id}");
        Ok(())
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

/// Python wrapper for `LiveNodeBuilder` that uses interior mutability
/// to work around PyO3's shared ownership model.
#[derive(Debug)]
#[pyclass(name = "LiveNodeBuilder", module = "nautilus_trader.live", unsendable)]
pub struct LiveNodeBuilderPy {
    inner: Rc<RefCell<Option<LiveNodeBuilder>>>,
}

#[pymethods]
impl LiveNodeBuilderPy {
    /// Sets the instance ID for the node.
    #[pyo3(name = "with_instance_id")]
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
    #[pyo3(name = "with_load_state")]
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
    #[pyo3(name = "with_save_state")]
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
    #[pyo3(name = "add_data_client")]
    fn py_add_data_client(
        &self,
        name: Option<String>,
        factory: PyObject,
        config: PyObject,
    ) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            Python::with_gil(|py| -> PyResult<Self> {
                // Use the global registry to extract PyObjects to trait objects
                let registry = get_global_pyo3_registry();

                let boxed_factory = registry.extract_factory(py, factory.clone_ref(py))?;
                let boxed_config = registry.extract_config(py, config.clone_ref(py))?;

                // Use the factory name from the original factory for the client name
                let factory_name = factory
                    .getattr(py, "name")?
                    .call0(py)?
                    .extract::<String>(py)?;
                let client_name = name.unwrap_or(factory_name);

                // Add the data client to the builder using boxed trait objects
                match builder.add_data_client(Some(client_name), boxed_factory, boxed_config) {
                    Ok(updated_builder) => {
                        *inner_ref = Some(updated_builder);
                        Ok(Self {
                            inner: self.inner.clone(),
                        })
                    }
                    Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                        "Failed to add data client: {e}"
                    ))),
                }
            })
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "Builder already consumed",
            ))
        }
    }

    /// Builds the node.
    #[pyo3(name = "build")]
    fn py_build(&self) -> PyResult<LiveNode> {
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
        format!("{self:?}")
    }
}
