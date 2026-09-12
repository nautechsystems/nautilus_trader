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
use nautilus_model::{
    data::close::InstrumentClose, enums::InstrumentCloseType, identifiers::InstrumentId,
};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_IDENTIFIER, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, decode_decimal_price, decode_required_timestamp, enum_dictionary_array,
    enum_dictionary_data_type, extract_column, extract_column_string, fixed_decimal_data_type,
    identifier_array_from_display, price_decimal_array,
};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for InstrumentClose {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("close_price", fixed_decimal_data_type(), true),
            Field::new("close_type", enum_dictionary_data_type(), false),
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

impl EncodeToRecordBatch for InstrumentClose {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for item in data.iter().map(std::borrow::Borrow::borrow) {
            ts_event_builder.append_value(item.ts_event.as_u64());
            ts_init_builder.append_value(item.ts_init.as_u64());
        }

        crate::arrow::record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|item| item.close_price.raw()),
                    "close_price",
                )?),
                Arc::new(enum_dictionary_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|item| item.close_type),
                )?),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
                Arc::new(identifier_array_from_display(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|item| item.instrument_id),
                )),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(&self.instrument_id, self.close_price.precision)
    }
}

impl DecodeFromRecordBatch for InstrumentClose {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision) = parse_metadata(metadata)?;
        let record_batch = crate::arrow::record_batch_with_u64_timestamps(&record_batch)?;
        let record_batch = &record_batch;
        let cols = record_batch.columns();

        let close_price_values =
            extract_column::<Decimal128Array>(cols, "close_price", 0, fixed_decimal_data_type())?;
        let close_type_values = extract_column_string(cols, "close_type", 1)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 2, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 3, DataType::UInt64)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                let close_price =
                    decode_decimal_price(close_price_values, price_precision, "close_price", row)?;
                let close_type_value = close_type_values.value(row);
                let close_type = InstrumentCloseType::from_str(close_type_value).map_err(|e| {
                    EncodingError::ParseError(stringify!(InstrumentCloseType), e.to_string())
                })?;
                Ok(Self {
                    instrument_id,
                    close_price,
                    close_type,
                    ts_event: decode_required_timestamp(ts_event_values, "ts_event", row)?,
                    ts_init: decode_required_timestamp(ts_init_values, "ts_init", row)?,
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for InstrumentClose {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let items: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(items.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Array, TimestampNanosecondArray};
    use nautilus_model::types::{Price, fixed::FIXED_SCALAR, price::PriceRaw};
    use rstest::rstest;

    use super::*;
    use crate::arrow::get_raw_price;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);
        let schema = InstrumentClose::get_schema(Some(metadata.clone()));

        let expected_fields = vec![
            Field::new("close_price", fixed_decimal_data_type(), true),
            Field::new("close_type", enum_dictionary_data_type(), false),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
        ];

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = InstrumentClose::get_schema_map();
        let mut expected_map = HashMap::new();

        let fixed_size_binary = "Decimal128(38, 16)".to_string();
        expected_map.insert("close_price".to_string(), fixed_size_binary);
        expected_map.insert(
            "close_type".to_string(),
            "Dictionary(Int8, Utf8)".to_string(),
        );
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
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let close1 = InstrumentClose {
            instrument_id,
            close_price: Price::from("150.50"),
            close_type: InstrumentCloseType::EndOfSession,
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let close2 = InstrumentClose {
            instrument_id,
            close_price: Price::from("151.25"),
            close_type: InstrumentCloseType::ContractExpired,
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![close1, close2];
        let record_batch = InstrumentClose::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let close_price_values = columns[0]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let close_type_values = extract_column_string(columns, "close_type", 1).unwrap();
        let ts_event_values = columns[2]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_values = columns[3]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(columns.len(), 5);
        assert_eq!(close_price_values.len(), 2);
        assert_eq!(
            get_raw_price(close_price_values.value(0)),
            (150.50 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(close_price_values.value(1)),
            (151.25 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(close_type_values.len(), 2);
        assert_eq!(close_type_values.value(0), "END_OF_SESSION");
        assert_eq!(close_type_values.value(1), "CONTRACT_EXPIRED");
        assert_eq!(ts_event_values.len(), 2);
        assert_eq!(ts_event_values.value(0), 1);
        assert_eq!(ts_event_values.value(1), 2);
        assert_eq!(ts_init_values.len(), 2);
        assert_eq!(ts_init_values.value(0), 3);
        assert_eq!(ts_init_values.value(1), 4);
    }

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let raw_price1 = (150.50 * FIXED_SCALAR) as PriceRaw;
        let raw_price2 = (151.25 * FIXED_SCALAR) as PriceRaw;
        let close_price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &raw_price1.to_le_bytes(),
            &raw_price2.to_le_bytes(),
        ]);
        let close_type = enum_dictionary_array([
            InstrumentCloseType::EndOfSession,
            InstrumentCloseType::ContractExpired,
        ])
        .unwrap();
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&InstrumentClose::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(close_price),
                Arc::new(close_type),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let decoded_data = InstrumentClose::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded_data.len(), 2);
        assert_eq!(decoded_data[0].instrument_id, instrument_id);
        assert_eq!(decoded_data[0].close_price, Price::from_raw(raw_price1, 2));
        assert_eq!(
            decoded_data[0].close_type,
            InstrumentCloseType::EndOfSession
        );
        assert_eq!(decoded_data[0].ts_event.as_u64(), 1);
        assert_eq!(decoded_data[0].ts_init.as_u64(), 3);

        assert_eq!(decoded_data[1].instrument_id, instrument_id);
        assert_eq!(decoded_data[1].close_price, Price::from_raw(raw_price2, 2));
        assert_eq!(
            decoded_data[1].close_type,
            InstrumentCloseType::ContractExpired
        );
        assert_eq!(decoded_data[1].ts_event.as_u64(), 2);
        assert_eq!(decoded_data[1].ts_init.as_u64(), 4);
    }

    #[rstest]
    fn test_decode_batch_rejects_null_timestamp_with_field_and_row() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);
        let close = InstrumentClose {
            instrument_id,
            close_price: Price::from("150.50"),
            close_type: InstrumentCloseType::EndOfSession,
            ts_event: 1.into(),
            ts_init: 2.into(),
        };
        let encoded = InstrumentClose::encode_batch(&metadata, &[close]).unwrap();
        let mut columns = encoded.columns().to_vec();
        columns[3] = Arc::new(TimestampNanosecondArray::from(vec![None]).with_timezone("UTC"));
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

        let error = InstrumentClose::decode_batch(&metadata, batch).unwrap_err();

        assert!(error.to_string().contains("ts_init"));
        assert!(error.to_string().contains("row 0"));
    }

    #[rstest]
    fn test_decode_batch_invalid_close_price_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let invalid_price: PriceRaw = PriceRaw::MAX - 1000;
        let close_price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &invalid_price.to_le_bytes(),
        ]);
        let close_type = enum_dictionary_array([InstrumentCloseType::EndOfSession]).unwrap();
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&InstrumentClose::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(close_price),
                Arc::new(close_type),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = InstrumentClose::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("close_price") && err.to_string().contains("row 0"),
            "Expected close_price error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_invalid_close_type_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let raw_price = (150.50 * FIXED_SCALAR) as PriceRaw;
        let close_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);
        let close_type = enum_dictionary_array(["INVALID"]).unwrap();
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&InstrumentClose::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(close_price),
                Arc::new(close_type),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = InstrumentClose::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("InstrumentCloseType"),
            "Expected InstrumentCloseType error, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_instrument_id_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let raw_price = (150.50 * FIXED_SCALAR) as PriceRaw;
        let close_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);
        let close_type = enum_dictionary_array([InstrumentCloseType::EndOfSession]).unwrap();
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&InstrumentClose::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(close_price),
                Arc::new(close_type),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        metadata.remove(KEY_INSTRUMENT_ID);

        let result = InstrumentClose::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("instrument_id"),
            "Expected missing instrument_id error, was: {err}"
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "2".to_string()),
        ]);

        let close1 = InstrumentClose {
            instrument_id,
            close_price: Price::from("150.50"),
            close_type: InstrumentCloseType::EndOfSession,
            ts_event: 1_000_000_000.into(),
            ts_init: 1_000_000_001.into(),
        };

        let close2 = InstrumentClose {
            instrument_id,
            close_price: Price::from("151.25"),
            close_type: InstrumentCloseType::ContractExpired,
            ts_event: 2_000_000_000.into(),
            ts_init: 2_000_000_001.into(),
        };

        let original = vec![close1, close2];
        let record_batch = InstrumentClose::encode_batch(&metadata, &original).unwrap();
        let decoded = InstrumentClose::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.close_price, orig.close_price);
            assert_eq!(dec.close_type, orig.close_type);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
