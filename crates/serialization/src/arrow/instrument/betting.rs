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
        BinaryArray, BinaryBuilder, Float64Array, Float64Builder, Int64Array, Int64Builder,
        StringArray, StringBuilder, UInt8Array, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
#[allow(unused_imports)]
use nautilus_core::Params;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::betting::BettingInstrument,
    types::{price::Price, quantity::Quantity},
};
#[allow(unused)]
use rust_decimal::Decimal;
#[allow(unused)]
use serde_json::Value;
use ustr::Ustr;

use crate::arrow::{
    ArrowSchemaProvider, EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION, extract_column,
};

impl ArrowSchemaProvider for BettingInstrument {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("id", DataType::Utf8, false),
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
        let mut info_builder = BinaryBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for bi in data {
            id_builder.append_value(bi.id.to_string());
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
    let _venue_name_values = extract_column::<StringArray>(cols, "venue_name", 1, DataType::Utf8)?; // Not used, extracted from id
    let currency_values = extract_column::<StringArray>(cols, "currency", 2, DataType::Utf8)?;
    let event_type_id_values =
        extract_column::<UInt64Array>(cols, "event_type_id", 3, DataType::UInt64)?;
    let event_type_name_values =
        extract_column::<StringArray>(cols, "event_type_name", 4, DataType::Utf8)?;
    let competition_id_values =
        extract_column::<UInt64Array>(cols, "competition_id", 5, DataType::UInt64)?;
    let competition_name_values =
        extract_column::<StringArray>(cols, "competition_name", 6, DataType::Utf8)?;
    let event_id_values = extract_column::<UInt64Array>(cols, "event_id", 7, DataType::UInt64)?;
    let event_name_values = extract_column::<StringArray>(cols, "event_name", 8, DataType::Utf8)?;
    let event_country_code_values =
        extract_column::<StringArray>(cols, "event_country_code", 9, DataType::Utf8)?;
    let event_open_date_values =
        extract_column::<UInt64Array>(cols, "event_open_date", 10, DataType::UInt64)?;
    let betting_type_values =
        extract_column::<StringArray>(cols, "betting_type", 11, DataType::Utf8)?;
    let market_id_values = extract_column::<StringArray>(cols, "market_id", 12, DataType::Utf8)?;
    let market_name_values =
        extract_column::<StringArray>(cols, "market_name", 13, DataType::Utf8)?;
    let market_type_values =
        extract_column::<StringArray>(cols, "market_type", 14, DataType::Utf8)?;
    let market_start_time_values =
        extract_column::<UInt64Array>(cols, "market_start_time", 15, DataType::UInt64)?;
    let selection_id_values =
        extract_column::<UInt64Array>(cols, "selection_id", 16, DataType::UInt64)?;
    let selection_name_values =
        extract_column::<StringArray>(cols, "selection_name", 17, DataType::Utf8)?;
    let selection_handicap_values =
        extract_column::<Float64Array>(cols, "selection_handicap", 18, DataType::Float64)?;
    let price_precision_values =
        extract_column::<UInt8Array>(cols, "price_precision", 19, DataType::UInt8)?;
    let size_precision_values =
        extract_column::<UInt8Array>(cols, "size_precision", 20, DataType::UInt8)?;
    let info_values = cols
        .get(21)
        .ok_or_else(|| EncodingError::MissingColumn("info", 21))?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 22, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 23, DataType::UInt64)?;

    let mut result = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let id = InstrumentId::from_str(id_values.value(i))
            .map_err(|e| EncodingError::ParseError("id", format!("row {i}: {e}")))?;
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

        // Note: BettingInstrument requires price_increment and size_increment, but they're not in the Python schema
        // We'll need to use defaults or extract from price_precision/size_precision
        // For now, using minimal defaults based on precision
        let price_increment = Price::new(0.01, price_prec);
        let size_increment = Quantity::new(1.0, size_prec);

        // Extract raw_symbol from id's symbol component
        let raw_symbol = id.symbol;

        let betting_instrument = BettingInstrument::new(
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
            None, // max_quantity - not in Python schema
            None, // min_quantity - not in Python schema
            None, // max_notional - not in Python schema
            None, // min_notional - not in Python schema
            None, // max_price - not in Python schema
            None, // min_price - not in Python schema
            None, // margin_init - not in Python schema, will default to 1
            None, // margin_maint - not in Python schema, will default to 1
            None, // maker_fee - not in Python schema, will default to 0
            None, // taker_fee - not in Python schema, will default to 0
            info,
            ts_event,
            ts_init,
        );

        result.push(betting_instrument);
    }

    Ok(result)
}
