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

//! Legacy Python Arrow schema detection and conversion to current Rust schemas.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::{Display, Write},
    str::FromStr,
    sync::Arc,
};

use arrow::{
    array::{
        Array, ArrayRef, BinaryArray, StringArray, StringBuilder, UInt8Builder, UInt16Array,
        UInt64Array, UInt64Builder, new_null_array,
    },
    compute::cast,
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick, TradeTick,
    },
    enums::InstrumentCloseType,
    types::Price,
};

use super::{
    ArrowSchemaProvider, KEY_IDENTIFIER, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION,
    normalized_legacy_data_type, price_decimal_array, schema_without_identifier_column,
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
    use crate::arrow::{DecodeFromRecordBatch, DecodeTypedFromRecordBatch};

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
