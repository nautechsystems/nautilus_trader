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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, num::NonZeroUsize, rc::Rc};

use indexmap::IndexMap;
use nautilus_core::{nanos::UnixNanos, python::to_pyvalue_err};
use nautilus_model::{
    data::{BarType, DataType},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, TraderId, Venue},
};
use pyo3::prelude::*;

use crate::{
    actor::{
        Actor,
        data_actor::{DataActor, DataActorConfig, DataActorCore},
    },
    cache::Cache,
    clock::Clock,
    enums::ComponentState,
};

/// Inner actor that implements `DataActor` and can be used as the generic type parameter.
///
/// Holds the `DataActorCore` and implements the `DataActor` trait, allowing it to be used
/// with the generic methods on `DataActorCore`.
#[derive(Debug)]
pub struct PyDataActorInner {
    core: DataActorCore,
}

impl PyDataActorInner {
    pub fn new(
        config: DataActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> Self {
        Self {
            core: DataActorCore::new(config, cache, clock),
        }
    }

    pub fn core(&self) -> &DataActorCore {
        &self.core
    }

    pub fn core_mut(&mut self) -> &mut DataActorCore {
        &mut self.core
    }
}

impl Actor for PyDataActorInner {
    fn id(&self) -> ustr::Ustr {
        self.core.actor_id.inner()
    }

    fn handle(&mut self, msg: &dyn std::any::Any) {
        self.core.handle(msg)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl DataActor for PyDataActorInner {
    fn state(&self) -> ComponentState {
        self.core.state()
    }
}

/// Provides a generic `DataActor`.
#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "DataActor",
    unsendable
)]
#[derive(Debug)]
pub struct PyDataActor {
    inner: Option<PyDataActorInner>,
    config: DataActorConfig,
}

impl PyDataActor {
    /// Gets a reference to the inner actor, returning an error if not registered.
    fn inner(&self) -> PyResult<&PyDataActorInner> {
        self.inner.as_ref().ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "DataActor has not been registered with a system",
            )
        })
    }

    /// Gets a mutable reference to the inner actor, returning an error if not registered.
    fn inner_mut(&mut self) -> PyResult<&mut PyDataActorInner> {
        self.inner.as_mut().ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "DataActor has not been registered with a system",
            )
        })
    }

    /// Gets a reference to the core, returning an error if not registered.
    fn core(&self) -> PyResult<&DataActorCore> {
        Ok(self.inner()?.core())
    }

    /// Gets a mutable reference to the core, returning an error if not registered.
    fn core_mut(&mut self) -> PyResult<&mut DataActorCore> {
        Ok(self.inner_mut()?.core_mut())
    }

    /// TODO: WIP
    /// This method should be called to properly initialize the actor
    /// with cache, clock and other components.
    ///
    /// # Errors
    ///
    /// Returns an error if already registered.
    pub fn register(
        &mut self,
        trader_id: TraderId,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> PyResult<()> {
        if self.inner.is_some() {
            return Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "DataActor has already been registered",
            ));
        }

        // Create the inner actor with the components
        let mut inner = PyDataActorInner::new(self.config.clone(), cache, clock);
        inner.core_mut().set_trader_id(trader_id);
        self.inner = Some(inner);

        Ok(())
    }
}

#[pymethods]
impl PyDataActor {
    #[new]
    #[pyo3(signature = (_config=None))]
    fn py_new(_config: Option<PyObject>) -> PyResult<Self> {
        // TODO: Create with default config but no inner actor until registered
        let config = DataActorConfig::default();

        Ok(Self {
            inner: None,
            config,
        })
    }

    #[getter]
    fn actor_id(&self) -> PyResult<String> {
        Ok(self.core()?.actor_id.to_string())
    }

    #[getter]
    fn state(&self) -> PyResult<ComponentState> {
        Ok(self.core()?.state())
    }

    #[getter]
    fn trader_id(&self) -> PyResult<Option<String>> {
        Ok(self.core()?.trader_id().map(|id| id.to_string()))
    }

    fn is_ready(&self) -> PyResult<bool> {
        Ok(self.state()? == ComponentState::Ready)
    }

    fn is_running(&self) -> PyResult<bool> {
        Ok(self.state()? == ComponentState::Running)
    }

    fn is_stopped(&self) -> PyResult<bool> {
        Ok(self.state()? == ComponentState::Stopped)
    }

    fn is_disposed(&self) -> PyResult<bool> {
        Ok(self.state()? == ComponentState::Disposed)
    }

    fn is_degraded(&self) -> PyResult<bool> {
        Ok(self.state()? == ComponentState::Degraded)
    }

    fn is_faulting(&self) -> PyResult<bool> {
        Ok(self.state()? == ComponentState::Faulted)
    }

    #[pyo3(name = "initialize")]
    fn py_initialize(&mut self) -> PyResult<()> {
        self.core_mut()?.initialize().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "start")]
    fn py_start(&mut self) -> PyResult<()> {
        self.core_mut()?.start().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "stop")]
    fn py_stop(&mut self) -> PyResult<()> {
        self.core_mut()?.stop().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "resume")]
    fn py_resume(&mut self) -> PyResult<()> {
        self.core_mut()?.resume().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) -> PyResult<()> {
        self.core_mut()?.reset().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) -> PyResult<()> {
        self.core_mut()?.dispose().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "degrade")]
    fn py_degrade(&mut self) -> PyResult<()> {
        self.core_mut()?.degrade().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "fault")]
    fn py_fault(&mut self) -> PyResult<()> {
        self.core_mut()?.fault().map_err(to_pyvalue_err)
    }

    #[pyo3(name = "register_warning_event")]
    fn py_register_warning_event(&mut self, event_type: &str) -> PyResult<()> {
        self.core_mut()?.register_warning_event(event_type);
        Ok(())
    }

    #[pyo3(name = "deregister_warning_event")]
    fn py_deregister_warning_event(&mut self, event_type: &str) -> PyResult<()> {
        self.core_mut()?.deregister_warning_event(event_type);
        Ok(())
    }

    #[pyo3(name = "shutdown_system")]
    #[pyo3(signature = (reason=None))]
    fn py_shutdown_system(&self, reason: Option<String>) -> PyResult<()> {
        self.core()?.shutdown_system(reason);
        Ok(())
    }

    #[pyo3(name = "subscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_subscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        self.inner_mut()?
            .core_mut()
            .subscribe_data::<PyDataActorInner>(data_type, client_id, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_instruments::<PyDataActorInner>(venue, client_id, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_instrument::<PyDataActorInner>(instrument_id, client_id, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_book_deltas::<PyDataActorInner>(
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
        let interval_ms = NonZeroUsize::new(interval_ms).ok_or_else(|| {
            PyErr::new::<pyo3::exceptions::PyValueError, _>("interval_ms must be > 0")
        })?;
        self.inner_mut()?
            .core_mut()
            .subscribe_book_at_interval::<PyDataActorInner>(
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
        self.inner_mut()?
            .core_mut()
            .subscribe_quotes::<PyDataActorInner>(instrument_id, client_id, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_trades::<PyDataActorInner>(instrument_id, client_id, params);
        Ok(())
    }

    #[pyo3(name = "subscribe_bars")]
    #[pyo3(signature = (bar_type, client_id=None, await_partial=false, params=None))]
    fn py_subscribe_bars(
        &mut self,
        bar_type: BarType,
        client_id: Option<ClientId>,
        await_partial: bool,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        self.inner_mut()?
            .core_mut()
            .subscribe_bars::<PyDataActorInner>(bar_type, client_id, await_partial, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_mark_prices::<PyDataActorInner>(instrument_id, client_id, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_index_prices::<PyDataActorInner>(instrument_id, client_id, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_instrument_status::<PyDataActorInner>(instrument_id, client_id, params);
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
        self.inner_mut()?
            .core_mut()
            .subscribe_instrument_close::<PyDataActorInner>(instrument_id, client_id, params);
        Ok(())
    }

    // Request methods
    #[pyo3(name = "request_data")]
    #[pyo3(signature = (data_type, client_id, start=None, end=None, limit=None, params=None))]
    fn py_request_data(
        &self,
        data_type: DataType,
        client_id: ClientId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(UnixNanos::from);
        let end = end.map(UnixNanos::from);

        // let request_id = self.core.request_data::<Self>(data_type, client_id, start, end, limit, params)
        //     .map_err(to_pyvalue_err)?;
        // Ok(request_id.to_string())
        Ok("placeholder".to_string()) // Placeholder for now
    }

    #[pyo3(name = "request_instrument")]
    #[pyo3(signature = (instrument_id, start=None, end=None, client_id=None, params=None))]
    fn py_request_instrument(
        &self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let start = start.map(UnixNanos::from);
        let end = end.map(UnixNanos::from);

        // let request_id = self.core.request_instrument::<Self>(instrument_id, start, end, client_id, params)
        //     .map_err(to_pyvalue_err)?;
        // Ok(request_id.to_string())
        Ok("placeholder".to_string()) // Placeholder for now
    }

    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (venue=None, start=None, end=None, client_id=None, params=None))]
    fn py_request_instruments(
        &self,
        venue: Option<Venue>,
        start: Option<u64>,
        end: Option<u64>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let start = start.map(UnixNanos::from);
        let end = end.map(UnixNanos::from);

        // let request_id = self.core.request_instruments::<Self>(venue, start, end, client_id, params)
        //     .map_err(to_pyvalue_err)?;
        // Ok(request_id.to_string())
        Ok("placeholder".to_string()) // Placeholder for now
    }

    #[pyo3(name = "request_book_snapshot")]
    #[pyo3(signature = (instrument_id, depth=None, client_id=None, params=None))]
    fn py_request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        depth: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let depth = depth.and_then(NonZeroUsize::new);

        // let request_id = self.core.request_book_snapshot::<Self>(instrument_id, depth, client_id, params)
        //     .map_err(to_pyvalue_err)?;
        // Ok(request_id.to_string())
        Ok("placeholder".to_string()) // Placeholder for now
    }

    #[pyo3(name = "request_quotes")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_quotes(
        &self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(UnixNanos::from);
        let end = end.map(UnixNanos::from);

        // let request_id = self.core.request_quotes::<Self>(instrument_id, start, end, limit, client_id, params)
        //     .map_err(to_pyvalue_err)?;
        // Ok(request_id.to_string())
        Ok("placeholder".to_string()) // Placeholder for now
    }

    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(UnixNanos::from);
        let end = end.map(UnixNanos::from);

        // let request_id = self.core.request_trades::<Self>(instrument_id, start, end, limit, client_id, params)
        //     .map_err(to_pyvalue_err)?;
        // Ok(request_id.to_string())
        Ok("placeholder".to_string()) // Placeholder for now
    }

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, client_id=None, params=None))]
    fn py_request_bars(
        &self,
        bar_type: BarType,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<usize>,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<String> {
        let limit = limit.and_then(NonZeroUsize::new);
        let start = start.map(UnixNanos::from);
        let end = end.map(UnixNanos::from);

        // let request_id = self.core.request_bars::<Self>(bar_type, start, end, limit, client_id, params)
        //     .map_err(to_pyvalue_err)?;
        // Ok(request_id.to_string())
        Ok("placeholder".to_string()) // Placeholder for now
    }
}

// TODO: WIP
impl Actor for PyDataActor {
    fn id(&self) -> ustr::Ustr {
        self.inner
            .as_ref()
            .map(|a| a.id())
            .unwrap_or_else(|| ustr::ustr("PyDataActor-Unregistered"))
    }

    fn handle(&mut self, msg: &dyn std::any::Any) {
        if let Some(inner) = &mut self.inner {
            inner.handle(msg)
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl DataActor for PyDataActor {
    fn state(&self) -> ComponentState {
        self.inner
            .as_ref()
            .map(|a| a.state())
            .unwrap_or(ComponentState::PreInitialized)
    }
}
