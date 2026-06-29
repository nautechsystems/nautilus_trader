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

//! Python bindings for [`BacktestEngine`].

use std::collections::HashMap;

use ahash::AHashMap;
use nautilus_common::{
    actor::data_actor::ImportableActorConfig,
    enums::ComponentState,
    python::{
        actor::{PyDataActor, register_python_exec_algorithm_endpoint},
        cache::PyCache,
        config_error_to_pyvalue_err,
    },
};
use nautilus_core::{
    UUID4, UnixNanos,
    python::{to_pyruntime_err, to_pytype_err, to_pyvalue_err},
};
use nautilus_execution::{
    models::{
        fill::{
            BestPriceFillModel, CompetitionAwareFillModel, DefaultFillModel, FillModelAny,
            LimitOrderPartialFillModel, MarketHoursFillModel, OneTickSlippageFillModel,
            ProbabilisticFillModel, SizeAwareFillModel, ThreeTierFillModel, TwoTierFillModel,
            VolumeSensitiveFillModel,
        },
        latency::{LatencyModelAny, StaticLatencyModel},
    },
    python::fee::pyobject_to_fee_model_any,
};
#[cfg(feature = "defi")]
use nautilus_model::defi::DefiData;
use nautilus_model::{
    accounts::margin_model::{LeveragedMarginModel, MarginModelAny, StandardMarginModel},
    data::{
        Bar, BorrowRate, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentClose,
        InstrumentStatus, MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDeltas,
        OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{AccountType, BookType, OmsType, OtoTriggerMode},
    identifiers::{
        AccountId, ActorId, ClientId, ComponentId, ExecAlgorithmId, InstrumentId, StrategyId,
        TraderId, Venue,
    },
    python::instruments::pyobject_to_instrument_any,
    types::{Currency, Money, Price},
};
use nautilus_portfolio::python::PyPortfolio;
#[cfg(feature = "examples")]
use nautilus_trading::examples::{
    actors::{BookImbalanceActor, BookImbalanceActorConfig},
    strategies::{
        CompositeMarketMaker, CompositeMarketMakerConfig, DeltaNeutralVol, DeltaNeutralVolConfig,
        EmaCross, EmaCrossConfig, GridMarketMaker, GridMarketMakerConfig, HurstVpinDirectional,
        HurstVpinDirectionalConfig,
    },
};
use nautilus_trading::{
    ImportableExecAlgorithmConfig, ImportableStrategyConfig,
    algorithm::{TwapAlgorithm, TwapAlgorithmConfig},
    python::strategy::{PyStrategy, PyStrategyInner},
};
use pyo3::prelude::*;
use rust_decimal::Decimal;

use super::node::create_config_instance;
use crate::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
    modules::{FXRolloverInterestModule, SimulationModuleAny},
    result::BacktestResult,
};

/// PyO3 wrapper around [`BacktestEngine`].
///
/// Exposes the backtest engine to Python as `BacktestEngine`.
/// Uses `unsendable` because the inner engine holds `Rc<RefCell<...>>`.
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.backtest",
    name = "BacktestEngine",
    unsendable
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")]
#[derive(Debug)]
pub struct PyBacktestEngine(BacktestEngine);

// DeFi methods live in their own fully gated `#[pymethods]` block (multiple-pymethods is enabled)
// so the `gen_stub`/pyo3 expansion never references `DefiData` in non-DeFi builds.
#[cfg(feature = "defi")]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyBacktestEngine {
    /// Adds DeFi data to the engine.
    #[pyo3(name = "add_defi_data", signature = (data, client_id=None, sort=true))]
    fn py_add_defi_data(
        &mut self,
        data: Vec<DefiData>,
        client_id: Option<ClientId>,
        sort: bool,
    ) -> PyResult<()> {
        self.0
            .add_defi_data(data, client_id, sort)
            .map_err(to_pyruntime_err)
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyBacktestEngine {
    #[new]
    fn py_new(config: BacktestEngineConfig) -> PyResult<Self> {
        let engine = BacktestEngine::new(config).map_err(to_pyruntime_err)?;
        Ok(Self(engine))
    }

    /// Adds a simulated exchange with the given parameters to the engine.
    ///
    /// # Liquidation parameters
    ///
    /// - `liquidation_enabled` (bool, default `False`): if margin liquidation should be
    ///   triggered when the account's equity falls to or below the maintenance
    ///   margin threshold scaled by `liquidation_trigger_ratio`.
    /// - `liquidation_trigger_ratio` (float, optional, default `1.0`): the ratio of
    ///   maintenance margin used as the liquidation threshold. A value of `1.0`
    ///   liquidates when equity <= maintenance margin; higher values trigger earlier.
    /// - `liquidation_cancel_open_orders` (bool, default `True`): if open resting
    ///   orders for the venue should be cancelled before synthetic close-out fills
    ///   are emitted for open positions.
    #[pyo3(
        name = "add_venue",
        signature = (
            venue,
            oms_type,
            account_type,
            starting_balances,
            base_currency = None,
            default_leverage = None,
            leverages = None,
            margin_model = None,
            fill_model = None,
            fee_model = None,
            latency_model = None,
            modules = None,
            book_type = BookType::L1_MBP,
            routing = false,
            reject_stop_orders = true,
            support_gtd_orders = true,
            support_contingent_orders = true,
            use_position_ids = true,
            use_random_ids = false,
            use_reduce_only = true,
            use_message_queue = true,
            use_market_order_acks = false,
            bar_execution = true,
            bar_adaptive_high_low_ordering = false,
            trade_execution = true,
            liquidity_consumption = false,
            queue_position = false,
            allow_cash_borrowing = false,
            frozen_account = false,
            oto_trigger_mode = OtoTriggerMode::Partial,
            price_protection_points = None,
            settlement_prices = None,
            liquidation_enabled = false,
            liquidation_trigger_ratio = None,
            liquidation_cancel_open_orders = true,
        )
    )]
    #[expect(
        clippy::fn_params_excessive_bools,
        clippy::too_many_arguments,
        reason = "method mirrors the existing Python keyword API"
    )]
    fn py_add_venue(
        &mut self,
        venue: Venue,
        oms_type: OmsType,
        account_type: AccountType,
        starting_balances: Vec<Money>,
        base_currency: Option<Currency>,
        default_leverage: Option<Decimal>,
        leverages: Option<HashMap<InstrumentId, Decimal>>,
        margin_model: Option<Py<PyAny>>,
        fill_model: Option<Py<PyAny>>,
        fee_model: Option<Py<PyAny>>,
        latency_model: Option<Py<PyAny>>,
        modules: Option<Vec<Py<PyAny>>>,
        book_type: BookType,
        routing: bool,
        reject_stop_orders: bool,
        support_gtd_orders: bool,
        support_contingent_orders: bool,
        use_position_ids: bool,
        use_random_ids: bool,
        use_reduce_only: bool,
        use_message_queue: bool,
        use_market_order_acks: bool,
        bar_execution: bool,
        bar_adaptive_high_low_ordering: bool,
        trade_execution: bool,
        liquidity_consumption: bool,
        queue_position: bool,
        allow_cash_borrowing: bool,
        frozen_account: bool,
        oto_trigger_mode: OtoTriggerMode,
        price_protection_points: Option<u32>,
        settlement_prices: Option<HashMap<InstrumentId, Price>>,
        liquidation_enabled: bool,
        liquidation_trigger_ratio: Option<f64>,
        liquidation_cancel_open_orders: bool,
    ) -> PyResult<()> {
        let leverages: AHashMap<InstrumentId, Decimal> = leverages
            .map(|m| m.into_iter().collect())
            .unwrap_or_default();
        let settlement_prices: AHashMap<InstrumentId, Price> = settlement_prices
            .map(|m| m.into_iter().collect())
            .unwrap_or_default();
        let margin_model = margin_model
            .map(|obj| Python::attach(|py| pyobject_to_margin_model_any(py, obj.bind(py))))
            .transpose()?;
        let fill_model = fill_model
            .map(|obj| Python::attach(|py| pyobject_to_fill_model_any(py, obj.bind(py))))
            .transpose()?
            .unwrap_or_default();
        let fee_model = fee_model
            .map(|obj| Python::attach(|py| pyobject_to_fee_model_any(obj.bind(py))))
            .transpose()?
            .unwrap_or_default();
        let latency_model = latency_model
            .map(|obj| Python::attach(|py| pyobject_to_latency_model_any(py, obj.bind(py))))
            .transpose()?
            .map(Into::into);
        let modules = modules
            .map(|objs| {
                objs.into_iter()
                    .map(|obj| {
                        Python::attach(|py| pyobject_to_simulation_module_any(py, obj.bind(py)))
                    })
                    .collect::<PyResult<Vec<_>>>()
            })
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(Into::into)
            .collect();

        let sim_config = SimulatedVenueConfig::builder()
            .venue(venue)
            .oms_type(oms_type)
            .account_type(account_type)
            .book_type(book_type)
            .starting_balances(starting_balances)
            .maybe_base_currency(base_currency)
            .maybe_default_leverage(default_leverage)
            .leverages(leverages)
            .maybe_margin_model(margin_model)
            .modules(modules)
            .fill_model(fill_model)
            .fee_model(fee_model)
            .maybe_latency_model(latency_model)
            .routing(routing)
            .reject_stop_orders(reject_stop_orders)
            .support_gtd_orders(support_gtd_orders)
            .support_contingent_orders(support_contingent_orders)
            .use_position_ids(use_position_ids)
            .use_random_ids(use_random_ids)
            .use_reduce_only(use_reduce_only)
            .use_message_queue(use_message_queue)
            .use_market_order_acks(use_market_order_acks)
            .bar_execution(bar_execution)
            .bar_adaptive_high_low_ordering(bar_adaptive_high_low_ordering)
            .trade_execution(trade_execution)
            .liquidity_consumption(liquidity_consumption)
            .allow_cash_borrowing(allow_cash_borrowing)
            .frozen_account(frozen_account)
            .queue_position(queue_position)
            .oto_full_trigger(oto_trigger_mode == OtoTriggerMode::Full)
            .maybe_price_protection_points(price_protection_points)
            .liquidation_enabled(liquidation_enabled)
            .liquidation_trigger_ratio(liquidation_trigger_ratio.unwrap_or(1.0))
            .liquidation_cancel_open_orders(liquidation_cancel_open_orders)
            .build()
            .map_err(config_error_to_pyvalue_err)?;

        self.0.add_venue(sim_config).map_err(to_pyruntime_err)?;

        for (instrument_id, price) in settlement_prices {
            self.0
                .set_settlement_price(venue, instrument_id, price)
                .map_err(to_pyruntime_err)?;
        }

        Ok(())
    }

    /// Changes the fill model for a venue.
    #[pyo3(name = "change_fill_model")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_change_fill_model(
        &mut self,
        py: Python,
        venue: Venue,
        fill_model: Py<PyAny>,
    ) -> PyResult<()> {
        let fill_model = pyobject_to_fill_model_any(py, fill_model.bind(py))?;
        self.0.change_fill_model(venue, fill_model);
        Ok(())
    }

    /// Adds data to the engine.
    #[pyo3(
        name = "add_data",
        signature = (data, client_id=None, validate=true, sort=true)
    )]
    fn py_add_data(
        &mut self,
        py: Python,
        data: Vec<Py<PyAny>>,
        client_id: Option<ClientId>,
        validate: bool,
        sort: bool,
    ) -> PyResult<()> {
        let rust_data: Vec<Data> = data
            .into_iter()
            .map(|obj| pyobject_to_data(py, obj.bind(py)))
            .collect::<PyResult<_>>()?;
        self.0
            .add_data(rust_data, client_id, validate, sort)
            .map_err(to_pyruntime_err)
    }

    /// Adds an instrument to the engine.
    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&mut self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let instrument_any = pyobject_to_instrument_any(py, instrument)?;
        self.0
            .add_instrument(&instrument_any)
            .map_err(to_pyruntime_err)
    }

    /// Adds an actor from a constructed Python instance.
    ///
    /// The actor ID and logging flags are sourced from the instance's config.
    #[pyo3(name = "add_actor")]
    fn py_add_actor(&mut self, actor: &Bound<'_, PyAny>) -> PyResult<()> {
        log::debug!("`add_actor` with a constructed instance");
        self.add_python_actor(&actor.clone().unbind())
    }

    /// Adds an actor from an importable config.
    #[pyo3(name = "add_actor_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_actor_from_config(
        &mut self,
        _py: Python,
        config: ImportableActorConfig,
    ) -> PyResult<()> {
        log::debug!("`add_actor_from_config` with: {config:?}");

        let parts: Vec<&str> = config.actor_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "actor_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing actor from module: {module_name} class: {class_name}");

        let actor = Python::attach(|py| -> anyhow::Result<Py<PyAny>> {
            let actor_module = py
                .import(module_name)
                .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
            let actor_class = actor_module
                .getattr(class_name)
                .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

            let config_instance = create_config_instance(py, &config.config_path, &config.config)?;

            let python_actor = if let Some(config_obj) = config_instance {
                actor_class.call1((config_obj,))?
            } else {
                actor_class.call0()?
            };

            Ok(python_actor.unbind())
        })
        .map_err(to_pyruntime_err)?;

        self.add_python_actor(&actor)
    }

    /// Adds a strategy from a constructed Python instance.
    ///
    /// The strategy ID, order ID tag, and logging flags are sourced from the instance's
    /// config.
    #[pyo3(name = "add_strategy")]
    fn py_add_strategy(&mut self, strategy: &Bound<'_, PyAny>) -> PyResult<()> {
        log::debug!("`add_strategy` with a constructed instance");
        self.add_python_strategy(&strategy.clone().unbind())
    }

    /// Adds a strategy from an importable config.
    #[pyo3(name = "add_strategy_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_strategy_from_config(
        &mut self,
        _py: Python,
        config: ImportableStrategyConfig,
    ) -> PyResult<()> {
        log::debug!("`add_strategy_from_config` with: {config:?}");

        let parts: Vec<&str> = config.strategy_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "strategy_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing strategy from module: {module_name} class: {class_name}");

        let strategy = Python::attach(|py| -> anyhow::Result<Py<PyAny>> {
            let strategy_module = py
                .import(module_name)
                .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
            let strategy_class = strategy_module
                .getattr(class_name)
                .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

            let config_instance = create_config_instance(py, &config.config_path, &config.config)?;

            let python_strategy = if let Some(config_obj) = config_instance {
                strategy_class.call1((config_obj,))?
            } else {
                strategy_class.call0()?
            };

            Ok(python_strategy.unbind())
        })
        .map_err(to_pyruntime_err)?;

        self.add_python_strategy(&strategy)
    }

    /// Adds an execution algorithm from a constructed Python instance.
    ///
    /// The execution algorithm ID and logging flags are sourced from the instance's
    /// config.
    #[pyo3(name = "add_exec_algorithm")]
    fn py_add_exec_algorithm(&mut self, exec_algorithm: &Bound<'_, PyAny>) -> PyResult<()> {
        log::debug!("`add_exec_algorithm` with a constructed instance");
        self.add_python_exec_algorithm(&exec_algorithm.clone().unbind())
    }

    /// Adds an execution algorithm from an importable config.
    #[pyo3(name = "add_exec_algorithm_from_config")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_add_exec_algorithm_from_config(
        &mut self,
        _py: Python,
        config: ImportableExecAlgorithmConfig,
    ) -> PyResult<()> {
        self.ensure_can_add_exec_algorithm()?;

        log::debug!("`add_exec_algorithm_from_config` with: {config:?}");

        let parts: Vec<&str> = config.exec_algorithm_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "exec_algorithm_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing exec algorithm from module: {module_name} class: {class_name}");

        let exec_algorithm = Python::attach(|py| -> anyhow::Result<Py<PyAny>> {
            let algo_module = py
                .import(module_name)
                .map_err(|e| anyhow::anyhow!("Failed to import module {module_name}: {e}"))?;
            let algo_class = algo_module
                .getattr(class_name)
                .map_err(|e| anyhow::anyhow!("Failed to get class {class_name}: {e}"))?;

            let config_instance = create_config_instance(py, &config.config_path, &config.config)?;

            let python_exec_algorithm = if let Some(config_obj) = config_instance {
                algo_class.call1((config_obj,))?
            } else {
                algo_class.call0()?
            };

            Ok(python_exec_algorithm.unbind())
        })
        .map_err(to_pyruntime_err)?;

        self.add_python_exec_algorithm(&exec_algorithm)
    }

    /// Adds a built-in example actor from its type name and config.
    ///
    /// This method exists only to single-source bundled example actor code across
    /// Rust and Python tests/examples. It is not a first-class extension path for
    /// adding native actors.
    #[cfg(feature = "examples")]
    #[pyo3(name = "add_builtin_actor")]
    fn py_add_builtin_actor(&mut self, type_name: &str, config: &Bound<'_, PyAny>) -> PyResult<()> {
        let register = builtin_actor_register(type_name).ok_or_else(|| {
            to_pytype_err(format!("Unsupported built-in actor type: {type_name}"))
        })?;
        register(&mut self.0, config)
    }

    /// Adds a built-in example strategy from its type name and config.
    ///
    /// This method exists only to single-source bundled example strategy code across
    /// Rust and Python tests/examples. It is not a first-class extension path for
    /// adding native strategies.
    #[cfg(feature = "examples")]
    #[pyo3(name = "add_builtin_strategy")]
    fn py_add_builtin_strategy(
        &mut self,
        type_name: &str,
        config: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let register = builtin_strategy_register(type_name).ok_or_else(|| {
            to_pytype_err(format!("Unsupported built-in strategy type: {type_name}"))
        })?;
        register(&mut self.0, config)
    }

    /// Adds a compiled-in native Rust execution algorithm from its type name and config.
    ///
    /// The type name determines which built-in execution algorithm is constructed.
    /// All execution happens in Rust; Python is the configuration layer.
    #[pyo3(name = "add_native_exec_algorithm")]
    fn py_add_native_exec_algorithm(
        &mut self,
        type_name: &str,
        config: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let register = native_exec_algorithm_register(type_name).ok_or_else(|| {
            to_pytype_err(format!(
                "Unsupported native exec algorithm type: {type_name}"
            ))
        })?;
        register(&mut self.0, config)
    }

    /// Runs the backtest engine.
    #[pyo3(
        name = "run",
        signature = (start=None, end=None, run_config_id=None, streaming=false)
    )]
    fn py_run(
        &mut self,
        start: Option<u64>,
        end: Option<u64>,
        run_config_id: Option<String>,
        streaming: bool,
    ) -> PyResult<()> {
        self.0
            .run(
                start.map(UnixNanos::from),
                end.map(UnixNanos::from),
                run_config_id,
                streaming,
            )
            .map_err(to_pyruntime_err)
    }

    /// Ends the backtest run, finalizing results.
    #[pyo3(name = "end")]
    fn py_end(&mut self) {
        self.0.end();
    }

    /// Resets the engine state for a new run.
    #[pyo3(name = "reset")]
    fn py_reset(&mut self) {
        self.0.reset();
    }

    /// Disposes of the engine, releasing all resources.
    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) {
        self.0.dispose();
    }

    /// Returns the backtest result from the last run.
    #[pyo3(name = "get_result")]
    fn py_get_result(&self) -> BacktestResult {
        self.0.get_result()
    }

    /// Clears all data from the engine.
    #[pyo3(name = "clear_data")]
    fn py_clear_data(&mut self) {
        self.0.clear_data();
    }

    /// Clears all actors from the engine.
    #[pyo3(name = "clear_actors")]
    fn py_clear_actors(&mut self) -> PyResult<()> {
        self.0.clear_actors().map_err(to_pyruntime_err)
    }

    /// Clears all strategies from the engine.
    #[pyo3(name = "clear_strategies")]
    fn py_clear_strategies(&mut self) -> PyResult<()> {
        self.0.clear_strategies().map_err(to_pyruntime_err)
    }

    /// Clears all execution algorithms from the engine.
    #[pyo3(name = "clear_exec_algorithms")]
    fn py_clear_exec_algorithms(&mut self) -> PyResult<()> {
        self.0.clear_exec_algorithms().map_err(to_pyruntime_err)
    }

    /// Adds multiple actors from importable configs. Stops at the first error.
    #[pyo3(name = "add_actors_from_configs")]
    fn py_add_actors_from_configs(
        &mut self,
        py: Python,
        configs: Vec<ImportableActorConfig>,
    ) -> PyResult<()> {
        for config in configs {
            self.py_add_actor_from_config(py, config)?;
        }
        Ok(())
    }

    /// Adds multiple strategies from importable configs. Stops at the first error.
    #[pyo3(name = "add_strategies_from_configs")]
    fn py_add_strategies_from_configs(
        &mut self,
        py: Python,
        configs: Vec<ImportableStrategyConfig>,
    ) -> PyResult<()> {
        for config in configs {
            self.py_add_strategy_from_config(py, config)?;
        }
        Ok(())
    }

    /// Adds multiple execution algorithms from importable configs. Stops at the first error.
    #[pyo3(name = "add_exec_algorithms_from_configs")]
    fn py_add_exec_algorithms_from_configs(
        &mut self,
        py: Python,
        configs: Vec<ImportableExecAlgorithmConfig>,
    ) -> PyResult<()> {
        for config in configs {
            self.py_add_exec_algorithm_from_config(py, config)?;
        }
        Ok(())
    }

    /// Adds multiple actors from constructed Python instances. Stops at the first error.
    #[pyo3(name = "add_actors")]
    fn py_add_actors(&mut self, actors: Vec<Py<PyAny>>) -> PyResult<()> {
        for actor in actors {
            self.add_python_actor(&actor)?;
        }
        Ok(())
    }

    /// Adds multiple strategies from constructed Python instances. Stops at the first error.
    #[pyo3(name = "add_strategies")]
    fn py_add_strategies(&mut self, strategies: Vec<Py<PyAny>>) -> PyResult<()> {
        for strategy in strategies {
            self.add_python_strategy(&strategy)?;
        }
        Ok(())
    }

    /// Adds multiple execution algorithms from constructed Python instances. Stops at the first error.
    #[pyo3(name = "add_exec_algorithms")]
    fn py_add_exec_algorithms(&mut self, exec_algorithms: Vec<Py<PyAny>>) -> PyResult<()> {
        for exec_algorithm in exec_algorithms {
            self.add_python_exec_algorithm(&exec_algorithm)?;
        }
        Ok(())
    }

    /// Sorts the engine's internal data stream by timestamp.
    #[pyo3(name = "sort_data")]
    fn py_sort_data(&mut self) {
        self.0.sort_data();
    }

    /// Returns the trader ID for this engine.
    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> TraderId {
        self.0.trader_id()
    }

    /// Returns the machine ID for this engine.
    #[getter]
    #[pyo3(name = "machine_id")]
    fn py_machine_id(&self) -> String {
        self.0.machine_id().to_string()
    }

    /// Returns the unique instance ID for this engine.
    #[getter]
    #[pyo3(name = "instance_id")]
    fn py_instance_id(&self) -> UUID4 {
        self.0.instance_id()
    }

    /// Returns the current iteration count.
    #[getter]
    #[pyo3(name = "iteration")]
    fn py_iteration(&self) -> usize {
        self.0.iteration()
    }

    /// Returns the last run config ID, if any.
    #[getter]
    #[pyo3(name = "run_config_id")]
    fn py_run_config_id(&self) -> Option<String> {
        self.0.run_config_id().map(str::to_string)
    }

    /// Returns the last run ID, if any.
    #[getter]
    #[pyo3(name = "run_id")]
    fn py_run_id(&self) -> Option<UUID4> {
        self.0.run_id()
    }

    /// Returns when the last run started, in nanoseconds since the UNIX epoch.
    #[getter]
    #[pyo3(name = "run_started")]
    fn py_run_started(&self) -> Option<u64> {
        self.0.run_started().map(|n| n.as_u64())
    }

    /// Returns when the last run finished, in nanoseconds since the UNIX epoch.
    #[getter]
    #[pyo3(name = "run_finished")]
    fn py_run_finished(&self) -> Option<u64> {
        self.0.run_finished().map(|n| n.as_u64())
    }

    /// Returns the last backtest range start, in nanoseconds since the UNIX epoch.
    #[getter]
    #[pyo3(name = "backtest_start")]
    fn py_backtest_start(&self) -> Option<u64> {
        self.0.backtest_start().map(|n| n.as_u64())
    }

    /// Returns the last backtest range end, in nanoseconds since the UNIX epoch.
    #[getter]
    #[pyo3(name = "backtest_end")]
    fn py_backtest_end(&self) -> Option<u64> {
        self.0.backtest_end().map(|n| n.as_u64())
    }

    /// Returns the list of registered venue identifiers.
    #[pyo3(name = "list_venues")]
    fn py_list_venues(&self) -> Vec<Venue> {
        self.0.list_venues()
    }

    /// Returns the cache shared with the kernel and registered components.
    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> PyCache {
        PyCache::from_rc(self.0.kernel().cache.clone())
    }

    /// Returns the portfolio shared with the kernel and registered components.
    #[getter]
    #[pyo3(name = "portfolio")]
    fn py_portfolio(&self) -> PyPortfolio {
        PyPortfolio::from_rc(self.0.kernel().portfolio.clone())
    }

    /// Generates an orders report as a pandas `DataFrame`.
    ///
    /// # Errors
    ///
    /// Returns an error if the Python `ReportProvider` import or call fails.
    #[pyo3(name = "generate_orders_report")]
    fn py_generate_orders_report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let orders = self.cache_bound(py)?.call_method0("orders")?;
        Self::report_provider(py)?.call_method1("generate_orders_report", (orders,))
    }

    /// Generates an order fills report as a pandas `DataFrame`.
    ///
    /// # Errors
    ///
    /// Returns an error if the Python `ReportProvider` import or call fails.
    #[pyo3(name = "generate_order_fills_report")]
    fn py_generate_order_fills_report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let orders = self.cache_bound(py)?.call_method0("orders")?;
        Self::report_provider(py)?.call_method1("generate_order_fills_report", (orders,))
    }

    /// Generates a fills report as a pandas `DataFrame`.
    ///
    /// # Errors
    ///
    /// Returns an error if the Python `ReportProvider` import or call fails.
    #[pyo3(name = "generate_fills_report")]
    fn py_generate_fills_report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let orders = self.cache_bound(py)?.call_method0("orders")?;
        Self::report_provider(py)?.call_method1("generate_fills_report", (orders,))
    }

    /// Generates a positions report as a pandas `DataFrame`.
    ///
    /// # Errors
    ///
    /// Returns an error if the Python `ReportProvider` import or call fails.
    #[pyo3(name = "generate_positions_report")]
    fn py_generate_positions_report<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let cache = self.cache_bound(py)?;
        let positions = cache.call_method0("positions")?;
        let snapshots = cache.call_method0("position_snapshots")?;
        Self::report_provider(py)?.call_method1("generate_positions_report", (positions, snapshots))
    }

    /// Generates an account report as a pandas `DataFrame`.
    ///
    /// At least one of `venue` or `account_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if neither `venue` nor `account_id` is provided, or if the Python
    /// `ReportProvider` import or call fails.
    #[pyo3(name = "generate_account_report", signature = (venue=None, account_id=None))]
    fn py_generate_account_report<'py>(
        &self,
        py: Python<'py>,
        venue: Option<Venue>,
        account_id: Option<AccountId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let cache = self.cache_bound(py)?;
        let account = match (account_id, venue) {
            (Some(aid), _) => cache.call_method1("account", (aid,))?,
            (None, Some(v)) => cache.call_method1("account_for_venue", (v,))?,
            (None, None) => {
                return Err(to_pyvalue_err(
                    "At least one of 'venue' or 'account_id' must be provided",
                ));
            }
        };

        if account.is_none() {
            return py.import("pandas")?.call_method0("DataFrame");
        }
        Self::report_provider(py)?.call_method1("generate_account_report", (account,))
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.0)
    }
}

impl PyBacktestEngine {
    fn cache_bound<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyCache>> {
        Ok(Py::new(py, self.py_cache())?.into_bound(py))
    }

    fn report_provider(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
        py.import("nautilus_trader.analysis.reporter")?
            .getattr("ReportProvider")
    }

    /// Provides access to the inner [`BacktestEngine`].
    #[must_use]
    pub fn inner(&self) -> &BacktestEngine {
        &self.0
    }

    /// Provides mutable access to the inner [`BacktestEngine`].
    pub fn inner_mut(&mut self) -> &mut BacktestEngine {
        &mut self.0
    }

    /// Configures and registers a constructed Python strategy instance.
    ///
    /// Shared by `add_strategy` (caller-constructed instance) and
    /// `add_strategy_from_config` (imported and constructed here). The strategy ID,
    /// order ID tag, and logging flags are sourced from the instance's retained
    /// `.config`, so both entry points use a single config object.
    #[allow(
        unsafe_code,
        reason = "Required for Python strategy component registration"
    )]
    fn add_python_strategy(&mut self, strategy: &Py<PyAny>) -> PyResult<()> {
        let strategy_id = Python::attach(|py| -> anyhow::Result<StrategyId> {
            let bound = strategy.bind(py);

            let config_instance = bound
                .getattr("config")
                .ok()
                .filter(|config| !config.is_none());

            let mut py_strategy_ref = bound
                .extract::<PyRefMut<PyStrategy>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

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
                    py_strategy_ref.set_strategy_id(strategy_id_val)?;
                }

                if let Ok(order_id_tag) = config_obj.getattr("order_id_tag")
                    && !order_id_tag.is_none()
                {
                    let order_id_tag_val = order_id_tag
                        .extract::<String>()
                        .map_err(|e| anyhow::anyhow!("Invalid `order_id_tag` type: {e}"))?;
                    py_strategy_ref.set_order_id_tag(&order_id_tag_val)?;
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

            py_strategy_ref.set_python_instance(strategy.clone_ref(py));
            let strategy_id = py_strategy_ref.strategy_id();

            Ok(strategy_id)
        })
        .map_err(to_pyruntime_err)?;

        if self
            .0
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

        let trader_id = self.0.kernel().config.trader_id();
        let cache = self.0.kernel().cache.clone();
        let portfolio = self.0.kernel().portfolio.clone();
        let component_id = ComponentId::new(strategy_id.inner().as_str());
        let clock = self
            .0
            .kernel_mut()
            .trader
            .borrow_mut()
            .create_component_clock(component_id);

        Python::attach(|py| -> anyhow::Result<()> {
            let py_strategy = strategy.bind(py);
            let mut py_strategy_ref = py_strategy
                .extract::<PyRefMut<PyStrategy>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyStrategy: {e}"))?;

            py_strategy_ref
                .register(trader_id, clock, cache, portfolio)
                .map_err(|e| anyhow::anyhow!("Failed to register PyStrategy: {e}"))?;

            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        Python::attach(|py| -> anyhow::Result<()> {
            let py_strategy = strategy.bind(py);
            let py_strategy_ref = py_strategy
                .cast::<PyStrategy>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyStrategy: {e}"))?;
            py_strategy_ref.borrow().register_in_global_registries();
            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        self.0
            .kernel_mut()
            .trader
            .borrow_mut()
            .add_strategy_id_with_subscriptions::<PyStrategyInner>(strategy_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python strategy {strategy_id}");
        Ok(())
    }

    /// Configures and registers a constructed Python actor instance.
    ///
    /// Shared by `add_actor` (caller-constructed instance) and `add_actor_from_config`
    /// (imported and constructed here). The actor ID and logging flags are sourced from
    /// the instance's retained `.config`, so both entry points use a single config object.
    #[allow(
        unsafe_code,
        reason = "Required for Python actor component registration"
    )]
    fn add_python_actor(&mut self, actor: &Py<PyAny>) -> PyResult<()> {
        let actor_id = Python::attach(|py| -> anyhow::Result<ActorId> {
            let bound = actor.bind(py);

            let config_instance = bound
                .getattr("config")
                .ok()
                .filter(|config| !config.is_none());

            let mut py_data_actor_ref = bound
                .extract::<PyRefMut<PyDataActor>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

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

            py_data_actor_ref.set_python_instance(actor.clone_ref(py));
            let actor_id = py_data_actor_ref.actor_id();

            Ok(actor_id)
        })
        .map_err(to_pyruntime_err)?;

        if self
            .0
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

        let trader_id = self.0.kernel().config.trader_id();
        let cache = self.0.kernel().cache.clone();
        let component_id = ComponentId::new(actor_id.inner().as_str());
        let clock = self
            .0
            .kernel_mut()
            .trader
            .borrow_mut()
            .create_component_clock(component_id);

        Python::attach(|py| -> anyhow::Result<()> {
            let py_actor = actor.bind(py);
            let mut py_data_actor_ref = py_actor
                .extract::<PyRefMut<PyDataActor>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

            py_data_actor_ref
                .register(trader_id, clock, cache)
                .map_err(|e| anyhow::anyhow!("Failed to register PyDataActor: {e}"))?;

            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        Python::attach(|py| -> anyhow::Result<()> {
            let py_actor = actor.bind(py);
            let py_data_actor_ref = py_actor
                .cast::<PyDataActor>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyDataActor: {e}"))?;
            py_data_actor_ref.borrow().register_in_global_registries();
            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        self.0
            .kernel_mut()
            .trader
            .borrow_mut()
            .add_actor_id_for_lifecycle(actor_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python actor {actor_id}");
        Ok(())
    }

    /// Configures and registers a constructed Python execution algorithm instance.
    ///
    /// Shared by `add_exec_algorithm` (caller-constructed instance) and
    /// `add_exec_algorithm_from_config` (imported and constructed here). The execution
    /// algorithm ID and logging flags are sourced from the instance's retained `.config`.
    #[allow(
        unsafe_code,
        reason = "Required for Python exec algorithm component registration"
    )]
    fn add_python_exec_algorithm(&mut self, exec_algorithm: &Py<PyAny>) -> PyResult<()> {
        self.ensure_can_add_exec_algorithm()?;

        let actor_id = Python::attach(|py| -> anyhow::Result<ActorId> {
            let bound = exec_algorithm.bind(py);

            let config_instance = bound
                .getattr("config")
                .ok()
                .filter(|config| !config.is_none());

            let mut py_data_actor_ref = bound
                .extract::<PyRefMut<PyDataActor>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

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

            py_data_actor_ref.set_python_instance(exec_algorithm.clone_ref(py));
            let actor_id = py_data_actor_ref.actor_id();

            Ok(actor_id)
        })
        .map_err(to_pyruntime_err)?;

        let exec_algorithm_id = ExecAlgorithmId::from(actor_id.inner().as_str());

        if self
            .0
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

        let trader_id = self.0.kernel().config.trader_id();
        let cache = self.0.kernel().cache.clone();
        let component_id = ComponentId::new(actor_id.inner().as_str());
        let clock = self
            .0
            .kernel_mut()
            .trader
            .borrow_mut()
            .create_component_clock(component_id);

        Python::attach(|py| -> anyhow::Result<()> {
            let py_algo = exec_algorithm.bind(py);
            let mut py_data_actor_ref = py_algo
                .extract::<PyRefMut<PyDataActor>>()
                .map_err(Into::<PyErr>::into)
                .map_err(|e| anyhow::anyhow!("Failed to extract PyDataActor: {e}"))?;

            py_data_actor_ref
                .register(trader_id, clock, cache)
                .map_err(|e| anyhow::anyhow!("Failed to register PyDataActor: {e}"))?;

            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        Python::attach(|py| -> anyhow::Result<()> {
            let py_algo = exec_algorithm.bind(py);
            let py_data_actor_ref = py_algo
                .cast::<PyDataActor>()
                .map_err(|e| anyhow::anyhow!("Failed to downcast to PyDataActor: {e}"))?;
            py_data_actor_ref.borrow().register_in_global_registries();
            Ok(())
        })
        .map_err(to_pyruntime_err)?;

        register_python_exec_algorithm_endpoint(exec_algorithm_id);

        self.0
            .kernel_mut()
            .trader
            .borrow_mut()
            .add_exec_algorithm_id_for_lifecycle(exec_algorithm_id)
            .map_err(to_pyruntime_err)?;

        log::info!("Registered Python exec algorithm {exec_algorithm_id}");
        Ok(())
    }

    /// Rejects adding an execution algorithm when the trader is running or disposed.
    ///
    /// Checked before importing and constructing the Python class in the `from_config`
    /// path so a rejected addition never runs user constructor code.
    fn ensure_can_add_exec_algorithm(&self) -> PyResult<()> {
        match self.0.kernel().trader.borrow().state() {
            ComponentState::PreInitialized | ComponentState::Ready | ComponentState::Stopped => {
                Ok(())
            }
            ComponentState::Running => Err(to_pyruntime_err(
                "Cannot add execution algorithms to running trader",
            )),
            ComponentState::Disposed => {
                Err(to_pyruntime_err("Cannot add components to disposed trader"))
            }
            state => Err(to_pyruntime_err(format!(
                "Cannot add execution algorithms in current state: {state}"
            ))),
        }
    }
}

#[cfg(feature = "examples")]
type BuiltinActorRegister = for<'py> fn(&mut BacktestEngine, &Bound<'py, PyAny>) -> PyResult<()>;

#[cfg(feature = "examples")]
type BuiltinStrategyRegister = for<'py> fn(&mut BacktestEngine, &Bound<'py, PyAny>) -> PyResult<()>;

#[cfg(feature = "examples")]
fn builtin_actor_register(type_name: &str) -> Option<BuiltinActorRegister> {
    match type_name {
        "BookImbalanceActor" => Some(register_book_imbalance_actor),
        _ => None,
    }
}

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

type NativeExecAlgorithmRegister =
    for<'py> fn(&mut BacktestEngine, &Bound<'py, PyAny>) -> PyResult<()>;

fn native_exec_algorithm_register(type_name: &str) -> Option<NativeExecAlgorithmRegister> {
    match type_name {
        "TwapAlgorithm" => Some(register_twap_algorithm),
        _ => None,
    }
}

fn register_twap_algorithm(engine: &mut BacktestEngine, config: &Bound<'_, PyAny>) -> PyResult<()> {
    let config = config.extract::<TwapAlgorithmConfig>()?;
    if config.exec_algorithm_id.is_none() {
        return Err(to_pyvalue_err(
            "TwapAlgorithm config requires `exec_algorithm_id`",
        ));
    }
    engine
        .add_exec_algorithm(TwapAlgorithm::new(config))
        .map_err(to_pyruntime_err)
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

#[cfg(feature = "examples")]
fn register_book_imbalance_actor(
    engine: &mut BacktestEngine,
    config: &Bound<'_, PyAny>,
) -> PyResult<()> {
    let config = config.extract::<BookImbalanceActorConfig>()?;
    engine
        .add_actor(BookImbalanceActor::from_config(config))
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
    #[case("BookImbalanceActor")]
    fn test_builtin_actor_register_accepts_supported_names(#[case] type_name: &str) {
        assert!(super::builtin_actor_register(type_name).is_some());
    }

    #[rstest]
    fn test_builtin_register_rejects_unknown_names() {
        assert!(super::builtin_strategy_register("UnknownStrategy").is_none());
        assert!(super::builtin_actor_register("UnknownActor").is_none());
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

    #[rstest]
    fn test_builtin_actor_register_rejects_mismatched_config() {
        Python::initialize();

        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        Python::attach(|py| {
            let register = super::builtin_actor_register("BookImbalanceActor").unwrap();
            let config = PyDict::new(py);
            let error = register(&mut engine, config.as_any()).unwrap_err();

            assert!(error.is_instance_of::<pyo3::exceptions::PyTypeError>(py));
        });
    }

    #[rstest]
    fn test_add_strategy_registers_python_instance() {
        use nautilus_model::identifiers::StrategyId;
        use nautilus_trading::python::strategy::PyStrategy;
        use pyo3::{ffi::c_str, types::PyAnyMethods};

        Python::initialize();

        let mut engine =
            super::PyBacktestEngine(BacktestEngine::new(BacktestEngineConfig::default()).unwrap());

        Python::attach(|py| {
            let config = py
                .eval(
                    c_str!("type('_Cfg', (), {'strategy_id': 'S-INSTANCE-001'})()"),
                    None,
                    None,
                )
                .unwrap();
            let instance = py
                .get_type::<PyStrategy>()
                .as_any()
                .call1((config,))
                .unwrap();

            engine.py_add_strategy(&instance).unwrap();

            assert!(
                engine
                    .0
                    .kernel()
                    .trader
                    .borrow()
                    .strategy_ids()
                    .contains(&StrategyId::from("S-INSTANCE-001"))
            );
        });
    }

    #[rstest]
    fn test_add_actor_registers_python_instance() {
        use nautilus_common::python::actor::PyDataActor;
        use nautilus_model::identifiers::ActorId;
        use pyo3::{ffi::c_str, types::PyAnyMethods};

        Python::initialize();

        let mut engine =
            super::PyBacktestEngine(BacktestEngine::new(BacktestEngineConfig::default()).unwrap());

        Python::attach(|py| {
            let config = py
                .eval(
                    c_str!("type('_Cfg', (), {'actor_id': 'A-INSTANCE-001'})()"),
                    None,
                    None,
                )
                .unwrap();
            let instance = py
                .get_type::<PyDataActor>()
                .as_any()
                .call1((config,))
                .unwrap();

            engine.py_add_actor(&instance).unwrap();

            assert!(
                engine
                    .0
                    .kernel()
                    .trader
                    .borrow()
                    .actor_ids()
                    .contains(&ActorId::from("A-INSTANCE-001"))
            );
        });
    }

    #[rstest]
    fn test_add_exec_algorithm_registers_python_instance() {
        use nautilus_common::python::actor::PyDataActor;
        use nautilus_model::identifiers::ExecAlgorithmId;
        use pyo3::{ffi::c_str, types::PyAnyMethods};

        Python::initialize();

        let mut engine =
            super::PyBacktestEngine(BacktestEngine::new(BacktestEngineConfig::default()).unwrap());

        Python::attach(|py| {
            let config = py
                .eval(
                    c_str!("type('_Cfg', (), {'exec_algorithm_id': 'EXEC-INSTANCE-001'})()"),
                    None,
                    None,
                )
                .unwrap();
            let instance = py
                .get_type::<PyDataActor>()
                .as_any()
                .call1((config,))
                .unwrap();

            engine.py_add_exec_algorithm(&instance).unwrap();

            assert!(
                engine
                    .0
                    .kernel()
                    .trader
                    .borrow()
                    .exec_algorithm_ids()
                    .contains(&ExecAlgorithmId::from("EXEC-INSTANCE-001"))
            );
        });
    }

    #[rstest]
    fn test_add_strategies_registers_multiple_python_instances() {
        use nautilus_model::identifiers::StrategyId;
        use nautilus_trading::python::strategy::PyStrategy;
        use pyo3::{ffi::c_str, types::PyAnyMethods};

        Python::initialize();

        let mut engine =
            super::PyBacktestEngine(BacktestEngine::new(BacktestEngineConfig::default()).unwrap());

        Python::attach(|py| {
            let strategy_type = py.get_type::<PyStrategy>();
            let first_config = py
                .eval(
                    c_str!("type('_Cfg', (), {'strategy_id': 'S-MULTI-001'})()"),
                    None,
                    None,
                )
                .unwrap();
            let second_config = py
                .eval(
                    c_str!("type('_Cfg', (), {'strategy_id': 'S-MULTI-002'})()"),
                    None,
                    None,
                )
                .unwrap();
            let instances = vec![
                strategy_type
                    .as_any()
                    .call1((first_config,))
                    .unwrap()
                    .unbind(),
                strategy_type
                    .as_any()
                    .call1((second_config,))
                    .unwrap()
                    .unbind(),
            ];

            engine.py_add_strategies(instances).unwrap();

            let trader = engine.0.kernel().trader.borrow();
            let strategy_ids = trader.strategy_ids();
            assert!(strategy_ids.contains(&StrategyId::from("S-MULTI-001")));
            assert!(strategy_ids.contains(&StrategyId::from("S-MULTI-002")));
        });
    }

    #[rstest]
    fn test_add_exec_algorithms_registers_multiple_python_instances() {
        use nautilus_common::python::actor::PyDataActor;
        use nautilus_model::identifiers::ExecAlgorithmId;
        use pyo3::{ffi::c_str, types::PyAnyMethods};

        Python::initialize();

        let mut engine =
            super::PyBacktestEngine(BacktestEngine::new(BacktestEngineConfig::default()).unwrap());

        Python::attach(|py| {
            let algo_type = py.get_type::<PyDataActor>();
            let first_config = py
                .eval(
                    c_str!("type('_Cfg', (), {'exec_algorithm_id': 'EXEC-MULTI-001'})()"),
                    None,
                    None,
                )
                .unwrap();
            let second_config = py
                .eval(
                    c_str!("type('_Cfg', (), {'exec_algorithm_id': 'EXEC-MULTI-002'})()"),
                    None,
                    None,
                )
                .unwrap();
            let instances = vec![
                algo_type.as_any().call1((first_config,)).unwrap().unbind(),
                algo_type.as_any().call1((second_config,)).unwrap().unbind(),
            ];

            engine.py_add_exec_algorithms(instances).unwrap();

            let trader = engine.0.kernel().trader.borrow();
            let exec_algorithm_ids = trader.exec_algorithm_ids();
            assert!(exec_algorithm_ids.contains(&ExecAlgorithmId::from("EXEC-MULTI-001")));
            assert!(exec_algorithm_ids.contains(&ExecAlgorithmId::from("EXEC-MULTI-002")));
        });
    }
}

pub(crate) fn pyobject_to_fill_model_any(
    _py: Python,
    obj: &Bound<'_, PyAny>,
) -> PyResult<FillModelAny> {
    if let Ok(m) = obj.extract::<DefaultFillModel>() {
        return Ok(FillModelAny::Default(m));
    }

    if let Ok(m) = obj.extract::<BestPriceFillModel>() {
        return Ok(FillModelAny::BestPrice(m));
    }

    if let Ok(m) = obj.extract::<OneTickSlippageFillModel>() {
        return Ok(FillModelAny::OneTickSlippage(m));
    }

    if let Ok(m) = obj.extract::<ProbabilisticFillModel>() {
        return Ok(FillModelAny::Probabilistic(m));
    }

    if let Ok(m) = obj.extract::<TwoTierFillModel>() {
        return Ok(FillModelAny::TwoTier(m));
    }

    if let Ok(m) = obj.extract::<ThreeTierFillModel>() {
        return Ok(FillModelAny::ThreeTier(m));
    }

    if let Ok(m) = obj.extract::<LimitOrderPartialFillModel>() {
        return Ok(FillModelAny::LimitOrderPartialFill(m));
    }

    if let Ok(m) = obj.extract::<SizeAwareFillModel>() {
        return Ok(FillModelAny::SizeAware(m));
    }

    if let Ok(m) = obj.extract::<CompetitionAwareFillModel>() {
        return Ok(FillModelAny::CompetitionAware(m));
    }

    if let Ok(m) = obj.extract::<VolumeSensitiveFillModel>() {
        return Ok(FillModelAny::VolumeSensitive(m));
    }

    if let Ok(m) = obj.extract::<MarketHoursFillModel>() {
        return Ok(FillModelAny::MarketHours(m));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to FillModel"
    )))
}

pub(crate) fn pyobject_to_simulation_module_any(
    _py: Python,
    obj: &Bound<'_, PyAny>,
) -> PyResult<SimulationModuleAny> {
    if let Ok(cell) = obj.cast::<FXRolloverInterestModule>() {
        let module = cell.borrow().clone();
        return Ok(SimulationModuleAny::FXRolloverInterest(module));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to SimulationModule"
    )))
}

pub(crate) fn pyobject_to_latency_model_any(
    _py: Python,
    obj: &Bound<'_, PyAny>,
) -> PyResult<LatencyModelAny> {
    if let Ok(m) = obj.extract::<StaticLatencyModel>() {
        return Ok(LatencyModelAny::Static(m));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to LatencyModel"
    )))
}

pub(crate) fn pyobject_to_margin_model_any(
    _py: Python,
    obj: &Bound<'_, PyAny>,
) -> PyResult<MarginModelAny> {
    if let Ok(m) = obj.extract::<StandardMarginModel>() {
        return Ok(MarginModelAny::Standard(m));
    }

    if let Ok(m) = obj.extract::<LeveragedMarginModel>() {
        return Ok(MarginModelAny::Leveraged(m));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to MarginModel"
    )))
}

fn pyobject_to_data(_py: Python, obj: &Bound<'_, PyAny>) -> PyResult<Data> {
    if let Ok(delta) = obj.extract::<OrderBookDelta>() {
        return Ok(Data::Delta(delta));
    }

    if let Ok(deltas) = obj.extract::<OrderBookDeltas>() {
        return Ok(Data::Deltas(OrderBookDeltas_API::new(deltas)));
    }

    if let Ok(quote) = obj.extract::<QuoteTick>() {
        return Ok(Data::Quote(quote));
    }

    if let Ok(trade) = obj.extract::<TradeTick>() {
        return Ok(Data::Trade(trade));
    }

    if let Ok(bar) = obj.extract::<Bar>() {
        return Ok(Data::Bar(bar));
    }

    if let Ok(depth) = obj.extract::<OrderBookDepth10>() {
        return Ok(Data::Depth10(Box::new(depth)));
    }

    if let Ok(mark) = obj.extract::<MarkPriceUpdate>() {
        return Ok(Data::MarkPriceUpdate(mark));
    }

    if let Ok(index) = obj.extract::<IndexPriceUpdate>() {
        return Ok(Data::IndexPriceUpdate(index));
    }

    if let Ok(funding_rate) = obj.extract::<FundingRateUpdate>() {
        return Ok(Data::FundingRateUpdate(funding_rate));
    }

    if let Ok(borrow_rate) = obj.extract::<BorrowRate>() {
        return Ok(Data::BorrowRate(borrow_rate));
    }

    if let Ok(greeks) = obj.extract::<OptionGreeks>() {
        return Ok(Data::OptionGreeks(greeks));
    }

    if let Ok(status) = obj.extract::<InstrumentStatus>() {
        return Ok(Data::InstrumentStatus(status));
    }

    if let Ok(close) = obj.extract::<InstrumentClose>() {
        return Ok(Data::InstrumentClose(close));
    }

    #[cfg(feature = "defi")]
    if let Ok(defi) = obj.extract::<DefiData>() {
        return Ok(Data::Defi(Box::new(defi)));
    }

    // Fall back to from_pyobject methods for Cython objects
    if let Ok(delta) = OrderBookDelta::from_pyobject(obj) {
        return Ok(Data::Delta(delta));
    }

    if let Ok(quote) = QuoteTick::from_pyobject(obj) {
        return Ok(Data::Quote(quote));
    }

    if let Ok(trade) = TradeTick::from_pyobject(obj) {
        return Ok(Data::Trade(trade));
    }

    if let Ok(bar) = Bar::from_pyobject(obj) {
        return Ok(Data::Bar(bar));
    }

    if let Ok(mark) = MarkPriceUpdate::from_pyobject(obj) {
        return Ok(Data::MarkPriceUpdate(mark));
    }

    if let Ok(index) = IndexPriceUpdate::from_pyobject(obj) {
        return Ok(Data::IndexPriceUpdate(index));
    }

    if let Ok(funding_rate) = FundingRateUpdate::from_pyobject(obj) {
        return Ok(Data::FundingRateUpdate(funding_rate));
    }

    if let Ok(borrow_rate) = BorrowRate::from_pyobject(obj) {
        return Ok(Data::BorrowRate(borrow_rate));
    }

    if let Ok(greeks) = OptionGreeks::from_pyobject(obj) {
        return Ok(Data::OptionGreeks(greeks));
    }

    if let Ok(status) = InstrumentStatus::from_pyobject(obj) {
        return Ok(Data::InstrumentStatus(status));
    }

    if let Ok(close) = InstrumentClose::from_pyobject(obj) {
        return Ok(Data::InstrumentClose(close));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!("Cannot convert {type_name} to Data")))
}
