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

//! Python bindings for backtest node.

use std::collections::HashMap;

use nautilus_common::{actor::data_actor::ImportableActorConfig, python::actor::PyDataActor};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::identifiers::{ActorId, ComponentId, StrategyId};
use nautilus_trading::{
    ImportableStrategyConfig,
    python::strategy::{PyStrategy, PyStrategyInner},
};
use pyo3::{prelude::*, types::PyDict};

use crate::{config::BacktestRunConfig, node::BacktestNode, result::BacktestResult};

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl BacktestNode {
    /// Orchestrates catalog-driven backtests from run configurations.
    ///
    /// `BacktestNode` connects the `ParquetDataCatalog` with `BacktestEngine` to load
    /// historical data and run backtests. Supports both oneshot and streaming modes.
    #[new]
    fn py_new(configs: Vec<BacktestRunConfig>) -> PyResult<Self> {
        Self::new(configs).map_err(to_pyruntime_err)
    }

    /// Builds backtest engines from the run configurations.
    ///
    /// For each config, creates a `BacktestEngine`, adds venues, and loads
    /// instruments from the catalog.
    ///
    /// # Errors
    ///
    /// Returns an error if engine creation, venue setup, or instrument loading fails.
    #[pyo3(name = "build")]
    fn py_build(&mut self) -> PyResult<()> {
        self.build().map_err(to_pyruntime_err)
    }

    /// Runs all configured backtests and returns results.
    ///
    /// Automatically calls `build()` if engines have not been created yet.
    /// For each run config, loads data from the catalog and runs the engine.
    /// Supports both oneshot (`chunk_size = None`) and streaming modes.
    ///
    /// # Errors
    ///
    /// Returns an error if building, data loading, or engine execution fails.
    #[pyo3(name = "run")]
    fn py_run(&mut self) -> PyResult<Vec<BacktestResult>> {
        self.run().map_err(to_pyruntime_err)
    }

    /// Disposes all engines and releases resources.
    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) {
        self.dispose();
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
        run_config_id: &str,
        config: ImportableActorConfig,
    ) -> PyResult<()> {
        log::debug!("`add_actor_from_config` with: {config:?}");

        let engine = self.get_engine_mut(run_config_id).ok_or_else(|| {
            to_pyruntime_err(format!("No engine for run config '{run_config_id}'"))
        })?;

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
                        let actor_id_val = if let Ok(actor_id_val) = actor_id.extract::<ActorId>() {
                            actor_id_val
                        } else if let Ok(actor_id_str) = actor_id.extract::<String>() {
                            ActorId::new_checked(&actor_id_str)?
                        } else {
                            anyhow::bail!("Invalid `actor_id` type");
                        };
                        py_data_actor_ref.set_actor_id(actor_id_val);
                    }

                    if let Ok(log_events) = config_obj.getattr("log_events")
                        && let Ok(log_events_val) = log_events.extract::<bool>()
                    {
                        py_data_actor_ref.set_log_events(log_events_val);
                    }

                    if let Ok(log_commands) = config_obj.getattr("log_commands")
                        && let Ok(log_commands_val) = log_commands.extract::<bool>()
                    {
                        py_data_actor_ref.set_log_commands(log_commands_val);
                    }
                }

                py_data_actor_ref.set_python_instance(python_actor.clone().unbind());

                let actor_id = py_data_actor_ref.actor_id();

                Ok((python_actor.unbind(), actor_id))
            })
            .map_err(to_pyruntime_err)?;

        // Validate no duplicate before any mutations
        if engine
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

        // Phase 2: Create per-component clock via the trader (individual
        // TestClock in backtest so each actor gets its own default timer handler)
        let trader_id = engine.kernel().config.trader_id();
        let cache = engine.kernel().cache.clone();
        let component_id = ComponentId::new(actor_id.inner().as_str());
        let clock = engine
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

        engine
            .kernel_mut()
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
        run_config_id: &str,
        config: ImportableStrategyConfig,
    ) -> PyResult<()> {
        log::debug!("`add_strategy_from_config` with: {config:?}");

        let engine = self.get_engine_mut(run_config_id).ok_or_else(|| {
            to_pyruntime_err(format!("No engine for run config '{run_config_id}'"))
        })?;

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

                    if let Ok(log_events) = config_obj.getattr("log_events")
                        && let Ok(log_events_val) = log_events.extract::<bool>()
                    {
                        py_strategy_ref.set_log_events(log_events_val);
                    }

                    if let Ok(log_commands) = config_obj.getattr("log_commands")
                        && let Ok(log_commands_val) = log_commands.extract::<bool>()
                    {
                        py_strategy_ref.set_log_commands(log_commands_val);
                    }
                }

                py_strategy_ref.set_python_instance(python_strategy.clone().unbind());

                let strategy_id = py_strategy_ref.strategy_id();

                Ok((python_strategy.unbind(), strategy_id))
            })
            .map_err(to_pyruntime_err)?;

        // Validate no duplicate before any mutations
        if engine
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

        // Phase 2: Create per-component clock via the trader (individual
        // TestClock in backtest so each strategy gets its own default timer handler)
        let trader_id = engine.kernel().config.trader_id();
        let cache = engine.kernel().cache.clone();
        let portfolio = engine.kernel().portfolio.clone();
        let component_id = ComponentId::new(strategy_id.inner().as_str());
        let clock = engine
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

        engine
            .kernel_mut()
            .trader
            .borrow_mut()
            .add_strategy_id_with_subscriptions::<PyStrategyInner>(strategy_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python strategy {strategy_id}");
        Ok(())
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

pub(crate) fn create_config_instance<'py>(
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
