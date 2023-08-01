// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    array::{Array, Int64Array, UInt64Array, UInt8Array},
    datatypes::{DataType, Field, Schema, SchemaRef},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{delta::OrderBookDelta, order::BookOrder},
    enums::{BookAction, FromU8, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use super::DecodeDataFromRecordBatch;
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for OrderBookDelta {
    fn get_schema(metadata: HashMap<String, String>) -> SchemaRef {
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

        Schema::new_with_metadata(fields, metadata).into()
    }
}

fn parse_metadata(metadata: &HashMap<String, String>) -> (InstrumentId, u8, u8) {
    let instrument_id =
        InstrumentId::from_str(metadata.get("instrument_id").unwrap().as_str()).unwrap();
    let price_precision = metadata
        .get("price_precision")
        .unwrap()
        .parse::<u8>()
        .unwrap();
    let size_precision = metadata
        .get("size_precision")
        .unwrap()
        .parse::<u8>()
        .unwrap();

    (instrument_id, price_precision, size_precision)
}

impl EncodeToRecordBatch for OrderBookDelta {
    fn encode_batch(metadata: &HashMap<String, String>, data: &[Self]) -> RecordBatch {
        // Create array builders
        let mut action_builder = UInt8Array::builder(data.len());
        let mut side_builder = UInt8Array::builder(data.len());
        let mut price_builder = Int64Array::builder(data.len());
        let mut size_builder = UInt64Array::builder(data.len());
        let mut order_id_builder = UInt64Array::builder(data.len());
        let mut flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        // Iterate over data
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

        // Build arrays
        let action_array = action_builder.finish();
        let side_array = side_builder.finish();
        let price_array = price_builder.finish();
        let size_array = size_builder.finish();
        let order_id_array = order_id_builder.finish();
        let flags_array = flags_builder.finish();
        let sequence_array = sequence_builder.finish();
        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

        // Build record batch
        RecordBatch::try_new(
            Self::get_schema(metadata.clone()),
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
        .unwrap()
    }
}

impl DecodeFromRecordBatch for OrderBookDelta {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Self> {
        // Parse and validate metadata
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata);

        // Extract field value arrays
        let cols = record_batch.columns();
        let action_values = cols[0].as_any().downcast_ref::<UInt8Array>().unwrap();
        let side_values = cols[1].as_any().downcast_ref::<UInt8Array>().unwrap();
        let price_values = cols[2].as_any().downcast_ref::<Int64Array>().unwrap();
        let size_values = cols[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let order_id_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let flags_values = cols[5].as_any().downcast_ref::<UInt8Array>().unwrap();
        let sequence_values = cols[6].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = cols[7].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[8].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from arrays
        let values = action_values
            .into_iter()
            .zip(side_values.iter())
            .zip(price_values.iter())
            .zip(size_values.iter())
            .zip(order_id_values.iter())
            .zip(flags_values.iter())
            .zip(sequence_values.iter())
            .zip(ts_event_values.iter())
            .zip(ts_init_values.iter())
            .map(
                |(
                    (((((((action, side), price), size), order_id), flags), sequence), ts_event),
                    ts_init,
                )| {
                    Self {
                        instrument_id,
                        action: BookAction::from_u8(action.unwrap()).unwrap(),
                        order: BookOrder {
                            side: OrderSide::from_u8(side.unwrap()).unwrap(),
                            price: Price::from_raw(price.unwrap(), price_precision),
                            size: Quantity::from_raw(size.unwrap(), size_precision),
                            order_id: order_id.unwrap(),
                        },
                        flags: flags.unwrap(),
                        sequence: sequence.unwrap(),
                        ts_event: ts_event.unwrap(),
                        ts_init: ts_init.unwrap(),
                    }
                },
            );

        values.collect()
    }
}

impl DecodeDataFromRecordBatch for OrderBookDelta {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Vec<Data> {
        let deltas: Vec<Self> = Self::decode_batch(metadata, record_batch);
        deltas.into_iter().map(Data::from).collect()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use datafusion::arrow::record_batch::RecordBatch;

    use super::*;

    #[test]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);
        let schema = OrderBookDelta::get_schema(metadata.clone());
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
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata).into();
        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_encode_batch() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 2, 0);

        let delta1 = OrderBookDelta {
            instrument_id,
            action: BookAction::Add,
            order: BookOrder {
                side: OrderSide::Buy,
                price: Price::new(100.10, 2),
                size: Quantity::new(100.0, 0),
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
                price: Price::new(101.20, 2),
                size: Quantity::new(200.0, 0),
                order_id: 2,
            },
            flags: 1,
            sequence: 2,
            ts_event: 2,
            ts_init: 4,
        };

        let data = vec![delta1, delta2];
        let record_batch = OrderBookDelta::encode_batch(&metadata, &data);

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

    #[test]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
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
            OrderBookDelta::get_schema(metadata.clone()),
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

        let decoded_data = OrderBookDelta::decode_batch(&metadata, record_batch);
        assert_eq!(decoded_data.len(), 2);
    }
}
