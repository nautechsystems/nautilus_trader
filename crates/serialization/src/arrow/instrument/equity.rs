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

//! Arrow serialization for Equity instruments.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{BinaryArray, BinaryBuilder, StringArray, StringBuilder, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_core::Params;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::equity::Equity,
    types::{currency::Currency, price::Price, quantity::Quantity},
};
#[allow(unused)]
use rust_decimal::Decimal;
#[allow(unused)]
use serde_json::Value;
use ustr::Ustr;

use crate::arrow::{
    ArrowSchemaProvider, EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, extract_column,
};

impl ArrowSchemaProvider for Equity {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("raw_symbol", DataType::Utf8, false),
            Field::new("currency", DataType::Utf8, false),
            Field::new("price_precision", DataType::UInt8, false),
            Field::new("price_increment", DataType::Utf8, false),
            Field::new("lot_size", DataType::Utf8, true), // nullable
            Field::new("isin", DataType::Utf8, true),     // nullable
            Field::new("max_quantity", DataType::Utf8, true), // nullable
            Field::new("min_quantity", DataType::Utf8, true), // nullable
            Field::new("max_price", DataType::Utf8, true), // nullable
            Field::new("min_price", DataType::Utf8, true), // nullable
            Field::new("margin_init", DataType::Utf8, false),
            Field::new("margin_maint", DataType::Utf8, false),
            Field::new("maker_fee", DataType::Utf8, false),
            Field::new("taker_fee", DataType::Utf8, false),
            Field::new("info", DataType::Binary, true), // nullable
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let mut final_metadata = HashMap::new();
        final_metadata.insert("class".to_string(), "Equity".to_string());

        if let Some(meta) = metadata {
            final_metadata.extend(meta);
        }

        Schema::new_with_metadata(fields, final_metadata)
    }
}

impl EncodeToRecordBatch for Equity {
    fn encode_batch(
        #[allow(unused)] metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut id_builder = StringBuilder::new();
        let mut raw_symbol_builder = StringBuilder::new();
        let mut currency_builder = StringBuilder::new();
        let mut price_precision_builder = UInt8Array::builder(data.len());
        let mut price_increment_builder = StringBuilder::new();
        let mut lot_size_builder = StringBuilder::new();
        let mut isin_builder = StringBuilder::new();
        let mut max_quantity_builder = StringBuilder::new();
        let mut min_quantity_builder = StringBuilder::new();
        let mut max_price_builder = StringBuilder::new();
        let mut min_price_builder = StringBuilder::new();
        let mut margin_init_builder = StringBuilder::new();
        let mut margin_maint_builder = StringBuilder::new();
        let mut maker_fee_builder = StringBuilder::new();
        let mut taker_fee_builder = StringBuilder::new();
        let mut info_builder = BinaryBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for equity in data {
            id_builder.append_value(equity.id.to_string());
            raw_symbol_builder.append_value(equity.raw_symbol);
            currency_builder.append_value(equity.currency.to_string());
            price_precision_builder.append_value(equity.price_precision);
            price_increment_builder.append_value(equity.price_increment.to_string());

            if let Some(lot_size) = equity.lot_size {
                lot_size_builder.append_value(lot_size.to_string());
            } else {
                lot_size_builder.append_null();
            }

            if let Some(isin) = equity.isin {
                isin_builder.append_value(isin);
            } else {
                isin_builder.append_null();
            }

            if let Some(max_qty) = equity.max_quantity {
                max_quantity_builder.append_value(max_qty.to_string());
            } else {
                max_quantity_builder.append_null();
            }

            if let Some(min_qty) = equity.min_quantity {
                min_quantity_builder.append_value(min_qty.to_string());
            } else {
                min_quantity_builder.append_null();
            }

            if let Some(max_p) = equity.max_price {
                max_price_builder.append_value(max_p.to_string());
            } else {
                max_price_builder.append_null();
            }

            if let Some(min_p) = equity.min_price {
                min_price_builder.append_value(min_p.to_string());
            } else {
                min_price_builder.append_null();
            }

            margin_init_builder.append_value(equity.margin_init.to_string());
            margin_maint_builder.append_value(equity.margin_maint.to_string());
            maker_fee_builder.append_value(equity.maker_fee.to_string());
            taker_fee_builder.append_value(equity.taker_fee.to_string());

            // Encode info dict as JSON bytes (matching Python's msgspec.json.encode)
            if let Some(ref info) = equity.info {
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

            ts_event_builder.append_value(equity.ts_event.as_u64());
            ts_init_builder.append_value(equity.ts_init.as_u64());
        }

        let mut final_metadata = metadata.clone();
        final_metadata.insert("class".to_string(), "Equity".to_string());

        RecordBatch::try_new(
            Self::get_schema(Some(final_metadata)).into(),
            vec![
                Arc::new(id_builder.finish()),
                Arc::new(raw_symbol_builder.finish()),
                Arc::new(currency_builder.finish()),
                Arc::new(price_precision_builder.finish()),
                Arc::new(price_increment_builder.finish()),
                Arc::new(lot_size_builder.finish()),
                Arc::new(isin_builder.finish()),
                Arc::new(max_quantity_builder.finish()),
                Arc::new(min_quantity_builder.finish()),
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
        metadata
    }
}

/// Helper function to decode Equity from RecordBatch
/// (Cannot implement DecodeFromRecordBatch trait due to `Into<Data>` bound)
///
/// # Errors
///
/// Returns an `EncodingError` if the RecordBatch cannot be decoded.
pub fn decode_equity_batch(
    #[allow(unused)] metadata: &HashMap<String, String>,
    record_batch: RecordBatch,
) -> Result<Vec<Equity>, EncodingError> {
    let cols = record_batch.columns();
    let num_rows = record_batch.num_rows();

    // Read precision from data columns (it's in the schema)
    let id_values = extract_column::<StringArray>(cols, "id", 0, DataType::Utf8)?;
    let raw_symbol_values = extract_column::<StringArray>(cols, "raw_symbol", 1, DataType::Utf8)?;
    let currency_values = extract_column::<StringArray>(cols, "currency", 2, DataType::Utf8)?;
    let price_precision_values =
        extract_column::<UInt8Array>(cols, "price_precision", 3, DataType::UInt8)?;
    let price_increment_values =
        extract_column::<StringArray>(cols, "price_increment", 4, DataType::Utf8)?;
    let lot_size_values = cols
        .get(5)
        .ok_or_else(|| EncodingError::MissingColumn("lot_size", 5))?;
    let isin_values = cols
        .get(6)
        .ok_or_else(|| EncodingError::MissingColumn("isin", 6))?;
    let max_quantity_values = cols
        .get(7)
        .ok_or_else(|| EncodingError::MissingColumn("max_quantity", 7))?;
    let min_quantity_values = cols
        .get(8)
        .ok_or_else(|| EncodingError::MissingColumn("min_quantity", 8))?;
    let max_price_values = cols
        .get(9)
        .ok_or_else(|| EncodingError::MissingColumn("max_price", 9))?;
    let min_price_values = cols
        .get(10)
        .ok_or_else(|| EncodingError::MissingColumn("min_price", 10))?;
    let margin_init_values =
        extract_column::<StringArray>(cols, "margin_init", 11, DataType::Utf8)?;
    let margin_maint_values =
        extract_column::<StringArray>(cols, "margin_maint", 12, DataType::Utf8)?;
    let maker_fee_values = extract_column::<StringArray>(cols, "maker_fee", 13, DataType::Utf8)?;
    let taker_fee_values = extract_column::<StringArray>(cols, "taker_fee", 14, DataType::Utf8)?;
    let info_values = cols
        .get(15)
        .ok_or_else(|| EncodingError::MissingColumn("info", 15))?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 16, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 17, DataType::UInt64)?;

    let mut result = Vec::with_capacity(num_rows);

    for i in 0..num_rows {
        let id = InstrumentId::from_str(id_values.value(i))
            .map_err(|e| EncodingError::ParseError("id", format!("row {i}: {e}")))?;
        let raw_symbol = Symbol::from(raw_symbol_values.value(i));
        let currency = Currency::from_str(currency_values.value(i))
            .map_err(|e| EncodingError::ParseError("currency", format!("row {i}: {e}")))?;
        let price_prec = price_precision_values.value(i);

        let price_increment = Price::from_str(price_increment_values.value(i))
            .map_err(|e| EncodingError::ParseError("price_increment", format!("row {i}: {e}")))?;

        let lot_size = if lot_size_values.is_null(i) {
            None
        } else {
            let lot_size_str = lot_size_values
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    EncodingError::ParseError("lot_size", format!("row {i}: invalid type"))
                })?
                .value(i);
            Some(
                Quantity::from_str(lot_size_str)
                    .map_err(|e| EncodingError::ParseError("lot_size", format!("row {i}: {e}")))?,
            )
        };

        let isin = if isin_values.is_null(i) {
            None
        } else {
            let isin_str = isin_values
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| EncodingError::ParseError("isin", format!("row {i}: invalid type")))?
                .value(i);
            Some(Ustr::from(isin_str))
        };

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

        let equity = Equity::new(
            id,
            raw_symbol,
            isin,
            currency,
            price_prec,
            price_increment,
            lot_size,
            max_quantity,
            min_quantity,
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

        result.push(equity);
    }

    Ok(result)
}
