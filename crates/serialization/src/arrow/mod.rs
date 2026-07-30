// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

pub mod account_state;
pub mod bar;
pub mod close;
pub mod custom;
pub mod delta;
pub mod depth;
pub mod funding;
pub mod index_price;
pub mod instrument;
pub mod instrument_status;
pub mod json;
pub mod mark_price;
pub mod option_greeks;
pub mod order_event;
pub mod position_event;
pub mod quote;
pub mod report;
pub mod snapshot;
pub mod trade;

#[cfg(feature = "display")]
pub mod display;

use std::{
    collections::HashMap,
    io::{self, Write},
};

use arrow::{
    array::{Array, ArrayRef, FixedSizeBinaryArray, StringArray, StringViewArray},
    datatypes::{DataType, Schema},
    error::ArrowError,
    ipc::writer::StreamWriter,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        Data, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate, bar::Bar,
        close::InstrumentClose, delta::OrderBookDelta, depth::OrderBookDepth10,
        option_chain::OptionGreeks, quote::QuoteTick, trade::TradeTick,
    },
    enums::BookAction,
    types::{
        PRICE_ERROR, PRICE_UNDEF, Price, QUANTITY_UNDEF, Quantity,
        fixed::{PRECISION_BYTES, correct_price_raw, correct_quantity_raw},
        price::PriceRaw,
        quantity::QuantityRaw,
    },
};
#[cfg(feature = "python")]
use pyo3::prelude::*;
use ustr::Ustr;

// Define metadata key constants constants
const KEY_BAR_TYPE: &str = "bar_type";
pub const KEY_INSTRUMENT_ID: &str = "instrument_id";
pub const KEY_PRICE_PRECISION: &str = "price_precision";
pub const KEY_SIZE_PRECISION: &str = "size_precision";

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
    #[error(
        "Mixed metadata at row {index}; encode each instrument, bar type, or precision separately"
    )]
    MixedMetadata { index: usize },
    #[error("Missing metadata key: `{0}`")]
    MissingMetadata(&'static str),
    #[error("Missing data column: `{0}` at index {1}")]
    MissingColumn(&'static str, usize),
    #[error("Error parsing `{0}`: {1}")]
    ParseError(&'static str, String),
    #[error("Invalid column type `{0}` at index {1}: expected {2}, found {3}")]
    InvalidColumnType(&'static str, usize, DataType, DataType),
    #[error(
        "Precision mode mismatch for `{field}`: catalog data has {actual_bytes} byte values, \
         but this build expects {expected_bytes} bytes. The catalog was created with a different \
         precision mode (standard=8 bytes, high=16 bytes). Rebuild the catalog or change your \
         build's precision mode. See: https://nautilustrader.io/docs/latest/getting_started/installation#precision-mode"
    )]
    PrecisionMismatch {
        field: &'static str,
        expected_bytes: i32,
        actual_bytes: i32,
    },
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

/// Gets raw price bytes and corrects for floating-point precision errors in stored data.
///
/// Data from catalogs may have been created with `int(value * FIXED_SCALAR)` which
/// introduces floating-point errors. This corrects the raw value to the nearest valid
/// multiple of the scale factor for the given precision.
///
/// Sentinel values (`PRICE_UNDEF`, `PRICE_ERROR`) are preserved unchanged.
#[inline]
fn get_corrected_raw_price(bytes: &[u8], precision: u8) -> PriceRaw {
    let raw = get_raw_price(bytes);

    // Preserve sentinel values unchanged
    if raw == PRICE_UNDEF || raw == PRICE_ERROR {
        return raw;
    }

    correct_price_raw(raw, precision)
}

/// Gets raw quantity bytes and corrects for floating-point precision errors in stored data.
///
/// Data from catalogs may have been created with `int(value * FIXED_SCALAR)` which
/// introduces floating-point errors. This corrects the raw value to the nearest valid
/// multiple of the scale factor for the given precision.
///
/// Sentinel values (`QUANTITY_UNDEF`) are preserved unchanged.
#[inline]
fn get_corrected_raw_quantity(bytes: &[u8], precision: u8) -> QuantityRaw {
    let raw = get_raw_quantity(bytes);

    // Preserve sentinel values unchanged
    if raw == QUANTITY_UNDEF {
        return raw;
    }

    correct_quantity_raw(raw, precision)
}

/// Decodes a [`Price`] from raw bytes with bounds validation.
///
/// Uses corrected raw values to handle floating-point precision errors in stored data.
/// Sentinel values (`PRICE_UNDEF`, `PRICE_ERROR`) are preserved unchanged.
///
/// # Errors
///
/// Returns an [`EncodingError::ParseError`] if the price value is out of bounds.
pub fn decode_price(
    bytes: &[u8],
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Price, EncodingError> {
    let raw = get_corrected_raw_price(bytes, precision);
    Price::from_raw_checked(raw, precision)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

/// Decodes a [`Quantity`] from raw bytes with bounds validation.
///
/// Uses corrected raw values to handle floating-point precision errors in stored data.
/// Sentinel values (`QUANTITY_UNDEF`) are preserved unchanged.
///
/// # Errors
///
/// Returns an [`EncodingError::ParseError`] if the quantity value is out of bounds.
pub fn decode_quantity(
    bytes: &[u8],
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Quantity, EncodingError> {
    let raw = get_corrected_raw_quantity(bytes, precision);
    Quantity::from_raw_checked(raw, precision)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

/// Decodes a [`Price`] from raw bytes, using precision 0 for sentinel values.
///
/// For order book data where sentinel values indicate empty levels.
///
/// # Errors
///
/// Returns an [`EncodingError::ParseError`] if the price value is out of bounds.
pub fn decode_price_with_sentinel(
    bytes: &[u8],
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Price, EncodingError> {
    let raw = get_raw_price(bytes);
    let (final_raw, final_precision) = if raw == PRICE_UNDEF {
        (raw, 0)
    } else {
        (get_corrected_raw_price(bytes, precision), precision)
    };
    Price::from_raw_checked(final_raw, final_precision)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

/// Decodes a [`Quantity`] from raw bytes, using precision 0 for sentinel values.
///
/// For order book data where sentinel values indicate empty levels.
///
/// # Errors
///
/// Returns an [`EncodingError::ParseError`] if the quantity value is out of bounds.
pub fn decode_quantity_with_sentinel(
    bytes: &[u8],
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Quantity, EncodingError> {
    let raw = get_raw_quantity(bytes);
    let (final_raw, final_precision) = if raw == QUANTITY_UNDEF {
        (raw, 0)
    } else {
        (get_corrected_raw_quantity(bytes, precision), precision)
    };
    Quantity::from_raw_checked(final_raw, final_precision)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
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

    /// Returns the metadata selected for a chunk.
    ///
    /// The default uses the first element. Implementations may override this when leading sentinel
    /// values do not carry meaningful metadata.
    ///
    /// # Panics
    ///
    /// Panics if `chunk` is empty.
    fn chunk_metadata(chunk: &[Self]) -> HashMap<String, String> {
        chunk
            .first()
            .map(Self::metadata)
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

/// Decodes strongly typed values from Apache Arrow RecordBatch format.
pub trait DecodeTypedFromRecordBatch
where
    Self: Sized + ArrowSchemaProvider,
{
    /// Decodes a `RecordBatch` into a vector of values of the implementing type.
    ///
    /// # Errors
    ///
    /// Returns an `EncodingError` if the decoding fails.
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError>;
}

impl<T> DecodeTypedFromRecordBatch for T
where
    T: DecodeFromRecordBatch,
{
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        Self::decode_batch(metadata, record_batch)
    }
}

/// Decodes raw Data objects from Apache Arrow RecordBatch format.
pub trait DecodeDataFromRecordBatch
where
    Self: Sized + ArrowSchemaProvider,
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

/// Extracts a string column, accepting both Utf8 (`StringArray`) and Utf8View (`StringViewArray`).
/// Parquet may return Utf8View when reading, so this handles both formats.
///
/// # Errors
///
/// Returns an error if:
/// - `column_index` is out of range: `EncodingError::MissingColumn`.
/// - The column type is neither Utf8 nor Utf8View: `EncodingError::InvalidColumnType`.
pub fn extract_column_string<'a>(
    cols: &'a [ArrayRef],
    column_key: &'static str,
    column_index: usize,
) -> Result<StringColumnRef<'a>, EncodingError> {
    let column_values = cols
        .get(column_index)
        .ok_or(EncodingError::MissingColumn(column_key, column_index))?;
    let dt = column_values.data_type();
    if let Some(arr) = column_values.as_any().downcast_ref::<StringArray>() {
        Ok(StringColumnRef::Utf8(arr))
    } else if let Some(arr) = column_values.as_any().downcast_ref::<StringViewArray>() {
        Ok(StringColumnRef::Utf8View(arr))
    } else {
        Err(EncodingError::InvalidColumnType(
            column_key,
            column_index,
            DataType::Utf8,
            dt.clone(),
        ))
    }
}

/// Reference to a string column, either Utf8 or Utf8View.
#[derive(Debug)]
pub enum StringColumnRef<'a> {
    Utf8(&'a StringArray),
    Utf8View(&'a StringViewArray),
}

impl StringColumnRef<'_> {
    /// Returns the string value at row `i`.
    #[inline]
    #[must_use]
    pub fn value(&self, i: usize) -> &str {
        match self {
            Self::Utf8(arr) => arr.value(i),
            Self::Utf8View(arr) => arr.value(i),
        }
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

/// Extracts a column by name when present, falling back to an index for older schemas.
///
/// # Errors
///
/// Returns an error if the resolved column is missing or has the wrong type.
pub fn extract_column_by_name_or_index<'a, T: Array + 'static>(
    record_batch: &'a RecordBatch,
    column_key: &'static str,
    fallback_index: usize,
    expected_type: DataType,
) -> Result<&'a T, EncodingError> {
    let column_index = record_batch
        .schema()
        .index_of(column_key)
        .unwrap_or(fallback_index);
    extract_column::<T>(
        record_batch.columns(),
        column_key,
        column_index,
        expected_type,
    )
}

/// Extracts an optional UTF-8 column by name.
///
/// # Errors
///
/// Returns an error if the column exists but is not UTF-8.
pub fn extract_optional_string_column_by_name<'a>(
    record_batch: &'a RecordBatch,
    column_key: &'static str,
) -> Result<Option<&'a StringArray>, EncodingError> {
    let Ok(column_index) = record_batch.schema().index_of(column_key) else {
        return Ok(None);
    };
    let column_values = record_batch
        .columns()
        .get(column_index)
        .ok_or(EncodingError::MissingColumn(column_key, column_index))?;
    let downcasted_values = column_values.as_any().downcast_ref::<StringArray>().ok_or(
        EncodingError::InvalidColumnType(
            column_key,
            column_index,
            DataType::Utf8,
            column_values.data_type().clone(),
        ),
    )?;
    Ok(Some(downcasted_values))
}

/// Returns an optional [`Ustr`] value from an optional string column.
#[must_use]
pub fn optional_ustr_value(values: Option<&StringArray>, row: usize) -> Option<Ustr> {
    values.and_then(|column| (!column.is_null(row)).then(|| Ustr::from(column.value(row))))
}

/// Validates that a [`FixedSizeBinaryArray`] has the expected precision byte width.
///
/// This detects precision mode mismatches that occur when catalog data was encoded
/// with a different precision mode (64-bit standard vs 128-bit high-precision).
///
/// # Errors
///
/// Returns [`EncodingError::PrecisionMismatch`] if the actual byte width doesn't
/// match [`PRECISION_BYTES`].
pub fn validate_precision_bytes(
    array: &FixedSizeBinaryArray,
    field: &'static str,
) -> Result<(), EncodingError> {
    let actual = array.value_length();
    if actual != PRECISION_BYTES {
        return Err(EncodingError::PrecisionMismatch {
            field,
            expected_bytes: PRECISION_BYTES,
            actual_bytes: actual,
        });
    }
    Ok(())
}

/// Converts a vector of `OrderBookDelta` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Instrument IDs differ, or non-clear precision metadata differs:
///   `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn book_deltas_to_arrow_record_batch_bytes(
    data: &[OrderBookDelta],
) -> Result<RecordBatch, EncodingError> {
    let Some(first) = data.first() else {
        return Err(EncodingError::EmptyData);
    };

    let metadata = OrderBookDelta::chunk_metadata(data);
    let instrument_id = data
        .iter()
        .find(|delta| delta.action != BookAction::Clear)
        .unwrap_or(first)
        .instrument_id;

    if let Some(index) = data.iter().position(|delta| {
        delta.instrument_id != instrument_id
            || (delta.action != BookAction::Clear && delta.metadata() != metadata)
    }) {
        return Err(EncodingError::MixedMetadata { index });
    }

    OrderBookDelta::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

/// Converts a vector of `OrderBookDepth10` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn book_depth10_to_arrow_record_batch_bytes(
    data: &[OrderBookDepth10],
) -> Result<RecordBatch, EncodingError> {
    let Some(first) = data.first() else {
        return Err(EncodingError::EmptyData);
    };
    let precision = data
        .iter()
        .flat_map(|depth| depth.bids.iter().chain(&depth.asks))
        .find(|order| !order.price.is_undefined() && !order.size.is_undefined())
        .map_or(
            (first.bids[0].price.precision, first.bids[0].size.precision),
            |order| (order.price.precision, order.size.precision),
        );

    if let Some(index) = data.iter().position(|depth| {
        depth.instrument_id != first.instrument_id || !depth_precision_is_uniform(depth, precision)
    }) {
        return Err(EncodingError::MixedMetadata { index });
    }

    let metadata = OrderBookDepth10::get_metadata(&first.instrument_id, precision.0, precision.1);
    OrderBookDepth10::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

fn depth_precision_is_uniform(depth: &OrderBookDepth10, precision: (u8, u8)) -> bool {
    depth.bids.iter().chain(&depth.asks).all(|order| {
        match (order.price.is_undefined(), order.size.is_undefined()) {
            (true, true) => true,
            (false, false) => {
                order.price.precision == precision.0 && order.size.precision == precision.1
            }
            _ => false,
        }
    })
}

/// Converts a vector of `QuoteTick` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn quotes_to_arrow_record_batch_bytes(
    data: &[QuoteTick],
) -> Result<RecordBatch, EncodingError> {
    encode_batch_with_metadata(data)
}

/// Converts a vector of `TradeTick` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn trades_to_arrow_record_batch_bytes(
    data: &[TradeTick],
) -> Result<RecordBatch, EncodingError> {
    encode_batch_with_metadata(data)
}

/// Converts a vector of `Bar` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn bars_to_arrow_record_batch_bytes(data: &[Bar]) -> Result<RecordBatch, EncodingError> {
    encode_batch_with_metadata(data)
}

/// Converts a vector of `MarkPriceUpdate` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn mark_prices_to_arrow_record_batch_bytes(
    data: &[MarkPriceUpdate],
) -> Result<RecordBatch, EncodingError> {
    encode_batch_with_metadata(data)
}

/// Converts a vector of `IndexPriceUpdate` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn index_prices_to_arrow_record_batch_bytes(
    data: &[IndexPriceUpdate],
) -> Result<RecordBatch, EncodingError> {
    encode_batch_with_metadata(data)
}

/// Converts a vector of `InstrumentStatus` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[expect(clippy::missing_panics_doc)] // Guarded by empty check
pub fn instrument_status_to_arrow_record_batch_bytes(
    data: &[InstrumentStatus],
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    let first = data.first().unwrap();
    let metadata = first.metadata();
    InstrumentStatus::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

/// Converts a vector of `OptionGreeks` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Encoding fails: `EncodingError::ArrowError`.
#[expect(clippy::missing_panics_doc)] // Guarded by empty check
pub fn option_greeks_to_arrow_record_batch_bytes(
    data: &[OptionGreeks],
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    let first = data.first().unwrap();
    let metadata = first.metadata();
    OptionGreeks::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

/// Converts a vector of `InstrumentClose` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn instrument_closes_to_arrow_record_batch_bytes(
    data: &[InstrumentClose],
) -> Result<RecordBatch, EncodingError> {
    encode_batch_with_metadata(data)
}

fn encode_batch_with_metadata<T>(data: &[T]) -> Result<RecordBatch, EncodingError>
where
    T: EncodeToRecordBatch,
{
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    let metadata = T::chunk_metadata(data);
    if let Some(index) = data.iter().position(|value| value.metadata() != metadata) {
        return Err(EncodingError::MixedMetadata { index });
    }

    T::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::{
            Bar, BarSpecification, BarType, BookOrder, OrderBookDelta, OrderBookDepth10, QuoteTick,
            depth::DEPTH10_LEN,
        },
        enums::{AggregationSource, BarAggregation, BookAction, OrderSide, PriceType},
        identifiers::InstrumentId,
        types::{PRICE_UNDEF, Price, QUANTITY_UNDEF, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_quotes_to_arrow_record_batch_rejects_mixed_instruments() {
        let first = QuoteTick::new(
            InstrumentId::from("AAPL.XNAS"),
            Price::from("100.01"),
            Price::from("100.02"),
            Quantity::from("10"),
            Quantity::from("11"),
            1.into(),
            1.into(),
        );
        let second = QuoteTick::new(
            InstrumentId::from("MSFT.XNAS"),
            Price::from("200.01"),
            Price::from("200.02"),
            Quantity::from("20"),
            Quantity::from("21"),
            2.into(),
            2.into(),
        );

        let result = quotes_to_arrow_record_batch_bytes(&[first, second]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 1 })
        ));
    }

    #[rstest]
    fn test_quotes_to_arrow_record_batch_rejects_mixed_precision() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let first = QuoteTick::new(
            instrument_id,
            Price::from("100.01"),
            Price::from("100.02"),
            Quantity::from("10.00"),
            Quantity::from("11.00"),
            1.into(),
            1.into(),
        );
        let second = QuoteTick::new(
            instrument_id,
            Price::from("100.010"),
            Price::from("100.020"),
            Quantity::from("10.000"),
            Quantity::from("11.000"),
            2.into(),
            2.into(),
        );

        let result = quotes_to_arrow_record_batch_bytes(&[first, second]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 1 })
        ));
    }

    #[rstest]
    fn test_bars_to_arrow_record_batch_rejects_mixed_bar_types() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let first_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::Internal,
        );
        let second_type = BarType::new(
            instrument_id,
            BarSpecification::new(5, BarAggregation::Minute, PriceType::Last),
            AggregationSource::Internal,
        );
        let first = Bar::new(
            first_type,
            Price::from("100.01"),
            Price::from("100.02"),
            Price::from("100.00"),
            Price::from("100.01"),
            Quantity::from("10"),
            1.into(),
            1.into(),
        );
        let second = Bar::new(
            second_type,
            Price::from("100.01"),
            Price::from("100.02"),
            Price::from("100.00"),
            Price::from("100.01"),
            Quantity::from("11"),
            2.into(),
            2.into(),
        );

        let result = bars_to_arrow_record_batch_bytes(&[first, second]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 1 })
        ));
    }

    #[rstest]
    fn test_depth10_to_arrow_record_batch_rejects_mixed_level_price_precision() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.23"),
            Quantity::from("100.00"),
            1,
        );
        let ask = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.24"),
            Quantity::from("100.00"),
            2,
        );
        let mut asks = [ask; DEPTH10_LEN];
        asks[1].price = Price::from("1.241");
        let depth = OrderBookDepth10::new(
            instrument_id,
            [bid; DEPTH10_LEN],
            asks,
            [1; DEPTH10_LEN],
            [1; DEPTH10_LEN],
            0,
            1,
            1.into(),
            1.into(),
        );

        let result = book_depth10_to_arrow_record_batch_bytes(&[depth]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 0 })
        ));
    }

    #[rstest]
    fn test_depth10_to_arrow_record_batch_rejects_mixed_level_size_precision() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.23"),
            Quantity::from("100.00"),
            1,
        );
        let ask = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.24"),
            Quantity::from("100.00"),
            2,
        );
        let mut bids = [bid; DEPTH10_LEN];
        bids[1].size = Quantity::from("100.000");
        let depth = OrderBookDepth10::new(
            instrument_id,
            bids,
            [ask; DEPTH10_LEN],
            [1; DEPTH10_LEN],
            [1; DEPTH10_LEN],
            0,
            1,
            1.into(),
            1.into(),
        );

        let result = book_depth10_to_arrow_record_batch_bytes(&[depth]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 0 })
        ));
    }

    #[rstest]
    fn test_depth10_to_arrow_record_batch_uses_first_defined_level_precision() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.23"),
            Quantity::from("100.00"),
            1,
        );
        let ask = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.24"),
            Quantity::from("100.00"),
            2,
        );
        let mut bids = [bid; DEPTH10_LEN];
        bids[0].price = Price::from_raw(PRICE_UNDEF, 0);
        bids[0].size = Quantity::from_raw(QUANTITY_UNDEF, 0);
        let depth = OrderBookDepth10::new(
            instrument_id,
            bids,
            [ask; DEPTH10_LEN],
            [0; DEPTH10_LEN],
            [1; DEPTH10_LEN],
            0,
            1,
            1.into(),
            1.into(),
        );

        let result = book_depth10_to_arrow_record_batch_bytes(&[depth]).unwrap();

        assert_eq!(
            result.schema().metadata().get(KEY_PRICE_PRECISION).unwrap(),
            "2"
        );
        assert_eq!(
            result.schema().metadata().get(KEY_SIZE_PRECISION).unwrap(),
            "2"
        );
    }

    #[rstest]
    #[case::price(true)]
    #[case::size(false)]
    fn test_depth10_to_arrow_record_batch_rejects_partial_undefined_level(
        #[case] price_undefined: bool,
    ) {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.23"),
            Quantity::from("100.00"),
            1,
        );
        let ask = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.24"),
            Quantity::from("100.00"),
            2,
        );
        let mut asks = [ask; DEPTH10_LEN];
        if price_undefined {
            asks[1].price = Price::from_raw(PRICE_UNDEF, 0);
        } else {
            asks[1].size = Quantity::from_raw(QUANTITY_UNDEF, 0);
        }
        let depth = OrderBookDepth10::new(
            instrument_id,
            [bid; DEPTH10_LEN],
            asks,
            [1; DEPTH10_LEN],
            [1; DEPTH10_LEN],
            0,
            1,
            1.into(),
            1.into(),
        );

        let result = book_depth10_to_arrow_record_batch_bytes(&[depth]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 0 })
        ));
    }

    #[rstest]
    fn test_deltas_to_arrow_record_batch_skips_leading_clears_for_precision() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let first = OrderBookDelta::clear(instrument_id, 0, 1.into(), 1.into());
        let second = OrderBookDelta::clear(instrument_id, 1, 2.into(), 2.into());
        let third = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1.23"),
                Quantity::from("100.000000"),
                1,
            ),
            0,
            2,
            3.into(),
            3.into(),
        );
        let expected = vec![first, second, third];

        let batch = book_deltas_to_arrow_record_batch_bytes(&expected).unwrap();
        let metadata = batch.schema().metadata().clone();
        assert_eq!(
            metadata.get(KEY_PRICE_PRECISION).map(String::as_str),
            Some("2")
        );
        assert_eq!(
            metadata.get(KEY_SIZE_PRECISION).map(String::as_str),
            Some("6")
        );

        let decoded = OrderBookDelta::decode_batch(&metadata, batch).unwrap();

        assert_eq!(decoded, expected);
        assert_eq!(decoded[2].order.price.precision, 2);
        assert_eq!(decoded[2].order.size.precision, 6);
    }

    #[rstest]
    fn test_deltas_to_arrow_record_batch_all_clear_roundtrip() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let expected = vec![
            OrderBookDelta::clear(instrument_id, 0, 1.into(), 1.into()),
            OrderBookDelta::clear(instrument_id, 1, 2.into(), 2.into()),
        ];

        let batch = book_deltas_to_arrow_record_batch_bytes(&expected).unwrap();
        let metadata = batch.schema().metadata().clone();
        let decoded = OrderBookDelta::decode_batch(&metadata, batch).unwrap();

        assert_eq!(decoded, expected);
    }

    #[rstest]
    fn test_deltas_to_arrow_record_batch_rejects_mixed_precision() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let first = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1.23"),
                Quantity::from("100.00"),
                1,
            ),
            0,
            1,
            1.into(),
            1.into(),
        );
        let second = OrderBookDelta::new(
            instrument_id,
            BookAction::Update,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1.234"),
                Quantity::from("100.000"),
                1,
            ),
            0,
            2,
            2.into(),
            2.into(),
        );

        let result = book_deltas_to_arrow_record_batch_bytes(&[first, second]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 1 })
        ));
    }

    #[rstest]
    fn test_deltas_to_arrow_record_batch_rejects_mixed_instruments() {
        let first = OrderBookDelta::clear(InstrumentId::from("AUD/USD.SIM"), 0, 1.into(), 1.into());
        let second = OrderBookDelta::new(
            InstrumentId::from("EUR/USD.SIM"),
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("1.23"),
                Quantity::from("100.00"),
                1,
            ),
            0,
            1,
            2.into(),
            2.into(),
        );

        let result = book_deltas_to_arrow_record_batch_bytes(&[first, second]);

        // The first non-clear delta supplies metadata, so the leading clear is the mismatched row.
        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 0 })
        ));
    }
}
