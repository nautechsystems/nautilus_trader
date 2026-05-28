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

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{Array, Float64Array, Float64Builder, StringBuilder, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{Data, greeks::OptionGreekValues, option_chain::OptionGreeks},
    enums::GreeksConvention,
    identifiers::InstrumentId,
};

use super::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, DecodeFromRecordBatch, EncodeToRecordBatch,
    EncodingError, KEY_INSTRUMENT_ID, extract_column, extract_column_string,
};

const TYPE_NAME: &str = "OptionGreeks";

impl ArrowSchemaProvider for OptionGreeks {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
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
        ];

        let mut metadata = metadata.unwrap_or_default();
        metadata.insert("type".to_string(), TYPE_NAME.to_string());
        Schema::new_with_metadata(fields, metadata)
    }
}

fn append_optional_f64(builder: &mut Float64Builder, value: Option<f64>) {
    match value {
        Some(value) => builder.append_value(value),
        None => builder.append_null(),
    }
}

fn optional_f64(values: &Float64Array, row: usize) -> Option<f64> {
    if values.is_null(row) {
        None
    } else {
        Some(values.value(row))
    }
}

impl EncodeToRecordBatch for OptionGreeks {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut instrument_id_builder = StringBuilder::new();
        let mut delta_builder = Float64Builder::new();
        let mut gamma_builder = Float64Builder::new();
        let mut vega_builder = Float64Builder::new();
        let mut theta_builder = Float64Builder::new();
        let mut rho_builder = Float64Builder::new();
        let mut mark_iv_builder = Float64Builder::new();
        let mut bid_iv_builder = Float64Builder::new();
        let mut ask_iv_builder = Float64Builder::new();
        let mut underlying_price_builder = Float64Builder::new();
        let mut open_interest_builder = Float64Builder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());
        let mut convention_builder = StringBuilder::new();

        for greeks in data {
            instrument_id_builder.append_value(greeks.instrument_id.to_string());
            delta_builder.append_value(greeks.delta);
            gamma_builder.append_value(greeks.gamma);
            vega_builder.append_value(greeks.vega);
            theta_builder.append_value(greeks.theta);
            rho_builder.append_value(greeks.rho);
            append_optional_f64(&mut mark_iv_builder, greeks.mark_iv);
            append_optional_f64(&mut bid_iv_builder, greeks.bid_iv);
            append_optional_f64(&mut ask_iv_builder, greeks.ask_iv);
            append_optional_f64(&mut underlying_price_builder, greeks.underlying_price);
            append_optional_f64(&mut open_interest_builder, greeks.open_interest);
            ts_event_builder.append_value(greeks.ts_event.as_u64());
            ts_init_builder.append_value(greeks.ts_init.as_u64());
            convention_builder.append_value(greeks.convention);
        }

        RecordBatch::try_new(
            Arc::new(Self::get_schema(Some(metadata.clone()))),
            vec![
                Arc::new(instrument_id_builder.finish()),
                Arc::new(delta_builder.finish()),
                Arc::new(gamma_builder.finish()),
                Arc::new(vega_builder.finish()),
                Arc::new(theta_builder.finish()),
                Arc::new(rho_builder.finish()),
                Arc::new(mark_iv_builder.finish()),
                Arc::new(bid_iv_builder.finish()),
                Arc::new(ask_iv_builder.finish()),
                Arc::new(underlying_price_builder.finish()),
                Arc::new(open_interest_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
                Arc::new(convention_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        HashMap::from([
            ("type".to_string(), TYPE_NAME.to_string()),
            (
                KEY_INSTRUMENT_ID.to_string(),
                self.instrument_id.to_string(),
            ),
        ])
    }
}

impl DecodeFromRecordBatch for OptionGreeks {
    fn decode_batch(
        _metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let cols = record_batch.columns();

        let instrument_id_values = extract_column_string(cols, "instrument_id", 0)?;
        let delta_values = extract_column::<Float64Array>(cols, "delta", 1, DataType::Float64)?;
        let gamma_values = extract_column::<Float64Array>(cols, "gamma", 2, DataType::Float64)?;
        let vega_values = extract_column::<Float64Array>(cols, "vega", 3, DataType::Float64)?;
        let theta_values = extract_column::<Float64Array>(cols, "theta", 4, DataType::Float64)?;
        let rho_values = extract_column::<Float64Array>(cols, "rho", 5, DataType::Float64)?;
        let mark_iv_values = extract_column::<Float64Array>(cols, "mark_iv", 6, DataType::Float64)?;
        let bid_iv_values = extract_column::<Float64Array>(cols, "bid_iv", 7, DataType::Float64)?;
        let ask_iv_values = extract_column::<Float64Array>(cols, "ask_iv", 8, DataType::Float64)?;
        let underlying_price_values =
            extract_column::<Float64Array>(cols, "underlying_price", 9, DataType::Float64)?;
        let open_interest_values =
            extract_column::<Float64Array>(cols, "open_interest", 10, DataType::Float64)?;
        let ts_event_values =
            extract_column::<UInt64Array>(cols, "ts_event", 11, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 12, DataType::UInt64)?;
        let convention_values = extract_column_string(cols, "convention", 13)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                let instrument_id = InstrumentId::from_str(instrument_id_values.value(row))
                    .map_err(|e| EncodingError::ParseError("instrument_id", e.to_string()))?;
                let convention = GreeksConvention::from_str(convention_values.value(row))
                    .map_err(|e| EncodingError::ParseError("convention", e.to_string()))?;

                Ok(Self {
                    instrument_id,
                    convention,
                    greeks: OptionGreekValues {
                        delta: delta_values.value(row),
                        gamma: gamma_values.value(row),
                        vega: vega_values.value(row),
                        theta: theta_values.value(row),
                        rho: rho_values.value(row),
                    },
                    mark_iv: optional_f64(mark_iv_values, row),
                    bid_iv: optional_f64(bid_iv_values, row),
                    ask_iv: optional_f64(ask_iv_values, row),
                    underlying_price: optional_f64(underlying_price_values, row),
                    open_interest: optional_f64(open_interest_values, row),
                    ts_event: ts_event_values.value(row).into(),
                    ts_init: ts_init_values.value(row).into(),
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for OptionGreeks {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let greeks = Self::decode_batch(metadata, record_batch)?;
        Ok(greeks.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{enums::GreeksConvention, identifiers::InstrumentId};
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("BTC-20260529-100000-C.OKX");
        let original = vec![
            OptionGreeks {
                instrument_id,
                convention: GreeksConvention::BlackScholes,
                greeks: OptionGreekValues {
                    delta: 0.55,
                    gamma: 0.012,
                    vega: 3.4,
                    theta: -1.2,
                    rho: 0.01,
                },
                mark_iv: Some(0.64),
                bid_iv: Some(0.62),
                ask_iv: Some(0.66),
                underlying_price: Some(100_000.0),
                open_interest: Some(42.0),
                ts_event: 1.into(),
                ts_init: 2.into(),
            },
            OptionGreeks {
                instrument_id,
                convention: GreeksConvention::PriceAdjusted,
                greeks: OptionGreekValues {
                    delta: 0.42,
                    gamma: 0.009,
                    vega: 2.9,
                    theta: -0.9,
                    rho: 0.02,
                },
                mark_iv: None,
                bid_iv: None,
                ask_iv: None,
                underlying_price: None,
                open_interest: None,
                ts_event: 3.into(),
                ts_init: 4.into(),
            },
        ];

        let metadata = original[0].metadata();
        let record_batch = OptionGreeks::encode_batch(&metadata, &original).unwrap();
        let decoded = OptionGreeks::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded, original);
    }
}
