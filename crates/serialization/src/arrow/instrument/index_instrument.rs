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

//! Arrow serialization for IndexInstrument instruments.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{BinaryArray, BinaryBuilder, StringArray, StringBuilder, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::index_instrument::IndexInstrument,
    types::{currency::Currency, price::Price, quantity::Quantity},
};
#[allow(unused)]
use rust_decimal::Decimal;
#[allow(unused)]
use serde_json::Value;

use crate::arrow::{
    ArrowSchemaProvider, EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, extract_column,
};

impl ArrowSchemaProvider for IndexInstrument {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("raw_symbol", DataType::Utf8, false),
            Field::new("currency", DataType::Utf8, false),
            Field::new("price_precision", DataType::UInt8, false),
            Field::new("price_increment", DataType::Utf8, false),
            Field::new("size_precision", DataType::UInt8, false),
            Field::new("size_increment", DataType::Utf8, false),
            Field::new("info", DataType::Binary, true), // nullable
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let mut final_metadata = HashMap::new();
        final_metadata.insert("class".to_string(), "IndexInstrument".to_string());

        if let Some(meta) = metadata {
            final_metadata.extend(meta);
        }

        Schema::new_with_metadata(fields, final_metadata)
    }
}

impl EncodeToRecordBatch for IndexInstrument {
    fn encode_batch(
        #[allow(unused)] metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut id_builder = StringBuilder::new();
        let mut raw_symbol_builder = StringBuilder::new();
        let mut currency_builder = StringBuilder::new();
        let mut price_precision_builder = UInt8Array::builder(data.len());
        let mut size_precision_builder = UInt8Array::builder(data.len());
        let mut price_increment_builder = StringBuilder::new();
        let mut size_increment_builder = StringBuilder::new();
        let mut info_builder = BinaryBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for index in data {
            id_builder.append_value(index.id.to_string());
            raw_symbol_builder.append_value(index.raw_symbol);
            currency_builder.append_value(index.currency.to_string());
            price_precision_builder.append_value(index.price_precision);
            price_increment_builder.append_value(index.price_increment.to_string());
            size_precision_builder.append_value(index.size_precision);
            size_increment_builder.append_value(index.size_increment.to_string());

            // Encode info dict as JSON bytes (matching Python's msgspec.json.encode)
            if let Some(ref info) = index.info {
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

            ts_event_builder.append_value(index.ts_event.as_u64());
            ts_init_builder.append_value(index.ts_init.as_u64());
        }

        let mut final_metadata = metadata.clone();
        final_metadata.insert("class".to_string(), "IndexInstrument".to_string());

        RecordBatch::try_new(
            Self::get_schema(Some(final_metadata)).into(),
            vec![
                Arc::new(id_builder.finish()),
                Arc::new(raw_symbol_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(price_precision_builder.finish()),
                Arc::new(price_increment_builder.finish()),
                Arc::new(size_precision_builder.finish()),
                Arc::new(size_increment_builder.finish()),
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
        metadata
    }
}

/// Helper function to decode IndexInstrument from RecordBatch
/// (Cannot implement DecodeFromRecordBatch trait due to `Into<Data>` bound)
///
/// # Errors
///
/// Returns an `EncodingError` if the RecordBatch cannot be decoded.
pub fn decode_index_instrument_batch(
    #[allow(unused)] metadata: &HashMap<String, String>,
    record_batch: RecordBatch,
) -> Result<Vec<IndexInstrument>, EncodingError> {
    let cols = record_batch.columns();
    let num_rows = record_batch.num_rows();

    let id_values = extract_column::<StringArray>(cols, "id", 0, DataType::Utf8)?;
    let raw_symbol_values = extract_column::<StringArray>(cols, "raw_symbol", 1, DataType::Utf8)?;
    let currency_values = extract_column::<StringArray>(cols, "currency", 2, DataType::Utf8)?;
    let price_precision_values =
        extract_column::<UInt8Array>(cols, "price_precision", 3, DataType::UInt8)?;
    let price_increment_values =
        extract_column::<StringArray>(cols, "price_increment", 4, DataType::Utf8)?;
    let size_precision_values =
        extract_column::<UInt8Array>(cols, "size_precision", 5, DataType::UInt8)?;
    let size_increment_values =
        extract_column::<StringArray>(cols, "size_increment", 6, DataType::Utf8)?;
    let info_values = cols
        .get(7)
        .ok_or_else(|| EncodingError::MissingColumn("info", 7))?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 8, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 9, DataType::UInt64)?;

    let mut result = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let id = InstrumentId::from_str(id_values.value(i))
            .map_err(|e| EncodingError::ParseError("id", format!("row {i}: {e}")))?;
        let raw_symbol = Symbol::from(raw_symbol_values.value(i));
        let currency = Currency::from_str(currency_values.value(i))
            .map_err(|e| EncodingError::ParseError("currency", format!("row {i}: {e}")))?;
        let price_prec = price_precision_values.value(i);
        let size_prec = size_precision_values.value(i);

        let price_increment = Price::from_str(price_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("price_increment", format!("row {i}: {e}")))?;
        let size_increment = Quantity::from_str(size_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("size_increment", format!("row {i}: {e}")))?;

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

        let ts_event = UnixNanos::from(ts_event_values.value(i));
        let ts_init = UnixNanos::from(ts_init_values.value(i));

        let index_instrument = IndexInstrument::new(
            id,
            raw_symbol,
            currency,
            price_prec,
            size_prec,
            price_increment,
            size_increment,
            info,
            ts_event,
            ts_init,
        );

        result.push(index_instrument);
    }

    Ok(result)
}
