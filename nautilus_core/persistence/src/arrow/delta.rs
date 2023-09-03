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
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{delta::OrderBookDelta, order::BookOrder},
    enums::{BookAction, FromU8, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION,
    KEY_SIZE_PRECISION,
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
        .unwrap()
    }
}

impl DecodeFromRecordBatch for OrderBookDelta {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        // Parse and validate metadata
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;

        // Extract field value arrays
        let cols = record_batch.columns();

        let action_key = "action";
        let action_index = 0;
        let action_type = DataType::UInt8;
        let action_values = cols
            .get(action_index)
            .ok_or(EncodingError::MissingColumn(action_key, action_index))?;
        let action_values = action_values.as_any().downcast_ref::<UInt8Array>().ok_or(
            EncodingError::InvalidColumnType(
                action_key,
                action_index,
                action_type,
                action_values.data_type().clone(),
            ),
        )?;

        let side_key = "side";
        let side_index = 1;
        let side_type = DataType::UInt8;
        let side_values = cols
            .get(side_index)
            .ok_or(EncodingError::MissingColumn(side_key, side_index))?;
        let side_values = side_values.as_any().downcast_ref::<UInt8Array>().ok_or(
            EncodingError::InvalidColumnType(
                side_key,
                side_index,
                side_type,
                side_values.data_type().clone(),
            ),
        )?;

        let price_key = "price";
        let price_index = 2;
        let price_type = DataType::Int64;
        let size_values = cols
            .get(price_index)
            .ok_or(EncodingError::MissingColumn(price_key, price_index))?;
        let price_values = size_values.as_any().downcast_ref::<Int64Array>().ok_or(
            EncodingError::InvalidColumnType(
                price_key,
                price_index,
                price_type,
                size_values.data_type().clone(),
            ),
        )?;

        let size_key = "size";
        let size_index = 3;
        let size_type = DataType::UInt8;
        let size_values = cols
            .get(size_index)
            .ok_or(EncodingError::MissingColumn(size_key, size_index))?;
        let size_values = size_values.as_any().downcast_ref::<UInt64Array>().ok_or(
            EncodingError::InvalidColumnType(
                size_key,
                size_index,
                size_type,
                size_values.data_type().clone(),
            ),
        )?;

        let order_id_key = "order_id";
        let order_id_index = 4;
        let order_id_type = DataType::UInt64;
        let order_id_values = cols
            .get(order_id_index)
            .ok_or(EncodingError::MissingColumn(order_id_key, order_id_index))?;
        let order_id_values = order_id_values
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or(EncodingError::InvalidColumnType(
                order_id_key,
                order_id_index,
                order_id_type,
                order_id_values.data_type().clone(),
            ))?;

        let flags_key = "flags";
        let flags_index = 5;
        let flags_type = DataType::UInt8;
        let flags_values = cols
            .get(flags_index)
            .ok_or(EncodingError::MissingColumn(flags_key, flags_index))?;
        let flags_values = flags_values.as_any().downcast_ref::<UInt8Array>().ok_or(
            EncodingError::InvalidColumnType(
                flags_key,
                flags_index,
                flags_type,
                flags_values.data_type().clone(),
            ),
        )?;

        let sequence_key = "sequence";
        let sequence_index = 6;
        let sequence_type = DataType::UInt64;
        let sequence_values = cols
            .get(sequence_index)
            .ok_or(EncodingError::MissingColumn(sequence_key, sequence_index))?;
        let sequence_values = sequence_values
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or(EncodingError::InvalidColumnType(
                sequence_key,
                sequence_index,
                sequence_type,
                sequence_values.data_type().clone(),
            ))?;

        let ts_event = "ts_event";
        let ts_event_index = 7;
        let ts_event_type = DataType::UInt64;
        let ts_event_values = cols
            .get(ts_event_index)
            .ok_or(EncodingError::MissingColumn(ts_event, ts_event_index))?;
        let ts_event_values = ts_event_values
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or(EncodingError::InvalidColumnType(
                ts_event,
                ts_event_index,
                ts_event_type,
                ts_event_values.data_type().clone(),
            ))?;

        let ts_init = "ts_init";
        let ts_init_index = 8;
        let ts_inir_type = DataType::UInt64;
        let ts_init_values = cols
            .get(ts_init_index)
            .ok_or(EncodingError::MissingColumn(ts_init, ts_init_index))?;
        let ts_init_values = ts_init_values
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or(EncodingError::InvalidColumnType(
                ts_init,
                ts_init_index,
                ts_inir_type,
                ts_init_values.data_type().clone(),
            ))?;

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

        Ok(values.collect())
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
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
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
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
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

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
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
