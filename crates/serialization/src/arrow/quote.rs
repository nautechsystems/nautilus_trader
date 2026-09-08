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

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{Decimal128Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::QuoteTick;
#[cfg(test)]
use nautilus_model::identifiers::InstrumentId;

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_IDENTIFIER, decode_required_decimal_price,
    decode_required_decimal_quantity, decode_required_timestamp, extract_column,
    fixed_decimal_data_type, identifier_array_from_display, parse_metadata,
    required_price_decimal_array, required_quantity_decimal_array,
};
#[cfg(test)]
use super::{KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for QuoteTick {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("bid_price", fixed_decimal_data_type(), true),
            Field::new("ask_price", fixed_decimal_data_type(), true),
            Field::new("bid_size", fixed_decimal_data_type(), true),
            Field::new("ask_size", fixed_decimal_data_type(), true),
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

impl EncodeToRecordBatch for QuoteTick {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for quote in data.iter().map(std::borrow::Borrow::borrow) {
            ts_event_builder.append_value(quote.ts_event.as_u64());
            ts_init_builder.append_value(quote.ts_init.as_u64());
        }

        crate::arrow::record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(required_price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|quote| quote.bid_price.raw),
                    "bid_price",
                )?),
                Arc::new(required_price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|quote| quote.ask_price.raw),
                    "ask_price",
                )?),
                Arc::new(required_quantity_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|quote| quote.bid_size.raw),
                    "bid_size",
                )?),
                Arc::new(required_quantity_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|quote| quote.ask_size.raw),
                    "ask_size",
                )?),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
                Arc::new(identifier_array_from_display(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|quote| quote.instrument_id),
                )),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(
            &self.instrument_id,
            self.bid_price.precision,
            self.bid_size.precision,
        )
    }
}

impl DecodeFromRecordBatch for QuoteTick {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let record_batch = crate::arrow::record_batch_with_u64_timestamps(&record_batch)?;
        let record_batch = &record_batch;
        let cols = record_batch.columns();

        let bid_price_values =
            extract_column::<Decimal128Array>(cols, "bid_price", 0, fixed_decimal_data_type())?;
        let ask_price_values =
            extract_column::<Decimal128Array>(cols, "ask_price", 1, fixed_decimal_data_type())?;
        let bid_size_values =
            extract_column::<Decimal128Array>(cols, "bid_size", 2, fixed_decimal_data_type())?;
        let ask_size_values =
            extract_column::<Decimal128Array>(cols, "ask_size", 3, fixed_decimal_data_type())?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 4, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 5, DataType::UInt64)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                let bid_price = decode_required_decimal_price(
                    bid_price_values,
                    price_precision,
                    "bid_price",
                    row,
                )?;
                let ask_price = decode_required_decimal_price(
                    ask_price_values,
                    price_precision,
                    "ask_price",
                    row,
                )?;
                let bid_size = decode_required_decimal_quantity(
                    bid_size_values,
                    size_precision,
                    "bid_size",
                    row,
                )?;
                let ask_size = decode_required_decimal_quantity(
                    ask_size_values,
                    size_precision,
                    "ask_size",
                    row,
                )?;
                Ok(Self {
                    instrument_id,
                    bid_price,
                    ask_price,
                    bid_size,
                    ask_size,
                    ts_event: decode_required_timestamp(ts_event_values, "ts_event", row)?,
                    ts_init: decode_required_timestamp(ts_init_values, "ts_init", row)?,
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for QuoteTick {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let ticks: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(ticks.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use arrow::array::{Array, StringArray, TimestampNanosecondArray};
    use nautilus_model::types::{
        Price, Quantity, fixed::FIXED_SCALAR, price::PriceRaw, quantity::QuantityRaw,
    };
    use rstest::rstest;

    use super::*;
    use crate::arrow::{KEY_IDENTIFIER, get_raw_price, get_raw_quantity};

    #[rstest]
    fn test_quote_nanoseconds_round_trip_within_one_microsecond() {
        let first = QuoteTick {
            instrument_id: InstrumentId::from("AAPL.XNAS"),
            bid_price: Price::from("123.45"),
            ask_price: Price::from("123.67"),
            bid_size: Quantity::from(17),
            ask_size: Quantity::from(29),
            ts_event: 1_788_652_800_123_456_789_u64.into(),
            ts_init: 1_788_652_800_123_456_799_u64.into(),
        };
        let second = QuoteTick {
            ts_event: 1_788_652_800_123_456_801_u64.into(),
            ts_init: 1_788_652_800_123_456_899_u64.into(),
            ..first
        };
        let values = vec![first, second];
        let metadata = QuoteTick::get_metadata(&first.instrument_id, 2, 0);
        let batch = QuoteTick::encode_batch(&metadata, &values).unwrap();

        for field in ["ts_event", "ts_init"] {
            assert_eq!(
                batch.schema().field_with_name(field).unwrap().data_type(),
                &DataType::Timestamp(arrow::datatypes::TimeUnit::Nanosecond, Some("UTC".into()))
            );
        }
        let decoded = QuoteTick::decode_batch(&metadata, batch).unwrap();
        assert_eq!(decoded, values);
    }

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        let schema = QuoteTick::get_schema(Some(metadata.clone()));

        let mut expected_fields = Vec::with_capacity(7);

        expected_fields.push(Field::new("bid_price", fixed_decimal_data_type(), true));
        expected_fields.push(Field::new("ask_price", fixed_decimal_data_type(), true));

        expected_fields.extend(vec![
            Field::new("bid_size", fixed_decimal_data_type(), true),
            Field::new("ask_size", fixed_decimal_data_type(), true),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
        ]);

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let arrow_schema = QuoteTick::get_schema_map();
        let mut expected_map = HashMap::new();

        let fixed_size_binary = "Decimal128(38, 16)".to_string();
        expected_map.insert("bid_price".to_string(), fixed_size_binary.clone());
        expected_map.insert("ask_price".to_string(), fixed_size_binary.clone());
        expected_map.insert("bid_size".to_string(), fixed_size_binary.clone());
        expected_map.insert("ask_size".to_string(), fixed_size_binary);
        expected_map.insert(
            "ts_event".to_string(),
            "Timestamp(Nanosecond, Some(\"UTC\"))".to_string(),
        );
        expected_map.insert(
            "ts_init".to_string(),
            "Timestamp(Nanosecond, Some(\"UTC\"))".to_string(),
        );
        expected_map.insert(KEY_IDENTIFIER.to_string(), "Utf8".to_string());
        assert_eq!(arrow_schema, expected_map);
    }

    #[rstest]
    fn test_encode_quote_tick() {
        // Create test data
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let tick1 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.10"),
            ask_price: Price::from("101.50"),
            bid_size: Quantity::from(1000),
            ask_size: Quantity::from(500),
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let tick2 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.75"),
            ask_price: Price::from("100.20"),
            bid_size: Quantity::from(750),
            ask_size: Quantity::from(300),
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![tick1, tick2];
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        let record_batch = QuoteTick::encode_batch(&metadata, &data).unwrap();

        // Verify the encoded data
        let columns = record_batch.columns();

        let bid_price_values = columns[0]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let ask_price_values = columns[1]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        assert_eq!(
            get_raw_price(bid_price_values.value(0)),
            (100.10 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(bid_price_values.value(1)),
            (100.75 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(ask_price_values.value(0)),
            (101.50 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(ask_price_values.value(1)),
            (100.20 * FIXED_SCALAR) as PriceRaw
        );

        let bid_size_values = columns[2]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let ask_size_values = columns[3]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let ts_event_values = columns[4]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_values = columns[5]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        let identifier_values = columns[6].as_any().downcast_ref::<StringArray>().unwrap();

        assert_eq!(columns.len(), 7);
        assert_eq!(bid_size_values.len(), 2);
        assert_eq!(
            get_raw_quantity(bid_size_values.value(0)),
            (1000.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(
            get_raw_quantity(bid_size_values.value(1)),
            (750.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(ask_size_values.len(), 2);
        assert_eq!(
            get_raw_quantity(ask_size_values.value(0)),
            (500.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(
            get_raw_quantity(ask_size_values.value(1)),
            (300.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(ts_event_values.len(), 2);
        assert_eq!(ts_event_values.value(0), 1);
        assert_eq!(ts_event_values.value(1), 2);
        assert_eq!(ts_init_values.len(), 2);
        assert_eq!(ts_init_values.value(0), 3);
        assert_eq!(ts_init_values.value(1), 4);
        assert_eq!(record_batch.schema().field(6).name(), KEY_IDENTIFIER);
        assert_eq!(identifier_values.len(), 2);
        assert_eq!(identifier_values.value(0), "AAPL.XNAS");
        assert_eq!(identifier_values.value(1), "AAPL.XNAS");
    }

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);

        let raw_bid1 = (100.00 * FIXED_SCALAR) as PriceRaw;
        let raw_bid2 = (99.00 * FIXED_SCALAR) as PriceRaw;
        let raw_ask1 = (101.00 * FIXED_SCALAR) as PriceRaw;
        let raw_ask2 = (100.00 * FIXED_SCALAR) as PriceRaw;

        let (bid_price, ask_price) = (
            crate::arrow::test_support::decimal_array_from_bytes(vec![
                &raw_bid1.to_le_bytes(),
                &raw_bid2.to_le_bytes(),
            ]),
            crate::arrow::test_support::decimal_array_from_bytes(vec![
                &raw_ask1.to_le_bytes(),
                &raw_ask2.to_le_bytes(),
            ]),
        );

        let bid_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((90.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let ask_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((110.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((100.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&QuoteTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(bid_price),
                Arc::new(ask_price),
                Arc::new(bid_size),
                Arc::new(ask_size),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let decoded_data = QuoteTick::decode_batch(&metadata, record_batch).unwrap();
        assert_eq!(decoded_data.len(), 2);

        // Verify decoded values
        assert_eq!(decoded_data[0].bid_price, Price::from_raw(raw_bid1, 2));
        assert_eq!(decoded_data[0].ask_price, Price::from_raw(raw_ask1, 2));
        assert_eq!(decoded_data[1].bid_price, Price::from_raw(raw_bid2, 2));
        assert_eq!(decoded_data[1].ask_price, Price::from_raw(raw_ask2, 2));
    }

    #[rstest]
    fn test_decode_batch_rejects_null_timestamp_with_field_and_row() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        let quote = QuoteTick::new(
            instrument_id,
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from(10),
            Quantity::from(20),
            1.into(),
            2.into(),
        );
        let encoded = QuoteTick::encode_batch(&metadata, &[quote]).unwrap();
        let mut columns = encoded.columns().to_vec();
        columns[5] = Arc::new(TimestampNanosecondArray::from(vec![None]).with_timezone("UTC"));
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

        let error = QuoteTick::decode_batch(&metadata, batch).unwrap_err();

        assert!(error.to_string().contains("ts_init"));
        assert!(error.to_string().contains("row 0"));
    }

    #[rstest]
    fn test_decode_batch_invalid_bid_price_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);

        let invalid_price: PriceRaw = PriceRaw::MAX - 1000;
        let valid_price = (100.00 * FIXED_SCALAR) as PriceRaw;

        let bid_price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &invalid_price.to_le_bytes(),
        ]);
        let ask_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let bid_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ask_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&QuoteTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(bid_price),
                Arc::new(ask_price),
                Arc::new(bid_size),
                Arc::new(ask_size),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = QuoteTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("bid_price") && err.to_string().contains("row 0"),
            "Expected bid_price error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_invalid_ask_size_returns_error() {
        use nautilus_model::types::{fixed::FIXED_PRECISION, quantity::QUANTITY_RAW_MAX};

        let instrument_id = InstrumentId::from("AAPL.XNAS");
        // Decode the size at full precision so the out-of-range raw value bypasses the
        // precision-0 correction, which would otherwise round it back within the bound.
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, FIXED_PRECISION);

        let valid_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let bid_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let ask_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let bid_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);

        let invalid_size = QUANTITY_RAW_MAX + 1;
        let ask_size =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&invalid_size.to_le_bytes()]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&QuoteTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(bid_price),
                Arc::new(ask_price),
                Arc::new(bid_size),
                Arc::new(ask_size),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = QuoteTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("ask_size") && err.to_string().contains("row 0"),
            "Expected ask_size error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_instrument_id_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        metadata.remove(KEY_INSTRUMENT_ID);

        let valid_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let bid_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let ask_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let bid_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ask_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&QuoteTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(bid_price),
                Arc::new(ask_price),
                Arc::new(bid_size),
                Arc::new(ask_size),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = QuoteTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("instrument_id"),
            "Expected missing instrument_id error, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_price_precision_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        metadata.remove(KEY_PRICE_PRECISION);

        let valid_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let bid_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let ask_price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let bid_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ask_size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&QuoteTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(bid_price),
                Arc::new(ask_price),
                Arc::new(bid_size),
                Arc::new(ask_size),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = QuoteTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("price_precision"),
            "Expected missing price_precision error, was: {err}"
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);

        let tick1 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.10"),
            ask_price: Price::from("100.20"),
            bid_size: Quantity::from(1000),
            ask_size: Quantity::from(500),
            ts_event: 1_000_000_000.into(),
            ts_init: 1_000_000_001.into(),
        };

        let tick2 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.15"),
            ask_price: Price::from("100.25"),
            bid_size: Quantity::from(750),
            ask_size: Quantity::from(250),
            ts_event: 2_000_000_000.into(),
            ts_init: 2_000_000_001.into(),
        };

        let original = vec![tick1, tick2];
        let record_batch = QuoteTick::encode_batch(&metadata, &original).unwrap();
        let decoded = QuoteTick::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.bid_price, orig.bid_price);
            assert_eq!(dec.ask_price, orig.ask_price);
            assert_eq!(dec.bid_size, orig.bid_size);
            assert_eq!(dec.ask_size, orig.ask_size);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
