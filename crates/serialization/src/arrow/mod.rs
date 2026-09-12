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
pub mod catalog_display;
pub mod close;
pub mod custom;
pub mod delta;
pub mod depth;
pub mod funding;
pub mod index_price;
pub mod instrument;
pub mod instrument_status;
pub mod json;
pub mod legacy;
pub mod mark_price;
pub mod option_greeks;
pub mod order_event;
pub mod position_event;
pub mod quote;
pub mod report;
pub mod snapshot;
pub mod trade;

#[cfg(feature = "arrow-display")]
pub mod display;

mod depth_display;

#[cfg(test)]
pub(crate) mod test_support;

use std::{
    borrow::Borrow,
    collections::HashMap,
    fmt::{Display, Write as FmtWrite},
    io::{self, Write},
    str::FromStr,
    sync::Arc,
};

use arrow::{
    array::{
        Array, ArrayRef, BinaryArray, BinaryViewArray, Decimal128Array, DictionaryArray,
        FixedSizeBinaryArray, FixedSizeListArray, Int32Array, Int64Array, StringArray,
        StringBuilder, StringDictionaryBuilder, StringViewArray, StructArray,
        TimestampNanosecondArray, UInt8Array, UInt32Array, UInt64Array,
    },
    buffer::NullBuffer,
    datatypes::{DataType, Field, Int8Type, Int32Type, Schema, TimeUnit},
    error::ArrowError,
    ipc::writer::StreamWriter,
    record_batch::RecordBatch,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        DEPTH10_LEN, Data, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate, bar::Bar,
        close::InstrumentClose, delta::OrderBookDelta, depth::OrderBookDepth,
        option_chain::OptionGreeks, quote::QuoteTick, trade::TradeTick,
    },
    enums::{AggressorSide, BookAction, FromU8, InstrumentCloseType, OrderSide},
    identifiers::InstrumentId,
    types::{
        Currency, Money, PRICE_ERROR, PRICE_UNDEF, Price, QUANTITY_UNDEF, Quantity,
        fixed::{
            FIXED_PRECISION, FIXED_PRECISION_STANDARD, PRECISION_BYTES, correct_price_raw,
            correct_quantity_raw,
        },
        money::MoneyRaw,
        price::PriceRaw,
        quantity::{QUANTITY_RAW_MAX, QuantityRaw},
    },
};
#[cfg(feature = "python")]
use pyo3::prelude::*;
use rust_decimal::Decimal;
use ustr::Ustr;

// Define metadata key constants constants
pub const KEY_BAR_TYPE: &str = "bar_type";
pub const KEY_IDENTIFIER: &str = "identifier";
pub const KEY_INSTRUMENT_ID: &str = "instrument_id";
pub const KEY_PRICE_PRECISION: &str = "price_precision";
pub const KEY_SIZE_PRECISION: &str = "size_precision";

pub(crate) fn parse_metadata(
    metadata: &HashMap<String, String>,
) -> Result<(InstrumentId, u8, u8), EncodingError> {
    let instrument_id = metadata
        .get(KEY_INSTRUMENT_ID)
        .ok_or(EncodingError::MissingMetadata(KEY_INSTRUMENT_ID))?
        .parse::<InstrumentId>()
        .map_err(|e| EncodingError::ParseError(KEY_INSTRUMENT_ID, e.to_string()))?;
    let price_precision = metadata
        .get(KEY_PRICE_PRECISION)
        .ok_or(EncodingError::MissingMetadata(KEY_PRICE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_PRICE_PRECISION, e.to_string()))?;
    let size_precision = metadata
        .get(KEY_SIZE_PRECISION)
        .ok_or(EncodingError::MissingMetadata(KEY_SIZE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_SIZE_PRECISION, e.to_string()))?;
    Ok((instrument_id, price_precision, size_precision))
}
pub const KEY_TYPE_NAME: &str = "type_name";
pub const FIXED_DECIMAL_PRECISION: u8 = 38;
pub const FIXED_DECIMAL_SCALE: i8 = 16;
pub(crate) const EMPTY_DEPTH_PRECISION: (u8, u8) = (0, 0);

/// Returns the open Arrow data type used for nanosecond instants.
#[must_use]
pub fn timestamp_data_type() -> DataType {
    DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))
}

/// Builds a UTC nanosecond timestamp array from model timestamp values.
///
/// # Errors
///
/// Returns an [`ArrowError`] when a value exceeds Arrow's signed timestamp range.
pub fn timestamp_array(
    values: impl IntoIterator<Item = u64>,
) -> Result<TimestampNanosecondArray, ArrowError> {
    optional_timestamp_array(values.into_iter().map(Some))
}

/// Builds a nullable UTC nanosecond timestamp array from model timestamp values.
///
/// # Errors
///
/// Returns an [`ArrowError`] when a value exceeds Arrow's signed timestamp range.
pub fn optional_timestamp_array(
    values: impl IntoIterator<Item = Option<u64>>,
) -> Result<TimestampNanosecondArray, ArrowError> {
    let values = values
        .into_iter()
        .map(|value| {
            value
                .map(|value| {
                    i64::try_from(value).map_err(|_| {
                        ArrowError::InvalidArgumentError(format!(
                            "Nanosecond timestamp {value} exceeds Arrow's signed timestamp range"
                        ))
                    })
                })
                .transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TimestampNanosecondArray::from(values).with_data_type(timestamp_data_type()))
}

/// Reads a non-negative Arrow nanosecond timestamp as the model's unsigned representation.
///
/// # Errors
///
/// Returns an [`EncodingError`] when the value is negative.
pub fn decode_timestamp(
    values: &TimestampNanosecondArray,
    name: &'static str,
    row: usize,
) -> Result<u64, EncodingError> {
    u64::try_from(values.value(row)).map_err(|_| {
        EncodingError::ParseError(
            name,
            format!(
                "row {row}: negative nanosecond timestamp {}",
                values.value(row)
            ),
        )
    })
}

/// Builds a record batch, converting unsigned nanosecond inputs for timestamp schema fields.
///
/// This keeps model encoders simple while ensuring their public Arrow batches use logical
/// timestamp columns.
///
/// # Errors
///
/// Returns an [`ArrowError`] when a timestamp exceeds the signed Arrow range or the batch is
/// otherwise invalid.
pub fn record_batch_with_timestamps(
    schema: Arc<Schema>,
    columns: Vec<ArrayRef>,
) -> Result<RecordBatch, ArrowError> {
    validate_encode_precisions(schema.metadata())?;

    let columns = schema
        .fields()
        .iter()
        .zip(columns)
        .map(|(field, column)| {
            if field.data_type() == &enum_dictionary_data_type()
                && column.data_type() == &DataType::UInt8
                && is_legacy_enum_field(field.name())
            {
                return legacy_enum_dictionary_column(field, column.as_ref());
            }

            if field.data_type() != &timestamp_data_type()
                || column.data_type() != &DataType::UInt64
            {
                return Ok(column);
            }
            timestamp_column(field, column.as_ref())
        })
        .collect::<Result<Vec<_>, ArrowError>>()?;
    RecordBatch::try_new(schema, columns)
}

// Rejects batch metadata whose precisions exceed the catalog's uniform decimal scale. Defi
// precisions (for example wei at 17 or 18) store raws at their own native scale, so encoding
// them bit-for-bit into `Decimal128(38, 16)` columns would inflate the externally visible
// values; mirror the SBE and custom-data macro encode guards and fail the write instead.
fn validate_encode_precisions(metadata: &HashMap<String, String>) -> Result<(), ArrowError> {
    for key in [KEY_PRICE_PRECISION, KEY_SIZE_PRECISION] {
        if let Some(value) = metadata.get(key)
            && let Ok(precision) = value.parse::<u8>()
            && precision > FIXED_DECIMAL_SCALE as u8
        {
            return Err(ArrowError::InvalidArgumentError(format!(
                "Metadata '{key}' is {precision}, maximum supported catalog scale is {FIXED_DECIMAL_SCALE}"
            )));
        }
    }
    Ok(())
}

/// Converts logical timestamp columns to unsigned nanoseconds for existing model decoders.
///
/// # Errors
///
/// Returns an [`EncodingError`] for negative timestamps or invalid arrays.
pub fn record_batch_with_u64_timestamps(batch: &RecordBatch) -> Result<RecordBatch, EncodingError> {
    let mut changed = false;
    let mut fields = Vec::with_capacity(batch.num_columns());
    let mut columns = Vec::with_capacity(batch.num_columns());

    for (field, column) in batch.schema().fields().iter().zip(batch.columns()) {
        if field.data_type() != &timestamp_data_type() {
            fields.push(field.clone());
            columns.push(column.clone());
            continue;
        }
        let values = column
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .ok_or_else(|| {
                EncodingError::ParseError(
                    "timestamp",
                    format!("Column '{}' is not TimestampNanosecond", field.name()),
                )
            })?;
        let values = (0..values.len())
            .map(|row| {
                if values.is_null(row) {
                    Ok(None)
                } else {
                    decode_timestamp(values, "timestamp", row).map(Some)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        fields.push(Arc::new(
            field.as_ref().clone().with_data_type(DataType::UInt64),
        ));
        columns.push(Arc::new(UInt64Array::from(values)) as ArrayRef);
        changed = true;
    }

    if !changed {
        return Ok(batch.clone());
    }
    RecordBatch::try_new(
        Arc::new(Schema::new_with_metadata(
            fields,
            batch.schema().metadata().clone(),
        )),
        columns,
    )
    .map_err(EncodingError::from)
}

/// Returns the open Arrow data type used for enum-valued catalog columns.
#[must_use]
pub fn enum_dictionary_data_type() -> DataType {
    DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8))
}

/// Builds a compact dictionary array containing enum display names.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the number of distinct values exceeds the `Int8` key range.
pub fn enum_dictionary_array(
    values: impl IntoIterator<Item = impl Display>,
) -> Result<DictionaryArray<Int8Type>, ArrowError> {
    let mut builder = StringDictionaryBuilder::<Int8Type>::new();
    for value in values {
        builder.append(value.to_string())?;
    }
    Ok(builder.finish())
}

/// Returns the open Arrow data type used for monetary values.
#[must_use]
pub fn money_data_type() -> DataType {
    DataType::Struct(
        vec![
            Field::new("amount", fixed_decimal_data_type(), false),
            Field::new(
                "currency",
                DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
                false,
            ),
        ]
        .into(),
    )
}

/// Builds a nullable struct array containing monetary amounts and currencies.
///
/// # Errors
///
/// Returns an [`ArrowError`] if an amount or dictionary value cannot be represented.
pub fn money_array(
    values: impl IntoIterator<Item = Option<Money>>,
) -> Result<StructArray, ArrowError> {
    let mut amounts = Vec::new();
    let mut currencies = StringDictionaryBuilder::<Int32Type>::new();
    let mut validity = Vec::new();

    for value in values {
        if let Some(value) = value {
            amounts.push(money_raw_to_decimal(value.raw()));
            currencies.append(value.currency.to_string())?;
            validity.push(true);
        } else {
            amounts.push(0);
            currencies.append_null();
            validity.push(false);
        }
    }
    let amounts = Decimal128Array::from(amounts)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)?;
    StructArray::try_new(
        match money_data_type() {
            DataType::Struct(fields) => fields,
            _ => unreachable!("money data type is a struct"),
        },
        vec![Arc::new(amounts), Arc::new(currencies.finish())],
        Some(NullBuffer::from(validity)),
    )
}

/// Encodes a Rust decimal at the catalog's scale.
///
/// # Errors
///
/// Returns an [`ArrowError`] naming `field` when the value has more than 16 decimal places or
/// cannot be rescaled exactly.
pub fn decimal_to_arrow(value: &Decimal, field: &'static str) -> Result<i128, ArrowError> {
    let value = value.normalize();
    let scale = value.scale();
    if scale > FIXED_DECIMAL_SCALE as u32 {
        return Err(ArrowError::InvalidArgumentError(format!(
            "Decimal field '{field}' has scale {scale}, maximum supported scale is {FIXED_DECIMAL_SCALE}"
        )));
    }
    let rescaled = value
        .mantissa()
        .checked_mul(10_i128.pow(FIXED_DECIMAL_SCALE as u32 - scale))
        .ok_or_else(|| {
            ArrowError::InvalidArgumentError(format!(
                "Decimal field '{field}' cannot be represented as Decimal128(38, 16)"
            ))
        })?;
    let max = Decimal::MAX.mantissa();
    if rescaled < -max || rescaled > max {
        return Err(ArrowError::InvalidArgumentError(format!(
            "Decimal field '{field}' exceeds the rust_decimal 96-bit range after rescaling"
        )));
    }
    Ok(rescaled)
}

/// Decodes a Rust decimal from the catalog's scale.
///
/// # Errors
///
/// Returns an [`EncodingError`] if the value is NULL or outside `rust_decimal`'s range.
pub fn decode_decimal(
    values: &Decimal128Array,
    field: &'static str,
    row: usize,
) -> Result<Decimal, EncodingError> {
    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required decimal is null"),
        ));
    }
    Decimal::try_from_i128_with_scale(values.value(row), FIXED_DECIMAL_SCALE as u32)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

/// Decodes a monetary value from its Arrow struct representation.
///
/// # Errors
///
/// Returns an [`EncodingError`] if the struct is NULL, malformed, or outside the model range.
pub fn decode_money(
    values: &StructArray,
    field: &'static str,
    row: usize,
) -> Result<Money, EncodingError> {
    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required money is null"),
        ));
    }
    let amounts = values
        .column_by_name("amount")
        .and_then(|array| array.as_any().downcast_ref::<Decimal128Array>())
        .ok_or_else(|| {
            EncodingError::ParseError(field, "money amount must be Decimal128(38, 16)".to_string())
        })?;
    let currencies = values
        .column_by_name("currency")
        .and_then(|array| StringColumnRef::try_from_array(array.as_ref()))
        .ok_or_else(|| {
            EncodingError::ParseError(
                field,
                "money currency must be Dictionary<Int32, Utf8>".to_string(),
            )
        })?;
    let raw = decimal_to_money_raw(amounts.value(row), field, row)?;
    let currency_code = currencies.value(row);
    let currency = Currency::from_str(currency_code).map_err(|e| {
        EncodingError::ParseError(
            field,
            format!(
                "row {row}: currency '{currency_code}' must be registered before decoding Money: {e}"
            ),
        )
    })?;
    Money::from_raw_checked(raw, currency)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

#[allow(
    clippy::useless_conversion,
    reason = "MoneyRaw is i64 or i128 depending on model feature unification"
)]
fn money_raw_to_decimal(raw: MoneyRaw) -> i128 {
    let mut decimal = i128::from(raw);
    if FIXED_PRECISION == FIXED_PRECISION_STANDARD {
        decimal *= STANDARD_TO_DECIMAL_SCALE;
    }
    decimal
}

fn decimal_to_money_raw(
    value: i128,
    field: &'static str,
    row: usize,
) -> Result<MoneyRaw, EncodingError> {
    decimal_to_raw(value, field, row, "MoneyRaw")
}

const STANDARD_TO_DECIMAL_SCALE: i128 =
    10_i128.pow((FIXED_DECIMAL_SCALE as u8 - FIXED_PRECISION_STANDARD) as u32);

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

/// Returns the open fixed-point Arrow data type used by catalog write schemas.
#[must_use]
pub const fn fixed_decimal_data_type() -> DataType {
    DataType::Decimal128(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
}

/// Returns the open Arrow type for a recognized legacy catalog field.
#[must_use]
pub fn normalized_legacy_data_type(name: &str, data_type: &DataType) -> DataType {
    match data_type {
        DataType::FixedSizeBinary(8 | 16) => fixed_decimal_data_type(),
        DataType::UInt64 if is_timestamp_field(name) => timestamp_data_type(),
        DataType::Timestamp(TimeUnit::Nanosecond, None) if is_timestamp_field(name) => {
            timestamp_data_type()
        }
        DataType::UInt8 if is_legacy_enum_field(name) => enum_dictionary_data_type(),
        _ => data_type.clone(),
    }
}

/// Returns a UTF-8 field annotated with the canonical Arrow JSON extension.
#[must_use]
pub fn json_string_field(name: impl Into<String>, nullable: bool) -> Field {
    Field::new(name, DataType::Utf8, nullable).with_metadata(HashMap::from([
        ("ARROW:extension:name".to_string(), "arrow.json".to_string()),
        ("ARROW:extension:metadata".to_string(), String::new()),
    ]))
}

/// Returns whether a field carries the canonical Arrow JSON extension.
#[must_use]
pub fn is_json_string_field(field: &Field) -> bool {
    field.extension_type_name() == Some("arrow.json")
}

/// Normalizes legacy fixed-point byte columns to the open decimal representation.
///
/// # Errors
///
/// Returns an [`ArrowError`] if a column has an invalid physical array or value.
pub fn normalize_legacy_fixed_columns(batch: &RecordBatch) -> Result<RecordBatch, ArrowError> {
    let mut changed = false;
    let mut fields = Vec::with_capacity(batch.num_columns());
    let mut columns = Vec::with_capacity(batch.num_columns());
    let normalize_named_fields = is_nautilus_legacy_schema(batch.schema_ref());
    let normalize_timestamps = is_nautilus_timestamp_schema(batch.schema_ref());

    for (field, column) in batch.schema().fields().iter().zip(batch.columns()) {
        let custom_timestamp = batch.schema().metadata().contains_key("type_name")
            && matches!(field.name().as_str(), "ts_event" | "ts_init");
        if field.data_type() == &DataType::UInt64
            && ((normalize_named_fields && is_timestamp_field(field.name())) || custom_timestamp)
        {
            let timestamps = timestamp_column(field, column.as_ref())?;
            fields.push(Arc::new(field.as_ref().clone().with_data_type(
                normalized_legacy_data_type(field.name(), field.data_type()),
            )));
            columns.push(timestamps);
            changed = true;
            continue;
        }

        if normalize_timestamps
            && normalized_timestamp_type(field.data_type()) != *field.data_type()
        {
            fields.push(Arc::new(
                field.as_ref().clone().with_data_type(timestamp_data_type()),
            ));
            let timestamps = column
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .ok_or_else(|| {
                    ArrowError::CastError(format!(
                        "Column '{}' is not a nanosecond timestamp",
                        field.name()
                    ))
                })?;
            columns.push(Arc::new(
                timestamps.clone().with_data_type(timestamp_data_type()),
            ));
            changed = true;
            continue;
        }

        if normalize_named_fields
            && field.data_type() == &DataType::UInt8
            && is_legacy_enum_field(field.name())
        {
            fields.push(Arc::new(field.as_ref().clone().with_data_type(
                normalized_legacy_data_type(field.name(), field.data_type()),
            )));
            columns.push(legacy_enum_dictionary_column(field, column.as_ref())?);
            changed = true;
            continue;
        }

        if normalize_named_fields
            && let DataType::FixedSizeList(item, length) = field.data_type()
            && let DataType::FixedSizeBinary(width @ (8 | 16)) = item.data_type()
        {
            let list = column
                .as_any()
                .downcast_ref::<FixedSizeListArray>()
                .ok_or_else(|| {
                    ArrowError::CastError(format!("Column '{}' is not FixedSizeList", field.name()))
                })?;
            let values = list
                .values()
                .as_any()
                .downcast_ref::<FixedSizeBinaryArray>()
                .ok_or_else(|| {
                    ArrowError::CastError(format!(
                        "Column '{}' values are not FixedSizeBinary",
                        field.name()
                    ))
                })?;
            let decimal = normalize_legacy_fixed_array(field.name(), values, *width)?;
            let item = Arc::new(
                item.as_ref()
                    .clone()
                    .with_data_type(fixed_decimal_data_type())
                    .with_nullable(true),
            );
            let list = FixedSizeListArray::try_new(
                Arc::clone(&item),
                *length,
                Arc::new(decimal),
                list.nulls().cloned(),
            )?;
            fields.push(Arc::new(
                field
                    .as_ref()
                    .clone()
                    .with_data_type(DataType::FixedSizeList(item, *length)),
            ));
            columns.push(Arc::new(list) as ArrayRef);
            changed = true;
            continue;
        }

        let DataType::FixedSizeBinary(width @ (8 | 16)) = field.data_type() else {
            fields.push(field.clone());
            columns.push(column.clone());
            continue;
        };

        if !normalize_named_fields {
            fields.push(field.clone());
            columns.push(column.clone());
            continue;
        }
        let values = column
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .ok_or_else(|| {
                ArrowError::CastError(format!("Column '{}' is not FixedSizeBinary", field.name()))
            })?;
        let decimal = normalize_legacy_fixed_array(field.name(), values, *width)?;
        fields.push(Arc::new(
            field
                .as_ref()
                .clone()
                .with_data_type(normalized_legacy_data_type(field.name(), field.data_type()))
                .with_nullable(true),
        ));
        columns.push(Arc::new(decimal) as ArrayRef);
        changed = true;
    }

    if !changed {
        return Ok(batch.clone());
    }

    let schema = Arc::new(Schema::new_with_metadata(
        fields,
        batch.schema().metadata().clone(),
    ));
    RecordBatch::try_new(schema, columns)
}

fn legacy_enum_dictionary_column(
    field: &Field,
    column: &dyn Array,
) -> Result<ArrayRef, ArrowError> {
    let values = column
        .as_any()
        .downcast_ref::<UInt8Array>()
        .ok_or_else(|| ArrowError::CastError(format!("Column '{}' is not UInt8", field.name())))?;
    let names = (0..values.len())
        .map(|row| legacy_enum_name(field.name(), values.value(row)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Arc::new(enum_dictionary_array(names)?) as ArrayRef)
}

fn timestamp_column(field: &Field, column: &dyn Array) -> Result<ArrayRef, ArrowError> {
    let values = column
        .as_any()
        .downcast_ref::<UInt64Array>()
        .ok_or_else(|| ArrowError::CastError(format!("Column '{}' is not UInt64", field.name())))?;
    let timestamps = optional_timestamp_array(
        (0..values.len()).map(|row| (!values.is_null(row)).then(|| values.value(row))),
    )?;
    Ok(Arc::new(timestamps) as ArrayRef)
}

fn normalize_legacy_fixed_array(
    name: &str,
    values: &FixedSizeBinaryArray,
    width: i32,
) -> Result<Decimal128Array, ArrowError> {
    let quantity = is_legacy_quantity_field(name);
    let mut decimals = Vec::with_capacity(values.len());

    for row in 0..values.len() {
        if values.is_null(row) {
            decimals.push(None);
            continue;
        }

        let bytes = values.value(row);
        let decimal = match (width, quantity) {
            (8, false) => {
                let raw = i64::from_le_bytes(bytes.try_into().map_err(|e| {
                    ArrowError::CastError(format!(
                        "Invalid legacy price column '{name}' at row {row}: {e}"
                    ))
                })?);

                if raw == i64::MIN {
                    return Err(ArrowError::CastError(format!(
                        "Legacy price column '{name}' contains PRICE_ERROR raw value {raw} at row {row}",
                    )));
                }
                (raw != i64::MAX).then_some(i128::from(raw) * STANDARD_TO_DECIMAL_SCALE)
            }
            (8, true) => {
                let raw = u64::from_le_bytes(bytes.try_into().map_err(|e| {
                    ArrowError::CastError(format!(
                        "Invalid legacy quantity column '{name}' at row {row}: {e}"
                    ))
                })?);
                (raw != u64::MAX).then_some(i128::from(raw) * STANDARD_TO_DECIMAL_SCALE)
            }
            (16, false) => {
                let raw = i128::from_le_bytes(bytes.try_into().map_err(|e| {
                    ArrowError::CastError(format!(
                        "Invalid legacy price column '{name}' at row {row}: {e}"
                    ))
                })?);

                if raw == i128::MIN {
                    return Err(ArrowError::CastError(format!(
                        "Legacy price column '{name}' contains PRICE_ERROR raw value {raw} at row {row}",
                    )));
                }
                (raw != i128::MAX).then_some(raw)
            }
            (16, true) => {
                let raw = u128::from_le_bytes(bytes.try_into().map_err(|e| {
                    ArrowError::CastError(format!(
                        "Invalid legacy quantity column '{name}' at row {row}: {e}"
                    ))
                })?);

                if raw == u128::MAX {
                    None
                } else {
                    Some(i128::try_from(raw).map_err(|_| {
                        ArrowError::CastError(format!(
                            "Legacy quantity column '{name}' exceeds Decimal128 at row {row}"
                        ))
                    })?)
                }
            }
            _ => unreachable!("legacy fixed width checked above"),
        };
        decimals.push(decimal);
    }

    Decimal128Array::from(decimals)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
}

type LegacySchemaFields = &'static [(&'static str, LegacyFieldType, bool)];

const LEGACY_SCHEMA_FAMILIES: &[(&[&str], LegacySchemaFields)] = &[
    (
        &["OrderBookDelta"],
        &[
            ("action", LegacyFieldType::UInt8, false),
            ("side", LegacyFieldType::UInt8, false),
            ("price", LegacyFieldType::Fixed, false),
            ("size", LegacyFieldType::Fixed, false),
            ("order_id", LegacyFieldType::UInt64, false),
            ("flags", LegacyFieldType::UInt8, false),
            ("sequence", LegacyFieldType::UInt64, false),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
        ],
    ),
    (
        &["TradeTick"],
        &[
            ("price", LegacyFieldType::Fixed, false),
            ("size", LegacyFieldType::Fixed, false),
            ("aggressor_side", LegacyFieldType::UInt8, false),
            ("trade_id", LegacyFieldType::Utf8, false),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
        ],
    ),
    (
        &["InstrumentClose"],
        &[
            ("close_price", LegacyFieldType::Fixed, false),
            ("close_type", LegacyFieldType::UInt8, false),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
        ],
    ),
    (
        &["InstrumentClose"],
        &[
            ("instrument_id", LegacyFieldType::Utf8, true),
            ("close_type", LegacyFieldType::Utf8, true),
            ("close_price", LegacyFieldType::Utf8, true),
            ("ts_event", LegacyFieldType::UInt64, true),
            ("ts_init", LegacyFieldType::UInt64, true),
        ],
    ),
    (
        &["QuoteTick"],
        &[
            ("bid_price", LegacyFieldType::Fixed, false),
            ("ask_price", LegacyFieldType::Fixed, false),
            ("bid_size", LegacyFieldType::Fixed, false),
            ("ask_size", LegacyFieldType::Fixed, false),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
        ],
    ),
    (
        &["Bar"],
        &[
            ("open", LegacyFieldType::Fixed, false),
            ("high", LegacyFieldType::Fixed, false),
            ("low", LegacyFieldType::Fixed, false),
            ("close", LegacyFieldType::Fixed, false),
            ("volume", LegacyFieldType::Fixed, false),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
        ],
    ),
    (
        &["MarkPriceUpdate", "IndexPriceUpdate"],
        &[
            ("value", LegacyFieldType::Fixed, false),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
        ],
    ),
    (
        &["FundingRateUpdate"],
        &[
            ("rate", LegacyFieldType::Binary, false),
            ("interval", LegacyFieldType::UInt16, true),
            ("next_funding_ns", LegacyFieldType::UInt64, true),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
        ],
    ),
    (
        &["InstrumentStatus"],
        &[
            ("instrument_id", LegacyFieldType::Utf8, true),
            ("action", LegacyFieldType::Utf8, true),
            ("reason", LegacyFieldType::Utf8, true),
            ("trading_event", LegacyFieldType::Utf8, true),
            ("is_trading", LegacyFieldType::Boolean, true),
            ("is_quoting", LegacyFieldType::Boolean, true),
            ("is_short_sell_restricted", LegacyFieldType::Boolean, true),
            ("ts_event", LegacyFieldType::UInt64, true),
            ("ts_init", LegacyFieldType::UInt64, true),
        ],
    ),
    (
        &["OptionGreeks"],
        &[
            ("instrument_id", LegacyFieldType::Utf8, false),
            ("delta", LegacyFieldType::Float64, false),
            ("gamma", LegacyFieldType::Float64, false),
            ("vega", LegacyFieldType::Float64, false),
            ("theta", LegacyFieldType::Float64, false),
            ("rho", LegacyFieldType::Float64, false),
            ("mark_iv", LegacyFieldType::Float64, true),
            ("bid_iv", LegacyFieldType::Float64, true),
            ("ask_iv", LegacyFieldType::Float64, true),
            ("underlying_price", LegacyFieldType::Float64, true),
            ("open_interest", LegacyFieldType::Float64, true),
            ("ts_event", LegacyFieldType::UInt64, false),
            ("ts_init", LegacyFieldType::UInt64, false),
            ("convention", LegacyFieldType::Utf8, false),
        ],
    ),
];

/// Returns whether a schema is a recognized Nautilus legacy family.
#[must_use]
pub fn is_nautilus_legacy_schema(schema: &Schema) -> bool {
    if let Some(type_name) = schema
        .metadata()
        .get("type_name")
        .or_else(|| schema.metadata().get("type"))
    {
        return legacy_metadata_family_matches(schema, type_name);
    }

    LEGACY_SCHEMA_FAMILIES
        .iter()
        .any(|(_, fields)| schema_fingerprint_matches(schema, fields))
        || legacy_flat_depth_fingerprint_matches(schema)
        || legacy_fixed_list_depth_fingerprint_matches(schema)
}

/// Returns whether timestamp annotations belong to a recognized Nautilus schema.
#[must_use]
pub fn is_nautilus_timestamp_schema(schema: &Schema) -> bool {
    if schema.metadata().contains_key("type_name") || schema.metadata().contains_key("type") {
        return true;
    }

    if is_nautilus_legacy_schema(schema) {
        return true;
    }
    let fields = schema
        .fields()
        .iter()
        .filter(|field| field.name() != KEY_IDENTIFIER)
        .collect::<Vec<_>>();
    LEGACY_SCHEMA_FAMILIES.iter().any(|(_, expected)| {
        fields.len() == expected.len()
            && fields
                .iter()
                .zip(*expected)
                .all(|(field, (name, expected, _))| {
                    field.name() == *name
                        && (legacy_field_type_matches(field.data_type(), *expected)
                            || (is_timestamp_field(name)
                                && matches!(
                                    field.data_type(),
                                    DataType::Timestamp(TimeUnit::Nanosecond, _)
                                ))
                            || (matches!(expected, LegacyFieldType::Fixed)
                                && field.data_type() == &fixed_decimal_data_type())
                            || (is_legacy_enum_field(name)
                                && field.data_type() == &enum_dictionary_data_type()))
                })
    })
}

/// Adds the UTC annotation to a nanosecond instant without a timezone.
#[must_use]
pub fn normalized_timestamp_type(data_type: &DataType) -> DataType {
    match data_type {
        DataType::Timestamp(TimeUnit::Nanosecond, None) => timestamp_data_type(),
        _ => data_type.clone(),
    }
}

fn legacy_metadata_family_matches(schema: &Schema, type_name: &str) -> bool {
    match type_name {
        "OrderBookDepth10" | "OrderBookDepth" => {
            return legacy_flat_depth_fingerprint_matches(schema)
                || legacy_fixed_list_depth_fingerprint_matches(schema);
        }
        _ => {}
    }

    LEGACY_SCHEMA_FAMILIES
        .iter()
        .filter(|(names, _)| names.contains(&type_name))
        .any(|(_, fields)| schema_fingerprint_matches(schema, fields))
}

#[derive(Clone, Copy)]
enum LegacyFieldType {
    Binary,
    Float64,
    Fixed,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Utf8,
    Boolean,
}

fn schema_fingerprint_matches(schema: &Schema, expected: &[(&str, LegacyFieldType, bool)]) -> bool {
    let fields = schema
        .fields()
        .iter()
        .filter(|field| field.name() != KEY_IDENTIFIER)
        .collect::<Vec<_>>();

    let fields_match = fields.len() == expected.len()
        && fields
            .iter()
            .zip(expected)
            .all(|(field, (name, data_type, _))| {
                field.name() == *name && legacy_field_type_matches(field.data_type(), *data_type)
            });
    let nullability_matches = fields
        .iter()
        .zip(expected)
        .all(|(field, (_, _, nullable))| field.is_nullable() == *nullable)
        || fields.iter().all(|field| field.is_nullable());

    fields_match
        && nullability_matches
        && fixed_width_is_consistent(fields.iter().map(|field| field.data_type()))
}

fn legacy_field_type_matches(data_type: &DataType, expected: LegacyFieldType) -> bool {
    match expected {
        LegacyFieldType::Binary => data_type == &DataType::Binary,
        LegacyFieldType::Float64 => data_type == &DataType::Float64,
        LegacyFieldType::Fixed => matches!(data_type, DataType::FixedSizeBinary(8 | 16)),
        LegacyFieldType::UInt8 => data_type == &DataType::UInt8,
        LegacyFieldType::UInt16 => data_type == &DataType::UInt16,
        LegacyFieldType::UInt32 => data_type == &DataType::UInt32,
        LegacyFieldType::UInt64 => data_type == &DataType::UInt64,
        LegacyFieldType::Utf8 => match data_type {
            DataType::Utf8 | DataType::Utf8View => true,
            DataType::Dictionary(_, value) => value.as_ref() == &DataType::Utf8,
            _ => false,
        },
        LegacyFieldType::Boolean => data_type == &DataType::Boolean,
    }
}

fn fixed_width_is_consistent<'a>(data_types: impl Iterator<Item = &'a DataType>) -> bool {
    let mut width = None;
    data_types
        .filter_map(|data_type| match data_type {
            DataType::FixedSizeBinary(width) => Some(*width),
            _ => None,
        })
        .all(|current| *width.get_or_insert(current) == current)
}

fn legacy_flat_depth_fingerprint_matches(schema: &Schema) -> bool {
    let fields = schema
        .fields()
        .iter()
        .filter(|field| field.name() != KEY_IDENTIFIER)
        .collect::<Vec<_>>();
    let market_fields = fields
        .iter()
        .filter(|field| {
            matches!(
                field.name().as_str(),
                "flags" | "sequence" | "ts_event" | "ts_init"
            )
        })
        .count();

    let order_id_fields = fields
        .iter()
        .filter(|field| {
            field.name().starts_with("bid_order_id_") || field.name().starts_with("ask_order_id_")
        })
        .count();
    let all_nullable = fields.iter().all(|field| field.is_nullable());

    fields.len() == 6 * DEPTH10_LEN + order_id_fields + market_fields
        && market_fields <= 4
        && matches!(order_id_fields, 0 | 20)
        && fixed_width_is_consistent(fields.iter().map(|field| field.data_type()))
        && ["bid", "ask"].iter().all(|side| {
            (0..DEPTH10_LEN).all(|level| {
                [
                    ("price", LegacyFieldType::Fixed),
                    ("size", LegacyFieldType::Fixed),
                    ("count", LegacyFieldType::UInt32),
                ]
                .iter()
                .all(|(value, data_type)| {
                    schema
                        .field_with_name(&format!("{side}_{value}_{level}"))
                        .is_ok_and(|field| {
                            (all_nullable || !field.is_nullable())
                                && legacy_field_type_matches(field.data_type(), *data_type)
                        })
                })
            })
        })
        && (order_id_fields == 0
            || ["bid", "ask"].iter().all(|side| {
                (0..DEPTH10_LEN).all(|level| {
                    schema
                        .field_with_name(&format!("{side}_order_id_{level}"))
                        .is_ok_and(|field| {
                            (all_nullable || !field.is_nullable())
                                && legacy_field_type_matches(
                                    field.data_type(),
                                    LegacyFieldType::UInt64,
                                )
                        })
                })
            }))
        && [
            ("flags", LegacyFieldType::UInt8),
            ("sequence", LegacyFieldType::UInt64),
            ("ts_event", LegacyFieldType::UInt64),
            ("ts_init", LegacyFieldType::UInt64),
        ]
        .iter()
        .all(|(name, data_type)| {
            let Ok(field) = schema.field_with_name(name) else {
                return true;
            };
            (all_nullable || !field.is_nullable())
                && legacy_field_type_matches(field.data_type(), *data_type)
        })
}

fn legacy_fixed_list_depth_fingerprint_matches(schema: &Schema) -> bool {
    let fixed_list_matches = |name: &str, expected: &DataType| {
        schema.field_with_name(name).is_ok_and(|field| {
            matches!(
                field.data_type(),
                DataType::FixedSizeList(item, length)
                    if *length == i32::try_from(DEPTH10_LEN).expect("depth length fits i32")
                        && item.data_type() == expected
            )
        })
    };
    let widths = ["bid_price", "ask_price", "bid_size", "ask_size"]
        .iter()
        .filter_map(|name| {
            let field = schema.field_with_name(name).ok()?;
            let DataType::FixedSizeList(item, _) = field.data_type() else {
                return None;
            };
            let DataType::FixedSizeBinary(width @ (8 | 16)) = item.data_type() else {
                return None;
            };
            Some(*width)
        })
        .collect::<Vec<_>>();
    let order_id_fields = ["bid_order_id", "ask_order_id"]
        .iter()
        .filter(|name| schema.field_with_name(name).is_ok())
        .count();

    widths.len() == 4
        && widths.iter().all(|width| *width == widths[0])
        && ["bid_count", "ask_count"]
            .iter()
            .all(|name| fixed_list_matches(name, &DataType::UInt32))
        && matches!(order_id_fields, 0 | 2)
        && (order_id_fields == 0
            || ["bid_order_id", "ask_order_id"]
                .iter()
                .all(|name| fixed_list_matches(name, &DataType::UInt64)))
}

/// Returns whether `name` identifies a legacy catalog timestamp field.
#[must_use]
pub fn is_timestamp_field(name: &str) -> bool {
    name.starts_with("ts_")
        || matches!(
            name,
            "activation_ns"
                | "expiration_ns"
                | "next_funding_ns"
                | "expire_time"
                | "event_open_date"
                | "market_start_time"
        )
}

/// Returns whether `name` identifies a legacy catalog enum field.
#[must_use]
pub fn is_legacy_enum_field(name: &str) -> bool {
    matches!(name, "action" | "side" | "aggressor_side" | "close_type")
}

fn legacy_enum_name(name: &str, value: u8) -> Result<String, ArrowError> {
    let name_value = match name {
        "action" => BookAction::from_u8(value).map(|value| value.to_string()),
        "side" => match value {
            0 => Some("NO_ORDER_SIDE".to_string()),
            1 => Some(OrderSide::Buy.to_string()),
            2 => Some(OrderSide::Sell.to_string()),
            _ => None,
        },
        "aggressor_side" => AggressorSide::from_u8(value).map(|value| value.to_string()),
        "close_type" => InstrumentCloseType::from_u8(value).map(|value| value.to_string()),
        _ => None,
    };
    name_value.ok_or_else(|| {
        ArrowError::CastError(format!(
            "Invalid legacy enum value {value} for column '{name}'"
        ))
    })
}

// Exact field inventory emitted by the legacy fixed-point schemas.
const LEGACY_QUANTITY_FIELDS: &[&str] = &[
    "size",
    "quantity",
    "qty",
    "volume",
    "bid_size",
    "ask_size",
    "bid_size_0",
    "bid_size_1",
    "bid_size_2",
    "bid_size_3",
    "bid_size_4",
    "bid_size_5",
    "bid_size_6",
    "bid_size_7",
    "bid_size_8",
    "bid_size_9",
    "ask_size_0",
    "ask_size_1",
    "ask_size_2",
    "ask_size_3",
    "ask_size_4",
    "ask_size_5",
    "ask_size_6",
    "ask_size_7",
    "ask_size_8",
    "ask_size_9",
];

fn is_legacy_quantity_field(name: &str) -> bool {
    LEGACY_QUANTITY_FIELDS.contains(&name)
}

/// Encodes a model price raw value at the catalog's uniform decimal scale.
///
/// # Errors
///
/// Returns an [`ArrowError::InvalidArgumentError`] if `raw` is [`PRICE_ERROR`].
pub fn price_raw_to_decimal(
    raw: PriceRaw,
    field: &'static str,
) -> Result<Option<i128>, ArrowError> {
    if raw == PRICE_UNDEF {
        return Ok(None);
    }

    if raw == PRICE_ERROR {
        return Err(ArrowError::InvalidArgumentError(format!(
            "Price field '{field}' contains PRICE_ERROR raw value {raw}"
        )));
    }

    #[allow(
        clippy::useless_conversion,
        reason = "PriceRaw is i64 or i128 depending on model feature unification"
    )]
    let mut decimal = i128::from(raw);
    if FIXED_PRECISION == FIXED_PRECISION_STANDARD {
        decimal *= STANDARD_TO_DECIMAL_SCALE;
    }

    Ok(Some(decimal))
}

/// Encodes a model quantity raw value at the catalog's uniform decimal scale.
///
/// # Errors
///
/// Returns an [`ArrowError::InvalidArgumentError`] if a non-sentinel quantity does not fit in
/// Arrow's signed decimal representation.
pub fn quantity_raw_to_decimal(
    raw: QuantityRaw,
    field: &'static str,
) -> Result<Option<i128>, ArrowError> {
    if raw == QUANTITY_UNDEF {
        return Ok(None);
    }

    if raw > QUANTITY_RAW_MAX {
        return Err(ArrowError::InvalidArgumentError(format!(
            "Quantity field '{field}' raw value {raw} exceeds QUANTITY_RAW_MAX={QUANTITY_RAW_MAX}"
        )));
    }

    #[allow(
        clippy::unnecessary_fallible_conversions,
        reason = "QuantityRaw is u64 or u128 depending on model feature unification"
    )]
    let mut decimal = i128::try_from(raw).map_err(|_| {
        ArrowError::InvalidArgumentError(format!(
            "Quantity field '{field}' raw value {raw} exceeds Decimal128 range"
        ))
    })?;

    if FIXED_PRECISION == FIXED_PRECISION_STANDARD {
        decimal *= STANDARD_TO_DECIMAL_SCALE;
    }

    Ok(Some(decimal))
}

/// Builds a scale-16 decimal array from model price raw values.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the values do not fit the declared decimal type.
pub fn price_decimal_array(
    values: impl IntoIterator<Item = PriceRaw>,
    field: &'static str,
) -> Result<Decimal128Array, ArrowError> {
    let values = values
        .into_iter()
        .map(|raw| price_raw_to_decimal(raw, field))
        .collect::<Result<Vec<_>, _>>()?;
    Decimal128Array::from(values)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
}

/// Builds a scale-16 decimal array from model quantity raw values.
///
/// # Errors
///
/// Returns an [`ArrowError`] if a value does not fit or the declared decimal type is invalid.
pub fn quantity_decimal_array(
    values: impl IntoIterator<Item = QuantityRaw>,
    field: &'static str,
) -> Result<Decimal128Array, ArrowError> {
    let values = values
        .into_iter()
        .map(|raw| quantity_raw_to_decimal(raw, field))
        .collect::<Result<Vec<_>, _>>()?;
    Decimal128Array::from(values)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
}

/// Builds a scale-16 decimal array from price raw values of a type with no NULL sentinel.
///
/// # Errors
///
/// Returns an [`ArrowError`] if a value is `PRICE_UNDEF` or does not fit the declared decimal
/// type, so the required decoders can read back every written value.
pub fn required_price_decimal_array(
    values: impl IntoIterator<Item = PriceRaw>,
    field: &'static str,
) -> Result<Decimal128Array, ArrowError> {
    let values = values
        .into_iter()
        .map(|raw| {
            price_raw_to_decimal(raw, field)?.ok_or_else(|| {
                ArrowError::InvalidArgumentError(format!(
                    "Price field '{field}' contains PRICE_UNDEF, which has no sentinel encoding for this type"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Decimal128Array::from(values)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
}

/// Builds a scale-16 decimal array from quantity raw values of a type with no NULL sentinel.
///
/// # Errors
///
/// Returns an [`ArrowError`] if a value is `QUANTITY_UNDEF` or does not fit the declared decimal
/// type, so the required decoders can read back every written value.
pub fn required_quantity_decimal_array(
    values: impl IntoIterator<Item = QuantityRaw>,
    field: &'static str,
) -> Result<Decimal128Array, ArrowError> {
    let values = values
        .into_iter()
        .map(|raw| {
            quantity_raw_to_decimal(raw, field)?.ok_or_else(|| {
                ArrowError::InvalidArgumentError(format!(
                    "Quantity field '{field}' contains QUANTITY_UNDEF, which has no sentinel encoding for this type"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Decimal128Array::from(values)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
}

fn decimal_to_price_raw(
    value: i128,
    field: &'static str,
    row: usize,
) -> Result<PriceRaw, EncodingError> {
    decimal_to_raw(value, field, row, "PriceRaw")
}

fn decimal_to_quantity_raw(
    value: i128,
    field: &'static str,
    row: usize,
) -> Result<QuantityRaw, EncodingError> {
    decimal_to_raw(value, field, row, "QuantityRaw")
}

fn decimal_to_raw<T: TryFrom<i128>>(
    value: i128,
    field: &'static str,
    row: usize,
    raw_type: &'static str,
) -> Result<T, EncodingError> {
    let raw_value = if FIXED_PRECISION == FIXED_PRECISION_STANDARD {
        if value % STANDARD_TO_DECIMAL_SCALE != 0 {
            return Err(EncodingError::ParseError(
                field,
                format!(
                    "row {row}: decimal value {value} has nonzero digits beyond build precision 9"
                ),
            ));
        }
        value / STANDARD_TO_DECIMAL_SCALE
    } else {
        value
    };

    T::try_from(raw_value).map_err(|_| {
        EncodingError::ParseError(
            field,
            format!("row {row}: decimal value {value} exceeds {raw_type} range"),
        )
    })
}

/// Decodes a price from a nullable scale-16 decimal column.
///
/// # Errors
///
/// Returns an [`EncodingError`] if the value cannot be represented by this build.
pub fn decode_decimal_price(
    values: &Decimal128Array,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Price, EncodingError> {
    if values.is_null(row) {
        return Price::from_raw_checked(PRICE_UNDEF, 0)
            .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")));
    }

    let raw = decimal_to_price_raw(values.value(row), field, row)?;
    Price::from_raw_checked(raw, precision)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

/// Decodes a required price from a scale-16 decimal column.
///
/// # Errors
///
/// Returns an [`EncodingError`] if the value is null or cannot be represented by this build.
pub fn decode_required_decimal_price(
    values: &Decimal128Array,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Price, EncodingError> {
    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required price is null"),
        ));
    }
    decode_decimal_price(values, precision, field, row)
}

/// Decodes a quantity from a nullable scale-16 decimal column.
///
/// # Errors
///
/// Returns an [`EncodingError`] if the value cannot be represented by this build.
pub fn decode_decimal_quantity(
    values: &Decimal128Array,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Quantity, EncodingError> {
    if values.is_null(row) {
        return Quantity::from_raw_checked(QUANTITY_UNDEF, 0)
            .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")));
    }

    let raw = decimal_to_quantity_raw(values.value(row), field, row)?;
    Quantity::from_raw_checked(raw, precision)
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
}

/// Decodes a required quantity from a scale-16 decimal column.
///
/// # Errors
///
/// Returns an [`EncodingError`] if the value is null or cannot be represented by this build.
pub fn decode_required_decimal_quantity(
    values: &Decimal128Array,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<Quantity, EncodingError> {
    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required quantity is null"),
        ));
    }
    decode_decimal_quantity(values, precision, field, row)
}

/// Returns a required timestamp value, naming the field and row on NULL.
///
/// # Errors
///
/// Returns an [`EncodingError`] if the value is null.
pub fn decode_required_timestamp(
    values: &UInt64Array,
    field: &'static str,
    row: usize,
) -> Result<UnixNanos, EncodingError> {
    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required timestamp is null"),
        ));
    }
    Ok(values.value(row).into())
}

pub(crate) fn decode_required_u64(
    values: &UInt64Array,
    field: &'static str,
    row: usize,
) -> Result<u64, EncodingError> {
    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required integer is null"),
        ));
    }
    Ok(values.value(row))
}

pub(crate) fn decode_required_u8(
    values: &UInt8Array,
    field: &'static str,
    row: usize,
) -> Result<u8, EncodingError> {
    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required integer is null"),
        ));
    }
    Ok(values.value(row))
}

#[cfg(test)]
trait PriceRawSource {
    fn raw_price(self) -> PriceRaw;
}

#[cfg(test)]
impl PriceRawSource for &[u8] {
    fn raw_price(self) -> PriceRaw {
        PriceRaw::from_le_bytes(
            self.try_into()
                .expect("Price raw bytes must be exactly the size of PriceRaw"),
        )
    }
}

#[cfg(test)]
impl PriceRawSource for i128 {
    fn raw_price(self) -> PriceRaw {
        decimal_to_price_raw(self, "test", 0).expect("Decimal price must fit the current build")
    }
}

#[inline]
#[cfg(test)]
fn get_raw_price(value: impl PriceRawSource) -> PriceRaw {
    value.raw_price()
}

#[inline]
#[cfg(not(test))]
fn get_raw_price(value: &[u8]) -> PriceRaw {
    PriceRaw::from_le_bytes(
        value
            .try_into()
            .expect("Price raw bytes must be exactly the size of PriceRaw"),
    )
}

#[cfg(test)]
trait QuantityRawSource {
    fn raw_quantity(self) -> QuantityRaw;
}

#[cfg(test)]
impl QuantityRawSource for &[u8] {
    fn raw_quantity(self) -> QuantityRaw {
        QuantityRaw::from_le_bytes(
            self.try_into()
                .expect("Quantity raw bytes must be exactly the size of QuantityRaw"),
        )
    }
}

#[cfg(test)]
impl QuantityRawSource for i128 {
    fn raw_quantity(self) -> QuantityRaw {
        decimal_to_quantity_raw(self, "test", 0)
            .expect("Decimal quantity must fit the current build")
    }
}

#[inline]
#[cfg(test)]
fn get_raw_quantity(value: impl QuantityRawSource) -> QuantityRaw {
    value.raw_quantity()
}

#[inline]
#[cfg(not(test))]
fn get_raw_quantity(value: &[u8]) -> QuantityRaw {
    QuantityRaw::from_le_bytes(
        value
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
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: Borrow<Self>;

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
    fn chunk_metadata<T>(chunk: &[T]) -> HashMap<String, String>
    where
        T: Borrow<Self>,
    {
        chunk
            .first()
            .map(|item| item.borrow().metadata())
            .expect("Chunk must contain at least one element to encode")
    }

    /// Returns whether this element is compatible with metadata selected for its chunk.
    fn matches_chunk_metadata(&self, metadata: &HashMap<String, String>) -> bool {
        self.metadata() == *metadata
    }
}

/// Returns the catalog row identifier from Arrow schema metadata.
///
/// Bars use `bar_type`; all other built-in catalog types use `instrument_id`.
/// Custom data can pass an explicit identifier to [`record_batch_with_identifier_column`].
#[must_use]
pub fn catalog_identifier_from_metadata(metadata: &HashMap<String, String>) -> Option<String> {
    metadata
        .get(KEY_BAR_TYPE)
        .cloned()
        .or_else(|| metadata.get(KEY_INSTRUMENT_ID).cloned())
}

/// Builds a schema with the catalog `identifier` column appended if absent.
#[must_use]
pub fn schema_with_identifier_column(schema: &Schema) -> Schema {
    if schema.index_of(KEY_IDENTIFIER).is_ok() {
        return schema.clone();
    }

    let mut fields = schema.fields().iter().cloned().collect::<Vec<_>>();
    fields.push(Arc::new(Field::new(KEY_IDENTIFIER, DataType::Utf8, true)));

    Schema::new_with_metadata(fields, schema.metadata().clone())
}

/// Builds a schema without the catalog `identifier` column.
#[must_use]
pub fn schema_without_identifier_column(schema: &Schema) -> Schema {
    let Ok(identifier_index) = schema.index_of(KEY_IDENTIFIER) else {
        return schema.clone();
    };

    let fields = schema
        .fields()
        .iter()
        .enumerate()
        .filter_map(|(index, field)| (index != identifier_index).then_some(field.clone()))
        .collect::<Vec<_>>();

    Schema::new_with_metadata(fields, schema.metadata().clone())
}

/// Adds the catalog `identifier` column used by table-oriented catalog storage.
///
/// The column is nullable so custom data without a `DataType.identifier()` can
/// still be stored in the same type table.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the record batch cannot be rebuilt.
pub fn record_batch_with_identifier_column(
    batch: RecordBatch,
    identifier: Option<&str>,
) -> Result<RecordBatch, ArrowError> {
    let identifier_values = vec![identifier.map(ToString::to_string); batch.num_rows()];
    record_batch_with_identifier_values(batch, identifier_values)
}

/// Adds the catalog `identifier` column with one identifier value per row.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the number of identifier values differs from
/// the record batch row count or the record batch cannot be rebuilt.
pub fn record_batch_with_identifier_values(
    batch: RecordBatch,
    identifier_values: Vec<Option<String>>,
) -> Result<RecordBatch, ArrowError> {
    if batch.schema().index_of(KEY_IDENTIFIER).is_ok() {
        return Ok(batch);
    }

    if identifier_values.len() != batch.num_rows() {
        return Err(ArrowError::InvalidArgumentError(format!(
            "identifier values length {} does not match record batch row count {}",
            identifier_values.len(),
            batch.num_rows()
        )));
    }

    let schema = schema_with_identifier_column(batch.schema().as_ref());
    let mut columns = batch.columns().to_vec();
    columns.push(Arc::new(StringArray::from(identifier_values)));

    RecordBatch::try_new(Arc::new(schema), columns)
}

/// Builds a string array for catalog identifiers without collecting intermediate strings.
#[must_use]
pub fn identifier_array_from_display<T: Display>(
    identifiers: impl IntoIterator<Item = T>,
) -> StringArray {
    let mut builder = StringBuilder::new();
    let mut scratch = String::new();

    for identifier in identifiers {
        scratch.clear();
        write!(&mut scratch, "{identifier}").expect("writing to String should not fail");
        builder.append_value(scratch.as_str());
    }

    builder.finish()
}

/// Drops the catalog `identifier` column when writing legacy per-identifier formats.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the record batch cannot be rebuilt.
pub fn record_batch_without_identifier_column(
    mut batch: RecordBatch,
) -> Result<RecordBatch, ArrowError> {
    let Ok(identifier_index) = batch.schema().index_of(KEY_IDENTIFIER) else {
        return Ok(batch);
    };

    batch.remove_column(identifier_index);
    Ok(batch)
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
    StringColumnRef::try_from_array(column_values.as_ref()).ok_or_else(|| {
        EncodingError::InvalidColumnType(
            column_key,
            column_index,
            DataType::Utf8,
            column_values.data_type().clone(),
        )
    })
}

/// Reference to a string column in a supported Arrow string representation.
#[derive(Debug)]
pub enum StringColumnRef<'a> {
    DictionaryInt8(&'a DictionaryArray<Int8Type>, &'a StringArray),
    DictionaryInt32(&'a DictionaryArray<Int32Type>, &'a StringArray),
    Utf8(&'a StringArray),
    Utf8View(&'a StringViewArray),
}

impl StringColumnRef<'_> {
    /// Returns the number of rows.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::DictionaryInt8(array, _) => array.len(),
            Self::DictionaryInt32(array, _) => array.len(),
            Self::Utf8(array) => array.len(),
            Self::Utf8View(array) => array.len(),
        }
    }

    /// Returns whether the column contains no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a string view when `array` uses `Utf8` or `Utf8View` encoding.
    #[must_use]
    pub fn try_from_array(array: &dyn Array) -> Option<StringColumnRef<'_>> {
        if let Some(array) = array.as_any().downcast_ref::<DictionaryArray<Int8Type>>()
            && let Some(values) = array.values().as_any().downcast_ref::<StringArray>()
        {
            return Some(StringColumnRef::DictionaryInt8(array, values));
        }

        if let Some(array) = array.as_any().downcast_ref::<DictionaryArray<Int32Type>>()
            && let Some(values) = array.values().as_any().downcast_ref::<StringArray>()
        {
            return Some(StringColumnRef::DictionaryInt32(array, values));
        }

        if let Some(array) = array.as_any().downcast_ref::<StringArray>() {
            return Some(StringColumnRef::Utf8(array));
        }

        array
            .as_any()
            .downcast_ref::<StringViewArray>()
            .map(StringColumnRef::Utf8View)
    }

    /// Returns the string value at row `i`.
    ///
    /// # Panics
    ///
    /// Panics if `i` is out of bounds or a dictionary column contains an invalid key.
    #[inline]
    #[must_use]
    pub fn value(&self, i: usize) -> &str {
        match self {
            Self::DictionaryInt8(array, values) => {
                let key = usize::try_from(array.keys().value(i)).expect("Int8 key is non-negative");
                values.value(key)
            }
            Self::DictionaryInt32(array, values) => {
                let key =
                    usize::try_from(array.keys().value(i)).expect("Int32 key is non-negative");
                values.value(key)
            }
            Self::Utf8(arr) => arr.value(i),
            Self::Utf8View(arr) => arr.value(i),
        }
    }

    /// Returns whether the value at row `i` is null.
    #[inline]
    #[must_use]
    pub fn is_null(&self, i: usize) -> bool {
        match self {
            Self::DictionaryInt8(array, _) => array.is_null(i),
            Self::DictionaryInt32(array, _) => array.is_null(i),
            Self::Utf8(arr) => arr.is_null(i),
            Self::Utf8View(arr) => arr.is_null(i),
        }
    }

    /// Returns the string value at row `i`, or `None` when it is null.
    #[must_use]
    pub fn value_opt(&self, i: usize) -> Option<&str> {
        (!self.is_null(i)).then(|| self.value(i))
    }
}

/// Reference to an unsigned 64-bit value stored as an integer or UTC nanosecond timestamp.
#[derive(Debug)]
pub enum U64ColumnRef<'a> {
    UInt64(&'a UInt64Array),
    Int64(&'a Int64Array),
    TimestampNanosecond(&'a TimestampNanosecondArray),
}

impl U64ColumnRef<'_> {
    /// Returns a compatible unsigned 64-bit view for `array`.
    #[must_use]
    pub fn try_from_array(array: &dyn Array) -> Option<U64ColumnRef<'_>> {
        if let Some(array) = array.as_any().downcast_ref::<UInt64Array>() {
            return Some(U64ColumnRef::UInt64(array));
        }

        if let Some(array) = array.as_any().downcast_ref::<Int64Array>() {
            return Some(U64ColumnRef::Int64(array));
        }

        array
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .map(U64ColumnRef::TimestampNanosecond)
    }

    /// Returns the number of values in the column.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::UInt64(array) => array.len(),
            Self::Int64(array) => array.len(),
            Self::TimestampNanosecond(array) => array.len(),
        }
    }

    /// Returns whether the column has no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns whether the value at row `i` is null.
    #[must_use]
    pub fn is_null(&self, i: usize) -> bool {
        match self {
            Self::UInt64(array) => array.is_null(i),
            Self::Int64(array) => array.is_null(i),
            Self::TimestampNanosecond(array) => array.is_null(i),
        }
    }

    /// Returns the value at row `i`, or `None` when a signed representation is negative.
    #[must_use]
    pub fn value(&self, i: usize) -> Option<u64> {
        match self {
            Self::UInt64(array) => Some(array.value(i)),
            Self::Int64(array) => u64::try_from(array.value(i)).ok(),
            Self::TimestampNanosecond(array) => u64::try_from(array.value(i)).ok(),
        }
    }
}

/// Reference to an unsigned 32-bit column stored as `UInt32`, `Int32`, or `Int64`.
#[derive(Debug)]
pub enum U32ColumnRef<'a> {
    UInt32(&'a UInt32Array),
    Int32(&'a Int32Array),
    Int64(&'a Int64Array),
}

impl U32ColumnRef<'_> {
    /// Returns a compatible unsigned 32-bit view for `array`.
    #[must_use]
    pub fn try_from_array(array: &dyn Array) -> Option<U32ColumnRef<'_>> {
        if let Some(array) = array.as_any().downcast_ref::<UInt32Array>() {
            return Some(U32ColumnRef::UInt32(array));
        }

        if let Some(array) = array.as_any().downcast_ref::<Int32Array>() {
            return Some(U32ColumnRef::Int32(array));
        }

        array
            .as_any()
            .downcast_ref::<Int64Array>()
            .map(U32ColumnRef::Int64)
    }
}

/// Extracts a binary column, accepting both Binary (`BinaryArray`) and BinaryView
/// (`BinaryViewArray`).
/// DataFusion may return BinaryView when reading Parquet, so this handles both formats.
///
/// # Errors
///
/// Returns an error if:
/// - `column_index` is out of range: `EncodingError::MissingColumn`.
/// - The column type is neither Binary nor BinaryView: `EncodingError::InvalidColumnType`.
pub fn extract_column_binary<'a>(
    cols: &'a [ArrayRef],
    column_key: &'static str,
    column_index: usize,
) -> Result<BinaryColumnRef<'a>, EncodingError> {
    let column_values = cols
        .get(column_index)
        .ok_or(EncodingError::MissingColumn(column_key, column_index))?;
    let dt = column_values.data_type();
    if let Some(arr) = column_values.as_any().downcast_ref::<BinaryArray>() {
        Ok(BinaryColumnRef::Binary(arr))
    } else if let Some(arr) = column_values.as_any().downcast_ref::<BinaryViewArray>() {
        Ok(BinaryColumnRef::BinaryView(arr))
    } else {
        Err(EncodingError::InvalidColumnType(
            column_key,
            column_index,
            DataType::Binary,
            dt.clone(),
        ))
    }
}

/// Reference to a binary column, either Binary or BinaryView.
#[derive(Debug)]
pub enum BinaryColumnRef<'a> {
    Binary(&'a BinaryArray),
    BinaryView(&'a BinaryViewArray),
}

impl BinaryColumnRef<'_> {
    /// Returns whether the row contains a null value.
    #[inline]
    #[must_use]
    pub fn is_null(&self, i: usize) -> bool {
        match self {
            Self::Binary(arr) => arr.is_null(i),
            Self::BinaryView(arr) => arr.is_null(i),
        }
    }

    /// Returns the bytes at row `i`.
    #[inline]
    #[must_use]
    pub fn value(&self, i: usize) -> &[u8] {
        match self {
            Self::Binary(arr) => arr.value(i),
            Self::BinaryView(arr) => arr.value(i),
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

/// Extracts a decimal column by its schema name.
///
/// # Errors
///
/// Returns an error if the named column is missing or is not `Decimal128(38, 16)`.
pub fn extract_decimal_column<'a>(
    record_batch: &'a RecordBatch,
    column_key: &'static str,
) -> Result<&'a Decimal128Array, EncodingError> {
    let column_index = record_batch.schema().index_of(column_key)?;
    let expected = fixed_decimal_data_type();
    let array: &Decimal128Array = extract_column(
        record_batch.columns(),
        column_key,
        column_index,
        expected.clone(),
    )?;

    if array.data_type() != &expected {
        return Err(EncodingError::InvalidColumnType(
            column_key,
            column_index,
            expected,
            array.data_type().clone(),
        ));
    }
    Ok(array)
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

/// Converts a vector of `OrderBookDepth` into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if:
/// - `data` is empty: `EncodingError::EmptyData`.
/// - Metadata differs between rows: `EncodingError::MixedMetadata`.
/// - Encoding fails: `EncodingError::ArrowError`.
pub fn book_depths_to_arrow_record_batch_bytes(
    data: &[OrderBookDepth],
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }
    let metadata = OrderBookDepth::chunk_metadata(data);

    if let Some(index) = data
        .iter()
        .position(|depth| !depth.matches_chunk_metadata(&metadata))
    {
        return Err(EncodingError::MixedMetadata { index });
    }

    OrderBookDepth::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
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
            Bar, BarSpecification, BarType, BookOrder, OrderBookDelta, OrderBookDepth, QuoteTick,
            order::NULL_ORDER,
        },
        enums::{AggregationSource, BarAggregation, BookAction, OrderSide, PriceType},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_encode_rejects_defi_precision_metadata() {
        let mut metadata = QuoteTick::get_metadata(
            &InstrumentId::from("WETH-USDC.UNISWAP"),
            FIXED_DECIMAL_SCALE as u8,
            0,
        );
        metadata.insert(KEY_PRICE_PRECISION.to_string(), "17".to_string());
        let schema = Arc::new(QuoteTick::get_schema(Some(metadata)));

        let err = record_batch_with_timestamps(schema, vec![]).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Invalid argument error: Metadata 'price_precision' is 17, maximum supported catalog scale is 16"
        );
    }

    #[rstest]
    fn test_timestamp_arrays_preserve_utc_nanoseconds_and_nulls() {
        let values = [Some(1_788_652_800_123_456_789), None, Some(i64::MAX as u64)];
        let array = optional_timestamp_array(values).unwrap();

        assert_eq!(
            timestamp_data_type(),
            DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))
        );
        assert_eq!(array.data_type(), &timestamp_data_type());
        assert_eq!(
            array.iter().collect::<Vec<_>>(),
            vec![Some(1_788_652_800_123_456_789), None, Some(i64::MAX)]
        );
        assert_eq!(
            decode_timestamp(&array, "ts_event", 0).unwrap(),
            1_788_652_800_123_456_789
        );
        assert_eq!(
            decode_timestamp(&array, "ts_event", 2).unwrap(),
            i64::MAX as u64
        );
        assert!(
            decode_timestamp(&TimestampNanosecondArray::from(vec![-1]), "ts_event", 0).is_err()
        );
    }

    #[rstest]
    fn test_timestamp_array_rejects_unsigned_overflow() {
        let error = timestamp_array([u64::MAX]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Invalid argument error: Nanosecond timestamp 18446744073709551615 exceeds Arrow's signed timestamp range",
        );
    }

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
    fn test_depths_to_arrow_record_batch_rejects_mixed_level_price_precision() {
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
        let depth = OrderBookDepth::new(
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

        let result = book_depths_to_arrow_record_batch_bytes(&[depth]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 0 })
        ));
    }

    #[rstest]
    fn test_depths_to_arrow_record_batch_rejects_mixed_level_size_precision() {
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
        let depth = OrderBookDepth::new(
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

        let result = book_depths_to_arrow_record_batch_bytes(&[depth]);

        assert!(matches!(
            result,
            Err(EncodingError::MixedMetadata { index: 0 })
        ));
    }

    #[rstest]
    fn test_depths_to_arrow_record_batch_uses_first_defined_level_precision() {
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
        bids[0] = NULL_ORDER;
        let depth = OrderBookDepth::new(
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

        let result = book_depths_to_arrow_record_batch_bytes(&[depth]).unwrap();

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

#[cfg(test)]
mod schema_invariant_tests {
    use std::sync::Arc;

    use arrow::{
        array::{Array, ArrayRef, Decimal128Array, FixedSizeBinaryArray, UInt8Array, UInt64Array},
        datatypes::{DataType, Field, Schema},
        record_batch::RecordBatch,
    };
    use nautilus_model::{
        data::{
            BookOrder, DEPTH10_LEN, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
            MarkPriceUpdate, OptionGreeks,
            bar::Bar,
            close::InstrumentClose,
            delta::OrderBookDelta,
            depth::OrderBookDepth,
            quote::QuoteTick,
            stubs::{stub_bar, stub_depth10},
            trade::TradeTick,
        },
        enums::{AggressorSide, BookAction, InstrumentCloseType, OrderSide},
        events::{
            AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
            OrderEmulated, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
            OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected,
            OrderReleased, OrderSnapshot, OrderSubmitted, OrderTriggered, OrderUpdated,
            PositionAdjusted, PositionChanged, PositionClosed, PositionOpened, PositionSnapshot,
        },
        identifiers::{InstrumentId, TradeId},
        instruments::{
            InstrumentAny, betting::BettingInstrument, binary_option::BinaryOption, cfd::Cfd,
            commodity::Commodity, crypto_future::CryptoFuture,
            crypto_futures_spread::CryptoFuturesSpread, crypto_option::CryptoOption,
            crypto_option_spread::CryptoOptionSpread, crypto_perpetual::CryptoPerpetual,
            currency_pair::CurrencyPair, equity::Equity, futures_contract::FuturesContract,
            futures_spread::FuturesSpread, index_instrument::IndexInstrument,
            option_contract::OptionContract, option_spread::OptionSpread,
            perpetual_contract::PerpetualContract, tokenized_asset::TokenizedAsset,
        },
        reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
        types::{PRICE_ERROR, PRICE_UNDEF, Price, QUANTITY_UNDEF, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::{
        ArrowSchemaProvider, DecodeFromRecordBatch, EncodeToRecordBatch, FIXED_DECIMAL_PRECISION,
        FIXED_DECIMAL_SCALE, KEY_IDENTIFIER, QUANTITY_RAW_MAX, StringColumnRef, decimal_to_arrow,
        decode_decimal, decode_decimal_price, fixed_decimal_data_type, is_legacy_quantity_field,
        is_nautilus_legacy_schema, is_timestamp_field, normalize_legacy_fixed_columns,
        price_decimal_array, quantity_decimal_array, timestamp_data_type,
    };

    #[derive(Clone, Copy, Debug)]
    enum FixedFamily {
        Quote,
        Trade,
        Bar,
        Delta,
        Depth,
        MarkPrice,
        IndexPrice,
        Close,
    }

    #[rstest]
    fn decimal_to_arrow_rejects_values_outside_decode_range() {
        let value = Decimal::from_i128_with_scale(8_000_000_000_000, 0);

        let error = decimal_to_arrow(&value, "amount").unwrap_err();

        assert!(error.to_string().contains("96-bit range"));
    }

    #[rstest]
    fn decimal_to_arrow_normalizes_trailing_zero_scale() {
        let value = Decimal::from_i128_with_scale(10_000_000_000_000_000, 18);
        let encoded = decimal_to_arrow(&value, "amount").unwrap();
        let array = Decimal128Array::from(vec![encoded])
            .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
            .unwrap();

        assert_eq!(
            decode_decimal(&array, "amount", 0).unwrap(),
            value.normalize()
        );
    }

    #[rstest]
    #[case("size", true)]
    #[case("volume", true)]
    #[case("bid_size_0", true)]
    #[case("ask_size_9", true)]
    #[case("price", false)]
    #[case("quantity_hint", false)]
    #[case("bid_size_10", false)]
    fn legacy_quantity_fields_use_exact_schema_names(#[case] name: &str, #[case] expected: bool) {
        assert_eq!(is_legacy_quantity_field(name), expected);
    }

    #[rstest]
    fn legacy_market_columns_normalize_enums_without_changing_flags() {
        let price = 1_i64.to_le_bytes();
        let size = 2_u64.to_le_bytes();
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("action", DataType::UInt8, false),
                Field::new("side", DataType::UInt8, false),
                Field::new("price", DataType::FixedSizeBinary(8), false),
                Field::new("size", DataType::FixedSizeBinary(8), false),
                Field::new("order_id", DataType::UInt64, false),
                Field::new("flags", DataType::UInt8, false),
                Field::new("sequence", DataType::UInt64, false),
                Field::new("ts_event", DataType::UInt64, false),
                Field::new("ts_init", DataType::UInt64, false),
            ])),
            vec![
                Arc::new(UInt8Array::from(vec![BookAction::Add as u8])),
                Arc::new(UInt8Array::from(vec![OrderSide::Buy as u8])),
                Arc::new(
                    FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                        [Some(price.as_slice())].into_iter(),
                        8,
                    )
                    .unwrap(),
                ),
                Arc::new(
                    FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                        [Some(size.as_slice())].into_iter(),
                        8,
                    )
                    .unwrap(),
                ),
                Arc::new(UInt64Array::from(vec![11])),
                Arc::new(UInt8Array::from(vec![7])),
                Arc::new(UInt64Array::from(vec![12])),
                Arc::new(UInt64Array::from(vec![13])),
                Arc::new(UInt64Array::from(vec![1])),
            ],
        )
        .unwrap();

        let normalized = normalize_legacy_fixed_columns(&batch).unwrap();
        let action =
            StringColumnRef::try_from_array(normalized.column_by_name("action").unwrap().as_ref())
                .unwrap();
        let side =
            StringColumnRef::try_from_array(normalized.column_by_name("side").unwrap().as_ref())
                .unwrap();

        assert_eq!(action.value(0), "ADD");
        assert_eq!(side.value(0), "BUY");
        assert_eq!(
            normalized.column_by_name("price").unwrap().data_type(),
            &fixed_decimal_data_type(),
        );
        assert_eq!(
            normalized.column_by_name("size").unwrap().data_type(),
            &fixed_decimal_data_type(),
        );
        assert_eq!(
            normalized.column_by_name("flags").unwrap().data_type(),
            &DataType::UInt8,
        );
        assert_eq!(
            normalized.column_by_name("ts_init").unwrap().data_type(),
            &timestamp_data_type(),
        );
    }

    #[rstest]
    fn unrelated_named_columns_are_not_retyped() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![
                Field::new("side", DataType::UInt8, false),
                Field::new("ts_recv", DataType::UInt64, false),
            ])),
            vec![
                Arc::new(UInt8Array::from(vec![127])),
                Arc::new(UInt64Array::from(vec![u64::MAX])),
            ],
        )
        .unwrap();

        let normalized = normalize_legacy_fixed_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn unrelated_fixed_binary_column_is_not_retyped() {
        let value = i64::MIN.to_le_bytes();
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "price",
                DataType::FixedSizeBinary(8),
                false,
            )])),
            vec![Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(value.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            )],
        )
        .unwrap();

        let normalized = normalize_legacy_fixed_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[rstest]
    fn legacy_family_fingerprints_cover_unpinned_schemas() {
        let fixed = DataType::FixedSizeBinary(8);
        let schemas = [
            Schema::new(vec![
                Field::new("close_price", fixed.clone(), false),
                Field::new("close_type", DataType::UInt8, false),
                Field::new("ts_event", DataType::UInt64, false),
                Field::new("ts_init", DataType::UInt64, false),
            ]),
            Schema::new(vec![
                Field::new("instrument_id", DataType::Utf8, true),
                Field::new("close_type", DataType::Utf8, true),
                Field::new("close_price", DataType::Utf8, true),
                Field::new("ts_event", DataType::UInt64, true),
                Field::new("ts_init", DataType::UInt64, true),
            ]),
            Schema::new(vec![
                Field::new("value", fixed, false),
                Field::new("ts_event", DataType::UInt64, false),
                Field::new("ts_init", DataType::UInt64, false),
            ]),
            Schema::new(vec![
                Field::new("rate", DataType::Binary, false),
                Field::new("interval", DataType::UInt16, true),
                Field::new("next_funding_ns", DataType::UInt64, true),
                Field::new("ts_event", DataType::UInt64, false),
                Field::new("ts_init", DataType::UInt64, false),
            ]),
            Schema::new(vec![
                Field::new("instrument_id", DataType::Utf8, true),
                Field::new("action", DataType::Utf8, true),
                Field::new("reason", DataType::Utf8, true),
                Field::new("trading_event", DataType::Utf8, true),
                Field::new("is_trading", DataType::Boolean, true),
                Field::new("is_quoting", DataType::Boolean, true),
                Field::new("is_short_sell_restricted", DataType::Boolean, true),
                Field::new("ts_event", DataType::UInt64, true),
                Field::new("ts_init", DataType::UInt64, true),
            ]),
        ];

        assert!(schemas.iter().all(is_nautilus_legacy_schema));
    }

    #[rstest]
    fn legacy_family_fingerprint_accepts_dictionary_string_fields() {
        let schema = Schema::new(vec![
            Field::new("price", DataType::FixedSizeBinary(8), false),
            Field::new("size", DataType::FixedSizeBinary(8), false),
            Field::new("aggressor_side", DataType::UInt8, false),
            Field::new(
                "trade_id",
                DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8)),
                false,
            ),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
            Field::new(
                KEY_IDENTIFIER,
                DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8)),
                false,
            ),
        ]);

        assert!(is_nautilus_legacy_schema(&schema));
    }

    #[rstest]
    fn legacy_flat_depth_fingerprint_accepts_all_nullable_fields() {
        let fixed = DataType::FixedSizeBinary(8);
        let mut fields = Vec::new();

        for side in ["bid", "ask"] {
            for level in 0..DEPTH10_LEN {
                fields.extend([
                    Field::new(format!("{side}_price_{level}"), fixed.clone(), true),
                    Field::new(format!("{side}_size_{level}"), fixed.clone(), true),
                    Field::new(format!("{side}_count_{level}"), DataType::UInt32, true),
                ]);
            }
        }
        fields.extend([
            Field::new("flags", DataType::UInt8, true),
            Field::new("sequence", DataType::UInt64, true),
            Field::new("ts_event", DataType::UInt64, true),
            Field::new("ts_init", DataType::UInt64, true),
        ]);
        let schema = Schema::new(fields);

        assert!(super::legacy_flat_depth_fingerprint_matches(&schema));
    }

    #[rstest]
    fn legacy_fixed_list_depth_fingerprint_accepts_missing_order_ids() {
        let fixed = Arc::new(Field::new("item", DataType::FixedSizeBinary(8), false));
        let count = Arc::new(Field::new("item", DataType::UInt32, false));
        let schema = Schema::new(vec![
            Field::new(
                "bid_price",
                DataType::FixedSizeList(fixed.clone(), 10),
                false,
            ),
            Field::new(
                "ask_price",
                DataType::FixedSizeList(fixed.clone(), 10),
                false,
            ),
            Field::new(
                "bid_size",
                DataType::FixedSizeList(fixed.clone(), 10),
                false,
            ),
            Field::new("ask_size", DataType::FixedSizeList(fixed, 10), false),
            Field::new(
                "bid_count",
                DataType::FixedSizeList(count.clone(), 10),
                false,
            ),
            Field::new("ask_count", DataType::FixedSizeList(count, 10), false),
        ]);

        assert!(is_nautilus_legacy_schema(&schema));
    }

    #[rstest]
    fn metadata_only_schema_is_not_a_legacy_family() {
        let schema = Schema::new_with_metadata(
            vec![
                Field::new("side", DataType::UInt8, false),
                Field::new("ts_recv", DataType::UInt64, false),
            ],
            [("type_name".to_string(), "CustomData".to_string())].into(),
        );

        assert!(!is_nautilus_legacy_schema(&schema));
    }

    #[rstest]
    fn known_metadata_families_require_and_normalize_their_legacy_shape() {
        let schemas = [
            Schema::new_with_metadata(
                vec![
                    Field::new("rate", DataType::Binary, false),
                    Field::new("interval", DataType::UInt16, true),
                    Field::new("next_funding_ns", DataType::UInt64, true),
                    Field::new("ts_event", DataType::UInt64, false),
                    Field::new("ts_init", DataType::UInt64, false),
                ],
                [("type".to_string(), "FundingRateUpdate".to_string())].into(),
            ),
            Schema::new_with_metadata(
                vec![
                    Field::new("instrument_id", DataType::Utf8, true),
                    Field::new("action", DataType::Utf8, true),
                    Field::new("reason", DataType::Utf8, true),
                    Field::new("trading_event", DataType::Utf8, true),
                    Field::new("is_trading", DataType::Boolean, true),
                    Field::new("is_quoting", DataType::Boolean, true),
                    Field::new("is_short_sell_restricted", DataType::Boolean, true),
                    Field::new("ts_event", DataType::UInt64, true),
                    Field::new("ts_init", DataType::UInt64, true),
                ],
                [("type".to_string(), "InstrumentStatus".to_string())].into(),
            ),
            Schema::new_with_metadata(
                vec![
                    Field::new("instrument_id", DataType::Utf8, false),
                    Field::new("delta", DataType::Float64, false),
                    Field::new("gamma", DataType::Float64, false),
                    Field::new("vega", DataType::Float64, false),
                    Field::new("theta", DataType::Float64, false),
                    Field::new("rho", DataType::Float64, false),
                    Field::new("mark_iv", DataType::Float64, true),
                    Field::new("bid_iv", DataType::Float64, true),
                    Field::new("ask_iv", DataType::Float64, true),
                    Field::new("underlying_price", DataType::Float64, true),
                    Field::new("open_interest", DataType::Float64, true),
                    Field::new("ts_event", DataType::UInt64, false),
                    Field::new("ts_init", DataType::UInt64, false),
                    Field::new("convention", DataType::Utf8, false),
                ],
                [("type".to_string(), "OptionGreeks".to_string())].into(),
            ),
        ];

        for schema in schemas {
            assert!(is_nautilus_legacy_schema(&schema));
            let normalized =
                normalize_legacy_fixed_columns(&RecordBatch::new_empty(Arc::new(schema))).unwrap();

            assert_eq!(
                normalized
                    .schema()
                    .field_with_name("ts_init")
                    .unwrap()
                    .data_type(),
                &timestamp_data_type(),
            );
        }
    }

    #[rstest]
    fn near_match_with_foreign_field_type_is_not_retyped() {
        let schema = Arc::new(Schema::new(vec![
            Field::new("action", DataType::UInt8, false),
            Field::new("side", DataType::UInt8, false),
            Field::new("price", DataType::FixedSizeBinary(8), false),
            Field::new("size", DataType::FixedSizeBinary(8), false),
            Field::new("order_id", DataType::UInt64, false),
            Field::new("flags", DataType::UInt8, false),
            Field::new("sequence", DataType::Int64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ]));
        let batch = RecordBatch::new_empty(schema);

        let normalized = normalize_legacy_fixed_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    macro_rules! collect_data_schemas {
        ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
            vec![
                $(
                    (stringify!($type), <$type as ArrowSchemaProvider>::get_schema(None)),
                )+
            ]
        };
    }

    macro_rules! assert_model_field_map {
        // InstrumentAny is an enum over concrete instrument types and has no field map.
        (InstrumentAny) => {};
        // InstrumentStatus has no model get_fields implementation.
        (InstrumentStatus) => {};
        // OptionGreeks has no model get_fields implementation.
        (OptionGreeks) => {};
        (OrderBookDelta) => {
            assert_fields_match_schema(
                catalog_field_map(OrderBookDelta::get_fields()),
                &OrderBookDelta::get_schema(None),
                &["price", "size", KEY_IDENTIFIER],
            );
        };
        (OrderBookDepth) => {
            assert_fields_match_schema(
                catalog_field_map(OrderBookDepth::get_fields()),
                &OrderBookDepth::get_schema(None),
                &[KEY_IDENTIFIER],
            );
        };
        (QuoteTick) => {
            assert_fields_match_schema(
                catalog_field_map(QuoteTick::get_fields()),
                &QuoteTick::get_schema(None),
                &[
                    "bid_price",
                    "ask_price",
                    "bid_size",
                    "ask_size",
                    KEY_IDENTIFIER,
                ],
            );
        };
        (TradeTick) => {
            assert_fields_match_schema(
                catalog_field_map(TradeTick::get_fields()),
                &TradeTick::get_schema(None),
                &["price", "size", KEY_IDENTIFIER],
            );
        };
        (Bar) => {
            assert_fields_match_schema(
                catalog_field_map(Bar::get_fields()),
                &Bar::get_schema(None),
                &["open", "high", "low", "close", "volume", KEY_IDENTIFIER],
            );
        };
        (MarkPriceUpdate) => {
            assert_fields_match_schema(
                catalog_field_map(MarkPriceUpdate::get_fields()),
                &MarkPriceUpdate::get_schema(None),
                &["value", KEY_IDENTIFIER],
            );
        };
        (IndexPriceUpdate) => {
            assert_fields_match_schema(
                catalog_field_map(IndexPriceUpdate::get_fields()),
                &IndexPriceUpdate::get_schema(None),
                &["value", KEY_IDENTIFIER],
            );
        };
        (FundingRateUpdate) => {
            assert_fields_match_schema(
                catalog_field_map(FundingRateUpdate::get_fields()),
                &FundingRateUpdate::get_schema(None),
                &["interval", "next_funding_ns", KEY_IDENTIFIER],
            );
        };
        (InstrumentClose) => {
            assert_fields_match_schema(
                catalog_field_map(InstrumentClose::get_fields()),
                &InstrumentClose::get_schema(None),
                &["close_price", KEY_IDENTIFIER],
            );
        };
    }

    macro_rules! assert_registered_model_field_maps {
        ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
            $(assert_model_field_map!($type);)+
        };
    }

    #[rstest]
    fn registered_write_schemas_have_no_opaque_byte_fields() {
        let mut schemas = nautilus_model::for_each_data_type!(collect_data_schemas);
        schemas.extend(instrument_schemas());
        schemas.extend(record_schemas());

        for (name, schema) in schemas {
            assert_open_schema(name, &schema);
        }
    }

    #[rstest]
    fn model_field_maps_match_encoder_schemas() {
        nautilus_model::for_each_data_type!(assert_registered_model_field_maps);
    }

    #[rstest]
    fn fixed_point_storage_uses_uniform_scale() {
        let price = Price::from("1.23456789");
        let array = price_decimal_array([price.raw()], "price").unwrap();

        assert_eq!(array.data_type(), &fixed_decimal_data_type());
        assert_eq!(
            decode_decimal_price(&array, price.precision, "price", 0).unwrap(),
            price,
        );
    }

    #[rstest]
    fn undefined_fixed_point_values_encode_as_null() {
        let prices = price_decimal_array([PRICE_UNDEF], "price").unwrap();
        let quantities = quantity_decimal_array([QUANTITY_UNDEF], "quantity").unwrap();

        assert!(prices.is_null(0));
        assert!(quantities.is_null(0));
    }

    #[rstest]
    fn price_error_fails_with_field_and_value() {
        let error = price_decimal_array([PRICE_ERROR], "bid_price").unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Invalid argument error: Price field 'bid_price' contains PRICE_ERROR raw value {PRICE_ERROR}"
            ),
        );
    }

    #[rstest]
    fn quantity_overflow_fails_with_field_and_value() {
        let raw = QUANTITY_RAW_MAX + 1;
        let error = quantity_decimal_array([raw], "bid_size").unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Invalid argument error: Quantity field 'bid_size' raw value {raw} exceeds QUANTITY_RAW_MAX={QUANTITY_RAW_MAX}"
            ),
        );
    }

    #[rstest]
    fn legacy_price_error_fails_with_field_and_row() {
        let fixed = |raw: [u8; 8]| -> ArrayRef {
            Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(raw.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            )
        };
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(
                vec![
                    Field::new("bid_price", DataType::FixedSizeBinary(8), false),
                    Field::new("ask_price", DataType::FixedSizeBinary(8), false),
                    Field::new("bid_size", DataType::FixedSizeBinary(8), false),
                    Field::new("ask_size", DataType::FixedSizeBinary(8), false),
                    Field::new("ts_event", DataType::UInt64, false),
                    Field::new("ts_init", DataType::UInt64, false),
                ],
                [("type".to_string(), "QuoteTick".to_string())].into(),
            )),
            vec![
                fixed(i64::MIN.to_le_bytes()),
                fixed(2_i64.to_le_bytes()),
                fixed(3_u64.to_le_bytes()),
                fixed(4_u64.to_le_bytes()),
                Arc::new(UInt64Array::from(vec![5])),
                Arc::new(UInt64Array::from(vec![6])),
            ],
        )
        .unwrap();

        let error = normalize_legacy_fixed_columns(&batch).unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Cast error: Legacy price column 'bid_price' contains PRICE_ERROR raw value {} at row 0",
                i64::MIN,
            ),
        );
    }

    #[rstest]
    fn metadata_family_near_match_passes_through_unchanged() {
        let raw = i64::MIN.to_le_bytes();
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(
                vec![Field::new("bid_price", DataType::FixedSizeBinary(8), false)],
                [("type".to_string(), "QuoteTick".to_string())].into(),
            )),
            vec![Arc::new(
                FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                    [Some(raw.as_slice())].into_iter(),
                    8,
                )
                .unwrap(),
            )],
        )
        .unwrap();

        let normalized = normalize_legacy_fixed_columns(&batch).unwrap();

        assert_eq!(normalized, batch);
    }

    #[cfg(not(feature = "high-precision"))]
    #[rstest]
    fn standard_precision_decode_rejects_nonzero_scale_remainder() {
        let array = Decimal128Array::from(vec![1_i128])
            .with_precision_and_scale(38, 16)
            .unwrap();

        let error = decode_decimal_price(&array, 9, "bid_price", 0).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Error parsing `bid_price`: row 0: decimal value 1 has nonzero digits beyond build \
             precision 9",
        );
    }

    #[cfg(feature = "high-precision")]
    #[rstest]
    fn high_precision_decimal_payload_is_bit_exact() {
        let raw = 1_234_567_890_123_456_i128;
        let array = price_decimal_array([raw], "price").unwrap();

        assert_eq!(array.value(0), raw);
        assert_eq!(
            decode_decimal_price(&array, 16, "price", 0).unwrap().raw(),
            raw
        );
    }

    #[rstest]
    #[case::quote(FixedFamily::Quote)]
    #[case::trade(FixedFamily::Trade)]
    #[case::bar(FixedFamily::Bar)]
    #[case::delta(FixedFamily::Delta)]
    #[case::depth(FixedFamily::Depth)]
    #[case::mark_price(FixedFamily::MarkPrice)]
    #[case::index_price(FixedFamily::IndexPrice)]
    #[case::close(FixedFamily::Close)]
    fn fixed_family_round_trips_multiple_precisions(#[case] family: FixedFamily) {
        round_trip_fixed_family(family, Price::from("1"), Quantity::from("2"));
        round_trip_fixed_family(family, Price::from("1.23456"), Quantity::from("2.34567"));
        #[cfg(feature = "high-precision")]
        round_trip_fixed_family(
            family,
            Price::from("1.2345678901234567"),
            Quantity::from("2.3456789012345678"),
        );
    }

    #[rstest]
    #[case::quote(FixedFamily::Quote)]
    #[case::trade(FixedFamily::Trade)]
    #[case::bar(FixedFamily::Bar)]
    #[case::delta(FixedFamily::Delta)]
    #[case::mark_price(FixedFamily::MarkPrice)]
    #[case::index_price(FixedFamily::IndexPrice)]
    #[case::close(FixedFamily::Close)]
    fn fixed_family_sentinel_encoding(#[case] family: FixedFamily) {
        let instrument_id = InstrumentId::from("SENTINEL.TEST");
        let price = Price::from_raw(PRICE_UNDEF, 0);
        let quantity = Quantity::from_raw(QUANTITY_UNDEF, 0);

        match family {
            FixedFamily::Quote => {
                let value = QuoteTick::new(
                    instrument_id,
                    price,
                    Price::from("1"),
                    quantity,
                    Quantity::from("1"),
                    1.into(),
                    2.into(),
                );
                let metadata = QuoteTick::get_metadata(&instrument_id, 0, 0);
                let error = QuoteTick::encode_batch(&metadata, &[value]).unwrap_err();
                assert!(error.to_string().contains("bid_price"));
                assert!(error.to_string().contains("PRICE_UNDEF"));
            }
            FixedFamily::Trade => {
                let value = TradeTick {
                    instrument_id,
                    price,
                    size: quantity,
                    aggressor_side: AggressorSide::Buy,
                    trade_id: TradeId::from("sentinel"),
                    ts_event: 1.into(),
                    ts_init: 2.into(),
                };
                let metadata = TradeTick::get_metadata(&instrument_id, 0, 0);
                let error = TradeTick::encode_batch(&metadata, &[value]).unwrap_err();
                assert!(error.to_string().contains("price"));
                assert!(error.to_string().contains("PRICE_UNDEF"));
            }
            FixedFamily::Bar => {
                let mut value = stub_bar();
                value.open = price;
                value.volume = quantity;
                let metadata = Bar::get_metadata(&value.bar_type, 0, 0);
                let error = Bar::encode_batch(&metadata, &[value]).unwrap_err();
                assert!(error.to_string().contains("open"));
                assert!(error.to_string().contains("PRICE_UNDEF"));
            }
            FixedFamily::Delta => {
                let value = OrderBookDelta {
                    instrument_id,
                    action: BookAction::Update,
                    order: BookOrder {
                        side: OrderSide::Buy.into(),
                        price,
                        size: quantity,
                        order_id: 1,
                    },
                    flags: 0,
                    sequence: 1,
                    ts_event: 1.into(),
                    ts_init: 2.into(),
                };
                let metadata = OrderBookDelta::get_metadata(&instrument_id, 0, 0);
                let batch = OrderBookDelta::encode_batch(&metadata, &[value]).unwrap();
                assert!(batch.column_by_name("price").unwrap().is_null(0));
                assert!(batch.column_by_name("size").unwrap().is_null(0));
                assert_eq!(
                    OrderBookDelta::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::Depth => unreachable!("depth sides omit absent levels"),
            FixedFamily::MarkPrice => {
                let value = MarkPriceUpdate::new(instrument_id, price, 1.into(), 2.into());
                let metadata = MarkPriceUpdate::get_metadata(&instrument_id, 0);
                let batch = MarkPriceUpdate::encode_batch(&metadata, &[value]).unwrap();
                assert!(batch.column_by_name("value").unwrap().is_null(0));
                assert_eq!(
                    MarkPriceUpdate::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::IndexPrice => {
                let value = IndexPriceUpdate::new(instrument_id, price, 1.into(), 2.into());
                let metadata = IndexPriceUpdate::get_metadata(&instrument_id, 0);
                let batch = IndexPriceUpdate::encode_batch(&metadata, &[value]).unwrap();
                assert!(batch.column_by_name("value").unwrap().is_null(0));
                assert_eq!(
                    IndexPriceUpdate::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::Close => {
                let value = InstrumentClose::new(
                    instrument_id,
                    price,
                    InstrumentCloseType::EndOfSession,
                    1.into(),
                    2.into(),
                );
                let metadata = InstrumentClose::get_metadata(&instrument_id, 0);
                let batch = InstrumentClose::encode_batch(&metadata, &[value]).unwrap();
                assert!(batch.column_by_name("close_price").unwrap().is_null(0));
                assert_eq!(
                    InstrumentClose::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
        }
    }

    fn round_trip_fixed_family(family: FixedFamily, price: Price, quantity: Quantity) {
        let instrument_id = InstrumentId::from("PRECISION.TEST");

        match family {
            FixedFamily::Quote => {
                let value = QuoteTick::new(
                    instrument_id,
                    price,
                    price,
                    quantity,
                    quantity,
                    1.into(),
                    2.into(),
                );
                let metadata =
                    QuoteTick::get_metadata(&instrument_id, price.precision, quantity.precision);
                let batch = QuoteTick::encode_batch(&metadata, &[value]).unwrap();
                assert_eq!(
                    QuoteTick::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::Trade => {
                let value = TradeTick::new(
                    instrument_id,
                    price,
                    quantity,
                    AggressorSide::Buy,
                    TradeId::from("precision"),
                    1.into(),
                    2.into(),
                );
                let metadata =
                    TradeTick::get_metadata(&instrument_id, price.precision, quantity.precision);
                let batch = TradeTick::encode_batch(&metadata, &[value]).unwrap();
                assert_eq!(
                    TradeTick::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::Bar => {
                let mut value = stub_bar();
                value.open = price;
                value.high = price;
                value.low = price;
                value.close = price;
                value.volume = quantity;
                let metadata =
                    Bar::get_metadata(&value.bar_type, price.precision, quantity.precision);
                let batch = Bar::encode_batch(&metadata, &[value]).unwrap();
                assert_eq!(Bar::decode_batch(&metadata, batch).unwrap(), vec![value]);
            }
            FixedFamily::Delta => {
                let value = OrderBookDelta::new(
                    instrument_id,
                    BookAction::Add,
                    BookOrder {
                        side: OrderSide::Buy.into(),
                        price,
                        size: quantity,
                        order_id: 1,
                    },
                    0,
                    1,
                    1.into(),
                    2.into(),
                );
                let metadata = OrderBookDelta::get_metadata(
                    &instrument_id,
                    price.precision,
                    quantity.precision,
                );
                let batch = OrderBookDelta::encode_batch(&metadata, &[value]).unwrap();
                assert_eq!(
                    OrderBookDelta::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::Depth => {
                let mut value = stub_depth10();
                for order in value.bids.iter_mut().chain(value.asks.iter_mut()) {
                    order.price = price;
                    order.size = quantity;
                }
                let metadata = OrderBookDepth::get_metadata(
                    &value.instrument_id,
                    price.precision,
                    quantity.precision,
                );
                let batch = OrderBookDepth::encode_batch(&metadata, &[value.clone()]).unwrap();
                assert_eq!(
                    OrderBookDepth::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::MarkPrice => {
                let value = MarkPriceUpdate::new(instrument_id, price, 1.into(), 2.into());
                let metadata = MarkPriceUpdate::get_metadata(&instrument_id, price.precision);
                let batch = MarkPriceUpdate::encode_batch(&metadata, &[value]).unwrap();
                assert_eq!(
                    MarkPriceUpdate::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::IndexPrice => {
                let value = IndexPriceUpdate::new(instrument_id, price, 1.into(), 2.into());
                let metadata = IndexPriceUpdate::get_metadata(&instrument_id, price.precision);
                let batch = IndexPriceUpdate::encode_batch(&metadata, &[value]).unwrap();
                assert_eq!(
                    IndexPriceUpdate::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
            FixedFamily::Close => {
                let value = InstrumentClose::new(
                    instrument_id,
                    price,
                    InstrumentCloseType::EndOfSession,
                    1.into(),
                    2.into(),
                );
                let metadata = InstrumentClose::get_metadata(&instrument_id, price.precision);
                let batch = InstrumentClose::encode_batch(&metadata, &[value]).unwrap();
                assert_eq!(
                    InstrumentClose::decode_batch(&metadata, batch).unwrap(),
                    vec![value],
                );
            }
        }
    }

    fn instrument_schemas() -> Vec<(&'static str, Schema)> {
        // Keep this list explicit until instrument types have a registry equivalent to data types.
        vec![
            schema::<BettingInstrument>(),
            schema::<BinaryOption>(),
            schema::<Cfd>(),
            schema::<Commodity>(),
            schema::<CryptoFuture>(),
            schema::<CryptoFuturesSpread>(),
            schema::<CryptoOption>(),
            schema::<CryptoOptionSpread>(),
            schema::<CryptoPerpetual>(),
            schema::<CurrencyPair>(),
            schema::<Equity>(),
            schema::<FuturesContract>(),
            schema::<FuturesSpread>(),
            schema::<IndexInstrument>(),
            schema::<OptionContract>(),
            schema::<OptionSpread>(),
            schema::<PerpetualContract>(),
            schema::<TokenizedAsset>(),
        ]
    }

    fn record_schemas() -> Vec<(&'static str, Schema)> {
        // Keep this list explicit until record types have a registry equivalent to data types.
        vec![
            schema::<AccountState>(),
            schema::<OrderInitialized>(),
            schema::<OrderDenied>(),
            schema::<OrderEmulated>(),
            schema::<OrderSubmitted>(),
            schema::<OrderAccepted>(),
            schema::<OrderRejected>(),
            schema::<OrderPendingCancel>(),
            schema::<OrderCanceled>(),
            schema::<OrderCancelRejected>(),
            schema::<OrderExpired>(),
            schema::<OrderTriggered>(),
            schema::<OrderPendingUpdate>(),
            schema::<OrderReleased>(),
            schema::<OrderModifyRejected>(),
            schema::<OrderUpdated>(),
            schema::<OrderFilled>(),
            schema::<OrderFillVoided>(),
            schema::<PositionOpened>(),
            schema::<PositionChanged>(),
            schema::<PositionClosed>(),
            schema::<PositionAdjusted>(),
            schema::<OrderStatusReport>(),
            schema::<FillReport>(),
            schema::<PositionStatusReport>(),
            schema::<ExecutionMassStatus>(),
            schema::<OrderSnapshot>(),
            schema::<PositionSnapshot>(),
        ]
    }

    fn schema<T: ArrowSchemaProvider>() -> (&'static str, Schema) {
        (std::any::type_name::<T>(), T::get_schema(None))
    }

    fn catalog_field_map(
        fields: impl IntoIterator<Item = (String, String)>,
    ) -> Vec<(String, String)> {
        let mut fields = fields.into_iter().collect::<Vec<_>>();
        fields.push((KEY_IDENTIFIER.to_string(), "Utf8".to_string()));
        fields
    }

    fn assert_fields_match_schema(
        expected: Vec<(String, String)>,
        schema: &Schema,
        nullable: &[&str],
    ) {
        let expected = expected
            .into_iter()
            .map(|(name, data_type)| {
                let is_nullable = nullable.contains(&name.as_str());
                (name, data_type, is_nullable)
            })
            .collect::<Vec<_>>();
        let actual: Vec<_> = schema
            .fields()
            .iter()
            .map(|field| {
                (
                    field.name().clone(),
                    arrow_type_name(field.data_type()),
                    field.is_nullable(),
                )
            })
            .collect();

        assert_eq!(actual, expected);
    }

    fn arrow_type_name(data_type: &DataType) -> String {
        match data_type {
            DataType::List(field) => format!("List({})", arrow_type_name(field.data_type())),
            DataType::Struct(fields) => {
                let fields = fields
                    .iter()
                    .map(|field| {
                        format!("{}: {}", field.name(), arrow_type_name(field.data_type()))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Struct({fields})")
            }
            _ => format!("{data_type:?}"),
        }
    }

    fn assert_open_schema(name: &str, schema: &Schema) {
        for field in schema.fields() {
            assert_open_field(name, field);
        }
    }

    fn assert_open_field(schema_name: &str, field: &Field) {
        if is_timestamp_field(field.name()) {
            assert_eq!(
                field.data_type(),
                &timestamp_data_type(),
                "write schema `{schema_name}` timestamp field `{}` is not a UTC nanosecond timestamp",
                field.name(),
            );
        }

        match field.data_type() {
            DataType::Binary
            | DataType::LargeBinary
            | DataType::BinaryView
            | DataType::FixedSizeBinary(_) => {
                panic!(
                    "write schema `{schema_name}` contains opaque byte field `{}`: {}",
                    field.name(),
                    field.data_type(),
                );
            }
            DataType::List(child)
            | DataType::LargeList(child)
            | DataType::ListView(child)
            | DataType::LargeListView(child)
            | DataType::FixedSizeList(child, _)
            | DataType::Map(child, _) => assert_open_field(schema_name, child),
            DataType::Struct(children) => {
                for child in children {
                    assert_open_field(schema_name, child);
                }
            }
            _ => {}
        }
    }
}
