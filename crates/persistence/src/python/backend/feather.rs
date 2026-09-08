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

#![expect(
    clippy::too_many_lines,
    reason = "PyO3 writer constructor mirrors Python keyword surface"
)]

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use nautilus_common::{
    clock::Clock,
    live::{block_on_nautilus_with, get_runtime},
    msgbus::typed_handler::ShareableMessageHandler,
    python::{cache::PyCache, clock::PyClock},
};
use nautilus_core::{UnixNanos, datetime::get_timezone, python::to_pyruntime_err};
use nautilus_model::{
    data::{
        Bar, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
        OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick, close::InstrumentClose,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSnapshot, OrderSubmitted, OrderTriggered, OrderUpdated, PositionAdjusted,
        PositionChanged, PositionClosed, PositionOpened, PositionSnapshot,
    },
    python::instruments::pyobject_to_instrument_any,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use object_store::ObjectStoreExt;
use pyo3::{exceptions::PyIOError, prelude::*};

use crate::{
    common::storage::{StorageBackend, create_storage_backend_from_path},
    python::backend::writer_record_filter_from_py,
    writer::feather::{FeatherWriter, RotationConfig, WriterClock},
};

/// Source clock plus the shared atomic the writer reads time from.
type ClockBridge = (Rc<RefCell<dyn Clock>>, Arc<AtomicU64>);

/// Python binding for the Rust `FeatherWriter`.
///
/// This provides a streaming writer of Nautilus objects into feather files with rotation
/// capabilities, matching the interface of Python's `StreamingFeatherWriter`.
#[pyclass(
    name = "StreamingFeatherWriter",
    module = "nautilus_trader.persistence",
    unsendable
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")]
pub struct PyStreamingFeatherWriter {
    writer: Rc<RefCell<FeatherWriter>>,
    handler: Option<ShareableMessageHandler>,
    run_manifest: Option<(StorageBackend, String, String)>,
    run_manifest_has_data: RefCell<bool>,
    /// Present when constructed with a non-live clock: the source clock plus the
    /// shared atomic the core writer reads, refreshed before each forwarded call.
    /// Note: writes arriving via the message bus subscription do not refresh the
    /// bridge; live usage pairs the subscription with a `LiveClock`.
    clock_bridge: Option<ClockBridge>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyStreamingFeatherWriter {
    /// Creates a new `StreamingFeatherWriter` instance.
    ///
    /// # Parameters
    ///
    /// - `path`: The path to persist the stream to. Must be a directory.
    /// - `cache`: The cache for query info (`PyCache`).
    /// - `clock`: The clock to use for time-related operations (`PyClock`).
    /// - `fs_protocol`: Optional filesystem protocol (default: "file").
    /// - `fs_storage_options`: Optional storage options for cloud backends.
    /// - `include_types`: Optional list of type names to include (e.g., `["quotes", "trades"]`).
    /// - `rotation_mode`: Rotation mode (0=SIZE, 1=INTERVAL, `2=SCHEDULED_DATES`, `3=NO_ROTATION`).
    /// - `max_file_size`: Maximum file size in bytes before rotation (for SIZE mode).
    /// - `rotation_interval_ns`: Rotation interval in nanoseconds (for `INTERVAL/SCHEDULED_DATES` modes).
    /// - `rotation_time_ns`: Scheduled rotation time in nanoseconds (for `SCHEDULED_DATES` mode).
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
        record_types=None,
        record_filters=None,
        rotation_mode=3,
        max_file_size=1_073_741_824,
        rotation_interval_ns=None,
        rotation_time_ns=None,
        rotation_timezone="UTC",
        flush_interval_ms=None,
        replace=false
    ))]
    #[expect(
        clippy::too_many_arguments,
        clippy::needless_pass_by_value,
        reason = "PyO3 constructor mirrors the writer configuration fields"
    )]
    pub fn py_new(
        path: String,
        cache: PyCache,
        clock: PyClock,
        fs_protocol: Option<&str>,
        fs_storage_options: Option<HashMap<String, String>>,
        include_types: Option<Vec<String>>,
        record_types: Option<&Bound<'_, PyAny>>,
        record_filters: Option<&Bound<'_, PyAny>>,
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
                path
            }
        } else {
            path
        };

        let storage_options = fs_storage_options
            .map(|map| map.into_iter().collect::<ahash::AHashMap<String, String>>());

        if replace
            && url::Url::parse(&full_path).is_ok_and(|url| {
                !matches!(url.scheme(), "file" | "memory")
                    && url.path().trim_matches('/').is_empty()
            })
        {
            return Err(PyIOError::new_err(
                "replace=True for remote streaming paths requires a non-empty prefix",
            ));
        }

        let storage = create_storage_backend_from_path(&full_path, storage_options)
            .map_err(|e| PyIOError::new_err(format!("Failed to create storage backend: {e}")))?;
        let object_store = storage.object_store.clone();

        // Handle replace parameter - delete existing files if requested
        if replace {
            let store_ref = object_store.clone();
            let base_path = storage.base_path.clone();
            block_on_nautilus_with(move || async move {
                let prefix = if base_path.is_empty() {
                    None
                } else {
                    Some(object_store::path::Path::from(
                        base_path.trim_start_matches('/'),
                    ))
                };
                let mut stream = store_ref.list(prefix.as_ref());
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
            .map_err(|e| PyIOError::new_err(format!("Failed to replace existing files: {e}")))?;
        }

        let run_manifest =
            if let Some((kind, instance_id)) = run_kind_and_instance_id_from_path(&full_path) {
                let manifest_storage = storage.clone();
                let manifest_kind = kind.clone();
                let manifest_instance_id = instance_id.clone();
                block_on_nautilus_with(move || async move {
                    manifest_storage
                        .write_current_run_manifest(
                            &manifest_kind,
                            &manifest_instance_id,
                            "in_progress",
                            true,
                        )
                        .await
                })
                .map_err(|e| PyIOError::new_err(format!("Failed to write run manifest: {e}")))?;
                Some((storage.clone(), kind, instance_id))
            } else {
                None
            };

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
                let tz = get_timezone(rotation_timezone).map_err(|e| {
                    PyIOError::new_err(format!("Failed to parse rotation_timezone: {e}"))
                })?;
                let time_ns = rotation_time_ns.unwrap_or(0);
                RotationConfig::ScheduledDates {
                    interval_ns: interval,
                    rotation_time: UnixNanos::from(time_ns),
                    rotation_timezone: tz,
                }
            }
            _ => RotationConfig::NoRotation, // Default to no rotation for invalid values
        };

        // Convert include_types to HashSet
        let type_filter = include_types.map(|types| types.into_iter().collect::<HashSet<String>>());
        let record_filter = writer_record_filter_from_py(record_types, record_filters)?;

        // Set up per-instrument types (matching Python's _per_instrument_writers)
        let mut per_instrument_types = HashSet::new();
        per_instrument_types.insert("bars".to_string());
        per_instrument_types.insert("order_book_deltas".to_string());
        per_instrument_types.insert("order_book_depths".to_string());
        per_instrument_types.insert("option_greeks".to_string());
        per_instrument_types.insert("quotes".to_string());
        per_instrument_types.insert("trades".to_string());
        per_instrument_types.insert("mark_prices".to_string());
        per_instrument_types.insert("index_prices".to_string());
        per_instrument_types.insert("funding_rates".to_string());

        // Extract Clock from Python wrapper and translate it into the core
        // writer's Send time source (live clocks read the wall clock directly;
        // test clocks are bridged through a shared atomic)
        let clock_rc = clock.clock_rc();
        let (writer_clock, shared_time) = WriterClock::from_shared_clock(&clock_rc);
        // Note: Cache parameter is kept for API compatibility with Python StreamingFeatherWriter
        // but is not directly used by FeatherWriter
        let _cache = cache;

        // Create FeatherWriter
        let writer = FeatherWriter::new(
            storage.base_path,
            object_store,
            writer_clock,
            rotation_config,
            type_filter,
            Some(per_instrument_types),
            flush_interval_ms, // Auto-flush interval in milliseconds
        )
        .with_record_filter(record_filter);

        Ok(Self {
            writer: Rc::new(RefCell::new(writer)),
            handler: None,
            run_manifest,
            run_manifest_has_data: RefCell::new(false),
            clock_bridge: shared_time.map(|shared| (clock_rc, shared)),
        })
    }

    /// Subscribes to all messages on the message bus (pattern "*").
    ///
    /// This matches the behavior of Python's `StreamingFeatherWriter` when subscribed
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
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 writer binding must downcast supported data variants inline"
    )]
    pub fn write(&self, py: Python, data: Py<PyAny>) -> PyResult<()> {
        self.refresh_writer_clock();

        macro_rules! try_write {
            ($type:ty, $name:literal) => {
                if let Ok(value) = data.extract::<$type>(py) {
                    let result =
                        self.writer.borrow_mut().write(value).map_err(|e| {
                            PyIOError::new_err(format!("Failed to write {}: {e}", $name))
                        });
                    return self.finish_write_result(result);
                }
            };
        }

        macro_rules! try_write_data {
            ($data:expr, $name:literal) => {{
                let result = self
                    .writer
                    .borrow_mut()
                    .write_data($data)
                    .map_err(|e| PyIOError::new_err(format!("Failed to write {}: {e}", $name)));
                return self.finish_write_result(result);
            }};
        }

        // Try to convert from common pyo3 data types
        if let Ok(quote) = data.extract::<QuoteTick>(py) {
            try_write_data!(Data::Quote(quote), "QuoteTick");
        }

        if let Ok(trade) = data.extract::<TradeTick>(py) {
            try_write_data!(Data::Trade(trade), "TradeTick");
        }

        if let Ok(bar) = data.extract::<Bar>(py) {
            try_write_data!(Data::Bar(bar), "Bar");
        }

        if let Ok(delta) = data.extract::<OrderBookDelta>(py) {
            try_write_data!(Data::BookDelta(delta), "OrderBookDelta");
        }

        if let Ok(depth) = data.extract::<OrderBookDepth>(py) {
            try_write_data!(Data::BookDepth(Box::new(depth)), "OrderBookDepth");
        }

        if let Ok(price) = data.extract::<IndexPriceUpdate>(py) {
            try_write_data!(Data::IndexPrice(price), "IndexPriceUpdate");
        }

        if let Ok(price) = data.extract::<MarkPriceUpdate>(py) {
            try_write_data!(Data::MarkPrice(price), "MarkPriceUpdate");
        }

        if let Ok(greeks) = data.extract::<OptionGreeks>(py) {
            try_write_data!(Data::OptionGreeks(greeks), "OptionGreeks");
        }

        if let Ok(close) = data.extract::<InstrumentClose>(py) {
            try_write_data!(Data::InstrumentClose(close), "InstrumentClose");
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
        try_write!(OrderFillVoided, "OrderFillVoided");
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
            let result = self
                .writer
                .borrow_mut()
                .write_instrument(instrument)
                .map_err(|e| PyIOError::new_err(format!("Failed to write instrument: {e}")));
            return self.finish_write_result(result);
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
        self.refresh_writer_clock();
        let mut writer = self.writer.borrow_mut();

        block_on_local("flush StreamingFeatherWriter", || async {
            writer.flush().await
        })?
        .map_err(|e| PyIOError::new_err(format!("Failed to flush: {e}")))
    }

    /// Closes all writers by flushing and removing them.
    ///
    /// After calling this, no further writes should be performed.
    pub fn close(&self) -> PyResult<()> {
        self.refresh_writer_clock();
        let mut writer = self.writer.borrow_mut();

        block_on_local("close StreamingFeatherWriter", || async {
            writer.close().await
        })?
        .map_err(|e| PyIOError::new_err(format!("Failed to close: {e}")))?;
        drop(writer);
        self.write_run_manifest(
            "completed",
            !*self.run_manifest_has_data.borrow(),
            "complete",
        )
    }

    /// Returns whether the writer has been closed (no active writers).
    #[getter]
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.writer.borrow().is_closed()
    }

    /// Returns information about the current files being written.
    ///
    /// Returns a dictionary mapping writer keys to (size, path) tuples.
    #[must_use]
    pub fn get_current_file_info(&self) -> HashMap<String, (u64, String)> {
        self.writer.borrow().get_current_file_info()
    }

    /// Returns the next rotation time for a writer, or None if not set.
    #[pyo3(signature = (type_str, instrument_id=None))]
    #[must_use]
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

impl PyStreamingFeatherWriter {
    // Pushes the source clock's current time into the core writer's shared
    // atomic so test clocks drive flush/rotation cadence correctly.
    fn refresh_writer_clock(&self) {
        if let Some((clock, shared)) = &self.clock_bridge {
            shared.store(clock.borrow().timestamp_ns().as_u64(), Ordering::Relaxed);
        }
    }

    fn finish_write_result(&self, result: PyResult<()>) -> PyResult<()> {
        result?;
        self.mark_run_non_empty()
    }

    fn mark_run_non_empty(&self) -> PyResult<()> {
        if *self.run_manifest_has_data.borrow() {
            return Ok(());
        }
        self.write_run_manifest("in_progress", false, "update")?;
        *self.run_manifest_has_data.borrow_mut() = true;
        Ok(())
    }

    fn write_run_manifest(&self, status: &str, empty: bool, operation: &str) -> PyResult<()> {
        let Some((storage, kind, instance_id)) = &self.run_manifest else {
            return Ok(());
        };

        let storage = storage.clone();
        let kind = kind.clone();
        let instance_id = instance_id.clone();
        let status = status.to_string();
        block_on_nautilus_with(move || async move {
            storage
                .write_current_run_manifest(&kind, &instance_id, &status, empty)
                .await
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to {operation} run manifest: {e}")))
    }
}

fn block_on_local<C, F>(operation: &str, create_future: C) -> PyResult<F::Output>
where
    C: FnOnce() -> F,
    F: std::future::Future,
{
    let run = move || get_runtime().block_on(async move { create_future().await });
    if tokio::runtime::Handle::try_current().is_err() {
        return Ok(run());
    }

    Err(to_pyruntime_err(format!(
        "Cannot {operation} from an active Tokio runtime"
    )))
}

fn run_kind_and_instance_id_from_path(path: &str) -> Option<(String, String)> {
    let parsed_url = url::Url::parse(path).ok();
    let path = parsed_url
        .as_ref()
        .map_or(path, |url| url.path().trim_start_matches('/'));
    let components: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    let instance_id = components.last()?;
    let kind = components.get(components.len().checked_sub(2)?)?;

    match *kind {
        "backtest" | "live" | "sandbox" => Some(((*kind).to_string(), (*instance_id).to_string())),
        _ => None,
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_types,
    reason = "tests exercise direct Tokio LocalSet interoperability"
)]
mod tests {
    use rstest::rstest;

    use super::block_on_local;

    #[rstest]
    fn block_on_local_rejects_current_thread_runtime() {
        pyo3::Python::initialize();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let error = runtime
            .block_on(async { block_on_local("run test operation", || async { 42 }).unwrap_err() });

        assert_eq!(
            error.to_string(),
            "RuntimeError: Cannot run test operation from an active Tokio runtime"
        );
    }

    #[rstest]
    fn block_on_local_rejects_multi_thread_local_set() {
        pyo3::Python::initialize();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let local_set = tokio::task::LocalSet::new();

        let error = runtime.block_on(local_set.run_until(async {
            block_on_local("run test operation", || async { 42 }).unwrap_err()
        }));

        assert_eq!(
            error.to_string(),
            "RuntimeError: Cannot run test operation from an active Tokio runtime"
        );
    }
}
