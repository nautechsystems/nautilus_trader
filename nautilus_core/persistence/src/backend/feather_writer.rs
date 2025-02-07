use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
use nautilus_serialization::arrow::EncodeToRecordBatch;

/// A FileWriter encodes data via an Arrow StreamWriter.
///
/// It flushes the internal buffer to the object according to rotation policy.
pub struct FileWriter {
    /// Output path.
    path: Path,
    /// Arrow StreamWriter that writes to an in-memory Vec<u8>.
    writer: StreamWriter<Vec<u8>>,
    /// Current size in bytes.
    size: u64,
    /// Optional next rotation timestamp.
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
    pub fn new(
        path: Path,
        schema: &Schema,
        rotation_config: RotationConfig,
    ) -> Result<Self, ArrowError> {
        let writer = StreamWriter::try_new(Vec::new(), schema)?;
        let mut max_buffer_size = 1_000_000_000_000; // 1 GB

        if let RotationConfig::Size { max_size } = &rotation_config {
            max_buffer_size = *max_size;
        };

        Ok(Self {
            path,
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

    /// Returns the file path where the data should be written.
    pub fn path(&self) -> &Path {
        &self.path
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
    base_path: PathBuf,
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
    writers: HashMap<String, FileWriter>,
}

impl FileWriterManager {
    /// Creates a new FileWriterManager instance.
    pub fn new(
        base_path: PathBuf,
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
    pub async fn write<T>(
        &mut self,
        data: T,
        instrument_id: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        if !self.should_write::<T>() {
            return Ok(());
        }

        let key = self.get_writer_key::<T>(instrument_id);

        // Create a new FileWriter if one does not exist.
        if !self.writers.contains_key(&key) {
            self.create_writer::<T>(&key)?;
        }

        // Encode the data into a RecordBatch using T's encoding logic.
        let batch = T::encode_batch(&T::metadata(&data), &[data])?;

        // Write the RecordBatch to the appropriate FileWriter.
        if let Some(writer) = self.writers.get_mut(&key) {
            let should_rotate = writer.write_record_batch(&batch)?;
            if should_rotate {
                self.rotate_writer(&key).await?;
            }
        }

        Ok(())
    }

    /// Rotates the FileWriter associated with `key`.
    /// This flushes its current buffer to the object store, then creates a new FileWriter.
    /// TODO: Fix error type to handle arrow error and object store error
    async fn rotate_writer(&mut self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(writer) = self.writers.get_mut(key) {
            let bytes = writer.take_buffer()?;
            self.store.put(writer.path(), bytes.into()).await?;
        }
        Ok(())
    }

    /// Creates (and inserts) a new FileWriter for type T.
    fn create_writer<T>(&mut self, key: &str) -> Result<(), ArrowError>
    where
        T: EncodeToRecordBatch + CatalogPathPrefix + 'static,
    {
        let path = Path::from(key);
        let writer = FileWriter::new(path, &T::get_schema(None), self.rotation_config.clone())?;
        self.writers.insert(key.to_string(), writer);
        Ok(())
    }

    /// Flushes all active FileWriters by writing any remaining buffered bytes to the object store.
    pub async fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for (_key, mut writer) in self.writers.drain() {
            let bytes = writer.take_buffer()?;
            self.store.put(writer.path(), bytes.into()).await?;
        }
        Ok(())
    }

    /// Determines whether type T should be written, based on the inclusion filter.
    fn should_write<T: CatalogPathPrefix>(&self) -> bool {
        self.included_types
            .as_ref()
            .map(|included| {
                let path = T::path_prefix();
                let type_str = path.to_str().unwrap();
                included.contains(type_str)
            })
            .unwrap_or(true)
    }

    /// Generates a key for a FileWriter based on type T and optional instrument ID.
    fn get_writer_key<T>(&self, instrument_id: Option<&str>) -> String
    where
        T: EncodeToRecordBatch + CatalogPathPrefix,
    {
        let path = T::path_prefix();
        let type_str = path.to_str().unwrap();
        if self.per_instrument_types.contains(type_str) {
            format!("{}_{}", type_str, instrument_id.unwrap_or("default"))
        } else {
            type_str.to_string()
        }
    }

    /// Generates a file path for a new FileWriter.
    fn make_path<T: CatalogPathPrefix>(&self, instrument_id: Option<&str>) -> Path {
        let path = T::path_prefix();
        let type_str = path.to_str().unwrap();
        let mut path = Path::from(self.base_path.to_str().unwrap());
        path = path.child(type_str);
        if let Some(id) = instrument_id {
            path = path.child(id);
        }
        let timestamp = self.clock.borrow().timestamp_ns();
        path = path.child(format!("{}_{}.feather", type_str, timestamp));
        path
    }

    // fn check_rotation(&self, writer: &FileWriter) -> bool {
    //     match &self.rotation_config {
    //         RotationConfig::Size { max_size } => writer.size() >= *max_size,
    //         RotationConfig::Interval { interval_ns: _ } => {
    //             if let Some(next) = writer.next_rotation() {
    //                 self.clock.borrow().timestamp_ns() >= next
    //             } else {
    //                 false
    //             }
    //         }
    //         RotationConfig::ScheduledDates { .. } => {
    //             if let Some(next) = writer.next_rotation() {
    //                 self.clock.borrow().timestamp_ns() >= next
    //             } else {
    //                 false
    //             }
    //         }
    //         RotationConfig::NoRotation => false,
    //     }
    // }

    /// Flush all writers
    async fn flush_all(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for (_key, mut writer) in self.writers.drain() {
            let bytes = writer.take_buffer()?;
            self.store.put(writer.path(), bytes.into()).await?;
        }
        Ok(())
    }
}
