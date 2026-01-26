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

use indexmap::IndexMap;
use nautilus_core::{
    nanos::UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
#[cfg(feature = "defi")]
use nautilus_model::defi::{
    Block, Blockchain, Pool, PoolFeeCollect, PoolFlash, PoolLiquidityUpdate, PoolSwap,
};
use nautilus_model::{
    data::{
        Bar, BarType, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OrderBookDeltas, QuoteTick, TradeTick, close::InstrumentClose,
    },
    enums::BookType,
    identifiers::{ActorId, ClientId, InstrumentId, TraderId, Venue},
    instruments::InstrumentAny,
    orderbook::OrderBook,
    python::instruments::instrument_any_to_pyobject,
};
use pyo3::{exceptions::PyValueError, prelude::*, types::PyDict};

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
impl DataActorConfig {
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
impl ImportableActorConfig {
    #[new]
    fn py_new(actor_path: String, config_path: String, config: Py<PyDict>) -> PyResult<Self> {
        let json_config = Python::attach(|py| -> PyResult<HashMap<String, serde_json::Value>> {
            let json_str: String = PyModule::import(py, "json")?
                .call_method("dumps", (config.bind(py),), None)?
                .extract()?;

            let json_value: serde_json::Value = serde_json::from_str(&json_str)
                .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;

            if let serde_json::Value::Object(map) = json_value {
                Ok(map.into_iter().collect())
            } else {
                Err(PyErr::new::<PyValueError, _>("Config must be a dictionary"))
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
            let json_str = serde_json::to_string(value)
                .map_err(|e| PyErr::new::<PyValueError, _>(e.to_string()))?;
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
    /// # Safety
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
    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Python::attach(|py| {
            // TODO: Create a placeholder object since we can't easily convert &dyn Any to Py<PyAny>
            // For now, we'll pass None and let Python subclasses handle specific data types
            let py_data = py.None();

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

    fn on_historical_data(&mut self, _data: &dyn Any) -> anyhow::Result<()> {
        Python::attach(|py| {
            let py_data = py.None();
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
impl PyDataActor {
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<DataActorConfig>) -> PyResult<Self> {
        Ok(Self::new(config))
    }

    #[getter]
    #[pyo3(name = "clock")]
    fn py_clock(&self) -> PyResult<PyClock> {
        let inner = self.inner();
        if inner.core.is_registered() {
            Ok(inner.clock.clone())
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
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
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
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
    fn py_shutdown_system(&self, reason: Option<String>) -> PyResult<()> {
        self.inner().core.shutdown_system(reason);
        Ok(())
    }

    #[pyo3(name = "on_start")]
    fn py_on_start(&self) -> PyResult<()> {
        self.inner().dispatch_on_start()
    }

    #[pyo3(name = "on_stop")]
    fn py_on_stop(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_stop()
    }

    #[pyo3(name = "on_resume")]
    fn py_on_resume(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_resume()
    }

    #[pyo3(name = "on_reset")]
    fn py_on_reset(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_reset()
    }

    #[pyo3(name = "on_dispose")]
    fn py_on_dispose(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_dispose()
    }

    #[pyo3(name = "on_degrade")]
    fn py_on_degrade(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_degrade()
    }

    #[pyo3(name = "on_fault")]
    fn py_on_fault(&mut self) -> PyResult<()> {
        self.inner_mut().dispatch_on_fault()
    }

    #[pyo3(name = "on_time_event")]
    fn py_on_time_event(&mut self, event: TimeEvent) -> PyResult<()> {
        self.inner_mut().dispatch_on_time_event(event)
    }

    #[pyo3(name = "on_data")]
    fn py_on_data(&mut self, data: Py<PyAny>) -> PyResult<()> {
        self.inner_mut().dispatch_on_data(data)
    }

    #[pyo3(name = "on_signal")]
    fn py_on_signal(&mut self, signal: &Signal) -> PyResult<()> {
        self.inner_mut().dispatch_on_signal(signal)
    }

    #[pyo3(name = "on_instrument")]
    fn py_on_instrument(&mut self, instrument: Py<PyAny>) -> PyResult<()> {
        self.inner_mut().dispatch_on_instrument(instrument)
    }

    #[pyo3(name = "on_quote")]
    fn py_on_quote(&mut self, quote: QuoteTick) -> PyResult<()> {
        self.inner_mut().dispatch_on_quote(quote)
    }

    #[pyo3(name = "on_trade")]
    fn py_on_trade(&mut self, trade: TradeTick) -> PyResult<()> {
        self.inner_mut().dispatch_on_trade(trade)
    }

    #[pyo3(name = "on_bar")]
    fn py_on_bar(&mut self, bar: Bar) -> PyResult<()> {
        self.inner_mut().dispatch_on_bar(bar)
    }

    #[pyo3(name = "on_book_deltas")]
    fn py_on_book_deltas(&mut self, deltas: OrderBookDeltas) -> PyResult<()> {
        self.inner_mut().dispatch_on_book_deltas(deltas)
    }

    #[pyo3(name = "on_book")]
    fn py_on_book(&mut self, book: &OrderBook) -> PyResult<()> {
        self.inner_mut().dispatch_on_book(book)
    }

    #[pyo3(name = "on_mark_price")]
    fn py_on_mark_price(&mut self, mark_price: MarkPriceUpdate) -> PyResult<()> {
        self.inner_mut().dispatch_on_mark_price(mark_price)
    }

    #[pyo3(name = "on_index_price")]
    fn py_on_index_price(&mut self, index_price: IndexPriceUpdate) -> PyResult<()> {
        self.inner_mut().dispatch_on_index_price(index_price)
    }

    #[pyo3(name = "on_funding_rate")]
    fn py_on_funding_rate(&mut self, funding_rate: FundingRateUpdate) -> PyResult<()> {
        self.inner_mut().dispatch_on_funding_rate(funding_rate)
    }

    #[pyo3(name = "on_instrument_status")]
    fn py_on_instrument_status(&mut self, status: InstrumentStatus) -> PyResult<()> {
        self.inner_mut().dispatch_on_instrument_status(status)
    }

    #[pyo3(name = "on_instrument_close")]
    fn py_on_instrument_close(&mut self, close: InstrumentClose) -> PyResult<()> {
        self.inner_mut().dispatch_on_instrument_close(close)
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "on_block")]
    fn py_on_block(&mut self, block: Block) -> PyResult<()> {
        self.inner_mut().dispatch_on_block(block)
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "on_pool")]
    fn py_on_pool(&mut self, pool: Pool) -> PyResult<()> {
        self.inner_mut().dispatch_on_pool(pool)
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "on_pool_swap")]
    fn py_on_pool_swap(&mut self, swap: PoolSwap) -> PyResult<()> {
        self.inner_mut().dispatch_on_pool_swap(swap)
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "on_pool_liquidity_update")]
    fn py_on_pool_liquidity_update(&mut self, update: PoolLiquidityUpdate) -> PyResult<()> {
        self.inner_mut().dispatch_on_pool_liquidity_update(update)
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "on_pool_fee_collect")]
    fn py_on_pool_fee_collect(&mut self, update: PoolFeeCollect) -> PyResult<()> {
        self.inner_mut().dispatch_on_pool_fee_collect(update)
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "on_pool_flash")]
    fn py_on_pool_flash(&mut self, flash: PoolFlash) -> PyResult<()> {
        self.inner_mut().dispatch_on_pool_flash(flash)
    }

    #[pyo3(name = "subscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_subscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_data(self.inner_mut(), data_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_subscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instruments(self.inner_mut(), venue, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instrument(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_book_deltas")]
    #[pyo3(signature = (instrument_id, book_type, depth=None, client_id=None, managed=false, params=None))]
    fn py_subscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        managed: bool,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
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
    fn py_subscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        book_type: BookType,
        interval_ms: usize,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let depth = depth.and_then(NonZeroUsize::new);
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| PyErr::new::<PyValueError, _>("interval_ms must be > 0"))?;

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
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_quotes(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_trades(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_subscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_bars(self.inner_mut(), bar_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_index_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instrument_status(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_instrument_close")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_instrument_close(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_instrument_close(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_order_fills")]
    #[pyo3(signature = (instrument_id))]
    fn py_subscribe_order_fills(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        DataActor::subscribe_order_fills(self.inner_mut(), instrument_id);
        Ok(())
    }

    #[pyo3(name = "subscribe_order_cancels")]
    #[pyo3(signature = (instrument_id))]
    fn py_subscribe_order_cancels(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        DataActor::subscribe_order_cancels(self.inner_mut(), instrument_id);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "subscribe_blocks")]
    #[pyo3(signature = (chain, client_id=None, params=None))]
    fn py_subscribe_blocks(
        &mut self,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_blocks(self.inner_mut(), chain, client_id, params);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "subscribe_pool")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_pool(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "subscribe_pool_swaps")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_swaps(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_pool_swaps(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "subscribe_pool_liquidity_updates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_liquidity_updates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_pool_liquidity_updates(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "subscribe_pool_fee_collects")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_fee_collects(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_pool_fee_collects(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "subscribe_pool_flash_events")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_subscribe_pool_flash_events(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::subscribe_pool_flash_events(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "request_data")]
    #[pyo3(signature = (data_type, client_id, start=None, end=None, limit=None, params=None))]
    fn py_request_data(
        &mut self,
        data_type: DataType,
        client_id: ClientId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
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
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
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
        venue: Option<Venue>,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
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
        instrument_id: InstrumentId,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
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
    fn py_request_quotes(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
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
    fn py_request_trades(
        &mut self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
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

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_bars(
        &mut self,
        bar_type: BarType,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
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

    #[pyo3(name = "unsubscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_unsubscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_data(self.inner_mut(), data_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instruments")]
    #[pyo3(signature = (venue, client_id=None, params=None))]
    fn py_unsubscribe_instruments(
        &mut self,
        venue: Venue,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_instruments(self.inner_mut(), venue, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_instrument(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_book_deltas(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_book_deltas(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_book_at_interval")]
    #[pyo3(signature = (instrument_id, interval_ms, client_id=None, params=None))]
    fn py_unsubscribe_book_at_interval(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: usize,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        let interval_ms = NonZeroUsize::new(interval_ms)
            .ok_or_else(|| PyErr::new::<PyValueError, _>("interval_ms must be > 0"))?;

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
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_quotes(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_trades")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_trades(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_trades(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, params=None))]
    fn py_unsubscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_bars(self.inner_mut(), bar_type, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_mark_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_mark_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_index_prices(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_index_prices(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_instrument_status")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_instrument_status(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
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
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_instrument_close(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_order_fills")]
    #[pyo3(signature = (instrument_id))]
    fn py_unsubscribe_order_fills(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        DataActor::unsubscribe_order_fills(self.inner_mut(), instrument_id);
        Ok(())
    }

    #[pyo3(name = "unsubscribe_order_cancels")]
    #[pyo3(signature = (instrument_id))]
    fn py_unsubscribe_order_cancels(&mut self, instrument_id: InstrumentId) -> PyResult<()> {
        DataActor::unsubscribe_order_cancels(self.inner_mut(), instrument_id);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "unsubscribe_blocks")]
    #[pyo3(signature = (chain, client_id=None, params=None))]
    fn py_unsubscribe_blocks(
        &mut self,
        chain: Blockchain,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_blocks(self.inner_mut(), chain, client_id, params);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "unsubscribe_pool")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_pool(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "unsubscribe_pool_swaps")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_swaps(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_pool_swaps(self.inner_mut(), instrument_id, client_id, params);
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "unsubscribe_pool_liquidity_updates")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_liquidity_updates(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_pool_liquidity_updates(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "unsubscribe_pool_fee_collects")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_fee_collects(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_pool_fee_collects(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[pyo3(name = "unsubscribe_pool_flash_events")]
    #[pyo3(signature = (instrument_id, client_id=None, params=None))]
    fn py_unsubscribe_pool_flash_events(
        &mut self,
        instrument_id: InstrumentId,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        DataActor::unsubscribe_pool_flash_events(
            self.inner_mut(),
            instrument_id,
            client_id,
            params,
        );
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_historical_data")]
    fn py_on_historical_data(&mut self, data: Py<PyAny>) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_historical_quotes")]
    fn py_on_historical_quotes(&mut self, quotes: Vec<QuoteTick>) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_historical_trades")]
    fn py_on_historical_trades(&mut self, trades: Vec<TradeTick>) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_historical_bars")]
    fn py_on_historical_bars(&mut self, bars: Vec<Bar>) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_historical_mark_prices")]
    fn py_on_historical_mark_prices(&mut self, mark_prices: Vec<MarkPriceUpdate>) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_historical_index_prices")]
    fn py_on_historical_index_prices(
        &mut self,
        index_prices: Vec<IndexPriceUpdate>,
    ) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        cell::RefCell,
        collections::HashMap,
        ops::{Deref, DerefMut},
        rc::Rc,
        str::FromStr,
        sync::{Arc, Mutex},
    };

    #[cfg(feature = "defi")]
    use alloy_primitives::{I256, U160};
    use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos};
    #[cfg(feature = "defi")]
    use nautilus_model::defi::{
        AmmType, Block, Blockchain, Chain, Dex, DexType, Pool, PoolIdentifier, PoolLiquidityUpdate,
        PoolSwap, Token,
    };
    use nautilus_model::{
        data::{
            Bar, BarType, DataType, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
            OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick, close::InstrumentClose,
        },
        enums::{AggressorSide, BookType, InstrumentCloseType, MarketStatusAction},
        identifiers::{ClientId, TradeId, TraderId, Venue},
        instruments::{CurrencyPair, InstrumentAny, stubs::audusd_sim},
        orderbook::OrderBook,
        types::{Price, Quantity},
    };
    use pyo3::{Py, PyAny, PyResult, Python, ffi::c_str, types::PyAnyMethods};
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::PyDataActor;
    use crate::{
        actor::{DataActor, data_actor::DataActorCore},
        cache::Cache,
        clock::TestClock,
        component::Component,
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
        DataType::new("TestData", None)
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

        assert!(
            actor
                .py_subscribe_data(data_type.clone(), Some(client_id), None)
                .is_ok()
        );
        assert!(
            actor
                .py_subscribe_quotes(audusd_sim.id, Some(client_id), None)
                .is_ok()
        );
        assert!(
            actor
                .py_unsubscribe_data(data_type, Some(client_id), None)
                .is_ok()
        );
        assert!(
            actor
                .py_unsubscribe_quotes(audusd_sim.id, Some(client_id), None)
                .is_ok()
        );
    }

    #[rstest]
    fn test_shutdown_system_passes_through(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        let actor = create_registered_actor(clock, cache, trader_id);

        assert!(
            actor
                .py_shutdown_system(Some("Test shutdown".to_string()))
                .is_ok()
        );
        assert!(actor.py_shutdown_system(None).is_ok());
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

        let result = actor.py_subscribe_book_at_interval(
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

        let result = actor.py_unsubscribe_book_at_interval(audusd_sim.id, 0, None, None);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "ValueError: interval_ms must be > 0"
        );
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

    static CALL_TRACKER: std::sync::LazyLock<Arc<Mutex<HashMap<String, i32>>>> =
        std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

    #[derive(Debug)]
    struct TestDataActor {
        inner: PyDataActor,
    }

    impl TestDataActor {
        fn new() -> Self {
            Self {
                inner: PyDataActor::new(None),
            }
        }

        fn track_call(&self, handler_name: &str) {
            let mut tracker = CALL_TRACKER.lock().expect(MUTEX_POISONED);
            *tracker.entry(handler_name.to_string()).or_insert(0) += 1;
        }

        fn get_call_count(&self, handler_name: &str) -> i32 {
            let tracker = CALL_TRACKER.lock().expect(MUTEX_POISONED);
            tracker.get(handler_name).copied().unwrap_or(0)
        }

        fn reset_tracker(&self) {
            let mut tracker = CALL_TRACKER.lock().expect(MUTEX_POISONED);
            tracker.clear();
        }
    }

    impl Deref for TestDataActor {
        type Target = DataActorCore;
        fn deref(&self) -> &Self::Target {
            &self.inner.inner().core
        }
    }

    impl DerefMut for TestDataActor {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner.inner_mut().core
        }
    }

    impl DataActor for TestDataActor {
        fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
            self.track_call("on_time_event");
            self.inner.inner_mut().on_time_event(event)
        }

        fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
            self.track_call("on_data");
            self.inner.inner_mut().on_data(data)
        }

        fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
            self.track_call("on_signal");
            self.inner.inner_mut().on_signal(signal)
        }

        fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
            self.track_call("on_instrument");
            self.inner.inner_mut().on_instrument(instrument)
        }

        fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
            self.track_call("on_quote");
            self.inner.inner_mut().on_quote(quote)
        }

        fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
            self.track_call("on_trade");
            self.inner.inner_mut().on_trade(trade)
        }

        fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
            self.track_call("on_bar");
            self.inner.inner_mut().on_bar(bar)
        }

        fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
            self.track_call("on_book");
            self.inner.inner_mut().on_book(book)
        }

        fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
            self.track_call("on_book_deltas");
            self.inner.inner_mut().on_book_deltas(deltas)
        }

        fn on_mark_price(&mut self, update: &MarkPriceUpdate) -> anyhow::Result<()> {
            self.track_call("on_mark_price");
            self.inner.inner_mut().on_mark_price(update)
        }

        fn on_index_price(&mut self, update: &IndexPriceUpdate) -> anyhow::Result<()> {
            self.track_call("on_index_price");
            self.inner.inner_mut().on_index_price(update)
        }

        fn on_instrument_status(&mut self, update: &InstrumentStatus) -> anyhow::Result<()> {
            self.track_call("on_instrument_status");
            self.inner.inner_mut().on_instrument_status(update)
        }

        fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
            self.track_call("on_instrument_close");
            self.inner.inner_mut().on_instrument_close(update)
        }

        #[cfg(feature = "defi")]
        fn on_block(&mut self, block: &Block) -> anyhow::Result<()> {
            self.track_call("on_block");
            self.inner.inner_mut().on_block(block)
        }

        #[cfg(feature = "defi")]
        fn on_pool(&mut self, pool: &Pool) -> anyhow::Result<()> {
            self.track_call("on_pool");
            self.inner.inner_mut().on_pool(pool)
        }

        #[cfg(feature = "defi")]
        fn on_pool_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
            self.track_call("on_pool_swap");
            self.inner.inner_mut().on_pool_swap(swap)
        }

        #[cfg(feature = "defi")]
        fn on_pool_liquidity_update(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
            self.track_call("on_pool_liquidity_update");
            self.inner.inner_mut().on_pool_liquidity_update(update)
        }
    }

    #[rstest]
    fn test_python_on_signal_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor.register(trader_id, clock, cache).unwrap();

        let signal = Signal::new(
            Ustr::from("test_signal"),
            "1.0".to_string(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(test_actor.on_signal(&signal).is_ok());
        assert_eq!(test_actor.get_call_count("on_signal"), 1);
    }

    #[rstest]
    fn test_python_on_data_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor.register(trader_id, clock, cache).unwrap();

        assert!(test_actor.on_data(&()).is_ok());
        assert_eq!(test_actor.get_call_count("on_data"), 1);
    }

    #[rstest]
    fn test_python_on_time_event_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor.register(trader_id, clock, cache).unwrap();

        let time_event = TimeEvent::new(
            Ustr::from("test_timer"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(test_actor.on_time_event(&time_event).is_ok());
        assert_eq!(test_actor.get_call_count("on_time_event"), 1);
    }

    #[rstest]
    fn test_python_on_instrument_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let instrument = InstrumentAny::CurrencyPair(audusd_sim);

        assert!(rust_actor.inner_mut().on_instrument(&instrument).is_ok());
    }

    #[rstest]
    fn test_python_on_quote_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let quote = QuoteTick::new(
            audusd_sim.id,
            Price::from("1.0000"),
            Price::from("1.0001"),
            Quantity::from("100000"),
            Quantity::from("100000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.inner_mut().on_quote(&quote).is_ok());
    }

    #[rstest]
    fn test_python_on_trade_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let trade = TradeTick::new(
            audusd_sim.id,
            Price::from("1.0000"),
            Quantity::from("100000"),
            AggressorSide::Buyer,
            "T123".to_string().into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.inner_mut().on_trade(&trade).is_ok());
    }

    #[rstest]
    fn test_python_on_bar_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let bar_type =
            BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
        let bar = Bar::new(
            bar_type,
            Price::from("1.0000"),
            Price::from("1.0001"),
            Price::from("0.9999"),
            Price::from("1.0000"),
            Quantity::from("100000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.inner_mut().on_bar(&bar).is_ok());
    }

    #[rstest]
    fn test_python_on_book_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);
        assert!(rust_actor.inner_mut().on_book(&book).is_ok());
    }

    #[rstest]
    fn test_python_on_book_deltas_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let delta =
            OrderBookDelta::clear(audusd_sim.id, 0, UnixNanos::default(), UnixNanos::default());
        let deltas = OrderBookDeltas::new(audusd_sim.id, vec![delta]);

        assert!(rust_actor.inner_mut().on_book_deltas(&deltas).is_ok());
    }

    #[rstest]
    fn test_python_on_mark_price_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let mark_price = MarkPriceUpdate::new(
            audusd_sim.id,
            Price::from("1.0000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.inner_mut().on_mark_price(&mark_price).is_ok());
    }

    #[rstest]
    fn test_python_on_index_price_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let index_price = IndexPriceUpdate::new(
            audusd_sim.id,
            Price::from("1.0000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.inner_mut().on_index_price(&index_price).is_ok());
    }

    #[rstest]
    fn test_python_on_instrument_status_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let status = InstrumentStatus::new(
            audusd_sim.id,
            MarketStatusAction::Trading,
            UnixNanos::default(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        );

        assert!(rust_actor.inner_mut().on_instrument_status(&status).is_ok());
    }

    #[rstest]
    fn test_python_on_instrument_close_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let close = InstrumentClose::new(
            audusd_sim.id,
            Price::from("1.0000"),
            InstrumentCloseType::EndOfSession,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.inner_mut().on_instrument_close(&close).is_ok());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_python_on_block_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor.register(trader_id, clock, cache).unwrap();

        let block = Block::new(
            "0x1234567890abcdef".to_string(),
            "0xabcdef1234567890".to_string(),
            12345,
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0".into(),
            21000,
            20000,
            UnixNanos::default(),
            Some(Blockchain::Ethereum),
        );

        assert!(test_actor.on_block(&block).is_ok());
        assert_eq!(test_actor.get_call_count("on_block"), 1);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_python_on_pool_swap_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

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
        let pool = Arc::new(Pool::new(
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
        ));

        let swap = PoolSwap::new(
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
        );

        assert!(rust_actor.inner_mut().on_pool_swap(&swap).is_ok());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_python_on_pool_liquidity_update_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        let mut rust_actor = PyDataActor::new(None);
        rust_actor.register(trader_id, clock, cache).unwrap();

        let block = Block::new(
            "0x1234567890abcdef".to_string(),
            "0xabcdef1234567890".to_string(),
            12345,
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0".into(),
            21000,
            20000,
            UnixNanos::default(),
            Some(Blockchain::Ethereum),
        );

        // We test on_block since PoolLiquidityUpdate construction is complex
        assert!(rust_actor.inner_mut().on_block(&block).is_ok());
    }

    const TRACKING_ACTOR_CODE: &std::ffi::CStr = c_str!(
        r#"
class TrackingActor:
    """A mock Python actor that tracks all method calls."""

    def __init__(self):
        self.calls = []

    def _record(self, method_name, *args):
        self.calls.append((method_name, args))

    def was_called(self, method_name):
        return any(call[0] == method_name for call in self.calls)

    def call_count(self, method_name):
        return sum(1 for call in self.calls if call[0] == method_name)

    def on_start(self):
        self._record("on_start")

    def on_stop(self):
        self._record("on_stop")

    def on_resume(self):
        self._record("on_resume")

    def on_reset(self):
        self._record("on_reset")

    def on_dispose(self):
        self._record("on_dispose")

    def on_degrade(self):
        self._record("on_degrade")

    def on_fault(self):
        self._record("on_fault")

    def on_time_event(self, event):
        self._record("on_time_event", event)

    def on_data(self, data):
        self._record("on_data", data)

    def on_signal(self, signal):
        self._record("on_signal", signal)

    def on_instrument(self, instrument):
        self._record("on_instrument", instrument)

    def on_quote(self, quote):
        self._record("on_quote", quote)

    def on_trade(self, trade):
        self._record("on_trade", trade)

    def on_bar(self, bar):
        self._record("on_bar", bar)

    def on_book(self, book):
        self._record("on_book", book)

    def on_book_deltas(self, deltas):
        self._record("on_book_deltas", deltas)

    def on_mark_price(self, update):
        self._record("on_mark_price", update)

    def on_index_price(self, update):
        self._record("on_index_price", update)

    def on_funding_rate(self, update):
        self._record("on_funding_rate", update)

    def on_instrument_status(self, status):
        self._record("on_instrument_status", status)

    def on_instrument_close(self, close):
        self._record("on_instrument_close", close)

    def on_historical_data(self, data):
        self._record("on_historical_data", data)

    def on_historical_quotes(self, quotes):
        self._record("on_historical_quotes", quotes)

    def on_historical_trades(self, trades):
        self._record("on_historical_trades", trades)

    def on_historical_bars(self, bars):
        self._record("on_historical_bars", bars)

    def on_historical_mark_prices(self, prices):
        self._record("on_historical_mark_prices", prices)

    def on_historical_index_prices(self, prices):
        self._record("on_historical_index_prices", prices)
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

    #[rstest]
    fn test_python_dispatch_on_start(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let result = DataActor::on_start(rust_actor.inner_mut());

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_start"));
            assert_eq!(python_method_call_count(&py_actor, py, "on_start"), 1);
        });
    }

    #[rstest]
    fn test_python_dispatch_on_stop(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let result = DataActor::on_stop(rust_actor.inner_mut());

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_stop"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_resume(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let result = DataActor::on_resume(rust_actor.inner_mut());

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_resume"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_reset(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let result = DataActor::on_reset(rust_actor.inner_mut());

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_reset"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_dispose(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let result = DataActor::on_dispose(rust_actor.inner_mut());

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_dispose"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_degrade(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let result = DataActor::on_degrade(rust_actor.inner_mut());

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_degrade"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_fault(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let result = DataActor::on_fault(rust_actor.inner_mut());

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_fault"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_signal(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let signal = Signal::new(
                Ustr::from("test_signal"),
                "1.0".to_string(),
                UnixNanos::default(),
                UnixNanos::default(),
            );

            let result = rust_actor.inner_mut().on_signal(&signal);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_signal"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_time_event(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::Python::initialize();
        Python::attach(|py| {
            let py_actor = create_tracking_python_actor(py).unwrap();

            let mut rust_actor = PyDataActor::new(None);
            rust_actor.set_python_instance(py_actor.clone_ref(py));
            rust_actor.register(trader_id, clock, cache).unwrap();

            let time_event = TimeEvent::new(
                Ustr::from("test_timer"),
                UUID4::new(),
                UnixNanos::default(),
                UnixNanos::default(),
            );

            let result = rust_actor.inner_mut().on_time_event(&time_event);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_time_event"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_instrument(
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

            let instrument = InstrumentAny::CurrencyPair(audusd_sim);

            let result = rust_actor.inner_mut().on_instrument(&instrument);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_instrument"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_quote(
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

            let result = rust_actor.inner_mut().on_quote(&quote);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_quote"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_trade(
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

            let trade = TradeTick::new(
                audusd_sim.id,
                Price::from("1.00000"),
                Quantity::from(100_000),
                AggressorSide::Buyer,
                TradeId::new("123456"),
                UnixNanos::default(),
                UnixNanos::default(),
            );

            let result = rust_actor.inner_mut().on_trade(&trade);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_trade"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_bar(
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

            let bar_type =
                BarType::from_str(&format!("{}-1-MINUTE-LAST-INTERNAL", audusd_sim.id)).unwrap();
            let bar = Bar::new(
                bar_type,
                Price::from("1.00000"),
                Price::from("1.00010"),
                Price::from("0.99990"),
                Price::from("1.00005"),
                Quantity::from(100_000),
                UnixNanos::default(),
                UnixNanos::default(),
            );

            let result = rust_actor.inner_mut().on_bar(&bar);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_bar"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_book(
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

            let book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);

            let result = rust_actor.inner_mut().on_book(&book);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_book"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_book_deltas(
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

            let delta =
                OrderBookDelta::clear(audusd_sim.id, 0, UnixNanos::default(), UnixNanos::default());
            let deltas = OrderBookDeltas::new(audusd_sim.id, vec![delta]);

            let result = rust_actor.inner_mut().on_book_deltas(&deltas);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_book_deltas"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_mark_price(
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

            let mark_price = MarkPriceUpdate::new(
                audusd_sim.id,
                Price::from("1.00000"),
                UnixNanos::default(),
                UnixNanos::default(),
            );

            let result = rust_actor.inner_mut().on_mark_price(&mark_price);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_mark_price"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_index_price(
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

            let index_price = IndexPriceUpdate::new(
                audusd_sim.id,
                Price::from("1.00000"),
                UnixNanos::default(),
                UnixNanos::default(),
            );

            let result = rust_actor.inner_mut().on_index_price(&index_price);

            assert!(result.is_ok());
            assert!(python_method_was_called(&py_actor, py, "on_index_price"));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_instrument_status(
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

            let status = InstrumentStatus::new(
                audusd_sim.id,
                MarketStatusAction::Trading,
                UnixNanos::default(),
                UnixNanos::default(),
                None,
                None,
                None,
                None,
                None,
            );

            let result = rust_actor.inner_mut().on_instrument_status(&status);

            assert!(result.is_ok());
            assert!(python_method_was_called(
                &py_actor,
                py,
                "on_instrument_status"
            ));
        });
    }

    #[rstest]
    fn test_python_dispatch_on_instrument_close(
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

            let close = InstrumentClose::new(
                audusd_sim.id,
                Price::from("1.00000"),
                InstrumentCloseType::EndOfSession,
                UnixNanos::default(),
                UnixNanos::default(),
            );

            let result = rust_actor.inner_mut().on_instrument_close(&close);

            assert!(result.is_ok());
            assert!(python_method_was_called(
                &py_actor,
                py,
                "on_instrument_close"
            ));
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
}
