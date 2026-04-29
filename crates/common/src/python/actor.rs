// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Python bindings for DataActor with complete command and event handler forwarding.

use std::{
    any::Any,
    cell::{RefCell, UnsafeCell},
    collections::HashMap,
    fmt::Debug,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use nautilus_core::{
    from_pydict,
    nanos::UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
#[cfg(feature = "defi")]
use nautilus_model::defi::{
    Block, Blockchain, Pool, PoolFeeCollect, PoolFlash, PoolLiquidityUpdate, PoolSwap,
};
use nautilus_model::{
    data::{
        Bar, BarType, CustomData, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick,
        close::InstrumentClose,
        option_chain::{OptionChainSlice, OptionGreeks},
    },
    enums::BookType,
    identifiers::{ActorId, ClientId, InstrumentId, OptionSeriesId, TraderId, Venue},
    instruments::{InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    python::{data::option_chain::PyStrikeRange, instruments::instrument_any_to_pyobject},
};
use pyo3::{prelude::*, types::PyDict};

use crate::{
    actor::{
        Actor, DataActor,
        data_actor::{DataActorConfig, DataActorCore, ImportableActorConfig},
        registry::{get_actor_registry, try_get_actor_unchecked},
    },
    cache::Cache,
    clock::Clock,
    component::{Component, get_component_registry},
    enums::ComponentState,
    python::{cache::PyCache, clock::PyClock, logging::PyLogger},
    signal::Signal,
    timer::{TimeEvent, TimeEventCallback},
};

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataActorConfig {
    /// Common configuration for `DataActor` based components.
    #[new]
    #[pyo3(signature = (actor_id=None, log_events=true, log_commands=true))]
    fn py_new(actor_id: Option<ActorId>, log_events: bool, log_commands: bool) -> Self {
        Self {
            actor_id,
            log_events,
            log_commands,
        }
    }
}

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ImportableActorConfig {
    /// Configuration for creating actors from importable paths.
    #[new]
    #[expect(clippy::needless_pass_by_value)]
    fn py_new(actor_path: String, config_path: String, config: Py<PyDict>) -> PyResult<Self> {
        let json_config = Python::attach(|py| -> PyResult<HashMap<String, serde_json::Value>> {
            let kwargs = PyDict::new(py);
            kwargs.set_item("default", py.eval(pyo3::ffi::c_str!("str"), None, None)?)?;
            let json_str: String = PyModule::import(py, "json")?
                .call_method("dumps", (config.bind(py),), Some(&kwargs))?
                .extract()?;

            let json_value: serde_json::Value =
                serde_json::from_str(&json_str).map_err(to_pyvalue_err)?;

            if let serde_json::Value::Object(map) = json_value {
                Ok(map.into_iter().collect())
            } else {
                Err(to_pyvalue_err("Config must be a dictionary"))
            }
        })?;

        Ok(Self {
            actor_path,
            config_path,
            config: json_config,
        })
    }

    #[getter]
    fn actor_path(&self) -> &String {
        &self.actor_path
    }

    #[getter]
    fn config_path(&self) -> &String {
        &self.config_path
    }

    #[getter]
    fn config(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        // Convert HashMap<String, serde_json::Value> back to Python dict
        let py_dict = PyDict::new(py);

        for (key, value) in &self.config {
            // Convert serde_json::Value back to Python object via JSON
            let json_str = serde_json::to_string(value).map_err(to_pyvalue_err)?;
            let py_value = PyModule::import(py, "json")?.call_method("loads", (json_str,), None)?;
            py_dict.set_item(key, py_value)?;
        }
        Ok(py_dict.unbind())
    }
}

/// Inner state of PyDataActor, shared between Python wrapper and Rust registries.
///
/// This type holds the actual actor state and implements all the actor traits.
/// It is wrapped in `Rc<UnsafeCell<>>` to allow shared ownership between Python
/// and the global registries without copying.
pub struct PyDataActorInner {
    core: DataActorCore,
    py_self: Option<Py<PyAny>>,
    clock: PyClock,
    logger: PyLogger,
}

impl Debug for PyDataActorInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyDataActorInner))
            .field("core", &self.core)
            .field("py_self", &self.py_self.as_ref().map(|_| "<Py<PyAny>>"))
            .field("clock", &self.clock)
            .field("logger", &self.logger)
            .finish()
    }
}

impl Deref for PyDataActorInner {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for PyDataActorInner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

#[expect(clippy::needless_pass_by_ref_mut)]
impl PyDataActorInner {
    fn dispatch_on_start(&self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_start"))?;
        }
        Ok(())
    }

    fn dispatch_on_stop(&mut self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_stop"))?;
        }
        Ok(())
    }

    fn dispatch_on_resume(&mut self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_resume"))?;
        }
        Ok(())
    }

    fn dispatch_on_reset(&mut self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_reset"))?;
        }
        Ok(())
    }

    fn dispatch_on_dispose(&mut self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_dispose"))?;
        }
        Ok(())
    }

    fn dispatch_on_degrade(&mut self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_degrade"))?;
        }
        Ok(())
    }

    fn dispatch_on_fault(&mut self) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method0(py, "on_fault"))?;
        }
        Ok(())
    }

    fn dispatch_on_time_event(&mut self, event: TimeEvent) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_time_event", (event.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_data(&mut self, data: Py<PyAny>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_data", (data,)))?;
        }
        Ok(())
    }

    fn dispatch_on_signal(&mut self, signal: &Signal) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_signal", (signal.clone().into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_instrument(&mut self, instrument: Py<PyAny>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_instrument", (instrument,)))?;
        }
        Ok(())
    }

    fn dispatch_on_quote(&mut self, quote: QuoteTick) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_quote", (quote.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_trade(&mut self, trade: TradeTick) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_trade", (trade.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_bar(&mut self, bar: Bar) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_bar", (bar.into_py_any_unwrap(py),)))?;
        }
        Ok(())
    }

    fn dispatch_on_book_deltas(&mut self, deltas: OrderBookDeltas) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_book_deltas", (deltas.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_book(&mut self, book: &OrderBook) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_book", (book.clone().into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_mark_price(&mut self, mark_price: MarkPriceUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_mark_price", (mark_price.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_index_price(&mut self, index_price: IndexPriceUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_index_price", (index_price.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_funding_rate(&mut self, funding_rate: FundingRateUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_funding_rate",
                    (funding_rate.into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    fn dispatch_on_instrument_status(&mut self, data: InstrumentStatus) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_instrument_status", (data.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_instrument_close(&mut self, update: InstrumentClose) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_instrument_close", (update.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_option_greeks(&mut self, greeks: OptionGreeks) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_option_greeks", (greeks.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_option_chain(&mut self, slice: OptionChainSlice) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_option_chain", (slice.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_data(&mut self, data: Py<PyAny>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| py_self.call_method1(py, "on_historical_data", (data,)))?;
        }
        Ok(())
    }

    fn dispatch_on_historical_quotes(&mut self, quotes: Vec<QuoteTick>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_quotes: Vec<_> = quotes
                    .into_iter()
                    .map(|q| q.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_quotes", (py_quotes,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_trades(&mut self, trades: Vec<TradeTick>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_trades: Vec<_> = trades
                    .into_iter()
                    .map(|t| t.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_trades", (py_trades,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_funding_rates(
        &mut self,
        funding_rates: Vec<FundingRateUpdate>,
    ) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_rates: Vec<_> = funding_rates
                    .into_iter()
                    .map(|r| r.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_funding_rates", (py_rates,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_bars(&mut self, bars: Vec<Bar>) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_bars: Vec<_> = bars.into_iter().map(|b| b.into_py_any_unwrap(py)).collect();
                py_self.call_method1(py, "on_historical_bars", (py_bars,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_mark_prices(
        &mut self,
        mark_prices: Vec<MarkPriceUpdate>,
    ) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_prices: Vec<_> = mark_prices
                    .into_iter()
                    .map(|p| p.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_mark_prices", (py_prices,))
            })?;
        }
        Ok(())
    }

    fn dispatch_on_historical_index_prices(
        &mut self,
        index_prices: Vec<IndexPriceUpdate>,
    ) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                let py_prices: Vec<_> = index_prices
                    .into_iter()
                    .map(|p| p.into_py_any_unwrap(py))
                    .collect();
                py_self.call_method1(py, "on_historical_index_prices", (py_prices,))
            })?;
        }
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn dispatch_on_block(&mut self, block: Block) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_block", (block.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn dispatch_on_pool(&mut self, pool: Pool) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_pool", (pool.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn dispatch_on_pool_swap(&mut self, swap: PoolSwap) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_pool_swap", (swap.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn dispatch_on_pool_liquidity_update(&mut self, update: PoolLiquidityUpdate) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(
                    py,
                    "on_pool_liquidity_update",
                    (update.into_py_any_unwrap(py),),
                )
            })?;
        }
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn dispatch_on_pool_fee_collect(&mut self, collect: PoolFeeCollect) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_pool_fee_collect", (collect.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }

    #[cfg(feature = "defi")]
    fn dispatch_on_pool_flash(&mut self, flash: PoolFlash) -> PyResult<()> {
        if let Some(ref py_self) = self.py_self {
            Python::attach(|py| {
                py_self.call_method1(py, "on_pool_flash", (flash.into_py_any_unwrap(py),))
            })?;
        }
        Ok(())
    }
}

fn dict_to_params(
    py: Python<'_>,
    params: Option<Py<PyDict>>,
) -> PyResult<Option<nautilus_core::Params>> {
    match params {
        Some(dict) => from_pydict(py, dict),
        None => Ok(None),
    }
}

/// Python-facing wrapper for DataActor.
///
/// This wrapper holds shared ownership of `PyDataActorInner` via `Rc<UnsafeCell<>>`.
/// Both Python (through this wrapper) and the global registries share the same
/// underlying actor instance, ensuring mutations are visible from both sides.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.common",
    name = "DataActor",
    unsendable,
    subclass
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")]
pub struct PyDataActor {
    inner: Rc<UnsafeCell<PyDataActorInner>>,
}

impl Debug for PyDataActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyDataActor))
            .field("inner", &self.inner())
            .finish()
    }
}

impl PyDataActor {
    /// Returns a reference to the inner actor state.
    ///
    /// # Safety
    ///
    /// This is safe for single-threaded use. The `UnsafeCell` allows interior
    /// mutability which is required for the registries to mutate the actor.
    #[inline]
    #[allow(unsafe_code)]
    pub(crate) fn inner(&self) -> &PyDataActorInner {
        unsafe { &*self.inner.get() }
    }

    /// Returns a mutable reference to the inner actor state.
    ///
    /// # Safety
    ///
    /// This is safe for single-threaded use. Callers must ensure no aliasing
    /// mutable references exist.
    #[inline]
    #[allow(unsafe_code, clippy::mut_from_ref)]
    pub(crate) fn inner_mut(&self) -> &mut PyDataActorInner {
        unsafe { &mut *self.inner.get() }
    }
}

impl Deref for PyDataActor {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.inner().core
    }
}

impl DerefMut for PyDataActor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner_mut().core
    }
}

impl PyDataActor {
    // Rust constructor for tests and direct Rust usage
    pub fn new(config: Option<DataActorConfig>) -> Self {
        let config = config.unwrap_or_default();
        let core = DataActorCore::new(config);
        let clock = PyClock::new_test(); // Temporary clock, will be updated on registration
        let logger = PyLogger::new(core.actor_id().as_str());

        let inner = PyDataActorInner {
            core,
            py_self: None,
            clock,
            logger,
        };

        Self {
            inner: Rc::new(UnsafeCell::new(inner)),
        }
    }

    /// Sets the Python instance reference for method dispatch.
    ///
    /// This enables the PyDataActor to forward method calls (like `on_start`, `on_stop`)
    /// to the original Python instance that contains this PyDataActor. This is essential
    /// for Python inheritance to work correctly, allowing Python subclasses to override
    /// DataActor methods and have them called by the Rust system.
    pub fn set_python_instance(&mut self, py_obj: Py<PyAny>) {
        self.inner_mut().py_self = Some(py_obj);
    }

    /// Updates the actor_id in both the core config and the actor_id field.
    ///
    /// This method is only exposed for the Python actor to assist with configuration and should
    /// **never** be called post registration. Calling this after registration will cause
    /// inconsistent state where the actor is registered under one ID but its internal actor_id
    /// field contains another, breaking message routing and lifecycle management.
    pub fn set_actor_id(&mut self, actor_id: ActorId) {
        let inner = self.inner_mut();
        inner.core.config.actor_id = Some(actor_id);
        inner.core.actor_id = actor_id;
    }

    /// Updates the log_events setting in the core config.
    pub fn set_log_events(&mut self, log_events: bool) {
        self.inner_mut().core.config.log_events = log_events;
    }

    /// Updates the log_commands setting in the core config.
    pub fn set_log_commands(&mut self, log_commands: bool) {
        self.inner_mut().core.config.log_commands = log_commands;
    }

    /// Returns the memory address of this instance as a hexadecimal string.
    pub fn mem_address(&self) -> String {
        self.inner().core.mem_address()
    }

    /// Returns a value indicating whether the actor has been registered with a trader.
    pub fn is_registered(&self) -> bool {
        self.inner().core.is_registered()
    }

    /// Register the actor with a trader.
    ///
    /// # Errors
    ///
    /// Returns an error if the actor is already registered or if the registration process fails.
    pub fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()> {
        let inner = self.inner_mut();
        inner.core.register(trader_id, clock, cache)?;

        inner.clock = PyClock::from_rc(inner.core.clock_rc());

        // Register default time event handler for this actor
        let actor_id = inner.actor_id().inner();
        let callback = TimeEventCallback::from(move |event: TimeEvent| {
            if let Some(mut actor) = try_get_actor_unchecked::<PyDataActorInner>(&actor_id) {
                if let Err(e) = actor.on_time_event(&event) {
                    log::error!("Python time event handler failed for actor {actor_id}: {e}");
                }
            } else {
                log::error!("Actor {actor_id} not found for time event handling");
            }
        });

        inner.clock.inner_mut().register_default_handler(callback);

        inner.initialize()
    }

    /// Registers this actor in the global component and actor registries.
    ///
    /// Clones the internal `Rc` and inserts into both registries. This ensures
    /// Python and the registries share the exact same actor instance.
    pub fn register_in_global_registries(&self) {
        let inner = self.inner();
        let component_id = inner.component_id().inner();
        let actor_id = Actor::id(inner);

        let inner_ref: Rc<UnsafeCell<PyDataActorInner>> = self.inner.clone();

        let component_trait_ref: Rc<UnsafeCell<dyn Component>> = inner_ref.clone();
        get_component_registry().insert(component_id, component_trait_ref);

        let actor_trait_ref: Rc<UnsafeCell<dyn Actor>> = inner_ref;
        get_actor_registry().insert(actor_id, actor_trait_ref);
    }
}

impl DataActor for PyDataActorInner {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_start()
            .map_err(|e| anyhow::anyhow!("Python on_start failed: {e}"))
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_stop()
            .map_err(|e| anyhow::anyhow!("Python on_stop failed: {e}"))
    }

    fn on_resume(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_resume()
            .map_err(|e| anyhow::anyhow!("Python on_resume failed: {e}"))
    }

    fn on_reset(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_reset()
            .map_err(|e| anyhow::anyhow!("Python on_reset failed: {e}"))
    }

    fn on_dispose(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_dispose()
            .map_err(|e| anyhow::anyhow!("Python on_dispose failed: {e}"))
    }

    fn on_degrade(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_degrade()
            .map_err(|e| anyhow::anyhow!("Python on_degrade failed: {e}"))
    }

    fn on_fault(&mut self) -> anyhow::Result<()> {
        self.dispatch_on_fault()
            .map_err(|e| anyhow::anyhow!("Python on_fault failed: {e}"))
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        self.dispatch_on_time_event(event.clone())
            .map_err(|e| anyhow::anyhow!("Python on_time_event failed: {e}"))
    }

    #[allow(unused_variables)]
    fn on_data(&mut self, data: &CustomData) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_data: Py<PyAny> = Py::new(py, data.clone())?.into_any();
            self.dispatch_on_data(py_data)
                .map_err(|e| anyhow::anyhow!("Python on_data failed: {e}"))
        })
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        self.dispatch_on_signal(signal)
            .map_err(|e| anyhow::anyhow!("Python on_signal failed: {e}"))
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_instrument = instrument_any_to_pyobject(py, instrument.clone())
                .map_err(|e| anyhow::anyhow!("Failed to convert InstrumentAny to Python: {e}"))?;
            self.dispatch_on_instrument(py_instrument)
                .map_err(|e| anyhow::anyhow!("Python on_instrument failed: {e}"))
        })
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.dispatch_on_quote(*quote)
            .map_err(|e| anyhow::anyhow!("Python on_quote failed: {e}"))
    }

    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        self.dispatch_on_trade(*tick)
            .map_err(|e| anyhow::anyhow!("Python on_trade failed: {e}"))
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        self.dispatch_on_bar(*bar)
            .map_err(|e| anyhow::anyhow!("Python on_bar failed: {e}"))
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        self.dispatch_on_book_deltas(deltas.clone())
            .map_err(|e| anyhow::anyhow!("Python on_book_deltas failed: {e}"))
    }

    fn on_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()> {
        self.dispatch_on_book(order_book)
            .map_err(|e| anyhow::anyhow!("Python on_book failed: {e}"))
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        self.dispatch_on_mark_price(*mark_price)
            .map_err(|e| anyhow::anyhow!("Python on_mark_price failed: {e}"))
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        self.dispatch_on_index_price(*index_price)
            .map_err(|e| anyhow::anyhow!("Python on_index_price failed: {e}"))
    }

    fn on_funding_rate(&mut self, funding_rate: &FundingRateUpdate) -> anyhow::Result<()> {
        self.dispatch_on_funding_rate(*funding_rate)
            .map_err(|e| anyhow::anyhow!("Python on_funding_rate failed: {e}"))
    }

    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        self.dispatch_on_instrument_status(*data)
            .map_err(|e| anyhow::anyhow!("Python on_instrument_status failed: {e}"))
    }

    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        self.dispatch_on_instrument_close(*update)
            .map_err(|e| anyhow::anyhow!("Python on_instrument_close failed: {e}"))
    }

    fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
        self.dispatch_on_option_greeks(*greeks)
            .map_err(|e| anyhow::anyhow!("Python on_option_greeks failed: {e}"))
    }

    fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
        self.dispatch_on_option_chain(slice.clone())
            .map_err(|e| anyhow::anyhow!("Python on_option_chain failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_block(&mut self, block: &Block) -> anyhow::Result<()> {
        self.dispatch_on_block(block.clone())
            .map_err(|e| anyhow::anyhow!("Python on_block failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool(&mut self, pool: &Pool) -> anyhow::Result<()> {
        self.dispatch_on_pool(pool.clone())
            .map_err(|e| anyhow::anyhow!("Python on_pool failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
        self.dispatch_on_pool_swap(swap.clone())
            .map_err(|e| anyhow::anyhow!("Python on_pool_swap failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool_liquidity_update(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
        self.dispatch_on_pool_liquidity_update(update.clone())
            .map_err(|e| anyhow::anyhow!("Python on_pool_liquidity_update failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool_fee_collect(&mut self, collect: &PoolFeeCollect) -> anyhow::Result<()> {
        self.dispatch_on_pool_fee_collect(collect.clone())
            .map_err(|e| anyhow::anyhow!("Python on_pool_fee_collect failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool_flash(&mut self, flash: &PoolFlash) -> anyhow::Result<()> {
        self.dispatch_on_pool_flash(flash.clone())
            .map_err(|e| anyhow::anyhow!("Python on_pool_flash failed: {e}"))
    }

    fn on_historical_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_data: Py<PyAny> = if let Some(custom_data) = data.downcast_ref::<CustomData>() {
                Py::new(py, custom_data.clone())?.into_any()
            } else {
                anyhow::bail!("Failed to convert historical data to Python: unsupported type");
            };
            self.dispatch_on_historical_data(py_data)
                .map_err(|e| anyhow::anyhow!("Python on_historical_data failed: {e}"))
        })
    }

    fn on_historical_quotes(&mut self, quotes: &[QuoteTick]) -> anyhow::Result<()> {
        self.dispatch_on_historical_quotes(quotes.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_quotes failed: {e}"))
    }

    fn on_historical_trades(&mut self, trades: &[TradeTick]) -> anyhow::Result<()> {
        self.dispatch_on_historical_trades(trades.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_trades failed: {e}"))
    }

    fn on_historical_funding_rates(
        &mut self,
        funding_rates: &[FundingRateUpdate],
    ) -> anyhow::Result<()> {
        self.dispatch_on_historical_funding_rates(funding_rates.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_funding_rates failed: {e}"))
    }

    fn on_historical_bars(&mut self, bars: &[Bar]) -> anyhow::Result<()> {
        self.dispatch_on_historical_bars(bars.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_bars failed: {e}"))
    }

    fn on_historical_mark_prices(&mut self, mark_prices: &[MarkPriceUpdate]) -> anyhow::Result<()> {
        self.dispatch_on_historical_mark_prices(mark_prices.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_mark_prices failed: {e}"))
    }

    fn on_historical_index_prices(
        &mut self,
        index_prices: &[IndexPriceUpdate],
    ) -> anyhow::Result<()> {
        self.dispatch_on_historical_index_prices(index_prices.to_vec())
            .map_err(|e| anyhow::anyhow!("Python on_historical_index_prices failed: {e}"))
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyDataActor {
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<DataActorConfig>) -> Self {
        Self::new(config)
    }

    #[pyo3(signature = (config=None))]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn __init__(slf: &Bound<'_, Self>, config: Option<DataActorConfig>) {
        let py_self: Py<PyAny> = slf.clone().unbind().into_any();
        slf.borrow_mut().set_python_instance(py_self);
    }

    #[getter]
    #[pyo3(name = "clock")]
    fn py_clock(&self) -> PyResult<PyClock> {
        let inner = self.inner();
        if inner.core.is_registered() {
            Ok(inner.clock.clone())
        } else {
            Err(to_pyruntime_err(
                "Actor must be registered with a trader before accessing clock",
            ))
        }
    }

    #[getter]
    #[pyo3(name = "cache")]
    fn py_cache(&self) -> PyResult<PyCache> {
        let inner = self.inner();
        if inner.core.is_registered() {
            Ok(PyCache::from_rc(inner.core.cache_rc()))
        } else {
            Err(to_pyruntime_err(
                "Actor must be registered with a trader before accessing cache",
            ))
        }
    }

    #[getter]
    #[pyo3(name = "log")]
    fn py_log(&self) -> PyLogger {
        self.inner().logger.clone()
    }

    #[getter]
    #[pyo3(name = "actor_id")]
    fn py_actor_id(&self) -> ActorId {
        self.inner().core.actor_id
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> Option<TraderId> {
        self.inner().core.trader_id()
    }

    #[pyo3(name = "state")]
    fn py_state(&self) -> ComponentState {
        Component::state(self.inner())
    }

    #[pyo3(name = "is_ready")]
    fn py_is_ready(&self) -> bool {
        Component::is_ready(self.inner())
    }

    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        Component::is_running(self.inner())
    }

    #[pyo3(name = "is_stopped")]
    fn py_is_stopped(&self) -> bool {
        Component::is_stopped(self.inner())
    }

    #[pyo3(name = "is_degraded")]
    fn py_is_degraded(&self) -> bool {
        Component::is_degraded(self.inner())
    }

    #[pyo3(name = "is_faulted")]
    fn py_is_faulted(&self) -> bool {
        Component::is_faulted(self.inner())
    }

    #[pyo3(name = "is_disposed")]
    fn py_is_disposed(&self) -> bool {
        Component::is_disposed(self.inner())
    }

    #[pyo3(name = "start")]
    fn py_start(&mut self) -> PyResult<()> {
        Component::start(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "stop")]
    fn py_stop(&mut self) -> PyResult<()> {
        Component::stop(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "resume")]
    fn py_resume(&mut self) -> PyResult<()> {
        Component::resume(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) -> PyResult<()> {
        Component::reset(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) -> PyResult<()> {
        Component::dispose(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "degrade")]
    fn py_degrade(&mut self) -> PyResult<()> {
        Component::degrade(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "fault")]
    fn py_fault(&mut self) -> PyResult<()> {
        Component::fault(self.inner_mut()).map_err(to_pyruntime_err)
    }

    #[pyo3(name = "shutdown_system")]
    #[pyo3(signature = (reason=None))]
    fn py_shutdown_system(&self, reason: Option<String>) {
        self.inner().core.shutdown_system(reason);
    }

    #[pyo3(name = "publish_data")]
    fn py_publish_data(&self, data_type: &DataType, data: &CustomData) {
        self.inner().core.publish_data(data_type, data);
    }

    #[pyo3(name = "publish_signal")]
    #[pyo3(signature = (name, value, ts_event=0))]
    #[allow(clippy::needless_pass_by_value)]
    fn py_publish_signal(
        &self,
        py: Python<'_>,
        name: &str,
        value: Py<PyAny>,
        ts_event: u64,
    ) -> PyResult<()> {
        // Accept any int / float / str / bool — match v1 behaviour by coercing with `str(value)`.
        let value_str: String = value.bind(py).str()?.extract()?;
        self.inner()
            .core
            .publish_signal(name, value_str, UnixNanos::from(ts_event));
        Ok(())
    }

    #[pyo3(name = "add_synthetic")]
    fn py_add_synthetic(&self, synthetic: SyntheticInstrument) -> PyResult<()> {
        self.inner()
            .core
            .add_synthetic(synthetic)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "update_synthetic")]
    fn py_update_synthetic(&self, synthetic: SyntheticInstrument) -> PyResult<()> {
        self.inner()
            .core
            .update_synthetic(synthetic)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "on_start")]
    fn py_on_start(&self) {}

    #[pyo3(name = "on_stop")]
    fn py_on_stop(&mut self) {}

    #[pyo3(name = "on_resume")]
    fn py_on_resume(&mut self) {}

    #[pyo3(name = "on_reset")]
    fn py_on_reset(&mut self) {}

    #[pyo3(name = "on_dispose")]
    fn py_on_dispose(&mut self) {}

    #[pyo3(name = "on_degrade")]
    fn py_on_degrade(&mut self) {}

    #[pyo3(name = "on_fault")]
    fn py_on_fault(&mut self) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_time_event")]
    fn py_on_time_event(&mut self, event: TimeEvent) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_data")]
    fn py_on_data(&mut self, data: Py<PyAny>) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_signal")]
    fn py_on_signal(&mut self, signal: &Signal) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_instrument")]
    fn py_on_instrument(&mut self, instrument: Py<PyAny>) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_quote")]
    fn py_on_quote(&mut self, quote: QuoteTick) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_trade")]
    fn py_on_trade(&mut self, trade: TradeTick) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_bar")]
    fn py_on_bar(&mut self, bar: Bar) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_book_deltas")]
    fn py_on_book_deltas(&mut self, deltas: OrderBookDeltas) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_book")]
    fn py_on_book(&mut self, book: &OrderBook) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_mark_price")]
    fn py_on_mark_price(&mut self, mark_price: MarkPriceUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_index_price")]
    fn py_on_index_price(&mut self, index_price: IndexPriceUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_funding_rate")]
    fn py_on_funding_rate(&mut self, funding_rate: FundingRateUpdate) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_instrument_status")]
    fn py_on_instrument_status(&mut self, status: InstrumentStatus) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_instrument_close")]
    fn py_on_instrument_close(&mut self, close: InstrumentClose) {}

    #[allow(unused_variables)]
    #[pyo3(name = "on_option_greeks")]
    fn py_on_option_greeks(&mut self, greeks: OptionGreeks) {}

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_option_chain")]
    fn py_on_option_chain(&mut self, slice: OptionChainSlice) {}

    #[pyo3(name = "subscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_subscribe_data(
        &mut self,
        py: Python<'_>,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_data(self.inner_mut(), data_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_signal")]
    #[pyo3(signature = (name=""))]
    fn py_subscribe_signal(&mut self, name: &str) {
        DataActor::subscribe_signal(self.inner_mut(), name);
    }

    #[pyo3(name = "subscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_subscribe_instruments(
        &mut self,
        py: Python<'_>,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_instruments(self.inner_mut(), venue, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_instrument(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_book_deltas")]
    #[pyo3(signature = (instrument_id, book_type, depth=None, client_id=None, managed=false, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_subscribe_book_deltas(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        managed: bool,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        let depth = depth.and_then(NonZeroUsize::new);
        DataActor::subscribe_book_deltas(
            self.inner_mut(),
            instrument_id,
            book_type,
            depth,
            client_id,
            managed,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_book_at_interval")]
    #[pyo3(signature = (instrument_id, book_type, interval_ms, depth=None, client_id=None, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_subscribe_book_at_interval(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        book_type: BookType,
        interval_ms: usize,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        let depth = depth.and_then(NonZeroUsize::new);
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| to_pyvalue_err("interval_ms must be > 0"))?;

        DataActor::subscribe_book_at_interval(
            self.inner_mut(),
            instrument_id,
            book_type,
            depth,
            interval_ms,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_quotes")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_quotes(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_quotes(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_trades(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_trades(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_subscribe_bars(
        &mut self,
        py: Python<'_>,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_bars(self.inner_mut(), bar_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_mark_prices(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_index_prices(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_index_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_funding_rates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_funding_rates(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_funding_rates(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_option_greeks")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_option_greeks(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_option_greeks(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_status(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_instrument_status(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_close")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_close(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_instrument_close(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_option_chain")]
    #[pyo3(signature = (series_id, strike_range, snapshot_interval_ms=None, client_id=None, params=None))]
    fn py_subscribe_option_chain(
        &mut self,
        py: Python<'_>,
        series_id: OptionSeriesId,
        strike_range: PyStrikeRange,
        snapshot_interval_ms: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_option_chain(
            self.inner_mut(),
            series_id,
            strike_range.inner,
            snapshot_interval_ms,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_order_fills")]
    #[pyo3(signature = (instrument_id))]
    fn py_subscribe_order_fills(&mut self, instrument_id: InstrumentId) {
        DataActor::subscribe_order_fills(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "subscribe_order_cancels")]
    #[pyo3(signature = (instrument_id))]
    fn py_subscribe_order_cancels(&mut self, instrument_id: InstrumentId) {
        DataActor::subscribe_order_cancels(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "unsubscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_unsubscribe_data(
        &mut self,
        py: Python<'_>,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_data(self.inner_mut(), data_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_signal")]
    #[pyo3(signature = (name=""))]
    fn py_unsubscribe_signal(&mut self, name: &str) {
        DataActor::unsubscribe_signal(self.inner_mut(), name);
    }

    #[pyo3(name = "unsubscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_unsubscribe_instruments(
        &mut self,
        py: Python<'_>,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_instruments(self.inner_mut(), venue, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_instrument(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_book_deltas(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_book_deltas(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_at_interval")]
    #[pyo3(signature = (instrument_id, interval_ms, client_id=None, params=None))]
    fn py_unsubscribe_book_at_interval(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        interval_ms: usize,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| to_pyvalue_err("interval_ms must be > 0"))?;

        DataActor::unsubscribe_book_at_interval(
            self.inner_mut(),
            instrument_id,
            interval_ms,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_quotes")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_quotes(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_quotes(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_trades(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_trades(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_unsubscribe_bars(
        &mut self,
        py: Python<'_>,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_bars(self.inner_mut(), bar_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_mark_prices(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_index_prices(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_index_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_funding_rates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_funding_rates(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_funding_rates(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_option_greeks")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_option_greeks(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_option_greeks(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument_status(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_instrument_status(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument_close")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument_close(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_instrument_close(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_option_chain")]
    #[pyo3(signature = (series_id, client_id=None))]
    fn py_unsubscribe_option_chain(
        &mut self,
        series_id: OptionSeriesId,
        client_id: Option<ClientId>,
    ) {
        DataActor::unsubscribe_option_chain(self.inner_mut(), series_id, client_id);
    }

    #[pyo3(name = "unsubscribe_order_fills")]
    #[pyo3(signature = (instrument_id))]
    fn py_unsubscribe_order_fills(&mut self, instrument_id: InstrumentId) {
        DataActor::unsubscribe_order_fills(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "unsubscribe_order_cancels")]
    #[pyo3(signature = (instrument_id))]
    fn py_unsubscribe_order_cancels(&mut self, instrument_id: InstrumentId) {
        DataActor::unsubscribe_order_cancels(self.inner_mut(), instrument_id);
    }

    #[pyo3(name = "request_data")]
    #[pyo3(signature = (data_type, client_id, start=None, end=None, limit=None, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_data(
        &mut self,
        py: Python<'_>,
        data_type: DataType,
        client_id: ClientId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_data(
            self.inner_mut(),
            data_type,
            client_id,
            start,
            end,
            limit,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_instrument")]
    #[pyo3(signature = (instrument_id, start=None, end=None, client_id=None, params=None))]
    fn py_request_instrument(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_instrument(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (venue=None, start=None, end=None, client_id=None, params=None))]
    fn py_request_instruments(
        &mut self,
        py: Python<'_>,
        venue: Option<Venue>,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id =
            DataActor::request_instruments(self.inner_mut(), venue, start, end, client_id, params)
                .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_book_snapshot")]
    #[pyo3(signature = (instrument_id, depth=None, client_id=None, params=None))]
    fn py_request_book_snapshot(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let depth = depth.and_then(NonZeroUsize::new);

        let request_id = DataActor::request_book_snapshot(
            self.inner_mut(),
            instrument_id,
            depth,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_quotes")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_quotes(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_quotes(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_trades(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_trades(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_funding_rates")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_funding_rates(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_funding_rates(
            self.inner_mut(),
            instrument_id,
            start,
            end,
            limit,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, client_id=None, params=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_bars(
        &mut self,
        py: Python<'_>,
        bar_type: BarType,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<String> {
        let params = dict_to_params(py, params)?;
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(|ts| UnixNanos::from(ts).to_datetime_utc());
        let end = end.map(|ts| UnixNanos::from(ts).to_datetime_utc());

        let request_id = DataActor::request_bars(
            self.inner_mut(),
            bar_type,
            start,
            end,
            limit,
            client_id,
            params,
        )
        .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_data")]
    fn py_on_historical_data(&mut self, data: Py<PyAny>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_quotes")]
    fn py_on_historical_quotes(&mut self, quotes: Vec<QuoteTick>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_trades")]
    fn py_on_historical_trades(&mut self, trades: Vec<TradeTick>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_funding_rates")]
    fn py_on_historical_funding_rates(&mut self, funding_rates: Vec<FundingRateUpdate>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_bars")]
    fn py_on_historical_bars(&mut self, bars: Vec<Bar>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_mark_prices")]
    fn py_on_historical_mark_prices(&mut self, mark_prices: Vec<MarkPriceUpdate>) {
        // Default implementation - can be overridden in Python subclasses
    }

    #[allow(unused_variables, clippy::needless_pass_by_value)]
    #[pyo3(name = "on_historical_index_prices")]
    fn py_on_historical_index_prices(&mut self, index_prices: Vec<IndexPriceUpdate>) {
        // Default implementation - can be overridden in Python subclasses
    }
}

#[cfg(feature = "defi")]
#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyDataActor {
    #[pyo3(name = "on_block")]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn py_on_block(&mut self, block: Block) {}

    #[pyo3(name = "on_pool")]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn py_on_pool(&mut self, pool: Pool) {}

    #[pyo3(name = "on_pool_swap")]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn py_on_pool_swap(&mut self, swap: PoolSwap) {}

    #[pyo3(name = "on_pool_liquidity_update")]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn py_on_pool_liquidity_update(&mut self, update: PoolLiquidityUpdate) {}

    #[pyo3(name = "on_pool_fee_collect")]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn py_on_pool_fee_collect(&mut self, update: PoolFeeCollect) {}

    #[pyo3(name = "on_pool_flash")]
    #[allow(unused_variables, clippy::needless_pass_by_value)]
    fn py_on_pool_flash(&mut self, flash: PoolFlash) {}

    #[pyo3(name = "subscribe_blocks")]
    #[pyo3(signature = (chain, client_id=None, params=None))]
    fn py_subscribe_blocks(
        &mut self,
        py: Python<'_>,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_blocks(self.inner_mut(), chain, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_pool")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_pool(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_pool_swaps")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_swaps(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_pool_swaps(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_pool_liquidity_updates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_liquidity_updates(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_pool_liquidity_updates(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "subscribe_pool_fee_collects")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_fee_collects(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_pool_fee_collects(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_pool_flash_events")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_flash_events(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::subscribe_pool_flash_events(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_blocks")]
    #[pyo3(signature = (chain, client_id=None, params=None))]
    fn py_unsubscribe_blocks(
        &mut self,
        py: Python<'_>,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_blocks(self.inner_mut(), chain, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_pool")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_pool(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_pool_swaps")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_swaps(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_pool_swaps(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_pool_liquidity_updates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_liquidity_updates(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_pool_liquidity_updates(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_pool_fee_collects")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_fee_collects(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_pool_fee_collects(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[pyo3(name = "unsubscribe_pool_flash_events")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_flash_events(
        &mut self,
        py: Python<'_>,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<Py<PyDict>>,
    ) -> PyResult<()> {
        let params = dict_to_params(py, params)?;
        DataActor::unsubscribe_pool_flash_events(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, str::FromStr, sync::Arc};

    #[cfg(feature = "defi")]
    use alloy_primitives::{I256, U160, U256};
    use nautilus_core::{UUID4, UnixNanos, python::IntoPyObjectNautilusExt};
    #[cfg(feature = "defi")]
    use nautilus_model::defi::{
        AmmType, Block, Blockchain, Chain, Dex, DexType, Pool, PoolFeeCollect, PoolFlash,
        PoolIdentifier, PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap, Token,
    };
    use nautilus_model::{
        data::{
            Bar, BarType, CustomData, DataType, FundingRateUpdate, IndexPriceUpdate,
            InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, QuoteTick,
            TradeTick,
            close::InstrumentClose,
            greeks::OptionGreekValues,
            option_chain::{OptionChainSlice, OptionGreeks},
            stubs::stub_custom_data,
        },
        enums::{
            AggressorSide, BookType, GreeksConvention, InstrumentCloseType, MarketStatusAction,
        },
        identifiers::{ClientId, OptionSeriesId, TradeId, TraderId, Venue},
        instruments::{CurrencyPair, InstrumentAny, stubs::audusd_sim},
        orderbook::OrderBook,
        types::{Price, Quantity},
    };
    use pyo3::{Py, PyAny, PyResult, Python, ffi::c_str, types::PyAnyMethods};
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::PyDataActor;
    use crate::{
        actor::DataActor,
        cache::Cache,
        clock::TestClock,
        enums::ComponentState,
        runner::{SyncDataCommandSender, set_data_cmd_sender},
        signal::Signal,
        timer::TimeEvent,
    };

    #[fixture]
    fn clock() -> Rc<RefCell<TestClock>> {
        Rc::new(RefCell::new(TestClock::new()))
    }

    #[fixture]
    fn cache() -> Rc<RefCell<Cache>> {
        Rc::new(RefCell::new(Cache::new(None, None)))
    }

    #[fixture]
    fn trader_id() -> TraderId {
        TraderId::from("TRADER-001")
    }

    #[fixture]
    fn client_id() -> ClientId {
        ClientId::new("TestClient")
    }

    #[fixture]
    fn venue() -> Venue {
        Venue::from("SIM")
    }

    #[fixture]
    fn data_type() -> DataType {
        DataType::new("TestData", None, None)
    }

    #[fixture]
    fn bar_type(audusd_sim: CurrencyPair) -> BarType {
        BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap()
    }

    fn create_unregistered_actor() -> PyDataActor {
        PyDataActor::new(None)
    }

    fn create_registered_actor(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) -> PyDataActor {
        // Set up sync data command sender for tests
        let sender = SyncDataCommandSender;
        set_data_cmd_sender(Arc::new(sender));

        let mut actor = PyDataActor::new(None);
        actor.register(trader_id, clock, cache).unwrap();
        actor
    }

    #[rstest]
    fn test_new_actor_creation() {
        let actor = PyDataActor::new(None);
        assert!(actor.trader_id().is_none());
    }

    #[rstest]
    fn test_clock_access_before_registration_raises_error() {
        let actor = PyDataActor::new(None);

        // Accessing clock before registration should raise PyRuntimeError
        let result = actor.py_clock();
        assert!(result.is_err());

        let error = result.unwrap_err();
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            assert!(error.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py));
        });

        let error_msg = error.to_string();
        assert!(
            error_msg.contains("Actor must be registered with a trader before accessing clock")
        );
    }

    #[rstest]
    fn test_unregistered_actor_methods_work() {
        let actor = create_unregistered_actor();

        assert!(!actor.py_is_ready());
        assert!(!actor.py_is_running());
        assert!(!actor.py_is_stopped());
        assert!(!actor.py_is_disposed());
        assert!(!actor.py_is_degraded());
        assert!(!actor.py_is_faulted());

        // Verify unregistered state
        assert_eq!(actor.trader_id(), None);
    }

    #[rstest]
    fn test_registration_success(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        let mut actor = create_unregistered_actor();
        actor.register(trader_id, clock, cache).unwrap();
        assert!(actor.trader_id().is_some());
        assert_eq!(actor.trader_id().unwrap(), trader_id);
    }

    #[rstest]
    fn test_registered_actor_basic_properties(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        let actor = create_registered_actor(clock, cache, trader_id);

        assert_eq!(actor.state(), ComponentState::Ready);
        assert_eq!(actor.trader_id(), Some(TraderId::from("TRADER-001")));
        assert!(actor.py_is_ready());
        assert!(!actor.py_is_running());
        assert!(!actor.py_is_stopped());
        assert!(!actor.py_is_disposed());
        assert!(!actor.py_is_degraded());
        assert!(!actor.py_is_faulted());
    }

    #[rstest]
    fn test_basic_subscription_methods_compile(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        data_type: DataType,
        client_id: ClientId,
        audusd_sim: CurrencyPair,
    ) {
        let mut actor = create_registered_actor(clock, cache, trader_id);

        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            assert!(
                actor
                    .py_subscribe_data(py, data_type.clone(), Some(client_id), None)
                    .is_ok()
            );
            assert!(
                actor
                    .py_subscribe_quotes(py, audusd_sim.id, Some(client_id), None)
                    .is_ok()
            );
            assert!(
                actor
                    .py_unsubscribe_data(py, data_type, Some(client_id), None)
                    .is_ok()
            );
            assert!(
                actor
                    .py_unsubscribe_quotes(py, audusd_sim.id, Some(client_id), None)
                    .is_ok()
            );
        });
    }

    #[rstest]
    fn test_shutdown_system_passes_through(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        let actor = create_registered_actor(clock, cache, trader_id);

        actor.py_shutdown_system(Some("Test shutdown".to_string()));
        actor.py_shutdown_system(None);
    }

    #[rstest]
    fn test_publish_data_delivers_to_any_subscriber(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::msgbus::{
            self, MessageBus, get_message_bus, switchboard::get_custom_topic,
            typed_handler::ShareableMessageHandler,
        };

        // Ensure clean msgbus for this test
        *get_message_bus().borrow_mut() = MessageBus::default();

        let actor = create_registered_actor(clock, cache, trader_id);
        let data = stub_custom_data(1, 42, None, None);
        let topic = get_custom_topic(&data.data_type);

        let received: Rc<RefCell<Vec<CustomData>>> = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();
        let handler = ShareableMessageHandler::from_typed(move |d: &CustomData| {
            received_clone.borrow_mut().push(d.clone());
        });
        msgbus::subscribe_any(topic.into(), handler, None);

        actor.py_publish_data(&data.data_type, &data);

        let received = received.borrow();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].data_type, data.data_type);
    }

    #[rstest]
    fn test_publish_signal_delivers_to_customdata_subscriber(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::{
            msgbus::{
                self, MessageBus, Pattern, get_message_bus, typed_handler::ShareableMessageHandler,
            },
            signal::Signal,
        };

        *get_message_bus().borrow_mut() = MessageBus::default();

        let actor = create_registered_actor(clock, cache, trader_id);

        // Signals travel as `CustomData` on the bus so persistence and other
        // `CustomData`-aware subscribers pick them up. Downcast inside the handler.
        let received: Rc<RefCell<Vec<Signal>>> = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();
        let handler = ShareableMessageHandler::from_typed(move |data: &CustomData| {
            if let Some(sig) = data.data.as_any().downcast_ref::<Signal>() {
                received_clone.borrow_mut().push(sig.clone());
            }
        });
        let pattern: crate::msgbus::MStr<Pattern> = "data.Signal*".to_string().into();
        msgbus::subscribe_any(pattern, handler, None);

        pyo3::Python::initialize();
        Python::attach(|py| {
            let val1: Py<PyAny> = 1.0_f64.into_py_any_unwrap(py);
            let val2: Py<PyAny> = "HIGH".into_py_any_unwrap(py);
            actor.py_publish_signal(py, "example", val1, 0).unwrap();
            actor
                .py_publish_signal(py, "risk", val2, 1_700_000_000_000_000_000)
                .unwrap();
        });

        let received = received.borrow();
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].name.as_str(), "example");
        assert_eq!(received[0].value, "1.0");
        assert_eq!(received[1].name.as_str(), "risk");
        assert_eq!(received[1].value, "HIGH");
        assert_eq!(
            received[1].ts_event,
            UnixNanos::from(1_700_000_000_000_000_000_u64)
        );
    }

    #[rstest]
    fn test_publish_signal_accepts_numeric_py_values(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::{
            msgbus::{
                self, MessageBus, Pattern, get_message_bus, typed_handler::ShareableMessageHandler,
            },
            signal::Signal,
        };

        *get_message_bus().borrow_mut() = MessageBus::default();

        let actor = create_registered_actor(clock, cache, trader_id);

        let received: Rc<RefCell<Vec<Signal>>> = Rc::new(RefCell::new(Vec::new()));
        let received_clone = received.clone();
        let handler = ShareableMessageHandler::from_typed(move |data: &CustomData| {
            if let Some(sig) = data.data.as_any().downcast_ref::<Signal>() {
                received_clone.borrow_mut().push(sig.clone());
            }
        });
        let pattern: crate::msgbus::MStr<Pattern> = "data.Signal*".to_string().into();
        msgbus::subscribe_any(pattern, handler, None);

        pyo3::Python::initialize();
        Python::attach(|py| {
            let int_value: Py<PyAny> = 42_i64.into_py_any_unwrap(py);
            let float_value: Py<PyAny> = 3.5_f64.into_py_any_unwrap(py);
            let bool_value: Py<PyAny> = true.into_py_any_unwrap(py);
            actor.py_publish_signal(py, "count", int_value, 0).unwrap();
            actor
                .py_publish_signal(py, "ratio", float_value, 0)
                .unwrap();
            actor
                .py_publish_signal(py, "active", bool_value, 0)
                .unwrap();
        });

        let received = received.borrow();
        assert_eq!(received.len(), 3);
        assert_eq!(received[0].value, "42");
        assert_eq!(received[1].value, "3.5");
        assert_eq!(received[2].value, "True");
    }

    #[rstest]
    fn test_subscribe_and_unsubscribe_signal_compile(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::msgbus::{MessageBus, get_message_bus};

        *get_message_bus().borrow_mut() = MessageBus::default();

        let mut actor = create_registered_actor(clock, cache, trader_id);
        actor.py_subscribe_signal("example");
        actor.py_unsubscribe_signal("example");
        actor.py_subscribe_signal("");
        actor.py_unsubscribe_signal("");
    }

    #[rstest]
    fn test_publish_data_dispatches_to_python_on_data(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::msgbus::{MessageBus, get_message_bus};

        *get_message_bus().borrow_mut() = MessageBus::default();

        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();
            rust_actor.register_in_global_registries();
            rust_actor.py_start().unwrap();

            let data = stub_custom_data(1, 42, None, None);
            rust_actor
                .py_subscribe_data(py, data.data_type.clone(), None, None)
                .unwrap();

            rust_actor.py_publish_data(&data.data_type, &data);
            rust_actor.py_publish_data(&data.data_type, &data);

            assert!(python_method_was_called(&py_actor, py, "on_data"));
            assert_eq!(python_method_call_count(&py_actor, py, "on_data"), 2);
        });
    }

    #[rstest]
    fn test_publish_signal_dispatches_to_python_on_signal(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::msgbus::{MessageBus, get_message_bus};

        *get_message_bus().borrow_mut() = MessageBus::default();

        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();
            rust_actor.register_in_global_registries();
            rust_actor.py_start().unwrap();

            rust_actor.py_subscribe_signal("example");
            let val1: Py<PyAny> = "1.5".into_py_any_unwrap(py);
            let val2: Py<PyAny> = 2.0_f64.into_py_any_unwrap(py);
            rust_actor
                .py_publish_signal(py, "example", val1, 0)
                .unwrap();
            rust_actor
                .py_publish_signal(py, "example", val2, 1_700_000_000_000_000_000)
                .unwrap();

            assert!(python_method_was_called(&py_actor, py, "on_signal"));
            assert_eq!(python_method_call_count(&py_actor, py, "on_signal"), 2);
        });
    }

    #[rstest]
    fn test_unsubscribe_signal_stops_python_dispatch(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::msgbus::{MessageBus, get_message_bus};

        *get_message_bus().borrow_mut() = MessageBus::default();

        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();
            rust_actor.register_in_global_registries();
            rust_actor.py_start().unwrap();

            rust_actor.py_subscribe_signal("example");
            let val1: Py<PyAny> = "1".into_py_any_unwrap(py);
            let val2: Py<PyAny> = "2".into_py_any_unwrap(py);
            rust_actor
                .py_publish_signal(py, "example", val1, 0)
                .unwrap();

            rust_actor.py_unsubscribe_signal("example");
            rust_actor
                .py_publish_signal(py, "example", val2, 0)
                .unwrap();

            assert_eq!(python_method_call_count(&py_actor, py, "on_signal"), 1);
        });
    }

    #[rstest]
    fn test_subscribe_signal_wildcard_dispatches_all_names_to_python(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use crate::msgbus::{MessageBus, get_message_bus};

        *get_message_bus().borrow_mut() = MessageBus::default();

        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();
            rust_actor.register_in_global_registries();
            rust_actor.py_start().unwrap();

            rust_actor.py_subscribe_signal("");
            let val1: Py<PyAny> = "1".into_py_any_unwrap(py);
            let val2: Py<PyAny> = "2".into_py_any_unwrap(py);
            let val3: Py<PyAny> = "3".into_py_any_unwrap(py);
            rust_actor.py_publish_signal(py, "alpha", val1, 0).unwrap();
            rust_actor.py_publish_signal(py, "beta", val2, 0).unwrap();
            rust_actor.py_publish_signal(py, "gamma", val3, 0).unwrap();

            assert_eq!(python_method_call_count(&py_actor, py, "on_signal"), 3);
        });
    }

    #[rstest]
    fn test_signal_customdata_unwraps_to_python_signal(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        // Exercises the `Signal::to_pyobject` path: a `CustomData` wrapping a
        // `Signal` reaches Python `on_data`, and the PyO3 `.data` getter must
        // successfully unwrap the inner `Arc<dyn CustomDataTrait>` into a
        // Python `Signal`. Without `Signal::to_pyobject`, the getter raises
        // `TypeError` and this assertion fails.
        use crate::msgbus::{MessageBus, get_message_bus};

        *get_message_bus().borrow_mut() = MessageBus::default();

        pyo3::Python::initialize();
        Python::attach(|py| {
            let capture_code = c_str!(
                r#"
class CapturingActor:
    def __init__(self):
        self.captured = []

    def on_start(self): pass
    def on_stop(self): pass
    def on_resume(self): pass
    def on_reset(self): pass
    def on_dispose(self): pass
    def on_degrade(self): pass
    def on_fault(self): pass
    def on_signal(self, signal): pass

    def on_data(self, custom):
        # Exercise the CustomData.data getter: raises TypeError if the
        # inner payload cannot be converted back to a Python object.
        inner = custom.data
        self.captured.append((type(inner).__name__, inner.name, inner.value))
"#
            );
            py.run(capture_code, None, None).unwrap();
            let cls = py.eval(c_str!("CapturingActor"), None, None).unwrap();
            let py_actor: Py<PyAny> = cls.call0().unwrap().unbind();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();
            rust_actor.register_in_global_registries();
            rust_actor.py_start().unwrap();

            // Subscribe as custom-data for the signal's advertised DataType
            // (`data.SignalExample`) so `on_data` fires with the wrapping CustomData.
            let data_type = DataType::new("SignalExample", None, None);
            rust_actor
                .py_subscribe_data(py, data_type, None, None)
                .unwrap();

            let val: Py<PyAny> = "1.5".into_py_any_unwrap(py);
            rust_actor.py_publish_signal(py, "example", val, 0).unwrap();

            let captured = py_actor
                .bind(py)
                .getattr("captured")
                .unwrap()
                .extract::<Vec<(String, String, String)>>()
                .unwrap();
            assert_eq!(captured.len(), 1);
            assert_eq!(captured[0].0, "Signal");
            assert_eq!(captured[0].1, "example");
            assert_eq!(captured[0].2, "1.5");
        });
    }

    #[rstest]
    fn test_add_and_update_synthetic_via_pyo3(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        use nautilus_model::{
            identifiers::{InstrumentId, Symbol},
            instruments::SyntheticInstrument,
        };

        let actor = create_registered_actor(clock, cache.clone(), trader_id);

        let comp1 = InstrumentId::from_str("BTC-USD.VENUE").unwrap();
        let comp2 = InstrumentId::from_str("ETH-USD.VENUE").unwrap();
        let formula = format!("({comp1} + {comp2}) / 2.0");
        let synthetic = SyntheticInstrument::new(
            Symbol::from("SYN"),
            2,
            vec![comp1, comp2],
            &formula,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let synthetic_id = synthetic.id;

        actor.py_add_synthetic(synthetic.clone()).unwrap();
        assert!(cache.borrow().synthetic(&synthetic_id).is_some());

        // Adding again raises
        assert!(actor.py_add_synthetic(synthetic).is_err());

        let new_formula = format!("{comp1} + {comp2}");
        let updated = SyntheticInstrument::new(
            Symbol::from("SYN"),
            2,
            vec![comp1, comp2],
            &new_formula,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        actor.py_update_synthetic(updated).unwrap();
        assert_eq!(
            cache.borrow().synthetic(&synthetic_id).unwrap().formula,
            new_formula
        );

        // Updating a non-existent raises
        let missing = SyntheticInstrument::new(
            Symbol::from("GONE"),
            2,
            vec![comp1, comp2],
            &formula,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        assert!(actor.py_update_synthetic(missing).is_err());
    }

    #[rstest]
    fn test_book_at_interval_invalid_interval_ms(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut actor = create_registered_actor(clock, cache, trader_id);

        pyo3::Python::attach(|py| {
            let result = actor.py_subscribe_book_at_interval(
                py,
                audusd_sim.id,
                BookType::L2_MBP,
                0,
                None,
                None,
                None,
            );
            assert!(result.is_err());
            assert_eq!(
                result.unwrap_err().to_string(),
                "ValueError: interval_ms must be > 0"
            );

            let result = actor.py_unsubscribe_book_at_interval(py, audusd_sim.id, 0, None, None);
            assert!(result.is_err());
            assert_eq!(
                result.unwrap_err().to_string(),
                "ValueError: interval_ms must be > 0"
            );
        });
    }

    #[rstest]
    fn test_request_methods_signatures_exist() {
        let actor = create_unregistered_actor();
        assert!(actor.trader_id().is_none());
    }

    #[rstest]
    fn test_data_actor_trait_implementation(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        let actor = create_registered_actor(clock, cache, trader_id);
        let state = actor.state();
        assert_eq!(state, ComponentState::Ready);
    }

    fn sample_instrument() -> CurrencyPair {
        audusd_sim()
    }

    fn sample_data() -> CustomData {
        stub_custom_data(1, 42, None, None)
    }

    fn sample_time_event() -> TimeEvent {
        TimeEvent::new(
            Ustr::from("test_timer"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_signal() -> Signal {
        Signal::new(
            Ustr::from("test_signal"),
            "1.0".to_string(),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_quote() -> QuoteTick {
        let instrument = sample_instrument();
        QuoteTick::new(
            instrument.id,
            Price::from("1.00000"),
            Price::from("1.00001"),
            Quantity::from(100_000),
            Quantity::from(100_000),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_trade() -> TradeTick {
        let instrument = sample_instrument();
        TradeTick::new(
            instrument.id,
            Price::from("1.00000"),
            Quantity::from(100_000),
            AggressorSide::Buyer,
            TradeId::new("123456"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_bar() -> Bar {
        let instrument = sample_instrument();
        let bar_type =
            BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", instrument.id)).unwrap();
        Bar::new(
            bar_type,
            Price::from("1.00000"),
            Price::from("1.00010"),
            Price::from("0.99990"),
            Price::from("1.00005"),
            Quantity::from(100_000),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_book() -> OrderBook {
        OrderBook::new(sample_instrument().id, BookType::L2_MBP)
    }

    fn sample_book_deltas() -> OrderBookDeltas {
        let instrument = sample_instrument();
        let delta =
            OrderBookDelta::clear(instrument.id, 0, UnixNanos::default(), UnixNanos::default());
        OrderBookDeltas::new(instrument.id, vec![delta])
    }

    fn sample_mark_price() -> MarkPriceUpdate {
        MarkPriceUpdate::new(
            sample_instrument().id,
            Price::from("1.00000"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_index_price() -> IndexPriceUpdate {
        IndexPriceUpdate::new(
            sample_instrument().id,
            Price::from("1.00000"),
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_funding_rate() -> FundingRateUpdate {
        FundingRateUpdate::new(
            sample_instrument().id,
            "0.0001".parse().unwrap(),
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_instrument_status() -> InstrumentStatus {
        InstrumentStatus::new(
            sample_instrument().id,
            MarketStatusAction::Trading,
            UnixNanos::default(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        )
    }

    fn sample_instrument_close() -> InstrumentClose {
        InstrumentClose::new(
            sample_instrument().id,
            Price::from("1.00000"),
            InstrumentCloseType::EndOfSession,
            UnixNanos::default(),
            UnixNanos::default(),
        )
    }

    fn sample_option_greeks() -> OptionGreeks {
        OptionGreeks {
            instrument_id: sample_instrument().id,
            convention: GreeksConvention::BlackScholes,
            greeks: OptionGreekValues {
                delta: 0.55,
                gamma: 0.03,
                vega: 0.12,
                theta: -0.05,
                rho: 0.01,
            },
            mark_iv: Some(0.25),
            bid_iv: None,
            ask_iv: None,
            underlying_price: None,
            open_interest: None,
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    fn sample_option_chain() -> OptionChainSlice {
        OptionChainSlice {
            series_id: OptionSeriesId::new(
                Venue::from("SIM"),
                Ustr::from("AUD"),
                Ustr::from("USD"),
                UnixNanos::from(1_711_036_800_000_000_000),
            ),
            atm_strike: None,
            calls: Default::default(),
            puts: Default::default(),
            ts_event: UnixNanos::default(),
            ts_init: UnixNanos::default(),
        }
    }

    #[cfg(feature = "defi")]
    fn sample_block() -> Block {
        Block::new(
            "0x1234567890abcdef".to_string(),
            "0xabcdef1234567890".to_string(),
            12345,
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0".into(),
            21000,
            20000,
            UnixNanos::default(),
            Some(Blockchain::Ethereum),
        )
    }

    #[cfg(feature = "defi")]
    fn sample_pool_components() -> (Arc<Chain>, Arc<Dex>, Pool) {
        let chain = Arc::new(Chain::new(Blockchain::Ethereum, 1));
        let dex = Arc::new(Dex::new(
            Chain::new(Blockchain::Ethereum, 1),
            DexType::UniswapV3,
            "0x1F98431c8aD98523631AE4a59f267346ea31F984",
            0,
            AmmType::CLAMM,
            "PoolCreated",
            "Swap",
            "Mint",
            "Burn",
            "Collect",
        ));
        let token0 = Token::new(
            chain.clone(),
            "0xa0b86a33e6441c8c06dd7b111a8c4e82e2b2a5e1"
                .parse()
                .unwrap(),
            "USDC".into(),
            "USD Coin".into(),
            6,
        );
        let token1 = Token::new(
            chain.clone(),
            "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"
                .parse()
                .unwrap(),
            "WETH".into(),
            "Wrapped Ether".into(),
            18,
        );
        let pool_address = "0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8"
            .parse()
            .unwrap();
        let pool_identifier: PoolIdentifier = "0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8"
            .parse()
            .unwrap();
        let pool = Pool::new(
            chain.clone(),
            dex.clone(),
            pool_address,
            pool_identifier,
            12345,
            token0,
            token1,
            Some(500),
            Some(10),
            UnixNanos::default(),
        );

        (chain, dex, pool)
    }

    #[cfg(feature = "defi")]
    fn sample_pool_swap() -> PoolSwap {
        let (chain, dex, pool) = sample_pool_components();
        PoolSwap::new(
            chain,
            dex,
            pool.instrument_id,
            pool.pool_identifier,
            12345,
            "0xabc123".to_string(),
            0,
            0,
            None,
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                .parse()
                .unwrap(),
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                .parse()
                .unwrap(),
            I256::from_str("1000000000000000000").unwrap(),
            I256::from_str("400000000000000").unwrap(),
            U160::from(59000000000000u128),
            1000000,
            100,
        )
    }

    #[cfg(feature = "defi")]
    fn sample_pool_liquidity_update() -> PoolLiquidityUpdate {
        let (chain, dex, pool) = sample_pool_components();
        PoolLiquidityUpdate::new(
            chain,
            dex,
            pool.instrument_id,
            pool.pool_identifier,
            PoolLiquidityUpdateType::Mint,
            12345,
            "0xabc123".to_string(),
            0,
            0,
            Some(
                "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                    .parse()
                    .unwrap(),
            ),
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                .parse()
                .unwrap(),
            1000,
            U256::from(1_000u64),
            U256::from(2_000u64),
            -10,
            10,
            Some(UnixNanos::default()),
        )
    }

    #[cfg(feature = "defi")]
    fn sample_pool_fee_collect() -> PoolFeeCollect {
        let (chain, dex, pool) = sample_pool_components();
        PoolFeeCollect::new(
            chain,
            dex,
            pool.instrument_id,
            pool.pool_identifier,
            12345,
            "0xabc123".to_string(),
            0,
            0,
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                .parse()
                .unwrap(),
            100,
            200,
            -10,
            10,
            Some(UnixNanos::default()),
        )
    }

    #[cfg(feature = "defi")]
    fn sample_pool_flash() -> PoolFlash {
        let (chain, dex, pool) = sample_pool_components();
        PoolFlash::new(
            chain,
            dex,
            pool.instrument_id,
            pool.pool_identifier,
            12345,
            "0xabc123".to_string(),
            0,
            0,
            Some(UnixNanos::default()),
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                .parse()
                .unwrap(),
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                .parse()
                .unwrap(),
            U256::from(100u64),
            U256::from(200u64),
            U256::from(101u64),
            U256::from(201u64),
        )
    }

    const TRACKING_ACTOR_CODE: &std::ffi::CStr = c_str!(
        r#"
class TrackingActor:
    """A mock Python actor that tracks all method calls."""

    TRACKED_METHODS = {
        "on_start",
        "on_stop",
        "on_resume",
        "on_reset",
        "on_dispose",
        "on_degrade",
        "on_fault",
        "on_time_event",
        "on_data",
        "on_signal",
        "on_instrument",
        "on_quote",
        "on_trade",
        "on_bar",
        "on_book",
        "on_book_deltas",
        "on_mark_price",
        "on_index_price",
        "on_funding_rate",
        "on_instrument_status",
        "on_instrument_close",
        "on_option_greeks",
        "on_option_chain",
        "on_historical_data",
        "on_historical_quotes",
        "on_historical_trades",
        "on_historical_funding_rates",
        "on_historical_bars",
        "on_historical_mark_prices",
        "on_historical_index_prices",
        "on_block",
        "on_pool",
        "on_pool_swap",
        "on_pool_liquidity_update",
        "on_pool_fee_collect",
        "on_pool_flash",
    }

    def __init__(self):
        self.calls = []

    def _record(self, method_name, *args):
        self.calls.append((method_name, args))

    def was_called(self, method_name):
        return any(call[0] == method_name for call in self.calls)

    def call_count(self, method_name):
        return sum(1 for call in self.calls if call[0] == method_name)

    def __getattr__(self, name):
        if name in self.TRACKED_METHODS:
            return lambda *args: self._record(name, *args)
        raise AttributeError(name)
"#
    );

    fn create_tracking_python_actor(py: Python<'_>) -> PyResult<Py<PyAny>> {
        py.run(TRACKING_ACTOR_CODE, None, None)?;
        let tracking_actor_class = py.eval(c_str!("TrackingActor"), None, None)?;
        let instance = tracking_actor_class.call0()?;
        Ok(instance.unbind())
    }

    fn python_method_was_called(py_actor: &Py<PyAny>, py: Python<'_>, method_name: &str) -> bool {
        py_actor
            .call_method1(py, "was_called", (method_name,))
            .and_then(|r| r.extract::<bool>(py))
            .unwrap_or(false)
    }

    fn python_method_call_count(py_actor: &Py<PyAny>, py: Python<'_>, method_name: &str) -> i32 {
        py_actor
            .call_method1(py, "call_count", (method_name,))
            .and_then(|r| r.extract::<i32>(py))
            .unwrap_or(0)
    }

    fn assert_python_dispatch<F>(
        py: Python<'_>,
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        method_name: &str,
        invoke: F,
    ) where
        F: FnOnce(&mut PyDataActor) -> anyhow::Result<()>,
    {
        let py_actor = create_tracking_python_actor(py).unwrap();

        let mut rust_actor = PyDataActor::new(None);
        rust_actor.set_python_instance(py_actor.clone_ref(py));
        rust_actor.register(trader_id, clock, cache).unwrap();

        let result = invoke(&mut rust_actor);

        assert!(result.is_ok());
        assert!(python_method_was_called(&py_actor, py, method_name));
        assert_eq!(python_method_call_count(&py_actor, py, method_name), 1);
    }

    #[rstest]
    #[case("on_start")]
    #[case("on_stop")]
    #[case("on_resume")]
    #[case("on_reset")]
    #[case("on_dispose")]
    #[case("on_degrade")]
    #[case("on_fault")]
    fn test_python_dispatch_lifecycle_matrix(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        #[case] method_name: &str,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, clock, cache, trader_id, method_name, |rust_actor| {
                match method_name {
                    "on_start" => DataActor::on_start(rust_actor.inner_mut()),
                    "on_stop" => DataActor::on_stop(rust_actor.inner_mut()),
                    "on_resume" => DataActor::on_resume(rust_actor.inner_mut()),
                    "on_reset" => DataActor::on_reset(rust_actor.inner_mut()),
                    "on_dispose" => DataActor::on_dispose(rust_actor.inner_mut()),
                    "on_degrade" => DataActor::on_degrade(rust_actor.inner_mut()),
                    "on_fault" => DataActor::on_fault(rust_actor.inner_mut()),
                    _ => unreachable!("unhandled lifecycle case: {method_name}"),
                }
            });
        });
    }

    #[rstest]
    #[case("on_time_event")]
    #[case("on_data")]
    #[case("on_signal")]
    #[case("on_instrument")]
    #[case("on_quote")]
    #[case("on_trade")]
    #[case("on_bar")]
    #[case("on_book")]
    #[case("on_book_deltas")]
    #[case("on_mark_price")]
    #[case("on_index_price")]
    #[case("on_funding_rate")]
    #[case("on_instrument_status")]
    #[case("on_instrument_close")]
    #[case("on_option_greeks")]
    #[case("on_option_chain")]
    fn test_python_dispatch_typed_callback_matrix(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        #[case] method_name: &str,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, clock, cache, trader_id, method_name, |rust_actor| {
                match method_name {
                    "on_time_event" => {
                        let event = sample_time_event();
                        rust_actor.inner_mut().on_time_event(&event)
                    }
                    "on_data" => {
                        let data = sample_data();
                        rust_actor.inner_mut().on_data(&data)
                    }
                    "on_signal" => {
                        let signal = sample_signal();
                        rust_actor.inner_mut().on_signal(&signal)
                    }
                    "on_instrument" => {
                        let instrument = InstrumentAny::CurrencyPair(sample_instrument());
                        rust_actor.inner_mut().on_instrument(&instrument)
                    }
                    "on_quote" => {
                        let quote = sample_quote();
                        rust_actor.inner_mut().on_quote(&quote)
                    }
                    "on_trade" => {
                        let trade = sample_trade();
                        rust_actor.inner_mut().on_trade(&trade)
                    }
                    "on_bar" => {
                        let bar = sample_bar();
                        rust_actor.inner_mut().on_bar(&bar)
                    }
                    "on_book" => {
                        let book = sample_book();
                        rust_actor.inner_mut().on_book(&book)
                    }
                    "on_book_deltas" => {
                        let deltas = sample_book_deltas();
                        rust_actor.inner_mut().on_book_deltas(&deltas)
                    }
                    "on_mark_price" => {
                        let update = sample_mark_price();
                        rust_actor.inner_mut().on_mark_price(&update)
                    }
                    "on_index_price" => {
                        let update = sample_index_price();
                        rust_actor.inner_mut().on_index_price(&update)
                    }
                    "on_funding_rate" => {
                        let update = sample_funding_rate();
                        rust_actor.inner_mut().on_funding_rate(&update)
                    }
                    "on_instrument_status" => {
                        let status = sample_instrument_status();
                        rust_actor.inner_mut().on_instrument_status(&status)
                    }
                    "on_instrument_close" => {
                        let close = sample_instrument_close();
                        rust_actor.inner_mut().on_instrument_close(&close)
                    }
                    "on_option_greeks" => {
                        let greeks = sample_option_greeks();
                        rust_actor.inner_mut().on_option_greeks(&greeks)
                    }
                    "on_option_chain" => {
                        let chain = sample_option_chain();
                        rust_actor.inner_mut().on_option_chain(&chain)
                    }
                    _ => unreachable!("unhandled typed callback case: {method_name}"),
                }
            });
        });
    }

    #[rstest]
    #[case("on_historical_data")]
    #[case("on_historical_quotes")]
    #[case("on_historical_trades")]
    #[case("on_historical_funding_rates")]
    #[case("on_historical_bars")]
    #[case("on_historical_mark_prices")]
    #[case("on_historical_index_prices")]
    fn test_python_dispatch_historical_callback_matrix(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        #[case] method_name: &str,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, clock, cache, trader_id, method_name, |rust_actor| {
                match method_name {
                    "on_historical_data" => {
                        let data = sample_data();
                        rust_actor.inner_mut().on_historical_data(&data)
                    }
                    "on_historical_quotes" => {
                        let quotes = vec![sample_quote()];
                        rust_actor.inner_mut().on_historical_quotes(&quotes)
                    }
                    "on_historical_trades" => {
                        let trades = vec![sample_trade()];
                        rust_actor.inner_mut().on_historical_trades(&trades)
                    }
                    "on_historical_funding_rates" => {
                        let funding_rates = vec![sample_funding_rate()];
                        rust_actor
                            .inner_mut()
                            .on_historical_funding_rates(&funding_rates)
                    }
                    "on_historical_bars" => {
                        let bars = vec![sample_bar()];
                        rust_actor.inner_mut().on_historical_bars(&bars)
                    }
                    "on_historical_mark_prices" => {
                        let mark_prices = vec![sample_mark_price()];
                        rust_actor
                            .inner_mut()
                            .on_historical_mark_prices(&mark_prices)
                    }
                    "on_historical_index_prices" => {
                        let index_prices = vec![sample_index_price()];
                        rust_actor
                            .inner_mut()
                            .on_historical_index_prices(&index_prices)
                    }
                    _ => unreachable!("unhandled historical callback case: {method_name}"),
                }
            });
        });
    }

    #[cfg(feature = "defi")]
    #[rstest]
    #[case("on_block")]
    #[case("on_pool")]
    #[case("on_pool_swap")]
    #[case("on_pool_liquidity_update")]
    #[case("on_pool_fee_collect")]
    #[case("on_pool_flash")]
    fn test_python_dispatch_defi_callback_matrix(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        #[case] method_name: &str,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            assert_python_dispatch(py, clock, cache, trader_id, method_name, |rust_actor| {
                match method_name {
                    "on_block" => {
                        let block = sample_block();
                        rust_actor.inner_mut().on_block(&block)
                    }
                    "on_pool" => {
                        let (_chain, _dex, pool) = sample_pool_components();
                        rust_actor.inner_mut().on_pool(&pool)
                    }
                    "on_pool_swap" => {
                        let swap = sample_pool_swap();
                        rust_actor.inner_mut().on_pool_swap(&swap)
                    }
                    "on_pool_liquidity_update" => {
                        let update = sample_pool_liquidity_update();
                        rust_actor.inner_mut().on_pool_liquidity_update(&update)
                    }
                    "on_pool_fee_collect" => {
                        let collect = sample_pool_fee_collect();
                        rust_actor.inner_mut().on_pool_fee_collect(&collect)
                    }
                    "on_pool_flash" => {
                        let flash = sample_pool_flash();
                        rust_actor.inner_mut().on_pool_flash(&flash)
                    }
                    _ => unreachable!("unhandled defi callback case: {method_name}"),
                }
            });
        });
    }

    #[rstest]
    fn test_python_dispatch_multiple_calls_tracked(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let quote = QuoteTick::new(
                audusd_sim.id,
                Price::from("1.00000"),
                Price::from("1.00001"),
                Quantity::from(100_000),
                Quantity::from(100_000),
                UnixNanos::default(),
                UnixNanos::default(),
            );

            rust_actor.inner_mut().on_quote(&quote).unwrap();
            rust_actor.inner_mut().on_quote(&quote).unwrap();
            rust_actor.inner_mut().on_quote(&quote).unwrap();

            assert_eq!(python_method_call_count(&py_actor, py, "on_quote"), 3);
        });
    }

    #[rstest]
    fn test_python_dispatch_no_call_when_py_self_not_set(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|_py| {
            let mut rust_actor = PyDataActor::new(None);
            rust_actor.register(trader_id, clock, cache).unwrap();

            // When py_self is None, the dispatch returns Ok(()) without calling Python
            let result = DataActor::on_start(rust_actor.inner_mut());
            assert!(result.is_ok());
        });
    }

    #[rstest]
    fn test_python_on_historical_data_rejects_non_custom_data(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let non_custom: String = "not CustomData".to_string();
        let result = rust_actor.inner_mut().on_historical_data(&non_custom);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsupported type"));
    }
}
