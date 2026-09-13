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
    array::{Decimal128Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{data::prices::IndexPriceUpdate, identifiers::InstrumentId};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_IDENTIFIER, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, decode_decimal_price, decode_required_timestamp, extract_column,
    fixed_decimal_data_type, identifier_array_from_display, price_decimal_array,
};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for IndexPriceUpdate {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("value", fixed_decimal_data_type(), true),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
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

impl EncodeToRecordBatch for IndexPriceUpdate {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for update in data.iter().map(std::borrow::Borrow::borrow) {
            ts_event_builder.append_value(update.ts_event.as_u64());
            ts_init_builder.append_value(update.ts_init.as_u64());
        }

        crate::arrow::record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|update| update.value.raw()),
                    "value",
                )?),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
                Arc::new(identifier_array_from_display(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|update| update.instrument_id),
                )),
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

impl DecodeFromRecordBatch for IndexPriceUpdate {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision) = parse_metadata(metadata)?;
        let record_batch = crate::arrow::record_batch_with_u64_timestamps(&record_batch)?;
        let record_batch = &record_batch;
        let cols = record_batch.columns();

        let value_values =
            extract_column::<Decimal128Array>(cols, "value", 0, fixed_decimal_data_type())?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 1, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 2, DataType::UInt64)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                let value = decode_decimal_price(value_values, price_precision, "value", row)?;
                Ok(Self {
                    instrument_id,
                    value,
                    ts_event: decode_required_timestamp(ts_event_values, "ts_event", row)?,
                    ts_init: decode_required_timestamp(ts_init_values, "ts_init", row)?,
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for IndexPriceUpdate {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let updates: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(updates.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Array, TimestampNanosecondArray};
    use nautilus_model::types::{Price, fixed::FIXED_SCALAR, price::PriceRaw};
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
        let schema = IndexPriceUpdate::get_schema(Some(metadata.clone()));

        let expected_fields = vec![
            Field::new("value", fixed_decimal_data_type(), true),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
        ];

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = IndexPriceUpdate::get_schema_map();
        let mut expected_map = HashMap::new();

        let fixed_size_binary = "Decimal128(38, 16)".to_string();
        expected_map.insert("value".to_string(), fixed_size_binary);
        expected_map.insert(
            "ts_event".to_string(),
            "Timestamp(Nanosecond, Some(\"UTC\"))".to_string(),
        );
        expected_map.insert(
            "ts_init".to_string(),
            "Timestamp(Nanosecond, Some(\"UTC\"))".to_string(),
        );
        expected_map.insert(KEY_IDENTIFIER.to_string(), "Utf8".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("BTC-USDT.BINANCE");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let update1 = IndexPriceUpdate {
            instrument_id,
            value: Price::from("50000.00"),
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let update2 = IndexPriceUpdate {
            instrument_id,
            value: Price::from("51000.00"),
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![update1, update2];
        let record_batch = IndexPriceUpdate::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let value_values = columns[0]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let ts_event_values = columns[1]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_values = columns[2]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(columns.len(), 4);
        assert_eq!(value_values.len(), 2);
        assert_eq!(
            get_raw_price(value_values.value(0)),
            Price::from(dec!(50000.00).to_string()).raw()
        );
        assert_eq!(
            get_raw_price(value_values.value(1)),
            Price::from(dec!(51000.00).to_string()).raw()
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

        let raw_price1 = (50.00 * FIXED_SCALAR) as PriceRaw;
        let raw_price2 = (51.00 * FIXED_SCALAR) as PriceRaw;
        let value = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &raw_price1.to_le_bytes(),
            &raw_price2.to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&IndexPriceUpdate::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![Arc::new(value), Arc::new(ts_event), Arc::new(ts_init)],
        )
        .unwrap();

        let decoded_data = IndexPriceUpdate::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded_data.len(), 2);
        assert_eq!(decoded_data[0].instrument_id, instrument_id);
        assert_eq!(decoded_data[0].value, Price::from_raw(raw_price1, 2));
        assert_eq!(decoded_data[0].ts_event.as_u64(), 1);
        assert_eq!(decoded_data[0].ts_init.as_u64(), 3);

        assert_eq!(decoded_data[1].instrument_id, instrument_id);
        assert_eq!(decoded_data[1].value, Price::from_raw(raw_price2, 2));
        assert_eq!(decoded_data[1].ts_event.as_u64(), 2);
        assert_eq!(decoded_data[1].ts_init.as_u64(), 4);
    }

    #[rstest]
    fn test_decode_batch_rejects_null_timestamp_with_field_and_row() {
        let instrument_id = InstrumentId::from("BTC-USDT.BINANCE");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);
        let update = IndexPriceUpdate {
            instrument_id,
            value: Price::from("50000.00"),
            ts_event: 1.into(),
            ts_init: 2.into(),
        };
        let encoded = IndexPriceUpdate::encode_batch(&metadata, &[update]).unwrap();
        let mut columns = encoded.columns().to_vec();
        columns[2] = Arc::new(TimestampNanosecondArray::from(vec![None]).with_timezone("UTC"));
        let fields = encoded
            .schema()
            .fields()
            .iter()
            .map(|field| {
                if field.name() == "ts_init" {
                    Arc::new(field.as_ref().clone().with_nullable(true))
                } else {
                    field.clone()
                }
            })
            .collect::<Vec<_>>();
        let schema = Arc::new(Schema::new_with_metadata(fields, metadata.clone()));
        let batch = RecordBatch::try_new(schema, columns).unwrap();

        let error = IndexPriceUpdate::decode_batch(&metadata, batch).unwrap_err();

        assert!(error.to_string().contains("ts_init"));
        assert!(error.to_string().contains("row 0"));
    }

    #[rstest]
    fn test_decode_batch_invalid_value_returns_error() {
        let instrument_id = InstrumentId::from("BTC-USDT.BINANCE");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let invalid_price: PriceRaw = PriceRaw::MAX - 1000;
        let value = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &invalid_price.to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&IndexPriceUpdate::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![Arc::new(value), Arc::new(ts_event), Arc::new(ts_init)],
        )
        .unwrap();

        let result = IndexPriceUpdate::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("value") && err.to_string().contains("row 0"),
            "Expected value error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_instrument_id_returns_error() {
        let mut metadata = HashMap::from([
            (
                KEY_INSTRUMENT_ID.to_string(),
                "BTC-USDT.BINANCE".to_string(),
            ),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let raw_price = (50.00 * FIXED_SCALAR) as PriceRaw;
        let value =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&IndexPriceUpdate::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![Arc::new(value), Arc::new(ts_event), Arc::new(ts_init)],
        )
        .unwrap();

        metadata.remove(KEY_INSTRUMENT_ID);

        let result = IndexPriceUpdate::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("instrument_id"),
            "Expected missing instrument_id error, was: {err}"
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("BTC-USDT.BINANCE");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let update1 = IndexPriceUpdate {
            instrument_id,
            value: Price::from("50000.00"),
            ts_event: 1_000_000_000.into(),
            ts_init: 1_000_000_001.into(),
        };

        let update2 = IndexPriceUpdate {
            instrument_id,
            value: Price::from("51000.00"),
            ts_event: 2_000_000_000.into(),
            ts_init: 2_000_000_001.into(),
        };

        let original = vec![update1, update2];
        let record_batch = IndexPriceUpdate::encode_batch(&metadata, &original).unwrap();
        let decoded = IndexPriceUpdate::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.value, orig.value);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
