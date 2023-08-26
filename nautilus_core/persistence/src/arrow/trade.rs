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
    array::{Array, Int64Array, StringArray, StringBuilder, UInt64Array, UInt8Array},
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::trade::TradeTick,
    enums::AggressorSide,
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    types::{price::Price, quantity::Quantity},
};

use super::DecodeDataFromRecordBatch;
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for TradeTick {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("price", DataType::Int64, false),
            Field::new("size", DataType::UInt64, false),
            Field::new("aggressor_side", DataType::UInt8, false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
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

impl EncodeToRecordBatch for TradeTick {
    fn encode_batch(metadata: &HashMap<String, String>, data: &[Self]) -> RecordBatch {
        // Create array builders
        let mut price_builder = Int64Array::builder(data.len());
        let mut size_builder = UInt64Array::builder(data.len());
        let mut aggressor_side_builder = UInt8Array::builder(data.len());
        let mut trade_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        // Iterate over data
        for tick in data {
            price_builder.append_value(tick.price.raw);
            size_builder.append_value(tick.size.raw);
            aggressor_side_builder.append_value(tick.aggressor_side as u8);
            trade_id_builder.append_value(tick.trade_id.to_string());
            ts_event_builder.append_value(tick.ts_event);
            ts_init_builder.append_value(tick.ts_init);
        }

        // Build arrays
        let price_array = price_builder.finish();
        let size_array = size_builder.finish();
        let aggressor_side_array = aggressor_side_builder.finish();
        let trade_id_array = trade_id_builder.finish();
        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

        // Build record batch
        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(price_array),
                Arc::new(size_array),
                Arc::new(aggressor_side_array),
                Arc::new(trade_id_array),
                Arc::new(ts_event_array),
                Arc::new(ts_init_array),
            ],
        )
        .unwrap()
    }
}

impl DecodeFromRecordBatch for TradeTick {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Self> {
        // Parse and validate metadata
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata);

        // Extract field value arrays
        let cols = record_batch.columns();
        let price_values = cols[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let size_values = cols[1].as_any().downcast_ref::<UInt64Array>().unwrap();
        let aggressor_side_values = cols[2].as_any().downcast_ref::<UInt8Array>().unwrap();
        let trade_id_values_values = cols[3].as_any().downcast_ref::<StringArray>().unwrap();
        let ts_event_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from arrays
        let values = price_values
            .into_iter()
            .zip(size_values)
            .zip(aggressor_side_values)
            .zip(trade_id_values_values)
            .zip(ts_event_values)
            .zip(ts_init_values)
            .map(
                |(((((price, size), aggressor_side), trade_id), ts_event), ts_init)| Self {
                    instrument_id,
                    price: Price::from_raw(price.unwrap(), price_precision),
                    size: Quantity::from_raw(size.unwrap(), size_precision),
                    aggressor_side: AggressorSide::from_repr(aggressor_side.unwrap() as usize)
                        .expect("cannot parse enum value"),
                    trade_id: TradeId::new(trade_id.unwrap()).unwrap(),
                    ts_event: ts_event.unwrap(),
                    ts_init: ts_init.unwrap(),
                },
            );

        values.collect()
    }
}

impl DecodeDataFromRecordBatch for TradeTick {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Vec<Data> {
        let ticks: Vec<Self> = Self::decode_batch(metadata, record_batch);
        ticks.into_iter().map(Data::from).collect()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use datafusion::arrow::{
        array::{Int64Array, StringArray, UInt64Array, UInt8Array},
        record_batch::RecordBatch,
    };

    use super::*;

    #[test]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);
        let schema = TradeTick::get_schema(Some(metadata.clone()));
        let expected_fields = vec![
            Field::new("price", DataType::Int64, false),
            Field::new("size", DataType::UInt64, false),
            Field::new("aggressor_side", DataType::UInt8, false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_get_schema_map() {
        let schema_map = TradeTick::get_schema_map();
        let mut expected_map = HashMap::new();
        expected_map.insert("price".to_string(), "Int64".to_string());
        expected_map.insert("size".to_string(), "UInt64".to_string());
        expected_map.insert("aggressor_side".to_string(), "UInt8".to_string());
        expected_map.insert("trade_id".to_string(), "Utf8".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[test]
    fn test_encode_trade_tick() {
        // Create test data
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let tick1 = TradeTick {
            instrument_id,
            price: Price::from("100.10"),
            size: Quantity::from(1000),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("1").unwrap(),
            ts_event: 1,
            ts_init: 3,
        };

        let tick2 = TradeTick {
            instrument_id,
            price: Price::from("100.50"),
            size: Quantity::from(500),
            aggressor_side: AggressorSide::Seller,
            trade_id: TradeId::new("2").unwrap(),
            ts_event: 2,
            ts_init: 4,
        };

        let data = vec![tick1, tick2];
        let record_batch = TradeTick::encode_batch(&metadata, &data);

        // Verify the encoded data
        let columns = record_batch.columns();
        let price_values = columns[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let size_values = columns[1].as_any().downcast_ref::<UInt64Array>().unwrap();
        let aggressor_side_values = columns[2].as_any().downcast_ref::<UInt8Array>().unwrap();
        let trade_id_values = columns[3].as_any().downcast_ref::<StringArray>().unwrap();
        let ts_event_values = columns[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 6);
        assert_eq!(price_values.len(), 2);
        assert_eq!(price_values.value(0), 100_100_000_000);
        assert_eq!(price_values.value(1), 100_500_000_000);
        assert_eq!(size_values.len(), 2);
        assert_eq!(size_values.value(0), 1_000_000_000_000);
        assert_eq!(size_values.value(1), 500_000_000_000);
        assert_eq!(aggressor_side_values.len(), 2);
        assert_eq!(aggressor_side_values.value(0), 1);
        assert_eq!(aggressor_side_values.value(1), 2);
        assert_eq!(trade_id_values.len(), 2);
        assert_eq!(trade_id_values.value(0), "1");
        assert_eq!(trade_id_values.value(1), "2");
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
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let price = Int64Array::from(vec![1_000_000_000_000, 1_010_000_000_000]);
        let size = UInt64Array::from(vec![1000, 900]);
        let aggressor_side = UInt8Array::from(vec![0, 1]); // 0 for BUY, 1 for SELL
        let trade_id = StringArray::from(vec!["1", "2"]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            TradeTick::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(price),
                Arc::new(size),
                Arc::new(aggressor_side),
                Arc::new(trade_id),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let decoded_data = TradeTick::decode_batch(&metadata, record_batch);
        assert_eq!(decoded_data.len(), 2);
    }
}
