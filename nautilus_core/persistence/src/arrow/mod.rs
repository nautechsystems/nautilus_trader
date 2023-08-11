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

pub mod bar;
pub mod delta;
pub mod quote;
pub mod trade;

use std::{
    collections::HashMap,
    io::{self, Write},
};

use datafusion::arrow::{datatypes::Schema, ipc::writer::StreamWriter, record_batch::RecordBatch};
use nautilus_model::data::Data;
use pyo3::prelude::*;
use thiserror;

#[repr(C)]
#[pyclass]
#[derive(Debug, Clone, Copy)]
pub enum NautilusDataType {
    // Custom = 0,  # First slot reserved for custom data
    OrderBookDelta = 1,
    QuoteTick = 2,
    TradeTick = 3,
    Bar = 4,
}

#[derive(thiserror::Error, Debug)]
pub enum DataStreamingError {
    #[error("Arrow error: {0}")]
    ArrowError(#[from] datafusion::arrow::error::ArrowError),
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
    #[error("Python error: {0}")]
    PythonError(#[from] PyErr),
}

pub trait ArrowSchemaProvider {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema;
}

pub trait EncodeToRecordBatch
where
    Self: Sized + ArrowSchemaProvider,
{
    fn encode_batch(metadata: &HashMap<String, String>, data: &[Self]) -> RecordBatch;
}

pub trait DecodeFromRecordBatch
where
    Self: Sized + Into<Data> + ArrowSchemaProvider,
{
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Self>;
}

pub trait DecodeDataFromRecordBatch
where
    Self: Sized + Into<Data> + ArrowSchemaProvider,
{
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Vec<Data>;
}

pub trait WriteStream {
    fn write(&mut self, record_batch: &RecordBatch) -> Result<(), DataStreamingError>;
}

impl<T: EncodeToRecordBatch + Write> WriteStream for T {
    fn write(&mut self, record_batch: &RecordBatch) -> Result<(), DataStreamingError> {
        let mut writer = StreamWriter::try_new(self, &record_batch.schema())?;
        writer.write(record_batch)?;
        writer.finish()?;
        Ok(())
    }
}
