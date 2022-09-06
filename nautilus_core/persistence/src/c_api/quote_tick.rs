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

use std::collections::BTreeMap;
use std::ffi::c_void;

use pyo3::ffi;
use pyo3::types::PyDict;
use pyo3::FromPyPointer;
use pyo3::Python;

use crate::parquet::ParquetReader;
use crate::parquet::{EncodeToChunk, ParquetWriter};
use nautilus_core::{cvec::CVec, string::pystr_to_string};
use nautilus_model::data::tick::QuoteTick;

#[repr(C)]
pub enum ParquetReaderType {
    QuoteTick = 0,
}

#[repr(C)]
pub enum ParquetWriterType {
    QuoteTick = 0,
}

/// # Safety
/// - Assumes `metadata` is borrowed from a valid Python `dict`.
pub unsafe fn pydict_to_btree_map(metadata: *mut ffi::PyObject) -> BTreeMap<String, String> {
    Python::with_gil(|py| {
        let _ = PyDict::from_borrowed_ptr(py, metadata);
        // TODO: Need to populate this metadata map
        BTreeMap::new()
    })
}

/// # Safety
/// - Assumes `file_path` is borrowed from a valid Python UTF-8 `str`.
pub unsafe extern "C" fn parquet_writer_new(
    file_path: *mut ffi::PyObject,
    writer_type: ParquetWriterType,
    metadata: *mut ffi::PyObject,
) -> *mut c_void {
    let file_path = pystr_to_string(file_path);
    let schema = QuoteTick::encode_schema(pydict_to_btree_map(metadata));
    match writer_type {
        ParquetWriterType::QuoteTick => {
            let b = Box::new(ParquetWriter::<QuoteTick>::new(&file_path, schema));
            Box::into_raw(b) as *mut c_void
        }
    }
}

/// # Safety
/// - Assumes `file_path` is borrowed from a valid Python UTF-8 `str`.
pub unsafe extern "C" fn parquet_reader_new(
    file_path: *mut ffi::PyObject,
    reader_type: ParquetReaderType,
) -> *mut c_void {
    let file_path = pystr_to_string(file_path);
    match reader_type {
        ParquetReaderType::QuoteTick => {
            let b = Box::new(ParquetReader::<QuoteTick>::new(&file_path, 1000));
            Box::into_raw(b) as *mut c_void
        }
    }
}

/// # Safety
/// Assumes `reader` is a valid `*mut ParquetReader<QuoteTick>`.
pub unsafe extern "C" fn parquet_reader_drop(reader: *mut c_void, reader_type: ParquetReaderType) {
    match reader_type {
        ParquetReaderType::QuoteTick => {
            let reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick>);
            drop(reader);
        }
    }
}

/// # Safety
/// - Assumes `reader` is a valid `*mut ParquetReader<QuoteTick>`.
pub unsafe extern "C" fn parquet_reader_next_chunk(
    reader: *mut c_void,
    reader_type: ParquetReaderType,
) -> CVec {
    match reader_type {
        ParquetReaderType::QuoteTick => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick>);
            let chunk = reader.next();
            // Leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::default, |data| data.into())
        }
    }
}

/// # Safety
/// - Assumes `chunk` is a valid `ptr` pointer to a contiguous array of `u64`.
pub unsafe extern "C" fn parquet_reader_drop_chunk(chunk: CVec, reader_type: ParquetReaderType) {
    let CVec { ptr, len, cap } = chunk;
    match reader_type {
        ParquetReaderType::QuoteTick => {
            let data: Vec<u64> = Vec::from_raw_parts(ptr as *mut u64, len, cap);
            drop(data);
        }
    }
}
