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

use nautilus_common::{actor::data_actor::ImportableActorConfig, python::cache::PyCache};
#[cfg(feature = "examples")]
use nautilus_core::python::to_pytype_err;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::identifiers::{AccountId, ActorId, Venue};
use nautilus_portfolio::python::PyPortfolio;
#[cfg(feature = "examples")]
use nautilus_trading::examples::strategies::{
    CompositeMarketMaker, CompositeMarketMakerConfig, DeltaNeutralVol, DeltaNeutralVolConfig,
    EmaCross, EmaCrossConfig, GridMarketMaker, GridMarketMakerConfig, HurstVpinDirectional,
    HurstVpinDirectionalConfig,
};
use nautilus_trading::{ImportableExecutionAlgorithmConfig, ImportableStrategyConfig};
use pyo3::{prelude::*, types::PyDict};

use super::engine::{
    PyBacktestEngine, engine_cache, engine_portfolio, generate_account_report,
    generate_fills_report, generate_order_fills_report, generate_orders_report,
    generate_positions_report,
};
use crate::{
    config::BacktestRunConfig, engine::BacktestEngine, node::BacktestNode, result::BacktestResult,
};

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

    /// Returns the run configurations.
    #[getter]
    #[pyo3(name = "configs")]
    fn py_configs(&self) -> Vec<BacktestRunConfig> {
        self.configs().to_vec()
    }

    /// Builds backtest engines from the run configurations.
    ///
    /// For each config, creates a `BacktestEngine`, adds venues, and loads
    /// instruments from the catalog. If building a config fails with
    /// `BacktestRunConfig.raise_exception` disabled, logs the error and skips that config;
    /// successful return does not guarantee an engine for every config.
    ///
    /// # Errors
    ///
    /// Returns an error if building an engine from a config fails and
    /// `BacktestRunConfig.raise_exception` is enabled for that config.
    #[pyo3(name = "build")]
    fn py_build(&mut self) -> PyResult<()> {
        self.build().map_err(to_pyruntime_err)
    }

    /// Runs all configured backtests and returns results.
    ///
    /// Automatically calls `build()` if engines have not been created yet.
    /// For each run config, loads data from the catalog and runs the engine.
    /// Supports both oneshot (`chunk_size = None`) and streaming modes.
    /// Configs without a built engine are skipped. If a run fails with
    /// `BacktestRunConfig.raise_exception` disabled, logs the error, clears its loaded data,
    /// leaves the engine undisposed, and omits its result.
    ///
    /// # Errors
    ///
    /// Returns an error if building, data loading, or engine execution fails and
    /// `BacktestRunConfig.raise_exception` is enabled for the run config.
    #[pyo3(name = "run")]
    fn py_run(&mut self) -> PyResult<Vec<BacktestResult>> {
        self.run().map_err(to_pyruntime_err)
    }

    /// Disposes all engines and releases resources.
    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) {
        self.dispose();
    }

    /// Returns the cache for the given run config engine.
    ///
    /// # Errors
    ///
    /// Returns an error if no engine exists for the run config ID.
    #[pyo3(name = "get_engine_cache")]
    fn py_get_engine_cache(&self, run_config_id: &str) -> PyResult<PyCache> {
        Ok(engine_cache(self.require_engine(run_config_id)?))
    }

    /// Returns the portfolio for the given run config engine.
    ///
    /// # Errors
    ///
    /// Returns an error if no engine exists for the run config ID.
    #[pyo3(name = "get_engine_portfolio")]
    fn py_get_engine_portfolio(&self, run_config_id: &str) -> PyResult<PyPortfolio> {
        Ok(engine_portfolio(self.require_engine(run_config_id)?))
    }

    /// Generates an orders report for the given run config engine.
    ///
    /// # Errors
    ///
    /// Returns an error if no engine exists or report generation fails.
    #[pyo3(name = "generate_orders_report")]
    fn py_generate_orders_report<'py>(
        &self,
        py: Python<'py>,
        run_config_id: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        generate_orders_report(self.require_engine(run_config_id)?, py)
    }

    /// Generates an order fills report for the given run config engine.
    ///
    /// # Errors
    ///
    /// Returns an error if no engine exists or report generation fails.
    #[pyo3(name = "generate_order_fills_report")]
    fn py_generate_order_fills_report<'py>(
        &self,
        py: Python<'py>,
        run_config_id: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        generate_order_fills_report(self.require_engine(run_config_id)?, py)
    }

    /// Generates a fills report for the given run config engine.
    ///
    /// # Errors
    ///
    /// Returns an error if no engine exists or report generation fails.
    #[pyo3(name = "generate_fills_report")]
    fn py_generate_fills_report<'py>(
        &self,
        py: Python<'py>,
        run_config_id: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        generate_fills_report(self.require_engine(run_config_id)?, py)
    }

    /// Generates a positions report for the given run config engine.
    ///
    /// # Errors
    ///
    /// Returns an error if no engine exists or report generation fails.
    #[pyo3(name = "generate_positions_report")]
    fn py_generate_positions_report<'py>(
        &self,
        py: Python<'py>,
        run_config_id: &str,
    ) -> PyResult<Bound<'py, PyAny>> {
        generate_positions_report(self.require_engine(run_config_id)?, py)
    }

    /// Generates an account report for the given run config engine.
    ///
    /// At least one of `venue` or `account_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if no engine exists, neither selector is provided, or report generation
    /// fails.
    #[pyo3(
        name = "generate_account_report",
        signature = (run_config_id, venue=None, account_id=None)
    )]
    fn py_generate_account_report<'py>(
        &self,
        py: Python<'py>,
        run_config_id: &str,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        generate_account_report(self.require_engine(run_config_id)?, py, venue, account_id)
    }

    /// Adds a constructed Python actor to the engine for the given run config.
    #[pyo3(name = "add_actor")]
    fn py_add_actor(&mut self, run_config_id: &str, actor: &Bound<'_, PyAny>) -> PyResult<()> {
        let engine = self.require_engine_mut(run_config_id)?;
        PyBacktestEngine::add_python_actor(engine, &actor.clone().unbind())
    }

    /// Adds an actor from an importable config to the engine for the given run config.
    #[pyo3(name = "add_actor_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_actor_from_config(
        &mut self,
        _py: Python,
        run_config_id: &str,
        config: ImportableActorConfig,
    ) -> PyResult<()> {
        log::debug!("`add_actor_from_config` with: {config:?}");
        let engine = self.require_engine_mut(run_config_id)?;
        let actor = create_importable_component(
            &config.actor_path,
            "actor_path",
            &config.config_path,
            &config.config,
            "actor",
        )?;
        PyBacktestEngine::add_python_actor(engine, &actor)
    }

    /// Adds a constructed Python strategy to the engine for the given run config.
    #[pyo3(name = "add_strategy")]
    fn py_add_strategy(
        &mut self,
        run_config_id: &str,
        strategy: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let engine = self.require_engine_mut(run_config_id)?;
        PyBacktestEngine::add_python_strategy(engine, &strategy.clone().unbind())
    }

    /// Adds a strategy from an importable config to the engine for the given run config.
    #[pyo3(name = "add_strategy_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_strategy_from_config(
        &mut self,
        _py: Python,
        run_config_id: &str,
        config: ImportableStrategyConfig,
    ) -> PyResult<()> {
        log::debug!("`add_strategy_from_config` with: {config:?}");
        let engine = self.require_engine_mut(run_config_id)?;
        let strategy = create_importable_component(
            &config.strategy_path,
            "strategy_path",
            &config.config_path,
            &config.config,
            "strategy",
        )?;
        PyBacktestEngine::add_python_strategy(engine, &strategy)
    }

    /// Adds a constructed Python execution algorithm to the engine for the given run config.
    #[pyo3(name = "add_exec_algorithm")]
    fn py_add_exec_algorithm(
        &mut self,
        run_config_id: &str,
        exec_algorithm: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let engine = self.require_engine_mut(run_config_id)?;
        PyBacktestEngine::add_python_exec_algorithm(engine, &exec_algorithm.clone().unbind())
    }

    /// Adds an execution algorithm from an importable config to the engine for the given run config.
    #[pyo3(name = "add_exec_algorithm_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_exec_algorithm_from_config(
        &mut self,
        _py: Python,
        run_config_id: &str,
        config: ImportableExecutionAlgorithmConfig,
    ) -> PyResult<()> {
        log::debug!("`add_exec_algorithm_from_config` with: {config:?}");
        let engine = self.require_engine_mut(run_config_id)?;
        PyBacktestEngine::ensure_can_add_exec_algorithm(engine)?;
        let exec_algorithm = create_importable_component(
            &config.exec_algorithm_path,
            "exec_algorithm_path",
            &config.config_path,
            &config.config,
            "exec algorithm",
        )?;
        PyBacktestEngine::add_python_exec_algorithm(engine, &exec_algorithm)
    }

    /// Adds a built-in example strategy to the engine for the given run config.
    ///
    /// This method exists only to single-source bundled example strategy code across
    /// Rust and Python tests/examples. It is not a first-class extension path for
    /// adding native strategies.
    #[pyo3(name = "add_builtin_strategy")]
    #[cfg_attr(
        not(feature = "examples"),
        expect(
            clippy::unused_self,
            reason = "PyO3 method keeps the instance API when examples are disabled"
        )
    )]
    fn py_add_builtin_strategy(
        &mut self,
        run_config_id: &str,
        type_name: &str,
        config: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        #[cfg(feature = "examples")]
        {
            let engine = self.get_engine_mut(run_config_id).ok_or_else(|| {
                to_pyruntime_err(format!("No engine for run config '{run_config_id}'"))
            })?;

            let register = builtin_strategy_register(type_name).ok_or_else(|| {
                to_pytype_err(format!("Unsupported built-in strategy type: {type_name}"))
            })?;
            register(engine, config)
        }

        #[cfg(not(feature = "examples"))]
        {
            let _ = (run_config_id, type_name, config);
            Err(to_pyruntime_err(
                "add_builtin_strategy requires the `examples` feature",
            ))
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

impl BacktestNode {
    fn require_engine(&self, run_config_id: &str) -> PyResult<&BacktestEngine> {
        self.get_engine(run_config_id)
            .ok_or_else(|| to_pyruntime_err(format!("No engine for run config '{run_config_id}'")))
    }

    fn require_engine_mut(&mut self, run_config_id: &str) -> PyResult<&mut BacktestEngine> {
        self.get_engine_mut(run_config_id)
            .ok_or_else(|| to_pyruntime_err(format!("No engine for run config '{run_config_id}'")))
    }
}

#[cfg(feature = "examples")]
type BuiltinStrategyRegister = for<'py> fn(&mut BacktestEngine, &Bound<'py, PyAny>) -> PyResult<()>;

#[cfg(feature = "examples")]
fn builtin_strategy_register(type_name: &str) -> Option<BuiltinStrategyRegister> {
    match type_name {
        "CompositeMarketMaker" => Some(register_composite_market_maker),
        "DeltaNeutralVol" => Some(register_delta_neutral_vol),
        "EmaCross" => Some(register_ema_cross),
        "GridMarketMaker" => Some(register_grid_market_maker),
        "HurstVpinDirectional" => Some(register_hurst_vpin_directional),
        _ => None,
    }
}

#[cfg(feature = "examples")]
fn register_composite_market_maker(
    engine: &mut BacktestEngine,
    config: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let config = config.extract::<CompositeMarketMakerConfig>()?;
    engine
        .add_strategy(CompositeMarketMaker::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_delta_neutral_vol(
    engine: &mut BacktestEngine,
    config: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let config = config.extract::<DeltaNeutralVolConfig>()?;
    engine
        .add_strategy(DeltaNeutralVol::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_ema_cross(engine: &mut BacktestEngine, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<EmaCrossConfig>()?;
    engine
        .add_strategy(EmaCross::from_config(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_grid_market_maker(
    engine: &mut BacktestEngine,
    config: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let config = config.extract::<GridMarketMakerConfig>()?;
    engine
        .add_strategy(GridMarketMaker::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(feature = "examples")]
fn register_hurst_vpin_directional(
    engine: &mut BacktestEngine,
    config: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let config = config.extract::<HurstVpinDirectionalConfig>()?;
    engine
        .add_strategy(HurstVpinDirectional::new(config))
        .map_err(to_pyruntime_err)
}

#[cfg(all(test, feature = "examples"))]
mod tests {
    use pyo3::{Python, types::PyDict};
    use rstest::rstest;

    use crate::{config::BacktestEngineConfig, engine::BacktestEngine};

    #[rstest]
    #[case("CompositeMarketMaker")]
    #[case("DeltaNeutralVol")]
    #[case("EmaCross")]
    #[case("GridMarketMaker")]
    #[case("HurstVpinDirectional")]
    fn test_builtin_strategy_register_accepts_supported_names(#[case] type_name: &str) {
        assert!(super::builtin_strategy_register(type_name).is_some());
    }

    #[rstest]
    fn test_builtin_strategy_register_rejects_unknown_name() {
        assert!(super::builtin_strategy_register("UnknownStrategy").is_none());
    }

    #[rstest]
    fn test_builtin_strategy_register_rejects_mismatched_config() {
        Python::initialize();

        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        Python::attach(|py| {
            let register = super::builtin_strategy_register("EmaCross").unwrap();
            let config = PyDict::new(py);
            let error = register(&mut engine, config.as_any()).unwrap_err();

            assert!(error.is_instance_of::<pyo3::exceptions::PyTypeError>(py));
        });
    }
}

pub(crate) fn create_importable_component(
    component_path: &str,
    path_field: &str,
    config_path: &str,
    config: &HashMap<String, serde_json::Value>,
    component_name: &str,
) -> PyResult<Py<PyAny>> {
    let Some((module_name, class_name)) = component_path.split_once(':') else {
        return Err(to_pyvalue_err(format!(
            "{path_field} must be in format 'module.path:ClassName'",
        )));
    };

    if module_name.is_empty() || class_name.is_empty() || class_name.contains(':') {
        return Err(to_pyvalue_err(format!(
            "{path_field} must be in format 'module.path:ClassName'",
        )));
    }

    log::info!("Importing {component_name} from module: {module_name} class: {class_name}");

    Python::attach(|py| -> anyhow::Result<Py<PyAny>> {
        let module = py
            .import(module_name)
            .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
        let class = module
            .getattr(class_name)
            .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;
        let config_instance = create_config_instance(py, config_path, config)?;
        let component = if let Some(config_obj) = config_instance {
            class.call1((config_obj,))?
        } else {
            class.call0()?
        };
        Ok(component.unbind())
    })
    .map_err(to_pyruntime_err)
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
        let py_value = config_value_to_py(py, key, value)?;
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
                        let py_value = config_value_to_py(py, key, value)?;

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
