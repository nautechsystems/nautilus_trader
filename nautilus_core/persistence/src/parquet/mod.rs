// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

mod implementations;
mod reader;
mod writer;

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::fs::File;
use std::io::Cursor;
use std::slice;

use arrow2::{array::Array, chunk::Chunk, datatypes::Schema, io::parquet::write::Encoding};
use pyo3::types::PyDict;
use pyo3::{ffi, FromPyPointer, Python};

use nautilus_core::cvec::CVec;
use nautilus_core::string::pystr_to_string;
use nautilus_model::data::tick::{QuoteTick, TradeTick};

#[repr(C)]
/// Filter groups based on a field's metadata values.
pub enum GroupFilterArg {
    /// Select groups that have minimum ts_init less than limit.
    TsInitLt(u64),
    /// Select groups that have maximum ts_init greater than limit.
    TsInitGt(u64),
    /// No group filtering applied (to avoid `Option).
    None,
}

impl GroupFilterArg {
    /// Scan metadata and choose which chunks to filter and returns a HashSet
    /// holding the indexes of the selected chunks.
    fn filter_groups(&self, metadata: &FileMetaData, schema: &Schema) -> HashSet<usize> {
        match self {
            // select groups that have minimum ts_init less than limit
            GroupFilterArg::TsInitLt(limit) => {
                if let Some(ts_init_field) =
                    schema.fields.iter().find(|field| field.name.eq("ts_init"))
                {
                    let statistics =
                        read::statistics::deserialize(ts_init_field, &metadata.row_groups)
                            .expect("Cannot extract ts_init statistics");
                    let min_values = statistics
                        .min_value
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .expect("Unable to unwrap minimum value metadata for ts_init statistics");
                    min_values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, ts_group_min)| {
                            let min = ts_group_min.unwrap_or(&u64::MAX);
                            if min < limit {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .collect()
                } else {
                    HashSet::new()
                }
            }
            // select groups that have maximum ts_init time greater than limit
            GroupFilterArg::TsInitGt(limit) => {
                if let Some(ts_init_field) =
                    schema.fields.iter().find(|field| field.name.eq("ts_init"))
                {
                    let statistics =
                        read::statistics::deserialize(ts_init_field, &metadata.row_groups)
                            .expect("Cannot extract ts_init statistics");
                    let max_values = statistics
                        .max_value
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .expect("Unable to unwrap maximum value metadata for ts_init statistics");
                    max_values
                        .iter()
                        .enumerate()
                        .filter_map(|(i, ts_group_max)| {
                            let max = ts_group_max.unwrap_or(&u64::MAX);
                            if max > limit {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .collect()
                } else {
                    HashSet::new()
                }
            }
            GroupFilterArg::None => {
                unreachable!("filter_groups should not be called with None filter")
            }
        }
    }
}

pub struct ParquetReader<A> {
    file_reader: FileReader<File>,
    reader_type: PhantomData<*const A>,
}

impl<A> ParquetReader<A> {
    pub fn new(file_path: &str, chunk_size: usize, filter_arg: GroupFilterArg) -> Self {
        let mut file = File::open(file_path)
            .unwrap_or_else(|_| panic!("unable to open parquet file {file_path}"));

        // TODO: duplicate type definition from arrow2 parquet file reader
        // because it does not expose it
        type GroupFilter = Arc<dyn Fn(usize, &RowGroupMetaData) -> bool + Send + Sync>;
        let group_filter = match filter_arg {
            GroupFilterArg::None => None,
            // a closure that captures the HashSet of indexes of selected chunks
            // and uses this to check if a chunk is selected based on it's index
            _ => {
                let metadata = read::read_metadata(&mut file).expect("unable to read metadata");
                let schema = read::infer_schema(&metadata).expect("unable to infer schema");
                let select_groups = filter_arg.filter_groups(&metadata, &schema);
                let filter_closure: GroupFilter = Arc::new(
                    move |group_index: usize, _metadata: &RowGroupMetaData| -> bool {
                        select_groups.contains(&group_index)
                    },
                );
                Some(filter_closure)
            }
        };

        let fr = FileReader::try_new(file, None, Some(chunk_size), None, group_filter)
            .expect("unable to create reader from file");
        ParquetReader {
            file_reader: fr,
            reader_type: PhantomData,
        }
    }
}

impl<A> Iterator for ParquetReader<A>
where
    A: DecodeFromChunk,
{
    type Item = Vec<A>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(Ok(chunk)) = self.file_reader.next() {
            Some(A::decode(self.file_reader.schema(), chunk))
        } else {
            None
        }
    }
}

pub struct ParquetWriter<A> {
    pub writer: FileWriter<File>,
    pub encodings: Vec<Vec<Encoding>>,
    pub options: WriteOptions,
    pub writer_type: PhantomData<*const A>,
}

impl<'a, A> ParquetWriter<A>
where
    A: EncodeToChunk + 'a + Sized,
{
    pub fn new(path: &str, schema: Schema) -> Self {
        let options = WriteOptions {
            write_statistics: true,
            compression: CompressionOptions::Uncompressed,
            version: Version::V2,
        };

        let encodings = A::encodings(schema.metadata.clone());

        // Create a new empty file
        let file = File::create(path).unwrap();

        let writer = FileWriter::try_new(file, schema, options).unwrap();

        ParquetWriter {
            writer,
            encodings,
            options,
            writer_type: PhantomData,
        }
    }

    pub fn write_bulk<I>(&mut self, data_stream: I) -> Result<()>
    where
        I: Iterator<Item = Vec<A>>,
    {
        let chunk_stream = data_stream.map(|chunk| Ok(A::encode(chunk.iter())));
        let row_groups = RowGroupIterator::try_new(
            chunk_stream,
            self.writer.schema(),
            self.options,
            self.encodings.clone(),
        )?;

        for group in row_groups {
            self.writer.write(group?)?;
        }
        let _size = self.writer.end(None);
        Ok(())
    }

    pub fn write(&mut self, data: &[A]) -> Result<()> {
        let cols = A::encode(data.iter());
        let iter = vec![Ok(cols)];
        let row_groups = RowGroupIterator::try_new(
            iter.into_iter(),
            self.writer.schema(),
            self.options,
            self.encodings.clone(),
        )?;

        for group in row_groups {
            self.writer.write(group?)?;
        }
        Ok(())
    }

    pub fn end_writer(&mut self) {
        let _size = self.writer.end(None);
    }
}
pub use crate::parquet::reader::{GroupFilterArg, ParquetReader};
pub use crate::parquet::writer::ParquetWriter;

pub trait DecodeFromChunk
where
    Self: Sized,
{
    fn decode(schema: &Schema, cols: Chunk<Box<dyn Array>>) -> Vec<Self>;
}

pub trait EncodeToChunk
where
    Self: Sized,
{
    /// Assert that metadata has the required keys
    /// ! Panics if a required key is missing
    fn assert_metadata(metadata: &BTreeMap<String, String>);
    /// Converts schema and metadata for consumption by the 'ParquetWriter'
    fn encodings(metadata: BTreeMap<String, String>) -> Vec<Vec<Encoding>>;
    /// Creates a schema using the given metadata for the given Struct
    /// ! Panics if metadata is not in the required shape
    fn encode_schema(metadata: BTreeMap<String, String>) -> Schema;
    /// this is the most generally type of an encoder
    /// it only needs an iterator of references
    /// it does not require ownership of the data nor that
    /// the data be collected in a container
    fn encode<'a, I>(data: I) -> Chunk<Box<dyn Array>>
    where
        I: Iterator<Item = &'a Self>,
        Self: 'a;
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////

/// Types that implement parquet reader writer traits should also have a
/// corresponding enum so that they can be passed across the ffi.
#[repr(C)]
pub enum ParquetType {
    QuoteTick = 0,
    TradeTick = 1,
}

#[repr(C)]
pub enum ParquetReaderType {
    File = 0,
    Buffer = 1,
}

/// # Safety
/// - Assumes `metadata` is borrowed from a valid Python `dict`.
#[no_mangle]
pub unsafe fn pydict_to_btree_map(py_metadata: *mut ffi::PyObject) -> BTreeMap<String, String> {
    assert!(!py_metadata.is_null(), "pointer was NULL");
    Python::with_gil(|py| {
        let py_metadata = PyDict::from_borrowed_ptr(py, py_metadata);
        py_metadata
            .extract()
            .expect("Unable to convert python metadata to rust btree")
    })
}

/// ParquetWriter is generic for any writer however for ffi it only supports
/// byte buffer writers. This is so that the byte buffer can be returned after
/// the writer is ended.
///
/// # Safety
/// - Assumes `file_path` is borrowed from a valid Python UTF-8 `str`.
/// - Assumes `metadata` is borrowed from a valid Python `dict`.
#[no_mangle]
pub unsafe extern "C" fn parquet_writer_new(
    parquet_type: ParquetType,
    metadata: *mut ffi::PyObject,
) -> *mut c_void {
    match parquet_type {
        ParquetType::QuoteTick => {
            let schema = QuoteTick::encode_schema(pydict_to_btree_map(metadata));
            let b = Box::new(ParquetWriter::<QuoteTick, Vec<u8>>::new_buffer_writer(
                schema,
            ));
            Box::into_raw(b) as *mut c_void
        }
        ParquetType::TradeTick => {
            let schema = TradeTick::encode_schema(pydict_to_btree_map(metadata));
            let b = Box::new(ParquetWriter::<TradeTick, Vec<u8>>::new_buffer_writer(
                schema,
            ));
            Box::into_raw(b) as *mut c_void
        }
    }
}

/// Writer is flushed, consumed and dropped. The underlying writer is returned.
/// While this is generic for ffi it only considers and returns a vector of bytes
/// if the underlying writer is anything else it will fail.
///
/// # Safety
/// - Assumes `writer` is a valid `*mut ParquetWriter<Struct>` where the struct
/// has a corresponding ParquetType enum.
#[no_mangle]
pub unsafe extern "C" fn parquet_writer_flush(
    writer: *mut c_void,
    parquet_type: ParquetType,
) -> CVec {
    let buffer = match parquet_type {
        ParquetType::QuoteTick => {
            let writer = Box::from_raw(writer as *mut ParquetWriter<QuoteTick, Vec<u8>>);
            writer.flush()
        }
        ParquetType::TradeTick => {
            let writer = Box::from_raw(writer as *mut ParquetWriter<TradeTick, Vec<u8>>);
            writer.flush()
        }
    };

    buffer.into()
}

#[no_mangle]
/// # Safety
/// - Assumes `writer` is a valid `*mut ParquetWriter<Struct>` where the struct
/// has a corresponding ParquetType enum.
/// - Assumes  `data` is a non-null valid pointer to a contiguous block of
/// C-style structs with `len` number of elements
pub unsafe extern "C" fn parquet_writer_write(
    writer: *mut c_void,
    parquet_type: ParquetType,
    data: *mut c_void,
    len: usize,
) {
    match parquet_type {
        ParquetType::QuoteTick => {
            let mut writer = Box::from_raw(writer as *mut ParquetWriter<QuoteTick, Vec<u8>>);
            let data: &[QuoteTick] = slice::from_raw_parts(data as *const QuoteTick, len);
            // TODO: handle errors better
            writer.write(data).expect("could not write data to file");
            // Leak writer value back otherwise it will be dropped after this function
            Box::into_raw(writer);
        }
        ParquetType::TradeTick => {
            let mut writer = Box::from_raw(writer as *mut ParquetWriter<TradeTick, Vec<u8>>);
            let data: &[TradeTick] = slice::from_raw_parts(data as *const TradeTick, len);
            // TODO: handle errors better
            writer.write(data).expect("Could not write data to file");
            // Leak writer value back otherwise it will be dropped after this function
            Box::into_raw(writer);
        }
    }
}

/// # Safety
/// - Assumes `metadata` is borrowed from a valid Python `dict`.
#[no_mangle]
pub unsafe fn pydict_to_btree_map(py_metadata: *mut ffi::PyObject) -> BTreeMap<String, String> {
    assert!(!py_metadata.is_null(), "pointer was NULL");
    Python::with_gil(|py| {
        let py_metadata = PyDict::from_borrowed_ptr(py, py_metadata);
        py_metadata
            .extract()
            .expect("unable to convert python metadata to rust btree")
    })
}

/// # Safety
/// - Assumes `file_path` is a valid `*mut ParquetReader<QuoteTick>`.
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_file_new(
    file_path: *mut ffi::PyObject,
    parquet_type: ParquetType,
    chunk_size: usize,
    // group_filter_arg: GroupFilterArg,  TODO: Comment out for now
) -> *mut c_void {
    let file_path = pystr_to_string(file_path);
    let file = File::open(&file_path)
        .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));
    match parquet_type {
        ParquetType::QuoteTick => {
            let b = Box::new(ParquetReader::<QuoteTick, File>::new(
                file,
                chunk_size,
                GroupFilterArg::None, // TODO: WIP
            ));
            Box::into_raw(b) as *mut c_void
        }
        ParquetType::TradeTick => {
            let b = Box::new(ParquetReader::<TradeTick, File>::new(
                file,
                chunk_size,
                GroupFilterArg::None, // TODO: WIP
            ));
            Box::into_raw(b) as *mut c_void
        }
    }
}

/// # Safety
/// - Assumes `data` is a valid CVec with an underlying byte buffer
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_buffer_new(
    data: CVec,
    parquet_type: ParquetType,
    chunk_size: usize,
    // group_filter_arg: GroupFilterArg,  TODO: Comment out for now
) -> *mut c_void {
    let CVec {
        ptr,
        len,
        cap: _cap,
    } = data;
    let buffer = slice::from_raw_parts(ptr as *const u8, len);
    let reader = Cursor::new(buffer);
    match parquet_type {
        ParquetType::QuoteTick => {
            let b = Box::new(ParquetReader::<QuoteTick, Cursor<&[u8]>>::new(
                reader,
                chunk_size,
                GroupFilterArg::None, // TODO: WIP
            ));
            Box::into_raw(b) as *mut c_void
        }
        ParquetType::TradeTick => {
            let b = Box::new(ParquetReader::<TradeTick, Cursor<&[u8]>>::new(
                reader,
                chunk_size,
                GroupFilterArg::None, // TODO: WIP
            ));
            Box::into_raw(b) as *mut c_void
        }
    }
}

/// # Safety
/// - Assumes `reader` is a valid `*mut ParquetReader<Struct>` where the struct
/// has a corresponding [ParquetType] enum.
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_free(
    reader: *mut c_void,
    parquet_type: ParquetType,
    reader_type: ParquetReaderType,
) {
    match (parquet_type, reader_type) {
        (ParquetType::QuoteTick, ParquetReaderType::File) => {
            let reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick, File>);
            drop(reader);
        }
        (ParquetType::TradeTick, ParquetReaderType::File) => {
            let reader = Box::from_raw(reader as *mut ParquetReader<TradeTick, File>);
            drop(reader);
        }
        (ParquetType::QuoteTick, ParquetReaderType::Buffer) => {
            let reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick, Cursor<&[u8]>>);
            drop(reader);
        }
        (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
            let reader = Box::from_raw(reader as *mut ParquetReader<TradeTick, Cursor<&[u8]>>);
            drop(reader);
        }
    }
}

/// # Safety
/// - Assumes `reader` is a valid `*mut ParquetReader<Struct>` where the struct
/// has a corresponding ParquetType enum.
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_next_chunk(
    reader: *mut c_void,
    parquet_type: ParquetType,
    reader_type: ParquetReaderType,
) -> CVec {
    match (parquet_type, reader_type) {
        (ParquetType::QuoteTick, ParquetReaderType::File) => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick, File>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::default, |data| data.into())
        }
        (ParquetType::TradeTick, ParquetReaderType::File) => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<TradeTick, File>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::default, |data| data.into())
        }
        (ParquetType::QuoteTick, ParquetReaderType::Buffer) => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick, Cursor<&[u8]>>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::empty, |data| data.into())
        }
        (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<TradeTick, Cursor<&[u8]>>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::empty, |data| data.into())
        }
    }
}

/// # Safety
/// - Assumes `chunk` is a valid `ptr` pointer to a contiguous array.
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_drop_chunk(chunk: CVec, parquet_type: ParquetType) {
    let CVec { ptr, len, cap } = chunk;
    match parquet_type {
        ParquetType::QuoteTick => {
            let data: Vec<QuoteTick> = Vec::from_raw_parts(ptr as *mut QuoteTick, len, cap);
            drop(data);
        }
        ParquetType::TradeTick => {
            let data: Vec<TradeTick> = Vec::from_raw_parts(ptr as *mut TradeTick, len, cap);
            drop(data);
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
#[allow(unused_variables)]
mod tests {
    use crate::parquet::{parquet_reader_file_new, ParquetType};
    use pyo3::types::PyString;
    use pyo3::{AsPyPointer, Python};

    #[test]
    #[allow(unused_assignments)]
    fn test_parquet_reader() {
        pyo3::prepare_freethreaded_python();

        let file_path = "../../tests/test_data/quote_tick_data.parquet";

        Python::with_gil(|py| {
            let file_path = PyString::new(py, file_path).as_ptr();
            let reader = unsafe { parquet_reader_file_new(file_path, ParquetType::QuoteTick, 0) };
        });
    }
}
