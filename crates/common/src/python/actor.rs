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

use std::{
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
};

use indexmap::IndexMap;
use nautilus_core::{
    nanos::UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    data::{BarType, DataType},
    enums::BookType,
    identifiers::{ActorId, ClientId, InstrumentId, TraderId, Venue},
};
use pyo3::{exceptions::PyValueError, prelude::*};

use crate::{
    actor::{
        DataActor,
        data_actor::{DataActorConfig, DataActorCore},
    },
    component::Component,
    enums::ComponentState,
};

#[allow(non_camel_case_types)]
#[pyo3::pyclass(
    module = "nautilus_trader.core.nautilus_pyo3.common",
    name = "DataActor",
    unsendable,
    subclass
)]
#[derive(Debug)]
pub struct PyDataActor {
    core: DataActorCore,
}

impl Deref for PyDataActor {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for PyDataActor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl DataActor for PyDataActor {}

#[pymethods]
impl PyDataActor {
    #[new]
    #[pyo3(signature = (_config=None))]
    fn py_new(_config: Option<PyObject>) -> PyResult<Self> {
        // TODO: Parse config from Python if provided
        let config = DataActorConfig::default();

        Ok(Self {
            core: DataActorCore::new(config),
        })
    }

    #[getter]
    #[pyo3(name = "actor_id")]
    fn py_actor_id(&self) -> ActorId {
        self.actor_id
    }

    #[getter]
    #[pyo3(name = "trader_id")]
    fn py_trader_id(&self) -> Option<TraderId> {
        self.trader_id()
    }

    #[pyo3(name = "state")]
    fn py_state(&self) -> ComponentState {
        self.state()
    }

    #[pyo3(name = "is_ready")]
    fn py_is_ready(&self) -> bool {
        self.is_ready()
    }

    #[pyo3(name = "is_running")]
    fn py_is_running(&self) -> bool {
        self.is_running()
    }

    #[pyo3(name = "is_stopped")]
    fn py_is_stopped(&self) -> bool {
        self.is_stopped()
    }

    #[pyo3(name = "is_degraded")]
    fn py_is_degraded(&self) -> bool {
        self.is_degraded()
    }

    #[pyo3(name = "is_faulted")]
    fn py_is_faulted(&self) -> bool {
        self.is_faulted()
    }

    #[pyo3(name = "is_disposed")]
    fn py_is_disposed(&self) -> bool {
        self.is_disposed()
    }

    #[pyo3(name = "start")]
    fn py_start(&mut self) -> PyResult<()> {
        self.start().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "stop")]
    fn py_stop(&mut self) -> PyResult<()> {
        self.stop().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "resume")]
    fn py_resume(&mut self) -> PyResult<()> {
        self.resume().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "reset")]
    fn py_reset(&mut self) -> PyResult<()> {
        self.reset().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "dispose")]
    fn py_dispose(&mut self) -> PyResult<()> {
        self.dispose().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "degrade")]
    fn py_degrade(&mut self) -> PyResult<()> {
        self.degrade().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "fault")]
    fn py_fault(&mut self) -> PyResult<()> {
        self.fault().map_err(to_pyruntime_err)
    }

    #[pyo3(name = "shutdown_system")]
    #[pyo3(signature = (reason=None))]
    fn py_shutdown_system(&self, reason: Option<String>) -> PyResult<()> {
        self.core.shutdown_system(reason);
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
        self.subscribe_data(data_type, client_id, params);
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
        self.subscribe_instruments(venue, client_id, params);
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
        self.subscribe_instrument(instrument_id, client_id, params);
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
        self.subscribe_book_deltas(instrument_id, book_type, depth, client_id, managed, params);
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

        self.subscribe_book_at_interval(
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
        self.subscribe_quotes(instrument_id, client_id, params);
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
        self.subscribe_trades(instrument_id, client_id, params);
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
        self.subscribe_bars(bar_type, client_id, await_partial, params);
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
        self.subscribe_mark_prices(instrument_id, client_id, params);
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
        self.subscribe_index_prices(instrument_id, client_id, params);
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
        self.subscribe_instrument_status(instrument_id, client_id, params);
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
        self.subscribe_instrument_close(instrument_id, client_id, params);
        Ok(())
    }

    // Request methods
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

        let request_id = self
            .request_data(data_type, client_id, start, end, limit, params)
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

        let request_id = self
            .request_instrument(instrument_id, start, end, client_id, params)
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

        let request_id = self
            .request_instruments(venue, start, end, client_id, params)
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

        let request_id = self
            .request_book_snapshot(instrument_id, depth, client_id, params)
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

        let request_id = self
            .request_quotes(instrument_id, start, end, limit, client_id, params)
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

        let request_id = self
            .request_trades(instrument_id, start, end, limit, client_id, params)
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

        let request_id = self
            .request_bars(bar_type, start, end, limit, client_id, params)
            .map_err(to_pyvalue_err)?;
        Ok(request_id.to_string())
    }

    // Unsubscribe methods
    #[pyo3(name = "unsubscribe_data")]
    #[pyo3(signature = (data_type, client_id=None, params=None))]
    fn py_unsubscribe_data(
        &mut self,
        data_type: DataType,
        client_id: Option<ClientId>,
        params: Option<IndexMap<String, String>>,
    ) -> PyResult<()> {
        self.unsubscribe_data(data_type, client_id, params);
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
        self.unsubscribe_instruments(venue, client_id, params);
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
        self.unsubscribe_instrument(instrument_id, client_id, params);
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
        self.unsubscribe_book_deltas(instrument_id, client_id, params);
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

        self.unsubscribe_book_at_interval(instrument_id, interval_ms, client_id, params);
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
        self.unsubscribe_quotes(instrument_id, client_id, params);
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
        self.unsubscribe_trades(instrument_id, client_id, params);
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
        self.unsubscribe_bars(bar_type, client_id, params);
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
        self.unsubscribe_mark_prices(instrument_id, client_id, params);
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
        self.unsubscribe_index_prices(instrument_id, client_id, params);
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
        self.unsubscribe_instrument_status(instrument_id, client_id, params);
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
        self.unsubscribe_instrument_close(instrument_id, client_id, params);
        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, str::FromStr};

    use nautilus_model::{
        data::{BarType, DataType},
        enums::BookType,
        identifiers::{ClientId, TraderId, Venue},
        instruments::{CurrencyPair, stubs::audusd_sim},
    };
    use rstest::{fixture, rstest};

    use super::PyDataActor;
    use crate::{
        cache::Cache,
        clock::TestClock,
        component::Component,
        enums::ComponentState,
        runner::{SyncDataCommandExecutor, set_data_cmd_executor},
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
        PyDataActor::py_new(None).unwrap()
    }

    fn create_registered_actor(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) -> PyDataActor {
        // Set up sync data command executor for tests
        let executor = SyncDataCommandExecutor;
        set_data_cmd_executor(Rc::new(RefCell::new(executor)));

        let mut actor = PyDataActor::py_new(None).unwrap();
        actor.register(trader_id, clock, cache).unwrap();
        actor
    }

    #[rstest]
    fn test_new_actor_creation() {
        let actor = PyDataActor::py_new(None).unwrap();
        assert!(actor.trader_id().is_none());
    }

    #[rstest]
    fn test_unregistered_actor_methods_work(data_type: DataType, client_id: ClientId) {
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

        let _ = actor.py_subscribe_data(data_type.clone(), Some(client_id.clone()), None);
        let _ = actor.py_subscribe_quotes(audusd_sim.id, Some(client_id.clone()), None);
        let _ = actor.py_unsubscribe_data(data_type, Some(client_id.clone()), None);
        let _ = actor.py_unsubscribe_quotes(audusd_sim.id, Some(client_id), None);
    }

    #[rstest]
    fn test_lifecycle_methods_pass_through(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        let mut actor = create_registered_actor(clock, cache, trader_id);

        assert!(actor.py_start().is_ok());
        assert!(actor.py_stop().is_ok());
        assert!(actor.py_dispose().is_ok());
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
        pyo3::prepare_freethreaded_python();

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

        // These methods exist and compile correctly
        // Verify it's unregistered
        assert!(actor.trader_id().is_none());
    }

    #[rstest]
    fn test_data_actor_trait_implementation(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        let actor = create_registered_actor(clock, cache, trader_id);

        // Test Component trait method (using the trait method directly)
        let state = actor.state();
        assert_eq!(state, ComponentState::Ready);
    }
}
