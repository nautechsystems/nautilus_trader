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

//! Arrow serialization for BinaryOption instruments.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{BinaryArray, BinaryBuilder, StringArray, StringBuilder, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
#[allow(unused_imports)]
use nautilus_core::Params;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::binary_option::BinaryOption,
    types::{currency::Currency, price::Price, quantity::Quantity},
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

// Helper function to convert AssetClass to string
fn asset_class_to_string(ac: AssetClass) -> String {
    match ac {
        AssetClass::FX => "FX".to_string(),
        AssetClass::Equity => "Equity".to_string(),
        AssetClass::Commodity => "Commodity".to_string(),
        AssetClass::Debt => "Debt".to_string(),
        AssetClass::Index => "Index".to_string(),
        AssetClass::Cryptocurrency => "Cryptocurrency".to_string(),
        AssetClass::Alternative => "Alternative".to_string(),
    }
}

// Helper function to parse AssetClass from string
fn asset_class_from_str(s: &str) -> Result<AssetClass, EncodingError> {
    match s {
        "FX" => Ok(AssetClass::FX),
        "Equity" => Ok(AssetClass::Equity),
        "Commodity" => Ok(AssetClass::Commodity),
        "Debt" => Ok(AssetClass::Debt),
        "Index" => Ok(AssetClass::Index),
        "Cryptocurrency" => Ok(AssetClass::Cryptocurrency),
        "Alternative" => Ok(AssetClass::Alternative),
        _ => Err(EncodingError::ParseError(
            "asset_class",
            format!("Unknown asset class: {s}"),
        )),
    }
}

impl ArrowSchemaProvider for BinaryOption {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("raw_symbol", DataType::Utf8, false),
            Field::new("asset_class", DataType::Utf8, false),
            Field::new("currency", DataType::Utf8, false),
            Field::new("price_precision", DataType::UInt8, false),
            Field::new("size_precision", DataType::UInt8, false),
            Field::new("price_increment", DataType::Utf8, false),
            Field::new("size_increment", DataType::Utf8, false),
            Field::new("activation_ns", DataType::UInt64, false),
            Field::new("expiration_ns", DataType::UInt64, false),
            Field::new("maker_fee", DataType::Utf8, false),
            Field::new("taker_fee", DataType::Utf8, false),
            Field::new("max_quantity", DataType::Utf8, true), // nullable
            Field::new("min_quantity", DataType::Utf8, true), // nullable
            Field::new("outcome", DataType::Utf8, true),      // nullable
            Field::new("description", DataType::Utf8, true),  // nullable
            Field::new("info", DataType::Binary, true),       // nullable
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let mut final_metadata = HashMap::new();
        final_metadata.insert("class".to_string(), "BinaryOption".to_string());

        if let Some(meta) = metadata {
            final_metadata.extend(meta);
        }

        Schema::new_with_metadata(fields, final_metadata)
    }
}

impl EncodeToRecordBatch for BinaryOption {
    fn encode_batch(
        #[allow(unused)] metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut id_builder = StringBuilder::new();
        let mut raw_symbol_builder = StringBuilder::new();
        let mut asset_class_builder = StringBuilder::new();
        let mut currency_builder = StringBuilder::new();
        let mut price_precision_builder = UInt8Array::builder(data.len());
        let mut size_precision_builder = UInt8Array::builder(data.len());
        let mut price_increment_builder = StringBuilder::new();
        let mut size_increment_builder = StringBuilder::new();
        let mut activation_ns_builder = UInt64Array::builder(data.len());
        let mut expiration_ns_builder = UInt64Array::builder(data.len());
        let mut maker_fee_builder = StringBuilder::new();
        let mut taker_fee_builder = StringBuilder::new();
        let mut max_quantity_builder = StringBuilder::new();
        let mut min_quantity_builder = StringBuilder::new();
        let mut outcome_builder = StringBuilder::new();
        let mut description_builder = StringBuilder::new();
        let mut info_builder = BinaryBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for bo in data {
            id_builder.append_value(bo.id.to_string());
            raw_symbol_builder.append_value(bo.raw_symbol);
            asset_class_builder.append_value(asset_class_to_string(bo.asset_class));
            currency_builder.append_value(bo.currency.to_string());
            price_precision_builder.append_value(bo.price_precision);
            size_precision_builder.append_value(bo.size_precision);
            price_increment_builder.append_value(bo.price_increment.to_string());
            size_increment_builder.append_value(bo.size_increment.to_string());
            activation_ns_builder.append_value(bo.activation_ns.as_u64());
            expiration_ns_builder.append_value(bo.expiration_ns.as_u64());
            maker_fee_builder.append_value(bo.maker_fee.to_string());
            taker_fee_builder.append_value(bo.taker_fee.to_string());

            if let Some(max_qty) = bo.max_quantity {
                max_quantity_builder.append_value(max_qty.to_string());
            } else {
                max_quantity_builder.append_null();
            }

            if let Some(min_qty) = bo.min_quantity {
                min_quantity_builder.append_value(min_qty.to_string());
            } else {
                min_quantity_builder.append_null();
            }

            if let Some(outcome) = bo.outcome {
                outcome_builder.append_value(outcome);
            } else {
                outcome_builder.append_null();
            }

            if let Some(desc) = bo.description {
                description_builder.append_value(desc);
            } else {
                description_builder.append_null();
            }

            // Encode info dict as JSON bytes (matching Python's msgspec.json.encode)
            if let Some(ref info) = bo.info {
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

            ts_event_builder.append_value(bo.ts_event.as_u64());
            ts_init_builder.append_value(bo.ts_init.as_u64());
        }

        let mut final_metadata = metadata.clone();
        final_metadata.insert("class".to_string(), "BinaryOption".to_string());

        RecordBatch::try_new(
            Self::get_schema(Some(final_metadata)).into(),
            vec![
                Arc::new(id_builder.finish()),
                Arc::new(raw_symbol_builder.finish()),
                Arc::new(asset_class_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(price_precision_builder.finish()),
                Arc::new(size_precision_builder.finish()),
                Arc::new(price_increment_builder.finish()),
                Arc::new(size_increment_builder.finish()),
                Arc::new(activation_ns_builder.finish()),
                Arc::new(expiration_ns_builder.finish()),
                Arc::new(maker_fee_builder.finish()),
                Arc::new(taker_fee_builder.finish()),
                Arc::new(max_quantity_builder.finish()),
                Arc::new(min_quantity_builder.finish()),
                Arc::new(outcome_builder.finish()),
                Arc::new(description_builder.finish()),
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

/// Helper function to decode BinaryOption from RecordBatch
/// (Cannot implement DecodeFromRecordBatch trait due to `Into<Data>` bound)
///
/// # Errors
///
/// Returns an `EncodingError` if the RecordBatch cannot be decoded.
pub fn decode_binary_option_batch(
    #[allow(unused)] metadata: &HashMap<String, String>,
    record_batch: RecordBatch,
) -> Result<Vec<BinaryOption>, EncodingError> {
    let cols = record_batch.columns();
    let num_rows = record_batch.num_rows();

    let id_values = extract_column::<StringArray>(cols, "id", 0, DataType::Utf8)?;
    let raw_symbol_values = extract_column::<StringArray>(cols, "raw_symbol", 1, DataType::Utf8)?;
    let asset_class_values = extract_column::<StringArray>(cols, "asset_class", 2, DataType::Utf8)?;
    let currency_values = extract_column::<StringArray>(cols, "currency", 3, DataType::Utf8)?;
    let price_precision_values =
        extract_column::<UInt8Array>(cols, "price_precision", 4, DataType::UInt8)?;
    let size_precision_values =
        extract_column::<UInt8Array>(cols, "size_precision", 5, DataType::UInt8)?;
    let price_increment_values =
        extract_column::<StringArray>(cols, "price_increment", 6, DataType::Utf8)?;
    let size_increment_values =
        extract_column::<StringArray>(cols, "size_increment", 7, DataType::Utf8)?;
    let activation_ns_values =
        extract_column::<UInt64Array>(cols, "activation_ns", 8, DataType::UInt64)?;
    let expiration_ns_values =
        extract_column::<UInt64Array>(cols, "expiration_ns", 9, DataType::UInt64)?;
    let maker_fee_values = extract_column::<StringArray>(cols, "maker_fee", 10, DataType::Utf8)?;
    let taker_fee_values = extract_column::<StringArray>(cols, "taker_fee", 11, DataType::Utf8)?;
    let max_quantity_values = cols
        .get(12)
        .ok_or_else(|| EncodingError::MissingColumn("max_quantity", 12))?;
    let min_quantity_values = cols
        .get(13)
        .ok_or_else(|| EncodingError::MissingColumn("min_quantity", 13))?;
    let outcome_values = cols
        .get(14)
        .ok_or_else(|| EncodingError::MissingColumn("outcome", 14))?;
    let description_values = cols
        .get(15)
        .ok_or_else(|| EncodingError::MissingColumn("description", 15))?;
    let info_values = cols
        .get(16)
        .ok_or_else(|| EncodingError::MissingColumn("info", 16))?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 17, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 18, DataType::UInt64)?;

    let mut result = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let id = InstrumentId::from_str(id_values.value(i))
            .map_err(|e| EncodingError::ParseError("id", format!("row {i}: {e}")))?;
        let raw_symbol = Symbol::from(raw_symbol_values.value(i));
        let asset_class = asset_class_from_str(asset_class_values.value(i))?;
        let currency = Currency::from_str(currency_values.value(i))
            .map_err(|e| EncodingError::ParseError("currency", format!("row {i}: {e}")))?;
        let price_prec = price_precision_values.value(i);
        let size_prec = size_precision_values.value(i);

        let price_increment = Price::from_str(price_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("price_increment", format!("row {i}: {e}")))?;
        let size_increment = Quantity::from_str(size_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("size_increment", format!("row {i}: {e}")))?;

        let activation_ns = nautilus_core::UnixNanos::from(activation_ns_values.value(i));
        let expiration_ns = nautilus_core::UnixNanos::from(expiration_ns_values.value(i));

        let maker_fee = Decimal::from_str(maker_fee_values.value(i))
            .map_err(|e| EncodingError::ParseError("maker_fee", format!("row {i}: {e}")))?;
        let taker_fee = Decimal::from_str(taker_fee_values.value(i))
            .map_err(|e| EncodingError::ParseError("taker_fee", format!("row {i}: {e}")))?;

        let max_quantity =
            if max_quantity_values.is_null(i) {
                None
            } else {
                let max_qty_str = max_quantity_values
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| {
                        EncodingError::ParseError("max_quantity", format!("row {i}: invalid type"))
                    })?
                    .value(i);
                Some(Quantity::from_str(max_qty_str).map_err(|e| {
                    EncodingError::ParseError("max_quantity", format!("row {i}: {e}"))
                })?)
            };

        let min_quantity =
            if min_quantity_values.is_null(i) {
                None
            } else {
                let min_qty_str = min_quantity_values
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| {
                        EncodingError::ParseError("min_quantity", format!("row {i}: invalid type"))
                    })?
                    .value(i);
                Some(Quantity::from_str(min_qty_str).map_err(|e| {
                    EncodingError::ParseError("min_quantity", format!("row {i}: {e}"))
                })?)
            };

        let outcome = if outcome_values.is_null(i) {
            None
        } else {
            let outcome_str = outcome_values
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    EncodingError::ParseError("outcome", format!("row {i}: invalid type"))
                })?
                .value(i);
            Some(Ustr::from(outcome_str))
        };

        let description = if description_values.is_null(i) {
            None
        } else {
            let desc_str = description_values
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    EncodingError::ParseError("description", format!("row {i}: invalid type"))
                })?
                .value(i);
            Some(Ustr::from(desc_str))
        };

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

        let binary_option = BinaryOption::new(
            id,
            raw_symbol,
            asset_class,
            currency,
            activation_ns,
            expiration_ns,
            price_prec,
            size_prec,
            price_increment,
            size_increment,
            outcome,
            description,
            max_quantity,
            min_quantity,
            None, // max_notional - not in Python schema
            None, // min_notional - not in Python schema
            None, // max_price - not in Python schema
            None, // min_price - not in Python schema
            None, // margin_init - not in Python schema
            None, // margin_maint - not in Python schema
            Some(maker_fee),
            Some(taker_fee),
            info,
            ts_event,
            ts_init,
        );

        result.push(binary_option);
    }

    Ok(result)
}
