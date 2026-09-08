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

//! Generic backend-selected streaming writer, mirroring the catalog backend dispatch.

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, atomic::AtomicU64},
};

use nautilus_common::{clock::Clock, python::clock::PyClock};
use nautilus_model::data::{
    Bar, CustomData, Data, IndexPriceUpdate, InstrumentClose, MarkPriceUpdate, OrderBookDelta,
    OrderBookDepth, QuoteTick, TradeTick,
};
use pyo3::{exceptions::PyIOError, prelude::*};

use crate::{
    backend::default_writer_factories,
    writer::{
        factory::{WriterBackendType, WriterConnectConfig, create_writer},
        feather::WriterClock,
        traits::StreamingSinkBox,
    },
};

/// Source clock plus the shared atomic the writer reads time from.
type ClockBridge = (Rc<RefCell<dyn Clock>>, Arc<AtomicU64>);

/// Python binding for the backend-selected streaming writer.
///
/// Resolves the writer through the persistence writer-factory registry, mirroring
/// `writer_backend` selection: `Feather`, `Parquet`, or a registered name.
#[pyclass(
    name = "StreamingWriter",
    module = "nautilus_trader.persistence",
    unsendable
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyStreamingWriter {
    sink: Rc<RefCell<StreamingSinkBox>>,
    backend: String,
    /// Present when constructed with a non-live clock: the source clock plus the
    /// shared atomic the core writer reads, refreshed before each forwarded call.
    clock_bridge: Option<ClockBridge>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyStreamingWriter {
    /// Creates a streaming writer for the given backend name.
    #[new]
    #[pyo3(signature = (backend, path, clock, storage_options=None))]
    #[expect(clippy::needless_pass_by_value)]
    pub fn py_new(
        backend: &str,
        path: String,
        clock: PyClock,
        storage_options: Option<HashMap<String, String>>,
    ) -> PyResult<Self> {
        let backend_type = backend
            .parse::<WriterBackendType>()
            .map_err(|e| PyIOError::new_err(format!("Invalid writer backend: {e}")))?;
        let clock_rc = clock.clock_rc();
        let (writer_clock, shared_time) = WriterClock::from_shared_clock(&clock_rc);
        let config = WriterConnectConfig::new(
            path,
            storage_options.map(|options| options.into_iter().collect()),
        );
        let sink = create_writer(
            &backend_type,
            &config,
            writer_clock,
            &default_writer_factories(),
        )
        .map_err(|e| PyIOError::new_err(format!("Failed to create writer: {e}")))?;

        Ok(Self {
            sink: Rc::new(RefCell::new(sink)),
            backend: backend_type.to_string(),
            clock_bridge: shared_time.map(|shared| (clock_rc, shared)),
        })
    }

    /// Returns the resolved writer backend name.
    #[getter]
    #[must_use]
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Writes a single Nautilus data value.
    pub fn write(&self, py: Python, data: Py<PyAny>) -> PyResult<()> {
        let data = pyobject_to_data(py, data)?;
        self.refresh_writer_clock();
        self.sink
            .borrow_mut()
            .write_data(data)
            .map_err(|e| PyIOError::new_err(format!("Failed to write data: {e}")))
    }

    /// Flushes buffered data to durable storage.
    pub fn flush(&self) -> PyResult<()> {
        self.refresh_writer_clock();
        self.sink
            .borrow_mut()
            .flush()
            .map_err(|e| PyIOError::new_err(format!("Failed to flush writer: {e}")))
    }

    /// Closes the writer after flushing buffered data.
    pub fn close(&self) -> PyResult<()> {
        self.refresh_writer_clock();
        self.sink
            .borrow_mut()
            .close()
            .map_err(|e| PyIOError::new_err(format!("Failed to close writer: {e}")))
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 transfers ownership of the Python object into this conversion boundary"
)]
pub(crate) fn pyobject_to_data(py: Python, data: Py<PyAny>) -> PyResult<Data> {
    let data = data.bind(py);

    if data.is_instance_of::<QuoteTick>() {
        return Ok(Data::Quote(data.extract::<QuoteTick>()?));
    }

    if data.is_instance_of::<TradeTick>() {
        return Ok(Data::Trade(data.extract::<TradeTick>()?));
    }

    if data.is_instance_of::<Bar>() {
        return Ok(Data::Bar(data.extract::<Bar>()?));
    }

    if data.is_instance_of::<OrderBookDelta>() {
        return Ok(Data::BookDelta(data.extract::<OrderBookDelta>()?));
    }

    if data.is_instance_of::<OrderBookDepth>() {
        return Ok(Data::BookDepth(Box::new(data.extract::<OrderBookDepth>()?)));
    }

    if data.is_instance_of::<IndexPriceUpdate>() {
        return Ok(Data::IndexPrice(data.extract::<IndexPriceUpdate>()?));
    }

    if data.is_instance_of::<MarkPriceUpdate>() {
        return Ok(Data::MarkPrice(data.extract::<MarkPriceUpdate>()?));
    }

    if data.is_instance_of::<InstrumentClose>() {
        return Ok(Data::InstrumentClose(data.extract::<InstrumentClose>()?));
    }

    if data.is_instance_of::<CustomData>() {
        return Ok(Data::Custom(data.extract::<CustomData>()?));
    }

    Err(PyIOError::new_err(
        "Unsupported data type for streaming writer",
    ))
}

impl PyStreamingWriter {
    // Pushes the source clock's current time into the shared atomic so test clocks
    // advanced from Python are observed by the core writer.
    fn refresh_writer_clock(&self) {
        if let Some((clock, shared)) = &self.clock_bridge {
            shared.store(
                clock.borrow().timestamp_ns().as_u64(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }
}
