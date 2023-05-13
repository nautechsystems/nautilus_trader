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

use std::collections::HashMap;
use std::str::FromStr;

use datafusion::arrow::array::{Array, Int64Array, UInt64Array};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use nautilus_model::data::tick::QuoteTick;
use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use crate::parquet::{Data, DecodeDataFromRecordBatch};

impl DecodeDataFromRecordBatch for QuoteTick {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Data> {
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

        // Extract field value arrays from record batch
        let cols = record_batch.columns();
        let bid_values = cols[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_values = cols[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_size_values = cols[2].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_values = cols[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from field value arrays
        let values = bid_values
            .into_iter()
            .zip(ask_values.iter())
            .zip(ask_size_values.iter())
            .zip(bid_size_values.iter())
            .zip(ts_event_values.iter())
            .zip(ts_init_values.iter())
            .map(
                |(((((bid, ask), ask_size), bid_size), ts_event), ts_init)| {
                    Self {
                        instrument_id: instrument_id.clone(),
                        bid: Price::from_raw(bid.unwrap(), price_precision),
                        ask: Price::from_raw(ask.unwrap(), price_precision),
                        bid_size: Quantity::from_raw(bid_size.unwrap(), size_precision),
                        ask_size: Quantity::from_raw(ask_size.unwrap(), size_precision),
                        ts_event: ts_event.unwrap(),
                        ts_init: ts_init.unwrap(),
                    }
                    .into()
                },
            );

        values.collect()
    }

    fn get_schema(metadata: std::collections::HashMap<String, String>) -> SchemaRef {
        let fields = vec![
            Field::new("bid", DataType::Int64, false),
            Field::new("ask", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        Schema::new_with_metadata(fields, metadata).into()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::record_batch::RecordBatch;
    use std::{collections::HashMap, sync::Arc};

    fn create_metadata() -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), "AAPL.NASDAQ".to_string());
        metadata.insert("price_precision".to_string(), "2".to_string());
        metadata.insert("size_precision".to_string(), "0".to_string());
        metadata
    }

    #[test]
    fn test_get_schema() {
        let metadata = create_metadata();
        let schema = QuoteTick::get_schema(metadata.clone());
        let expected_fields = vec![
            Field::new("bid", DataType::Int64, false),
            Field::new("ask", DataType::Int64, false),
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata).into();
        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_decode_batch() {
        let metadata = create_metadata();

        let bid = Int64Array::from(vec![10000, 9900]);
        let ask = Int64Array::from(vec![10100, 10000]);
        let bid_size = UInt64Array::from(vec![100, 90]);
        let ask_size = UInt64Array::from(vec![110, 100]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            QuoteTick::get_schema(metadata.clone()),
            vec![
                Arc::new(bid),
                Arc::new(ask),
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
