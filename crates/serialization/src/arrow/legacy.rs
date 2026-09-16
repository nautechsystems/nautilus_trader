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

//! Legacy Arrow schema detection and conversion to current Rust schemas.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::{Display, Write},
    str::FromStr,
    sync::Arc,
};

use arrow::{
    array::{
        Array, ArrayRef, BinaryArray, Decimal128Array, FixedSizeBinaryArray, FixedSizeListArray,
        StringArray, StringBuilder, TimestampNanosecondArray, UInt8Array, UInt8Builder,
        UInt16Array, UInt64Array, UInt64Builder, new_null_array,
    },
    compute::cast,
    datatypes::{DataType, Field, Schema, TimeUnit},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        Bar, DEPTH10_LEN, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick,
    },
    enums::{AggressorSide, BookAction, FromU8, InstrumentCloseType, OrderSide},
    types::Price,
};

use super::{
    ArrowSchemaProvider, FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE, KEY_IDENTIFIER,
    KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION, STANDARD_TO_DECIMAL_SCALE, enum_dictionary_array,
    enum_dictionary_data_type, fixed_decimal_data_type, price_decimal_array,
    schema_without_identifier_column, timestamp_column, timestamp_data_type,
};

/// Stable identity for Arrow field names, types, and nullability.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SchemaFingerprint(String);

impl Display for SchemaFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Registry decision applied to a source batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyTranscodeKind {
    PassThrough,
    InstrumentStatusV1,
    FundingRateUpdateV1,
    InstrumentCloseV1,
}

/// Record batches resolved to a current Rust Arrow schema.
#[derive(Debug)]
pub struct LegacyTranscodeResult {
    pub batches: Vec<RecordBatch>,
    pub kind: LegacyTranscodeKind,
}

/// File-scoped state for legacy Arrow transcoding.
#[derive(Debug, Default)]
pub struct LegacyTranscodeState {
    instrument_close_precisions: BTreeMap<String, u8>,
}

/// Preflight schema resolution for a source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LegacySchemaResolution {
    pub kind: LegacyTranscodeKind,
    pub source_fingerprint: SchemaFingerprint,
    pub target_fingerprint: SchemaFingerprint,
}

/// Failure to resolve or convert a legacy Arrow schema.
#[derive(Debug, thiserror::Error)]
pub enum LegacyArrowError {
    #[error(
        "No Arrow transcoder is registered for type {type_name}, file {file_path}, \
         and schema fingerprint {fingerprint}"
    )]
    UnknownSchema {
        type_name: String,
        file_path: String,
        fingerprint: SchemaFingerprint,
    },
    #[error("Failed to transcode type {type_name} from {file_path}: {message}")]
    Transcode {
        type_name: String,
        file_path: String,
        message: String,
    },
}

/// Returns a schema fingerprint that excludes schema metadata.
#[must_use]
pub fn schema_fingerprint(schema: &Schema) -> SchemaFingerprint {
    let mut value = String::new();

    for field in schema.fields() {
        write!(
            value,
            "{}:{:?}:{};",
            field.name(),
            field.data_type(),
            field.is_nullable()
        )
        .expect("writing a schema fingerprint to a string cannot fail");
    }
    SchemaFingerprint(value)
}

/// Resolves a normalized source batch through the legacy schema registry.
///
/// # Errors
///
/// Returns [`LegacyArrowError::UnknownSchema`] when a registered Nautilus type matches neither its
/// current schema nor a known v1 schema. Returns [`LegacyArrowError::Transcode`] when a known v1
/// batch contains values that cannot be converted without loss.
pub fn transcode_legacy_record_batch(
    type_name: &str,
    file_path: &str,
    batch: RecordBatch,
) -> Result<LegacyTranscodeResult, LegacyArrowError> {
    transcode_legacy_record_batch_with_state(
        type_name,
        file_path,
        batch,
        &mut LegacyTranscodeState::default(),
    )
}

/// Resolves a normalized source batch through the legacy schema registry with file-scoped state.
///
/// # Errors
///
/// Returns [`LegacyArrowError::UnknownSchema`] when a registered Nautilus type matches neither its
/// current schema nor a known v1 schema. Returns [`LegacyArrowError::Transcode`] when a known v1
/// batch contains values that cannot be converted without loss.
pub fn transcode_legacy_record_batch_with_state(
    type_name: &str,
    file_path: &str,
    batch: RecordBatch,
    state: &mut LegacyTranscodeState,
) -> Result<LegacyTranscodeResult, LegacyArrowError> {
    let resolution = resolve_legacy_schema(type_name, file_path, batch.schema().as_ref())?;
    if resolution.kind == LegacyTranscodeKind::PassThrough {
        return Ok(pass_through(batch));
    }
    let batches = match resolution.kind {
        LegacyTranscodeKind::InstrumentStatusV1 => {
            vec![transcode_instrument_status(&batch, file_path)?]
        }
        LegacyTranscodeKind::FundingRateUpdateV1 => {
            vec![transcode_funding_rate(&batch, file_path)?]
        }
        LegacyTranscodeKind::InstrumentCloseV1 => {
            transcode_instrument_close(&batch, file_path, state)?
        }
        LegacyTranscodeKind::PassThrough => {
            unreachable!("pass-through schemas are handled before legacy registry lookup")
        }
    };

    Ok(LegacyTranscodeResult {
        batches,
        kind: resolution.kind,
    })
}

/// Resolves a normalized source schema without decoding its record batches.
///
/// # Errors
///
/// Returns [`LegacyArrowError::UnknownSchema`] for an unregistered schema of a built-in type.
pub fn resolve_legacy_schema(
    type_name: &str,
    file_path: &str,
    schema: &Schema,
) -> Result<LegacySchemaResolution, LegacyArrowError> {
    let source_fingerprint = schema_fingerprint(schema);
    let Some(current_schema) = current_schema(type_name, schema.metadata().clone()) else {
        return Ok(LegacySchemaResolution {
            kind: LegacyTranscodeKind::PassThrough,
            target_fingerprint: source_fingerprint.clone(),
            source_fingerprint,
        });
    };
    let target_fingerprint = schema_fingerprint(&current_schema);
    let current_without_identifier =
        schema_fingerprint(&schema_without_identifier_column(&current_schema));
    let current_plain_strings = schema_with_plain_dictionary_strings(&current_schema);
    let current_plain_strings_fingerprint = schema_fingerprint(&current_plain_strings);
    let current_plain_strings_without_identifier =
        schema_fingerprint(&schema_without_identifier_column(&current_plain_strings));

    if source_fingerprint == target_fingerprint
        || source_fingerprint == current_without_identifier
        || source_fingerprint == current_plain_strings_fingerprint
        || source_fingerprint == current_plain_strings_without_identifier
    {
        return Ok(LegacySchemaResolution {
            kind: LegacyTranscodeKind::PassThrough,
            source_fingerprint,
            target_fingerprint,
        });
    }

    let kind = registered_legacy_kind(type_name, &source_fingerprint).ok_or_else(|| {
        LegacyArrowError::UnknownSchema {
            type_name: type_name.to_string(),
            file_path: file_path.to_string(),
            fingerprint: source_fingerprint.clone(),
        }
    })?;
    Ok(LegacySchemaResolution {
        kind,
        source_fingerprint,
        target_fingerprint,
    })
}

fn schema_with_plain_dictionary_strings(schema: &Schema) -> Schema {
    let fields = schema
        .fields()
        .iter()
        .map(|field| {
            let data_type = match field.data_type() {
                DataType::Dictionary(_, value_type)
                    if matches!(value_type.as_ref(), DataType::Utf8) =>
                {
                    DataType::Utf8
                }
                data_type => data_type.clone(),
            };
            field.as_ref().clone().with_data_type(data_type)
        })
        .collect::<Vec<_>>();
    Schema::new_with_metadata(fields, schema.metadata().clone())
}

fn pass_through(batch: RecordBatch) -> LegacyTranscodeResult {
    LegacyTranscodeResult {
        batches: vec![batch],
        kind: LegacyTranscodeKind::PassThrough,
    }
}

fn current_schema(type_name: &str, metadata: HashMap<String, String>) -> Option<Schema> {
    let metadata = Some(metadata);
    match type_name {
        "quotes" => Some(QuoteTick::get_schema(metadata)),
        "trades" => Some(TradeTick::get_schema(metadata)),
        "bars" => Some(Bar::get_schema(metadata)),
        "order_book_deltas" => Some(OrderBookDelta::get_schema(metadata)),
        "order_book_depths" => Some(OrderBookDepth::get_schema(metadata)),
        "mark_prices" => Some(MarkPriceUpdate::get_schema(metadata)),
        "index_prices" => Some(IndexPriceUpdate::get_schema(metadata)),
        "funding_rates" => Some(FundingRateUpdate::get_schema(metadata)),
        "instrument_status" => Some(InstrumentStatus::get_schema(metadata)),
        "option_greeks" => Some(OptionGreeks::get_schema(metadata)),
        "instrument_closes" => Some(InstrumentClose::get_schema(metadata)),
        _ => None,
    }
}

fn registered_legacy_kind(
    type_name: &str,
    fingerprint: &SchemaFingerprint,
) -> Option<LegacyTranscodeKind> {
    let entries = [
        (
            "instrument_status",
            legacy_instrument_status_schema(),
            LegacyTranscodeKind::InstrumentStatusV1,
        ),
        (
            "funding_rates",
            legacy_funding_rate_schema(),
            LegacyTranscodeKind::FundingRateUpdateV1,
        ),
        (
            "instrument_closes",
            legacy_instrument_close_schema(),
            LegacyTranscodeKind::InstrumentCloseV1,
        ),
    ];

    entries
        .into_iter()
        .find(|(entry_type, schema, _)| {
            type_name == *entry_type
                && (schema_fingerprint(schema) == *fingerprint
                    || schema_fingerprint(&schema_with_normalized_legacy_types(schema))
                        == *fingerprint)
        })
        .map(|(_, _, kind)| kind)
}

fn schema_with_normalized_legacy_types(schema: &Schema) -> Schema {
    let fields = schema
        .fields()
        .iter()
        .map(|field| {
            Arc::new(
                field
                    .as_ref()
                    .clone()
                    .with_data_type(normalized_legacy_data_type(field.name(), field.data_type())),
            )
        })
        .collect::<Vec<_>>();
    Schema::new_with_metadata(fields, schema.metadata().clone())
}

fn legacy_instrument_status_schema() -> Schema {
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
    ])
}

fn legacy_funding_rate_schema() -> Schema {
    Schema::new(vec![
        Field::new("rate", DataType::Binary, false),
        Field::new("interval", DataType::UInt16, true),
        Field::new("next_funding_ns", DataType::UInt64, true),
        Field::new("ts_event", DataType::UInt64, false),
        Field::new("ts_init", DataType::UInt64, false),
    ])
}

fn legacy_instrument_close_schema() -> Schema {
    Schema::new(vec![
        Field::new("instrument_id", DataType::Utf8, true),
        Field::new("close_type", DataType::Utf8, true),
        Field::new("close_price", DataType::Utf8, true),
        Field::new("ts_event", DataType::UInt64, true),
        Field::new("ts_init", DataType::UInt64, true),
    ])
}

fn transcode_instrument_status(
    batch: &RecordBatch,
    file_path: &str,
) -> Result<RecordBatch, LegacyArrowError> {
    let mut metadata = batch.schema().metadata().clone();
    metadata.insert("type".to_string(), "InstrumentStatus".to_string());
    let schema = InstrumentStatus::get_schema(Some(metadata));
    let instrument_id = required_column(batch, "instrument_id", file_path)?;
    let mut columns = Vec::with_capacity(schema.fields().len());

    for field in schema.fields() {
        let column = if field.name() == KEY_IDENTIFIER {
            instrument_id.clone()
        } else if let Some(column) = batch.column_by_name(field.name()) {
            column.clone()
        } else if field.is_nullable() {
            new_null_array(field.data_type(), batch.num_rows())
        } else {
            return Err(transcode_error(
                "instrument_status",
                file_path,
                format!("required field {} is absent", field.name()),
            ));
        };

        let convertible_timestamp = field.data_type() == &super::timestamp_data_type()
            && column.data_type() == &DataType::UInt64;
        if column.data_type() != field.data_type() && !convertible_timestamp {
            return Err(transcode_error(
                "instrument_status",
                file_path,
                format!(
                    "field {} expected {:?}, found {:?}",
                    field.name(),
                    field.data_type(),
                    column.data_type()
                ),
            ));
        }

        if !field.is_nullable() && column.null_count() > 0 {
            return Err(transcode_error(
                "instrument_status",
                file_path,
                format!("required field {} contains nulls", field.name()),
            ));
        }
        columns.push(column);
    }

    super::record_batch_with_timestamps(Arc::new(schema), columns)
        .map_err(|e| transcode_error("instrument_status", file_path, e.to_string()))
}

fn transcode_funding_rate(
    batch: &RecordBatch,
    file_path: &str,
) -> Result<RecordBatch, LegacyArrowError> {
    let instrument_id = batch
        .schema()
        .metadata()
        .get(KEY_INSTRUMENT_ID)
        .cloned()
        .ok_or_else(|| {
            transcode_error(
                "funding_rates",
                file_path,
                "instrument_id schema metadata is absent",
            )
        })?;
    let rate = required_typed_column::<BinaryArray>(batch, "rate", file_path, "funding_rates")?;
    let interval =
        required_typed_column::<UInt16Array>(batch, "interval", file_path, "funding_rates")?;
    let mut rate_builder = StringBuilder::with_capacity(batch.num_rows(), rate.value_data().len());

    for row in 0..batch.num_rows() {
        if rate.is_null(row) {
            return Err(transcode_error(
                "funding_rates",
                file_path,
                format!("required field rate is null at row {row}"),
            ));
        }
        let value = serde_json::from_slice::<String>(rate.value(row)).map_err(|e| {
            transcode_error(
                "funding_rates",
                file_path,
                format!("rate at row {row} is not a msgspec JSON string: {e}"),
            )
        })?;
        rate_builder.append_value(value);
    }

    let mut metadata = batch.schema().metadata().clone();
    metadata.insert("type".to_string(), "FundingRateUpdate".to_string());
    metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.clone());
    let schema = FundingRateUpdate::get_schema(Some(metadata));
    let identifiers = StringArray::from(vec![instrument_id; batch.num_rows()]);
    let columns: Vec<ArrayRef> = vec![
        Arc::new(identifiers.clone()),
        Arc::new(rate_builder.finish()),
        cast(interval, &DataType::UInt64)
            .map_err(|e| transcode_error("funding_rates", file_path, e.to_string()))?,
        required_column(batch, "next_funding_ns", file_path)?,
        required_column(batch, "ts_event", file_path)?,
        required_column(batch, "ts_init", file_path)?,
        Arc::new(identifiers),
    ];

    super::record_batch_with_timestamps(Arc::new(schema), columns)
        .map_err(|e| transcode_error("funding_rates", file_path, e.to_string()))
}

fn transcode_instrument_close(
    batch: &RecordBatch,
    file_path: &str,
    state: &mut LegacyTranscodeState,
) -> Result<Vec<RecordBatch>, LegacyArrowError> {
    let instrument_ids = required_typed_column::<StringArray>(
        batch,
        "instrument_id",
        file_path,
        "instrument_closes",
    )?;
    let close_types =
        required_typed_column::<StringArray>(batch, "close_type", file_path, "instrument_closes")?;
    let close_prices =
        required_typed_column::<StringArray>(batch, "close_price", file_path, "instrument_closes")?;
    let ts_events =
        required_typed_column::<UInt64Array>(batch, "ts_event", file_path, "instrument_closes")?;
    let ts_inits =
        required_typed_column::<UInt64Array>(batch, "ts_init", file_path, "instrument_closes")?;
    let mut rows_by_instrument: BTreeMap<String, Vec<usize>> = BTreeMap::new();

    for row in 0..batch.num_rows() {
        if instrument_ids.is_null(row) {
            return Err(transcode_error(
                "instrument_closes",
                file_path,
                format!("instrument_id is null at row {row}"),
            ));
        }
        rows_by_instrument
            .entry(instrument_ids.value(row).to_string())
            .or_default()
            .push(row);
    }

    let mut batches = Vec::with_capacity(rows_by_instrument.len());
    for (instrument_id, rows) in rows_by_instrument {
        let (batch, precision) = transcode_instrument_close_rows(
            batch,
            file_path,
            &instrument_id,
            &rows,
            close_types,
            close_prices,
            ts_events,
            ts_inits,
            state
                .instrument_close_precisions
                .get(&instrument_id)
                .copied(),
        )?;
        state
            .instrument_close_precisions
            .insert(instrument_id, precision);
        batches.push(batch);
    }
    Ok(batches)
}

#[expect(
    clippy::too_many_arguments,
    reason = "the arguments are the validated source columns for one legacy close batch"
)]
fn transcode_instrument_close_rows(
    batch: &RecordBatch,
    file_path: &str,
    instrument_id: &str,
    rows: &[usize],
    close_types: &StringArray,
    close_prices: &StringArray,
    ts_events: &UInt64Array,
    ts_inits: &UInt64Array,
    expected_precision: Option<u8>,
) -> Result<(RecordBatch, u8), LegacyArrowError> {
    let mut prices = Vec::with_capacity(rows.len());
    let mut type_builder = UInt8Builder::with_capacity(rows.len());
    let mut event_builder = UInt64Builder::with_capacity(rows.len());
    let mut init_builder = UInt64Builder::with_capacity(rows.len());
    let mut precision = expected_precision;

    for &row in rows {
        if close_types.is_null(row)
            || close_prices.is_null(row)
            || ts_events.is_null(row)
            || ts_inits.is_null(row)
        {
            return Err(transcode_error(
                "instrument_closes",
                file_path,
                format!("required close field is null at row {row}"),
            ));
        }

        let value = close_prices.value(row);
        let row_precision = decimal_precision(value).map_err(|message| {
            transcode_error(
                "instrument_closes",
                file_path,
                format!("invalid close_price {value:?} at row {row}: {message}"),
            )
        })?;

        if let Some(existing) = precision
            && existing != row_precision
        {
            return Err(transcode_error(
                "instrument_closes",
                file_path,
                format!(
                    "close_price precision conflict at row {row}: found {row_precision}, \
                     expected {existing}"
                ),
            ));
        }
        precision = Some(row_precision);

        let price = Price::from_str(value).map_err(|e| {
            transcode_error(
                "instrument_closes",
                file_path,
                format!("invalid close_price {value:?} at row {row}: {e}"),
            )
        })?;
        let close_type = InstrumentCloseType::from_str(close_types.value(row)).map_err(|e| {
            transcode_error(
                "instrument_closes",
                file_path,
                format!(
                    "unknown close_type {:?} at row {row}: {e}",
                    close_types.value(row)
                ),
            )
        })?;

        prices.push(price.raw());
        type_builder.append_value(close_type as u8);
        event_builder.append_value(ts_events.value(row));
        init_builder.append_value(ts_inits.value(row));
    }

    let precision = precision.unwrap_or(0);
    let mut metadata = batch.schema().metadata().clone();
    metadata.insert("type".to_string(), "InstrumentClose".to_string());
    metadata.insert(KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string());
    metadata.insert(KEY_PRICE_PRECISION.to_string(), precision.to_string());
    let schema = InstrumentClose::get_schema(Some(metadata));
    let identifiers = StringArray::from(vec![instrument_id; rows.len()]);

    let batch = super::record_batch_with_timestamps(
        Arc::new(schema),
        vec![
            Arc::new(
                price_decimal_array(prices, "close_price")
                    .map_err(|e| transcode_error("instrument_closes", file_path, e.to_string()))?,
            ),
            Arc::new(type_builder.finish()),
            Arc::new(event_builder.finish()),
            Arc::new(init_builder.finish()),
            Arc::new(identifiers),
        ],
    )
    .map_err(|e| transcode_error("instrument_closes", file_path, e.to_string()))?;
    Ok((batch, precision))
}

fn decimal_precision(value: &str) -> Result<u8, String> {
    let precision = value
        .split_once('.')
        .map_or(0, |(_, fractional)| fractional.len());
    u8::try_from(precision).map_err(|e| e.to_string())
}

fn required_column(
    batch: &RecordBatch,
    name: &str,
    file_path: &str,
) -> Result<ArrayRef, LegacyArrowError> {
    batch.column_by_name(name).cloned().ok_or_else(|| {
        transcode_error(
            "legacy_arrow",
            file_path,
            format!("required field {name} is absent"),
        )
    })
}

fn required_typed_column<'a, T: Array + 'static>(
    batch: &'a RecordBatch,
    name: &str,
    file_path: &str,
    type_name: &str,
) -> Result<&'a T, LegacyArrowError> {
    let column = batch.column_by_name(name).ok_or_else(|| {
        transcode_error(
            type_name,
            file_path,
            format!("required field {name} is absent"),
        )
    })?;
    column.as_any().downcast_ref::<T>().ok_or_else(|| {
        transcode_error(
            type_name,
            file_path,
            format!("field {name} has unexpected type {:?}", column.data_type()),
        )
    })
}

fn transcode_error(
    type_name: &str,
    file_path: &str,
    message: impl Into<String>,
) -> LegacyArrowError {
    LegacyArrowError::Transcode {
        type_name: type_name.to_string(),
        file_path: file_path.to_string(),
        message: message.into(),
    }
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

pub(super) fn legacy_enum_dictionary_column(
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use arrow::{
        array::{BinaryArray, BooleanArray},
        datatypes::Schema,
    };
    use nautilus_model::{
        data::{FundingRateUpdate, InstrumentClose},
        enums::{InstrumentCloseType, MarketStatusAction},
    };
    use rstest::rstest;

    use super::*;
    use crate::arrow::{DecodeFromRecordBatch, DecodeTypedFromRecordBatch, StringColumnRef};

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

    #[rstest]
    fn schema_fingerprint_ignores_metadata_and_includes_nullability() {
        let fields = vec![Field::new("value", DataType::Utf8, false)];
        let schema1 = Schema::new_with_metadata(
            fields.clone(),
            HashMap::from([("a".to_string(), "1".to_string())]),
        );
        let schema2 =
            Schema::new_with_metadata(fields, HashMap::from([("b".to_string(), "2".to_string())]));
        let nullable = Schema::new(vec![Field::new("value", DataType::Utf8, true)]);

        assert_eq!(schema_fingerprint(&schema1), schema_fingerprint(&schema2));
        assert_ne!(schema_fingerprint(&schema1), schema_fingerprint(&nullable));
    }

    #[rstest]
    fn current_schema_passes_through() {
        let batch = RecordBatch::new_empty(Arc::new(QuoteTick::get_schema(None)));
        let result = transcode_legacy_record_batch("quotes", "quotes.parquet", batch).unwrap();

        assert_eq!(result.kind, LegacyTranscodeKind::PassThrough);
        assert_eq!(result.batches.len(), 1);
    }

    #[rstest]
    fn parquet_schema_with_plain_dictionary_strings_passes_through() {
        let schema = schema_without_identifier_column(&schema_with_plain_dictionary_strings(
            &TradeTick::get_schema(None),
        ));
        let batch = RecordBatch::new_empty(Arc::new(schema));
        let result = transcode_legacy_record_batch("trades", "trades.parquet", batch).unwrap();

        assert_eq!(result.kind, LegacyTranscodeKind::PassThrough);
        assert_eq!(result.batches.len(), 1);
    }

    #[rstest]
    fn unknown_registered_schema_is_rejected() {
        let batch = RecordBatch::new_empty(Arc::new(Schema::new(vec![Field::new(
            "unexpected",
            DataType::UInt64,
            false,
        )])));
        let error = transcode_legacy_record_batch("quotes", "quotes.parquet", batch).unwrap_err();

        assert!(matches!(error, LegacyArrowError::UnknownSchema { .. }));
        assert!(error.to_string().contains("quotes.parquet"));
    }

    #[rstest]
    fn instrument_status_fields_are_reordered() {
        let schema = Arc::new(legacy_instrument_status_schema());
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec!["AAPL.XNAS"])),
                Arc::new(StringArray::from(vec!["TRADING"])),
                Arc::new(StringArray::from(vec![Some("Normal")])),
                Arc::new(StringArray::from(vec![Some("OPEN")])),
                Arc::new(BooleanArray::from(vec![Some(true)])),
                Arc::new(BooleanArray::from(vec![Some(true)])),
                Arc::new(BooleanArray::from(vec![Some(false)])),
                Arc::new(UInt64Array::from(vec![1])),
                Arc::new(UInt64Array::from(vec![2])),
            ],
        )
        .unwrap();

        let result =
            transcode_legacy_record_batch("instrument_status", "status.parquet", batch).unwrap();
        let output = &result.batches[0];

        assert_eq!(result.kind, LegacyTranscodeKind::InstrumentStatusV1);
        assert_eq!(
            output
                .schema()
                .fields()
                .iter()
                .map(|field| field.name().as_str())
                .collect::<Vec<_>>(),
            vec![
                "instrument_id",
                "action",
                "ts_event",
                "ts_init",
                "reason",
                "trading_event",
                "is_trading",
                "is_quoting",
                "is_short_sell_restricted",
                "identifier",
            ]
        );
        let decoded =
            InstrumentStatus::decode_typed_batch(output.schema().metadata(), output.clone())
                .unwrap();
        assert_eq!(decoded[0].action, MarketStatusAction::Trading);
        assert_eq!(decoded[0].instrument_id.to_string(), "AAPL.XNAS");
        assert_eq!(decoded[0].ts_event.as_u64(), 1);
        assert_eq!(decoded[0].ts_init.as_u64(), 2);
    }

    #[rstest]
    fn funding_rate_is_decoded_and_interval_is_widened() {
        let schema = Arc::new(Schema::new_with_metadata(
            legacy_funding_rate_schema()
                .fields()
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            HashMap::from([(
                KEY_INSTRUMENT_ID.to_string(),
                "BTCUSDT-PERP.BINANCE".to_string(),
            )]),
        ));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(BinaryArray::from(vec![b"\"0.0001\"".as_slice()])),
                Arc::new(UInt16Array::from(vec![Some(480)])),
                Arc::new(UInt64Array::from(vec![Some(9)])),
                Arc::new(UInt64Array::from(vec![1])),
                Arc::new(UInt64Array::from(vec![2])),
            ],
        )
        .unwrap();

        let result =
            transcode_legacy_record_batch("funding_rates", "funding.parquet", batch).unwrap();
        let output = &result.batches[0];
        let decoded =
            FundingRateUpdate::decode_typed_batch(output.schema().metadata(), output.clone())
                .unwrap();

        assert_eq!(result.kind, LegacyTranscodeKind::FundingRateUpdateV1);
        assert_eq!(decoded[0].instrument_id.to_string(), "BTCUSDT-PERP.BINANCE");
        assert_eq!(decoded[0].rate.to_string(), "0.0001");
        assert_eq!(decoded[0].interval, Some(480));
        assert_eq!(
            decoded[0].next_funding_ns.map(|value| value.as_u64()),
            Some(9)
        );
    }

    #[rstest]
    fn instrument_close_is_split_and_decoded() {
        let schema = Arc::new(legacy_instrument_close_schema());
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(vec![
                    "AUD/USD.SIM",
                    "GBP/USD.SIM",
                    "AUD/USD.SIM",
                ])),
                Arc::new(StringArray::from(vec![
                    "END_OF_SESSION",
                    "CONTRACT_EXPIRED",
                    "END_OF_SESSION",
                ])),
                Arc::new(StringArray::from(vec!["1.0500", "2.1000", "1.0600"])),
                Arc::new(UInt64Array::from(vec![1, 2, 3])),
                Arc::new(UInt64Array::from(vec![4, 5, 6])),
            ],
        )
        .unwrap();

        let result =
            transcode_legacy_record_batch("instrument_closes", "close.parquet", batch).unwrap();
        let decoded = result
            .batches
            .iter()
            .flat_map(|batch| {
                InstrumentClose::decode_batch(batch.schema().metadata(), batch.clone()).unwrap()
            })
            .collect::<Vec<_>>();

        assert_eq!(result.kind, LegacyTranscodeKind::InstrumentCloseV1);
        assert_eq!(result.batches.len(), 2);
        assert_eq!(
            decoded
                .iter()
                .map(|close| close.instrument_id.to_string())
                .collect::<Vec<_>>(),
            vec!["AUD/USD.SIM", "AUD/USD.SIM", "GBP/USD.SIM"]
        );
        assert_eq!(
            decoded
                .iter()
                .map(|close| close.close_price.to_string())
                .collect::<Vec<_>>(),
            vec!["1.0500", "1.0600", "2.1000"]
        );
        assert_eq!(
            decoded
                .iter()
                .map(|close| close.close_type)
                .collect::<Vec<_>>(),
            vec![
                InstrumentCloseType::EndOfSession,
                InstrumentCloseType::EndOfSession,
                InstrumentCloseType::ContractExpired,
            ]
        );
    }

    #[rstest]
    fn instrument_close_rejects_mixed_precision() {
        let batch = RecordBatch::try_new(
            Arc::new(legacy_instrument_close_schema()),
            vec![
                Arc::new(StringArray::from(vec!["AUD/USD.SIM", "AUD/USD.SIM"])),
                Arc::new(StringArray::from(vec!["END_OF_SESSION", "END_OF_SESSION"])),
                Arc::new(StringArray::from(vec!["1.0500", "1.060"])),
                Arc::new(UInt64Array::from(vec![1, 2])),
                Arc::new(UInt64Array::from(vec![3, 4])),
            ],
        )
        .unwrap();

        let error =
            transcode_legacy_record_batch("instrument_closes", "close.parquet", batch).unwrap_err();

        assert!(error.to_string().contains("close.parquet"));
        assert!(error.to_string().contains("precision conflict at row 1"));
    }

    #[rstest]
    fn instrument_close_rejects_unknown_close_type() {
        let batch = RecordBatch::try_new(
            Arc::new(legacy_instrument_close_schema()),
            vec![
                Arc::new(StringArray::from(vec!["AUD/USD.SIM"])),
                Arc::new(StringArray::from(vec!["UNKNOWN"])),
                Arc::new(StringArray::from(vec!["1.0500"])),
                Arc::new(UInt64Array::from(vec![1])),
                Arc::new(UInt64Array::from(vec![2])),
            ],
        )
        .unwrap();

        let error =
            transcode_legacy_record_batch("instrument_closes", "close.parquet", batch).unwrap_err();

        assert!(error.to_string().contains("close.parquet"));
        assert!(
            error
                .to_string()
                .contains("unknown close_type \"UNKNOWN\" at row 0")
        );
    }
}
