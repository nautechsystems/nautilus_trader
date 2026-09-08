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
    array::{Decimal128Array, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
#[cfg(test)]
use nautilus_model::identifiers::InstrumentId;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta},
    enums::{BookAction, OrderSide},
};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_IDENTIFIER, decode_decimal_price,
    decode_decimal_quantity, decode_required_timestamp, decode_required_u8, decode_required_u64,
    enum_dictionary_array, enum_dictionary_data_type, extract_column, extract_column_string,
    fixed_decimal_data_type, identifier_array_from_display, parse_metadata, price_decimal_array,
    quantity_decimal_array,
};
#[cfg(test)]
use super::{KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for OrderBookDelta {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("action", enum_dictionary_data_type(), false),
            Field::new("side", enum_dictionary_data_type(), false),
            Field::new("price", fixed_decimal_data_type(), true),
            Field::new("size", fixed_decimal_data_type(), true),
            Field::new("order_id", DataType::UInt64, false),
            Field::new("flags", DataType::UInt8, false),
            Field::new("sequence", DataType::UInt64, false),
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

impl EncodeToRecordBatch for OrderBookDelta {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut order_id_builder = UInt64Array::builder(data.len());
        let mut flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for delta in data.iter().map(std::borrow::Borrow::borrow) {
            order_id_builder.append_value(delta.order.order_id);
            flags_builder.append_value(delta.flags);
            sequence_builder.append_value(delta.sequence);
            ts_event_builder.append_value(delta.ts_event.as_u64());
            ts_init_builder.append_value(delta.ts_init.as_u64());
        }

        let action_array = enum_dictionary_array(
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|delta| delta.action),
        )?;
        let side_array =
            enum_dictionary_array(data.iter().map(std::borrow::Borrow::borrow).map(|delta| {
                delta
                    .order
                    .side
                    .map_or_else(|| "NO_ORDER_SIDE".to_string(), |side| side.to_string())
            }))?;
        let price_array = price_decimal_array(
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|delta| delta.order.price.raw),
            "price",
        )?;
        let size_array = quantity_decimal_array(
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|delta| delta.order.size.raw),
            "size",
        )?;
        let order_id_array = order_id_builder.finish();
        let flags_array = flags_builder.finish();
        let sequence_array = sequence_builder.finish();
        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

        crate::arrow::record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(action_array),
                Arc::new(side_array),
                Arc::new(price_array),
                Arc::new(size_array),
                Arc::new(order_id_array),
                Arc::new(flags_array),
                Arc::new(sequence_array),
                Arc::new(ts_event_array),
                Arc::new(ts_init_array),
                Arc::new(identifier_array_from_display(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|delta| delta.instrument_id),
                )),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(
            &self.instrument_id,
            self.order.price.precision,
            self.order.size.precision,
        )
    }

    /// Extracts metadata from the first non-clear delta, falling back to the first clear.
    fn chunk_metadata<T>(chunk: &[T]) -> HashMap<String, String>
    where
        T: std::borrow::Borrow<Self>,
    {
        chunk
            .iter()
            .map(std::borrow::Borrow::borrow)
            .find(|delta| delta.action != BookAction::Clear)
            .or_else(|| chunk.first().map(std::borrow::Borrow::borrow))
            .map(EncodeToRecordBatch::metadata)
            .expect("Chunk must contain at least one element to encode")
    }

    fn matches_chunk_metadata(&self, metadata: &HashMap<String, String>) -> bool {
        if self.action != BookAction::Clear {
            return self.metadata() == *metadata;
        }

        parse_metadata(metadata)
            .is_ok_and(|(instrument_id, _, _)| self.instrument_id == instrument_id)
    }
}

impl DecodeFromRecordBatch for OrderBookDelta {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let record_batch = crate::arrow::record_batch_with_u64_timestamps(&record_batch)?;
        let record_batch = &record_batch;
        let cols = record_batch.columns();

        let action_values = extract_column_string(cols, "action", 0)?;
        let side_values = extract_column_string(cols, "side", 1)?;
        let price_values =
            extract_column::<Decimal128Array>(cols, "price", 2, fixed_decimal_data_type())?;
        let size_values =
            extract_column::<Decimal128Array>(cols, "size", 3, fixed_decimal_data_type())?;
        let order_id_values = extract_column::<UInt64Array>(cols, "order_id", 4, DataType::UInt64)?;
        let flags_values = extract_column::<UInt8Array>(cols, "flags", 5, DataType::UInt8)?;
        let sequence_values = extract_column::<UInt64Array>(cols, "sequence", 6, DataType::UInt64)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 7, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 8, DataType::UInt64)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|i| {
                let action_value = action_values.value(i);
                let action = BookAction::from_str(action_value).map_err(|e| {
                    EncodingError::ParseError(stringify!(BookAction), e.to_string())
                })?;
                let side_value = side_values.value(i);
                let side = if side_value.eq_ignore_ascii_case("NO_ORDER_SIDE") {
                    None
                } else {
                    Some(OrderSide::from_str(side_value).map_err(|e| {
                        EncodingError::ParseError(stringify!(OrderSide), e.to_string())
                    })?)
                };
                let price = decode_decimal_price(price_values, price_precision, "price", i)?;
                let size = decode_decimal_quantity(size_values, size_precision, "size", i)?;
                let order_id = decode_required_u64(order_id_values, "order_id", i)?;
                let flags = decode_required_u8(flags_values, "flags", i)?;
                let sequence = decode_required_u64(sequence_values, "sequence", i)?;
                let ts_event = decode_required_timestamp(ts_event_values, "ts_event", i)?;
                let ts_init = decode_required_timestamp(ts_init_values, "ts_init", i)?;

                Ok(Self {
                    instrument_id,
                    action,
                    order: BookOrder {
                        side,
                        price,
                        size,
                        order_id,
                    },
                    flags,
                    sequence,
                    ts_event,
                    ts_init,
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for OrderBookDelta {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let deltas: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(deltas.into_iter().map(Data::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Array, ArrayRef, TimestampNanosecondArray};
    use nautilus_model::types::{
        Price, Quantity,
        fixed::FIXED_SCALAR,
        price::{PRICE_UNDEF, PriceRaw},
        quantity::{QUANTITY_UNDEF, QuantityRaw},
    };
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;
    use crate::arrow::get_raw_price;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);
        let schema = OrderBookDelta::get_schema(Some(metadata.clone()));

        let expected_fields = vec![
            Field::new("action", enum_dictionary_data_type(), false),
            Field::new("side", enum_dictionary_data_type(), false),
            Field::new("price", fixed_decimal_data_type(), true),
            Field::new("size", fixed_decimal_data_type(), true),
            Field::new("order_id", DataType::UInt64, false),
            Field::new("flags", DataType::UInt8, false),
            Field::new("sequence", DataType::UInt64, false),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
        ];

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = OrderBookDelta::get_schema_map();
        let fixed_size_binary = "Decimal128(38, 16)".to_string();

        assert_eq!(schema_map.get("action").unwrap(), "Dictionary(Int8, Utf8)");
        assert_eq!(schema_map.get("side").unwrap(), "Dictionary(Int8, Utf8)");
        assert_eq!(*schema_map.get("price").unwrap(), fixed_size_binary);
        assert_eq!(*schema_map.get("size").unwrap(), fixed_size_binary);
        assert_eq!(schema_map.get("order_id").unwrap(), "UInt64");
        assert_eq!(schema_map.get("flags").unwrap(), "UInt8");
        assert_eq!(schema_map.get("sequence").unwrap(), "UInt64");
        assert_eq!(
            schema_map.get("ts_event").unwrap(),
            "Timestamp(Nanosecond, Some(\"UTC\"))"
        );
        assert_eq!(
            schema_map.get("ts_init").unwrap(),
            "Timestamp(Nanosecond, Some(\"UTC\"))"
        );
        assert_eq!(schema_map.get(KEY_IDENTIFIER).unwrap(), "Utf8");
    }

    #[rstest]
    fn clear_delta_rejects_other_instrument_chunk_metadata() {
        let delta = OrderBookDelta::clear(InstrumentId::from("AAPL.XNAS"), 0, 1.into(), 1.into());
        let metadata = OrderBookDelta::get_metadata(&InstrumentId::from("MSFT.XNAS"), 2, 0);

        assert!(!delta.matches_chunk_metadata(&metadata));
    }

    #[rstest]
    fn clear_delta_rejects_chunk_metadata_without_price_precision() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let delta = OrderBookDelta::clear(instrument_id, 0, 1.into(), 1.into());
        let mut metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);
        metadata.remove(KEY_PRICE_PRECISION);

        assert!(!delta.matches_chunk_metadata(&metadata));
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        let delta1 = OrderBookDelta {
            instrument_id,
            action: BookAction::Add,
            order: BookOrder {
                side: OrderSide::Buy.into(),
                price: Price::from("100.10"),
                size: Quantity::from(100),
                order_id: 1,
            },
            flags: 0,
            sequence: 1,
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let delta2 = OrderBookDelta {
            instrument_id,
            action: BookAction::Update,
            order: BookOrder {
                side: OrderSide::Sell.into(),
                price: Price::from("101.20"),
                size: Quantity::from(200),
                order_id: 2,
            },
            flags: 1,
            sequence: 2,
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![delta1, delta2];
        let record_batch = OrderBookDelta::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let action_values = extract_column_string(columns, "action", 0).unwrap();
        let side_values = extract_column_string(columns, "side", 1).unwrap();
        let price_values = columns[2]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let size_values = columns[3]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let order_id_values = columns[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let flags_values = columns[5].as_any().downcast_ref::<UInt8Array>().unwrap();
        let sequence_values = columns[6].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = columns[7]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_values = columns[8]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(columns.len(), 10);
        assert_eq!(action_values.len(), 2);
        assert_eq!(action_values.value(0), "ADD");
        assert_eq!(action_values.value(1), "UPDATE");
        assert_eq!(side_values.len(), 2);
        assert_eq!(side_values.value(0), "BUY");
        assert_eq!(side_values.value(1), "SELL");

        assert_eq!(price_values.len(), 2);
        assert_eq!(
            get_raw_price(price_values.value(0)),
            (100.10 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(price_values.value(1)),
            (101.20 * FIXED_SCALAR) as PriceRaw
        );

        assert_eq!(size_values.len(), 2);
        assert_eq!(
            get_raw_price(size_values.value(0)),
            (100.0 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(size_values.value(1)),
            (200.0 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(order_id_values.len(), 2);
        assert_eq!(order_id_values.value(0), 1);
        assert_eq!(order_id_values.value(1), 2);
        assert_eq!(flags_values.len(), 2);
        assert_eq!(flags_values.value(0), 0);
        assert_eq!(flags_values.value(1), 1);
        assert_eq!(sequence_values.len(), 2);
        assert_eq!(sequence_values.value(0), 1);
        assert_eq!(sequence_values.value(1), 2);
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
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        let action = enum_dictionary_array([BookAction::Add, BookAction::Update]).unwrap();
        let side = enum_dictionary_array([OrderSide::Buy, OrderSide::Buy]).unwrap();
        let price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((101.10 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((101.20 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((10000.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((9000.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let order_id = UInt64Array::from(vec![1, 2]);
        let flags = UInt8Array::from(vec![0, 0]);
        let sequence = UInt64Array::from(vec![1, 2]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&OrderBookDelta::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(action),
                Arc::new(side),
                Arc::new(price),
                Arc::new(size),
                Arc::new(order_id),
                Arc::new(flags),
                Arc::new(sequence),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let decoded_data = OrderBookDelta::decode_batch(&metadata, record_batch).unwrap();
        assert_eq!(decoded_data.len(), 2);
    }

    #[rstest]
    fn test_decode_batch_rejects_null_timestamp_with_field_and_row() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);
        let delta = OrderBookDelta {
            instrument_id,
            action: BookAction::Add,
            order: BookOrder {
                side: OrderSide::Buy.into(),
                price: Price::from("100.10"),
                size: Quantity::from(100),
                order_id: 1,
            },
            flags: 0,
            sequence: 1,
            ts_event: 1.into(),
            ts_init: 2.into(),
        };
        let encoded = OrderBookDelta::encode_batch(&metadata, &[delta]).unwrap();
        let mut columns = encoded.columns().to_vec();
        columns[8] = Arc::new(TimestampNanosecondArray::from(vec![None]).with_timezone("UTC"));
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

        let error = OrderBookDelta::decode_batch(&metadata, batch).unwrap_err();

        assert!(error.to_string().contains("ts_init"));
        assert!(error.to_string().contains("row 0"));
    }

    #[rstest]
    fn test_decode_batch_rejects_null_required_integers_with_field_and_row() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);
        let delta = OrderBookDelta {
            instrument_id,
            action: BookAction::Add,
            order: BookOrder::new(
                OrderSide::Buy,
                Price::from("100.10"),
                Quantity::from(100),
                1,
            ),
            flags: 0,
            sequence: 1,
            ts_event: 1.into(),
            ts_init: 2.into(),
        };
        let encoded = OrderBookDelta::encode_batch(&metadata, &[delta]).unwrap();
        let corruptions: [(usize, &str, ArrayRef); 3] = [
            (4, "order_id", Arc::new(UInt64Array::from(vec![None]))),
            (5, "flags", Arc::new(UInt8Array::from(vec![None]))),
            (6, "sequence", Arc::new(UInt64Array::from(vec![None]))),
        ];

        for (index, field, column) in corruptions {
            let mut columns = encoded.columns().to_vec();
            columns[index] = column;
            let fields = encoded
                .schema()
                .fields()
                .iter()
                .map(|schema_field| {
                    if schema_field.name() == field {
                        Arc::new(schema_field.as_ref().clone().with_nullable(true))
                    } else {
                        schema_field.clone()
                    }
                })
                .collect::<Vec<_>>();
            let schema = Arc::new(Schema::new_with_metadata(fields, metadata.clone()));
            let batch = RecordBatch::try_new(schema, columns).unwrap();

            let error = OrderBookDelta::decode_batch(&metadata, batch).unwrap_err();
            assert!(error.to_string().contains(field));
            assert!(error.to_string().contains("row 0"));
        }
    }

    #[rstest]
    fn test_decode_batch_with_undef_values() {
        let instrument_id = InstrumentId::from("PLTR.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        // Create test data with 'R' (clear) action which has PRICE_UNDEF and QUANTITY_UNDEF
        let action = enum_dictionary_array([BookAction::Clear, BookAction::Add]).unwrap();
        let side = enum_dictionary_array(["NO_ORDER_SIDE", "BUY"]).unwrap();
        let price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &PRICE_UNDEF.to_le_bytes(),
            &((100.50 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &QUANTITY_UNDEF.to_le_bytes(),
            &((1000.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let order_id = UInt64Array::from(vec![0, 1]);
        let flags = UInt8Array::from(vec![0, 0]);
        let sequence = UInt64Array::from(vec![1, 2]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&OrderBookDelta::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(action),
                Arc::new(side),
                Arc::new(price),
                Arc::new(size),
                Arc::new(order_id),
                Arc::new(flags),
                Arc::new(sequence),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let decoded_data = OrderBookDelta::decode_batch(&metadata, record_batch).unwrap();
        assert_eq!(decoded_data.len(), 2);
        assert_eq!(decoded_data[0].order.price.raw, PRICE_UNDEF);
        assert_eq!(decoded_data[0].order.price.precision, 0);
        assert_eq!(decoded_data[0].order.size.raw, QUANTITY_UNDEF);
        assert_eq!(decoded_data[0].order.size.precision, 0);
        assert_eq!(decoded_data[1].order.price.precision, 2);
        assert_eq!(decoded_data[1].order.size.precision, 0);
    }

    #[rstest]
    fn test_decode_batch_invalid_price_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        let action = enum_dictionary_array([BookAction::Add]).unwrap();
        let side = enum_dictionary_array([OrderSide::Buy]).unwrap();

        let invalid_price: PriceRaw = PriceRaw::MAX - 1000;
        let price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &invalid_price.to_le_bytes(),
        ]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let order_id = UInt64Array::from(vec![1]);
        let flags = UInt8Array::from(vec![0]);
        let sequence = UInt64Array::from(vec![1]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&OrderBookDelta::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(action),
                Arc::new(side),
                Arc::new(price),
                Arc::new(size),
                Arc::new(order_id),
                Arc::new(flags),
                Arc::new(sequence),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = OrderBookDelta::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("price") && err.to_string().contains("row 0"),
            "Expected price error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_invalid_action_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        let action = enum_dictionary_array(["INVALID"]).unwrap();
        let side = enum_dictionary_array([OrderSide::Buy]).unwrap();
        let price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let order_id = UInt64Array::from(vec![1]);
        let flags = UInt8Array::from(vec![0]);
        let sequence = UInt64Array::from(vec![1]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&OrderBookDelta::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(action),
                Arc::new(side),
                Arc::new(price),
                Arc::new(size),
                Arc::new(order_id),
                Arc::new(flags),
                Arc::new(sequence),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = OrderBookDelta::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("BookAction"),
            "Expected BookAction error, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_instrument_id_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);
        metadata.remove(KEY_INSTRUMENT_ID);

        let action = enum_dictionary_array([BookAction::Add]).unwrap();
        let side = enum_dictionary_array([OrderSide::Buy]).unwrap();
        let price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let order_id = UInt64Array::from(vec![1]);
        let flags = UInt8Array::from(vec![0]);
        let sequence = UInt64Array::from(vec![1]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&OrderBookDelta::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
            vec![
                Arc::new(action),
                Arc::new(side),
                Arc::new(price),
                Arc::new(size),
                Arc::new(order_id),
                Arc::new(flags),
                Arc::new(sequence),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let result = OrderBookDelta::decode_batch(&metadata, record_batch);
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
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        let delta1 = OrderBookDelta {
            instrument_id,
            action: BookAction::Add,
            order: BookOrder {
                side: OrderSide::Buy.into(),
                price: Price::from("100.10"),
                size: Quantity::from(100),
                order_id: 1,
            },
            flags: 0,
            sequence: 1,
            ts_event: 1_000_000_000.into(),
            ts_init: 1_000_000_001.into(),
        };

        let delta2 = OrderBookDelta {
            instrument_id,
            action: BookAction::Update,
            order: BookOrder {
                side: OrderSide::Sell.into(),
                price: Price::from("101.20"),
                size: Quantity::from(200),
                order_id: 2,
            },
            flags: 1,
            sequence: 2,
            ts_event: 2_000_000_000.into(),
            ts_init: 2_000_000_001.into(),
        };

        let original = vec![delta1, delta2];
        let record_batch = OrderBookDelta::encode_batch(&metadata, &original).unwrap();
        let decoded = OrderBookDelta::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.action, orig.action);
            assert_eq!(dec.order.side, orig.order.side);
            assert_eq!(dec.order.price, orig.order.price);
            assert_eq!(dec.order.size, orig.order.size);
            assert_eq!(dec.order.order_id, orig.order.order_id);
            assert_eq!(dec.flags, orig.flags);
            assert_eq!(dec.sequence, orig.sequence);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
