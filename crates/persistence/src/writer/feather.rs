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

#![expect(
    clippy::missing_errors_doc,
    reason = "Feather writer public methods forward encoding and object-store errors directly"
)]

use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use datafusion::arrow::{
    array::StringArray,
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    ipc::writer::StreamWriter,
    record_batch::RecordBatch,
};
use jiff::{
    SignedDuration,
    civil::Time,
    tz::{AmbiguousOffset, TimeZone},
};
use nautilus_common::{
    clock::Clock,
    live::{LiveClock, block_on_nautilus_with},
    msgbus::{mstr::MStr, subscribe_any, typed_handler::ShareableMessageHandler, unsubscribe_any},
};
use nautilus_core::{UnixNanos, time::nanos_since_unix_epoch};
use nautilus_model::{
    data::{
        Bar, CustomData, CustomDataTrait, Data, DataBatch, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDeltas, OrderBookDepth, QuoteTick,
        TradeTick, close::InstrumentClose, encode_custom_to_arrow, get_arrow_schema,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSnapshot, OrderSubmitted, OrderTriggered, OrderUpdated, PositionAdjusted,
        PositionChanged, PositionClosed, PositionEvent, PositionOpened, PositionSnapshot,
    },
    instruments::InstrumentAny,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use nautilus_serialization::arrow::{
    EncodeToRecordBatch, KEY_INSTRUMENT_ID, catalog_identifier_from_metadata,
    record_batch_with_identifier_column, schema_with_identifier_column,
};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};

use crate::{
    common::{
        custom::{augment_batch_with_data_type_column, schema_with_data_type_column},
        paths::{CatalogPathPrefix, urisafe_instrument_id},
    },
    writer::{filter::WriterRecordFilter, traits::StreamingDataSink},
};

pub(crate) type FeatherWriteCommand =
    Box<dyn FnOnce(&mut FeatherWriter) -> Result<(), Box<dyn std::error::Error>> + Send + 'static>;

#[expect(
    clippy::needless_pass_by_value,
    reason = "map_err transfers ownership of the boxed writer error"
)]
pub(crate) fn feather_error(e: Box<dyn std::error::Error>) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

macro_rules! define_builtin_data_batch_dispatch {
    ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
        fn write_builtin_data_batch(
            writer: &mut FeatherWriter,
            batch: &DataBatch,
        ) -> Option<Result<(), Box<dyn std::error::Error>>> {
            match batch {
                $(
                    DataBatch::$batch(data) => Some(writer.write_batch(data.as_ref().to_vec())),
                )+
                _ => None,
            }
        }
    };
}

nautilus_model::for_each_data_type!(define_builtin_data_batch_dispatch);

pub(crate) const NAUTILUS_ARROW_METADATA_ID_COLUMN: &str = "nautilus_metadata_id";
pub(crate) const NAUTILUS_ARROW_METADATA_JSON_COLUMN: &str = "nautilus_metadata_json";

#[derive(Clone, Debug, Eq, PartialEq)]
struct StagedArrowMetadataRow {
    metadata_id: String,
    metadata_json: String,
}

#[derive(Debug, Default, PartialEq, PartialOrd, Hash, Eq, Clone)]
pub struct FileWriterPath {
    path: Path,
    type_str: String,
    instrument_id: Option<String>,
}

/// Minimal `Send` time source for streaming writers.
///
/// `Live` reads the wall clock directly; `Test` reads a shared atomic that the
/// owner of the source clock (e.g. a PyO3 wrapper holding a `TestClock`)
/// refreshes before forwarding writer calls.
#[derive(Clone, Debug)]
pub enum WriterClock {
    /// Wall-clock time via [`nanos_since_unix_epoch`].
    Live,
    /// Externally driven time stored in a shared atomic (nanoseconds since the UNIX epoch).
    Test(Arc<AtomicU64>),
}

impl WriterClock {
    /// Returns the current timestamp in nanoseconds since the UNIX epoch.
    #[must_use]
    pub fn timestamp_ns(&self) -> UnixNanos {
        match self {
            Self::Live => UnixNanos::from(nanos_since_unix_epoch()),
            Self::Test(shared) => UnixNanos::from(shared.load(Ordering::Relaxed)),
        }
    }

    /// Builds a writer clock from a shared `dyn Clock`.
    ///
    /// Returns [`WriterClock::Live`] for a [`LiveClock`]. For any other clock
    /// (e.g. `TestClock`) it returns a [`WriterClock::Test`] source seeded with
    /// the clock's current time, plus the shared atomic the caller must refresh
    /// from the source clock before each forwarded writer call.
    #[must_use]
    pub fn from_shared_clock(clock: &Rc<RefCell<dyn Clock>>) -> (Self, Option<Arc<AtomicU64>>) {
        let borrowed = clock.borrow();
        let any_ref: &dyn Any = &*borrowed;
        if any_ref.downcast_ref::<LiveClock>().is_some() {
            (Self::Live, None)
        } else {
            let shared = Arc::new(AtomicU64::new(borrowed.timestamp_ns().as_u64()));
            (Self::Test(Arc::clone(&shared)), Some(shared))
        }
    }
}

fn record_batch_with_delta_staged_metadata(
    batch: &RecordBatch,
    metadata_rows: &[StagedArrowMetadataRow],
) -> anyhow::Result<RecordBatch> {
    anyhow::ensure!(
        batch.num_rows() == metadata_rows.len(),
        "Delta Feather staging metadata row count {} does not match batch row count {}",
        metadata_rows.len(),
        batch.num_rows()
    );
    anyhow::ensure!(
        batch
            .schema()
            .index_of(NAUTILUS_ARROW_METADATA_ID_COLUMN)
            .is_err(),
        "Delta Feather staging schema already has {NAUTILUS_ARROW_METADATA_ID_COLUMN}"
    );
    anyhow::ensure!(
        batch
            .schema()
            .index_of(NAUTILUS_ARROW_METADATA_JSON_COLUMN)
            .is_err(),
        "Delta Feather staging schema already has {NAUTILUS_ARROW_METADATA_JSON_COLUMN}"
    );

    let mut fields = batch
        .schema()
        .fields()
        .iter()
        .map(|field| {
            Arc::new(Field::new(
                field.name().clone(),
                field.data_type().clone(),
                field.is_nullable(),
            ))
        })
        .collect::<Vec<_>>();
    fields.push(Arc::new(Field::new(
        NAUTILUS_ARROW_METADATA_ID_COLUMN,
        DataType::Utf8,
        false,
    )));
    fields.push(Arc::new(Field::new(
        NAUTILUS_ARROW_METADATA_JSON_COLUMN,
        DataType::Utf8,
        false,
    )));

    let mut columns = batch.columns().to_vec();
    columns.push(Arc::new(StringArray::from(
        metadata_rows
            .iter()
            .map(|row| row.metadata_id.clone())
            .collect::<Vec<_>>(),
    )));
    columns.push(Arc::new(StringArray::from(
        metadata_rows
            .iter()
            .map(|row| row.metadata_json.clone())
            .collect::<Vec<_>>(),
    )));

    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(fields)),
        columns,
    )?)
}

fn arrow_metadata_row(
    metadata: &HashMap<String, String>,
    fields: &arrow::datatypes::Fields,
) -> anyhow::Result<StagedArrowMetadataRow> {
    let schema_metadata = metadata
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let field_metadata = fields
        .iter()
        .filter(|field| !field.metadata().is_empty())
        .map(|field| {
            (
                field.name().clone(),
                field
                    .metadata()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let metadata_json = serde_json::to_string(&serde_json::json!({
        "format_version": 1,
        "schema_metadata": schema_metadata,
        "field_metadata": field_metadata,
    }))?;
    let metadata_id = staged_metadata_id(&canonical_metadata_json(metadata)?);

    Ok(StagedArrowMetadataRow {
        metadata_id,
        metadata_json,
    })
}

/// Canonical schema-level metadata JSON: keys serialized in sorted order.
///
/// The staged metadata id hashes this exact string so readers can verify the schema metadata
/// independently of its storage location.
pub(crate) fn canonical_metadata_json(
    metadata: &HashMap<String, String>,
) -> anyhow::Result<String> {
    Ok(serde_json::to_string(
        &metadata
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )?)
}

pub(crate) fn staged_metadata_id(metadata_json: &str) -> String {
    format!("blake3:{}", blake3::hash(metadata_json.as_bytes()).to_hex())
}

/// A `FeatherBuffer` encodes data via an Arrow `StreamWriter`.
///
/// It flushes the internal byte buffer according to rotation policy.
pub struct FeatherBuffer {
    /// Arrow `StreamWriter` that writes to an in-memory `Vec<u8>`.
    writer: StreamWriter<Vec<u8>>,
    /// Current size in bytes.
    size: u64,
    /// Current number of buffered rows.
    rows: u64,
    /// Schema of the data being written.
    schema: Schema,
    /// Maximum buffer size in bytes.
    max_buffer_size: u64,
}

impl FeatherBuffer {
    /// Creates a new [`FeatherBuffer`] using the given path, schema and maximum buffer size.
    pub fn new(schema: &Schema, rotation_config: &RotationConfig) -> Result<Self, ArrowError> {
        let writer = StreamWriter::try_new(Vec::new(), schema)?;
        let mut max_buffer_size = 1_073_741_824; // 1 GiB fallback cap without size rotation

        if let RotationConfig::Size { max_size } = &rotation_config {
            max_buffer_size = *max_size;
        }

        Ok(Self {
            writer,
            size: 0,
            rows: 0,
            max_buffer_size,
            schema: schema.clone(),
        })
    }

    /// Writes the given `RecordBatch` to the internal buffer.
    ///
    /// Returns true if it should be rotated according rotation policy
    pub fn write_record_batch(&mut self, batch: &RecordBatch) -> Result<bool, ArrowError> {
        let batch = if batch.schema().as_ref() == &self.schema {
            batch.clone()
        } else {
            RecordBatch::try_new(Arc::new(self.schema.clone()), batch.columns().to_vec())?
        };
        self.writer.write(&batch)?;
        self.size += batch.get_array_memory_size() as u64;
        self.rows += batch.num_rows() as u64;
        Ok(self.size >= self.max_buffer_size)
    }

    /// Consumes the writer and returns the buffer of bytes from the `StreamWriter`
    pub fn take_buffer(&mut self) -> Result<Vec<u8>, ArrowError> {
        let mut writer = StreamWriter::try_new(Vec::new(), &self.schema)?;
        std::mem::swap(&mut self.writer, &mut writer);
        let buffer = writer.into_inner()?;
        self.size = 0;
        self.rows = 0;
        Ok(buffer)
    }
}

/// Deferred IO produced by the synchronous encode path.
///
/// Rotations and flushes are the only operations that touch the object store,
/// so they are collected here and executed in one runtime entry when due.
#[derive(Debug, Default)]
struct PendingIo {
    rotate_paths: Vec<FileWriterPath>,
    flush_due: bool,
}

/// Configuration for file rotation.
#[derive(Debug, Clone)]
pub enum RotationConfig {
    /// Rotate based on file size.
    Size {
        /// Maximum buffer size in bytes before rotation.
        max_size: u64,
    },
    /// Rotate based on a time interval.
    Interval {
        /// Interval in nanoseconds.
        interval_ns: u64,
    },
    /// Rotate based on scheduled dates.
    ScheduledDates {
        /// Interval in nanoseconds.
        interval_ns: u64,
        /// Time of day for rotation (nanoseconds since midnight).
        rotation_time: UnixNanos,
        /// Timezone for rotation calculations.
        rotation_timezone: TimeZone,
    },
    /// No automatic rotation.
    NoRotation,
}

impl RotationConfig {
    /// Creates scheduled rotation using UTC.
    ///
    /// This keeps timezone ownership in writer backend when callers expose
    /// only a time-of-day schedule without a timezone field.
    #[must_use]
    pub const fn scheduled_utc(interval_ns: u64, rotation_time: UnixNanos) -> Self {
        Self::ScheduledDates {
            interval_ns,
            rotation_time,
            rotation_timezone: jiff::tz::TimeZone::UTC,
        }
    }
}

/// Manages multiple `FeatherBuffers` and handles encoding, rotation, and flushing to the object store.
///
/// The `write()` method is the single entry point for clients: they supply a data value (of generic type T)
/// and the manager encodes it (using T's metadata via `EncodeToRecordBatch`), routes it by `CatalogPathPrefix`,
/// and writes it to the appropriate `FileWriter`. When a writer's buffer is full or rotation criteria are met,
/// its contents are flushed to the object store and it is replaced.
pub struct FeatherWriter {
    /// Base directory for writing files.
    base_path: String,
    /// Object store for persistence.
    store: Arc<dyn ObjectStore>,
    /// Send time source for timestamps, rotation, and flush cadence.
    clock: WriterClock,
    /// Rotation configuration.
    rotation_config: RotationConfig,
    /// Optional set of type names to include.
    included_types: Option<HashSet<String>>,
    /// Optional typed record-family filter.
    record_filter: Option<WriterRecordFilter>,
    /// Set of types that should be split by instrument.
    per_instrument_types: HashSet<String>,
    /// Map of active `FeatherBuffers` keyed by their path.
    writers: HashMap<FileWriterPath, FeatherBuffer>,
    /// Paths already handed out by this writer instance.
    reserved_paths: HashSet<Path>,
    /// Map of next rotation times keyed by their path.
    next_rotation_times: HashMap<FileWriterPath, UnixNanos>,
    /// Flush interval in milliseconds (0 = no automatic flushing).
    flush_interval_ms: u64,
    /// Last flush timestamp in nanoseconds.
    last_flush_ns: UnixNanos,
    /// Whether staged batches include the catalog row identifier column.
    catalog_identifier_column: bool,
}

impl FeatherWriter {
    /// Creates a new [`FeatherWriter`] instance.
    pub fn new(
        base_path: String,
        store: Arc<dyn ObjectStore>,
        clock: WriterClock,
        rotation_config: RotationConfig,
        included_types: Option<HashSet<String>>,
        per_instrument_types: Option<HashSet<String>>,
        flush_interval_ms: Option<u64>,
    ) -> Self {
        let flush_interval_ms = flush_interval_ms.unwrap_or(1000); // Default 1 second
        if flush_interval_ms == 0 && matches!(rotation_config, RotationConfig::NoRotation) {
            log::warn!(
                "FeatherWriter has auto-flush disabled (flush_interval_ms=0) with \
                 RotationConfig::NoRotation; buffers grow until the 1 GiB fallback cap \
                 per stream - configure size rotation or a flush interval for live use"
            );
        }
        let last_flush_ns = clock.timestamp_ns();

        Self {
            base_path,
            store,
            clock,
            rotation_config,
            included_types,
            record_filter: None,
            per_instrument_types: per_instrument_types.unwrap_or_default(),
            writers: HashMap::new(),
            reserved_paths: HashSet::new(),
            next_rotation_times: HashMap::new(),
            flush_interval_ms,
            last_flush_ns,
            catalog_identifier_column: false,
        }
    }

    /// Sets typed record-family filter for subsequent writes.
    #[must_use]
    pub fn with_record_filter(mut self, record_filter: Option<WriterRecordFilter>) -> Self {
        self.record_filter = record_filter;
        self
    }

    /// Includes the catalog row identifier column in staged batches.
    #[must_use]
    pub fn with_catalog_identifier_column(mut self) -> Self {
        self.catalog_identifier_column = true;
        self
    }

    /// Writes a single data value.
    ///
    /// This is the user entry point. The data is encoded into a `RecordBatch` and written to the
    /// appropriate `FileWriter`. The encode path is fully synchronous; the runtime is entered
    /// only when a rotation or auto-flush boundary is actually hit.
    pub fn write<T>(&mut self, data: T) -> Result<(), Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        let metadata = T::metadata(&data);
        let identifier = catalog_identifier_from_metadata(&metadata);
        let instrument_type = if T::path_prefix() == InstrumentAny::path_prefix() {
            metadata.get("class").map(String::as_str)
        } else {
            None
        };

        if !self.should_write_record(T::path_prefix(), identifier.as_deref(), instrument_type) {
            return Ok(());
        }

        let path = self.get_writer_path(&data)?;

        // Create a new FileWriter if one does not exist.
        if !self.writers.contains_key(&path) {
            self.create_writer::<T>(path.clone(), &data)?;
        }

        // Encode the data into a RecordBatch using T's encoding logic.
        let mut batch = T::encode_batch(&metadata, &[data])?;
        let stage_delta_metadata =
            self.catalog_identifier_column && T::path_prefix() != InstrumentAny::path_prefix();

        if stage_delta_metadata {
            let metadata_row = arrow_metadata_row(&metadata, batch.schema().fields())?;
            batch = record_batch_with_identifier_column(batch, identifier.as_deref())?;
            batch = record_batch_with_delta_staged_metadata(
                &batch,
                std::slice::from_ref(&metadata_row),
            )?;
        }

        // Write the RecordBatch to the appropriate FileWriter.
        let mut pending = PendingIo::default();

        self.stage_batch_write(path, &batch, &mut pending)?;
        pending.flush_due = self.flush_is_due();

        self.complete_pending_io(&pending)
    }

    /// Writes a batch of data values as one or more `RecordBatch`es.
    ///
    /// Uses `T::chunk_metadata` to derive the file schema metadata. This protects
    /// types like `OrderBookDelta` from having their file metadata poisoned by a
    /// leading sentinel row (e.g. `BookAction::Clear`, which carries
    /// `price_precision=0, size_precision=0`).
    ///
    /// Per-instrument types are partitioned by instrument so a mixed-instrument
    /// batch lands in the correct file for each instrument.
    pub fn write_batch<T>(&mut self, data: Vec<T>) -> Result<(), Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        if data.is_empty() || !self.should_write_type::<T>() {
            return Ok(());
        }

        // Group by logical writer identity. Catalog staging opts into
        // `catalog_identifier_column`, so row identifiers remain in the batch and
        // mixed-identifier input can share the type-level Feather file path.
        // Grouping on FileWriterPath would split same-instrument rows across distinct
        // timestamped paths when the writer does not yet exist under a LiveClock.
        let type_str = T::path_prefix();
        let needs_instrument = type_str == InstrumentAny::path_prefix()
            || self.per_instrument_types.contains(type_str)
            || type_str.starts_with("custom_");

        let mut groups: AHashMap<Option<String>, Vec<T>> = AHashMap::new();

        for item in data {
            let metadata = T::metadata(&item);
            let identifier = catalog_identifier_from_metadata(&metadata);
            let instrument_type = if type_str == InstrumentAny::path_prefix() {
                metadata.get("class").map(String::as_str)
            } else {
                None
            };

            if !self.should_write_record(type_str, identifier.as_deref(), instrument_type) {
                continue;
            }
            let group_identifier = if self.catalog_identifier_column
                && type_str != InstrumentAny::path_prefix()
            {
                identifier.clone()
            } else if type_str == InstrumentAny::path_prefix() || !self.catalog_identifier_column {
                needs_instrument
                        .then(|| {
                            metadata.get(KEY_INSTRUMENT_ID).cloned().ok_or_else(|| {
                                format!(
                                    "Data {type_str} expected instrument_id metadata for per instrument writer"
                                )
                            })
                        })
                        .transpose()?
            } else {
                None
            };
            groups.entry(group_identifier).or_default().push(item);
        }

        if groups.is_empty() {
            return Ok(());
        }

        let mut pending = PendingIo::default();

        for group in groups.into_values() {
            let path = self.get_writer_path(&group[0])?;
            let metadata = T::chunk_metadata(&group);

            if !self.writers.contains_key(&path) {
                self.create_writer_with_metadata::<T>(path.clone(), metadata.clone())?;
            }

            let identifier = catalog_identifier_from_metadata(&metadata);
            let stage_delta_metadata =
                self.catalog_identifier_column && type_str != InstrumentAny::path_prefix();
            let mut batch = T::encode_batch(&metadata, &group)?;
            let metadata_rows = if stage_delta_metadata {
                group
                    .iter()
                    .map(T::metadata)
                    .map(|metadata| arrow_metadata_row(&metadata, batch.schema().fields()))
                    .collect::<anyhow::Result<Vec<_>>>()?
            } else {
                Vec::new()
            };

            if stage_delta_metadata {
                batch = record_batch_with_identifier_column(batch, identifier.as_deref())?;
                batch = record_batch_with_delta_staged_metadata(&batch, &metadata_rows)?;
            }

            self.stage_batch_write(path, &batch, &mut pending)?;
        }

        pending.flush_due = self.flush_is_due();

        self.complete_pending_io(&pending)
    }

    fn stage_batch_write(
        &mut self,
        path: FileWriterPath,
        batch: &RecordBatch,
        pending: &mut PendingIo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(writer) = self.writers.get_mut(&path) {
            let should_rotate = writer.write_record_batch(batch)?;
            if should_rotate || self.check_scheduled_rotation(&path) {
                pending.rotate_paths.push(path);
            }
        }
        Ok(())
    }

    /// Returns whether the auto-flush interval has elapsed since the last flush.
    fn flush_is_due(&self) -> bool {
        if self.flush_interval_ms == 0 {
            return false; // Auto-flush disabled
        }

        let now_ns = self.clock.timestamp_ns();
        let elapsed_ms = now_ns.as_u64().saturating_sub(self.last_flush_ns.as_u64()) / 1_000_000;
        elapsed_ms >= self.flush_interval_ms
    }

    // Runs deferred rotation/flush IO; enters the runtime only when a boundary was hit.
    fn complete_pending_io(
        &mut self,
        pending: &PendingIo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if pending.rotate_paths.is_empty() && !pending.flush_due {
            return Ok(());
        }

        block_on_nautilus_with(|| async {
            for path in &pending.rotate_paths {
                self.rotate_writer(path).await.map_err(feather_error)?;
            }

            if pending.flush_due {
                self.flush().await.map_err(feather_error)?;
            }
            Ok::<(), anyhow::Error>(())
        })
        .map_err(Into::into)
    }

    fn check_scheduled_rotation(&mut self, path: &FileWriterPath) -> bool {
        match &self.rotation_config {
            RotationConfig::Interval { interval_ns } => {
                let now = self.clock.timestamp_ns();
                let next_rotation = self.next_rotation_times.get(path).copied();

                match next_rotation {
                    None => {
                        self.next_rotation_times.insert(
                            path.clone(),
                            now + nautilus_core::DurationNanos::new(*interval_ns),
                        );
                        false
                    }
                    Some(next) if now >= next => {
                        self.next_rotation_times.insert(
                            path.clone(),
                            now + nautilus_core::DurationNanos::new(*interval_ns),
                        );
                        true
                    }
                    _ => false,
                }
            }
            RotationConfig::ScheduledDates {
                interval_ns,
                rotation_time,
                rotation_timezone,
            } => {
                let now = self.clock.timestamp_ns();
                let next_rotation = self.next_rotation_times.get(path).copied();

                match next_rotation {
                    None => {
                        let next = self.calculate_next_scheduled_rotation(
                            *rotation_time,
                            rotation_timezone,
                            *interval_ns,
                        );
                        self.next_rotation_times.insert(path.clone(), next);
                        false
                    }
                    Some(next) if now >= next => {
                        let next = self.calculate_next_scheduled_rotation(
                            *rotation_time,
                            rotation_timezone,
                            *interval_ns,
                        );
                        self.next_rotation_times.insert(path.clone(), next);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn calculate_next_scheduled_rotation(
        &self,
        rotation_time: UnixNanos,
        rotation_timezone: &TimeZone,
        interval_ns: u64,
    ) -> UnixNanos {
        let now_utc = self.clock.timestamp_ns().to_datetime_utc();
        let now_local = rotation_timezone.to_datetime(now_utc);

        let rotation_time_secs = u32::try_from(*rotation_time / 1_000_000_000).unwrap_or(0);
        let rotation_time_nanos = i32::try_from(*rotation_time % 1_000_000_000).unwrap_or(0);
        let rotation_time = if rotation_time_secs < 86_400 {
            Time::new(
                i8::try_from(rotation_time_secs / 3_600).unwrap_or(0),
                i8::try_from(rotation_time_secs % 3_600 / 60).unwrap_or(0),
                i8::try_from(rotation_time_secs % 60).unwrap_or(0),
                rotation_time_nanos,
            )
            .unwrap_or(Time::MIN)
        } else {
            Time::MIN
        };

        let local_rotation = now_local.date().to_datetime(rotation_time);
        let ambiguous = rotation_timezone.to_ambiguous_timestamp(local_rotation);
        let mut next_rotation = match ambiguous.offset() {
            AmbiguousOffset::Gap { .. } => now_utc,
            _ => ambiguous.earlier().unwrap_or(now_utc),
        };

        if next_rotation <= now_utc {
            // If the time has already passed today, we would usually add the interval
            // But let's align exactly with how Python does it:
            while next_rotation <= now_utc {
                next_rotation += SignedDuration::from_nanos_i128(i128::from(interval_ns));
            }
        }

        UnixNanos::from(u64::try_from(next_rotation.as_nanosecond()).unwrap_or(0))
    }

    /// Flushes and rotates `FileWriter` associated with `key`.
    async fn rotate_writer(
        &mut self,
        path: &FileWriterPath,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut writer = self.writers.remove(path).unwrap();
        let bytes = writer.take_buffer()?;
        self.store.put(&path.path, bytes.into()).await?;
        let new_path = self.regen_writer_path(path);
        self.writers.insert(new_path, writer);
        Ok(())
    }

    /// Creates (and inserts) a new `FileWriter` for type T.
    fn create_writer<T>(&mut self, path: FileWriterPath, data: &T) -> Result<(), ArrowError>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        self.create_writer_with_metadata::<T>(path, T::metadata(data))
    }

    /// Creates (and inserts) a new `FileWriter` for type T with pre-computed metadata.
    ///
    /// Use this variant when the caller has selected metadata from a chunk
    /// (e.g. via `T::chunk_metadata`) to avoid schema poisoning by sentinel rows.
    fn create_writer_with_metadata<T>(
        &mut self,
        path: FileWriterPath,
        metadata: HashMap<String, String>,
    ) -> Result<(), ArrowError>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        let type_str = T::path_prefix();
        let stage_delta_metadata =
            self.catalog_identifier_column && type_str != InstrumentAny::path_prefix();
        let schema = if self.catalog_identifier_column
            || type_str == InstrumentAny::path_prefix()
            || self.per_instrument_types.contains(type_str)
        {
            T::get_schema(Some(metadata))
        } else {
            T::get_schema(None)
        };

        let schema = if stage_delta_metadata {
            Self::schema_with_delta_staging_columns(&schema_with_identifier_column(&schema))
        } else {
            schema
        };
        let writer = FeatherBuffer::new(&schema, &self.rotation_config)?;
        self.writers.insert(path, writer);
        Ok(())
    }

    /// Creates (and inserts) a new `FeatherBuffer` for custom data at the given path.
    fn create_custom_writer(
        &mut self,
        path: FileWriterPath,
        type_name: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.writers.contains_key(&path) {
            return Ok(());
        }
        let base_schema = get_arrow_schema(type_name).ok_or_else(|| {
            format!("Custom data type \"{type_name}\" is not registered for Arrow encoding")
        })?;
        let schema = schema_with_data_type_column(base_schema.as_ref(), type_name);
        let schema = if self.catalog_identifier_column {
            Self::schema_with_delta_staging_columns(&schema_with_identifier_column(&schema))
        } else {
            schema
        };
        let writer = FeatherBuffer::new(&schema, &self.rotation_config)
            .map_err(|e| format!("Failed to create feather buffer for custom {type_name}: {e}"))?;
        self.writers.insert(path, writer);
        Ok(())
    }

    /// Encodes a single `CustomData` into a `RecordBatch` with `data_type` column (catalog-compatible).
    pub(crate) fn encode_custom_to_batch(
        custom: &CustomData,
    ) -> Result<RecordBatch, Box<dyn std::error::Error>> {
        let type_name = custom.data.type_name();
        let data_type_json = custom
            .data_type
            .to_persistence_json()
            .map_err(|e| format!("Failed to serialize data_type for persistence: {e}"))?;
        let dt_meta = custom.data_type.metadata_string_map();
        let items: [Arc<dyn CustomDataTrait>; 1] = [Arc::clone(&custom.data)];
        let batch = encode_custom_to_arrow(type_name, &items)
            .map_err(|e| format!("Failed to encode custom data: {e}"))?
            .ok_or_else(|| {
                format!("Custom data type \"{type_name}\" is not registered for Arrow")
            })?;
        let batch = augment_batch_with_data_type_column(
            &batch,
            &data_type_json,
            type_name,
            dt_meta.as_ref(),
        )
        .map_err(|e| e.to_string())?;
        Ok(batch)
    }

    fn schema_with_delta_staging_columns(schema: &Schema) -> Schema {
        let mut fields = schema
            .fields()
            .iter()
            .map(|field| {
                Arc::new(Field::new(
                    field.name().clone(),
                    field.data_type().clone(),
                    field.is_nullable(),
                ))
            })
            .collect::<Vec<_>>();

        if schema.index_of(NAUTILUS_ARROW_METADATA_ID_COLUMN).is_err() {
            fields.push(Arc::new(Field::new(
                NAUTILUS_ARROW_METADATA_ID_COLUMN,
                DataType::Utf8,
                false,
            )));
        }

        if schema
            .index_of(NAUTILUS_ARROW_METADATA_JSON_COLUMN)
            .is_err()
        {
            fields.push(Arc::new(Field::new(
                NAUTILUS_ARROW_METADATA_JSON_COLUMN,
                DataType::Utf8,
                false,
            )));
        }

        Schema::new(fields)
    }

    /// Flushes all active `FeatherBuffers` by writing any remaining buffered bytes to the object store.
    ///
    /// This is called automatically based on `flush_interval_ms` if configured, but can also
    /// be called manually by the client.
    ///
    /// Note: In Rust, we use in-memory buffers. Flushing writes the current buffer to the
    /// object store and creates a new buffer for continued writing. This is different from
    /// Python which just flushes OS buffers.
    pub async fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Collect paths and their current buffers before flushing
        let paths_to_flush: Vec<FileWriterPath> = self.writers.keys().cloned().collect();

        // Flush each writer and recreate it
        for path in paths_to_flush {
            if let Some(mut writer) = self.writers.remove(&path) {
                if writer.rows == 0 {
                    continue;
                }
                let bytes = writer.take_buffer()?;
                if !bytes.is_empty() {
                    // Write to the object store
                    self.store.put(&path.path, bytes.into()).await?;
                }
            }
        }

        self.last_flush_ns = self.clock.timestamp_ns();
        Ok(())
    }

    /// Closes all writers by flushing and removing them.
    ///
    /// After calling this, no further writes should be performed.
    pub async fn close(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.flush().await?;
        self.writers.clear();
        Ok(())
    }

    /// Returns whether the writer has been closed (all writers cleared).
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.writers.is_empty()
    }

    /// Returns information about the current files being written.
    ///
    /// Each entry maps a writer key (`type_str` and optional `instrument_id`) to
    /// its current buffer size and file path.
    #[must_use]
    pub fn get_current_file_info(&self) -> HashMap<String, (u64, String)> {
        let mut info = HashMap::new();

        for (path, buffer) in &self.writers {
            let key = match &path.instrument_id {
                Some(id) => format!("{}:{}", path.type_str, id),
                None => path.type_str.clone(),
            };
            info.insert(key, (buffer.size, path.path.to_string()));
        }
        info
    }

    /// Returns the total buffered (unflushed) bytes and rows across all active buffers.
    #[must_use]
    pub fn buffered_totals(&self) -> (u64, u64) {
        self.writers.values().fold((0, 0), |(bytes, rows), buffer| {
            (bytes + buffer.size, rows + buffer.rows)
        })
    }

    /// Returns the next rotation time for a specific writer key, if set.
    #[must_use]
    pub fn get_next_rotation_time(
        &self,
        type_str: &str,
        instrument_id: Option<&str>,
    ) -> Option<UnixNanos> {
        self.next_rotation_times
            .iter()
            .find(|(k, _)| k.type_str == type_str && k.instrument_id.as_deref() == instrument_id)
            .map(|(_, &v)| v)
    }

    /// Determines whether type T can be written before row-level identifier checks.
    fn should_write_type<T: CatalogPathPrefix>(&self) -> bool {
        self.should_write_prefix(T::path_prefix())
    }

    fn should_write_prefix(&self, record_prefix: &str) -> bool {
        self.included_types
            .as_ref()
            .is_none_or(|included| included.contains(record_prefix))
            && self
                .record_filter
                .as_ref()
                .is_none_or(|filter| filter.contains_prefix(record_prefix))
    }

    fn should_write_record(
        &self,
        record_prefix: &str,
        identifier: Option<&str>,
        instrument_type: Option<&str>,
    ) -> bool {
        self.included_types
            .as_ref()
            .is_none_or(|included| included.contains(record_prefix))
            && self
                .record_filter
                .as_ref()
                .is_none_or(|filter| filter.allows(record_prefix, identifier, instrument_type))
    }

    fn regen_writer_path(&mut self, path: &FileWriterPath) -> FileWriterPath {
        self.reserve_writer_path(&path.type_str, path.instrument_id.clone())
    }

    fn reserve_writer_path(
        &mut self,
        type_str: &str,
        instrument_id: Option<String>,
    ) -> FileWriterPath {
        let timestamp = self.clock.timestamp_ns();

        for sequence in 0.. {
            let path =
                self.build_writer_path(type_str, instrument_id.as_deref(), timestamp, sequence);

            if self.reserved_paths.insert(path.clone()) {
                return FileWriterPath {
                    path,
                    type_str: type_str.to_string(),
                    instrument_id,
                };
            }
        }

        unreachable!("unbounded writer path sequence exhausted")
    }

    fn build_writer_path(
        &self,
        type_str: &str,
        instrument_id: Option<&str>,
        timestamp: UnixNanos,
        sequence: u64,
    ) -> Path {
        // Note: Path removes prefixing slashes
        let mut path = Path::from(self.base_path.clone());

        if type_str.starts_with("data/custom/") {
            let type_name = type_str.strip_prefix("data/custom/").unwrap_or(type_str);
            path = path.join("data").join("custom").join(type_name.to_string());

            // Use a single flat urisafe segment so the writer path, the stream-promotion
            // parser, and the catalog layout (custom_data_path_components) agree.
            let safe_id = instrument_id
                .map(urisafe_instrument_id)
                .filter(|safe| !safe.is_empty());

            if let Some(safe) = &safe_id {
                path = path.join(safe.clone());
            }
            let file_stem = safe_id.as_deref().unwrap_or(type_name);
            path = path.join(Self::timestamped_feather_file_name(
                file_stem, timestamp, sequence,
            ));
        } else if let Some(instrument_id) = instrument_id {
            let safe_id = urisafe_instrument_id(instrument_id);
            path = path.join(type_str);
            path = path.join(safe_id);
            path = path.join(Self::timestamped_feather_file_name(
                type_str, timestamp, sequence,
            ));
        } else {
            path = path.join(Self::timestamped_feather_file_name(
                type_str, timestamp, sequence,
            ));
        }

        path
    }

    fn timestamped_feather_file_name(stem: &str, timestamp: UnixNanos, sequence: u64) -> String {
        if sequence == 0 {
            format!("{stem}_{timestamp}.feather")
        } else {
            format!("{stem}_{timestamp}-{sequence}.feather")
        }
    }

    /// Builds `FileWriterPath` for custom data using `DataType` identifier as folder partition (catalog layout).
    fn get_writer_path_custom(
        &mut self,
        type_name: &str,
        identifier: Option<&str>,
    ) -> FileWriterPath {
        let type_str = format!("data/custom/{type_name}");

        if let Some(existing) = self
            .writers
            .keys()
            .find(|path| path.type_str == type_str && path.instrument_id.as_deref() == identifier)
        {
            return existing.clone();
        }
        self.reserve_writer_path(&type_str, identifier.map(String::from))
    }

    /// Generates a key for a `FileWriter` based on type T and optional instrument ID.
    /// Reuses an existing writer key (same `type_str` and `instrument_id`) if present, so we
    /// buffer multiple items in the same file until rotation; otherwise creates a new path with current timestamp.
    fn get_writer_path<T>(&mut self, data: &T) -> Result<FileWriterPath, Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix,
    {
        let type_str = T::path_prefix();
        let metadata = T::metadata(data);

        let instrument_id = if type_str == InstrumentAny::path_prefix()
            || self.per_instrument_types.contains(type_str)
            || (type_str.starts_with("custom_") && metadata.contains_key(KEY_INSTRUMENT_ID))
        {
            Some(metadata.get(KEY_INSTRUMENT_ID).cloned().ok_or_else(|| {
                format!("Data {type_str} expected instrument_id metadata for per instrument writer")
            })?)
        } else {
            None
        };

        // Reuse existing writer for same (type_str, instrument_id) so we buffer in one file until rotation
        if let Some(existing) = self
            .writers
            .keys()
            .find(|k| k.type_str == type_str && k.instrument_id == instrument_id)
        {
            return Ok(existing.clone());
        }

        Ok(self.reserve_writer_path(type_str, instrument_id))
    }

    /// Writes a Data enum value to the appropriate writer.
    ///
    /// This is a convenience method that routes the Data enum to the appropriate
    /// typed write method.
    pub fn write_data(&mut self, data: Data) -> Result<(), Box<dyn std::error::Error>> {
        match data {
            Data::Instrument(instrument) => self.write(*instrument),
            Data::Quote(quote) => self.write(quote),
            Data::Trade(trade) => self.write(trade),
            Data::Bar(bar) => self.write(bar),
            Data::BookDelta(delta) => self.write(delta),
            Data::BookDepth(depth) => self.write(*depth),
            Data::IndexPrice(price) => self.write(price),
            Data::MarkPrice(price) => self.write(price),
            Data::FundingRate(funding) => self.write(funding),
            Data::InstrumentStatus(status) => self.write(status),
            Data::OptionGreeks(greeks) => self.write(greeks),
            Data::InstrumentClose(close) => self.write(close),
            Data::Custom(custom) => self.write_custom_data(&custom),
            Data::BookDeltas(deltas_api) => {
                // Batch write so chunk_metadata can skip a leading BookAction::Clear sentinel
                self.write_batch(deltas_api.deltas.clone())
            }
            #[cfg(feature = "defi")]
            Data::Defi(_) => Err("Unsupported DeFi data variant for feather writes".into()),
        }
    }

    /// Writes mixed data as typed Arrow batches.
    ///
    /// # Panics
    ///
    /// Panics if the built-in data batch dispatch becomes non-exhaustive.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "the public writer API accepts ownership of each submitted data batch"
    )]
    pub fn write_data_batch(&mut self, data: Vec<Data>) -> Result<(), Box<dyn std::error::Error>> {
        for batch in DataBatch::from_data_vec_grouped(&data)? {
            match &batch {
                DataBatch::Custom(data) => {
                    for custom in data.as_ref() {
                        self.write_custom_data(custom)?;
                    }
                }
                batch => write_builtin_data_batch(self, batch)
                    .expect("built-in data batch dispatch is exhaustive")?,
            }
        }
        Ok(())
    }

    /// Writes supported message bus value.
    pub fn write_any_message(
        &mut self,
        message: &dyn Any,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(command) = Self::write_command(message) else {
            return Ok(false);
        };
        command(self)?;
        Ok(true)
    }

    pub(crate) fn write_command(message: &dyn Any) -> Option<FeatherWriteCommand> {
        macro_rules! try_write {
            ($message:expr, $type:ty) => {
                if let Some(value) = $message.downcast_ref::<$type>() {
                    let value = value.clone();
                    return Some(Box::new(move |writer| writer.write(value)));
                }
            };
        }

        try_write!(message, QuoteTick);
        try_write!(message, TradeTick);
        try_write!(message, Bar);
        try_write!(message, OrderBookDelta);
        try_write!(message, OrderBookDepth);
        try_write!(message, IndexPriceUpdate);
        try_write!(message, MarkPriceUpdate);
        try_write!(message, InstrumentStatus);
        try_write!(message, OptionGreeks);
        try_write!(message, InstrumentClose);
        try_write!(message, InstrumentAny);
        try_write!(message, AccountState);
        try_write!(message, OrderInitialized);
        try_write!(message, OrderDenied);
        try_write!(message, OrderEmulated);
        try_write!(message, OrderSubmitted);
        try_write!(message, OrderAccepted);
        try_write!(message, OrderRejected);
        try_write!(message, OrderPendingCancel);
        try_write!(message, OrderCanceled);
        try_write!(message, OrderCancelRejected);
        try_write!(message, OrderExpired);
        try_write!(message, OrderTriggered);
        try_write!(message, OrderPendingUpdate);
        try_write!(message, OrderReleased);
        try_write!(message, OrderModifyRejected);
        try_write!(message, OrderUpdated);
        try_write!(message, OrderFilled);
        try_write!(message, OrderFillVoided);
        try_write!(message, PositionOpened);
        try_write!(message, PositionChanged);
        try_write!(message, PositionClosed);
        try_write!(message, PositionAdjusted);
        try_write!(message, OrderSnapshot);
        try_write!(message, PositionSnapshot);
        try_write!(message, OrderStatusReport);
        try_write!(message, FillReport);
        try_write!(message, PositionStatusReport);
        try_write!(message, ExecutionMassStatus);

        if let Some(deltas) = message.downcast_ref::<OrderBookDeltas>() {
            let deltas = deltas.deltas.clone();
            return Some(Box::new(move |writer| writer.write_batch(deltas)));
        }

        if let Some(data) = message.downcast_ref::<Data>() {
            let data = data.clone();
            return Some(Box::new(move |writer| writer.write_data(data)));
        }

        if let Some(custom) = message.downcast_ref::<CustomData>() {
            let custom = custom.clone();
            return Some(Box::new(move |writer| {
                writer.write_data(Data::Custom(custom))
            }));
        }

        if let Some(event) = message.downcast_ref::<OrderEventAny>() {
            let event = event.clone();
            return Some(Box::new(move |writer| writer.write_order_event(&event)));
        }

        if let Some(event) = message.downcast_ref::<PositionEvent>() {
            let event = event.clone();
            return Some(Box::new(move |writer| writer.write_position_event(&event)));
        }

        None
    }

    fn write_order_event(
        &mut self,
        event: &OrderEventAny,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match event {
            OrderEventAny::Initialized(event) => self.write(event.clone()),
            OrderEventAny::Denied(event) => self.write(*event),
            OrderEventAny::Emulated(event) => self.write(*event),
            OrderEventAny::Released(event) => self.write(*event),
            OrderEventAny::Submitted(event) => self.write(*event),
            OrderEventAny::Accepted(event) => self.write(*event),
            OrderEventAny::Rejected(event) => self.write(*event),
            OrderEventAny::Canceled(event) => self.write(*event),
            OrderEventAny::Expired(event) => self.write(*event),
            OrderEventAny::Triggered(event) => self.write(*event),
            OrderEventAny::PendingUpdate(event) => self.write(*event),
            OrderEventAny::PendingCancel(event) => self.write(*event),
            OrderEventAny::ModifyRejected(event) => self.write(*event),
            OrderEventAny::CancelRejected(event) => self.write(*event),
            OrderEventAny::Updated(event) => self.write(*event),
            OrderEventAny::Filled(event) => self.write(event.clone()),
            OrderEventAny::FillVoided(event) => self.write(event.clone()),
        }
    }

    fn write_position_event(
        &mut self,
        event: &PositionEvent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match event {
            PositionEvent::PositionOpened(event) => self.write(event.clone()),
            PositionEvent::PositionChanged(event) => self.write(event.clone()),
            PositionEvent::PositionClosed(event) => self.write(event.clone()),
            PositionEvent::PositionAdjusted(event) => self.write(*event),
        }
    }

    /// Writes a single custom data value (catalog layout: `data/custom/{type_name}[/{identifier}]`).
    fn write_custom_data(&mut self, custom: &CustomData) -> Result<(), Box<dyn std::error::Error>> {
        let batch = Self::encode_custom_to_batch(custom)?;
        self.write_custom_batch(custom, batch)
    }

    pub(crate) fn write_custom_batch(
        &mut self,
        custom: &CustomData,
        mut batch: RecordBatch,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let type_name = custom.data.type_name();
        let identifier = custom.data_type.identifier().map(String::from);

        if !self.should_write_custom(type_name, identifier.as_deref()) {
            return Ok(());
        }

        let path = self.get_writer_path_custom(type_name, identifier.as_deref());
        if !self.writers.contains_key(&path) {
            self.create_custom_writer(path.clone(), type_name)?;
        }

        if self.catalog_identifier_column {
            let metadata_row =
                arrow_metadata_row(batch.schema().metadata(), batch.schema().fields())?;
            batch = record_batch_with_identifier_column(batch, custom.data_type.identifier())?;
            batch = record_batch_with_delta_staged_metadata(
                &batch,
                std::slice::from_ref(&metadata_row),
            )?;
        }

        let mut pending = PendingIo::default();

        self.stage_batch_write(path, &batch, &mut pending)?;
        pending.flush_due = self.flush_is_due();

        self.complete_pending_io(&pending)
    }

    pub(crate) fn should_write_custom(&self, type_name: &str, identifier: Option<&str>) -> bool {
        let record_prefix = format!("custom/{type_name}");
        self.included_types.as_ref().is_none_or(|included| {
            included.contains(type_name)
                || included.contains("custom")
                || included.contains(&record_prefix)
        }) && self
            .record_filter
            .as_ref()
            .is_none_or(|filter| filter.allows(&record_prefix, identifier, None))
    }

    /// Writes an instrument to the appropriate writer.
    ///
    /// Instruments are written to feather files and organized by instrument ID.
    /// This method supports writing instruments that implement `EncodeToRecordBatch` and `CatalogPathPrefix`.
    pub fn write_instrument(
        &mut self,
        instrument: InstrumentAny,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.write(instrument)
    }

    /// Subscribes to all messages on the message bus (pattern "*").
    ///
    /// This will automatically write all supported data types that are published
    /// on the message bus to the feather files.
    ///
    /// The writer must be wrapped in `Rc<RefCell<>>` to be shareable with the message bus handler.
    ///
    /// Note: Writes are synchronous; the writer enters the runtime internally only when a
    /// rotation or auto-flush boundary is hit.
    pub fn subscribe_to_message_bus(
        writer: Rc<RefCell<Self>>,
    ) -> Result<ShareableMessageHandler, Box<dyn std::error::Error>> {
        let handler = ShareableMessageHandler::from_any(move |message: &dyn Any| {
            if let Err(e) = writer.borrow_mut().write_any_message(message) {
                log::warn!("Failed to write streaming message: {e}");
            }
        });

        subscribe_any(MStr::pattern("*"), handler.clone(), None);

        Ok(handler)
    }

    /// Unsubscribes from message bus.
    pub fn unsubscribe_from_message_bus(handler: &ShareableMessageHandler) {
        unsubscribe_any(MStr::pattern("*"), handler);
    }
}

impl Debug for FeatherWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(FeatherWriter))
            .finish_non_exhaustive()
    }
}

impl StreamingDataSink for FeatherWriter {
    fn write_data(&mut self, data: Data) -> anyhow::Result<()> {
        Self::write_data(self, data).map_err(feather_error)
    }

    fn write_batch(&mut self, data: Vec<Data>) -> anyhow::Result<()> {
        Self::write_data_batch(self, data).map_err(feather_error)
    }

    fn write_any(&mut self, message: &dyn Any) -> anyhow::Result<bool> {
        self.write_any_message(message).map_err(feather_error)
    }

    fn flush(&mut self) -> anyhow::Result<()> {
        block_on_nautilus_with(|| async { self.flush().await.map_err(feather_error) })
    }

    fn close(&mut self) -> anyhow::Result<()> {
        block_on_nautilus_with(|| async { self.close().await.map_err(feather_error) })
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use datafusion::arrow::ipc::reader::StreamReader;
    use nautilus_common::{
        clock::TestClock,
        live::{LiveClock, get_runtime},
    };
    use nautilus_model::{
        data::{Data, QuoteTick, TradeTick},
        enums::AggressorSide,
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::{
        ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch,
    };
    use object_store::{ObjectStore, local::LocalFileSystem};
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    fn test_writer_manager_keys() {
        // Create a temporary directory for base path
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();

        // Create a LocalFileSystem based object store using the temp directory
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);

        // Create a test time source
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
        let timestamp = clock.timestamp_ns();

        let quote_type_str = QuoteTick::path_prefix();

        let mut per_instrument = HashSet::new();
        per_instrument.insert(quote_type_str.to_string());

        let mut manager = FeatherWriter::new(
            base_path.clone(),
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            Some(per_instrument),
            None, // flush_interval_ms
        );

        let instrument_id = "AAPL.AAPL";
        // Write a dummy value
        let quote = QuoteTick::new(
            InstrumentId::from(instrument_id),
            Price::from("100.0"),
            Price::from("100.0"),
            Quantity::from("100.0"),
            Quantity::from("100.0"),
            UnixNanos::from(1_000_000_000_000_000_000),
            UnixNanos::from(1_000_000_000_000_000_000),
        );

        let trade = TradeTick::new(
            InstrumentId::from(instrument_id),
            Price::from("100.0"),
            Quantity::from("100.0"),
            AggressorSide::Buy,
            TradeId::from("1"),
            UnixNanos::from(1_000_000_000_000_000_000),
            UnixNanos::from(1_000_000_000_000_000_000),
        );

        manager.write(quote).unwrap();
        manager.write(trade).unwrap();

        // Check keys and paths for quotes and trades
        let path = manager.get_writer_path(&quote).unwrap();
        let safe_id = instrument_id.replace('/', "");
        let expected_path = Path::from(format!(
            "{base_path}/quotes/{safe_id}/quotes_{timestamp}.feather"
        ));
        assert_eq!(path.path, expected_path);
        assert!(manager.writers.contains_key(&path));
        let writer = manager.writers.get(&path).unwrap();
        assert!(writer.size > 0);

        let path = manager.get_writer_path(&trade).unwrap();
        let expected_path = Path::from(format!("{base_path}/trades_{timestamp}.feather"));
        assert_eq!(path.path, expected_path);
        assert!(manager.writers.contains_key(&path));
        let writer = manager.writers.get(&path).unwrap();
        assert!(writer.size > 0);
    }

    #[rstest]
    fn test_per_instrument_path_keeps_long_id_out_of_filename() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
        let manager = FeatherWriter::new(
            base_path.clone(),
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );
        let instrument_id = format!("{}.VENUE", "A".repeat(240));

        let path = manager.build_writer_path(
            QuoteTick::path_prefix(),
            Some(&instrument_id),
            UnixNanos::default(),
            0,
        );

        let safe_id = urisafe_instrument_id(&instrument_id);
        let expected = Path::from(format!("{base_path}/quotes/{safe_id}/quotes_0.feather"));
        assert_eq!(path, expected);
    }

    #[rstest]
    fn existing_feather_writer_implements_streaming_data_sink() {
        let temp_dir = TempDir::new().unwrap();
        let storage = crate::common::storage::create_storage_backend_from_path(
            temp_dir.path().to_str().unwrap(),
            None,
        )
        .unwrap();
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));
        let mut writer = FeatherWriter::new(
            storage.base_path.clone(),
            storage.object_store.clone(),
            clock,
            RotationConfig::NoRotation,
            None,
            Some(HashSet::from(["quotes".to_string()])),
            None,
        );
        let quote = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.0"),
            Price::from("1.1"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );

        StreamingDataSink::write_data(&mut writer, Data::Quote(quote)).unwrap();
        StreamingDataSink::flush(&mut writer).unwrap();

        let files = get_runtime()
            .block_on(storage.list_files("quotes", Some(".feather")))
            .unwrap();
        assert_eq!(files.len(), 1);
        assert!(
            std::path::PathBuf::from(&files[0])
                .components()
                .any(|component| component.as_os_str() == "AUDUSD.SIM"),
        );
    }

    #[rstest]
    fn scheduled_rotation_keeps_time_of_day_anchor_after_late_rotation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = crate::common::storage::create_storage_backend_from_path(
            temp_dir.path().to_str().unwrap(),
            None,
        )
        .unwrap();
        let now = Arc::new(AtomicU64::new(
            1_767_258_000_000_000_000, // 2026-01-01 09:00:00 UTC
        ));
        let path = FileWriterPath {
            path: Path::from("quotes.feather"),
            type_str: "quotes".to_string(),
            instrument_id: None,
        };
        let mut writer = FeatherWriter::new(
            storage.base_path,
            storage.object_store,
            WriterClock::Test(Arc::clone(&now)),
            RotationConfig::ScheduledDates {
                interval_ns: 86_400_000_000_000,
                rotation_time: UnixNanos::from(36_000_000_000_000u64),
                rotation_timezone: jiff::tz::TimeZone::UTC,
            },
            None,
            None,
            None,
        );

        assert!(!writer.check_scheduled_rotation(&path));
        assert_eq!(
            writer.next_rotation_times[&path],
            UnixNanos::from(1_767_261_600_000_000_000u64),
        );

        now.store(1_767_263_400_000_000_000, Ordering::Relaxed); // 10:30 UTC
        assert!(writer.check_scheduled_rotation(&path));
        assert_eq!(
            writer.next_rotation_times[&path],
            UnixNanos::from(1_767_348_000_000_000_000u64),
        );
    }

    #[rstest]
    fn test_file_writer_round_trip() {
        let instrument_id = "AAPL.AAPL";
        // Write a dummy value.
        let quote = QuoteTick::new(
            InstrumentId::from(instrument_id),
            Price::from("100.0"),
            Price::from("100.0"),
            Quantity::from("100.0"),
            Quantity::from("100.0"),
            UnixNanos::from(100),
            UnixNanos::from(100),
        );
        let metadata = QuoteTick::metadata(&quote);
        let schema = QuoteTick::get_schema(Some(metadata.clone()));
        let batch = QuoteTick::encode_batch(&QuoteTick::metadata(&quote), &[quote]).unwrap();

        let mut writer = FeatherBuffer::new(&schema, &RotationConfig::NoRotation).unwrap();
        writer.write_record_batch(&batch).unwrap();

        let buffer = writer.take_buffer().unwrap();
        let mut reader = StreamReader::try_new(Cursor::new(buffer.as_slice()), None).unwrap();

        let read_metadata = reader.schema().metadata().clone();
        assert_eq!(read_metadata, metadata);

        let read_batch = reader.next().unwrap().unwrap();
        assert_eq!(read_batch.column(0), batch.column(0));

        let decoded = QuoteTick::decode_data_batch(&metadata, batch).unwrap();
        assert_eq!(decoded[0], Data::from(quote));
    }

    #[rstest]
    fn test_round_trip() {
        // Create a temporary directory for base path
        let temp_dir = TempDir::new_in(".").unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();

        // Create a LocalFileSystem based object store using the temp directory
        let local_fs = LocalFileSystem::new_with_prefix(&base_path).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);

        // Create a test time source
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

        let quote_type_str = QuoteTick::path_prefix();
        let trade_type_str = TradeTick::path_prefix();

        let mut per_instrument = HashSet::new();
        per_instrument.insert(quote_type_str.to_string());
        per_instrument.insert(trade_type_str.to_string());

        let mut manager = FeatherWriter::new(
            base_path.clone(),
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            Some(per_instrument),
            None, // flush_interval_ms
        );

        let instrument_id = "AAPL.AAPL";
        // Write a dummy value.
        let quote = QuoteTick::new(
            InstrumentId::from(instrument_id),
            Price::from("100.0"),
            Price::from("100.0"),
            Quantity::from("100.0"),
            Quantity::from("100.0"),
            UnixNanos::from(100),
            UnixNanos::from(100),
        );

        let trade = TradeTick::new(
            InstrumentId::from(instrument_id),
            Price::from("100.0"),
            Quantity::from("100.0"),
            AggressorSide::Buy,
            TradeId::from("1"),
            UnixNanos::from(100),
            UnixNanos::from(100),
        );

        manager.write(quote).unwrap();
        manager.write(trade).unwrap();

        let paths = manager.writers.keys().cloned().collect::<Vec<_>>();
        assert_eq!(paths.len(), 2);

        // Flush data
        get_runtime().block_on(manager.flush()).unwrap();

        // Read files from the temporary directory
        let mut recovered_quotes = Vec::new();
        let mut recovered_trades = Vec::new();
        let local_fs = LocalFileSystem::new_with_prefix(&base_path).unwrap();
        for path in paths {
            let path_str = local_fs.path_to_filesystem(&path.path).unwrap();
            let buffer = std::fs::File::open(&path_str).unwrap();
            let reader = StreamReader::try_new(buffer, None).unwrap();
            let metadata = reader.schema().metadata().clone();
            for batch in reader {
                let batch = batch.unwrap();
                if path_str.to_str().unwrap().contains("quotes") {
                    let decoded = QuoteTick::decode_data_batch(&metadata, batch).unwrap();
                    recovered_quotes.extend(decoded);
                } else if path_str.to_str().unwrap().contains("trades") {
                    let decoded = TradeTick::decode_data_batch(&metadata, batch).unwrap();
                    recovered_trades.extend(decoded);
                }
            }
        }

        // Assert that the recovered data matches the written data
        assert_eq!(recovered_quotes.len(), 1, "Expected one QuoteTick record");
        assert_eq!(recovered_trades.len(), 1, "Expected one TradeTick record");

        // Check key fields to ensure the data round-tripped correctly
        assert_eq!(recovered_quotes[0], Data::from(quote));
        assert_eq!(recovered_trades[0], Data::from(trade));
    }

    #[rstest]
    fn test_write_data_enum() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

        let mut writer = FeatherWriter::new(
            base_path,
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );

        let quote = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.0"),
            Price::from("1.0"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(1000),
            UnixNanos::from(1000),
        );

        // Test writing via write_data
        writer.write_data(Data::Quote(quote)).unwrap();
        get_runtime().block_on(writer.flush()).unwrap();

        // Verify file was created
        assert!(!writer.writers.is_empty() || temp_dir.path().read_dir().unwrap().count() > 0);
    }

    #[rstest]
    fn test_write_data_all_types() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

        let mut writer = FeatherWriter::new(
            base_path,
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );

        let instrument_id = InstrumentId::from("AUD/USD.SIM");

        // Test all data types
        let quote = QuoteTick::new(
            instrument_id,
            Price::from("1.0"),
            Price::from("1.0"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(1000),
            UnixNanos::from(1000),
        );
        writer.write_data(Data::Quote(quote)).unwrap();

        let trade = TradeTick::new(
            instrument_id,
            Price::from("1.0"),
            Quantity::from("1000"),
            AggressorSide::Buy,
            TradeId::from("1"),
            UnixNanos::from(2000),
            UnixNanos::from(2000),
        );
        writer.write_data(Data::Trade(trade)).unwrap();

        let delta = OrderBookDelta::clear(
            instrument_id,
            0,
            UnixNanos::from(3000),
            UnixNanos::from(3000),
        );
        writer.write_data(Data::BookDelta(delta)).unwrap();

        get_runtime().block_on(writer.flush()).unwrap();
    }

    #[rstest]
    fn test_auto_flush() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let shared_time = Arc::new(AtomicU64::new(0));
        let clock = WriterClock::Test(Arc::clone(&shared_time));

        let mut writer = FeatherWriter::new(
            base_path,
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            Some(100), // 100ms flush interval
        );

        let quote = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.0"),
            Price::from("1.0"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(1000),
            UnixNanos::from(1000),
        );

        // Write first quote; interval not elapsed, so the buffer is retained
        writer.write(quote).unwrap();
        assert_eq!(writer.writers.len(), 1);
        assert_eq!(writer.last_flush_ns, UnixNanos::from(0));

        // Advance the shared time source past the 100ms flush interval
        shared_time.store(200_000_000, Ordering::Relaxed);

        // Second write hits the flush boundary: buffers are flushed and removed
        let quote2 = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.1"),
            Price::from("1.1"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(2000),
            UnixNanos::from(2000),
        );
        writer.write(quote2).unwrap();

        assert_eq!(writer.writers.len(), 0);
        assert_eq!(writer.last_flush_ns, UnixNanos::from(200_000_000));
        assert_eq!(temp_dir.path().read_dir().unwrap().count(), 1);
    }

    #[rstest]
    fn test_close() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

        let mut writer = FeatherWriter::new(
            base_path,
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );

        let quote = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.0"),
            Price::from("1.0"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(1000),
            UnixNanos::from(1000),
        );

        writer.write(quote).unwrap();
        assert!(!writer.writers.is_empty());

        get_runtime().block_on(writer.close()).unwrap();
        assert!(writer.writers.is_empty());
    }

    #[rstest]
    fn test_write_data_orderbook_deltas() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

        let mut writer = FeatherWriter::new(
            base_path,
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );

        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let delta1 = OrderBookDelta::clear(
            instrument_id,
            0,
            UnixNanos::from(1000),
            UnixNanos::from(1000),
        );
        let delta2 = OrderBookDelta::clear(
            instrument_id,
            0,
            UnixNanos::from(2000),
            UnixNanos::from(2000),
        );

        let deltas = OrderBookDeltas::new(instrument_id, vec![delta1, delta2]);
        // Test writing OrderBookDeltas via write_data
        writer
            .write_data(Data::BookDeltas(Box::new(deltas)))
            .unwrap();
        get_runtime().block_on(writer.flush()).unwrap();
    }

    #[rstest]
    fn feather_writer_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<FeatherWriter>();
        assert_send::<WriterClock>();
    }

    #[rstest]
    fn writer_clock_test_source_reads_shared_atomic() {
        let shared = Arc::new(AtomicU64::new(7));
        let clock = WriterClock::Test(Arc::clone(&shared));
        assert_eq!(clock.timestamp_ns(), UnixNanos::from(7));

        shared.store(42, Ordering::Relaxed);
        assert_eq!(clock.timestamp_ns(), UnixNanos::from(42));
    }

    #[rstest]
    fn writer_clock_from_shared_clock_wires_test_clock() {
        let test_clock = Rc::new(RefCell::new(TestClock::new()));
        test_clock
            .borrow_mut()
            .advance_time(UnixNanos::from(42), true);
        let clock: Rc<RefCell<dyn Clock>> = test_clock.clone();

        let (writer_clock, shared) = WriterClock::from_shared_clock(&clock);
        let shared = shared.expect("non-live clocks must return a shared atomic");

        // Seeded with the source clock's current time
        assert_eq!(writer_clock.timestamp_ns(), UnixNanos::from(42));

        // Bridge refresh: advancing the source clock and storing into the atomic
        // is what the PyO3 wrapper does before each forwarded call
        test_clock
            .borrow_mut()
            .advance_time(UnixNanos::from(99), true);
        shared.store(clock.borrow().timestamp_ns().as_u64(), Ordering::Relaxed);
        assert_eq!(writer_clock.timestamp_ns(), UnixNanos::from(99));
    }

    #[rstest]
    fn writer_clock_from_shared_clock_live_clock_is_live() {
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(LiveClock::new(None)));

        let (writer_clock, shared) = WriterClock::from_shared_clock(&clock);

        assert!(matches!(writer_clock, WriterClock::Live));
        assert!(shared.is_none());
    }

    #[rstest]
    fn buffered_totals_tracks_bytes_and_rows() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

        let mut writer = FeatherWriter::new(
            base_path,
            store,
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );
        assert_eq!(writer.buffered_totals(), (0, 0));

        let quote = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.0"),
            Price::from("1.0"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(1000),
            UnixNanos::from(1000),
        );
        let metadata = QuoteTick::metadata(&quote);
        let batch = QuoteTick::encode_batch(&metadata, &[quote]).unwrap();
        let expected_bytes = batch.get_array_memory_size() as u64;

        writer.write(quote).unwrap();
        assert_eq!(writer.buffered_totals(), (expected_bytes, 1));

        get_runtime().block_on(writer.flush()).unwrap();
        assert_eq!(writer.buffered_totals(), (0, 0));
    }

    #[tokio::test]
    async fn size_rotation_with_due_flush_does_not_persist_empty_file() {
        use futures::StreamExt;

        let temp_dir = TempDir::new().unwrap();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let shared_clock = Arc::new(AtomicU64::new(0));
        let mut writer = FeatherWriter::new(
            temp_dir.path().to_str().unwrap().to_string(),
            Arc::clone(&store),
            WriterClock::Test(Arc::clone(&shared_clock)),
            RotationConfig::Size { max_size: 1 },
            None,
            None,
            Some(1),
        );
        shared_clock.store(1_000_000, Ordering::Relaxed);

        writer
            .write(QuoteTick::new(
                InstrumentId::from("AUD/USD.SIM"),
                Price::from("1.0"),
                Price::from("1.0"),
                Quantity::from("1000"),
                Quantity::from("1000"),
                UnixNanos::from(1000),
                UnixNanos::from(1000),
            ))
            .unwrap();

        let mut objects = store.list(None);
        let mut object_count = 0;

        while let Some(object) = objects.next().await {
            object.unwrap();
            object_count += 1;
        }

        assert_eq!(object_count, 1);
    }

    #[tokio::test]
    #[cfg(feature = "python")]
    async fn test_write_custom_data_round_trip() {
        use std::sync::Arc;

        use futures::StreamExt;
        use nautilus_model::{
            data::{CustomData, Data, DataType},
            identifiers::InstrumentId,
        };
        use nautilus_serialization::{
            arrow::custom::CustomDataDecoder, ensure_custom_data_registered,
        };

        use crate::test_data::RustTestCustomData;

        ensure_custom_data_registered::<RustTestCustomData>();

        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock = WriterClock::Test(Arc::new(AtomicU64::new(0)));

        let mut writer = FeatherWriter::new(
            base_path.clone(),
            store.clone(),
            clock,
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );

        let instrument_id = InstrumentId::from("RUST.TEST");
        let data_type = DataType::new("RustTestCustomData", None, Some(instrument_id.to_string()));
        let original = RustTestCustomData {
            instrument_id,
            value: 1.23,
            flag: true,
            ts_event: UnixNanos::from(1000),
            ts_init: UnixNanos::from(1000),
        };
        let custom = CustomData::new(Arc::new(original.clone()), data_type);

        writer
            .write_data(Data::Custom(custom))
            .expect("write_data CustomData");
        writer.flush().await.expect("flush");

        let prefix = Path::from(format!("{base_path}/data/custom/RustTestCustomData"));
        let mut list_stream = store.list(Some(&prefix));
        let first = list_stream.next().await.expect("at least one object");
        let meta = first.expect("list item");
        let bytes = store
            .get(&meta.location)
            .await
            .expect("get")
            .bytes()
            .await
            .expect("bytes");
        let mut reader =
            StreamReader::try_new(Cursor::new(bytes.as_ref()), None).expect("StreamReader");
        let schema = reader.schema();
        let metadata: std::collections::HashMap<String, String> = schema
            .metadata()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let batch = reader.next().expect("batch").expect("batch ok");
        let decoded =
            CustomDataDecoder::decode_data_batch(&metadata, batch).expect("decode_data_batch");
        assert_eq!(decoded.len(), 1);
        if let Data::Custom(decoded_custom) = &decoded[0] {
            assert_eq!(decoded_custom.data_type.type_name(), "RustTestCustomData");
            let rust: &RustTestCustomData = decoded_custom
                .data
                .as_any()
                .downcast_ref::<RustTestCustomData>()
                .expect("RustTestCustomData");
            assert_eq!(rust, &original);
        } else {
            panic!("Expected Data::Custom");
        }
    }

    #[tokio::test]
    #[cfg(feature = "python")]
    async fn test_write_custom_data_reuses_writer_until_rotation() {
        use futures::StreamExt;
        use nautilus_model::data::{CustomData, DataType};
        use nautilus_serialization::ensure_custom_data_registered;

        use crate::test_data::RustTestCustomData;

        ensure_custom_data_registered::<RustTestCustomData>();
        let temp_dir = TempDir::new().unwrap();
        let storage = crate::common::storage::create_storage_backend_from_path(
            temp_dir.path().to_str().unwrap(),
            None,
        )
        .unwrap();
        let mut writer = FeatherWriter::new(
            storage.base_path.clone(),
            storage.object_store.clone(),
            WriterClock::Test(Arc::new(AtomicU64::new(0))),
            RotationConfig::NoRotation,
            None,
            None,
            None,
        );
        let instrument_id = InstrumentId::from("RUST.TEST");
        let data_type = DataType::new("RustTestCustomData", None, Some(instrument_id.to_string()));

        for (ts, value) in [(1_000, 1_000.0), (2_000, 2_000.0)] {
            writer
                .write_data(Data::Custom(CustomData::new(
                    Arc::new(RustTestCustomData {
                        instrument_id,
                        value,
                        flag: true,
                        ts_event: UnixNanos::from(ts),
                        ts_init: UnixNanos::from(ts),
                    }),
                    data_type.clone(),
                )))
                .unwrap();
        }
        assert_eq!(writer.get_current_file_info().len(), 1);
        writer.flush().await.unwrap();

        let prefix = Path::from(format!(
            "{}/data/custom/RustTestCustomData",
            storage.base_path
        ));
        let files = storage
            .object_store
            .list(Some(&prefix))
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .into_iter()
            .filter(|meta| meta.location.as_ref().ends_with(".feather"))
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
        let bytes = storage
            .object_store
            .get(&files[0].location)
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let rows = StreamReader::try_new(Cursor::new(bytes.as_ref()), None)
            .unwrap()
            .map(|batch| batch.unwrap().num_rows())
            .sum::<usize>();
        assert_eq!(rows, 2);
    }
}
