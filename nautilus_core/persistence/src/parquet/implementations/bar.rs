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
use nautilus_model::data::bar::Bar;
use nautilus_model::data::bar::BarType;
use nautilus_model::types::{price::Price, quantity::Quantity};

use crate::parquet::{Data, DecodeDataFromRecordBatch};

impl DecodeDataFromRecordBatch for Bar {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Data> {
        // Parse and validate metadata
        let (bar_type, price_precision, size_precision) = parse_metadata(metadata);

        // Extract field value arrays from record batch
        let cols = record_batch.columns();
        let open_values = cols[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let high_values = cols[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let low_values = cols[2].as_any().downcast_ref::<Int64Array>().unwrap();
        let close_values = cols[3].as_any().downcast_ref::<Int64Array>().unwrap();
        let volume_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = cols[5].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[6].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from field value arrays
        let values = open_values
            .into_iter()
            .zip(high_values.iter())
            .zip(low_values.iter())
            .zip(close_values.iter())
            .zip(volume_values.iter())
            .zip(ts_event_values.iter())
            .zip(ts_init_values.iter())
            .map(
                |((((((open, high), low), close), volume), ts_event), ts_init)| {
                    Self {
                        bar_type: bar_type.clone(),
                        open: Price::from_raw(open.unwrap(), price_precision),
                        high: Price::from_raw(high.unwrap(), price_precision),
                        low: Price::from_raw(low.unwrap(), price_precision),
                        close: Price::from_raw(close.unwrap(), price_precision),
                        volume: Quantity::from_raw(volume.unwrap(), size_precision),
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
            Field::new("open", DataType::Int64, false),
            Field::new("high", DataType::Int64, false),
            Field::new("low", DataType::Int64, false),
            Field::new("close", DataType::Int64, false),
            Field::new("volume", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        Schema::new_with_metadata(fields, metadata).into()
    }
}

fn parse_metadata(metadata: &HashMap<String, String>) -> (BarType, u8, u8) {
    let bar_type = BarType::from_str(metadata.get("bar_type").unwrap().as_str()).unwrap();
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

    (bar_type, price_precision, size_precision)
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
        metadata.insert(
            "bar_type".to_string(),
            "AAPL.NASDAQ-1-MINUTE-LAST-INTERNAL".to_string(),
        );
        metadata.insert("price_precision".to_string(), "2".to_string());
        metadata.insert("size_precision".to_string(), "0".to_string());
        metadata
    }

    #[test]
    fn test_get_schema() {
        let metadata = create_metadata();
        let schema = Bar::get_schema(metadata.clone());
        let expected_fields = vec![
            Field::new("open", DataType::Int64, false),
            Field::new("high", DataType::Int64, false),
            Field::new("low", DataType::Int64, false),
            Field::new("close", DataType::Int64, false),
            Field::new("volume", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata).into();
        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_decode_batch() {
        let metadata = create_metadata();

        let open = Int64Array::from(vec![10010, 10000]);
        let high = Int64Array::from(vec![10200, 10000]);
        let low = Int64Array::from(vec![10000, 10000]);
        let close = Int64Array::from(vec![10100, 10010]);
        let volume = UInt64Array::from(vec![110, 100]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            Bar::get_schema(metadata.clone()),
            vec![
                Arc::new(open),
                Arc::new(high),
                Arc::new(low),
                Arc::new(close),
                Arc::new(volume),
                Arc::new(ts_event),
                Arc::new(ts_init),
            ],
        )
        .unwrap();

        let decoded_data = Bar::decode_batch(&metadata, record_batch);
        assert_eq!(decoded_data.len(), 2);
    }
}
