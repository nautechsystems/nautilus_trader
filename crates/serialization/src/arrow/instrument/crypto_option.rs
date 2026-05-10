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

//! Arrow serialization for CryptoOption instruments.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{
        BinaryArray, BinaryBuilder, BooleanArray, BooleanBuilder, StringArray, StringBuilder,
        UInt8Array, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_core::Params;
use nautilus_model::{
    enums::OptionKind,
    identifiers::{InstrumentId, Symbol},
    instruments::crypto_option::CryptoOption,
    types::{money::Money, price::Price, quantity::Quantity},
};
#[allow(unused)]
use rust_decimal::Decimal;
#[allow(unused)]
use serde_json::Value;

use crate::arrow::{
    ArrowSchemaProvider, EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION, extract_column,
};

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

impl ArrowSchemaProvider for CryptoOption {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("raw_symbol", DataType::Utf8, false),
            Field::new("underlying", DataType::Utf8, false),
            Field::new("quote_currency", DataType::Utf8, false),
            Field::new("settlement_currency", DataType::Utf8, false),
            Field::new("is_inverse", DataType::Boolean, false),
            Field::new("option_kind", DataType::Utf8, false),
            Field::new("strike_price", DataType::Utf8, false),
            Field::new("activation_ns", DataType::UInt64, false),
            Field::new("expiration_ns", DataType::UInt64, false),
            Field::new("price_precision", DataType::UInt8, false),
            Field::new("size_precision", DataType::UInt8, false),
            Field::new("price_increment", DataType::Utf8, false),
            Field::new("size_increment", DataType::Utf8, false),
            Field::new("multiplier", DataType::Utf8, false),
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
            Field::new("info", DataType::Binary, true), // nullable
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let mut final_metadata = HashMap::new();
        final_metadata.insert("class".to_string(), "CryptoOption".to_string());

        if let Some(meta) = metadata {
            final_metadata.extend(meta);
        }

        Schema::new_with_metadata(fields, final_metadata)
    }
}

impl EncodeToRecordBatch for CryptoOption {
    fn encode_batch(
        #[allow(unused)] metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut id_builder = StringBuilder::new();
        let mut raw_symbol_builder = StringBuilder::new();
        let mut underlying_builder = StringBuilder::new();
        let mut quote_currency_builder = StringBuilder::new();
        let mut settlement_currency_builder = StringBuilder::new();
        let mut is_inverse_builder = BooleanBuilder::new();
        let mut option_kind_builder = StringBuilder::new();
        let mut strike_price_builder = StringBuilder::new();
        let mut activation_ns_builder = UInt64Array::builder(data.len());
        let mut expiration_ns_builder = UInt64Array::builder(data.len());
        let mut price_precision_builder = UInt8Array::builder(data.len());
        let mut size_precision_builder = UInt8Array::builder(data.len());
        let mut price_increment_builder = StringBuilder::new();
        let mut size_increment_builder = StringBuilder::new();
        let mut multiplier_builder = StringBuilder::new();
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
        let mut info_builder = BinaryBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for co in data {
            id_builder.append_value(co.id.to_string());
            raw_symbol_builder.append_value(co.raw_symbol);
            underlying_builder.append_value(co.underlying.to_string());
            quote_currency_builder.append_value(co.quote_currency.to_string());
            settlement_currency_builder.append_value(co.settlement_currency.to_string());
            is_inverse_builder.append_value(co.is_inverse);
            option_kind_builder.append_value(option_kind_to_string(co.option_kind));
            strike_price_builder.append_value(co.strike_price.to_string());
            activation_ns_builder.append_value(co.activation_ns.as_u64());
            expiration_ns_builder.append_value(co.expiration_ns.as_u64());
            price_precision_builder.append_value(co.price_precision);
            size_precision_builder.append_value(co.size_precision);
            price_increment_builder.append_value(co.price_increment.to_string());
            size_increment_builder.append_value(co.size_increment.to_string());
            multiplier_builder.append_value(co.multiplier.to_string());

            if let Some(max_qty) = co.max_quantity {
                max_quantity_builder.append_value(max_qty.to_string());
            } else {
                max_quantity_builder.append_null();
            }

            if let Some(min_qty) = co.min_quantity {
                min_quantity_builder.append_value(min_qty.to_string());
            } else {
                min_quantity_builder.append_null();
            }

            if let Some(max_not) = co.max_notional {
                max_notional_builder.append_value(max_not.to_string());
            } else {
                max_notional_builder.append_null();
            }

            if let Some(min_not) = co.min_notional {
                min_notional_builder.append_value(min_not.to_string());
            } else {
                min_notional_builder.append_null();
            }

            if let Some(max_p) = co.max_price {
                max_price_builder.append_value(max_p.to_string());
            } else {
                max_price_builder.append_null();
            }

            if let Some(min_p) = co.min_price {
                min_price_builder.append_value(min_p.to_string());
            } else {
                min_price_builder.append_null();
            }

            margin_init_builder.append_value(co.margin_init.to_string());
            margin_maint_builder.append_value(co.margin_maint.to_string());
            maker_fee_builder.append_value(co.maker_fee.to_string());
            taker_fee_builder.append_value(co.taker_fee.to_string());

            // Encode info dict as JSON bytes (matching Python's msgspec.json.encode)
            if let Some(ref info) = co.info {
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

            ts_event_builder.append_value(co.ts_event.as_u64());
            ts_init_builder.append_value(co.ts_init.as_u64());
        }

        let mut final_metadata = metadata.clone();
        final_metadata.insert("class".to_string(), "CryptoOption".to_string());

        RecordBatch::try_new(
            Self::get_schema(Some(final_metadata)).into(),
            vec![
                Arc::new(id_builder.finish()),
                Arc::new(raw_symbol_builder.finish()),
                Arc::new(underlying_builder.finish()),
                Arc::new(quote_currency_builder.finish()),
                Arc::new(settlement_currency_builder.finish()),
                Arc::new(is_inverse_builder.finish()),
                Arc::new(option_kind_builder.finish()),
                Arc::new(strike_price_builder.finish()),
                Arc::new(activation_ns_builder.finish()),
                Arc::new(expiration_ns_builder.finish()),
                Arc::new(price_precision_builder.finish()),
                Arc::new(size_precision_builder.finish()),
                Arc::new(price_increment_builder.finish()),
                Arc::new(size_increment_builder.finish()),
                Arc::new(multiplier_builder.finish()),
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

/// Helper function to decode CryptoOption from RecordBatch
/// (Cannot implement DecodeFromRecordBatch trait due to `Into<Data>` bound)
///
/// # Errors
///
/// Returns an `EncodingError` if the RecordBatch cannot be decoded.
pub fn decode_crypto_option_batch(
    #[allow(unused)] metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<Vec<CryptoOption>, EncodingError> {
    let cols = record_batch.columns();
    let num_rows = record_batch.num_rows();

    let id_values = extract_column::<StringArray>(cols, "id", 0, DataType::Utf8)?;
    let raw_symbol_values = extract_column::<StringArray>(cols, "raw_symbol", 1, DataType::Utf8)?;
    let underlying_values = extract_column::<StringArray>(cols, "underlying", 2, DataType::Utf8)?;
    let quote_currency_values =
        extract_column::<StringArray>(cols, "quote_currency", 3, DataType::Utf8)?;
    let settlement_currency_values =
        extract_column::<StringArray>(cols, "settlement_currency", 4, DataType::Utf8)?;
    let is_inverse_values =
        extract_column::<BooleanArray>(cols, "is_inverse", 5, DataType::Boolean)?;
    let option_kind_values = extract_column::<StringArray>(cols, "option_kind", 6, DataType::Utf8)?;
    let strike_price_values =
        extract_column::<StringArray>(cols, "strike_price", 7, DataType::Utf8)?;
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
    let max_quantity_values = cols
        .get(15)
        .ok_or_else(|| EncodingError::MissingColumn("max_quantity", 15))?;
    let min_quantity_values = cols
        .get(16)
        .ok_or_else(|| EncodingError::MissingColumn("min_quantity", 16))?;
    let max_notional_values = cols
        .get(17)
        .ok_or_else(|| EncodingError::MissingColumn("max_notional", 17))?;
    let min_notional_values = cols
        .get(18)
        .ok_or_else(|| EncodingError::MissingColumn("min_notional", 18))?;
    let max_price_values = cols
        .get(19)
        .ok_or_else(|| EncodingError::MissingColumn("max_price", 19))?;
    let min_price_values = cols
        .get(20)
        .ok_or_else(|| EncodingError::MissingColumn("min_price", 20))?;
    let margin_init_values =
        extract_column::<StringArray>(cols, "margin_init", 21, DataType::Utf8)?;
    let margin_maint_values =
        extract_column::<StringArray>(cols, "margin_maint", 22, DataType::Utf8)?;
    let maker_fee_values = extract_column::<StringArray>(cols, "maker_fee", 23, DataType::Utf8)?;
    let taker_fee_values = extract_column::<StringArray>(cols, "taker_fee", 24, DataType::Utf8)?;
    let info_values = cols
        .get(25)
        .ok_or_else(|| EncodingError::MissingColumn("info", 25))?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 26, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 27, DataType::UInt64)?;

    let mut result = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let id = InstrumentId::from_str(id_values.value(i))
            .map_err(|e| EncodingError::ParseError("id", format!("row {i}: {e}")))?;
        let raw_symbol = Symbol::from(raw_symbol_values.value(i));
        let underlying = super::decode_currency(
            underlying_values.value(i),
            "underlying",
            "crypto_option.underlying",
            i,
        )?;
        let quote_currency = super::decode_currency(
            quote_currency_values.value(i),
            "quote_currency",
            "crypto_option.quote_currency",
            i,
        )?;
        let settlement_currency = super::decode_currency(
            settlement_currency_values.value(i),
            "settlement_currency",
            "crypto_option.settlement_currency",
            i,
        )?;
        let is_inverse = is_inverse_values.value(i);
        let option_kind = option_kind_from_str(option_kind_values.value(i))?;
        let strike_price = Price::from_str(strike_price_values.value(i))
            .map_err(|e| EncodingError::ParseError("strike_price", format!("row {i}: {e}")))?;
        let activation_ns = nautilus_core::UnixNanos::from(activation_ns_values.value(i));
        let expiration_ns = nautilus_core::UnixNanos::from(expiration_ns_values.value(i));
        let price_prec = price_precision_values.value(i);
        let size_prec = size_precision_values.value(i);

        let price_increment = Price::from_str(price_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("price_increment", format!("row {i}: {e}")))?;
        let size_increment = Quantity::from_str(size_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("size_increment", format!("row {i}: {e}")))?;
        let multiplier = Quantity::from_str(multiplier_values.value(i))
            .map_err(|e| EncodingError::ParseError("multiplier", format!("row {i}: {e}")))?;

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

        let max_notional =
            if max_notional_values.is_null(i) {
                None
            } else {
                let max_not_str = max_notional_values
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| {
                        EncodingError::ParseError("max_notional", format!("row {i}: invalid type"))
                    })?
                    .value(i);
                Some(Money::from_str(max_not_str).map_err(|e| {
                    EncodingError::ParseError("max_notional", format!("row {i}: {e}"))
                })?)
            };

        let min_notional =
            if min_notional_values.is_null(i) {
                None
            } else {
                let min_not_str = min_notional_values
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(|| {
                        EncodingError::ParseError("min_notional", format!("row {i}: invalid type"))
                    })?
                    .value(i);
                Some(Money::from_str(min_not_str).map_err(|e| {
                    EncodingError::ParseError("min_notional", format!("row {i}: {e}"))
                })?)
            };

        let max_price = if max_price_values.is_null(i) {
            None
        } else {
            let max_p_str = max_price_values
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    EncodingError::ParseError("max_price", format!("row {i}: invalid type"))
                })?
                .value(i);
            Some(
                Price::from_str(max_p_str)
                    .map_err(|e| EncodingError::ParseError("max_price", format!("row {i}: {e}")))?,
            )
        };

        let min_price = if min_price_values.is_null(i) {
            None
        } else {
            let min_p_str = min_price_values
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    EncodingError::ParseError("min_price", format!("row {i}: invalid type"))
                })?
                .value(i);
            Some(
                Price::from_str(min_p_str)
                    .map_err(|e| EncodingError::ParseError("min_price", format!("row {i}: {e}")))?,
            )
        };

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

        let crypto_option = CryptoOption::new(
            id,
            raw_symbol,
            underlying,
            quote_currency,
            settlement_currency,
            is_inverse,
            option_kind,
            strike_price,
            activation_ns,
            expiration_ns,
            price_prec,
            size_prec,
            price_increment,
            size_increment,
            Some(multiplier),
            None, // lot_size - not in Python schema, will default to 1
            max_quantity,
            min_quantity,
            max_notional,
            min_notional,
            max_price,
            min_price,
            Some(margin_init),
            Some(margin_maint),
            Some(maker_fee),
            Some(taker_fee),
            info,
            ts_event,
            ts_init,
        );

        result.push(crypto_option);
    }

    Ok(result)
}
