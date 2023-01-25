// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{collections::BTreeMap, ffi::c_void, fs::File, io::Cursor, ptr::null_mut, slice};

use nautilus_core::cvec::CVec;
use nautilus_model::data::tick::{QuoteTick, TradeTick};
use parquet::{
    EncodeToChunk, GroupFilterArg, ParquetReader, ParquetReaderType, ParquetType, ParquetWriter,
};
use pyo3::types::PyBytes;
use pyo3::{prelude::*, types::PyCapsule};

#[pyclass(name = "ParquetReader")]
struct PythonParquetReader {
    reader: *mut c_void,
    parquet_type: ParquetType,
    reader_type: ParquetReaderType,
    current_chunk: Option<CVec>,
}

/// pyo3 automatically calls drop on the underlying rust struct when
/// the python object is deallocated.
///
/// so answer: https://stackoverflow.com/q/66401814
impl Drop for PythonParquetReader {
    fn drop(&mut self) {
        self.drop_chunk();
        match (self.parquet_type, self.reader_type) {
            (ParquetType::QuoteTick, ParquetReaderType::File) => {
                let reader =
                    unsafe { Box::from_raw(self.reader as *mut ParquetReader<QuoteTick, File>) };
                drop(reader);
            }
            (ParquetType::TradeTick, ParquetReaderType::File) => {
                let reader =
                    unsafe { Box::from_raw(self.reader as *mut ParquetReader<TradeTick, File>) };
                drop(reader);
            }
            (ParquetType::QuoteTick, ParquetReaderType::Buffer) => {
                let reader = unsafe {
                    Box::from_raw(self.reader as *mut ParquetReader<QuoteTick, Cursor<&[u8]>>)
                };
                drop(reader);
            }
            (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
                let reader = unsafe {
                    Box::from_raw(self.reader as *mut ParquetReader<TradeTick, Cursor<&[u8]>>)
                };
                drop(reader);
            }
        }

        self.reader = null_mut();
    }
}

/// Empty derivation for Send to satisfy `pyclass` requirements,
/// however this is only designed for single threaded use for now.
unsafe impl Send for PythonParquetReader {}

#[pymethods]
impl PythonParquetReader {
    #[new]
    #[pyo3(signature = (file_path, chunk_size, parquet_type, reader_type, buffer=None, ts_init_filter=0))]
    fn new(
        file_path: String,
        chunk_size: usize,
        parquet_type: ParquetType,
        reader_type: ParquetReaderType,
        buffer: Option<&[u8]>,
        ts_init_filter: i64,
    ) -> Self {
        let group_filter: GroupFilterArg = ts_init_filter.into();
        let reader = match (parquet_type, reader_type) {
            (ParquetType::QuoteTick, ParquetReaderType::File) => {
                let file = File::open(&file_path)
                    .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));
                let reader = ParquetReader::<QuoteTick, File>::new(file, chunk_size, group_filter);
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
            (ParquetType::TradeTick, ParquetReaderType::File) => {
                let file = File::open(&file_path)
                    .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));
                let reader = ParquetReader::<TradeTick, File>::new(file, chunk_size, group_filter);
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
            (ParquetType::QuoteTick, ParquetReaderType::Buffer) => {
                let cursor = Cursor::new(buffer.expect("Buffer reader needs a byte buffer"));
                let reader = ParquetReader::<QuoteTick, Cursor<&[u8]>>::new(
                    cursor,
                    chunk_size,
                    group_filter,
                );
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
            (ParquetType::TradeTick, ParquetReaderType::Buffer) => {
                let cursor = Cursor::new(buffer.expect("Buffer reader needs a byte buffer"));
                let reader = ParquetReader::<TradeTick, Cursor<&[u8]>>::new(
                    cursor,
                    chunk_size,
                    group_filter,
                );
                let reader = Box::new(reader);
                Box::into_raw(reader) as *mut c_void
            }
        };

        PythonParquetReader {
            reader,
            parquet_type,
            reader_type,
            current_chunk: None,
        }
    }

    /// The reader implements an iterator.
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Each iteration returns a chunk of values read from the parquet file.
    unsafe fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        slf.drop_chunk();

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

        slf.current_chunk = chunk;
        match chunk {
            Some(cvec) => Python::with_gil(|py| {
                Some(PyCapsule::new::<CVec>(py, cvec, None).unwrap().into_py(py))
            }),
            None => None,
        }
    }

    /// After reading is complete the reader must be dropped, otherwise it will
    /// leak memory and resources. Also drop the current chunk if it exists.
    ///
    /// # Safety: Do not use the reader after it's been dropped
    ///
    /// May not be necessary as Drop is automatically called on the rust struct
    /// when the object is deallocated on the python side.
    unsafe fn drop(slf: PyRefMut<'_, Self>) {
        drop(slf)
    }
}

impl PythonParquetReader {
    /// Chunks generated by iteration must be dropped after use, otherwise
    /// it will leak memory. Current chunk is held by the reader,
    /// drop if exists and reset the field.
    fn drop_chunk(&mut self) {
        if let Some(CVec { ptr, len, cap }) = self.current_chunk {
            match self.parquet_type {
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

            // reset current chunk field
            self.current_chunk = None;
        };
    }
}

#[pyclass(name = "ParquetWriter")]
struct PythonParquetWriter {
    writer: *mut c_void,
    parquet_type: ParquetType,
}

/// Empty derivation for Send to satisfy `pyclass` requirements
/// however this is only designed for single threaded use for now.
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

    /// # Safety
    /// Assumes  `data` is a `PyCapsule` that stores a CVec with a non-null
    /// pointer to a contiguous buffer of C-style structs with `len`
    /// number of elements.
    unsafe fn write(slf: PyRef<'_, Self>, data: &PyCapsule) {
        let CVec { ptr, len, cap: _ } = *(PyCapsule::pointer(data) as *const CVec);
        match slf.parquet_type {
            ParquetType::QuoteTick => {
                let mut writer =
                    Box::from_raw(slf.writer as *mut ParquetWriter<QuoteTick, Vec<u8>>);
                let data: &[QuoteTick] = slice::from_raw_parts(ptr as *const QuoteTick, len);
                // TODO: handle errors better
                writer.write(data).expect("Could not write data");
                // Leak writer value back otherwise it will be dropped after this function
                Box::into_raw(writer);
            }
            ParquetType::TradeTick => {
                let mut writer =
                    Box::from_raw(slf.writer as *mut ParquetWriter<TradeTick, Vec<u8>>);
                let data: &[TradeTick] = slice::from_raw_parts(ptr as *const TradeTick, len);
                // TODO: handle errors better
                writer.write(data).expect("Could not write data");
                // Leak writer value back otherwise it will be dropped after this function
                Box::into_raw(writer);
            }
        }
    }

    /// Writer is flushed, consumed and dropped. The underlying writer is returned.
    /// While this is generic for FFI it only considers and returns a vector of bytes
    /// if the underlying writer is anything else it will fail.
    ///
    /// The vector of bytes is converted to PyBytes which is the bytes type
    /// in python.
    ///
    /// # Safety Do not use writer after flushing it
    unsafe fn flush_bytes(mut slf: PyRefMut<'_, Self>) -> PyObject {
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

        slf.writer = null_mut(); // Release memory
        Python::with_gil(|py| PyBytes::new(py, &buffer).into_py(py))
    }
}

/// Loaded as nautilus_pyo3.persistence
#[pymodule]
pub fn persistence(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<PythonParquetReader>()?;
    m.add_class::<PythonParquetWriter>()?;
    m.add_class::<ParquetType>()?;
    m.add_class::<ParquetReaderType>()?;
    Ok(())
}
