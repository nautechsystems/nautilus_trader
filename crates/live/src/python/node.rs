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

//! Python bindings for live node.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nautilus_common::{
    actor::data_actor::ImportableActorConfig, cache::CacheConfig, enums::Environment,
    live::get_runtime, logging::logger::LoggerConfig, python::actor::PyDataActor,
};
#[cfg(feature = "examples")]
use nautilus_core::python::to_pytype_err;
use nautilus_core::{
    UUID4,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::identifiers::{ActorId, ComponentId, ExecAlgorithmId, StrategyId, TraderId};
use nautilus_portfolio::config::PortfolioConfig;
use nautilus_system::get_global_pyo3_registry;
use nautilus_trading::{
    ImportableExecAlgorithmConfig, ImportableStrategyConfig,
    python::strategy::{PyStrategy, PyStrategyInner},
};
use pyo3::{
    prelude::*,
    types::{PyDict, PyTuple},
};
use serde_json;

use crate::{
    builder::LiveNodeBuilder,
    config::{LiveDataEngineConfig, LiveExecEngineConfig, LiveNodeConfig, LiveRiskEngineConfig},
    node::LiveNode,
};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl LiveNode {
    #[staticmethod]
    #[pyo3(name = "build")]
    #[pyo3(signature = (name, config=None))]
    fn py_build(name: String, config: Option<LiveNodeConfig>) -> PyResult<Self> {
        Self::build(name, config).map_err(to_pyruntime_err)
    }

    #[staticmethod]
    #[pyo3(name = "builder")]
    fn py_builder(
        name: String,
        trader_id: TraderId,
        environment: Environment,
    ) -> PyResult<LiveNodeBuilderPy> {
        match Self::builder(trader_id, environment) {
            Ok(builder) => Ok(LiveNodeBuilderPy {
                inner: Rc::new(RefCell::new(Some(builder.with_name(name)))),
            }),
            Err(e) => Err(to_pyruntime_err(e)),
        }
    }

    #[getter]
    #[pyo3(name = "environment")]
    fn py_environment(&self) -> Environment {
        self.environment()
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.trader_id()
    }

    #[getter]
    #[pyo3(name = "instance_id")]
    const fn py_instance_id(&self) -> UUID4 {
        self.instance_id()
    }

    #[getter]
    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        self.is_running()
    }

    #[pyo3(name = "start")]
    fn py_start(&mut self) -> PyResult<()> {
        if self.is_running() {
            return Err(to_pyruntime_err("LiveNode is already running"));
        }

        // Non-blocking start - just start the node in the background
        get_runtime().block_on(async { self.start().await.map_err(to_pyruntime_err) })
    }

    #[pyo3(name = "run")]
    fn py_run(&mut self, py: Python) -> PyResult<()> {
        if self.is_running() {
            return Err(to_pyruntime_err("LiveNode is already running"));
        }

        // Get a handle for coordinating with the signal checker
        let handle = self.handle();

        // Import signal module
        let signal_module = py.import("signal")?;
        let original_handler =
            signal_module.call_method1("signal", (2, signal_module.getattr("SIG_DFL")?))?; // Save original SIGINT handler (signal 2)

        // Set up a custom signal handler that uses our handle
        let handle_for_signal = handle;
        let signal_callback = pyo3::types::PyCFunction::new_closure(
            py,
            None,
            None,
            move |_args: &pyo3::Bound<'_, PyTuple>,
                  _kwargs: Option<&pyo3::Bound<'_, PyDict>>|
                  -> PyResult<()> {
                log::info!("Python signal handler called");
                handle_for_signal.stop();
                Ok(())
            },
        )?;

        // Install our signal handler
        signal_module.call_method1("signal", (2, signal_callback))?;

        // Run the node and restore signal handler afterward
        let result =
            { get_runtime().block_on(async { self.run().await.map_err(to_pyruntime_err) }) };

        // Restore original signal handler
        signal_module.call_method1("signal", (2, original_handler))?;

        result
    }

    #[pyo3(name = "stop")]
    fn py_stop(&self) -> PyResult<()> {
        if !self.is_running() {
            return Err(to_pyruntime_err("LiveNode is not running"));
        }

        // Use the handle to signal stop - this is thread-safe and doesn't require async
        self.handle().stop();
        Ok(())
    }

    #[allow(
        unsafe_code,
        reason = "Required for Python actor component registration"
    )]
    #[pyo3(name = "add_actor_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_actor_from_config(
        &mut self,
        _py: Python,
        config: ImportableActorConfig,
    ) -> PyResult<()> {
        log::debug!("`add_actor_from_config` with: {config:?}");

        // Extract module and class name from actor_path
        let parts: Vec<&str> = config.actor_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "actor_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing actor from module: {module_name} class: {class_name}");

        // Phase 1: Create and configure the Python actor, extract its actor_id
        let (python_actor, actor_id) =
            Python::attach(|py| -> anyhow::Result<(Py<PyAny>, ActorId)> {
                let actor_module = py
                    .import(module_name)
                    .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
                let actor_class = actor_module
                    .getattr(class_name)
                    .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

                let config_instance =
                    create_config_instance(py, &config.config_path, &config.config)?;

                let python_actor = if let Some(config_obj) = config_instance.clone() {
                    actor_class.call1((config_obj,))?
                } else {
                    actor_class.call0()?
                };

                log::debug!("Created Python actor instance: {python_actor:?}");

                let mut py_data_actor_ref = python_actor
                    .extract::<PyRefMut<PyDataActor>>()
                    .map_err(Into::<PyErr>::into)
                    .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

                // Extract inherited config fields from the Python config
                if let Some(config_obj) = config_instance.as_ref() {
                    if let Ok(actor_id) = config_obj.getattr("actor_id")
                        && !actor_id.is_none()
                    {
                        let actor_id_val = if let Ok(aid) = actor_id.extract::<ActorId>() {
                            aid
                        } else if let Ok(aid_str) = actor_id.extract::<String>() {
                            ActorId::new_checked(&aid_str)?
                        } else {
                            anyhow::bail!("Invalid `actor_id` type");
                        };
                        py_data_actor_ref.set_actor_id(actor_id_val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_events") {
                        py_data_actor_ref.set_log_events(val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_commands") {
                        py_data_actor_ref.set_log_commands(val);
                    }
                }

                py_data_actor_ref.set_python_instance(python_actor.clone().unbind());

                let actor_id = py_data_actor_ref.actor_id();

                Ok((python_actor.unbind(), actor_id))
            })
            .map_err(to_pyruntime_err)?;

        // Validate no duplicate before any mutations
        if self
            .kernel()
            .trader
            .borrow()
            .actor_ids()
            .contains(&actor_id)
        {
            return Err(to_pyruntime_err(format!(
                "Actor '{actor_id}' is already registered"
            )));
        }

        // Phase 2: Create per-component clock via the trader.
        // This requires `&mut self` access to the kernel, which cannot be held
        // inside a `Python::attach` block, hence the separate phases.
        let trader_id = self.kernel().trader_id();
        let cache = self.kernel().cache();
        let component_id = ComponentId::new(actor_id.inner().as_str());
        let clock = self
            .kernel_mut()
            .trader
            .borrow_mut()
            .create_component_clock(component_id);

        // Phase 3: Register the actor with its dedicated clock
        Python::attach(|py| -> anyhow::Result<()> {
            let py_actor = python_actor.bind(py);
            let mut py_data_actor_ref = py_actor
                .extract::<PyRefMut<PyDataActor>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

            py_data_actor_ref
                .register(trader_id, clock, cache)
                .map_err(|e| anyhow::anyhow!("Failed to register PyDataActor: {e}"))?;

            log::debug!(
                "Internal PyDataActor registered: {}, state: {:?}",
                py_data_actor_ref.is_registered(),
                py_data_actor_ref.state()
            );

            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        // Phase 4: Register in global registries and track for lifecycle
        Python::attach(|py| -> anyhow::Result<()> {
            let py_actor = python_actor.bind(py);
            let py_data_actor_ref = py_actor
                .cast::<PyDataActor>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyDataActor: {e}"))?;
            py_data_actor_ref.borrow().register_in_global_registries();
            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        self.kernel_mut()
            .trader
            .borrow_mut()
            .add_actor_id_for_lifecycle(actor_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python actor {actor_id}");
        Ok(())
    }

    #[allow(
        unsafe_code,
        reason = "Required for Python strategy component registration"
    )]
    #[pyo3(name = "add_strategy_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_strategy_from_config(
        &mut self,
        _py: Python,
        config: ImportableStrategyConfig,
    ) -> PyResult<()> {
        log::debug!("`add_strategy_from_config` with: {config:?}");

        // Extract module and class name from strategy_path
        let parts: Vec<&str> = config.strategy_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "strategy_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing strategy from module: {module_name} class: {class_name}");

        // Phase 1: Create and configure the Python strategy, extract its strategy_id
        let (python_strategy, strategy_id) =
            Python::attach(|py| -> anyhow::Result<(Py<PyAny>, StrategyId)> {
                let strategy_module = py
                    .import(module_name)
                    .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
                let strategy_class = strategy_module
                    .getattr(class_name)
                    .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

                let config_instance =
                    create_config_instance(py, &config.config_path, &config.config)?;

                let python_strategy = if let Some(config_obj) = config_instance.clone() {
                    strategy_class.call1((config_obj,))?
                } else {
                    strategy_class.call0()?
                };

                log::debug!("Created Python strategy instance: {python_strategy:?}");

                let mut py_strategy_ref = python_strategy
                    .extract::<PyRefMut<PyStrategy>>()
                    .map_err(Into::<PyErr>::into)
                    .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

                // Extract inherited config fields from the Python config
                if let Some(config_obj) = config_instance.as_ref() {
                    if let Ok(strategy_id) = config_obj.getattr("strategy_id")
                        && !strategy_id.is_none()
                    {
                        let strategy_id_val = if let Ok(sid) = strategy_id.extract::<StrategyId>() {
                            sid
                        } else if let Ok(sid_str) = strategy_id.extract::<String>() {
                            StrategyId::new_checked(&sid_str)?
                        } else {
                            anyhow::bail!("Invalid `strategy_id` type");
                        };
                        py_strategy_ref.set_strategy_id(strategy_id_val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_events") {
                        py_strategy_ref.set_log_events(val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_commands") {
                        py_strategy_ref.set_log_commands(val);
                    }
                }

                py_strategy_ref.set_python_instance(python_strategy.clone().unbind());

                let strategy_id = py_strategy_ref.strategy_id();

                Ok((python_strategy.unbind(), strategy_id))
            })
            .map_err(to_pyruntime_err)?;

        // Validate no duplicate before any mutations
        if self
            .kernel()
            .trader
            .borrow()
            .strategy_ids()
            .contains(&strategy_id)
        {
            return Err(to_pyruntime_err(format!(
                "Strategy '{strategy_id}' is already registered"
            )));
        }

        // Phase 2: Create per-component clock via the trader.
        // This requires `&mut self` access to the kernel, which cannot be held
        // inside a `Python::attach` block, hence the separate phases.
        let trader_id = self.kernel().trader_id();
        let cache = self.kernel().cache();
        let portfolio = self.kernel().portfolio.clone();
        let component_id = ComponentId::new(strategy_id.inner().as_str());
        let clock = self
            .kernel_mut()
            .trader
            .borrow_mut()
            .create_component_clock(component_id);

        // Phase 3: Register the strategy with its dedicated clock
        Python::attach(|py| -> anyhow::Result<()> {
            let py_strategy = python_strategy.bind(py);
            let mut py_strategy_ref = py_strategy
                .extract::<PyRefMut<PyStrategy>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

            py_strategy_ref
                .register(trader_id, clock, cache, portfolio)
                .map_err(|e| anyhow::anyhow!("Failed to register PyStrategy: {e}"))?;

            log::debug!(
                "Internal PyStrategy registered: {}",
                py_strategy_ref.is_registered()
            );

            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        // Phase 4: Register in global registries and install event subscriptions
        Python::attach(|py| -> anyhow::Result<()> {
            let py_strategy = python_strategy.bind(py);
            let py_strategy_ref = py_strategy
                .cast::<PyStrategy>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyStrategy: {e}"))?;
            py_strategy_ref.borrow().register_in_global_registries();
            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        // TODO: Register external order claims with the execution manager
        // once PyStrategy exposes external_order_claims publicly.
        // Currently external_order_claims are only handled for Rust-native
        // strategies via LiveNode::add_strategy<T>().
        Python::attach(|py| {
            let py_strategy = python_strategy.bind(py);
            if let Ok(claims) = py_strategy.getattr("external_order_claims")
                && !claims.is_none()
                && claims.len().unwrap_or(0) > 0
            {
                log::warn!(
                    "Strategy '{strategy_id}' has external_order_claims configured, \
                     but these are not yet supported for Python strategies on the Rust backend"
                );
            }
        });

        self.kernel_mut()
            .trader
            .borrow_mut()
            .add_strategy_id_with_subscriptions::<PyStrategyInner>(strategy_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python strategy {strategy_id}");
        Ok(())
    }

    /// Adds a native Rust strategy from its config.
    ///
    /// The config type determines which built-in strategy is constructed.
    /// All execution happens in Rust; Python is the configuration layer.
    #[cfg(feature = "examples")]
    #[pyo3(name = "add_native_strategy")]
    fn py_add_native_strategy(&mut self, config: &Bound<'_, PyAny>) -> PyResult<()> {
        use nautilus_trading::examples::strategies::{
            DeltaNeutralVol, DeltaNeutralVolConfig, EmaCross, EmaCrossConfig, GridMarketMaker,
            GridMarketMakerConfig, HurstVpinDirectional, HurstVpinDirectionalConfig,
        };

        if let Ok(config) = config.extract::<EmaCrossConfig>() {
            self.add_strategy(EmaCross::from_config(config))
                .map_err(to_pyruntime_err)
        } else if let Ok(config) = config.extract::<GridMarketMakerConfig>() {
            self.add_strategy(GridMarketMaker::new(config))
                .map_err(to_pyruntime_err)
        } else if let Ok(config) = config.extract::<DeltaNeutralVolConfig>() {
            self.add_strategy(DeltaNeutralVol::new(config))
                .map_err(to_pyruntime_err)
        } else if let Ok(config) = config.extract::<HurstVpinDirectionalConfig>() {
            self.add_strategy(HurstVpinDirectional::new(config))
                .map_err(to_pyruntime_err)
        } else {
            let type_name = config.get_type().name()?;
            Err(to_pytype_err(format!(
                "Unsupported native strategy config type: {type_name}",
            )))
        }
    }

    /// Adds a native Rust actor from its config.
    ///
    /// The config type determines which built-in actor is constructed.
    /// All execution happens in Rust; Python is the configuration layer.
    #[cfg(feature = "examples")]
    #[pyo3(name = "add_native_actor")]
    fn py_add_native_actor(&mut self, config: &Bound<'_, PyAny>) -> PyResult<()> {
        use nautilus_trading::examples::actors::{BookImbalanceActor, BookImbalanceActorConfig};

        if let Ok(config) = config.extract::<BookImbalanceActorConfig>() {
            self.add_actor(BookImbalanceActor::from_config(config))
                .map_err(to_pyruntime_err)
        } else {
            let type_name = config.get_type().name()?;
            Err(to_pytype_err(format!(
                "Unsupported native actor config type: {type_name}",
            )))
        }
    }

    #[allow(
        unsafe_code,
        reason = "Required for Python exec algorithm component registration"
    )]
    #[pyo3(name = "add_exec_algorithm_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_exec_algorithm_from_config(
        &mut self,
        _py: Python,
        config: ImportableExecAlgorithmConfig,
    ) -> PyResult<()> {
        if self.is_running() {
            return Err(to_pyruntime_err(
                "Cannot add exec algorithm while node is running",
            ));
        }

        log::debug!("`add_exec_algorithm_from_config` with: {config:?}");

        let parts: Vec<&str> = config.exec_algorithm_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "exec_algorithm_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing exec algorithm from module: {module_name} class: {class_name}");

        // Phase 1: Create and configure the Python exec algorithm, extract its actor_id
        let (python_exec_algorithm, actor_id) =
            Python::attach(|py| -> anyhow::Result<(Py<PyAny>, ActorId)> {
                let algo_module = py
                    .import(module_name)
                    .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
                let algo_class = algo_module
                    .getattr(class_name)
                    .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

                let config_instance =
                    create_config_instance(py, &config.config_path, &config.config)?;

                let python_exec_algorithm = if let Some(config_obj) = config_instance.clone() {
                    algo_class.call1((config_obj,))?
                } else {
                    algo_class.call0()?
                };

                log::debug!("Created Python exec algorithm instance: {python_exec_algorithm:?}");

                let mut py_data_actor_ref = python_exec_algorithm
                    .extract::<PyRefMut<PyDataActor>>()
                    .map_err(Into::<PyErr>::into)
                    .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

                // Extract ID from config: prefer exec_algorithm_id, fall back to actor_id
                if let Some(config_obj) = config_instance.as_ref() {
                    let id_attr = config_obj
                        .getattr("exec_algorithm_id")
                        .ok()
                        .filter(|v| !v.is_none())
                        .or_else(|| config_obj.getattr("actor_id").ok().filter(|v| !v.is_none()));

                    if let Some(id_value) = id_attr {
                        let actor_id_val = if let Ok(eaid) = id_value.extract::<ExecAlgorithmId>() {
                            ActorId::new(eaid.inner().as_str())
                        } else if let Ok(aid) = id_value.extract::<ActorId>() {
                            aid
                        } else if let Ok(aid_str) = id_value.extract::<String>() {
                            ActorId::new_checked(&aid_str)?
                        } else {
                            anyhow::bail!("Invalid `exec_algorithm_id`/`actor_id` type");
                        };
                        py_data_actor_ref.set_actor_id(actor_id_val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_events") {
                        py_data_actor_ref.set_log_events(val);
                    }

                    if let Some(val) = extract_bool_config_attr(config_obj, "log_commands") {
                        py_data_actor_ref.set_log_commands(val);
                    }
                }

                py_data_actor_ref.set_python_instance(python_exec_algorithm.clone().unbind());

                let actor_id = py_data_actor_ref.actor_id();

                Ok((python_exec_algorithm.unbind(), actor_id))
            })
            .map_err(to_pyruntime_err)?;

        let exec_algorithm_id = ExecAlgorithmId::from(actor_id.inner().as_str());

        if self
            .kernel()
            .trader
            .borrow()
            .exec_algorithm_ids()
            .contains(&exec_algorithm_id)
        {
            return Err(to_pyruntime_err(format!(
                "Execution algorithm '{exec_algorithm_id}' is already registered"
            )));
        }

        // Phase 2: Create per-component clock via the trader.
        // This requires `&mut self` access to the kernel, which cannot be held
        // inside a `Python::attach` block, hence the separate phases.
        let trader_id = self.kernel().trader_id();
        let cache = self.kernel().cache();
        let component_id = ComponentId::new(actor_id.inner().as_str());
        let clock = self
            .kernel_mut()
            .trader
            .borrow_mut()
            .create_component_clock(component_id);

        // Phase 3: Register the exec algorithm with its dedicated clock
        Python::attach(|py| -> anyhow::Result<()> {
            let py_algo = python_exec_algorithm.bind(py);
            let mut py_data_actor_ref = py_algo
                .extract::<PyRefMut<PyDataActor>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

            py_data_actor_ref
                .register(trader_id, clock, cache)
                .map_err(|e| anyhow::anyhow!("Failed to register PyDataActor: {e}"))?;

            log::debug!(
                "Internal PyDataActor registered: {}, state: {:?}",
                py_data_actor_ref.is_registered(),
                py_data_actor_ref.state()
            );

            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        // Phase 4: Register in global registries and track for lifecycle
        Python::attach(|py| -> anyhow::Result<()> {
            let py_algo = python_exec_algorithm.bind(py);
            let py_data_actor_ref = py_algo
                .cast::<PyDataActor>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyDataActor: {e}"))?;
            py_data_actor_ref.borrow().register_in_global_registries();
            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        self.kernel_mut()
            .trader
            .borrow_mut()
            .add_exec_algorithm_id_for_lifecycle(exec_algorithm_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python exec algorithm {exec_algorithm_id}");
        Ok(())
    }

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
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.live")]
pub struct LiveNodeBuilderPy {
    inner: Rc<RefCell<Option<LiveNodeBuilder>>>,
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl LiveNodeBuilderPy {
    #[pyo3(name = "with_instance_id")]
    fn py_with_instance_id(&self, instance_id: UUID4) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_instance_id(instance_id));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_load_state")]
    fn py_with_load_state(&self, load_state: bool) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_load_state(load_state));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_save_state")]
    fn py_with_save_state(&self, save_state: bool) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_save_state(save_state));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_timeout_connection")]
    fn py_with_timeout_connection(&self, timeout_secs: u64) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_timeout_connection(timeout_secs));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_timeout_reconciliation")]
    fn py_with_timeout_reconciliation(&self, timeout_secs: u64) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_timeout_reconciliation(timeout_secs));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_timeout_portfolio")]
    fn py_with_timeout_portfolio(&self, timeout_secs: u64) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_timeout_portfolio(timeout_secs));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_timeout_disconnection_secs")]
    fn py_with_timeout_disconnection_secs(&self, timeout_secs: u64) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_timeout_disconnection_secs(timeout_secs));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_delay_post_stop_secs")]
    fn py_with_delay_post_stop_secs(&self, delay_secs: u64) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_delay_post_stop_secs(delay_secs));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_delay_shutdown_secs")]
    fn py_with_delay_shutdown_secs(&self, delay_secs: u64) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_delay_shutdown_secs(delay_secs));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_reconciliation")]
    fn py_with_reconciliation(&self, reconciliation: bool) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_reconciliation(reconciliation));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_reconciliation_lookback_mins")]
    fn py_with_reconciliation_lookback_mins(&self, mins: u32) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_reconciliation_lookback_mins(mins));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_cache_config")]
    fn py_with_cache_config(&self, config: CacheConfig) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_cache_config(config));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_portfolio_config")]
    fn py_with_portfolio_config(&self, config: PortfolioConfig) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_portfolio_config(config));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_data_engine_config")]
    fn py_with_data_engine_config(&self, config: LiveDataEngineConfig) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_data_engine_config(config));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_risk_engine_config")]
    fn py_with_risk_engine_config(&self, config: LiveRiskEngineConfig) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_risk_engine_config(config));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_exec_engine_config")]
    fn py_with_exec_engine_config(&self, config: LiveExecEngineConfig) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_exec_engine_config(config));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "with_logging")]
    fn py_with_logging(&self, logging: LoggerConfig) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            *inner_ref = Some(builder.with_logging(logging));
            Ok(Self {
                inner: self.inner.clone(),
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "add_data_client")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_data_client(
        &self,
        name: Option<String>,
        factory: Py<PyAny>,
        config: Py<PyAny>,
    ) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            Python::attach(|py| -> PyResult<Self> {
                // Use the global registry to extract Py<PyAny>s to trait objects
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
                    Err(e) => Err(to_pyruntime_err(format!("Failed to add data client: {e}"))),
                }
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "add_exec_client")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_exec_client(
        &self,
        name: Option<String>,
        factory: Py<PyAny>,
        config: Py<PyAny>,
    ) -> PyResult<Self> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            Python::attach(|py| -> PyResult<Self> {
                let registry = get_global_pyo3_registry();

                let boxed_factory = registry.extract_exec_factory(py, factory.clone_ref(py))?;
                let boxed_config = registry.extract_config(py, config.clone_ref(py))?;

                let factory_name = factory
                    .getattr(py, "name")?
                    .call0(py)?
                    .extract::<String>(py)?;
                let client_name = name.unwrap_or(factory_name);

                match builder.add_exec_client(Some(client_name), boxed_factory, boxed_config) {
                    Ok(updated_builder) => {
                        *inner_ref = Some(updated_builder);
                        Ok(Self {
                            inner: self.inner.clone(),
                        })
                    }
                    Err(e) => Err(to_pyruntime_err(format!("Failed to add exec client: {e}"))),
                }
            })
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    #[pyo3(name = "build")]
    fn py_build(&self) -> PyResult<LiveNode> {
        let mut inner_ref = self.inner.borrow_mut();
        if let Some(builder) = inner_ref.take() {
            match builder.build() {
                Ok(node) => Ok(node),
                Err(e) => Err(to_pyruntime_err(e)),
            }
        } else {
            Err(to_pyruntime_err("Builder already consumed"))
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

/// Creates a Python config instance from a config path and config dictionary.
///
/// This helper is shared between `add_actor_from_config` and `add_strategy_from_config`.
/// It handles:
/// 1. Importing the config class from the module path
/// 2. Converting the `HashMap<String, serde_json::Value>` to a Python dict
/// 3. Trying kwargs-first construction, falling back to default + setattr
/// 4. Calling `__post_init__` for dataclasses when using the setattr path
fn create_config_instance<'py>(
    py: Python<'py>,
    config_path: &str,
    config: &HashMap<String, serde_json::Value>,
) -> anyhow::Result<Option<Bound<'py, PyAny>>> {
    if config_path.is_empty() && config.is_empty() {
        log::debug!("No config_path or empty config, using None");
        return Ok(None);
    }

    let config_parts: Vec<&str> = config_path.split(':').collect();
    if config_parts.len() != 2 {
        anyhow::bail!("config_path must be in format 'module.path:ClassName', was {config_path}");
    }
    let (config_module_name, config_class_name) = (config_parts[0], config_parts[1]);

    log::debug!(
        "Importing config class from module: {config_module_name} class: {config_class_name}"
    );

    let config_module = py
        .import(config_module_name)
        .map_err(|e| anyhow::anyhow!("Failed to import config module {config_module_name}: {e}"))?;
    let config_class = config_module
        .getattr(config_class_name)
        .map_err(|e| anyhow::anyhow!("Failed to get config class {config_class_name}: {e}"))?;

    // Convert config dict to Python dict
    let py_dict = PyDict::new(py);

    for (key, value) in config {
        let json_str = serde_json::to_string(value)
            .map_err(|e| anyhow::anyhow!("Failed to serialize config value: {e}"))?;
        let py_value = PyModule::import(py, "json")?.call_method("loads", (json_str,), None)?;
        py_dict.set_item(key, py_value)?;
    }

    log::debug!("Created config dict: {py_dict:?}");

    // Try kwargs first, then default constructor with setattr
    let config_instance = match config_class.call((), Some(&py_dict)) {
        Ok(instance) => {
            log::debug!("Created config instance with kwargs");
            instance
        }
        Err(kwargs_err) => {
            log::debug!("Failed to create config with kwargs: {kwargs_err}");

            match config_class.call0() {
                Ok(instance) => {
                    log::debug!("Created default config instance, setting attributes");
                    for (key, value) in config {
                        let json_str = serde_json::to_string(value).map_err(|e| {
                            anyhow::anyhow!("Failed to serialize config value: {e}")
                        })?;
                        let py_value = PyModule::import(py, "json")?.call_method(
                            "loads",
                            (json_str,),
                            None,
                        )?;

                        if let Err(setattr_err) = instance.setattr(key, py_value) {
                            log::warn!("Failed to set attribute {key}: {setattr_err}");
                        }
                    }

                    // Only call __post_init__ if it exists (setattr path
                    // needs it, kwargs path already triggered it via __init__)
                    if instance.hasattr("__post_init__")? {
                        instance.call_method0("__post_init__")?;
                    }

                    instance
                }
                Err(default_err) => {
                    anyhow::bail!(
                        "Failed to create config instance. \
                         Tried kwargs: {kwargs_err}, default: {default_err}"
                    );
                }
            }
        }
    };

    log::debug!("Created config instance: {config_instance:?}");

    Ok(Some(config_instance))
}

/// Extracts an optional boolean attribute from a Python config object.
///
/// Returns `None` if the attribute doesn't exist or isn't a bool,
/// without raising an error (config fields are optional overrides).
fn extract_bool_config_attr(config_obj: &Bound<'_, PyAny>, attr: &str) -> Option<bool> {
    config_obj
        .getattr(attr)
        .ok()
        .and_then(|val| val.extract::<bool>().ok())
}

#[cfg(all(test, feature = "python"))]
mod tests {
    use std::{
        any::Any,
        cell::RefCell,
        collections::HashMap,
        ffi::CString,
        fmt::Debug,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use async_trait::async_trait;
    use nautilus_common::{
        cache::Cache,
        clients::DataClient,
        clock::Clock,
        enums::Environment,
        factories::{ClientConfig, DataClientFactory},
        live::runner::get_data_event_sender,
        messages::{
            DataEvent, DataResponse,
            data::{BarsResponse, RequestBars},
        },
        msgbus::get_message_bus,
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{Bar, BarType},
        identifiers::{ClientId, TraderId, Venue},
        types::{Price, Quantity},
    };
    use nautilus_trading::{ImportableStrategyConfig, python::strategy::PyStrategy};
    use pyo3::{
        Python,
        types::{PyAnyMethods, PyDict, PyModule, PyModuleMethods},
    };

    use super::LiveNode;
    #[derive(Debug, Default)]
    struct TestDataClientConfig;

    impl ClientConfig for TestDataClientConfig {
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[derive(Debug)]
    struct TestHistoricalBarsDataClientFactory {
        request_count: Arc<AtomicUsize>,
        response_sent_count: Arc<AtomicUsize>,
        handler_visible_count: Arc<AtomicUsize>,
    }

    impl TestHistoricalBarsDataClientFactory {
        fn new(
            request_count: Arc<AtomicUsize>,
            response_sent_count: Arc<AtomicUsize>,
            handler_visible_count: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                request_count,
                response_sent_count,
                handler_visible_count,
            }
        }
    }

    impl DataClientFactory for TestHistoricalBarsDataClientFactory {
        fn create(
            &self,
            name: &str,
            _config: &dyn ClientConfig,
            _cache: Rc<RefCell<Cache>>,
            _clock: Rc<RefCell<dyn Clock>>,
        ) -> anyhow::Result<Box<dyn DataClient>> {
            Ok(Box::new(TestHistoricalBarsDataClient::new(
                ClientId::from(name),
                Venue::from("SIM"),
                self.request_count.clone(),
                self.response_sent_count.clone(),
                self.handler_visible_count.clone(),
            )))
        }

        fn name(&self) -> &'static str {
            "TEST_DATA"
        }

        fn config_type(&self) -> &'static str {
            "TestDataClientConfig"
        }
    }

    #[derive(Debug)]
    struct TestHistoricalBarsDataClient {
        client_id: ClientId,
        venue: Venue,
        connected: Arc<AtomicBool>,
        request_count: Arc<AtomicUsize>,
        response_sent_count: Arc<AtomicUsize>,
        handler_visible_count: Arc<AtomicUsize>,
    }

    impl TestHistoricalBarsDataClient {
        fn new(
            client_id: ClientId,
            venue: Venue,
            request_count: Arc<AtomicUsize>,
            response_sent_count: Arc<AtomicUsize>,
            handler_visible_count: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                client_id,
                venue,
                connected: Arc::new(AtomicBool::new(false)),
                request_count,
                response_sent_count,
                handler_visible_count,
            }
        }

        fn make_bar(bar_type: BarType) -> Bar {
            Bar::new(
                bar_type,
                Price::from("1.0000"),
                Price::from("1.1000"),
                Price::from("0.9000"),
                Price::from("1.0500"),
                Quantity::from("1000"),
                UnixNanos::from(1_700_000_000_000_000_000u64),
                UnixNanos::from(1_700_000_000_000_000_001u64),
            )
        }
    }

    #[async_trait(?Send)]
    impl DataClient for TestHistoricalBarsDataClient {
        fn client_id(&self) -> ClientId {
            self.client_id
        }

        fn venue(&self) -> Option<Venue> {
            Some(self.venue)
        }

        fn start(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn dispose(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn is_connected(&self) -> bool {
            self.connected.load(Ordering::Relaxed)
        }

        fn is_disconnected(&self) -> bool {
            !self.is_connected()
        }

        async fn connect(&mut self) -> anyhow::Result<()> {
            self.connected.store(true, Ordering::Relaxed);
            Ok(())
        }

        async fn disconnect(&mut self) -> anyhow::Result<()> {
            self.connected.store(false, Ordering::Relaxed);
            Ok(())
        }

        fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
            self.request_count.fetch_add(1, Ordering::Relaxed);

            if get_message_bus()
                .borrow()
                .get_response_handler(&request.request_id)
                .is_some()
            {
                self.handler_visible_count.fetch_add(1, Ordering::Relaxed);
            }

            let sender = get_data_event_sender();
            let client_id = self.client_id;
            let response_sent_count = self.response_sent_count.clone();
            let response = BarsResponse::new(
                request.request_id,
                client_id,
                request.bar_type,
                vec![Self::make_bar(request.bar_type)],
                None,
                None,
                UnixNanos::from(1_700_000_000_000_000_002u64),
                None,
            );

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                response_sent_count.fetch_add(1, Ordering::Relaxed);
                sender
                    .send(DataEvent::Response(DataResponse::Bars(response)))
                    .expect("test bars response should send");
            });

            Ok(())
        }
    }

    fn install_tracking_strategy_module(py: Python<'_>, module_name: &str) {
        let module = PyModule::new(py, module_name).expect("test module should create");
        module
            .setattr("Strategy", py.get_type::<PyStrategy>())
            .expect("Strategy type should bind");
        module
            .setattr("BarType", py.get_type::<BarType>())
            .expect("BarType type should bind");
        module
            .setattr("RESULTS", PyDict::new(py))
            .expect("RESULTS should bind");

        let code = CString::new(
            r#"
RESULTS["on_start"] = 0
RESULTS["on_historical_bars"] = 0
RESULTS["historical_bar_count"] = 0
RESULTS["last_request_id"] = ""

class HistoricalBarsStrategy(Strategy):
    def __init__(self):
        super().__init__()
        self.bar_type = BarType.from_str("AUDUSD.SIM-1-MINUTE-LAST-EXTERNAL")

    def on_start(self):
        RESULTS["on_start"] += 1
        RESULTS["last_request_id"] = self.request_bars(self.bar_type)

    def on_stop(self):
        pass

    def on_historical_bars(self, bars):
        RESULTS["on_historical_bars"] += 1
        RESULTS["historical_bar_count"] += len(bars)
"#,
        )
        .expect("python test code should be valid CString");

        py.run(code.as_c_str(), Some(&module.dict()), None)
            .expect("test strategy code should execute");

        let sys_modules = py
            .import("sys")
            .expect("sys should import")
            .getattr("modules")
            .expect("sys.modules should exist");
        sys_modules
            .set_item(module_name, module)
            .expect("test strategy module should register");
    }

    fn get_results(py: Python<'_>, module_name: &str) -> (usize, usize, usize) {
        let module = py
            .import(module_name)
            .expect("test strategy module should import");
        let results_obj = module.getattr("RESULTS").expect("RESULTS should exist");
        let results = results_obj
            .cast::<PyDict>()
            .expect("RESULTS should be a dict");

        let on_start = results
            .get_item("on_start")
            .expect("on_start key should exist")
            .extract::<usize>()
            .expect("on_start should extract");
        let on_historical_bars = results
            .get_item("on_historical_bars")
            .expect("on_historical_bars key should exist")
            .extract::<usize>()
            .expect("on_historical_bars should extract");
        let historical_bar_count = results
            .get_item("historical_bar_count")
            .expect("historical_bar_count key should exist")
            .extract::<usize>()
            .expect("historical_bar_count should extract");

        (on_start, on_historical_bars, historical_bar_count)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_live_node_pystrategy_request_bars_dispatches_on_historical_bars() {
        Python::initialize();

        let module_name = "test_live_node_historical_bars_strategy";
        Python::attach(|py| install_tracking_strategy_module(py, module_name));

        let request_count = Arc::new(AtomicUsize::new(0));
        let response_sent_count = Arc::new(AtomicUsize::new(0));
        let handler_visible_count = Arc::new(AtomicUsize::new(0));
        let factory = TestHistoricalBarsDataClientFactory::new(
            request_count.clone(),
            response_sent_count.clone(),
            handler_visible_count.clone(),
        );
        let config = TestDataClientConfig;

        let mut node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Sandbox)
            .unwrap()
            .with_reconciliation(false)
            .with_delay_post_stop_secs(0)
            .with_timeout_connection(1)
            .add_data_client(
                Some("TEST_DATA".to_string()),
                Box::new(factory),
                Box::new(config),
            )
            .unwrap()
            .build()
            .unwrap();

        let importable = ImportableStrategyConfig {
            strategy_path: format!("{module_name}:HistoricalBarsStrategy"),
            config_path: String::new(),
            config: HashMap::new(),
        };

        Python::attach(|py| {
            node.py_add_strategy_from_config(py, importable)
                .expect("strategy should register");
        });

        let handle = node.handle();
        let stop_handle = handle.clone();
        let response_sent_count_for_stop = response_sent_count.clone();

        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

            loop {
                if response_sent_count_for_stop.load(Ordering::Relaxed) == 1
                    || tokio::time::Instant::now() >= deadline
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            stop_handle.stop();
        });

        node.run().await.expect("node should run cleanly");

        let (on_start, on_historical_bars, historical_bar_count) =
            Python::attach(|py| get_results(py, module_name));

        assert_eq!(request_count.load(Ordering::Relaxed), 1);
        assert_eq!(handler_visible_count.load(Ordering::Relaxed), 1);
        assert_eq!(response_sent_count.load(Ordering::Relaxed), 1);
        assert_eq!(on_start, 1);
        assert_eq!(on_historical_bars, 1);
        assert_eq!(historical_bar_count, 1);
    }
}
