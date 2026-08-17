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

//! Registration of Python actors, strategies, controllers, and execution algorithms.
//!
//! Every Python component reaches the trader through one of the `add_python_*` methods here, so the
//! sequence each component needs (component clock, register, global registries, lifecycle tracking)
//! is expressed once. Registering in the global registries retains the component's Python wrapper,
//! so a caller of that path cannot forget the wrapper. A [`PyExecutionAlgorithm`] registers through
//! the native path instead, so [`Trader::add_py_execution_algorithm_instance`] retains its wrapper
//! directly and is the only place that has to.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use nautilus_common::{
    actor::data_actor::ImportableActorConfig,
    python::{
        actor::{
            PyDataActor, PyDataActorInner, apply_class_derived_actor_id,
            register_python_exec_algorithm_endpoint,
        },
        wrappers::retain_python_wrapper,
    },
};
use nautilus_model::identifiers::{
    ActorId, ComponentId, ExecAlgorithmId, StrategyId, normalize_order_id_tag,
};
use nautilus_trading::{
    ImportableControllerConfig, ImportableStrategyConfig,
    python::{
        algorithm::PyExecutionAlgorithm,
        strategy::{PyStrategy, PyStrategyInner},
    },
};
use pyo3::{
    prelude::*,
    types::{PyDict, PyModule},
};

use crate::{registration::ensure_unique_order_id_tag, trader::Trader};

impl Trader {
    /// Adds an importable Python actor to the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor cannot be imported, configured, registered, or tracked.
    pub fn add_actor_from_importable_config(
        &mut self,
        config: &ImportableActorConfig,
    ) -> anyhow::Result<ActorId> {
        self.validate_actor_or_strategy_registration()?;

        let (python_actor, actor_id) = create_python_actor(config)?;
        if self.actor_ids.contains(&actor_id) {
            anyhow::bail!("Actor {actor_id} is already registered");
        }

        self.add_python_actor_instance(&python_actor, actor_id)?;

        log::info!(
            "Registered Python actor {actor_id} with trader {}",
            self.trader_id
        );
        Ok(actor_id)
    }

    /// Adds a constructed Python actor instance to the trader under `actor_id`.
    ///
    /// The actor must already be configured; this runs the registration sequence every Python
    /// actor needs and rolls back everything the attempt created if any step fails.
    ///
    /// # Errors
    ///
    /// Returns an error if the trader already tracks a component under the actor's ID, or if the
    /// actor cannot be registered or tracked.
    pub fn add_python_actor_instance(
        &mut self,
        actor: &Py<PyAny>,
        actor_id: ActorId,
    ) -> anyhow::Result<()> {
        let component_id = ComponentId::from(actor_id);
        self.ensure_component_id_available(component_id)?;

        if let Err(e) = self.register_python_actor_components(actor, actor_id) {
            // Leave no clock, registry entry, or wrapper behind from a failed attempt
            self.release_component(component_id);
            return Err(e);
        }

        Ok(())
    }

    fn register_python_actor_components(
        &mut self,
        actor: &Py<PyAny>,
        actor_id: ActorId,
    ) -> anyhow::Result<()> {
        self.register_python_data_actor(actor, ComponentId::from(actor_id))?;

        self.add_actor_id_for_lifecycle::<PyDataActorInner>(actor_id)
    }

    /// Adds an importable Python controller to the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the controller cannot be imported, configured, registered, or tracked.
    pub fn add_controller_from_importable_config(
        trader: &Rc<RefCell<Self>>,
        config: &ImportableControllerConfig,
    ) -> anyhow::Result<ActorId> {
        trader.borrow().validate_actor_or_strategy_registration()?;

        let actor_config = ImportableActorConfig {
            actor_path: config.controller_path.clone(),
            config_path: config.config_path.clone(),
            config: config.config.clone(),
        };
        let (python_controller, actor_id) = create_python_actor(&actor_config)?;
        if trader.borrow().actor_ids.contains(&actor_id) {
            anyhow::bail!("Actor {actor_id} is already registered");
        }

        crate::python::controller::bind_controller_trader(&python_controller, trader)?;

        trader
            .borrow_mut()
            .add_python_actor_instance(&python_controller, actor_id)?;

        log::info!(
            "Registered Python controller {actor_id} with trader {}",
            trader.borrow().trader_id
        );
        Ok(actor_id)
    }

    /// Adds an importable Python strategy to the trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy cannot be imported, configured, registered, or tracked.
    pub fn add_strategy_from_importable_config(
        &mut self,
        config: &ImportableStrategyConfig,
    ) -> anyhow::Result<StrategyId> {
        // Checked before importing and constructing the Python class so a rejected addition never
        // runs user constructor code
        self.validate_actor_or_strategy_registration()?;

        let python_strategy = create_python_strategy(config)?;

        self.add_python_strategy_instance(&python_strategy)
    }

    /// Adds a constructed Python strategy instance to the trader.
    ///
    /// This is the instance-based counterpart to [`Self::add_strategy_from_importable_config`]:
    /// the strategy is already constructed in Python, avoiding the `dict`-to-JSON round trip of
    /// the importable-config path. The strategy ID, order ID tag, and logging flags are sourced
    /// from the instance's retained `.config`.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy cannot be configured, registered, or tracked.
    pub fn add_python_strategy_instance(
        &mut self,
        strategy: &Py<PyAny>,
    ) -> anyhow::Result<StrategyId> {
        self.prepare_python_strategy_instance(strategy)?;
        self.commit_python_strategy_instance(strategy)
    }

    /// Prepares a constructed Python strategy instance for registration without committing it.
    ///
    /// # Errors
    ///
    /// Returns an error if the strategy cannot be configured, or its ID or order ID tag is
    /// already registered.
    pub fn prepare_python_strategy_instance(
        &mut self,
        strategy: &Py<PyAny>,
    ) -> anyhow::Result<StrategyId> {
        self.validate_actor_or_strategy_registration()?;

        let existing_order_id_tags: Vec<&str> =
            self.strategy_ids.iter().map(StrategyId::get_tag).collect();

        let strategy_id = Python::attach(|py| -> anyhow::Result<StrategyId> {
            let bound = strategy.bind(py);

            let config_instance = bound
                .getattr("config")
                .ok()
                .filter(|config| !config.is_none());

            let class_name = bound.get_type().name()?.to_string();

            let mut py_strategy_ref = bound
                .extract::<PyRefMut<PyStrategy>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

            if let Some(config_obj) = config_instance.as_ref() {
                configure_py_strategy(&mut py_strategy_ref, config_obj)?;
            }

            // Mirrors the native path: a configured ID is kept, otherwise the runtime class name
            // takes the configured order ID tag, or the next positional tag
            let runtime_order_id_tag = py_strategy_ref.order_id_tag();
            let strategy_id = if let Some(strategy_id) = py_strategy_ref.configured_strategy_id() {
                strategy_id
            } else {
                let order_id_tag = normalize_order_id_tag(runtime_order_id_tag.as_deref())
                    .map_or_else(
                        || format!("{:03}", existing_order_id_tags.len()),
                        str::to_string,
                    );
                StrategyId::new_checked(format!("{class_name}-{order_id_tag}"))?
            };

            if self.strategy_ids.contains(&strategy_id) {
                anyhow::bail!("Strategy {strategy_id} is already registered");
            }
            ensure_unique_order_id_tag(&existing_order_id_tags, strategy_id.get_tag())?;

            py_strategy_ref.set_strategy_id(strategy_id)?;
            py_strategy_ref.set_python_instance(bound)?;

            Ok(py_strategy_ref.strategy_id())
        })?;

        // Rejected here as well as on commit so a caller which acts between the two phases, such
        // as registering external order claims, does not act on a doomed registration
        self.ensure_component_id_available(ComponentId::from(strategy_id))?;

        Ok(strategy_id)
    }

    /// Commits a previously prepared Python strategy instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the trader already tracks a component under the strategy's ID, or if
    /// the strategy cannot be registered or its subscriptions cannot be installed.
    pub fn commit_python_strategy_instance(
        &mut self,
        strategy: &Py<PyAny>,
    ) -> anyhow::Result<StrategyId> {
        let strategy_id = Python::attach(|py| -> anyhow::Result<StrategyId> {
            Ok(strategy
                .bind(py)
                .extract::<PyRef<PyStrategy>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?
                .strategy_id())
        })?;

        let component_id = ComponentId::from(strategy_id);
        self.ensure_component_id_available(component_id)?;

        if let Err(e) = self.register_python_strategy_components(strategy, strategy_id) {
            // Leave no clock, registry entry, or wrapper behind from a failed attempt
            self.release_component(component_id);
            return Err(e);
        }

        log::info!(
            "Registered Python strategy {strategy_id} with trader {}",
            self.trader_id
        );
        Ok(strategy_id)
    }

    fn register_python_strategy_components(
        &mut self,
        strategy: &Py<PyAny>,
        strategy_id: StrategyId,
    ) -> anyhow::Result<()> {
        let clock = self.create_component_clock(ComponentId::from(strategy_id));
        let trader_id = self.trader_id;
        let cache = self.cache.clone();
        let portfolio = self.portfolio.clone();

        Python::attach(|py| -> anyhow::Result<()> {
            let py_strategy = strategy.bind(py);
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
        })?;

        Python::attach(|py| -> anyhow::Result<()> {
            let py_strategy = strategy.bind(py);
            let py_strategy_ref = py_strategy
                .cast::<PyStrategy>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyStrategy: {e}"))?;
            py_strategy_ref.borrow().register_in_global_registries()?;
            Ok(())
        })?;

        self.add_strategy_id_with_subscriptions::<PyStrategyInner>(strategy_id)
    }

    /// Adds a constructed [`PyExecutionAlgorithm`] instance to the trader.
    ///
    /// `wrapper` is the Python object which owns `algorithm`; the trader's registries keep it
    /// alive for as long as the algorithm stays registered.
    ///
    /// # Errors
    ///
    /// Returns an error if the trader already tracks a component under the algorithm's ID, or if
    /// the algorithm cannot be registered or tracked.
    pub fn add_py_execution_algorithm_instance(
        &mut self,
        algorithm: PyExecutionAlgorithm,
        wrapper: &Py<PyAny>,
    ) -> anyhow::Result<ExecAlgorithmId> {
        let exec_algorithm_id = algorithm.exec_algorithm_id();

        // Checked before the shared guard so a same-kind duplicate keeps its own message
        if self.exec_algorithm_ids.contains(&exec_algorithm_id) {
            anyhow::bail!("Execution algorithm '{exec_algorithm_id}' is already registered");
        }

        let component_id = ComponentId::from(exec_algorithm_id);
        self.ensure_component_id_available(component_id)?;

        if let Err(e) = self.add_exec_algorithm(algorithm) {
            // Without this the guard sees the stranded clock and dead-ends this ID until disposal
            self.release_component(component_id);
            return Err(e);
        }

        Python::attach(|py| {
            retain_python_wrapper(component_id, wrapper.clone_ref(py));
        });

        Ok(exec_algorithm_id)
    }

    /// Adds a constructed Python actor instance to the trader as an execution algorithm.
    ///
    /// This is the [`PyDataActor`]-backed execution algorithm path, used when the Python class
    /// derives from `DataActor` rather than `ExecutionAlgorithm`.
    ///
    /// # Errors
    ///
    /// Returns an error if the trader already tracks a component under the algorithm's ID, or if
    /// the algorithm cannot be registered or tracked.
    pub fn add_python_exec_algorithm_instance(
        &mut self,
        exec_algorithm: &Py<PyAny>,
        actor_id: ActorId,
    ) -> anyhow::Result<ExecAlgorithmId> {
        let exec_algorithm_id = ExecAlgorithmId::from(actor_id.inner().as_str());

        if self.exec_algorithm_ids.contains(&exec_algorithm_id) {
            anyhow::bail!("Execution algorithm '{exec_algorithm_id}' is already registered");
        }

        let component_id = ComponentId::from(exec_algorithm_id);
        self.ensure_component_id_available(component_id)?;

        if let Err(e) =
            self.register_python_exec_algorithm_components(exec_algorithm, exec_algorithm_id)
        {
            // Leave no clock, registry entry, or wrapper behind from a failed attempt
            self.release_component(component_id);
            return Err(e);
        }

        Ok(exec_algorithm_id)
    }

    fn register_python_exec_algorithm_components(
        &mut self,
        exec_algorithm: &Py<PyAny>,
        exec_algorithm_id: ExecAlgorithmId,
    ) -> anyhow::Result<()> {
        self.register_python_data_actor(exec_algorithm, ComponentId::from(exec_algorithm_id))?;

        self.add_exec_algorithm_id_for_lifecycle(exec_algorithm_id)?;

        // Registered once tracking has succeeded, so a rolled back attempt leaves no endpoint
        register_python_exec_algorithm_endpoint(exec_algorithm_id);

        Ok(())
    }

    /// Gives `actor` its component clock and registers it in the global component, actor, and
    /// wrapper registries.
    fn register_python_data_actor(
        &mut self,
        actor: &Py<PyAny>,
        component_id: ComponentId,
    ) -> anyhow::Result<()> {
        let clock = self.create_component_clock(component_id);
        let trader_id = self.trader_id;
        let cache = self.cache.clone();

        Python::attach(|py| -> anyhow::Result<()> {
            let py_actor = actor.bind(py);
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
        })?;

        Python::attach(|py| -> anyhow::Result<()> {
            let py_actor = actor.bind(py);
            let py_data_actor_ref = py_actor
                .cast::<PyDataActor>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyDataActor: {e}"))?;
            py_data_actor_ref.borrow().register_in_global_registries()?;
            Ok(())
        })
    }

    /// Rejects a component ID this trader already tracks, whatever kind registered it.
    ///
    /// Duplicate adds are otherwise checked only within a kind, so an actor sharing an ID with a
    /// live strategy would overwrite that strategy's clock, registry entries, and wrapper, and a
    /// rollback would then remove state the attempt did not create. The lifecycle collections are
    /// checked alongside the clocks because a component registered externally and tracked through
    /// `add_*_id_for_lifecycle` has no trader-owned clock.
    fn ensure_component_id_available(&self, component_id: ComponentId) -> anyhow::Result<()> {
        let id = component_id.inner();
        let tracked = self.clocks.contains_key(&component_id)
            || self.actor_ids.iter().any(|actor_id| actor_id.inner() == id)
            || self
                .strategy_ids
                .iter()
                .any(|strategy_id| strategy_id.inner() == id)
            || self
                .exec_algorithm_ids
                .iter()
                .any(|exec_algorithm_id| exec_algorithm_id.inner() == id);

        if tracked {
            anyhow::bail!(
                "Component {component_id} is already registered with trader {}",
                self.trader_id
            );
        }

        Ok(())
    }
}

fn create_python_actor(config: &ImportableActorConfig) -> anyhow::Result<(Py<PyAny>, ActorId)> {
    let (module_name, class_name) = split_import_path(&config.actor_path, "actor_path")?;

    log::info!("Importing actor from module: {module_name} class: {class_name}");

    Python::attach(|py| -> anyhow::Result<(Py<PyAny>, ActorId)> {
        let actor_class = import_python_class(py, module_name, class_name)?;
        let config_instance = create_config_instance(py, &config.config_path, &config.config)?;

        let python_actor = if let Some(config_obj) = config_instance.as_ref() {
            actor_class.call1((config_obj,))?
        } else {
            actor_class.call0()?
        };

        let mut py_data_actor_ref = python_actor
            .extract::<PyRefMut<PyDataActor>>()
            .map_err(Into::<PyErr>::into)
            .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

        if let Some(config_obj) = config_instance.as_ref() {
            configure_py_data_actor(&mut py_data_actor_ref, config_obj)?;
        }

        py_data_actor_ref.set_python_instance(&python_actor)?;
        apply_class_derived_actor_id(&mut py_data_actor_ref, &python_actor)?;
        let actor_id = py_data_actor_ref.actor_id();

        Ok((python_actor.unbind(), actor_id))
    })
}

fn create_python_strategy(config: &ImportableStrategyConfig) -> anyhow::Result<Py<PyAny>> {
    let (module_name, class_name) = split_import_path(&config.strategy_path, "strategy_path")?;

    log::info!("Importing strategy from module: {module_name} class: {class_name}");

    Python::attach(|py| -> anyhow::Result<Py<PyAny>> {
        let strategy_class = import_python_class(py, module_name, class_name)?;
        let config_instance = create_config_instance(py, &config.config_path, &config.config)?;

        let python_strategy = if let Some(config_obj) = config_instance.as_ref() {
            strategy_class.call1((config_obj,))?
        } else {
            strategy_class.call0()?
        };

        Ok(python_strategy.unbind())
    })
}

fn split_import_path<'a>(path: &'a str, field: &str) -> anyhow::Result<(&'a str, &'a str)> {
    let Some((module_name, class_name)) = path.split_once(':') else {
        anyhow::bail!("{field} must be in format 'module.path:ClassName'");
    };

    if module_name.is_empty() || class_name.is_empty() || class_name.contains(':') {
        anyhow::bail!("{field} must be in format 'module.path:ClassName'");
    }

    Ok((module_name, class_name))
}

fn import_python_class<'py>(
    py: Python<'py>,
    module_name: &str,
    class_name: &str,
) -> anyhow::Result<Bound<'py, PyAny>> {
    let module = py
        .import(module_name)
        .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;

    module
        .getattr(class_name)
        .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))
}

fn create_config_instance<'py>(
    py: Python<'py>,
    config_path: &str,
    config: &HashMap<String, serde_json::Value>,
) -> anyhow::Result<Option<Bound<'py, PyAny>>> {
    if config_path.is_empty() && config.is_empty() {
        log::debug!("No config_path or empty config, using None");
        return Ok(None);
    }

    let Some((config_module_name, config_class_name)) = config_path.split_once(':') else {
        anyhow::bail!("config_path must be in format 'module.path:ClassName', was {config_path}");
    };

    if config_module_name.is_empty()
        || config_class_name.is_empty()
        || config_class_name.contains(':')
    {
        anyhow::bail!("config_path must be in format 'module.path:ClassName', was {config_path}");
    }

    log::debug!(
        "Importing config class from module: {config_module_name} class: {config_class_name}"
    );

    let config_module = py
        .import(config_module_name)
        .map_err(|e| anyhow::anyhow!("Failed to import config module {config_module_name}: {e}"))?;
    let config_class = config_module
        .getattr(config_class_name)
        .map_err(|e| anyhow::anyhow!("Failed to get config class {config_class_name}: {e}"))?;
    let py_dict = PyDict::new(py);

    for (key, value) in config {
        let py_value = config_value_to_py(py, key, value)?;
        py_dict.set_item(key, py_value)?;
    }

    let config_instance = match config_class.call((), Some(&py_dict)) {
        Ok(instance) => instance,
        Err(kwargs_err) => match config_class.call0() {
            Ok(instance) => {
                for (key, value) in config {
                    let py_value = config_value_to_py(py, key, value)?;

                    if let Err(setattr_err) = instance.setattr(key, py_value) {
                        log::warn!("Failed to set attribute {key}: {setattr_err}");
                    }
                }

                if instance.hasattr("__post_init__")? {
                    instance.call_method0("__post_init__")?;
                }

                instance
            }
            Err(default_err) => {
                anyhow::bail!(
                    "Failed to create config instance. Tried kwargs: {kwargs_err}, default: {default_err}"
                );
            }
        },
    };

    Ok(Some(config_instance))
}

fn config_value_to_py<'py>(
    py: Python<'py>,
    key: &str,
    value: &serde_json::Value,
) -> anyhow::Result<Bound<'py, PyAny>> {
    if key == "actor_id"
        && let Some(actor_id) = value.as_str()
    {
        return Ok(ActorId::new_checked(actor_id)?
            .into_pyobject(py)?
            .into_any());
    }

    let json_str = serde_json::to_string(value)
        .map_err(|e| anyhow::anyhow!("Failed to serialize config value: {e}"))?;

    Ok(PyModule::import(py, "json")?
        .call_method("loads", (json_str,), None)?
        .into_any())
}

fn configure_py_data_actor(
    actor: &mut PyRefMut<'_, PyDataActor>,
    config_obj: &Bound<'_, PyAny>,
) -> anyhow::Result<()> {
    if let Some(actor_id) = config_obj
        .getattr("actor_id")
        .ok()
        .filter(|value| !value.is_none())
    {
        let actor_id = if let Ok(actor_id) = actor_id.extract::<ActorId>() {
            actor_id
        } else if let Ok(actor_id_str) = actor_id.extract::<String>() {
            ActorId::new_checked(&actor_id_str)?
        } else {
            anyhow::bail!("Invalid `actor_id` type");
        };
        actor.set_actor_id(actor_id);
    }

    if let Some(log_events) = extract_bool_config_attr(config_obj, "log_events") {
        actor.set_log_events(log_events);
    }

    if let Some(log_commands) = extract_bool_config_attr(config_obj, "log_commands") {
        actor.set_log_commands(log_commands);
    }

    Ok(())
}

fn configure_py_strategy(
    strategy: &mut PyRefMut<'_, PyStrategy>,
    config_obj: &Bound<'_, PyAny>,
) -> anyhow::Result<()> {
    if let Some(strategy_id) = config_obj
        .getattr("strategy_id")
        .ok()
        .filter(|value| !value.is_none())
    {
        let strategy_id = if let Ok(strategy_id) = strategy_id.extract::<StrategyId>() {
            strategy_id
        } else if let Ok(strategy_id_str) = strategy_id.extract::<String>() {
            StrategyId::new_checked(&strategy_id_str)?
        } else {
            anyhow::bail!("Invalid `strategy_id` type");
        };
        strategy.set_strategy_id(strategy_id)?;
    }

    if let Some(order_id_tag) = config_obj
        .getattr("order_id_tag")
        .ok()
        .filter(|value| !value.is_none())
    {
        let order_id_tag = order_id_tag
            .extract::<String>()
            .map_err(|e| anyhow::anyhow!("Invalid `order_id_tag` type: {e}"))?;
        strategy.set_order_id_tag(&order_id_tag)?;
    }

    if let Some(log_events) = extract_bool_config_attr(config_obj, "log_events") {
        strategy.set_log_events(log_events);
    }

    if let Some(log_commands) = extract_bool_config_attr(config_obj, "log_commands") {
        strategy.set_log_commands(log_commands);
    }

    Ok(())
}

fn extract_bool_config_attr(config_obj: &Bound<'_, PyAny>, attr: &str) -> Option<bool> {
    config_obj
        .getattr(attr)
        .ok()
        .and_then(|value| value.extract::<bool>().ok())
}
