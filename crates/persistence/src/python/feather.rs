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

//! Python bindings for the Rust `FeatherWriter` as `StreamingFeatherWriter`.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use nautilus_common::{
    live::get_runtime,
    msgbus::typed_handler::ShareableMessageHandler,
    python::{cache::PyCache, clock::PyClock},
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
        OrderBookDelta, OrderBookDepth10, QuoteTick, TradeTick, close::InstrumentClose,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSnapshot,
        OrderSubmitted, OrderTriggered, OrderUpdated, PositionAdjusted, PositionChanged,
        PositionClosed, PositionOpened, PositionSnapshot,
    },
    python::instruments::pyobject_to_instrument_any,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use object_store::ObjectStoreExt;
use pyo3::{exceptions::PyIOError, prelude::*};

use crate::{
    backend::feather::{FeatherWriter, RotationConfig},
    parquet::create_object_store_from_path,
};

/// Python binding for the Rust `FeatherWriter`.
///
/// This provides a streaming writer of Nautilus objects into feather files with rotation
/// capabilities, matching the interface of Python's `StreamingFeatherWriter`.
#[pyclass(
    name = "StreamingFeatherWriter",
    module = "nautilus_trader.core.nautilus_pyo3.persistence",
    unsendable
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyStreamingFeatherWriter {
    writer: Rc<RefCell<FeatherWriter>>,
    handler: Option<ShareableMessageHandler>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyStreamingFeatherWriter {
    /// Creates a new `StreamingFeatherWriter` instance.
    ///
    /// # Parameters
    ///
    /// - `path`: The path to persist the stream to. Must be a directory.
    /// - `cache`: The cache for query info (PyCache).
    /// - `clock`: The clock to use for time-related operations (PyClock).
    /// - `fs_protocol`: Optional filesystem protocol (default: "file").
    /// - `fs_storage_options`: Optional storage options for cloud backends.
    /// - `include_types`: Optional list of type names to include (e.g., ["quotes", "trades"]).
    /// - `rotation_mode`: Rotation mode (0=SIZE, 1=INTERVAL, 2=SCHEDULED_DATES, 3=NO_ROTATION).
    /// - `max_file_size`: Maximum file size in bytes before rotation (for SIZE mode).
    /// - `rotation_interval_ns`: Rotation interval in nanoseconds (for INTERVAL/SCHEDULED_DATES modes).
    /// - `rotation_time_ns`: Scheduled rotation time in nanoseconds (for SCHEDULED_DATES mode).
    /// - `flush_interval_ms`: Flush interval in milliseconds (default: 1000). Set to 0 to disable auto-flush.
    /// - `replace`: If existing files at the given path should be replaced (default: False).
    #[new]
    #[pyo3(signature = (
        path,
        cache,
        clock,
        fs_protocol=None,
        fs_storage_options=None,
        include_types=None,
        rotation_mode=3,
        max_file_size=1024*1024*1024,
        rotation_interval_ns=None,
        rotation_time_ns=None,
        rotation_timezone="UTC",
        flush_interval_ms=None,
        replace=false
    ))]
    #[expect(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    pub fn new(
        path: String,
        cache: PyCache,
        clock: PyClock,
        fs_protocol: Option<&str>,
        fs_storage_options: Option<HashMap<String, String>>,
        include_types: Option<Vec<String>>,
        rotation_mode: u8,
        max_file_size: u64,
        rotation_interval_ns: Option<u64>,
        rotation_time_ns: Option<u64>,
        rotation_timezone: &str,
        flush_interval_ms: Option<u64>,
        replace: bool,
    ) -> PyResult<Self> {
        // Create object store from path
        // Use fs_protocol to construct the full path if it's a cloud protocol
        let full_path = if let Some(protocol) = fs_protocol {
            if protocol != "file" && !path.contains("://") {
                format!("{protocol}://{path}")
            } else {
                path.clone()
            }
        } else {
            path.clone()
        };

        let storage_options = fs_storage_options
            .map(|map| map.into_iter().collect::<ahash::AHashMap<String, String>>());

        let (object_store, _base_path, _original_uri) =
            create_object_store_from_path(&full_path, storage_options)
                .map_err(|e| PyIOError::new_err(format!("Failed to create object store: {e}")))?;

        // Handle replace parameter - delete existing files if requested
        if replace {
            let runtime = get_runtime();
            let store_ref = object_store.clone();
            runtime
                .block_on(async {
                    let prefix =
                        object_store::path::Path::from(path.trim_start_matches('/').to_string());
                    let mut stream = store_ref.list(Some(&prefix));
                    let mut to_delete = Vec::new();

                    while let Some(result) = futures::StreamExt::next(&mut stream).await {
                        if let Ok(meta) = result {
                            to_delete.push(meta.location);
                        }
                    }

                    for path in to_delete {
                        let _ = store_ref.delete(&path).await;
                    }
                    Ok::<(), anyhow::Error>(())
                })
                .map_err(|e| {
                    PyIOError::new_err(format!("Failed to replace existing files: {e}"))
                })?;
        }

        // Convert rotation mode to RotationConfig
        // Python RotationMode: 0=SIZE, 1=INTERVAL, 2=SCHEDULED_DATES, 3=NO_ROTATION
        let rotation_config = match rotation_mode {
            0 => RotationConfig::Size {
                max_size: max_file_size,
            },
            1 => {
                let interval = rotation_interval_ns.unwrap_or(86_400_000_000_000); // Default 1 day
                RotationConfig::Interval {
                    interval_ns: interval,
                }
            }
            2 => {
                let interval = rotation_interval_ns.unwrap_or(86_400_000_000_000); // Default 1 day
                let tz = rotation_timezone.parse::<chrono_tz::Tz>().map_err(|e| {
                    PyIOError::new_err(format!("Failed to parse rotation_timezone: {e}"))
                })?;
                let time_ns = rotation_time_ns.unwrap_or(0);
                RotationConfig::ScheduledDates {
                    interval_ns: interval,
                    rotation_time: UnixNanos::from(time_ns),
                    rotation_timezone: tz,
                }
            }
            3 => RotationConfig::NoRotation,
            _ => RotationConfig::NoRotation, // Default to no rotation for invalid values
        };

        // Convert include_types to HashSet
        let included_types =
            include_types.map(|types| types.into_iter().collect::<HashSet<String>>());

        // Set up per-instrument types (matching Python's _per_instrument_writers)
        let mut per_instrument_types = HashSet::new();
        per_instrument_types.insert("bars".to_string());
        per_instrument_types.insert("order_book_deltas".to_string());
        per_instrument_types.insert("order_book_depths".to_string());
        per_instrument_types.insert("quotes".to_string());
        per_instrument_types.insert("trades".to_string());

        // Extract Clock from Python wrapper
        // PyClock wraps Rc<RefCell<dyn Clock>>, we get the inner Rc
        let clock_rc = clock.clock_rc();
        // Note: Cache parameter is kept for API compatibility with Python StreamingFeatherWriter
        // but is not directly used by FeatherWriter
        let _cache = cache;

        // Create FeatherWriter
        let writer = FeatherWriter::new(
            path,
            object_store,
            clock_rc,
            rotation_config,
            included_types,
            Some(per_instrument_types),
            flush_interval_ms, // Auto-flush interval in milliseconds
        );

        Ok(Self {
            writer: Rc::new(RefCell::new(writer)),
            handler: None,
        })
    }

    /// Subscribes to all messages on the message bus (pattern "*").
    ///
    /// This matches the behavior of Python's StreamingFeatherWriter when subscribed
    /// via `trader.subscribe("*", writer.write)`.
    pub fn subscribe(&mut self) -> PyResult<()> {
        if self.handler.is_some() {
            // Already subscribed
            return Ok(());
        }

        let handler = FeatherWriter::subscribe_to_message_bus(self.writer.clone())
            .map_err(|e| PyIOError::new_err(format!("Failed to subscribe to message bus: {e}")))?;

        self.handler = Some(handler);
        Ok(())
    }

    /// Unsubscribes from the message bus.
    pub fn unsubscribe(&mut self) -> PyResult<()> {
        if let Some(handler) = self.handler.take() {
            FeatherWriter::unsubscribe_from_message_bus(&handler);
        }
        Ok(())
    }

    /// Writes a data object to the stream.
    ///
    /// # Parameters
    ///
    /// - `data`: The data object to write (must be a Nautilus data type from pyo3).
    ///
    #[expect(clippy::needless_pass_by_value)]
    pub fn write(&self, py: Python, data: Py<PyAny>) -> PyResult<()> {
        macro_rules! try_write {
            ($type:ty, $name:literal) => {
                if let Ok(value) = data.extract::<$type>(py) {
                    let mut writer = self.writer.borrow_mut();
                    let runtime = get_runtime();
                    return runtime
                        .block_on(async { writer.write(value).await })
                        .map_err(|e| {
                            PyIOError::new_err(format!("Failed to write {}: {e}", $name))
                        });
                }
            };
        }

        // Try to convert from common pyo3 data types
        if let Ok(quote) = data.extract::<QuoteTick>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::Quote(quote)).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write QuoteTick: {e}")));
        }

        if let Ok(trade) = data.extract::<TradeTick>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::Trade(trade)).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write TradeTick: {e}")));
        }

        if let Ok(bar) = data.extract::<Bar>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::Bar(bar)).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write Bar: {e}")));
        }

        if let Ok(delta) = data.extract::<OrderBookDelta>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::Delta(delta)).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write OrderBookDelta: {e}")));
        }

        if let Ok(depth) = data.extract::<OrderBookDepth10>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::Depth10(Box::new(depth))).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write OrderBookDepth10: {e}")));
        }

        if let Ok(price) = data.extract::<IndexPriceUpdate>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::IndexPriceUpdate(price)).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write IndexPriceUpdate: {e}")));
        }

        if let Ok(price) = data.extract::<MarkPriceUpdate>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::MarkPriceUpdate(price)).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write MarkPriceUpdate: {e}")));
        }

        if let Ok(close) = data.extract::<InstrumentClose>(py) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_data(Data::InstrumentClose(close)).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write InstrumentClose: {e}")));
        }

        try_write!(FundingRateUpdate, "FundingRateUpdate");
        try_write!(InstrumentStatus, "InstrumentStatus");
        try_write!(AccountState, "AccountState");
        try_write!(OrderInitialized, "OrderInitialized");
        try_write!(OrderDenied, "OrderDenied");
        try_write!(OrderEmulated, "OrderEmulated");
        try_write!(OrderSubmitted, "OrderSubmitted");
        try_write!(OrderAccepted, "OrderAccepted");
        try_write!(OrderRejected, "OrderRejected");
        try_write!(OrderPendingCancel, "OrderPendingCancel");
        try_write!(OrderCanceled, "OrderCanceled");
        try_write!(OrderCancelRejected, "OrderCancelRejected");
        try_write!(OrderExpired, "OrderExpired");
        try_write!(OrderTriggered, "OrderTriggered");
        try_write!(OrderPendingUpdate, "OrderPendingUpdate");
        try_write!(OrderReleased, "OrderReleased");
        try_write!(OrderModifyRejected, "OrderModifyRejected");
        try_write!(OrderUpdated, "OrderUpdated");
        try_write!(OrderFilled, "OrderFilled");
        try_write!(PositionOpened, "PositionOpened");
        try_write!(PositionChanged, "PositionChanged");
        try_write!(PositionClosed, "PositionClosed");
        try_write!(PositionAdjusted, "PositionAdjusted");
        try_write!(OrderSnapshot, "OrderSnapshot");
        try_write!(PositionSnapshot, "PositionSnapshot");
        try_write!(OrderStatusReport, "OrderStatusReport");
        try_write!(FillReport, "FillReport");
        try_write!(PositionStatusReport, "PositionStatusReport");
        try_write!(ExecutionMassStatus, "ExecutionMassStatus");

        // Try instrument types (uses type_str attribute for dispatch)
        if let Ok(instrument) = pyobject_to_instrument_any(py, data.clone_ref(py)) {
            let mut writer = self.writer.borrow_mut();
            let runtime = get_runtime();
            return runtime
                .block_on(async { writer.write_instrument(instrument).await })
                .map_err(|e| PyIOError::new_err(format!("Failed to write instrument: {e}")));
        }

        Err(PyIOError::new_err(
            "Unsupported data type for feather writer",
        ))
    }

    /// Flushes all active buffers by writing any remaining buffered bytes to the object store.
    ///
    /// This is called automatically based on `flush_interval_ms` if configured, but can also
    /// be called manually by the client.
    pub fn flush(&self) -> PyResult<()> {
        let mut writer = self.writer.borrow_mut();
        let runtime = get_runtime();

        runtime
            .block_on(async { writer.flush().await })
            .map_err(|e| PyIOError::new_err(format!("Failed to flush: {e}")))
    }

    /// Closes all writers by flushing and removing them.
    ///
    /// After calling this, no further writes should be performed.
    pub fn close(&self) -> PyResult<()> {
        let mut writer = self.writer.borrow_mut();
        let runtime = get_runtime();

        runtime
            .block_on(async { writer.close().await })
            .map_err(|e| PyIOError::new_err(format!("Failed to close: {e}")))
    }

    /// Returns whether the writer has been closed (no active writers).
    #[getter]
    pub fn is_closed(&self) -> bool {
        self.writer.borrow().is_closed()
    }

    /// Returns information about the current files being written.
    ///
    /// Returns a dictionary mapping writer keys to (size, path) tuples.
    pub fn get_current_file_info(&self) -> HashMap<String, (u64, String)> {
        self.writer.borrow().get_current_file_info()
    }

    /// Returns the next rotation time for a writer, or None if not set.
    #[pyo3(signature = (type_str, instrument_id=None))]
    pub fn get_next_rotation_time(
        &self,
        type_str: &str,
        instrument_id: Option<&str>,
    ) -> Option<u64> {
        self.writer
            .borrow()
            .get_next_rotation_time(type_str, instrument_id)
            .map(|ns| ns.as_u64())
    }
}
