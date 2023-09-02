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
    array::{Array, Int64Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::quote::QuoteTick,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use super::DecodeDataFromRecordBatch;
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for QuoteTick {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("bid_price", DataType::Int64, false),
            Field::new("ask_price", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
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
    // TODO: Properly handle errors
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

impl EncodeToRecordBatch for QuoteTick {
    fn encode_batch(metadata: &HashMap<String, String>, data: &[Self]) -> RecordBatch {
        // Create array builders
        let mut bid_price_builder = Int64Array::builder(data.len());
        let mut ask_price_builder = Int64Array::builder(data.len());
        let mut bid_size_builder = UInt64Array::builder(data.len());
        let mut ask_size_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        // Iterate over data
        for tick in data {
            bid_price_builder.append_value(tick.bid_price.raw);
            ask_price_builder.append_value(tick.ask_price.raw);
            bid_size_builder.append_value(tick.bid_size.raw);
            ask_size_builder.append_value(tick.ask_size.raw);
            ts_event_builder.append_value(tick.ts_event);
            ts_init_builder.append_value(tick.ts_init);
        }

        // Build arrays
        let bid_price_array = bid_price_builder.finish();
        let ask_price_array = ask_price_builder.finish();
        let bid_size_array = bid_size_builder.finish();
        let ask_size_array = ask_size_builder.finish();
        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

        // Build record batch
        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(bid_price_array),
                Arc::new(ask_price_array),
                Arc::new(bid_size_array),
                Arc::new(ask_size_array),
                Arc::new(ts_event_array),
                Arc::new(ts_init_array),
            ],
        )
        .unwrap()
    }
}

impl DecodeFromRecordBatch for QuoteTick {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Self> {
        // Parse and validate metadata
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata);

        // Extract field value arrays
        let cols = record_batch.columns();
        let bid_price_values = cols[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_values = cols[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_size_values = cols[2].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_values = cols[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from arrays
        let values = bid_price_values
            .into_iter()
            .zip(ask_price_values.iter())
            .zip(ask_size_values.iter())
            .zip(bid_size_values.iter())
            .zip(ts_event_values.iter())
            .zip(ts_init_values.iter())
            .map(
                |(((((bid_price, ask_price), ask_size), bid_size), ts_event), ts_init)| Self {
                    instrument_id,
                    bid_price: Price::from_raw(bid_price.unwrap(), price_precision),
                    ask_price: Price::from_raw(ask_price.unwrap(), price_precision),
                    bid_size: Quantity::from_raw(bid_size.unwrap(), size_precision),
                    ask_size: Quantity::from_raw(ask_size.unwrap(), size_precision),
                    ts_event: ts_event.unwrap(),
                    ts_init: ts_init.unwrap(),
                },
            );

        values.collect()
    }
}

impl DecodeDataFromRecordBatch for QuoteTick {
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
    use std::{collections::HashMap, sync::Arc};

    use datafusion::arrow::record_batch::RecordBatch;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        let schema = QuoteTick::get_schema(Some(metadata.clone()));
        let expected_fields = vec![
            Field::new("bid_price", DataType::Int64, false),
            Field::new("ask_price", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let arrow_schema = QuoteTick::get_schema_map();
        let mut expected_map = HashMap::new();
        expected_map.insert("bid_price".to_string(), "Int64".to_string());
        expected_map.insert("ask_price".to_string(), "Int64".to_string());
        expected_map.insert("bid_size".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size".to_string(), "UInt64".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(arrow_schema, expected_map);
    }

    #[rstest]
    fn test_encode_quote_tick() {
        // Create test data
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let tick1 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.10"),
            ask_price: Price::from("101.50"),
            bid_size: Quantity::from(1000),
            ask_size: Quantity::from(500),
            ts_event: 1,
            ts_init: 3,
        };

        let tick2 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.75"),
            ask_price: Price::from("100.20"),
            bid_size: Quantity::from(750),
            ask_size: Quantity::from(300),
            ts_event: 2,
            ts_init: 4,
        };

        let data = vec![tick1, tick2];
        let metadata: HashMap<String, String> = HashMap::new();
        let record_batch = QuoteTick::encode_batch(&metadata, &data);

        // Verify the encoded data
        let columns = record_batch.columns();
        let bid_price_values = columns[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_values = columns[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_size_values = columns[2].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_values = columns[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = columns[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 6);
        assert_eq!(bid_price_values.len(), 2);
        assert_eq!(bid_price_values.value(0), 100_100_000_000);
        assert_eq!(bid_price_values.value(1), 100_750_000_000);
        assert_eq!(ask_price_values.len(), 2);
        assert_eq!(ask_price_values.value(0), 101_500_000_000);
        assert_eq!(ask_price_values.value(1), 100_200_000_000);
        assert_eq!(bid_size_values.len(), 2);
        assert_eq!(bid_size_values.value(0), 1_000_000_000_000);
        assert_eq!(bid_size_values.value(1), 750_000_000_000);
        assert_eq!(ask_size_values.len(), 2);
        assert_eq!(ask_size_values.value(0), 500_000_000_000);
        assert_eq!(ask_size_values.value(1), 300_000_000_000);
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
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);

        let bid_price = Int64Array::from(vec![10000, 9900]);
        let ask_price = Int64Array::from(vec![10100, 10000]);
        let bid_size = UInt64Array::from(vec![100, 90]);
        let ask_size = UInt64Array::from(vec![110, 100]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            QuoteTick::get_schema(Some(metadata.clone())).into(),
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

        let decoded_data = QuoteTick::decode_batch(&metadata, record_batch);
        assert_eq!(decoded_data.len(), 2);
    }
}
