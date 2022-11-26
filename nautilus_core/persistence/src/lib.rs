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

pub mod parquet;

use std::{collections::BTreeMap, ffi::c_void, fs::File, io::Cursor, slice};

use nautilus_core::cvec::CVec;
use nautilus_model::data::tick::{QuoteTick, TradeTick};
use parquet::{
    EncodeToChunk, GroupFilterArg, ParquetReader, ParquetReaderType, ParquetType, ParquetWriter,
};
use pyo3::prelude::*;

#[pyclass(name = "ParquetReader")]
struct PythonParquetReader {
    reader: *mut c_void,
    parquet_type: ParquetType,
    reader_type: ParquetReaderType,
}

/// Empty derivation for Send to satisfy `pyclass` requirements
/// however this is only designed for single threaded use for now
unsafe impl Send for PythonParquetReader {}

#[pymethods]
impl PythonParquetReader {
    #[new]
    fn new(
        file_path: String,
        buffer: Vec<u8>,
        chunk_size: usize,
        parquet_type: ParquetType,
        reader_type: ParquetReaderType,
    ) -> Self {
        let reader = match (parquet_type, reader_type) {
            (ParquetType::QuoteTick, ParquetReaderType::File) => {
                let file = File::open(&file_path)
                    .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));
                let reader =
                    ParquetReader::<QuoteTick, File>::new(file, chunk_size, GroupFilterArg::None);
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
            (ParquetType::TradeTick, ParquetReaderType::File) => {
                let file = File::open(&file_path)
                    .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));
                let reader =
                    ParquetReader::<TradeTick, File>::new(file, chunk_size, GroupFilterArg::None);
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
            (ParquetType::QuoteTick, ParquetReaderType::Buffer) => {
                let cursor = Cursor::new(buffer);
                let reader = ParquetReader::<QuoteTick, Cursor<Vec<u8>>>::new(
                    cursor,
                    chunk_size,
                    GroupFilterArg::None,
                );
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
            (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
                let cursor = Cursor::new(buffer);
                let reader = ParquetReader::<TradeTick, Cursor<Vec<u8>>>::new(
                    cursor,
                    chunk_size,
                    GroupFilterArg::None,
                );
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
        };

        PythonParquetReader {
            reader,
            parquet_type,
            reader_type,
        }
    }

    /// the reader implements an iterator
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// each iteration returns a chunk of values read from the parquet file
    unsafe fn __next__(slf: PyRef<'_, Self>) -> Option<PyObject> {
        let chunk: Option<CVec> = match (slf.parquet_type, slf.reader_type) {
            (ParquetType::QuoteTick, ParquetReaderType::File) => {
                let mut reader = Box::from_raw(slf.reader as *mut ParquetReader<QuoteTick, File>);
                let chunk = reader.next();
                // Leak reader value back otherwise it will be dropped after this function
                Box::into_raw(reader);
                chunk.map_or_else(|| None, |data| Some(data.into()))
            }
            (ParquetType::TradeTick, ParquetReaderType::File) => {
                let mut reader = Box::from_raw(slf.reader as *mut ParquetReader<TradeTick, File>);
                let chunk = reader.next();
                // Leak reader value back otherwise it will be dropped after this function
                Box::into_raw(reader);
                chunk.map_or_else(|| None, |data| Some(data.into()))
            }
            (ParquetType::QuoteTick, ParquetReaderType::Buffer) => {
                let mut reader =
                    Box::from_raw(slf.reader as *mut ParquetReader<QuoteTick, Cursor<&[u8]>>);
                let chunk = reader.next();
                // Leak reader value back otherwise it will be dropped after this function
                Box::into_raw(reader);
                chunk.map_or_else(|| None, |data| Some(data.into()))
            }
            (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
                let mut reader =
                    Box::from_raw(slf.reader as *mut ParquetReader<TradeTick, Cursor<&[u8]>>);
                let chunk = reader.next();
                // Leak reader value back otherwise it will be dropped after this function
                Box::into_raw(reader);
                chunk.map_or_else(|| None, |data| Some(data.into()))
            }
        };

        match chunk {
            Some(cvec) => Python::with_gil(|py| Some(cvec.into_py(py))),
            None => None,
        }
    }

    /// After reading is complete the reader must be dropped, otherwise it will
    /// leak memory and resources
    unsafe fn drop(slf: PyRef<'_, Self>) {
        match (slf.parquet_type, slf.reader_type) {
            (ParquetType::QuoteTick, ParquetReaderType::File) => {
                let reader = Box::from_raw(slf.reader as *mut ParquetReader<QuoteTick, File>);
                drop(reader);
            }
            (ParquetType::TradeTick, ParquetReaderType::File) => {
                let reader = Box::from_raw(slf.reader as *mut ParquetReader<TradeTick, File>);
                drop(reader);
            }
            (ParquetType::QuoteTick, ParquetReaderType::Buffer) => {
                let reader =
                    Box::from_raw(slf.reader as *mut ParquetReader<QuoteTick, Cursor<Vec<u8>>>);
                drop(reader);
            }
            (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
                let reader =
                    Box::from_raw(slf.reader as *mut ParquetReader<TradeTick, Cursor<Vec<u8>>>);
                drop(reader);
            }
        }
    }

    /// Chunks generated by iteration must be dropped after use, otherwise
    /// it will leak memory
    fn drop_chunk(slf: PyRef<'_, Self>, chunk: CVec) {
        let CVec { ptr, len, cap } = chunk;
        match slf.parquet_type {
            ParquetType::QuoteTick => {
                let data: Vec<QuoteTick> =
                    unsafe { Vec::from_raw_parts(ptr as *mut QuoteTick, len, cap) };
                drop(data);
            }
            ParquetType::TradeTick => {
                let data: Vec<TradeTick> =
                    unsafe { Vec::from_raw_parts(ptr as *mut TradeTick, len, cap) };
                drop(data);
            }
        }
    }
}

#[pyclass(name = "ParquetWriter")]
struct PythonParquetWriter {
    writer: *mut c_void,
    parquet_type: ParquetType,
}

/// Empty derivation for Send to satisfy `pyclass` requirements
/// however this is only designed for single threaded use for now
unsafe impl Send for PythonParquetWriter {}

#[pymethods]
impl PythonParquetWriter {
    #[new]
    fn new(parquet_type: ParquetType, metadata: BTreeMap<String, String>) -> Self {
        let writer = match parquet_type {
            ParquetType::QuoteTick => {
                let schema = QuoteTick::encode_schema(metadata);
                let b = Box::new(ParquetWriter::<QuoteTick, Vec<u8>>::new_buffer_writer(
                    schema,
                ));
                Box::into_raw(b) as *mut c_void
            }
            ParquetType::TradeTick => {
                let schema = TradeTick::encode_schema(metadata);
                let b = Box::new(ParquetWriter::<TradeTick, Vec<u8>>::new_buffer_writer(
                    schema,
                ));
                Box::into_raw(b) as *mut c_void
            }
        };

        PythonParquetWriter {
            writer,
            parquet_type,
        }
    }

    /// - Assumes  `data` is a non-null valid pointer to a contiguous block of
    /// C-style structs with `len` number of elements.
    /// 
    /// # Safety: Here CVec is just used to transfer data to the rust side
    /// it is expected that the data is allocated in the cython side and
    /// NOT on the rust side. So this CVec does not need to be dropped.
    unsafe fn parquet_writer_write(slf: PyRef<'_, Self>, data: CVec) {
        let CVec {
            ptr: data,
            len,
            cap: _,
        } = data;
        match slf.parquet_type {
            ParquetType::QuoteTick => {
                let mut writer =
                    Box::from_raw(slf.writer as *mut ParquetWriter<QuoteTick, Vec<u8>>);
                let data: &[QuoteTick] = slice::from_raw_parts(data as *const QuoteTick, len);
                // TODO: handle errors better
                writer.write(data).expect("Could not write data to file");
                // Leak writer value back otherwise it will be dropped after this function
                Box::into_raw(writer);
            }
            ParquetType::TradeTick => {
                let mut writer =
                    Box::from_raw(slf.writer as *mut ParquetWriter<TradeTick, Vec<u8>>);
                let data: &[TradeTick] = slice::from_raw_parts(data as *const TradeTick, len);
                // TODO: handle errors better
                writer.write(data).expect("Could not write data to file");
                // Leak writer value back otherwise it will be dropped after this function
                Box::into_raw(writer);
            }
        }
    }

    /// Writer is flushed, consumed and dropped. The underlying writer is returned.
    /// While this is generic for ffi it only considers and returns a vector of bytes
    /// if the underlying writer is anything else it will fail.
    unsafe fn flush(slf: PyRef<'_, Self>) -> CVec {
        let buffer = match slf.parquet_type {
            ParquetType::QuoteTick => {
                let writer = Box::from_raw(slf.writer as *mut ParquetWriter<QuoteTick, Vec<u8>>);
                writer.flush()
            }
            ParquetType::TradeTick => {
                let writer = Box::from_raw(slf.writer as *mut ParquetWriter<TradeTick, Vec<u8>>);
                writer.flush()
            }
        };

        buffer.into()
    }
}

#[pymodule]
fn nautilus_persistence(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<PythonParquetReader>()?;
    m.add_class::<PythonParquetWriter>()?;
    m.add_class::<ParquetType>()?;
    m.add_class::<ParquetReaderType>()?;
    m.add_class::<CVec>()?;
    Ok(())
}
