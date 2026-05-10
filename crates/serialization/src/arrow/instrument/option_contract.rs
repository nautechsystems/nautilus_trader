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

//! Arrow serialization for OptionContract instruments.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{BinaryArray, BinaryBuilder, StringArray, StringBuilder, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_core::Params;
use nautilus_model::{
    enums::{AssetClass, OptionKind},
    identifiers::{InstrumentId, Symbol},
    instruments::option_contract::OptionContract,
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

// Helper function to convert OptionKind to string
fn option_kind_to_string(ok: OptionKind) -> String {
    match ok {
        OptionKind::Call => "Call".to_string(),
        OptionKind::Put => "Put".to_string(),
    }
}

// Helper function to parse OptionKind from string
fn option_kind_from_str(s: &str) -> Result<OptionKind, EncodingError> {
    match s {
        "Call" => Ok(OptionKind::Call),
        "Put" => Ok(OptionKind::Put),
        _ => Err(EncodingError::ParseError(
            "option_kind",
            format!("Unknown option kind: {s}"),
        )),
    }
}

impl ArrowSchemaProvider for OptionContract {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("raw_symbol", DataType::Utf8, false),
            Field::new("underlying", DataType::Utf8, false),
            Field::new("asset_class", DataType::Utf8, false),
            Field::new("exchange", DataType::Utf8, true), // nullable
            Field::new("option_kind", DataType::Utf8, false),
            Field::new("strike_price", DataType::Utf8, false),
            Field::new("currency", DataType::Utf8, false),
            Field::new("activation_ns", DataType::UInt64, false),
            Field::new("expiration_ns", DataType::UInt64, false),
            Field::new("price_precision", DataType::UInt8, false),
            Field::new("size_precision", DataType::UInt8, false),
            Field::new("price_increment", DataType::Utf8, false),
            Field::new("size_increment", DataType::Utf8, false),
            Field::new("multiplier", DataType::Utf8, false),
            Field::new("lot_size", DataType::Utf8, false),
            Field::new("margin_init", DataType::Utf8, false),
            Field::new("margin_maint", DataType::Utf8, false),
            Field::new("maker_fee", DataType::Utf8, false),
            Field::new("taker_fee", DataType::Utf8, false),
            Field::new("info", DataType::Binary, true), // nullable
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let mut final_metadata = HashMap::new();
        final_metadata.insert("class".to_string(), "OptionContract".to_string());

        if let Some(meta) = metadata {
            final_metadata.extend(meta);
        }

        Schema::new_with_metadata(fields, final_metadata)
    }
}

impl EncodeToRecordBatch for OptionContract {
    fn encode_batch(
        #[allow(unused)] metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut id_builder = StringBuilder::new();
        let mut raw_symbol_builder = StringBuilder::new();
        let mut underlying_builder = StringBuilder::new();
        let mut asset_class_builder = StringBuilder::new();
        let mut exchange_builder = StringBuilder::new();
        let mut option_kind_builder = StringBuilder::new();
        let mut strike_price_builder = StringBuilder::new();
        let mut currency_builder = StringBuilder::new();
        let mut activation_ns_builder = UInt64Array::builder(data.len());
        let mut expiration_ns_builder = UInt64Array::builder(data.len());
        let mut price_precision_builder = UInt8Array::builder(data.len());
        let mut size_precision_builder = UInt8Array::builder(data.len());
        let mut price_increment_builder = StringBuilder::new();
        let mut size_increment_builder = StringBuilder::new();
        let mut multiplier_builder = StringBuilder::new();
        let mut lot_size_builder = StringBuilder::new();
        let mut margin_init_builder = StringBuilder::new();
        let mut margin_maint_builder = StringBuilder::new();
        let mut maker_fee_builder = StringBuilder::new();
        let mut taker_fee_builder = StringBuilder::new();
        let mut info_builder = BinaryBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for oc in data {
            id_builder.append_value(oc.id.to_string());
            raw_symbol_builder.append_value(oc.raw_symbol);
            underlying_builder.append_value(oc.underlying);
            asset_class_builder.append_value(asset_class_to_string(oc.asset_class));

            if let Some(exchange) = oc.exchange {
                exchange_builder.append_value(exchange);
            } else {
                exchange_builder.append_null();
            }

            option_kind_builder.append_value(option_kind_to_string(oc.option_kind));
            strike_price_builder.append_value(oc.strike_price.to_string());
            currency_builder.append_value(oc.currency.to_string());
            activation_ns_builder.append_value(oc.activation_ns.as_u64());
            expiration_ns_builder.append_value(oc.expiration_ns.as_u64());
            price_precision_builder.append_value(oc.price_precision);
            size_precision_builder.append_value(oc.size_precision);
            price_increment_builder.append_value(oc.price_increment.to_string());
            size_increment_builder.append_value(oc.size_increment.to_string());
            multiplier_builder.append_value(oc.multiplier.to_string());
            lot_size_builder.append_value(oc.lot_size.to_string());
            margin_init_builder.append_value(oc.margin_init.to_string());
            margin_maint_builder.append_value(oc.margin_maint.to_string());
            maker_fee_builder.append_value(oc.maker_fee.to_string());
            taker_fee_builder.append_value(oc.taker_fee.to_string());

            // Encode info dict as JSON bytes (matching Python's msgspec.json.encode)
            if let Some(ref info) = oc.info {
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

            ts_event_builder.append_value(oc.ts_event.as_u64());
            ts_init_builder.append_value(oc.ts_init.as_u64());
        }

        let mut final_metadata = metadata.clone();
        final_metadata.insert("class".to_string(), "OptionContract".to_string());

        RecordBatch::try_new(
            Self::get_schema(Some(final_metadata)).into(),
            vec![
                Arc::new(id_builder.finish()),
                Arc::new(raw_symbol_builder.finish()),
                Arc::new(underlying_builder.finish()),
                Arc::new(asset_class_builder.finish()),
                Arc::new(exchange_builder.finish()),
                Arc::new(option_kind_builder.finish()),
                Arc::new(strike_price_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(activation_ns_builder.finish()),
                Arc::new(expiration_ns_builder.finish()),
                Arc::new(price_precision_builder.finish()),
                Arc::new(size_precision_builder.finish()),
                Arc::new(price_increment_builder.finish()),
                Arc::new(size_increment_builder.finish()),
                Arc::new(multiplier_builder.finish()),
                Arc::new(lot_size_builder.finish()),
                Arc::new(margin_init_builder.finish()),
                Arc::new(margin_maint_builder.finish()),
                Arc::new(maker_fee_builder.finish()),
                Arc::new(taker_fee_builder.finish()),
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

/// Helper function to decode OptionContract from RecordBatch
/// (Cannot implement DecodeFromRecordBatch trait due to `Into<Data>` bound)
///
/// # Errors
///
/// Returns an `EncodingError` if the RecordBatch cannot be decoded.
pub fn decode_option_contract_batch(
    #[allow(unused)] metadata: &HashMap<String, String>,
    record_batch: RecordBatch,
) -> Result<Vec<OptionContract>, EncodingError> {
    let cols = record_batch.columns();
    let num_rows = record_batch.num_rows();

    let id_values = extract_column::<StringArray>(cols, "id", 0, DataType::Utf8)?;
    let raw_symbol_values = extract_column::<StringArray>(cols, "raw_symbol", 1, DataType::Utf8)?;
    let underlying_values = extract_column::<StringArray>(cols, "underlying", 2, DataType::Utf8)?;
    let asset_class_values = extract_column::<StringArray>(cols, "asset_class", 3, DataType::Utf8)?;
    let exchange_values = cols
        .get(4)
        .ok_or_else(|| EncodingError::MissingColumn("exchange", 4))?;
    let option_kind_values = extract_column::<StringArray>(cols, "option_kind", 5, DataType::Utf8)?;
    let strike_price_values =
        extract_column::<StringArray>(cols, "strike_price", 6, DataType::Utf8)?;
    let currency_values = extract_column::<StringArray>(cols, "currency", 7, DataType::Utf8)?;
    let activation_ns_values =
        extract_column::<UInt64Array>(cols, "activation_ns", 8, DataType::UInt64)?;
    let expiration_ns_values =
        extract_column::<UInt64Array>(cols, "expiration_ns", 9, DataType::UInt64)?;
    let price_precision_values =
        extract_column::<UInt8Array>(cols, "price_precision", 10, DataType::UInt8)?;
    let size_precision_values =
        extract_column::<UInt8Array>(cols, "size_precision", 11, DataType::UInt8)?;
    let price_increment_values =
        extract_column::<StringArray>(cols, "price_increment", 12, DataType::Utf8)?;
    let size_increment_values =
        extract_column::<StringArray>(cols, "size_increment", 13, DataType::Utf8)?;
    let multiplier_values = extract_column::<StringArray>(cols, "multiplier", 14, DataType::Utf8)?;
    let lot_size_values = extract_column::<StringArray>(cols, "lot_size", 15, DataType::Utf8)?;
    let margin_init_values =
        extract_column::<StringArray>(cols, "margin_init", 16, DataType::Utf8)?;
    let margin_maint_values =
        extract_column::<StringArray>(cols, "margin_maint", 17, DataType::Utf8)?;
    let maker_fee_values = extract_column::<StringArray>(cols, "maker_fee", 18, DataType::Utf8)?;
    let taker_fee_values = extract_column::<StringArray>(cols, "taker_fee", 19, DataType::Utf8)?;
    let info_values = cols
        .get(20)
        .ok_or_else(|| EncodingError::MissingColumn("info", 20))?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 21, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 22, DataType::UInt64)?;

    let mut result = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let id = InstrumentId::from_str(id_values.value(i))
            .map_err(|e| EncodingError::ParseError("id", format!("row {i}: {e}")))?;
        let raw_symbol = Symbol::from(raw_symbol_values.value(i));
        let underlying = Ustr::from(underlying_values.value(i));
        let asset_class = asset_class_from_str(asset_class_values.value(i))?;

        let exchange = if exchange_values.is_null(i) {
            None
        } else {
            let exchange_str = exchange_values
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    EncodingError::ParseError("exchange", format!("row {i}: invalid type"))
                })?
                .value(i);
            Some(Ustr::from(exchange_str))
        };

        let option_kind = option_kind_from_str(option_kind_values.value(i))?;
        let strike_price = Price::from_str(strike_price_values.value(i))
            .map_err(|e| EncodingError::ParseError("strike_price", format!("row {i}: {e}")))?;
        let currency = Currency::from_str(currency_values.value(i))
            .map_err(|e| EncodingError::ParseError("currency", format!("row {i}: {e}")))?;
        let activation_ns = nautilus_core::UnixNanos::from(activation_ns_values.value(i));
        let expiration_ns = nautilus_core::UnixNanos::from(expiration_ns_values.value(i));
        let price_prec = price_precision_values.value(i);
        let _size_prec = size_precision_values.value(i); // Not used in constructor, set to default

        let price_increment = Price::from_str(price_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("price_increment", format!("row {i}: {e}")))?;
        let _size_increment = Quantity::from_str(size_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("size_increment", format!("row {i}: {e}")))?; // Not used in constructor, set to default
        let multiplier = Quantity::from_str(multiplier_values.value(i))
            .map_err(|e| EncodingError::ParseError("multiplier", format!("row {i}: {e}")))?;
        let lot_size = Quantity::from_str(lot_size_values.value(i))
            .map_err(|e| EncodingError::ParseError("lot_size", format!("row {i}: {e}")))?;

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

        let option_contract = OptionContract::new(
            id,
            raw_symbol,
            asset_class,
            exchange,
            underlying,
            option_kind,
            strike_price,
            currency,
            activation_ns,
            expiration_ns,
            price_prec,
            price_increment,
            multiplier,
            lot_size,
            None, // max_quantity - not in Python schema
            None, // min_quantity - not in Python schema
            None, // max_price - not in Python schema
            None, // min_price - not in Python schema
            Some(margin_init),
            Some(margin_maint),
            Some(maker_fee),
            Some(taker_fee),
            info,
            ts_event,
            ts_init,
        );

        result.push(option_contract);
    }

    Ok(result)
}
