// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    array::{FixedSizeBinaryArray, FixedSizeBinaryBuilder, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::prices::MarkPriceUpdate,
    identifiers::InstrumentId,
    types::{Price, fixed::PRECISION_BYTES},
};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION,
    extract_column,
};
use crate::arrow::{
    ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch, get_raw_price,
};

impl ArrowSchemaProvider for MarkPriceUpdate {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("value", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

fn parse_metadata(metadata: &HashMap<String, String>) -> Result<(InstrumentId, u8), EncodingError> {
    let instrument_id_str = metadata
        .get(KEY_INSTRUMENT_ID)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_INSTRUMENT_ID))?;
    let instrument_id = InstrumentId::from_str(instrument_id_str)
        .map_err(|e| EncodingError::ParseError(KEY_INSTRUMENT_ID, e.to_string()))?;

    let price_precision = metadata
        .get(KEY_PRICE_PRECISION)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_PRICE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_PRICE_PRECISION, e.to_string()))?;

    Ok((instrument_id, price_precision))
}

impl EncodeToRecordBatch for MarkPriceUpdate {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut value_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for update in data {
            value_builder
                .append_value(update.value.raw.to_le_bytes())
                .unwrap();
            ts_event_builder.append_value(update.ts_event.as_u64());
            ts_init_builder.append_value(update.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(value_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert(
            KEY_INSTRUMENT_ID.to_string(),
            self.instrument_id.to_string(),
        );
        metadata.insert(
            KEY_PRICE_PRECISION.to_string(),
            self.value.precision.to_string(),
        );
        metadata
    }
}

impl DecodeFromRecordBatch for MarkPriceUpdate {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        let value_values = extract_column::<FixedSizeBinaryArray>(
            cols,
            "value",
            0,
            DataType::FixedSizeBinary(PRECISION_BYTES),
        )?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 1, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 2, DataType::UInt64)?;

        if value_values.value_length() != PRECISION_BYTES {
            return Err(EncodingError::ParseError(
                "value",
                format!(
                    "Invalid value length: expected {PRECISION_BYTES}, found {}",
                    value_values.value_length()
                ),
            ));
        }

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                Ok(Self {
                    instrument_id,
                    value: Price::from_raw(get_raw_price(value_values.value(row)), price_precision),
                    ts_event: ts_event_values.value(row).into(),
                    ts_init: ts_init_values.value(row).into(),
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for MarkPriceUpdate {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let updates: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(updates.into_iter().map(Data::from).collect())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::{array::Array, record_batch::RecordBatch};
    use nautilus_model::types::price::PriceRaw;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::arrow::get_raw_price;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("BTC-USDT.BINANCE");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);
        let schema = MarkPriceUpdate::get_schema(Some(metadata.clone()));

        let expected_fields = vec![
            Field::new("value", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = MarkPriceUpdate::get_schema_map();
        let mut expected_map = HashMap::new();

        let fixed_size_binary = format!("FixedSizeBinary({PRECISION_BYTES})");
        expected_map.insert("value".to_string(), fixed_size_binary);
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("BTC-USDT.BINANCE");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let update1 = MarkPriceUpdate {
            instrument_id,
            value: Price::from("50200.00"),
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let update2 = MarkPriceUpdate {
            instrument_id,
            value: Price::from("50300.00"),
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![update1, update2];
        let record_batch = MarkPriceUpdate::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let value_values = columns[0]
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .unwrap();
        let ts_event_values = columns[1].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[2].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 3);
        assert_eq!(value_values.len(), 2);
        assert_eq!(
            get_raw_price(value_values.value(0)),
            Price::from(dec!(50200.00).to_string()).raw
        );
        assert_eq!(
            get_raw_price(value_values.value(1)),
            Price::from(dec!(50300.00).to_string()).raw
        );
        assert_eq!(ts_event_values.len(), 2);
        assert_eq!(ts_event_values.value(0), 1);
        assert_eq!(ts_event_values.value(1), 2);
        assert_eq!(ts_init_values.len(), 2);
        assert_eq!(ts_init_values.value(0), 3);
        assert_eq!(ts_init_values.value(1), 4);
    }

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("BTC-USDT.BINANCE");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let value = FixedSizeBinaryArray::from(vec![
            &(5020000 as PriceRaw).to_le_bytes(),
            &(5030000 as PriceRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            MarkPriceUpdate::get_schema(Some(metadata.clone())).into(),
            vec![Arc::new(value), Arc::new(ts_event), Arc::new(ts_init)],
        )
        .unwrap();

        let decoded_data = MarkPriceUpdate::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded_data.len(), 2);
        assert_eq!(decoded_data[0].instrument_id, instrument_id);
        assert_eq!(decoded_data[0].value, Price::from_raw(5020000, 2));
        assert_eq!(decoded_data[0].ts_event.as_u64(), 1);
        assert_eq!(decoded_data[0].ts_init.as_u64(), 3);

        assert_eq!(decoded_data[1].instrument_id, instrument_id);
        assert_eq!(decoded_data[1].value, Price::from_raw(5030000, 2));
        assert_eq!(decoded_data[1].ts_event.as_u64(), 2);
        assert_eq!(decoded_data[1].ts_init.as_u64(), 4);
    }
}
