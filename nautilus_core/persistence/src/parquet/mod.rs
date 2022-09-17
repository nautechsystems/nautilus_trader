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
pub enum ParquetWriterType {
    File = 0,
    Buffer = 1,
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
    let schema = QuoteTick::encode_schema(pydict_to_btree_map(metadata));
    match parquet_type {
        ParquetType::QuoteTick => {
            let b = Box::new(ParquetWriter::<QuoteTick, Vec<u8>>::new_buffer_writer(
                schema,
            ));
            Box::into_raw(b) as *mut c_void
        }
        ParquetType::TradeTick => {
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
pub unsafe extern "C" fn parquet_writer_drop(
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

/// # Safety
/// - Assumes `file_path` is a valid `*mut ParquetReader<QuoteTick>`.
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_new(
    file_path: *mut ffi::PyObject,
    reader_type: ParquetType,
    chunk_size: usize,
    // group_filter_arg: GroupFilterArg,  TODO: Comment out for now
) -> *mut c_void {
    let file_path = pystr_to_string(file_path);
    match reader_type {
        ParquetType::QuoteTick => {
            let b = Box::new(ParquetReader::<QuoteTick>::new(
                &file_path,
                chunk_size,
                GroupFilterArg::None, // TODO: WIP
            ));
            Box::into_raw(b) as *mut c_void
        }
        ParquetType::TradeTick => {
            let b = Box::new(ParquetReader::<TradeTick>::new(
                &file_path,
                chunk_size,
                GroupFilterArg::None, // TODO: WIP
            ));
            Box::into_raw(b) as *mut c_void
        }
    }
}

/// # Safety
/// - Assumes `reader` is a valid `*mut ParquetReader<Struct>` where the struct
/// has a corresponding ParquetType enum.
#[no_mangle]
pub unsafe extern "C" fn parquet_reader_drop(reader: *mut c_void, reader_type: ParquetType) {
    match reader_type {
        ParquetType::QuoteTick => {
            let reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick>);
            drop(reader);
        }
        ParquetType::TradeTick => {
            let reader = Box::from_raw(reader as *mut ParquetReader<TradeTick>);
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
    reader_type: ParquetType,
) -> CVec {
    match reader_type {
        ParquetType::QuoteTick => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::default, |data| data.into())
        }
        ParquetType::TradeTick => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<TradeTick>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::default, |data| data.into())
        }
    }
}

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
pub unsafe extern "C" fn parquet_reader_drop_chunk(chunk: CVec, reader_type: ParquetType) {
    let CVec { ptr, len, cap } = chunk;
    match reader_type {
        ParquetType::QuoteTick => {
            let data: Vec<u64> = Vec::from_raw_parts(ptr as *mut u64, len, cap);
            drop(data);
        }
        ParquetType::TradeTick => {
            let data: Vec<u64> = Vec::from_raw_parts(ptr as *mut u64, len, cap);
            drop(data);
        }
    }
}
