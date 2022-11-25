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

/// # Safety
/// - Assumes `writer` is a valid `*mut ParquetWriter<Struct>` where the struct
/// has a corresponding [ParquetType] enum.
#[no_mangle]
pub unsafe extern "C" fn parquet_writer_free(writer: *mut c_void, parquet_type: ParquetType) {
    match parquet_type {
        ParquetType::QuoteTick => {
            let writer = Box::from_raw(writer as *mut ParquetWriter<QuoteTick, Vec<u8>>);
            drop(writer);
        }
        ParquetType::TradeTick => {
            let writer = Box::from_raw(writer as *mut ParquetWriter<TradeTick, Vec<u8>>);
            drop(writer);
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
            writer.write(data).expect("Could not write data to file");
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
            chunk.map_or_else(CVec::default, |data| data.into())
        }
        (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<TradeTick, Cursor<&[u8]>>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::default, |data| data.into())
        }
    }
}

/// TODO: Is this needed?
///
/// # Safety
/// - Assumes `chunk` is a valid `ptr` pointer to a contiguous array.
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_index_chunk(
    chunk: CVec,
    reader_type: ParquetType,
    index: usize,
) -> *mut c_void {
    match reader_type {
        ParquetType::QuoteTick => (chunk.ptr as *mut QuoteTick).add(index) as *mut c_void,
        ParquetType::TradeTick => (chunk.ptr as *mut TradeTick).add(index) as *mut c_void,
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

        let file_path = "../../tests/test_kit/data/quote_tick_data.parquet";

        Python::with_gil(|py| {
            let file_path = PyString::new(py, file_path).as_ptr();
            let reader = unsafe { parquet_reader_file_new(file_path, ParquetType::QuoteTick, 0) };
        });
    }
}
