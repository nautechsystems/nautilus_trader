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
    python::{actor::PyDataActor, cache::PyCache},
};
use nautilus_core::{
    UUID4, UnixNanos,
    python::{to_pyruntime_err, to_pytype_err, to_pyvalue_err},
};
use nautilus_execution::models::{
    fee::{FeeModelAny, FixedFeeModel, MakerTakerFeeModel, PerContractFeeModel},
    fill::{
        BestPriceFillModel, CompetitionAwareFillModel, DefaultFillModel, FillModelAny,
        LimitOrderPartialFillModel, MarketHoursFillModel, OneTickSlippageFillModel,
        ProbabilisticFillModel, SizeAwareFillModel, ThreeTierFillModel, TwoTierFillModel,
        VolumeSensitiveFillModel,
    },
    latency::{LatencyModelAny, StaticLatencyModel},
};
use nautilus_model::{
    accounts::margin_model::{LeveragedMarginModel, MarginModelAny, StandardMarginModel},
    data::{
        Bar, Data, IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API, OrderBookDepth10, QuoteTick,
        TradeTick,
    },
    enums::{AccountType, BookType, OmsType, OtoTriggerMode},
    identifiers::{ActorId, ClientId, ComponentId, InstrumentId, StrategyId, TraderId, Venue},
    python::instruments::pyobject_to_instrument_any,
    types::{Currency, Money, Price},
};
use nautilus_trading::{
    ImportableStrategyConfig,
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

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyBacktestEngine {
    #[new]
    fn py_new(config: BacktestEngineConfig) -> PyResult<Self> {
        let engine = BacktestEngine::new(config).map_err(to_pyruntime_err)?;
        Ok(Self(engine))
    }

    /// Adds a simulated exchange with the given parameters to the engine.
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
        )
    )]
    #[expect(clippy::too_many_arguments)]
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
            .map(|obj| Python::attach(|py| pyobject_to_fee_model_any(py, obj.bind(py))))
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
            .build();

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

    /// Adds an actor from an importable config.
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

        let parts: Vec<&str> = config.actor_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "actor_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing actor from module: {module_name} class: {class_name}");

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

                let mut py_data_actor_ref = python_actor
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

                py_data_actor_ref.set_python_instance(python_actor.clone().unbind());
                let actor_id = py_data_actor_ref.actor_id();

                Ok((python_actor.unbind(), actor_id))
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
            let py_actor = python_actor.bind(py);
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
            let py_actor = python_actor.bind(py);
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

    /// Adds a strategy from an importable config.
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

        let parts: Vec<&str> = config.strategy_path.split(':').collect();
        if parts.len() != 2 {
            return Err(to_pyvalue_err(
                "strategy_path must be in format 'module.path:ClassName'",
            ));
        }
        let (module_name, class_name) = (parts[0], parts[1]);

        log::info!("Importing strategy from module: {module_name} class: {class_name}");

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

                let mut py_strategy_ref = python_strategy
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
            let py_strategy = python_strategy.bind(py);
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
            let py_strategy = python_strategy.bind(py);
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
            self.0
                .add_strategy(EmaCross::from_config(config))
                .map_err(to_pyruntime_err)
        } else if let Ok(config) = config.extract::<GridMarketMakerConfig>() {
            self.0
                .add_strategy(GridMarketMaker::new(config))
                .map_err(to_pyruntime_err)
        } else if let Ok(config) = config.extract::<DeltaNeutralVolConfig>() {
            self.0
                .add_strategy(DeltaNeutralVol::new(config))
                .map_err(to_pyruntime_err)
        } else if let Ok(config) = config.extract::<HurstVpinDirectionalConfig>() {
            self.0
                .add_strategy(HurstVpinDirectional::new(config))
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
            self.0
                .add_actor(BookImbalanceActor::from_config(config))
                .map_err(to_pyruntime_err)
        } else {
            let type_name = config.get_type().name()?;
            Err(to_pytype_err(format!(
                "Unsupported native actor config type: {type_name}",
            )))
        }
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

    fn __repr__(&self) -> String {
        format!("{:?}", self.0)
    }
}

impl PyBacktestEngine {
    /// Provides access to the inner [`BacktestEngine`].
    #[must_use]
    pub fn inner(&self) -> &BacktestEngine {
        &self.0
    }

    /// Provides mutable access to the inner [`BacktestEngine`].
    pub fn inner_mut(&mut self) -> &mut BacktestEngine {
        &mut self.0
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

pub(crate) fn pyobject_to_fee_model_any(
    _py: Python,
    obj: &Bound<'_, PyAny>,
) -> PyResult<FeeModelAny> {
    if let Ok(m) = obj.extract::<FixedFeeModel>() {
        return Ok(FeeModelAny::Fixed(m));
    }

    if let Ok(m) = obj.extract::<MakerTakerFeeModel>() {
        return Ok(FeeModelAny::MakerTaker(m));
    }

    if let Ok(m) = obj.extract::<PerContractFeeModel>() {
        return Ok(FeeModelAny::PerContract(m));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to FeeModel"
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

    if let Ok(status) = obj.extract::<InstrumentStatus>() {
        return Ok(Data::InstrumentStatus(status));
    }

    if let Ok(close) = obj.extract::<InstrumentClose>() {
        return Ok(Data::InstrumentClose(close));
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

    if let Ok(status) = InstrumentStatus::from_pyobject(obj) {
        return Ok(Data::InstrumentStatus(status));
    }

    if let Ok(close) = InstrumentClose::from_pyobject(obj) {
        return Ok(Data::InstrumentClose(close));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!("Cannot convert {type_name} to Data")))
}
