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
pub mod close;
pub mod delta;
pub mod depth;
pub mod index_price;
pub mod mark_price;
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
        Data, IndexPriceUpdate, MarkPriceUpdate, bar::Bar, close::InstrumentClose,
        delta::OrderBookDelta, depth::OrderBookDepth10, quote::QuoteTick, trade::TradeTick,
    },
    types::{price::PriceRaw, quantity::QuantityRaw},
};
#[cfg(feature = "python")]
use pyo3::prelude::*;

// Define metadata key constants constants
const KEY_BAR_TYPE: &str = "bar_type";
pub const KEY_INSTRUMENT_ID: &str = "instrument_id";
const KEY_PRICE_PRECISION: &str = "price_precision";
const KEY_SIZE_PRECISION: &str = "size_precision";

#[derive(thiserror::Error, Debug)]
pub enum DataStreamingError {
    #[error("I/O error: {0}")]
    IoError(#[from] io::Error),
    #[error("Arrow error: {0}")]
    ArrowError(#[from] arrow::error::ArrowError),
    #[cfg(feature = "python")]
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
    PriceRaw::from_le_bytes(
        bytes
            .try_into()
            .expect("Price raw bytes must be exactly the size of PriceRaw"),
    )
}

#[inline]
fn get_raw_quantity(bytes: &[u8]) -> QuantityRaw {
    QuantityRaw::from_le_bytes(
        bytes
            .try_into()
            .expect("Quantity raw bytes must be exactly the size of QuantityRaw"),
    )
}

/// Provides Apache Arrow schema definitions for data types.
pub trait ArrowSchemaProvider {
    /// Returns the Arrow schema for this type with optional metadata.
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema;

    /// Returns a map of field names to their Arrow data types.
    #[must_use]
    fn get_schema_map() -> HashMap<String, String> {
        let schema = Self::get_schema(None);
        let mut map = HashMap::new();
        for field in schema.fields() {
            let name = field.name().clone();
            let data_type = format!("{:?}", field.data_type());
            map.insert(name, data_type);
        }
        map
    }
}

/// Encodes data types to Apache Arrow RecordBatch format.
pub trait EncodeToRecordBatch
where
    Self: Sized + ArrowSchemaProvider,
{
    /// Encodes a batch of values into an Arrow `RecordBatch` using the provided metadata.
    ///
    /// # Errors
    ///
    /// Returns an `ArrowError` if the encoding fails.
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError>;

    /// Returns the metadata for this data element.
    fn metadata(&self) -> HashMap<String, String>;

    /// Returns the metadata for the first element in a chunk.
    ///
    /// # Panics
    ///
    /// Panics if `chunk` is empty.
    fn chunk_metadata(chunk: &[Self]) -> HashMap<String, String> {
        chunk
            .first()
            .map(|elem| elem.metadata())
            .expect("Chunk must have at least one element to encode")
    }
}

/// Decodes data types from Apache Arrow RecordBatch format.
pub trait DecodeFromRecordBatch
where
    Self: Sized + Into<Data> + ArrowSchemaProvider,
{
    /// Decodes a `RecordBatch` into a vector of values of the implementing type, using the provided metadata.
    ///
    /// # Errors
    ///
    /// Returns an `EncodingError` if the decoding fails.
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError>;
}

/// Decodes raw Data objects from Apache Arrow RecordBatch format.
pub trait DecodeDataFromRecordBatch
where
    Self: Sized + Into<Data> + ArrowSchemaProvider,
{
    /// Decodes a `RecordBatch` into raw `Data` values, using the provided metadata.
    ///
    /// # Errors
    ///
    /// Returns an `EncodingError` if the decoding fails.
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError>;
}

/// Writes RecordBatch data to output streams.
pub trait WriteStream {
    /// Writes a `RecordBatch` to the implementing output stream.
    ///
    /// # Errors
    ///
    /// Returns a `DataStreamingError` if writing or finishing the stream fails.
    fn write(&mut self, record_batch: &RecordBatch) -> Result<(), DataStreamingError>;
}

impl<T: Write> WriteStream for T {
    fn write(&mut self, record_batch: &RecordBatch) -> Result<(), DataStreamingError> {
        let mut writer = StreamWriter::try_new(self, &record_batch.schema())?;
        writer.write(record_batch)?;
        writer.finish()?;
        Ok(())
    }
}

/// Extracts and downcasts the specified `column_key` column from an Arrow array slice.
///
/// # Errors
///
/// Returns an error if:
/// - `column_index` is out of range: `EncodingError::MissingColumn`.
/// - The column type does not match `expected_type`: `EncodingError::InvalidColumnType`.
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

/// Converts a vector of `OrderBookDelta` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn book_deltas_to_arrow_record_batch_bytes(
    data: Vec<OrderBookDelta>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Extract metadata from chunk
    let metadata = OrderBookDelta::chunk_metadata(&data);
    OrderBookDelta::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

/// Converts a vector of `OrderBookDepth10` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
///
/// # Panics
///
/// Panics if `data` is empty (after the explicit empty check, unwrap is safe).
pub fn book_depth10_to_arrow_record_batch_bytes(
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

/// Converts a vector of `QuoteTick` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
///
/// # Panics
///
/// Panics if `data` is empty (after the explicit empty check, unwrap is safe).
pub fn quotes_to_arrow_record_batch_bytes(
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

/// Converts a vector of `TradeTick` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
///
/// # Panics
///
/// Panics if `data` is empty (after the explicit empty check, unwrap is safe).
pub fn trades_to_arrow_record_batch_bytes(
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

/// Converts a vector of `Bar` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
///
/// # Panics
///
/// Panics if `data` is empty (after the explicit empty check, unwrap is safe).
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

/// Converts a vector of `MarkPriceUpdate` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
///
/// # Panics
///
/// Panics if `data` is empty (after the explicit empty check, unwrap is safe).
pub fn mark_prices_to_arrow_record_batch_bytes(
    data: Vec<MarkPriceUpdate>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Take first element and extract metadata
    // SAFETY: Unwrap safe as already checked that `data` not empty
    let first = data.first().unwrap();
    let metadata = first.metadata();
    MarkPriceUpdate::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

/// Converts a vector of `IndexPriceUpdate` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
///
/// # Panics
///
/// Panics if `data` is empty (after the explicit empty check, unwrap is safe).
pub fn index_prices_to_arrow_record_batch_bytes(
    data: Vec<IndexPriceUpdate>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Take first element and extract metadata
    // SAFETY: Unwrap safe as already checked that `data` not empty
    let first = data.first().unwrap();
    let metadata = first.metadata();
    IndexPriceUpdate::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}

/// Converts a vector of `InstrumentClose` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
///
/// # Panics
///
/// Panics if `data` is empty (after the explicit empty check, unwrap is safe).
pub fn instrument_closes_to_arrow_record_batch_bytes(
    data: Vec<InstrumentClose>,
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    // Take first element and extract metadata
    // SAFETY: Unwrap safe as already checked that `data` not empty
    let first = data.first().unwrap();
    let metadata = first.metadata();
    InstrumentClose::encode_batch(&metadata, &data).map_err(EncodingError::ArrowError)
}
