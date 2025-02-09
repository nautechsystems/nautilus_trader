use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use datafusion::arrow::datatypes::Schema;
use datafusion::arrow::error::ArrowError;
use datafusion::arrow::ipc::writer::StreamWriter;
use datafusion::arrow::record_batch::RecordBatch;

use object_store::{path::Path, ObjectStore};

use super::catalog::CatalogPathPrefix;
use nautilus_common::clock::Clock;
use nautilus_core::UnixNanos;
use nautilus_serialization::arrow::{EncodeToRecordBatch, KEY_INSTRUMENT_ID};

#[derive(Debug, Default, PartialEq, PartialOrd, Hash, Eq, Clone)]
pub struct FileWriterPath {
    path: Path,
    type_str: String,
    instrument_id: Option<String>,
}

/// A FileWriter encodes data via an Arrow StreamWriter.
///
/// It flushes the internal buffer to the object according to rotation policy.
pub struct FileWriter {
    /// Arrow StreamWriter that writes to an in-memory Vec<u8>.
    writer: StreamWriter<Vec<u8>>,
    /// Current size in bytes.
    size: u64,
    /// TODO: Optional next rotation timestamp.
    next_rotation: Option<UnixNanos>,
    /// Schema of the data being written.
    schema: Schema,
    /// Maximum buffer size in bytes.
    max_buffer_size: u64,
    /// Rotation config
    rotation_config: RotationConfig,
}

impl FileWriter {
    /// Creates a new FileWriter using the given path, schema and maximum buffer size.
    pub fn new(schema: &Schema, rotation_config: RotationConfig) -> Result<Self, ArrowError> {
        let writer = StreamWriter::try_new(Vec::new(), schema)?;
        let mut max_buffer_size = 1_000_000_000_000; // 1 GB

        if let RotationConfig::Size { max_size } = &rotation_config {
            max_buffer_size = *max_size;
        };

        Ok(Self {
            writer,
            size: 0,
            next_rotation: None,
            max_buffer_size,
            schema: schema.clone(),
            rotation_config,
        })
    }

    /// Writes the given RecordBatch to the internal buffer.
    ///
    /// Returns true if it should be rotated according rotation policy
    pub fn write_record_batch(&mut self, batch: &RecordBatch) -> Result<bool, ArrowError> {
        self.writer.write(batch)?;
        self.size += batch.get_array_memory_size() as u64;
        Ok(self.size >= self.max_buffer_size)
    }

    /// Consumes the writer and returns the buffer of bytes from the StreamWriter
    pub fn take_buffer(&mut self) -> Result<Vec<u8>, ArrowError> {
        let mut writer = StreamWriter::try_new(Vec::new(), &self.schema)?;
        std::mem::swap(&mut self.writer, &mut writer);
        let buffer = writer.into_inner()?;
        // TODO: Handle rotation config here
        self.size = 0;
        Ok(buffer)
    }

    /// Should rotate
    pub fn should_rotate(&self) -> bool {
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

/// Manages multiple FileWriters and handles encoding, rotation, and flushing to the object store.
///
/// The `write()` method is the single entry point for clients: they supply a data value (of generic type T)
/// and the manager encodes it (using T's metadata via EncodeToRecordBatch), routes it by CatalogPathPrefix,
/// and writes it to the appropriate FileWriter. When a writer's buffer is full or rotation criteria are met,
/// its contents are flushed to the object store and it is replaced.
pub struct FileWriterManager {
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
    /// Map of active FileWriters keyed by their writer key.
    writers: HashMap<FileWriterPath, FileWriter>,
}

impl FileWriterManager {
    /// Creates a new FileWriterManager instance.
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
    /// This is the user entry point. The data is encoded into a RecordBatch and written to the appropriate FileWriter.
    /// If the writer's buffer reaches capacity or meets rotation criteria (based on the rotation configuration),
    /// the FileWriter is flushed to the object store and replaced.
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
            self.create_writer::<T>(path.clone())?;
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

    /// Flushes and rotates FileWriter associated with `key`.
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

    /// Creates (and inserts) a new FileWriter for type T.
    fn create_writer<T>(&mut self, path: FileWriterPath) -> Result<(), ArrowError>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        let writer = FileWriter::new(&T::get_schema(None), self.rotation_config.clone())?;
        self.writers.insert(path, writer);
        Ok(())
    }

    /// Flushes all active FileWriters by writing any remaining buffered bytes to the object store.
    pub async fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for (path, mut writer) in self.writers.drain() {
            let bytes = writer.take_buffer()?;
            self.store.put(&path.path, bytes.into()).await?;
        }
        Ok(())
    }

    /// Determines whether type T should be written, based on the inclusion filter.
    fn should_write<T: CatalogPathPrefix>(&self) -> bool {
        self.included_types
            .as_ref()
            .map(|included| {
                let path = T::path_prefix();
                included.contains(path)
            })
            .unwrap_or(true)
    }

    fn regen_writer_path(
        &self,
        path: &FileWriterPath,
    ) -> Result<FileWriterPath, Box<dyn std::error::Error>> {
        let type_str = path.type_str.clone();
        let instrument_id = path.instrument_id.clone();
        let timestamp = self.clock.borrow().timestamp_ns();
        let mut path = Path::from(self.base_path.clone());
        if let Some(ref instrument_id) = instrument_id {
            path = path.child(type_str.clone());
            path = path.child(format!("{}_{}.feather", instrument_id, timestamp));
        } else {
            path = path.child(format!("{}_{}.feather", type_str, timestamp));
        }

        Ok(FileWriterPath {
            path,
            type_str,
            instrument_id,
        })
    }

    /// Generates a key for a FileWriter based on type T and optional instrument ID.
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
            path = path.child(format!("{}_{}.feather", instrument_id, timestamp));
        } else {
            path = path.child(format!("{}_{}.feather", type_str, timestamp));
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
    use super::*;
    use datafusion::arrow::ipc::reader::FileReader;
    use nautilus_common::clock::TestClock;
    use nautilus_model::data::Data;
    use nautilus_model::{
        data::{QuoteTick, TradeTick},
        enums::AggressorSide,
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };
    use nautilus_serialization::arrow::DecodeDataFromRecordBatch;
    use object_store::local::LocalFileSystem;
    use object_store::ObjectStore;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_writer_manager_keys() {
        // Create a temporary directory for base path.
        let temp_dir = TempDir::new().unwrap();
        let base_path = temp_dir.path().to_str().unwrap().to_string();

        // Create a LocalFileSystem based object store using the temp directory.
        let local_fs = LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);

        // Create a test clock.
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let timestamp = clock.borrow().timestamp_ns();

        let quote_type_str = QuoteTick::path_prefix();

        let mut per_instrument = HashSet::new();
        per_instrument.insert(quote_type_str.to_string());

        let mut manager = FileWriterManager::new(
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
        let expected_path = format!("/{base_path}/quotes/{instrument_id}_{timestamp}.feather");
        assert_eq!(path.path.to_string(), expected_path);
        assert!(manager.writers.contains_key(&path));
        let writer = manager.writers.get(&path).unwrap();
        assert!(writer.size > 0);

        let path = manager.get_writer_path(&trade).unwrap();
        let expected_path = format!("/{base_path}/trades_{timestamp}.feather");
        assert_eq!(path.path.to_string(), expected_path);
        assert!(manager.writers.contains_key(&path));
        let writer = manager.writers.get(&path).unwrap();
        assert!(writer.size > 0);
    }

    #[tokio::test]
    async fn test_round_trip() {
        // Create a temporary directory for base path.
        // let temp_dir = TempDir::new_in(".").unwrap();
        // let base_path = temp_dir.path().to_str().unwrap().to_string();
        let base_path = ".".to_string();

        // Create a LocalFileSystem based object store using the temp directory.
        let local_fs = LocalFileSystem::new_with_prefix(&base_path).unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(local_fs);

        // Create a test clock.
        let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(TestClock::new()));
        let timestamp = clock.borrow().timestamp_ns();

        let quote_type_str = QuoteTick::path_prefix();

        let mut per_instrument = HashSet::new();
        per_instrument.insert(quote_type_str.to_string());

        let mut manager = FileWriterManager::new(
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

        // Read files from the temporary directory.
        let mut recovered_quotes = Vec::new();
        let mut recovered_trades = Vec::new();
        dbg!(&paths);
        for path in paths {
            let path_str = path.path.to_string();
            let file = std::fs::File::open(&path_str).unwrap();
            let mut reader = FileReader::try_new(file, None).unwrap();
            let metadata = reader.custom_metadata().clone();
            while let Some(batch) = reader.next() {
                let batch = batch.unwrap();
                if path_str.contains("quotes") {
                    // Use QuoteTick's decode_batch for files with "quotes" in their paths.
                    let decoded = QuoteTick::decode_data_batch(&metadata, batch).unwrap();
                    recovered_quotes.extend(decoded);
                } else if path_str.contains("trades") {
                    // Use TradeTick's decode_batch for files with "trades" in their paths.
                    let decoded = TradeTick::decode_data_batch(&metadata, batch).unwrap();
                    recovered_trades.extend(decoded);
                }
            }
        }

        // Assert that the recovered data matches the written data.
        assert_eq!(recovered_quotes.len(), 1, "Expected one QuoteTick record");
        assert_eq!(recovered_trades.len(), 1, "Expected one TradeTick record");

        // Check key fields to ensure the data round-tripped correctly.
        assert_eq!(recovered_quotes[0], Data::from(quote));
        assert_eq!(recovered_trades[0], Data::from(trade));
    }
}
