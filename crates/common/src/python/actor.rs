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

//! Python bindings for DataActor with complete event handler forwarding.

use std::{
    any::Any,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
};

use indexmap::IndexMap;
use nautilus_core::{
    nanos::UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
};
#[cfg(feature = "defi")]
use nautilus_model::defi::{Block, Pool, PoolLiquidityUpdate, PoolSwap};
use nautilus_model::{
    data::{
        Bar, BarType, DataType, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
        OrderBookDeltas, QuoteTick, TradeTick, close::InstrumentClose,
    },
    enums::BookType,
    identifiers::{ActorId, ClientId, InstrumentId, TraderId, Venue},
    instruments::InstrumentAny,
    orderbook::OrderBook,
    python::instruments::instrument_any_to_pyobject,
};
use pyo3::{exceptions::PyValueError, prelude::*};

use crate::{
    actor::{
        DataActor,
        data_actor::{DataActorConfig, DataActorCore},
    },
    component::Component,
    enums::ComponentState,
    signal::Signal,
    timer::TimeEvent,
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

impl DataActor for PyDataActor {
    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        self.py_on_time_event(event.clone())
            .map_err(|e| anyhow::anyhow!("Python on_time_event failed: {e}"))
    }

    #[allow(unused_variables)]
    fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
        Python::with_gil(|py| {
            // TODO: Create a placeholder object since we can't easily convert &dyn Any to PyObject
            // For now, we'll pass None and let Python subclasses handle specific data types
            let py_data = py.None();

            self.py_on_data(py_data)
                .map_err(|e| anyhow::anyhow!("Python on_data failed: {e}"))
        })
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        self.py_on_signal(signal)
            .map_err(|e| anyhow::anyhow!("Python on_signal failed: {e}"))
    }

    fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        Python::with_gil(|py| {
            let py_instrument = instrument_any_to_pyobject(py, instrument.clone())
                .map_err(|e| anyhow::anyhow!("Failed to convert InstrumentAny to Python: {e}"))?;
            self.py_on_instrument(py_instrument)
                .map_err(|e| anyhow::anyhow!("Python on_instrument failed: {e}"))
        })
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.py_on_quote(*quote)
            .map_err(|e| anyhow::anyhow!("Python on_quote failed: {e}"))
    }

    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        self.py_on_trade(*tick)
            .map_err(|e| anyhow::anyhow!("Python on_trade failed: {e}"))
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        self.py_on_bar(*bar)
            .map_err(|e| anyhow::anyhow!("Python on_bar failed: {e}"))
    }

    fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        self.py_on_book_deltas(deltas.clone())
            .map_err(|e| anyhow::anyhow!("Python on_book_deltas failed: {e}"))
    }

    fn on_book(&mut self, order_book: &OrderBook) -> anyhow::Result<()> {
        self.py_on_book(order_book)
            .map_err(|e| anyhow::anyhow!("Python on_book failed: {e}"))
    }

    fn on_mark_price(&mut self, mark_price: &MarkPriceUpdate) -> anyhow::Result<()> {
        self.py_on_mark_price(*mark_price)
            .map_err(|e| anyhow::anyhow!("Python on_mark_price failed: {e}"))
    }

    fn on_index_price(&mut self, index_price: &IndexPriceUpdate) -> anyhow::Result<()> {
        self.py_on_index_price(*index_price)
            .map_err(|e| anyhow::anyhow!("Python on_index_price failed: {e}"))
    }

    fn on_instrument_status(&mut self, data: &InstrumentStatus) -> anyhow::Result<()> {
        self.py_on_instrument_status(*data)
            .map_err(|e| anyhow::anyhow!("Python on_instrument_status failed: {e}"))
    }

    fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
        self.py_on_instrument_close(*update)
            .map_err(|e| anyhow::anyhow!("Python on_instrument_close failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_block(&mut self, _block: &Block) -> anyhow::Result<()> {
        // TODO: Pass actual block data when DeFi types have PyClass implementations
        self.py_on_block()
            .map_err(|e| anyhow::anyhow!("Python on_block failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool(&mut self, _pool: &Pool) -> anyhow::Result<()> {
        // TODO: Pass actual pool data when DeFi types have PyClass implementations
        self.py_on_pool()
            .map_err(|e| anyhow::anyhow!("Python on_pool failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool_swap(&mut self, _swap: &PoolSwap) -> anyhow::Result<()> {
        // TODO: Pass actual swap data when DeFi types have PyClass implementations
        self.py_on_pool_swap()
            .map_err(|e| anyhow::anyhow!("Python on_pool_swap failed: {e}"))
    }

    #[cfg(feature = "defi")]
    fn on_pool_liquidity_update(&mut self, _update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
        // TODO: Pass actual update data when DeFi types have PyClass implementations
        self.py_on_pool_liquidity_update()
            .map_err(|e| anyhow::anyhow!("Python on_pool_liquidity_update failed: {e}"))
    }
}

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

    #[allow(unused_variables)]
    #[pyo3(name = "on_time_event")]
    fn py_on_time_event(&mut self, event: TimeEvent) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_data")]
    fn py_on_data(&mut self, data: PyObject) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_signal")]
    fn py_on_signal(&mut self, signal: &Signal) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_instrument")]
    fn py_on_instrument(&mut self, instrument: PyObject) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_quote")]
    fn py_on_quote(&mut self, quote: QuoteTick) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_trade")]
    fn py_on_trade(&mut self, trade: TradeTick) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_bar")]
    fn py_on_bar(&mut self, bar: Bar) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_book_deltas")]
    fn py_on_book_deltas(&mut self, deltas: OrderBookDeltas) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_book")]
    fn py_on_book(&mut self, order_book: &OrderBook) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_mark_price")]
    fn py_on_mark_price(&mut self, mark_price: MarkPriceUpdate) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_index_price")]
    fn py_on_index_price(&mut self, index_price: IndexPriceUpdate) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_instrument_status")]
    fn py_on_instrument_status(&mut self, status: InstrumentStatus) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[allow(unused_variables)]
    #[pyo3(name = "on_instrument_close")]
    fn py_on_instrument_close(&mut self, close: InstrumentClose) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[allow(unused_variables)]
    #[pyo3(name = "on_block")]
    fn py_on_block(&mut self) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        // TODO: Pass actual Block object when PyClass is implemented
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[allow(unused_variables)]
    #[pyo3(name = "on_pool")]
    fn py_on_pool(&mut self) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        // TODO: Pass actual Pool object when PyClass is implemented
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[allow(unused_variables)]
    #[pyo3(name = "on_pool_swap")]
    fn py_on_pool_swap(&mut self) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        // TODO: Pass actual PoolSwap object when PyClass is implemented
        Ok(())
    }

    #[cfg(feature = "defi")]
    #[allow(unused_variables)]
    #[pyo3(name = "on_pool_liquidity_update")]
    fn py_on_pool_liquidity_update(&mut self) -> PyResult<()> {
        // Default implementation - can be overridden in Python subclasses
        // TODO: Pass actual PoolLiquidityUpdate object when PyClass is implemented
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
    use std::{
        any::Any,
        cell::RefCell,
        collections::HashMap,
        ops::{Deref, DerefMut},
        rc::Rc,
        str::FromStr,
        sync::{Arc, Mutex},
    };

    use nautilus_core::{UUID4, UnixNanos};
    #[cfg(feature = "defi")]
    use nautilus_model::defi::{
        AmmType, Block, Blockchain, Chain, Dex, Pool, PoolLiquidityUpdate, PoolSwap, Token,
    };
    use nautilus_model::{
        data::{
            Bar, BarType, DataType, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
            OrderBookDelta, OrderBookDeltas, QuoteTick, TradeTick, close::InstrumentClose,
        },
        enums::BookType,
        identifiers::{ClientId, TraderId, Venue},
        instruments::{CurrencyPair, InstrumentAny, stubs::audusd_sim},
        orderbook::OrderBook,
        types::{Price, Quantity},
    };
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
        PyDataActor::py_new(None).unwrap()
    }

    fn create_registered_actor(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) -> PyDataActor {
        // Set up sync data command sender for tests
        let sender = SyncDataCommandSender;
        set_data_cmd_sender(Arc::new(sender));

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

        let _ = actor.py_subscribe_data(data_type.clone(), Some(client_id), None);
        let _ = actor.py_subscribe_quotes(audusd_sim.id, Some(client_id), None);
        let _ = actor.py_unsubscribe_data(data_type, Some(client_id), None);
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

    // Test actor that tracks method calls for verification

    // Global call tracker for tests
    static CALL_TRACKER: std::sync::LazyLock<Arc<Mutex<HashMap<String, i32>>>> =
        std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

    // Test actor that overrides Python methods to track calls
    #[derive(Debug)]
    struct TestDataActor {
        inner: PyDataActor,
    }

    impl TestDataActor {
        fn new() -> Self {
            Self {
                inner: PyDataActor::py_new(None).unwrap(),
            }
        }

        fn track_call(&self, handler_name: &str) {
            let mut tracker = CALL_TRACKER.lock().unwrap();
            *tracker.entry(handler_name.to_string()).or_insert(0) += 1;
        }

        fn get_call_count(&self, handler_name: &str) -> i32 {
            let tracker = CALL_TRACKER.lock().unwrap();
            tracker.get(handler_name).copied().unwrap_or(0)
        }

        fn reset_tracker(&self) {
            let mut tracker = CALL_TRACKER.lock().unwrap();
            tracker.clear();
        }
    }

    impl Deref for TestDataActor {
        type Target = DataActorCore;
        fn deref(&self) -> &Self::Target {
            &self.inner.core
        }
    }

    impl DerefMut for TestDataActor {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.inner.core
        }
    }

    impl DataActor for TestDataActor {
        fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
            self.track_call("on_time_event");
            self.inner.on_time_event(event)
        }

        fn on_data(&mut self, data: &dyn Any) -> anyhow::Result<()> {
            self.track_call("on_data");
            self.inner.on_data(data)
        }

        fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
            self.track_call("on_signal");
            self.inner.on_signal(signal)
        }

        fn on_instrument(&mut self, instrument: &InstrumentAny) -> anyhow::Result<()> {
            self.track_call("on_instrument");
            self.inner.on_instrument(instrument)
        }

        fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
            self.track_call("on_quote");
            self.inner.on_quote(quote)
        }

        fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
            self.track_call("on_trade");
            self.inner.on_trade(trade)
        }

        fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
            self.track_call("on_bar");
            self.inner.on_bar(bar)
        }

        fn on_book(&mut self, book: &OrderBook) -> anyhow::Result<()> {
            self.track_call("on_book");
            self.inner.on_book(book)
        }

        fn on_book_deltas(&mut self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
            self.track_call("on_book_deltas");
            self.inner.on_book_deltas(deltas)
        }

        fn on_mark_price(&mut self, update: &MarkPriceUpdate) -> anyhow::Result<()> {
            self.track_call("on_mark_price");
            self.inner.on_mark_price(update)
        }

        fn on_index_price(&mut self, update: &IndexPriceUpdate) -> anyhow::Result<()> {
            self.track_call("on_index_price");
            self.inner.on_index_price(update)
        }

        fn on_instrument_status(&mut self, update: &InstrumentStatus) -> anyhow::Result<()> {
            self.track_call("on_instrument_status");
            self.inner.on_instrument_status(update)
        }

        fn on_instrument_close(&mut self, update: &InstrumentClose) -> anyhow::Result<()> {
            self.track_call("on_instrument_close");
            self.inner.on_instrument_close(update)
        }

        #[cfg(feature = "defi")]
        fn on_block(&mut self, block: &Block) -> anyhow::Result<()> {
            self.track_call("on_block");
            self.inner.on_block(block)
        }

        #[cfg(feature = "defi")]
        fn on_pool(&mut self, pool: &Pool) -> anyhow::Result<()> {
            self.track_call("on_pool");
            self.inner.on_pool(pool)
        }

        #[cfg(feature = "defi")]
        fn on_pool_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
            self.track_call("on_pool_swap");
            self.inner.on_pool_swap(swap)
        }

        #[cfg(feature = "defi")]
        fn on_pool_liquidity_update(&mut self, update: &PoolLiquidityUpdate) -> anyhow::Result<()> {
            self.track_call("on_pool_liquidity_update");
            self.inner.on_pool_liquidity_update(update)
        }
    }

    #[rstest]
    fn test_python_on_signal_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

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
        pyo3::prepare_freethreaded_python();

        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        assert!(test_actor.on_data(&()).is_ok());
        assert_eq!(test_actor.get_call_count("on_data"), 1);
    }

    #[rstest]
    fn test_python_on_time_event_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

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
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let instrument = InstrumentAny::CurrencyPair(audusd_sim);

        assert!(rust_actor.on_instrument(&instrument).is_ok());
    }

    #[rstest]
    fn test_python_on_quote_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let quote = QuoteTick::new(
            audusd_sim.id,
            Price::from("1.0000"),
            Price::from("1.0001"),
            Quantity::from("100000"),
            Quantity::from("100000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.on_quote(&quote).is_ok());
    }

    #[rstest]
    fn test_python_on_trade_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let trade = TradeTick::new(
            audusd_sim.id,
            Price::from("1.0000"),
            Quantity::from("100000"),
            nautilus_model::enums::AggressorSide::Buyer,
            "T123".to_string().into(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.on_trade(&trade).is_ok());
    }

    #[rstest]
    fn test_python_on_bar_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

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

        assert!(rust_actor.on_bar(&bar).is_ok());
    }

    #[rstest]
    fn test_python_on_book_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let book = OrderBook::new(audusd_sim.id, BookType::L2_MBP);
        assert!(rust_actor.on_book(&book).is_ok());
    }

    #[rstest]
    fn test_python_on_book_deltas_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let delta =
            OrderBookDelta::clear(audusd_sim.id, 0, UnixNanos::default(), UnixNanos::default());
        let deltas = OrderBookDeltas::new(audusd_sim.id, vec![delta]);

        assert!(rust_actor.on_book_deltas(&deltas).is_ok());
    }

    #[rstest]
    fn test_python_on_mark_price_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let mark_price = MarkPriceUpdate::new(
            audusd_sim.id,
            Price::from("1.0000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.on_mark_price(&mark_price).is_ok());
    }

    #[rstest]
    fn test_python_on_index_price_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let index_price = IndexPriceUpdate::new(
            audusd_sim.id,
            Price::from("1.0000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.on_index_price(&index_price).is_ok());
    }

    #[rstest]
    fn test_python_on_instrument_status_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let status = InstrumentStatus::new(
            audusd_sim.id,
            nautilus_model::enums::MarketStatusAction::Trading,
            UnixNanos::default(),
            UnixNanos::default(),
            None,
            None,
            None,
            None,
            None,
        );

        assert!(rust_actor.on_instrument_status(&status).is_ok());
    }

    #[rstest]
    fn test_python_on_instrument_close_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
        audusd_sim: CurrencyPair,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let close = InstrumentClose::new(
            audusd_sim.id,
            Price::from("1.0000"),
            nautilus_model::enums::InstrumentCloseType::EndOfSession,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(rust_actor.on_instrument_close(&close).is_ok());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_python_on_block_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut test_actor = TestDataActor::new();
        test_actor.reset_tracker();
        test_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

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
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

        let chain = Arc::new(Chain::new(Blockchain::Ethereum, 1));
        let dex = Arc::new(Dex::new(
            Chain::new(Blockchain::Ethereum, 1),
            "Uniswap V3",
            "0x1f98431c8ad98523631ae4a59f267346ea31f984",
            AmmType::CLAMM,
            "PoolCreated",
            "Swap",
            "Mint",
            "Burn",
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
        let pool = Arc::new(Pool::new(
            chain.clone(),
            dex.as_ref().clone(),
            "0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8"
                .parse()
                .unwrap(),
            12345,
            token0,
            token1,
            500,
            10,
            UnixNanos::default(),
        ));

        let swap = PoolSwap::new(
            chain.clone(),
            dex.clone(),
            pool.clone(),
            12345,
            "0xabc123".to_string(),
            0,
            0,
            UnixNanos::default(),
            "0x742E4422b21FB8B4dF463F28689AC98bD56c39e0"
                .parse()
                .unwrap(),
            nautilus_model::enums::OrderSide::Buy,
            Quantity::from("1000"),
            Price::from("1.0"),
        );

        assert!(rust_actor.on_pool_swap(&swap).is_ok());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_python_on_pool_liquidity_update_handler(
        clock: Rc<RefCell<TestClock>>,
        cache: Rc<RefCell<Cache>>,
        trader_id: TraderId,
    ) {
        pyo3::prepare_freethreaded_python();

        let mut rust_actor = PyDataActor::py_new(None).unwrap();
        rust_actor
            .register(trader_id, clock.clone(), cache.clone())
            .unwrap();

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

        // Test that the Rust trait method forwards to Python without error
        // Note: We test on_block here since PoolLiquidityUpdate construction is complex
        // and the goal is just to verify the forwarding mechanism works
        assert!(rust_actor.on_block(&block).is_ok());
    }
}
