// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use datafusion::arrow::{
    array::{Int64Array, UInt64Array, UInt8Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{delta::OrderBookDelta, order::BookOrder},
    enums::{BookAction, FromU8, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use super::{
    extract_column, DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION,
};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for OrderBookDelta {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("action", DataType::UInt8, false),
            Field::new("side", DataType::UInt8, false),
            Field::new("price", DataType::Int64, false),
            Field::new("size", DataType::UInt64, false),
            Field::new("order_id", DataType::UInt64, false),
            Field::new("flags", DataType::UInt8, false),
            Field::new("sequence", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

fn parse_metadata(
    metadata: &HashMap<String, String>,
) -> Result<(InstrumentId, u8, u8), EncodingError> {
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

    let size_precision = metadata
        .get(KEY_SIZE_PRECISION)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_SIZE_PRECISION))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(KEY_SIZE_PRECISION, e.to_string()))?;

    Ok((instrument_id, price_precision, size_precision))
}

impl EncodeToRecordBatch for OrderBookDelta {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut action_builder = UInt8Array::builder(data.len());
        let mut side_builder = UInt8Array::builder(data.len());
        let mut price_builder = Int64Array::builder(data.len());
        let mut size_builder = UInt64Array::builder(data.len());
        let mut order_id_builder = UInt64Array::builder(data.len());
        let mut flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for delta in data {
            action_builder.append_value(delta.action as u8);
            side_builder.append_value(delta.order.side as u8);
            price_builder.append_value(delta.order.price.raw);
            size_builder.append_value(delta.order.size.raw);
            order_id_builder.append_value(delta.order.order_id);
            flags_builder.append_value(delta.flags);
            sequence_builder.append_value(delta.sequence);
            ts_event_builder.append_value(delta.ts_event);
            ts_init_builder.append_value(delta.ts_init);
        }

        let action_array = action_builder.finish();
        let side_array = side_builder.finish();
        let price_array = price_builder.finish();
        let size_array = size_builder.finish();
        let order_id_array = order_id_builder.finish();
        let flags_array = flags_builder.finish();
        let sequence_array = sequence_builder.finish();
        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

        RecordBatch::try_new(
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
            ],
        )
    }
}

impl DecodeFromRecordBatch for OrderBookDelta {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        let action_values = extract_column::<UInt8Array>(cols, "action", 0, DataType::UInt8)?;
        let side_values = extract_column::<UInt8Array>(cols, "side", 1, DataType::UInt8)?;
        let price_values = extract_column::<Int64Array>(cols, "price", 2, DataType::Int64)?;
        let size_values = extract_column::<UInt64Array>(cols, "size", 3, DataType::UInt64)?;
        let order_id_values = extract_column::<UInt64Array>(cols, "order_id", 4, DataType::UInt64)?;
        let flags_values = extract_column::<UInt8Array>(cols, "flags", 5, DataType::UInt8)?;
        let sequence_values = extract_column::<UInt64Array>(cols, "sequence", 6, DataType::UInt64)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 7, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 8, DataType::UInt64)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|i| {
                let action_value = action_values.value(i);
                let action = BookAction::from_u8(action_value).ok_or_else(|| {
                    EncodingError::ParseError(
                        stringify!(BookAction),
                        format!("Invalid enum value, was {action_value}"),
                    )
                })?;
                let side_value = side_values.value(i);
                let side = OrderSide::from_u8(side_value).ok_or_else(|| {
                    EncodingError::ParseError(
                        stringify!(OrderSide),
                        format!("Invalid enum value, was {side_value}"),
                    )
                })?;
                let price = Price::from_raw(price_values.value(i), price_precision).unwrap();
                let size = Quantity::from_raw(size_values.value(i), size_precision).unwrap();
                let order_id = order_id_values.value(i);
                let flags = flags_values.value(i);
                let sequence = sequence_values.value(i);
                let ts_event = ts_event_values.value(i);
                let ts_init = ts_init_values.value(i);

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use datafusion::arrow::record_batch::RecordBatch;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);
        let schema = OrderBookDelta::get_schema(Some(metadata.clone()));
        let expected_fields = vec![
            Field::new("action", DataType::UInt8, false),
            Field::new("side", DataType::UInt8, false),
            Field::new("price", DataType::Int64, false),
            Field::new("size", DataType::UInt64, false),
            Field::new("order_id", DataType::UInt64, false),
            Field::new("flags", DataType::UInt8, false),
            Field::new("sequence", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = OrderBookDelta::get_schema_map();
        let mut expected_map = HashMap::new();
        expected_map.insert("action".to_string(), "UInt8".to_string());
        expected_map.insert("side".to_string(), "UInt8".to_string());
        expected_map.insert("price".to_string(), "Int64".to_string());
        expected_map.insert("size".to_string(), "UInt64".to_string());
        expected_map.insert("order_id".to_string(), "UInt64".to_string());
        expected_map.insert("flags".to_string(), "UInt8".to_string());
        expected_map.insert("sequence".to_string(), "UInt64".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[rstest]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        let delta1 = OrderBookDelta {
            instrument_id,
            action: BookAction::Add,
            order: BookOrder {
                side: OrderSide::Buy,
                price: Price::from("100.10"),
                size: Quantity::from(100),
                order_id: 1,
            },
            flags: 0,
            sequence: 1,
            ts_event: 1,
            ts_init: 3,
        };

        let delta2 = OrderBookDelta {
            instrument_id,
            action: BookAction::Update,
            order: BookOrder {
                side: OrderSide::Sell,
                price: Price::from("101.20"),
                size: Quantity::from(200),
                order_id: 2,
            },
            flags: 1,
            sequence: 2,
            ts_event: 2,
            ts_init: 4,
        };

        let data = vec![delta1, delta2];
        let record_batch = OrderBookDelta::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let action_values = columns[0].as_any().downcast_ref::<UInt8Array>().unwrap();
        let side_values = columns[1].as_any().downcast_ref::<UInt8Array>().unwrap();
        let price_values = columns[2].as_any().downcast_ref::<Int64Array>().unwrap();
        let size_values = columns[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let order_id_values = columns[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let flags_values = columns[5].as_any().downcast_ref::<UInt8Array>().unwrap();
        let sequence_values = columns[6].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = columns[7].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[8].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 9);
        assert_eq!(action_values.len(), 2);
        assert_eq!(action_values.value(0), 1);
        assert_eq!(action_values.value(1), 2);
        assert_eq!(side_values.len(), 2);
        assert_eq!(side_values.value(0), 1);
        assert_eq!(side_values.value(1), 2);
        assert_eq!(price_values.len(), 2);
        assert_eq!(price_values.value(0), 100_100_000_000);
        assert_eq!(price_values.value(1), 101_200_000_000);
        assert_eq!(size_values.len(), 2);
        assert_eq!(size_values.value(0), 100_000_000_000);
        assert_eq!(size_values.value(1), 200_000_000_000);
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

        let action = UInt8Array::from(vec![1, 2]);
        let side = UInt8Array::from(vec![1, 1]);
        let price = Int64Array::from(vec![100_100_000_000, 100_100_000_000]);
        let size = UInt64Array::from(vec![10000, 9000]);
        let order_id = UInt64Array::from(vec![1, 2]);
        let flags = UInt8Array::from(vec![0, 0]);
        let sequence = UInt64Array::from(vec![1, 2]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            OrderBookDelta::get_schema(Some(metadata.clone())).into(),
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
}
