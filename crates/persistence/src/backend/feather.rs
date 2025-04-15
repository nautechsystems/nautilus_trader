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

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use datafusion::arrow::{
    datatypes::Schema, error::ArrowError, ipc::writer::StreamWriter, record_batch::RecordBatch,
};
use nautilus_common::clock::Clock;
use nautilus_core::UnixNanos;
use nautilus_serialization::arrow::{EncodeToRecordBatch, KEY_INSTRUMENT_ID};
use object_store::{ObjectStore, path::Path};

use super::catalog::CatalogPathPrefix;

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
    /// Creates a new `FileWriter` using the given path, schema and maximum buffer size.
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
        /// Start of the scheduled rotation period.
        schedule_ns: UnixNanos,
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
}

impl FeatherWriter {
    /// Creates a new `FileWriterManager` instance.
    pub fn new(
        base_path: String,
        store: Arc<dyn ObjectStore>,
        clock: Rc<RefCell<dyn Clock>>,
        rotation_config: RotationConfig,
        included_types: Option<HashSet<String>>,
        per_instrument_types: Option<HashSet<String>>,
    ) -> Self {
        Self {
            base_path,
            store,
            clock,
            rotation_config,
            included_types,
            per_instrument_types: per_instrument_types.unwrap_or_default(),
            writers: HashMap::new(),
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
            if should_rotate {
                self.rotate_writer(&path).await?;
            }
        }

        Ok(())
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
        let new_path = self.regen_writer_path(path)?;
        self.writers.insert(new_path, writer);
        Ok(())
    }

    /// Creates (and inserts) a new `FileWriter` for type T.
    fn create_writer<T>(&mut self, path: FileWriterPath, data: &T) -> Result<(), ArrowError>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        let schema = if self.per_instrument_types.contains(T::path_prefix()) {
            let metadata = T::metadata(data);
            T::get_schema(Some(metadata))
        } else {
            T::get_schema(None)
        };

        let writer = FeatherBuffer::new(&schema, self.rotation_config.clone())?;
        self.writers.insert(path, writer);
        Ok(())
    }

    /// Flushes all active `FeatherBuffers` by writing any remaining buffered bytes to the object store.
    ///
    /// Note: This is not called automatically and must be called by the client.
    /// It is expected that no other writes are performed after this.
    pub async fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for (path, mut writer) in self.writers.drain() {
            let bytes = writer.take_buffer()?;
            self.store.put(&path.path, bytes.into()).await?;
        }
        Ok(())
    }

    /// Determines whether type T should be written, based on the inclusion filter.
    fn should_write<T: CatalogPathPrefix>(&self) -> bool {
        self.included_types.as_ref().is_none_or(|included| {
            let path = T::path_prefix();
            included.contains(path)
        })
    }

    fn regen_writer_path(
        &self,
        path: &FileWriterPath,
    ) -> Result<FileWriterPath, Box<dyn std::error::Error>> {
        let type_str = path.type_str.clone();
        let instrument_id = path.instrument_id.clone();
        let timestamp = self.clock.borrow().timestamp_ns();
        // Note: Path removes prefixing slashes
        let mut path = Path::from(self.base_path.clone());
        if let Some(ref instrument_id) = instrument_id {
            path = path.child(type_str.clone());
            path = path.child(format!("{instrument_id}_{timestamp}.feather"));
        } else {
            path = path.child(format!("{type_str}_{timestamp}.feather"));
        }

        Ok(FileWriterPath {
            path,
            type_str,
            instrument_id,
        })
    }

    /// Generates a key for a `FileWriter` based on type T and optional instrument ID.
    fn get_writer_path<T>(&self, data: &T) -> Result<FileWriterPath, Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix,
    {
        let type_str = T::path_prefix();
        let instrument_id = self.per_instrument_types.contains(type_str).then(|| {
            let metadata = T::metadata(data);
            metadata
                .get(KEY_INSTRUMENT_ID)
                .cloned()
                .expect("Data {type_str} expected instrument_id metadata for per instrument writer")
        });

        let timestamp = self.clock.borrow().timestamp_ns();
        let mut path = Path::from(self.base_path.clone());
        if let Some(ref instrument_id) = instrument_id {
            path = path.child(type_str);
            path = path.child(format!("{instrument_id}_{timestamp}.feather"));
        } else {
            path = path.child(format!("{type_str}_{timestamp}.feather"));
        }

        Ok(FileWriterPath {
            path,
            type_str: type_str.to_string(),
            instrument_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use datafusion::arrow::ipc::reader::StreamReader;
    use nautilus_common::clock::TestClock;
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
        let expected_path = Path::from(format!(
            "{base_path}/quotes/{instrument_id}_{timestamp}.feather"
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
}
