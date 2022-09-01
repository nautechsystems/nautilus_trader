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

mod quote_tick;

use std::{ffi::c_void, fs::File, marker::PhantomData};

use arrow2::{
    array::Array,
    chunk::Chunk,
    datatypes::Schema,
    error::Result,
    io::parquet::{
        read::FileReader,
        write::{
            CompressionOptions, Encoding, FileWriter, RowGroupIterator, Version, WriteOptions,
        },
    },
};
use pyo3::{AsPyPointer, PyObject};

use nautilus_core::{cvec::CVec, string::pystr_to_string};
use nautilus_model::data::tick::QuoteTick;

pub struct ParquetReader<A> {
    file_reader: FileReader<File>,
    reader_type: PhantomData<*const A>,
}

impl<A> ParquetReader<A> {
    pub fn new(file_path: &str, chunk_size: usize) -> Self {
        let file = File::open(file_path)
            .unwrap_or_else(|_| panic!("Unable to open parquet file {file_path}"));
        let fr = FileReader::try_new(file, None, Some(chunk_size), None, None)
            .expect("Unable to create reader from file");
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

impl<A> ParquetWriter<A>
where
    A: EncodeToChunk,
{
    pub fn new(path: &str, schema: Schema) -> Self {
        let options = WriteOptions {
            write_statistics: true,
            compression: CompressionOptions::Uncompressed,
            version: Version::V2,
        };

        let encodings = A::encodings();

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
        let chunk_stream = data_stream.map(|chunk| Ok(A::encode(chunk)));
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

    pub fn write(&mut self, data: Vec<A>) -> Result<()> {
        let cols = A::encode(data);
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
    fn encodings() -> Vec<Vec<Encoding>>;
    fn encode_schema() -> Schema;
    fn encode(data: Vec<Self>) -> Chunk<Box<dyn Array>>;
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[repr(C)]
pub enum ParquetReaderType {
    QuoteTick,
}

/// # Safety
/// Assumes `file_path` is a valid `*mut ParquetReader<QuoteTick>`.
pub unsafe extern "C" fn parquet_reader_new(
    file_path: PyObject,
    reader_type: ParquetReaderType,
) -> *mut c_void {
    let file_path = pystr_to_string(file_path.as_ptr());
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
/// Assumes `reader` is a valid `*mut ParquetReader<QuoteTick>`.
pub unsafe extern "C" fn parquet_reader_next_chunk(
    reader: *mut c_void,
    reader_type: ParquetReaderType,
) -> CVec {
    match reader_type {
        ParquetReaderType::QuoteTick => {
            let mut reader = Box::from_raw(reader as *mut ParquetReader<QuoteTick>);
            let chunk = reader.next();
            // leak reader value back otherwise it will be dropped after this function
            Box::into_raw(reader);
            chunk.map_or_else(CVec::default, |data| data.into())
        }
    }
}

/// # Safety
/// Assumes `chunk` is a valid `ptr` pointer to a contiguous array of u64.
pub unsafe extern "C" fn parquet_reader_drop_chunk(chunk: CVec, reader_type: ParquetReaderType) {
    let CVec { ptr, len, cap } = chunk;
    match reader_type {
        ParquetReaderType::QuoteTick => {
            let data: Vec<u64> = Vec::from_raw_parts(ptr as *mut u64, len, cap);
            drop(data);
        }
    }
}
