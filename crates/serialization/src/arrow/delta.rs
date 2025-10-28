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
    array::{FixedSizeBinaryArray, FixedSizeBinaryBuilder, UInt8Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta},
    enums::{BookAction, FromU8, OrderSide},
    identifiers::InstrumentId,
    types::{Price, Quantity, fixed::PRECISION_BYTES},
};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION,
    KEY_SIZE_PRECISION, extract_column,
};
use crate::arrow::{
    ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch, get_raw_price,
    get_raw_quantity,
};

impl ArrowSchemaProvider for OrderBookDelta {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("action", DataType::UInt8, false),
            Field::new("side", DataType::UInt8, false),
            Field::new("price", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("size", DataType::FixedSizeBinary(PRECISION_BYTES), false),
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
        let mut price_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut size_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut order_id_builder = UInt64Array::builder(data.len());
        let mut flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for delta in data {
            action_builder.append_value(delta.action as u8);
            side_builder.append_value(delta.order.side as u8);
            price_builder
                .append_value(delta.order.price.raw.to_le_bytes())
                .unwrap();
            size_builder
                .append_value(delta.order.size.raw.to_le_bytes())
                .unwrap();
            order_id_builder.append_value(delta.order.order_id);
            flags_builder.append_value(delta.flags);
            sequence_builder.append_value(delta.sequence);
            ts_event_builder.append_value(delta.ts_event.as_u64());
            ts_init_builder.append_value(delta.ts_init.as_u64());
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

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(
            &self.instrument_id,
            self.order.price.precision,
            self.order.size.precision,
        )
    }

    /// Extract metadata from first two deltas
    ///
    /// Use the second delta if the first one has 0 precision
    fn chunk_metadata(chunk: &[Self]) -> HashMap<String, String> {
        let delta = chunk
            .first()
            .expect("Chunk should have at least one element to encode");

        if delta.order.price.precision == 0
            && delta.order.size.precision == 0
            && let Some(delta) = chunk.get(1)
        {
            return EncodeToRecordBatch::metadata(delta);
        }

        EncodeToRecordBatch::metadata(delta)
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
        let price_values = extract_column::<FixedSizeBinaryArray>(
            cols,
            "price",
            2,
            DataType::FixedSizeBinary(PRECISION_BYTES),
        )?;
        let size_values = extract_column::<FixedSizeBinaryArray>(
            cols,
            "size",
            3,
            DataType::FixedSizeBinary(PRECISION_BYTES),
        )?;
        let order_id_values = extract_column::<UInt64Array>(cols, "order_id", 4, DataType::UInt64)?;
        let flags_values = extract_column::<UInt8Array>(cols, "flags", 5, DataType::UInt8)?;
        let sequence_values = extract_column::<UInt64Array>(cols, "sequence", 6, DataType::UInt64)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 7, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 8, DataType::UInt64)?;

        if price_values.value_length() != PRECISION_BYTES {
            return Err(EncodingError::ParseError(
                "price",
                format!(
                    "Invalid value length: expected {PRECISION_BYTES}, found {}",
                    price_values.value_length()
                ),
            ));
        }

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
                let price = Price::from_raw(get_raw_price(price_values.value(i)), price_precision);
                let size =
                    Quantity::from_raw(get_raw_quantity(size_values.value(i)), size_precision);
                let order_id = order_id_values.value(i);
                let flags = flags_values.value(i);
                let sequence = sequence_values.value(i);
                let ts_event = ts_event_values.value(i).into();
                let ts_init = ts_init_values.value(i).into();

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

    use arrow::{array::Array, record_batch::RecordBatch};
    use nautilus_model::types::{fixed::FIXED_SCALAR, price::PriceRaw};
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
            Field::new("action", DataType::UInt8, false),
            Field::new("side", DataType::UInt8, false),
            Field::new("price", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("size", DataType::FixedSizeBinary(PRECISION_BYTES), false),
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
        let fixed_size_binary = format!("FixedSizeBinary({PRECISION_BYTES})");

        assert_eq!(schema_map.get("action").unwrap(), "UInt8");
        assert_eq!(schema_map.get("side").unwrap(), "UInt8");
        assert_eq!(*schema_map.get("price").unwrap(), fixed_size_binary);
        assert_eq!(*schema_map.get("size").unwrap(), fixed_size_binary);
        assert_eq!(schema_map.get("order_id").unwrap(), "UInt64");
        assert_eq!(schema_map.get("flags").unwrap(), "UInt8");
        assert_eq!(schema_map.get("sequence").unwrap(), "UInt64");
        assert_eq!(schema_map.get("ts_event").unwrap(), "UInt64");
        assert_eq!(schema_map.get("ts_init").unwrap(), "UInt64");
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
            ts_event: 1.into(),
            ts_init: 3.into(),
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
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![delta1, delta2];
        let record_batch = OrderBookDelta::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let action_values = columns[0].as_any().downcast_ref::<UInt8Array>().unwrap();
        let side_values = columns[1].as_any().downcast_ref::<UInt8Array>().unwrap();
        let price_values = columns[2]
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .unwrap();
        let size_values = columns[3]
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .unwrap();
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

        let action = UInt8Array::from(vec![1, 2]);
        let side = UInt8Array::from(vec![1, 1]);
        let price = FixedSizeBinaryArray::from(vec![
            &((101.10 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((101.20 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let size = FixedSizeBinaryArray::from(vec![
            &((10000.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((9000.0 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
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
