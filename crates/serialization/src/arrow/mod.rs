// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Defines the Apache Arrow schema for Nautilus types.

pub mod bar;
pub mod delta;
pub mod depth;
pub mod quote;
pub mod trade;

use std::{
    collections::HashMap,
    io::{self, Write},
};

use arrow::{
    array::{Array, ArrayRef},
    datatypes::{DataType, Schema},
    error::ArrowError,
    ipc::writer::StreamWriter,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        Data, bar::Bar, delta::OrderBookDelta, depth::OrderBookDepth10, quote::QuoteTick,
        trade::TradeTick,
    },
    types::{price::PriceRaw, quantity::QuantityRaw},
};
use pyo3::prelude::*;

// Define metadata key constants constants
const KEY_BAR_TYPE: &str = "bar_type";
pub const KEY_INSTRUMENT_ID: &str = "instrument_id";
const KEY_PRICE_PRECISION: &str = "price_precision";
const KEY_SIZE_PRECISION: &str = "size_precision";

#[derive(thiserror::Error, Debug)]
pub enum DataStreamingError {
    #[error("Arrow error: {0}")]
    ArrowError(#[from] arrow::error::ArrowError),
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
    #[error("Python error: {0}")]
    PythonError(#[from] PyErr),
}

#[derive(thiserror::Error, Debug)]
pub enum EncodingError {
    #[error("Empty data")]
    EmptyData,
    #[error("Missing metadata key: `{0}`")]
    MissingMetadata(&'static str),
    #[error("Missing data column: `{0}` at index {1}")]
    MissingColumn(&'static str, usize),
    #[error("Error parsing `{0}`: {1}")]
    ParseError(&'static str, String),
    #[error("Invalid column type `{0}` at index {1}: expected {2}, found {3}")]
    InvalidColumnType(&'static str, usize, DataType, DataType),
    #[error("Arrow error: {0}")]
    ArrowError(#[from] arrow::error::ArrowError),
}

#[inline]
fn get_raw_price(bytes: &[u8]) -> PriceRaw {
    PriceRaw::from_le_bytes(bytes.try_into().unwrap())
}

#[inline]
fn get_raw_quantity(bytes: &[u8]) -> QuantityRaw {
    QuantityRaw::from_le_bytes(bytes.try_into().unwrap())
}

pub trait ArrowSchemaProvider {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema;

    #[must_use]
    fn get_schema_map() -> HashMap<String, String> {
        let schema = Self::get_schema(None);
        let mut map = HashMap::new();
        for field in schema.fields() {
            let name = field.name().to_string();
            let data_type = format!("{:?}", field.data_type());
            map.insert(name, data_type);
        }
        map
    }
}

pub trait EncodeToRecordBatch
where
    Self: Sized + ArrowSchemaProvider,
{
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError>;

    fn metadata(&self) -> HashMap<String, String>;
    fn chunk_metadata(chunk: &[Self]) -> HashMap<String, String> {
        chunk
            .first()
            .map(|elem| elem.metadata())
            .expect("Chunk must have atleast one element to encode")
    }
}

pub trait DecodeFromRecordBatch
where
    Self: Sized + Into<Data> + ArrowSchemaProvider,
{
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError>;
}

pub trait DecodeDataFromRecordBatch
where
    Self: Sized + Into<Data> + ArrowSchemaProvider,
{
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError>;
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

pub fn extract_column<'a, T: Array + 'static>(
    cols: &'a [ArrayRef],
    column_key: &'static str,
    column_index: usize,
    expected_type: DataType,
) -> Result<&'a T, EncodingError> {
    let column_values = cols
        .get(column_index)
        .ok_or(EncodingError::MissingColumn(column_key, column_index))?;
    let downcasted_values =
        column_values
            .as_any()
            .downcast_ref::<T>()
            .ok_or(EncodingError::InvalidColumnType(
                column_key,
                column_index,
                expected_type,
                column_values.data_type().clone(),
            ))?;
    Ok(downcasted_values)
}

pub fn order_book_deltas_to_arrow_record_batch_bytes(
    data: Vec<OrderBookDelta>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Extract metadata from chunk
    let metadata = OrderBookDelta::chunk_metadata(&data);
    OrderBookDelta::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

pub fn order_book_depth10_to_arrow_record_batch_bytes(
    data: Vec<OrderBookDepth10>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Take first element and extract metadata
    // SAFETY: Unwrap safe as already checked that `data` not empty
    let first = data.first().unwrap();
    let metadata = first.metadata();
    OrderBookDepth10::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

pub fn quote_ticks_to_arrow_record_batch_bytes(
    data: Vec<QuoteTick>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Take first element and extract metadata
    // SAFETY: Unwrap safe as already checked that `data` not empty
    let first = data.first().unwrap();
    let metadata = first.metadata();
    QuoteTick::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

pub fn trade_ticks_to_arrow_record_batch_bytes(
    data: Vec<TradeTick>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Take first element and extract metadata
    // SAFETY: Unwrap safe as already checked that `data` not empty
    let first = data.first().unwrap();
    let metadata = first.metadata();
    TradeTick::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

pub fn bars_to_arrow_record_batch_bytes(data: Vec<Bar>) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Take first element and extract metadata
    // SAFETY: Unwrap safe as already checked that `data` not empty
    let first = data.first().unwrap();
    let metadata = first.metadata();
    Bar::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}
