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
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use nautilus_common::{
    clock::Clock,
    enums::Environment,
    live::block_on_nautilus_with,
    python::{cache::PyCache, clock::PyClock},
};
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::{
        Bar, CustomData, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick,
        close::InstrumentClose,
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
use pyo3::{exceptions::PyIOError, prelude::*};

use crate::{
    common::{
        paths::{environment_from_directory, local_writer_directory, normalize_path_separators},
        storage::{StorageBackend, create_storage_backend_from_path},
    },
    config::RotationConfig,
    python::{
        backend::{PyCatalogDataType, catalog_filter_family_from_py, writer_record_filter_from_py},
        config::PyRotationConfig,
    },
    writer::{
        factory::{WriterConnectConfig, replace_existing_writer_data},
        feather::{FeatherWriter, WriterClock, recover_partial_feather_files},
        run::RunStatus,
        subscription::StreamingSinkSubscription,
    },
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
    handler: Option<StreamingSinkSubscription>,
    run_manifest: Option<(StorageBackend, Environment, String)>,
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
    /// - `path`: The local directory to append the stream files to.
    /// - `cache`: The cache for query info (`PyCache`).
    /// - `clock`: The clock to use for time-related operations (`PyClock`).
    /// - `include_types`: Optional data or record types to include, as `NautilusDataType` or
    ///   `NautilusRecordType` values or their catalog names (e.g., `["quotes", "trades"]`).
    /// - `rotation_config`: File rotation policy (default: no rotation).
    /// - `flush_interval_ms`: Interval in milliseconds for flushing open files to disk (default:
    ///   1000). Set to 0 to disable auto-flush.
    /// - `replace`: If existing files at the given path should be replaced (default: False).
    #[new]
    #[pyo3(signature = (
        path,
        cache,
        clock,
        include_types=None,
        record_types=None,
        record_filters=None,
        rotation_config=None,
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
        include_types: Option<Vec<Bound<'_, PyAny>>>,
        record_types: Option<&Bound<'_, PyAny>>,
        record_filters: Option<&Bound<'_, PyAny>>,
        rotation_config: Option<PyRotationConfig>,
        flush_interval_ms: Option<u64>,
        replace: bool,
    ) -> PyResult<Self> {
        let directory =
            local_writer_directory(&path).map_err(|e| PyIOError::new_err(e.to_string()))?;
        let rotation_config = rotation_config
            .map_or(RotationConfig::NoRotation, Into::into)
            .to_writer_rotation_config()
            .map_err(to_pyvalue_err)?;

        if replace {
            replace_existing_writer_data(&WriterConnectConfig::new(path.clone(), None)).map_err(
                |e| PyIOError::new_err(format!("Failed to replace existing files: {e}")),
            )?;
        }

        recover_partial_feather_files(&directory);

        let storage = create_storage_backend_from_path(&path, None)
            .map_err(|e| PyIOError::new_err(format!("Failed to create storage backend: {e}")))?;

        let run_manifest = if let Some((environment, instance_id)) =
            run_environment_and_instance_id_from_path(&path)
        {
            let manifest_storage = storage.clone();
            let manifest_instance_id = instance_id.clone();
            block_on_nautilus_with(move || async move {
                manifest_storage
                    .write_current_run_manifest(
                        environment,
                        &manifest_instance_id,
                        RunStatus::InProgress,
                        true,
                    )
                    .await
            })
            .map_err(|e| PyIOError::new_err(format!("Failed to write run manifest: {e}")))?;

            Some((storage, environment, instance_id))
        } else {
            None
        };

        let type_filter = include_types
            .map(|types| {
                types
                    .iter()
                    .map(catalog_filter_family_from_py)
                    .collect::<PyResult<HashSet<_>>>()
            })
            .transpose()?;

        let record_filter = writer_record_filter_from_py(record_types, record_filters)?;

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
            directory,
            writer_clock,
            rotation_config,
            type_filter,
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

        if let Ok(custom) = data.extract::<CustomData>(py) {
            try_write_data!(Data::Custom(custom), "CustomData");
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

    /// Flushes buffered bytes of every open file to disk.
    ///
    /// This is called automatically based on `flush_interval_ms` if configured, but can also
    /// be called manually by the client. Files stay open, so flushing never starts a new file.
    pub fn flush(&self) -> PyResult<()> {
        self.refresh_writer_clock();
        self.writer
            .borrow_mut()
            .flush()
            .map_err(|e| PyIOError::new_err(format!("Failed to flush: {e}")))
    }

    /// Seals all open files so each complete stream is visible as a `.feather` file.
    ///
    /// Writes after closing open new files.
    pub fn close(&self) -> PyResult<()> {
        self.refresh_writer_clock();
        self.writer
            .borrow_mut()
            .close()
            .map_err(|e| PyIOError::new_err(format!("Failed to close: {e}")))?;

        self.write_run_manifest(
            RunStatus::Completed,
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

    /// Returns the current file size and path of each open file, keyed by the type it stages.
    #[must_use]
    pub fn get_current_file_info(&self) -> HashMap<PyCatalogDataType, (u64, String)> {
        self.writer
            .borrow()
            .get_current_file_info()
            .into_iter()
            .map(|(data_type, info)| (PyCatalogDataType::new(data_type), info))
            .collect()
    }

    /// Returns the next rotation time of a type's file, or None if not set.
    ///
    /// Pass a `NautilusInstrumentType` for the file of one instrument class.
    #[must_use]
    pub fn get_next_rotation_time(&self, data_type: PyCatalogDataType) -> Option<u64> {
        self.writer
            .borrow()
            .get_next_rotation_time(&data_type.into_inner())
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

        self.write_run_manifest(RunStatus::InProgress, false, "update")?;
        *self.run_manifest_has_data.borrow_mut() = true;
        Ok(())
    }

    fn write_run_manifest(&self, status: RunStatus, empty: bool, operation: &str) -> PyResult<()> {
        let Some((storage, environment, instance_id)) = &self.run_manifest else {
            return Ok(());
        };

        let storage = storage.clone();
        let environment = *environment;
        let instance_id = instance_id.clone();
        block_on_nautilus_with(move || async move {
            storage
                .write_current_run_manifest(environment, &instance_id, status, empty)
                .await
        })
        .map_err(|e| PyIOError::new_err(format!("Failed to {operation} run manifest: {e}")))
    }
}

fn run_environment_and_instance_id_from_path(path: &str) -> Option<(Environment, String)> {
    let normalized = normalize_path_separators(path);
    let parsed_url = url::Url::parse(&normalized).ok();

    let path = parsed_url.as_ref().map_or(normalized.as_str(), |url| {
        url.path().trim_start_matches('/')
    });

    let components: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|component| !component.is_empty())
        .collect();
    let instance_id = components.last()?;
    let environment =
        environment_from_directory(components.get(components.len().checked_sub(2)?)?)?;

    Some((environment, (*instance_id).to_string()))
}

#[cfg(test)]
mod tests {
    use nautilus_common::enums::Environment;
    use rstest::rstest;

    use super::run_environment_and_instance_id_from_path;

    #[rstest]
    #[case(
        r"C:\Users\Administrator\AppData\Local\Temp\pytest-0\backtest\run-greeks",
        Environment::Backtest,
        "run-greeks"
    )]
    #[case("C:/catalog/backtest/run-1", Environment::Backtest, "run-1")]
    #[case(r"\\server\share\live\run-2", Environment::Live, "run-2")]
    #[case("/tmp/catalog/sandbox/run-3", Environment::Sandbox, "run-3")]
    #[case("file:///C:/catalog/backtest/run-1", Environment::Backtest, "run-1")]
    fn run_environment_and_instance_id_handles_platform_paths(
        #[case] path: &str,
        #[case] environment: Environment,
        #[case] instance_id: &str,
    ) {
        assert_eq!(
            run_environment_and_instance_id_from_path(path),
            Some((environment, instance_id.to_string())),
        );
    }

    #[rstest]
    fn run_environment_and_instance_id_rejects_non_run_paths() {
        assert_eq!(
            run_environment_and_instance_id_from_path(r"C:\catalog\data\quotes"),
            None
        );
        assert_eq!(
            run_environment_and_instance_id_from_path("/tmp/catalog"),
            None
        );
    }
}
