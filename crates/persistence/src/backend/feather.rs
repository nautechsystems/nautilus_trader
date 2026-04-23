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

use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use ahash::AHashMap;
use chrono_tz::Tz;
use datafusion::arrow::{
    datatypes::Schema, error::ArrowError, ipc::writer::StreamWriter, record_batch::RecordBatch,
};
use nautilus_common::{
    cache::fifo::FifoCache,
    clock::Clock,
    msgbus::{mstr::MStr, subscribe_any, typed_handler::ShareableMessageHandler, unsubscribe_any},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{
        Bar, CatalogPathPrefix, CustomData, CustomDataTrait, Data, FundingRateUpdate,
        IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick, close::InstrumentClose, encode_custom_to_arrow,
        get_arrow_schema,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderExpired, OrderFilled, OrderInitialized, OrderModifyRejected,
        OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased, OrderSnapshot,
        OrderSubmitted, OrderTriggered, OrderUpdated, PositionAdjusted, PositionChanged,
        PositionClosed, PositionOpened, PositionSnapshot,
    },
    instruments::InstrumentAny,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};
use nautilus_serialization::arrow::{EncodeToRecordBatch, KEY_INSTRUMENT_ID};
use object_store::{ObjectStore, ObjectStoreExt, path::Path};

use super::catalog::urisafe_instrument_id;
use crate::backend::{
    catalog::safe_directory_identifier,
    custom::{augment_batch_with_data_type_column, schema_with_data_type_column},
};

#[derive(Debug, Default, PartialEq, PartialOrd, Hash, Eq, Clone)]
pub struct FileWriterPath {
    path: Path,
    type_str: String,
    instrument_id: Option<String>,
}

/// A `FeatherBuffer` encodes data via an Arrow `StreamWriter`.
///
/// It flushes the internal byte buffer according to rotation policy.
pub struct FeatherBuffer {
    /// Arrow `StreamWriter` that writes to an in-memory `Vec<u8>`.
    writer: StreamWriter<Vec<u8>>,
    /// Current size in bytes.
    size: u64,
    /// TODO: Optional next rotation timestamp.
    // next_rotation: Option<UnixNanos>,
    /// Schema of the data being written.
    schema: Schema,
    /// Maximum buffer size in bytes.
    max_buffer_size: u64,
    /// Rotation config
    rotation_config: RotationConfig,
}

impl FeatherBuffer {
    /// Creates a new [`FeatherBuffer`] using the given path, schema and maximum buffer size.
    pub fn new(schema: &Schema, rotation_config: RotationConfig) -> Result<Self, ArrowError> {
        let writer = StreamWriter::try_new(Vec::new(), schema)?;
        let mut max_buffer_size = 1_000_000_000_000; // 1 GB

        if let RotationConfig::Size { max_size } = &rotation_config {
            max_buffer_size = *max_size;
        }

        Ok(Self {
            writer,
            size: 0,
            // next_rotation: None,
            max_buffer_size,
            schema: schema.clone(),
            rotation_config,
        })
    }

    /// Writes the given `RecordBatch` to the internal buffer.
    ///
    /// Returns true if it should be rotated according rotation policy
    pub fn write_record_batch(&mut self, batch: &RecordBatch) -> Result<bool, ArrowError> {
        self.writer.write(batch)?;
        self.size += batch.get_array_memory_size() as u64;
        Ok(self.size >= self.max_buffer_size)
    }

    /// Consumes the writer and returns the buffer of bytes from the `StreamWriter`
    pub fn take_buffer(&mut self) -> Result<Vec<u8>, ArrowError> {
        let mut writer = StreamWriter::try_new(Vec::new(), &self.schema)?;
        std::mem::swap(&mut self.writer, &mut writer);
        let buffer = writer.into_inner()?;
        // TODO: Handle rotation config here
        self.size = 0;
        Ok(buffer)
    }

    /// Should rotate
    #[must_use]
    pub const fn should_rotate(&self) -> bool {
        match &self.rotation_config {
            RotationConfig::Size { max_size } => self.size >= *max_size,
            _ => false,
        }
    }
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
        rotation_timezone: Tz,
    },
    /// No automatic rotation.
    NoRotation,
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
    /// Clock for timestamps and rotation.
    clock: Rc<RefCell<dyn Clock>>,
    /// Rotation configuration.
    rotation_config: RotationConfig,
    /// Optional set of type names to include.
    included_types: Option<HashSet<String>>,
    /// Set of types that should be split by instrument.
    per_instrument_types: HashSet<String>,
    /// Map of active `FeatherBuffers` keyed by their path.
    writers: HashMap<FileWriterPath, FeatherBuffer>,
    /// Map of next rotation times keyed by their path.
    next_rotation_times: HashMap<FileWriterPath, UnixNanos>,
    /// Runtime handle for async operations.
    runtime: tokio::runtime::Handle,
    /// Flush interval in milliseconds (0 = no automatic flushing).
    flush_interval_ms: u64,
    /// Last flush timestamp in nanoseconds.
    last_flush_ns: UnixNanos,
    /// Bounded cache of recently seen event IDs for deduplication.
    seen_event_ids: Box<FifoCache<UUID4, 10_000>>,
}

impl FeatherWriter {
    /// Creates a new [`FeatherWriter`] instance.
    pub fn new(
        base_path: String,
        store: Arc<dyn ObjectStore>,
        clock: Rc<RefCell<dyn Clock>>,
        rotation_config: RotationConfig,
        included_types: Option<HashSet<String>>,
        per_instrument_types: Option<HashSet<String>>,
        flush_interval_ms: Option<u64>,
    ) -> Self {
        // Get the runtime handle for async operations
        let runtime = nautilus_common::live::get_runtime().handle().clone();
        let flush_interval_ms = flush_interval_ms.unwrap_or(1000); // Default 1 second
        let last_flush_ns = clock.borrow().timestamp_ns();

        Self {
            base_path,
            store,
            clock,
            rotation_config,
            included_types,
            per_instrument_types: per_instrument_types.unwrap_or_default(),
            writers: HashMap::new(),
            next_rotation_times: HashMap::new(),
            runtime,
            flush_interval_ms,
            last_flush_ns,
            seen_event_ids: Box::new(FifoCache::new()),
        }
    }

    /// Writes a single data value.
    /// This is the user entry point. The data is encoded into a `RecordBatch` and written to the appropriate `FileWriter`.
    /// If the writer's buffer reaches capacity or meets rotation criteria (based on the rotation configuration),
    /// the `FileWriter` is flushed to the object store and replaced.
    pub async fn write<T>(&mut self, data: T) -> Result<(), Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        if !self.should_write::<T>() {
            return Ok(());
        }

        let path = self.get_writer_path(&data)?;

        // Create a new FileWriter if one does not exist.
        if !self.writers.contains_key(&path) {
            self.create_writer::<T>(path.clone(), &data)?;
        }

        // Encode the data into a RecordBatch using T's encoding logic.
        let batch = T::encode_batch(&T::metadata(&data), &[data])?;

        // Write the RecordBatch to the appropriate FileWriter.
        if let Some(writer) = self.writers.get_mut(&path) {
            let should_rotate = writer.write_record_batch(&batch)?;
            if should_rotate || self.check_scheduled_rotation(&path) {
                self.rotate_writer(&path).await?;
            }
        }

        // Check if we should auto-flush based on time interval
        self.check_flush().await?;

        Ok(())
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
    pub async fn write_batch<T>(&mut self, data: Vec<T>) -> Result<(), Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        if data.is_empty() || !self.should_write::<T>() {
            return Ok(());
        }

        // Group by logical writer identity (instrument_id for per-instrument types).
        // Grouping on FileWriterPath would split same-instrument rows across distinct
        // timestamped paths when the writer does not yet exist under a LiveClock.
        let type_str = T::path_prefix();
        let needs_instrument =
            self.per_instrument_types.contains(type_str) || type_str.starts_with("custom_");

        let mut groups: AHashMap<Option<String>, Vec<T>> = AHashMap::new();

        for item in data {
            let instrument_id = if needs_instrument {
                T::metadata(&item).get(KEY_INSTRUMENT_ID).cloned()
            } else {
                None
            };
            groups.entry(instrument_id).or_default().push(item);
        }

        for group in groups.into_values() {
            let path = self.get_writer_path(&group[0])?;
            let metadata = T::chunk_metadata(&group);

            if !self.writers.contains_key(&path) {
                self.create_writer_with_metadata::<T>(path.clone(), metadata.clone())?;
            }

            let batch = T::encode_batch(&metadata, &group)?;

            if let Some(writer) = self.writers.get_mut(&path) {
                let should_rotate = writer.write_record_batch(&batch)?;
                if should_rotate || self.check_scheduled_rotation(&path) {
                    self.rotate_writer(&path).await?;
                }
            }
        }

        self.check_flush().await?;

        Ok(())
    }

    /// Checks if enough time has passed since last flush and flushes if needed.
    async fn check_flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.flush_interval_ms == 0 {
            return Ok(()); // Auto-flush disabled
        }

        let now_ns = self.clock.borrow().timestamp_ns();
        let elapsed_ms = (now_ns.as_u64() - self.last_flush_ns.as_u64()) / 1_000_000;

        if elapsed_ms >= self.flush_interval_ms {
            self.flush().await?;
            self.last_flush_ns = now_ns;
        }

        Ok(())
    }

    fn check_scheduled_rotation(&mut self, path: &FileWriterPath) -> bool {
        match self.rotation_config {
            RotationConfig::Interval { interval_ns } => {
                let now = self.clock.borrow().timestamp_ns();
                let next_rotation = self.next_rotation_times.get(path).copied();

                match next_rotation {
                    None => {
                        self.next_rotation_times
                            .insert(path.clone(), now + interval_ns);
                        false
                    }
                    Some(next) if now >= next => {
                        self.next_rotation_times
                            .insert(path.clone(), now + interval_ns);
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
                let now = self.clock.borrow().timestamp_ns();
                let next_rotation = self.next_rotation_times.get(path).copied();

                match next_rotation {
                    None => {
                        let next = self.calculate_next_scheduled_rotation(
                            rotation_time,
                            rotation_timezone,
                            interval_ns,
                        );
                        self.next_rotation_times.insert(path.clone(), next);
                        false
                    }
                    Some(next) if now >= next => {
                        self.next_rotation_times
                            .insert(path.clone(), now + interval_ns);
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
        rotation_timezone: Tz,
        interval_ns: u64,
    ) -> UnixNanos {
        use chrono::TimeZone;
        let now_utc = self.clock.borrow().utc_now();
        let now_tz = now_utc.with_timezone(&rotation_timezone);

        let rotation_time_secs = (*rotation_time / 1_000_000_000) as u32;
        let rotation_time_nanos = (*rotation_time % 1_000_000_000) as u32;
        let rotation_time_naive = chrono::NaiveTime::from_num_seconds_from_midnight_opt(
            rotation_time_secs,
            rotation_time_nanos,
        )
        .unwrap_or_else(|| chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap());

        let mut next_rotation_tz = rotation_timezone
            .from_local_datetime(&now_tz.date_naive().and_time(rotation_time_naive))
            .earliest()
            .unwrap_or(now_tz);

        if next_rotation_tz <= now_tz {
            // If the time has already passed today, we would usually add the interval
            // But let's align exactly with how Python does it:
            while next_rotation_tz <= now_tz {
                // Add interval_ns to next_rotation_tz
                // Since chrono::Duration doesn't take u64 nanos directly comfortably for large values,
                // we'll convert to seconds and nanos.
                let secs = (interval_ns / 1_000_000_000) as i64;
                let nanos = (interval_ns % 1_000_000_000) as u32;
                next_rotation_tz = next_rotation_tz
                    + chrono::Duration::seconds(secs)
                    + chrono::Duration::nanoseconds(nanos as i64);
            }
        }

        UnixNanos::from(
            next_rotation_tz
                .with_timezone(&chrono::Utc)
                .timestamp_nanos_opt()
                .unwrap_or(0) as u64,
        )
    }

    /// Flushes and rotates `FileWriter` associated with `key`.
    /// TODO: Fix error type to handle arrow error and object store error
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
        let schema = if self.per_instrument_types.contains(T::path_prefix()) {
            T::get_schema(Some(metadata))
        } else {
            T::get_schema(None)
        };

        let writer = FeatherBuffer::new(&schema, self.rotation_config.clone())?;
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
        let writer = FeatherBuffer::new(&schema, self.rotation_config.clone())
            .map_err(|e| format!("Failed to create feather buffer for custom {type_name}: {e}"))?;
        self.writers.insert(path, writer);
        Ok(())
    }

    /// Encodes a single `CustomData` into a `RecordBatch` with `data_type` column (catalog-compatible).
    fn encode_custom_to_batch(
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
                let bytes = writer.take_buffer()?;
                if !bytes.is_empty() {
                    // Write to the object store
                    self.store.put(&path.path, bytes.into()).await?;
                }

                // Recreate writer with same schema for continued writing
                // We need the schema and type info - for now, we'll recreate on next write
                // The writer will be recreated automatically when write() is called again
            }
        }

        self.last_flush_ns = self.clock.borrow().timestamp_ns();
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
    pub fn is_closed(&self) -> bool {
        self.writers.is_empty()
    }

    /// Returns information about the current files being written.
    ///
    /// Each entry maps a writer key (type_str and optional instrument_id) to
    /// its current buffer size and file path.
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

    /// Returns the next rotation time for a specific writer key, if set.
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

    /// Determines whether type T should be written, based on the inclusion filter.
    fn should_write<T: CatalogPathPrefix>(&self) -> bool {
        self.included_types.as_ref().is_none_or(|included| {
            let path = T::path_prefix();
            included.contains(path)
        })
    }

    /// Returns whether the given event ID has already been seen,
    /// adding it to the cache if new.
    pub fn is_duplicate_event_id(&mut self, event_id: &UUID4) -> bool {
        if self.seen_event_ids.contains(event_id) {
            return true;
        }

        self.seen_event_ids.add(*event_id);

        false
    }

    fn regen_writer_path(&self, path: &FileWriterPath) -> FileWriterPath {
        let type_str = path.type_str.clone();
        let instrument_id = path.instrument_id.clone();
        let timestamp = self.clock.borrow().timestamp_ns();
        // Note: Path removes prefixing slashes
        let mut path = Path::from(self.base_path.clone());

        if type_str.starts_with("data/custom/") {
            // Custom data: data/custom/{type_name}/[{identifier_segments}/]{file_stem}_{ts}.feather
            let type_name = type_str.strip_prefix("data/custom/").unwrap_or(&type_str);
            path = path.join("data").join("custom").join(type_name.to_string());

            if let Some(ref id) = instrument_id {
                let safe = safe_directory_identifier(id);
                if !safe.is_empty() {
                    for segment in safe.split('/') {
                        path = path.join(segment.to_string());
                    }
                }
            }
            let file_stem = instrument_id.as_deref().unwrap_or(type_name);
            path = path.join(format!("{file_stem}_{timestamp}.feather"));
        } else if let Some(ref instrument_id) = instrument_id {
            let safe_id = urisafe_instrument_id(instrument_id);
            path = path.join(type_str.clone());
            path = path.join(safe_id.clone());
            path = path.join(format!("{safe_id}_{timestamp}.feather"));
        } else {
            path = path.join(format!("{type_str}_{timestamp}.feather"));
        }

        FileWriterPath {
            path,
            type_str,
            instrument_id,
        }
    }

    /// Builds `FileWriterPath` for custom data using DataType identifier as folder partition (catalog layout).
    fn get_writer_path_custom(&self, type_name: &str, identifier: Option<&str>) -> FileWriterPath {
        let timestamp = self.clock.borrow().timestamp_ns();
        let type_str = format!("data/custom/{type_name}");
        let instrument_id = identifier.map(String::from);

        let mut path = Path::from(self.base_path.clone());
        path = path.join("data").join("custom").join(type_name.to_string());

        if let Some(id) = &identifier {
            let safe = safe_directory_identifier(id);
            if !safe.is_empty() {
                for segment in safe.split('/') {
                    path = path.join(segment.to_string());
                }
            }
        }
        let file_stem = identifier.unwrap_or(type_name);
        path = path.join(format!("{file_stem}_{timestamp}.feather"));

        FileWriterPath {
            path,
            type_str,
            instrument_id,
        }
    }

    /// Generates a key for a `FileWriter` based on type T and optional instrument ID.
    /// Reuses an existing writer key (same type_str and instrument_id) if present, so we
    /// buffer multiple items in the same file until rotation; otherwise creates a new path with current timestamp.
    fn get_writer_path<T>(&self, data: &T) -> Result<FileWriterPath, Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix,
    {
        let type_str = T::path_prefix();
        let metadata = T::metadata(data);

        let instrument_id = if self.per_instrument_types.contains(type_str)
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

        let timestamp = self.clock.borrow().timestamp_ns();
        let mut path = Path::from(self.base_path.clone());

        if let Some(ref instrument_id) = instrument_id {
            let safe_id = urisafe_instrument_id(instrument_id);
            path = path.join(type_str);
            path = path.join(safe_id.clone());
            path = path.join(format!("{safe_id}_{timestamp}.feather"));
        } else {
            path = path.join(format!("{type_str}_{timestamp}.feather"));
        }

        Ok(FileWriterPath {
            path,
            type_str: type_str.to_string(),
            instrument_id,
        })
    }

    /// Writes a Data enum value to the appropriate writer.
    ///
    /// This is a convenience method that routes the Data enum to the appropriate
    /// typed write method.
    pub async fn write_data(&mut self, data: Data) -> Result<(), Box<dyn std::error::Error>> {
        match data {
            Data::Quote(quote) => self.write(quote).await,
            Data::Trade(trade) => self.write(trade).await,
            Data::Bar(bar) => self.write(bar).await,
            Data::Delta(delta) => self.write(delta).await,
            Data::Depth10(depth) => self.write(*depth).await,
            Data::IndexPriceUpdate(price) => self.write(price).await,
            Data::MarkPriceUpdate(price) => self.write(price).await,
            Data::InstrumentStatus(status) => self.write(status).await,
            Data::InstrumentClose(close) => self.write(close).await,
            Data::Custom(custom) => self.write_custom_data(&custom).await,
            Data::Deltas(deltas_api) => {
                // Batch write so chunk_metadata can skip a leading BookAction::Clear sentinel
                self.write_batch(deltas_api.deltas.clone()).await
            }
        }
    }

    /// Writes a single custom data value (catalog layout: data/custom/{type_name}/[{identifier}/]).
    async fn write_custom_data(
        &mut self,
        custom: &CustomData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let type_name = custom.data.type_name();
        let identifier = custom.data_type.identifier().map(String::from);

        if !self.should_write_custom(type_name) {
            return Ok(());
        }

        let path = self.get_writer_path_custom(type_name, identifier.as_deref());
        if !self.writers.contains_key(&path) {
            self.create_custom_writer(path.clone(), type_name)?;
        }

        let batch = Self::encode_custom_to_batch(custom)?;

        if let Some(writer) = self.writers.get_mut(&path) {
            let should_rotate = writer.write_record_batch(&batch)?;
            if should_rotate || self.check_scheduled_rotation(&path) {
                self.rotate_writer(&path).await?;
            }
        }

        self.check_flush().await?;
        Ok(())
    }

    fn should_write_custom(&self, type_name: &str) -> bool {
        self.included_types.as_ref().is_none_or(|included| {
            included.contains(type_name)
                || included.contains("custom")
                || included.contains(&format!("custom/{type_name}"))
        })
    }

    /// Writes an instrument to the appropriate writer.
    ///
    /// Instruments are written to feather files and organized by instrument ID.
    /// This method supports writing instruments that implement `EncodeToRecordBatch` and `CatalogPathPrefix`.
    pub async fn write_instrument(
        &mut self,
        instrument: InstrumentAny,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.write(instrument).await
    }

    /// Subscribes to all messages on the message bus (pattern "*").
    ///
    /// This will automatically write all supported data types that are published
    /// on the message bus to the feather files.
    ///
    /// The writer must be wrapped in `Rc<RefCell<>>` to be shareable with the message bus handler.
    ///
    /// Note: The handler spawns async tasks to write data, so writes happen asynchronously
    /// and won't block the message bus.
    pub fn subscribe_to_message_bus(
        writer: Rc<RefCell<Self>>,
    ) -> Result<ShareableMessageHandler, Box<dyn std::error::Error>> {
        let runtime = writer.borrow().runtime.clone();

        // Create handler that downcasts messages and writes them
        // Note: We use Handle::enter() to allow blocking in the handler context
        // This works when the handler is called from outside an async runtime
        let handler = ShareableMessageHandler::from_any(move |message: &dyn Any| {
            // Enter the runtime context to allow blocking
            let _guard = runtime.enter();

            // Try to downcast to various data types and write them
            macro_rules! try_write {
                ($message:expr, $type:ty, $name:literal) => {
                    if let Some(value) = $message.downcast_ref::<$type>() {
                        let mut writer = writer.borrow_mut();
                        if let Err(e) = runtime.block_on(writer.write(value.clone())) {
                            log::warn!("Failed to write {}: {e}", $name);
                        }
                        return;
                    }
                };
            }

            try_write!(message, QuoteTick, "QuoteTick");
            try_write!(message, TradeTick, "TradeTick");
            try_write!(message, Bar, "Bar");
            try_write!(message, OrderBookDelta, "OrderBookDelta");
            try_write!(message, OrderBookDepth10, "OrderBookDepth10");
            try_write!(message, IndexPriceUpdate, "IndexPriceUpdate");
            try_write!(message, MarkPriceUpdate, "MarkPriceUpdate");
            try_write!(message, InstrumentStatus, "InstrumentStatus");
            try_write!(message, InstrumentClose, "InstrumentClose");
            try_write!(message, FundingRateUpdate, "FundingRateUpdate");
            try_write!(message, AccountState, "AccountState");
            try_write!(message, OrderInitialized, "OrderInitialized");
            try_write!(message, OrderDenied, "OrderDenied");
            try_write!(message, OrderEmulated, "OrderEmulated");
            try_write!(message, OrderSubmitted, "OrderSubmitted");
            try_write!(message, OrderAccepted, "OrderAccepted");
            try_write!(message, OrderRejected, "OrderRejected");
            try_write!(message, OrderPendingCancel, "OrderPendingCancel");
            try_write!(message, OrderCanceled, "OrderCanceled");
            try_write!(message, OrderCancelRejected, "OrderCancelRejected");
            try_write!(message, OrderExpired, "OrderExpired");
            try_write!(message, OrderTriggered, "OrderTriggered");
            try_write!(message, OrderPendingUpdate, "OrderPendingUpdate");
            try_write!(message, OrderReleased, "OrderReleased");
            try_write!(message, OrderModifyRejected, "OrderModifyRejected");
            try_write!(message, OrderUpdated, "OrderUpdated");
            try_write!(message, OrderFilled, "OrderFilled");
            try_write!(message, PositionOpened, "PositionOpened");
            try_write!(message, PositionChanged, "PositionChanged");
            try_write!(message, PositionClosed, "PositionClosed");
            try_write!(message, PositionAdjusted, "PositionAdjusted");
            try_write!(message, OrderSnapshot, "OrderSnapshot");
            try_write!(message, PositionSnapshot, "PositionSnapshot");
            try_write!(message, OrderStatusReport, "OrderStatusReport");
            try_write!(message, FillReport, "FillReport");
            try_write!(message, PositionStatusReport, "PositionStatusReport");
            try_write!(message, ExecutionMassStatus, "ExecutionMassStatus");

            if let Some(deltas) = message.downcast_ref::<OrderBookDeltas>() {
                // Batch write so chunk_metadata can skip a leading BookAction::Clear sentinel
                let mut writer = writer.borrow_mut();
                if let Err(e) = runtime.block_on(writer.write_batch(deltas.deltas.clone())) {
                    log::warn!("Failed to write OrderBookDeltas: {e}");
                }
            } else if let Some(custom) = message.downcast_ref::<CustomData>() {
                let mut writer = writer.borrow_mut();
                if let Err(e) = runtime.block_on(writer.write_data(Data::Custom(custom.clone()))) {
                    log::warn!("Failed to write CustomData: {e}");
                }
            } else if let Some(instrument) = message.downcast_ref::<InstrumentAny>() {
                let mut writer = writer.borrow_mut();
                if let Err(e) = runtime.block_on(writer.write_instrument(instrument.clone())) {
                    log::warn!("Failed to write InstrumentAny: {e}");
                }
            }
            // Silently ignore unsupported message types.
        });

        // Subscribe to all messages using wildcard pattern
        subscribe_any(
            MStr::pattern("*"),
            handler.clone(),
            None, // No priority
        );

        Ok(handler)
    }

    /// Unsubscribes from the message bus.
    pub fn unsubscribe_from_message_bus(handler: &ShareableMessageHandler) {
        unsubscribe_any(MStr::pattern("*"), handler);
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use datafusion::arrow::ipc::reader::StreamReader;
    use nautilus_common::clock::TestClock;
    use nautilus_model::{
        data::{Data, OrderBookDeltas_API, QuoteTick, TradeTick},
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

    #[tokio::test]
    async fn test_writer_manager_keys() {
        // Create a temporary directory for base path
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();

        // Create a LocalFileSystem based object store using the temp directory
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);

        // Create a test clock
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let timestamp = clock.borrow().timestamp_ns();

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
            UnixNanos::from(1000000000000000000),
            UnixNanos::from(1000000000000000000),
        );

        let trade = TradeTick::new(
            InstrumentId::from(instrument_id),
            Price::from("100.0"),
            Quantity::from("100.0"),
            AggressorSide::Buyer,
            TradeId::from("1"),
            UnixNanos::from(1000000000000000000),
            UnixNanos::from(1000000000000000000),
        );

        manager.write(quote).await.unwrap();
        manager.write(trade).await.unwrap();

        // Check keys and paths for quotes and trades
        let path = manager.get_writer_path(&quote).unwrap();
        let safe_id = instrument_id.replace('/', "");
        let expected_path = Path::from(format!(
            "{base_path}/quotes/{safe_id}/{safe_id}_{timestamp}.feather"
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

        let mut writer = FeatherBuffer::new(&schema, RotationConfig::NoRotation).unwrap();
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

    #[tokio::test]
    async fn test_round_trip() {
        // Create a temporary directory for base path
        let temp_dir = TempDir::new_in(".").unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();

        // Create a LocalFileSystem based object store using the temp directory
        let local_fs = LocalFileSystem::new_with_prefix(&base_path).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);

        // Create a test clock
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

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
            AggressorSide::Buyer,
            TradeId::from("1"),
            UnixNanos::from(100),
            UnixNanos::from(100),
        );

        manager.write(quote).await.unwrap();
        manager.write(trade).await.unwrap();

        let paths = manager.writers.keys().cloned().collect::<Vec<_>>();
        assert_eq!(paths.len(), 2);

        // Flush data
        manager.flush().await.unwrap();

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

    #[tokio::test]
    async fn test_write_data_enum() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

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
        writer.write_data(Data::Quote(quote)).await.unwrap();
        writer.flush().await.unwrap();

        // Verify file was created
        assert!(!writer.writers.is_empty() || temp_dir.path().read_dir().unwrap().count() > 0);
    }

    #[tokio::test]
    async fn test_write_data_all_types() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

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
        writer.write_data(Data::Quote(quote)).await.unwrap();

        let trade = TradeTick::new(
            instrument_id,
            Price::from("1.0"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::from("1"),
            UnixNanos::from(2000),
            UnixNanos::from(2000),
        );
        writer.write_data(Data::Trade(trade)).await.unwrap();

        let delta = OrderBookDelta::clear(
            instrument_id,
            0,
            UnixNanos::from(3000),
            UnixNanos::from(3000),
        );
        writer.write_data(Data::Delta(delta)).await.unwrap();

        writer.flush().await.unwrap();
    }

    #[tokio::test]
    async fn test_auto_flush() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

        let mut writer = FeatherWriter::new(
            base_path,
            store,
            clock.clone(),
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

        // Write first quote
        writer.write(quote).await.unwrap();

        // Note: TestClock doesn't have set_time_ns, so we can't easily test auto-flush
        // with time advancement. Instead, we test that check_flush is called during write.
        // For a proper test, we'd need a mock clock or use LiveClock with time advancement.

        // Write second quote - check_flush will be called but won't flush if time hasn't advanced
        let quote2 = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.1"),
            Price::from("1.1"),
            Quantity::from("1000"),
            Quantity::from("1000"),
            UnixNanos::from(2000),
            UnixNanos::from(2000),
        );
        writer.write(quote2).await.unwrap();

        // Verify that writes succeeded (check_flush was called, even if it didn't flush)
        // The flush_interval_ms is set, so check_flush runs but won't flush without time advancement
    }

    #[tokio::test]
    async fn test_close() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

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

        writer.write(quote).await.unwrap();
        assert!(!writer.writers.is_empty());

        writer.close().await.unwrap();
        assert!(writer.writers.is_empty());
    }

    // Note: Message bus subscription test is skipped due to async/sync boundary complexity.
    // The handler uses block_on which can't be used from within an async runtime.
    // This functionality is better tested via Python integration tests where the message bus
    // is used in a non-async context or via proper async task spawning.

    #[tokio::test]
    async fn test_write_data_orderbook_deltas() {
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

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
        let deltas_api = OrderBookDeltas_API::new(deltas);

        // Test writing OrderBookDeltas via write_data
        writer.write_data(Data::Deltas(deltas_api)).await.unwrap();
        writer.flush().await.unwrap();
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
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));

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
            .await
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
}
