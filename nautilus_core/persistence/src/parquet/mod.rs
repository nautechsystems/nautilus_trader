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

mod implementations;
mod reader;
mod writer;

use std::collections::BTreeMap;

use arrow2::{array::Array, chunk::Chunk, datatypes::Schema, io::parquet::write::Encoding};
use pyo3::prelude::*;

pub use crate::parquet::reader::{GroupFilterArg, ParquetReader};
pub use crate::parquet::writer::ParquetWriter;

#[repr(C)]
#[pyclass]
#[derive(Debug, Clone, Copy)]
pub enum ParquetType {
    QuoteTick = 0,
    TradeTick = 1,
}

#[repr(C)]
#[pyclass]
#[derive(Debug, Clone, Copy)]
pub enum ParquetReaderType {
    File = 0,
    Buffer = 1,
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
    /// Assert that metadata has the required keys
    /// ! Panics if a required key is missing.
    fn assert_metadata(metadata: &BTreeMap<String, String>);
    /// Converts schema and metadata for consumption by the `ParquetWriter`.
    fn encodings(metadata: BTreeMap<String, String>) -> Vec<Vec<Encoding>>;
    /// Creates a schema using the given metadata for the given Struct
    /// ! Panics if metadata is not in the required shape.
    fn encode_schema(metadata: BTreeMap<String, String>) -> Schema;
    /// This is the most general type of an encoder. It only needs an iterator
    /// of references it does not require ownership of the data, nor for
    /// the data to be collected in a container.
    fn encode<'a, I>(data: I) -> Chunk<Box<dyn Array>>
    where
        I: Iterator<Item = &'a Self>,
        Self: 'a;
}
