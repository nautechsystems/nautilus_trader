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

//! Python bindings and native extractor registry for simulation module types.

use std::sync::LazyLock;

use ahash::AHashMap;
use jiff::civil::{Time, Weekday};
use nautilus_core::python::{
    clone_py_object, to_pynotimplemented_err, to_pytype_err, to_pyvalue_err,
};
use nautilus_model::{
    data::Data,
    identifiers::{InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    position::Position,
    python::{data::data_to_pyobject, instruments::instrument_any_to_pyobject},
    types::{Currency, Money},
};
use parking_lot::Mutex;
use pyo3::{
    PyClass,
    prelude::*,
    types::{PyDict, PyTuple},
};
use rust_decimal::Decimal;

use crate::modules::{
    AccountAdjustmentOutcome, CfdSwapModule, CfdSwapRate, ExchangeContext,
    FXRolloverInterestModule, SimulationModule, SimulationModuleAny, SimulationModuleHandle,
    SimulationModuleResult, fx_rollover::InterestRateRecord,
};

/// Function pointer for extracting a linked native simulation module from Python.
pub type SimulationModuleExtractor =
    for<'py> fn(Python<'py>, &Bound<'py, PyAny>) -> PyResult<SimulationModuleHandle>;

static SIMULATION_MODULE_EXTRACTORS: LazyLock<Mutex<AHashMap<usize, SimulationModuleExtractor>>> =
    LazyLock::new(|| Mutex::new(AHashMap::new()));

/// Registers an extractor for a linked native simulation module Python class.
///
/// Registering the same function for the same Python type more than once succeeds without change.
///
/// # Errors
///
/// Returns an error if a different extractor is already registered for `T`.
pub fn register_simulation_module_extractor<T: PyClass>(
    py: Python<'_>,
    extractor: SimulationModuleExtractor,
) -> anyhow::Result<()> {
    let type_object = py.get_type::<T>();
    let type_id = type_object.as_ptr() as usize;
    let type_name = type_object.name()?;
    let mut extractors = SIMULATION_MODULE_EXTRACTORS.lock();
    if let Some(registered) = extractors.get(&type_id) {
        if std::ptr::fn_addr_eq(*registered, extractor) {
            return Ok(());
        }
        anyhow::bail!(
            "A different simulation module extractor is already registered for '{type_name}'"
        );
    }
    extractors.insert(type_id, extractor);
    Ok(())
}

#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")]
#[pyclass(
    module = "nautilus_trader.backtest",
    name = "SimulationModule",
    subclass
)]
#[derive(Debug)]
pub struct PySimulationModule;

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
#[allow(
    clippy::unused_self,
    reason = "PyO3 exposes these hooks as overridable instance methods"
)]
impl PySimulationModule {
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn py_new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self
    }

    fn pre_process(&self, _data: &Bound<'_, PyAny>) {}

    fn process(
        &self,
        _ts_now: u64,
        _context: &PySimulationModuleContext,
    ) -> PyResult<Option<Vec<Money>>> {
        Err(to_pynotimplemented_err(
            "Method 'process' must be implemented in a subclass.",
        ))
    }

    fn acknowledge(&self, _outcomes: &Bound<'_, PyAny>) {}

    fn log_diagnostics(&self) {}

    fn reset(&self) {}
}

/// Read-only owned snapshot of the exchange state exposed to Python simulation modules.
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")]
#[pyclass(
    module = "nautilus_trader.backtest",
    name = "SimulationModuleContext",
    frozen,
    unsendable
)]
#[derive(Debug)]
pub struct PySimulationModuleContext {
    venue: Venue,
    base_currency: Option<Currency>,
    instruments: Vec<InstrumentAny>,
    order_books: Vec<OrderBook>,
    positions: Vec<Position>,
}

impl PySimulationModuleContext {
    fn from_exchange(ctx: &ExchangeContext<'_>) -> Self {
        let mut instruments = ctx.instruments.values().cloned().collect::<Vec<_>>();
        instruments.sort_unstable_by_key(Instrument::id);

        let mut order_books = ctx
            .matching_engines
            .values()
            .map(|engine| engine.get_book().clone())
            .collect::<Vec<_>>();
        order_books.sort_unstable_by_key(|book| book.instrument_id);

        let mut positions = ctx
            .cache
            .positions_open(Some(&ctx.venue), None, None, None, None)
            .into_iter()
            .map(|position| (*position).clone())
            .collect::<Vec<_>>();
        positions.sort_unstable_by_key(|position| position.id);

        Self {
            venue: ctx.venue,
            base_currency: ctx.base_currency,
            instruments,
            order_books,
            positions,
        }
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PySimulationModuleContext {
    #[getter]
    fn venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[getter]
    fn instruments(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.instruments
            .iter()
            .cloned()
            .map(|instrument| instrument_any_to_pyobject(py, instrument))
            .collect()
    }

    #[getter]
    fn order_books(&self) -> Vec<OrderBook> {
        self.order_books.clone()
    }

    #[getter]
    fn positions(&self) -> Vec<Position> {
        self.positions.clone()
    }
}

/// Read-only account adjustment result passed to Python module acknowledgements.
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.backtest")]
#[pyclass(
    module = "nautilus_trader.backtest",
    name = "AccountAdjustmentOutcome",
    frozen,
    skip_from_py_object
)]
#[derive(Debug, Clone)]
pub struct PyAccountAdjustmentOutcome {
    applied: bool,
    error: Option<String>,
}

impl From<&AccountAdjustmentOutcome> for PyAccountAdjustmentOutcome {
    fn from(outcome: &AccountAdjustmentOutcome) -> Self {
        match outcome {
            AccountAdjustmentOutcome::Applied => Self {
                applied: true,
                error: None,
            },
            AccountAdjustmentOutcome::Failed(error) => Self {
                applied: false,
                error: Some(error.to_string()),
            },
        }
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyAccountAdjustmentOutcome {
    #[getter]
    const fn applied(&self) -> bool {
        self.applied
    }

    #[getter]
    fn error(&self) -> Option<String> {
        self.error.clone()
    }
}

#[derive(Debug)]
pub struct PythonSimulationModule {
    obj: Py<PyAny>,
}

impl Clone for PythonSimulationModule {
    fn clone(&self) -> Self {
        Self::new(clone_py_object(&self.obj))
    }
}

impl PythonSimulationModule {
    #[must_use]
    pub const fn new(obj: Py<PyAny>) -> Self {
        Self { obj }
    }

    pub(crate) fn clone_ref(&self, py: Python<'_>) -> Py<PyAny> {
        self.obj.clone_ref(py)
    }
}

impl SimulationModule for PythonSimulationModule {
    fn pre_process(&self, data: &Data) -> anyhow::Result<()> {
        Python::attach(|py| -> anyhow::Result<()> {
            let data = data_to_pyobject(py, data.clone())?;
            self.obj.bind(py).call_method1("pre_process", (data,))?;
            Ok(())
        })
        .map_err(|e| anyhow::anyhow!("Python SimulationModule.pre_process failed: {e}"))
    }

    fn process(
        &self,
        ts_now: nautilus_core::UnixNanos,
        ctx: &ExchangeContext,
    ) -> anyhow::Result<SimulationModuleResult> {
        Python::attach(|py| -> anyhow::Result<SimulationModuleResult> {
            let context = Py::new(py, PySimulationModuleContext::from_exchange(ctx))?;
            let adjustments = self
                .obj
                .bind(py)
                .call_method1("process", (ts_now.as_u64(), context))?
                .extract::<Option<Vec<Money>>>()?;
            Ok(adjustments.map_or(
                SimulationModuleResult::NotReady,
                SimulationModuleResult::Completed,
            ))
        })
        .map_err(|e| anyhow::anyhow!("Python SimulationModule.process failed: {e}"))
    }

    fn acknowledge(&self, outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
        Python::attach(|py| -> anyhow::Result<()> {
            let outcomes = outcomes
                .iter()
                .map(PyAccountAdjustmentOutcome::from)
                .collect::<Vec<_>>();
            self.obj.bind(py).call_method1("acknowledge", (outcomes,))?;
            Ok(())
        })
        .map_err(|e| anyhow::anyhow!("Python SimulationModule.acknowledge failed: {e}"))
    }

    fn log_diagnostics(&self) -> anyhow::Result<()> {
        Python::attach(|py| -> anyhow::Result<()> {
            self.obj.bind(py).call_method0("log_diagnostics")?;
            Ok(())
        })
        .map_err(|e| anyhow::anyhow!("Python SimulationModule.log_diagnostics failed: {e}"))
    }

    fn reset(&self) -> anyhow::Result<()> {
        Python::attach(|py| -> anyhow::Result<()> {
            self.obj.bind(py).call_method0("reset")?;
            Ok(())
        })
        .map_err(|e| anyhow::anyhow!("Python SimulationModule.reset failed: {e}"))
    }
}

fn pyobject_to_builtin_simulation_module_any(
    obj: &Bound<'_, PyAny>,
) -> Option<SimulationModuleAny> {
    if let Ok(module) = obj.extract::<PyRef<'_, CfdSwapModule>>() {
        return Some(SimulationModuleAny::CfdSwap((*module).clone()));
    }

    if let Ok(module) = obj.extract::<PyRef<'_, FXRolloverInterestModule>>() {
        return Some(SimulationModuleAny::FXRolloverInterest((*module).clone()));
    }
    None
}

/// Extracts a Python object into a declarative simulation module.
///
/// # Errors
///
/// Returns an error if `obj` is neither a built-in nor a Python `SimulationModule` instance.
pub fn pyobject_to_simulation_module_any(obj: &Bound<'_, PyAny>) -> PyResult<SimulationModuleAny> {
    if let Some(module) = pyobject_to_builtin_simulation_module_any(obj) {
        return Ok(module);
    }

    if obj.is_instance_of::<PySimulationModule>() {
        return Ok(SimulationModuleAny::Python(PythonSimulationModule::new(
            obj.clone().unbind(),
        )));
    }

    let type_name = obj.get_type().name()?;
    Err(to_pytype_err(format!(
        "Cannot convert {type_name} to SimulationModule"
    )))
}

/// Extracts a Python object into a runtime simulation module handle.
///
/// Built-ins resolve first, followed by linked native extractors, then Python subclasses.
///
/// # Errors
///
/// Returns an error if the object cannot be resolved or its native extractor fails.
pub fn pyobject_to_simulation_module_handle(
    py: Python<'_>,
    obj: &Bound<'_, PyAny>,
) -> PyResult<SimulationModuleHandle> {
    if let Some(module) = pyobject_to_builtin_simulation_module_any(obj) {
        return Ok(module.into());
    }

    let type_object = obj.get_type();
    let type_id = type_object.as_ptr() as usize;
    let extractor = SIMULATION_MODULE_EXTRACTORS.lock().get(&type_id).copied();

    if let Some(extractor) = extractor {
        return extractor(py, obj);
    }

    if obj.is_instance_of::<PySimulationModule>() {
        return Ok(SimulationModuleHandle::new(PythonSimulationModule::new(
            obj.clone().unbind(),
        )));
    }

    Err(to_pytype_err(format!(
        "Cannot convert {} to SimulationModule",
        type_object.name()?
    )))
}

/// Converts a declarative simulation module into its Python binding object.
///
/// # Errors
///
/// Returns an error if the Python object cannot be allocated.
pub fn simulation_module_any_to_pyobject(
    py: Python<'_>,
    module: &SimulationModuleAny,
) -> PyResult<Py<PyAny>> {
    match module {
        SimulationModuleAny::CfdSwap(module) => Ok(Py::new(
            py,
            PyClassInitializer::from(PySimulationModule).add_subclass(module.clone()),
        )?
        .into_any()),
        SimulationModuleAny::FXRolloverInterest(module) => Ok(Py::new(
            py,
            PyClassInitializer::from(PySimulationModule).add_subclass(module.clone()),
        )?
        .into_any()),
        SimulationModuleAny::Python(module) => Ok(module.clone_ref(py)),
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl InterestRateRecord {
    /// A single interest rate data entry.
    #[new]
    fn py_new(location: String, time: String, value: f64) -> PyResult<Self> {
        let record = Self {
            location,
            time,
            value,
        };
        record.validate().map_err(to_pyvalue_err)?;
        Ok(record)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl FXRolloverInterestModule {
    /// Simulates FX rollover (swap) interest applied at 5 PM US/Eastern daily.
    ///
    /// When holding FX positions overnight, the interest rate differential
    /// between the two currencies is credited or debited. Wednesday and Friday
    /// rollovers are tripled (Wednesday for T+2 settlement, Friday for the weekend).
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    fn py_new(records: Vec<InterestRateRecord>) -> PyResult<PyClassInitializer<Self>> {
        let module = Self::new(records).map_err(to_pyvalue_err)?;
        Ok(PyClassInitializer::from(PySimulationModule).add_subclass(module))
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl CfdSwapRate {
    /// Daily long and short swap rates for a CFD instrument.
    #[new]
    fn py_new(instrument_id: InstrumentId, long_rate: Decimal, short_rate: Decimal) -> Self {
        Self::new(instrument_id, long_rate, short_rate)
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn long_rate(&self) -> Decimal {
        self.long_rate
    }

    #[getter]
    fn short_rate(&self) -> Decimal {
        self.short_rate
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl CfdSwapModule {
    /// Simulates daily CFD swap adjustments at a configurable UTC rollover time.
    #[new]
    #[gen_stub(override_return_type(type_repr = "typing.Self", imports = ("typing",)))]
    #[pyo3(signature = (rates, rollover_hour=17, rollover_minute=0, triple_roll_weekday=5))]
    fn py_new(
        rates: Vec<CfdSwapRate>,
        rollover_hour: i8,
        rollover_minute: i8,
        triple_roll_weekday: i8,
    ) -> PyResult<PyClassInitializer<Self>> {
        let rollover_time =
            Time::new(rollover_hour, rollover_minute, 0, 0).map_err(to_pyvalue_err)?;
        let triple_roll_weekday =
            Weekday::from_monday_one_offset(triple_roll_weekday).map_err(to_pyvalue_err)?;
        Ok(
            PyClassInitializer::from(PySimulationModule).add_subclass(Self::new(
                rates,
                rollover_time,
                triple_roll_weekday,
            )),
        )
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use indexmap::IndexMap;
    use nautilus_common::cache::Cache;
    use nautilus_model::{
        data::Data,
        enums::{AccountType, BookType, OmsType},
        identifiers::Venue,
        types::{Currency, Money},
    };
    use pyo3::{IntoPyObjectExt, exceptions::PyAttributeError, ffi::c_str, types::PyDict};
    use rstest::rstest;

    use super::*;
    use crate::{
        config::{BacktestEngineConfig, SimulatedVenueConfig},
        engine::BacktestEngine,
    };

    fn with_empty_context<T>(f: impl FnOnce(&ExchangeContext<'_>) -> T) -> T {
        let instruments = AHashMap::new();
        let matching_engines = IndexMap::new();
        let cache = Cache::default();
        f(&ExchangeContext {
            venue: Venue::new("SIM"),
            base_currency: Some(Currency::USD()),
            instruments: &instruments,
            matching_engines: &matching_engines,
            cache: &cache,
        })
    }

    #[rstest]
    fn test_pure_python_simulation_module_dispatch() {
        Python::initialize();

        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("SimulationModule", py.get_type::<PySimulationModule>())
                .unwrap();
            let module = py
                .eval(
                    c_str!(
                        "type('PurePythonSimulationModule', (SimulationModule,), {\
                            'process': lambda self, ts_now, context: \
                                (setattr(self, 'context', context), [self.adjustment])[1]\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();
            module
                .setattr(
                    "adjustment",
                    Money::from("1.25 USD").into_py_any(py).unwrap(),
                )
                .unwrap();

            assert!(matches!(
                pyobject_to_simulation_module_any(&module).unwrap(),
                SimulationModuleAny::Python(_)
            ));
            let handle = pyobject_to_simulation_module_handle(py, &module).unwrap();
            let result =
                with_empty_context(|ctx| handle.process(nautilus_core::UnixNanos::from(10), ctx))
                    .unwrap();
            handle
                .acknowledge(&[AccountAdjustmentOutcome::Applied])
                .unwrap();
            handle.reset().unwrap();

            let context = module.getattr("context").unwrap();

            assert_eq!(
                result,
                SimulationModuleResult::Completed(vec![Money::from("1.25 USD")])
            );
            assert_eq!(
                context
                    .getattr("venue")
                    .unwrap()
                    .extract::<Venue>()
                    .unwrap(),
                Venue::new("SIM")
            );
            assert_eq!(
                context
                    .getattr("base_currency")
                    .unwrap()
                    .extract::<Option<Currency>>()
                    .unwrap(),
                Some(Currency::USD())
            );
            assert_eq!(context.getattr("instruments").unwrap().len().unwrap(), 0);
            assert_eq!(context.getattr("order_books").unwrap().len().unwrap(), 0);
            assert_eq!(context.getattr("positions").unwrap().len().unwrap(), 0);
            let error = context.setattr("venue", Venue::new("OTHER")).unwrap_err();
            assert!(error.is_instance_of::<PyAttributeError>(py));
        });
    }

    #[derive(Debug)]
    struct NativeRustSimulationModule {
        calls: Rc<Cell<u32>>,
    }

    impl SimulationModule for NativeRustSimulationModule {
        fn pre_process(&self, _data: &Data) -> anyhow::Result<()> {
            Ok(())
        }

        fn process(
            &self,
            _ts_now: nautilus_core::UnixNanos,
            _ctx: &ExchangeContext,
        ) -> anyhow::Result<SimulationModuleResult> {
            self.calls.set(self.calls.get() + 1);
            Ok(SimulationModuleResult::NotReady)
        }

        fn acknowledge(&self, _outcomes: &[AccountAdjustmentOutcome]) -> anyhow::Result<()> {
            Ok(())
        }

        fn log_diagnostics(&self) -> anyhow::Result<()> {
            Ok(())
        }

        fn reset(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[pyclass(
        name = "NativeSimulationModuleTest",
        module = "native_simulation_module_test.registered",
        unsendable
    )]
    #[derive(Debug)]
    struct NativeSimulationModuleBinding {
        calls: Rc<Cell<u32>>,
    }

    #[pyclass(
        name = "NativeSimulationModuleTest",
        module = "native_simulation_module_test.unregistered",
        unsendable
    )]
    #[derive(Debug)]
    struct UnregisteredNativeSimulationModuleBinding;

    fn extract_native_simulation_module(
        _py: Python<'_>,
        obj: &Bound<'_, PyAny>,
    ) -> PyResult<SimulationModuleHandle> {
        let binding = obj.extract::<PyRef<'_, NativeSimulationModuleBinding>>()?;
        Ok(SimulationModuleHandle::new(NativeRustSimulationModule {
            calls: binding.calls.clone(),
        }))
    }

    #[rstest]
    fn test_registered_native_simulation_module_dispatch() {
        Python::initialize();

        Python::attach(|py| {
            register_simulation_module_extractor::<NativeSimulationModuleBinding>(
                py,
                extract_native_simulation_module,
            )
            .unwrap();
            let calls = Rc::new(Cell::new(0));
            let binding = Py::new(
                py,
                NativeSimulationModuleBinding {
                    calls: calls.clone(),
                },
            )
            .unwrap();
            let handle =
                pyobject_to_simulation_module_handle(py, binding.bind(py).as_any()).unwrap();

            let result =
                with_empty_context(|ctx| handle.process(nautilus_core::UnixNanos::from(10), ctx))
                    .unwrap();

            assert_eq!(result, SimulationModuleResult::NotReady);
            assert_eq!(calls.get(), 1);
        });
    }

    #[rstest]
    fn test_registered_native_simulation_module_uses_exact_python_type() {
        Python::initialize();

        Python::attach(|py| {
            register_simulation_module_extractor::<NativeSimulationModuleBinding>(
                py,
                extract_native_simulation_module,
            )
            .unwrap();
            let binding = Py::new(py, UnregisteredNativeSimulationModuleBinding).unwrap();

            let error =
                pyobject_to_simulation_module_handle(py, binding.bind(py).as_any()).unwrap_err();

            assert_eq!(
                error.to_string(),
                "TypeError: Cannot convert NativeSimulationModuleTest to SimulationModule"
            );
        });
    }

    fn engine_with_module(module: SimulationModuleHandle) -> BacktestEngine {
        let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
        engine
            .add_venue(
                SimulatedVenueConfig::builder()
                    .venue(Venue::new("SIM"))
                    .oms_type(OmsType::Netting)
                    .account_type(AccountType::Margin)
                    .book_type(BookType::L1_MBP)
                    .starting_balances(vec![Money::from("1000 USD")])
                    .modules(vec![module])
                    .build()
                    .unwrap(),
            )
            .unwrap();
        engine
    }

    #[rstest]
    fn test_python_simulation_module_exception_propagates_through_run() {
        Python::initialize();

        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("SimulationModule", py.get_type::<PySimulationModule>())
                .unwrap();
            let module = py
                .eval(
                    c_str!(
                        "type('FailingSimulationModule', (SimulationModule,), {\
                            'process': lambda self, ts_now, context: \
                                (_ for _ in ()).throw(ValueError('module boom'))\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();
            let handle = pyobject_to_simulation_module_handle(py, &module).unwrap();
            let mut engine = engine_with_module(handle);

            let error = engine.run(None, None, None, false).unwrap_err();
            let message = error.to_string();
            assert!(message.contains("Simulation module 0 process failed"));
            assert!(message.contains("Python SimulationModule.process failed"));
            assert!(message.contains("ValueError: module boom"));

            assert_eq!(
                engine.run(None, None, None, false).unwrap_err().to_string(),
                format!("Simulation module failure requires exchange reset: {message}")
            );
        });
    }

    #[rstest]
    fn test_python_simulation_module_diagnostics_exception_propagates_through_run() {
        Python::initialize();

        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("SimulationModule", py.get_type::<PySimulationModule>())
                .unwrap();
            let module = py
                .eval(
                    c_str!(
                        "type('FailingDiagnosticsSimulationModule', (SimulationModule,), {\
                            'process': lambda self, ts_now, context: None,\
                            'log_diagnostics': lambda self: \
                                (_ for _ in ()).throw(ValueError('diagnostics boom'))\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();
            let handle = pyobject_to_simulation_module_handle(py, &module).unwrap();
            let mut engine = engine_with_module(handle);

            let error = engine.run(None, None, None, false).unwrap_err();
            let message = error.to_string();

            assert!(message.contains("Simulation module 0 log_diagnostics failed"));
            assert!(message.contains("Python SimulationModule.log_diagnostics failed"));
            assert!(message.contains("ValueError: diagnostics boom"));
        });
    }

    #[rstest]
    fn test_engine_reset_finishes_after_python_diagnostics_exception() {
        Python::initialize();

        Python::attach(|py| {
            let locals = PyDict::new(py);
            locals
                .set_item("SimulationModule", py.get_type::<PySimulationModule>())
                .unwrap();
            let module = py
                .eval(
                    c_str!(
                        "type('ResetAfterDiagnosticsFailureModule', (SimulationModule,), {\
                            'process': lambda self, ts_now, context: None,\
                            'log_diagnostics': lambda self: \
                                (_ for _ in ()).throw(ValueError('diagnostics boom')) \
                                if self.fail_diagnostics else None,\
                            'reset': lambda self: \
                                setattr(self, 'resets', self.resets + 1)\
                        })()"
                    ),
                    None,
                    Some(&locals),
                )
                .unwrap();
            module.setattr("fail_diagnostics", true).unwrap();
            module.setattr("resets", 0).unwrap();
            let handle = pyobject_to_simulation_module_handle(py, &module).unwrap();
            let mut engine = engine_with_module(handle);
            engine.run(None, None, None, true).unwrap();

            let error = engine.reset().unwrap_err();

            assert!(
                error
                    .to_string()
                    .contains("Simulation module 0 log_diagnostics failed")
            );
            assert_eq!(
                module.getattr("resets").unwrap().extract::<u32>().unwrap(),
                1
            );
            module.setattr("fail_diagnostics", false).unwrap();
            engine.run(None, None, None, false).unwrap();
        });
    }
}
