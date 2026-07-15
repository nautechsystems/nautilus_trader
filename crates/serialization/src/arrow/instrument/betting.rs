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

//! Arrow serialization for BettingInstrument instruments.

use std::{collections::HashMap, str::FromStr, sync::Arc};

#[allow(unused_imports)]
use arrow::{
    array::{
        Array, BinaryArray, BinaryBuilder, Float64Array, Float64Builder, Int64Array, Int64Builder,
        StringArray, StringBuilder, UInt8Array, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
#[allow(unused_imports)]
use nautilus_core::Params;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::betting::BettingInstrument,
    types::{money::Money, price::Price, quantity::Quantity},
};
#[allow(unused)]
use rust_decimal::Decimal;
#[allow(unused)]
use serde_json::Value;
use ustr::Ustr;

use crate::arrow::{
    ArrowSchemaProvider, EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION, extract_column, extract_column_by_name_or_index,
    extract_optional_string_column_by_name, optional_ustr_value,
};

impl ArrowSchemaProvider for BettingInstrument {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("raw_symbol", DataType::Utf8, false),
            Field::new("venue_name", DataType::Utf8, false),
            Field::new("currency", DataType::Utf8, false),
            Field::new("event_type_id", DataType::UInt64, false),
            Field::new("event_type_name", DataType::Utf8, false),
            Field::new("competition_id", DataType::UInt64, false),
            Field::new("competition_name", DataType::Utf8, false),
            Field::new("event_id", DataType::UInt64, false),
            Field::new("event_name", DataType::Utf8, false),
            Field::new("event_country_code", DataType::Utf8, false),
            Field::new("event_open_date", DataType::UInt64, false),
            Field::new("betting_type", DataType::Utf8, false),
            Field::new("market_id", DataType::Utf8, false),
            Field::new("market_name", DataType::Utf8, false),
            Field::new("market_type", DataType::Utf8, false),
            Field::new("market_start_time", DataType::UInt64, false),
            Field::new("selection_id", DataType::UInt64, false),
            Field::new("selection_name", DataType::Utf8, false),
            Field::new("selection_handicap", DataType::Float64, false),
            Field::new("price_precision", DataType::UInt8, false),
            Field::new("size_precision", DataType::UInt8, false),
            Field::new("price_increment", DataType::Utf8, false),
            Field::new("size_increment", DataType::Utf8, false),
            Field::new("max_quantity", DataType::Utf8, true), // nullable
            Field::new("min_quantity", DataType::Utf8, true), // nullable
            Field::new("max_notional", DataType::Utf8, true), // nullable
            Field::new("min_notional", DataType::Utf8, true), // nullable
            Field::new("max_price", DataType::Utf8, true),    // nullable
            Field::new("min_price", DataType::Utf8, true),    // nullable
            Field::new("margin_init", DataType::Utf8, false),
            Field::new("margin_maint", DataType::Utf8, false),
            Field::new("maker_fee", DataType::Utf8, false),
            Field::new("taker_fee", DataType::Utf8, false),
            Field::new("tick_scheme", DataType::Utf8, true),
            Field::new("info", DataType::Binary, true), // nullable
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let mut final_metadata = HashMap::new();
        final_metadata.insert("class".to_string(), "BettingInstrument".to_string());

        if let Some(meta) = metadata {
            final_metadata.extend(meta);
        }

        Schema::new_with_metadata(fields, final_metadata)
    }
}

impl EncodeToRecordBatch for BettingInstrument {
    fn encode_batch(
        #[allow(unused)] metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut id_builder = StringBuilder::new();
        let mut raw_symbol_builder = StringBuilder::new();
        let mut venue_name_builder = StringBuilder::new();
        let mut currency_builder = StringBuilder::new();
        let mut event_type_id_builder = UInt64Array::builder(data.len());
        let mut event_type_name_builder = StringBuilder::new();
        let mut competition_id_builder = UInt64Array::builder(data.len());
        let mut competition_name_builder = StringBuilder::new();
        let mut event_id_builder = UInt64Array::builder(data.len());
        let mut event_name_builder = StringBuilder::new();
        let mut event_country_code_builder = StringBuilder::new();
        let mut event_open_date_builder = UInt64Array::builder(data.len());
        let mut betting_type_builder = StringBuilder::new();
        let mut market_id_builder = StringBuilder::new();
        let mut market_name_builder = StringBuilder::new();
        let mut market_type_builder = StringBuilder::new();
        let mut market_start_time_builder = UInt64Array::builder(data.len());
        let mut selection_id_builder = UInt64Array::builder(data.len());
        let mut selection_name_builder = StringBuilder::new();
        let mut selection_handicap_builder = Float64Array::builder(data.len());
        let mut price_precision_builder = UInt8Array::builder(data.len());
        let mut size_precision_builder = UInt8Array::builder(data.len());
        let mut price_increment_builder = StringBuilder::new();
        let mut size_increment_builder = StringBuilder::new();
        let mut max_quantity_builder = StringBuilder::new();
        let mut min_quantity_builder = StringBuilder::new();
        let mut max_notional_builder = StringBuilder::new();
        let mut min_notional_builder = StringBuilder::new();
        let mut max_price_builder = StringBuilder::new();
        let mut min_price_builder = StringBuilder::new();
        let mut margin_init_builder = StringBuilder::new();
        let mut margin_maint_builder = StringBuilder::new();
        let mut maker_fee_builder = StringBuilder::new();
        let mut taker_fee_builder = StringBuilder::new();
        let mut tick_scheme_builder = StringBuilder::new();
        let mut info_builder = BinaryBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for bi in data {
            id_builder.append_value(bi.id.to_string());
            raw_symbol_builder.append_value(bi.raw_symbol);
            // Extract venue_name from instrument_id (format: "SYMBOL.VENUE")
            let venue_name = bi.id.venue.to_string();
            venue_name_builder.append_value(venue_name);
            currency_builder.append_value(bi.currency.to_string());
            event_type_id_builder.append_value(bi.event_type_id);
            event_type_name_builder.append_value(bi.event_type_name);
            competition_id_builder.append_value(bi.competition_id);
            competition_name_builder.append_value(bi.competition_name);
            event_id_builder.append_value(bi.event_id);
            event_name_builder.append_value(bi.event_name);
            event_country_code_builder.append_value(bi.event_country_code);
            event_open_date_builder.append_value(bi.event_open_date.as_u64());
            betting_type_builder.append_value(bi.betting_type);
            market_id_builder.append_value(bi.market_id);
            market_name_builder.append_value(bi.market_name);
            market_type_builder.append_value(bi.market_type);
            market_start_time_builder.append_value(bi.market_start_time.as_u64());
            selection_id_builder.append_value(bi.selection_id);
            selection_name_builder.append_value(bi.selection_name);
            selection_handicap_builder.append_value(bi.selection_handicap);
            price_precision_builder.append_value(bi.price_precision);
            size_precision_builder.append_value(bi.size_precision);
            price_increment_builder.append_value(bi.price_increment.to_string());
            size_increment_builder.append_value(bi.size_increment.to_string());

            if let Some(max_quantity) = bi.max_quantity {
                max_quantity_builder.append_value(max_quantity.to_string());
            } else {
                max_quantity_builder.append_null();
            }

            if let Some(min_quantity) = bi.min_quantity {
                min_quantity_builder.append_value(min_quantity.to_string());
            } else {
                min_quantity_builder.append_null();
            }

            if let Some(max_notional) = bi.max_notional {
                max_notional_builder.append_value(max_notional.to_string());
            } else {
                max_notional_builder.append_null();
            }

            if let Some(min_notional) = bi.min_notional {
                min_notional_builder.append_value(min_notional.to_string());
            } else {
                min_notional_builder.append_null();
            }

            if let Some(max_price) = bi.max_price {
                max_price_builder.append_value(max_price.to_string());
            } else {
                max_price_builder.append_null();
            }

            if let Some(min_price) = bi.min_price {
                min_price_builder.append_value(min_price.to_string());
            } else {
                min_price_builder.append_null();
            }

            margin_init_builder.append_value(bi.margin_init.to_string());
            margin_maint_builder.append_value(bi.margin_maint.to_string());
            maker_fee_builder.append_value(bi.maker_fee.to_string());
            taker_fee_builder.append_value(bi.taker_fee.to_string());

            if let Some(tick_scheme) = bi.tick_scheme {
                tick_scheme_builder.append_value(tick_scheme);
            } else {
                tick_scheme_builder.append_null();
            }

            // Encode info dict as JSON bytes (matching Python's msgspec.json.encode)
            if let Some(ref info) = bi.info {
                match serde_json::to_vec(info) {
                    Ok(json_bytes) => {
                        info_builder.append_value(json_bytes);
                    }
                    Err(e) => {
                        return Err(ArrowError::InvalidArgumentError(format!(
                            "Failed to serialize info dict to JSON: {e}"
                        )));
                    }
                }
            } else {
                info_builder.append_null();
            }

            ts_event_builder.append_value(bi.ts_event.as_u64());
            ts_init_builder.append_value(bi.ts_init.as_u64());
        }

        let mut final_metadata = metadata.clone();
        final_metadata.insert("class".to_string(), "BettingInstrument".to_string());

        RecordBatch::try_new(
            Self::get_schema(Some(final_metadata)).into(),
            vec![
                Arc::new(id_builder.finish()),
                Arc::new(raw_symbol_builder.finish()),
                Arc::new(venue_name_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(event_type_id_builder.finish()),
                Arc::new(event_type_name_builder.finish()),
                Arc::new(competition_id_builder.finish()),
                Arc::new(competition_name_builder.finish()),
                Arc::new(event_id_builder.finish()),
                Arc::new(event_name_builder.finish()),
                Arc::new(event_country_code_builder.finish()),
                Arc::new(event_open_date_builder.finish()),
                Arc::new(betting_type_builder.finish()),
                Arc::new(market_id_builder.finish()),
                Arc::new(market_name_builder.finish()),
                Arc::new(market_type_builder.finish()),
                Arc::new(market_start_time_builder.finish()),
                Arc::new(selection_id_builder.finish()),
                Arc::new(selection_name_builder.finish()),
                Arc::new(selection_handicap_builder.finish()),
                Arc::new(price_precision_builder.finish()),
                Arc::new(size_precision_builder.finish()),
                Arc::new(price_increment_builder.finish()),
                Arc::new(size_increment_builder.finish()),
                Arc::new(max_quantity_builder.finish()),
                Arc::new(min_quantity_builder.finish()),
                Arc::new(max_notional_builder.finish()),
                Arc::new(min_notional_builder.finish()),
                Arc::new(max_price_builder.finish()),
                Arc::new(min_price_builder.finish()),
                Arc::new(margin_init_builder.finish()),
                Arc::new(margin_maint_builder.finish()),
                Arc::new(maker_fee_builder.finish()),
                Arc::new(taker_fee_builder.finish()),
                Arc::new(tick_scheme_builder.finish()),
                Arc::new(info_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert(KEY_INSTRUMENT_ID.to_string(), self.id.to_string());
        metadata.insert(
            KEY_PRICE_PRECISION.to_string(),
            self.price_precision.to_string(),
        );
        metadata.insert(
            KEY_SIZE_PRECISION.to_string(),
            self.size_precision.to_string(),
        );
        metadata
    }
}

/// Helper function to decode BettingInstrument from RecordBatch
/// (Cannot implement DecodeFromRecordBatch trait due to `Into<Data>` bound)
///
/// # Errors
///
/// Returns an `EncodingError` if the RecordBatch cannot be decoded.
pub fn decode_betting_instrument_batch(
    #[allow(unused)] metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<Vec<BettingInstrument>, EncodingError> {
    let cols = record_batch.columns();
    let num_rows = record_batch.num_rows();

    let id_values = extract_column::<StringArray>(cols, "id", 0, DataType::Utf8)?;
    let raw_symbol_values = extract_column::<StringArray>(cols, "raw_symbol", 1, DataType::Utf8)?;
    let _venue_name_values = extract_column::<StringArray>(cols, "venue_name", 2, DataType::Utf8)?; // Not used, extracted from id
    let currency_values = extract_column::<StringArray>(cols, "currency", 3, DataType::Utf8)?;
    let event_type_id_values =
        extract_column::<UInt64Array>(cols, "event_type_id", 4, DataType::UInt64)?;
    let event_type_name_values =
        extract_column::<StringArray>(cols, "event_type_name", 5, DataType::Utf8)?;
    let competition_id_values =
        extract_column::<UInt64Array>(cols, "competition_id", 6, DataType::UInt64)?;
    let competition_name_values =
        extract_column::<StringArray>(cols, "competition_name", 7, DataType::Utf8)?;
    let event_id_values = extract_column::<UInt64Array>(cols, "event_id", 8, DataType::UInt64)?;
    let event_name_values = extract_column::<StringArray>(cols, "event_name", 9, DataType::Utf8)?;
    let event_country_code_values =
        extract_column::<StringArray>(cols, "event_country_code", 10, DataType::Utf8)?;
    let event_open_date_values =
        extract_column::<UInt64Array>(cols, "event_open_date", 11, DataType::UInt64)?;
    let betting_type_values =
        extract_column::<StringArray>(cols, "betting_type", 12, DataType::Utf8)?;
    let market_id_values = extract_column::<StringArray>(cols, "market_id", 13, DataType::Utf8)?;
    let market_name_values =
        extract_column::<StringArray>(cols, "market_name", 14, DataType::Utf8)?;
    let market_type_values =
        extract_column::<StringArray>(cols, "market_type", 15, DataType::Utf8)?;
    let market_start_time_values =
        extract_column::<UInt64Array>(cols, "market_start_time", 16, DataType::UInt64)?;
    let selection_id_values =
        extract_column::<UInt64Array>(cols, "selection_id", 17, DataType::UInt64)?;
    let selection_name_values =
        extract_column::<StringArray>(cols, "selection_name", 18, DataType::Utf8)?;
    let selection_handicap_values =
        extract_column::<Float64Array>(cols, "selection_handicap", 19, DataType::Float64)?;
    let price_precision_values =
        extract_column::<UInt8Array>(cols, "price_precision", 20, DataType::UInt8)?;
    let size_precision_values =
        extract_column::<UInt8Array>(cols, "size_precision", 21, DataType::UInt8)?;
    let price_increment_values =
        extract_column::<StringArray>(cols, "price_increment", 22, DataType::Utf8)?;
    let size_increment_values =
        extract_column::<StringArray>(cols, "size_increment", 23, DataType::Utf8)?;
    let max_quantity_values = extract_optional_string_column_by_name(record_batch, "max_quantity")?;
    let min_quantity_values = extract_optional_string_column_by_name(record_batch, "min_quantity")?;
    let max_notional_values = extract_optional_string_column_by_name(record_batch, "max_notional")?;
    let min_notional_values = extract_optional_string_column_by_name(record_batch, "min_notional")?;
    let max_price_values = extract_optional_string_column_by_name(record_batch, "max_price")?;
    let min_price_values = extract_optional_string_column_by_name(record_batch, "min_price")?;
    let margin_init_values =
        extract_column::<StringArray>(cols, "margin_init", 30, DataType::Utf8)?;
    let margin_maint_values =
        extract_column::<StringArray>(cols, "margin_maint", 31, DataType::Utf8)?;
    let maker_fee_values = extract_column::<StringArray>(cols, "maker_fee", 32, DataType::Utf8)?;
    let taker_fee_values = extract_column::<StringArray>(cols, "taker_fee", 33, DataType::Utf8)?;
    let tick_scheme_values = extract_optional_string_column_by_name(record_batch, "tick_scheme")?;
    let info_values =
        extract_column_by_name_or_index::<BinaryArray>(record_batch, "info", 35, DataType::Binary)?;
    let ts_event_values = extract_column_by_name_or_index::<UInt64Array>(
        record_batch,
        "ts_event",
        36,
        DataType::UInt64,
    )?;
    let ts_init_values = extract_column_by_name_or_index::<UInt64Array>(
        record_batch,
        "ts_init",
        37,
        DataType::UInt64,
    )?;

    let mut result = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let id = InstrumentId::from_str(id_values.value(i))
            .map_err(|e| EncodingError::ParseError("id", format!("row {i}: {e}")))?;
        let raw_symbol = Symbol::from(raw_symbol_values.value(i));
        let currency = super::decode_currency(
            currency_values.value(i),
            "currency",
            "betting_instrument.currency",
            i,
        )?;
        let event_type_id = event_type_id_values.value(i);
        let event_type_name = Ustr::from(event_type_name_values.value(i));
        let competition_id = competition_id_values.value(i);
        let competition_name = Ustr::from(competition_name_values.value(i));
        let event_id = event_id_values.value(i);
        let event_name = Ustr::from(event_name_values.value(i));
        let event_country_code = Ustr::from(event_country_code_values.value(i));
        let event_open_date = nautilus_core::UnixNanos::from(event_open_date_values.value(i));
        let betting_type = Ustr::from(betting_type_values.value(i));
        let market_id = Ustr::from(market_id_values.value(i));
        let market_name = Ustr::from(market_name_values.value(i));
        let market_type = Ustr::from(market_type_values.value(i));
        let market_start_time = nautilus_core::UnixNanos::from(market_start_time_values.value(i));
        let selection_id = selection_id_values.value(i);
        let selection_name = Ustr::from(selection_name_values.value(i));
        let selection_handicap = selection_handicap_values.value(i);
        let price_prec = price_precision_values.value(i);
        let size_prec = size_precision_values.value(i);

        let price_increment = Price::from_str(price_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("price_increment", format!("row {i}: {e}")))?;
        let size_increment = Quantity::from_str(size_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("size_increment", format!("row {i}: {e}")))?;

        let margin_init = Decimal::from_str(margin_init_values.value(i))
            .map_err(|e| EncodingError::ParseError("margin_init", format!("row {i}: {e}")))?;
        let margin_maint = Decimal::from_str(margin_maint_values.value(i))
            .map_err(|e| EncodingError::ParseError("margin_maint", format!("row {i}: {e}")))?;
        let maker_fee = Decimal::from_str(maker_fee_values.value(i))
            .map_err(|e| EncodingError::ParseError("maker_fee", format!("row {i}: {e}")))?;
        let taker_fee = Decimal::from_str(taker_fee_values.value(i))
            .map_err(|e| EncodingError::ParseError("taker_fee", format!("row {i}: {e}")))?;

        // Decode info dict from JSON bytes (matching Python's msgspec.json.decode)
        let info = if info_values.is_null(i) {
            None
        } else {
            let info_bytes = info_values
                .as_any()
                .downcast_ref::<BinaryArray>()
                .ok_or_else(|| EncodingError::ParseError("info", format!("row {i}: invalid type")))?
                .value(i);

            match serde_json::from_slice::<Params>(info_bytes) {
                Ok(info_dict) => Some(info_dict),
                Err(e) => {
                    return Err(EncodingError::ParseError(
                        "info",
                        format!("row {i}: failed to deserialize JSON: {e}"),
                    ));
                }
            }
        };

        let ts_event = nautilus_core::UnixNanos::from(ts_event_values.value(i));
        let ts_init = nautilus_core::UnixNanos::from(ts_init_values.value(i));

        let tick_scheme = optional_ustr_value(tick_scheme_values, i);

        let max_notional = match max_notional_values {
            Some(column) if !column.is_null(i) => {
                Some(Money::from_str(column.value(i)).map_err(|e| {
                    EncodingError::ParseError("max_notional", format!("row {i}: {e}"))
                })?)
            }
            _ => None,
        };

        let min_notional = match min_notional_values {
            Some(column) if !column.is_null(i) => {
                Some(Money::from_str(column.value(i)).map_err(|e| {
                    EncodingError::ParseError("min_notional", format!("row {i}: {e}"))
                })?)
            }
            _ => None,
        };

        let betting_instrument = BettingInstrument::new_checked(
            id,
            raw_symbol,
            event_type_id,
            event_type_name,
            competition_id,
            competition_name,
            event_id,
            event_name,
            event_country_code,
            event_open_date,
            betting_type,
            market_id,
            market_name,
            market_type,
            market_start_time,
            selection_id,
            selection_name,
            selection_handicap,
            currency,
            price_prec,
            size_prec,
            price_increment,
            size_increment,
            super::optional_quantity_value(max_quantity_values, "max_quantity", i)?,
            super::optional_quantity_value(min_quantity_values, "min_quantity", i)?,
            max_notional,
            min_notional,
            super::optional_price_value(max_price_values, "max_price", i)?,
            super::optional_price_value(min_price_values, "min_price", i)?,
            Some(margin_init),
            Some(margin_maint),
            Some(maker_fee),
            Some(taker_fee),
            tick_scheme,
            info,
            ts_event,
            ts_init,
        )
        .map_err(|e| super::instrument_validation_error::<BettingInstrument>(i, e))?;

        result.push(betting_instrument);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use arrow::{array::UInt8Array, record_batch::RecordBatch};
    use nautilus_model::instruments::stubs::betting;
    use rstest::rstest;

    use super::*;
    use crate::arrow::EncodeToRecordBatch;

    const PRICE_PRECISION_COLUMN: usize = 20;
    const SIZE_PRECISION_COLUMN: usize = 21;

    fn betting_batch_with_precision(column_index: usize, precision: u8) -> RecordBatch {
        betting_batch_with_precision_values(column_index, &[precision])
    }

    fn betting_batch_with_precision_values(column_index: usize, precisions: &[u8]) -> RecordBatch {
        let instruments = vec![betting(); precisions.len()];
        let batch = BettingInstrument::encode_batch(&HashMap::new(), &instruments).unwrap();
        let mut columns = batch.columns().to_vec();
        columns[column_index] = Arc::new(UInt8Array::from(precisions.to_vec()));

        RecordBatch::try_new(batch.schema(), columns).unwrap()
    }

    #[rstest]
    fn decode_betting_instrument_invalid_price_precision_returns_error() {
        let batch = betting_batch_with_precision(PRICE_PRECISION_COLUMN, u8::MAX);
        let error = decode_betting_instrument_batch(&HashMap::new(), &batch).unwrap_err();

        match error {
            EncodingError::ParseError(field, message) => {
                assert_eq!(field, super::super::INSTRUMENT_VALIDATION_FIELD);
                assert!(message.starts_with("row 0:"));
                assert!(message.contains("price_increment"));
                assert!(message.contains("precision"));
            }
            _ => panic!("Expected instrument parse error, was: {error}"),
        }
    }

    #[rstest]
    fn decode_betting_instrument_invalid_second_row_precision_reports_row_index() {
        let batch = betting_batch_with_precision_values(PRICE_PRECISION_COLUMN, &[2, u8::MAX]);
        let error = decode_betting_instrument_batch(&HashMap::new(), &batch).unwrap_err();

        match error {
            EncodingError::ParseError(field, message) => {
                assert_eq!(field, super::super::INSTRUMENT_VALIDATION_FIELD);
                assert!(message.starts_with("row 1:"));
                assert!(message.contains("price_increment"));
                assert!(message.contains("precision"));
            }
            _ => panic!("Expected instrument parse error, was: {error}"),
        }
    }

    #[rstest]
    fn decode_betting_instrument_invalid_size_precision_returns_error() {
        let batch = betting_batch_with_precision(SIZE_PRECISION_COLUMN, u8::MAX);
        let error = decode_betting_instrument_batch(&HashMap::new(), &batch).unwrap_err();

        match error {
            EncodingError::ParseError(field, message) => {
                assert_eq!(field, super::super::INSTRUMENT_VALIDATION_FIELD);
                assert!(message.starts_with("row 0:"));
                assert!(message.contains("size_increment"));
                assert!(message.contains("precision"));
            }
            _ => panic!("Expected instrument parse error, was: {error}"),
        }
    }

    #[rstest]
    fn decode_betting_instrument_invalid_default_price_increment_returns_error() {
        let batch = betting_batch_with_precision(PRICE_PRECISION_COLUMN, 1);
        let error = decode_betting_instrument_batch(&HashMap::new(), &batch).unwrap_err();

        match error {
            EncodingError::ParseError(field, message) => {
                assert_eq!(field, super::super::INSTRUMENT_VALIDATION_FIELD);
                assert!(message.starts_with("row 0:"));
                assert!(message.contains("BettingInstrument"));
                assert!(message.contains("price_increment"));
            }
            _ => panic!("Expected instrument parse error, was: {error}"),
        }
    }
}
