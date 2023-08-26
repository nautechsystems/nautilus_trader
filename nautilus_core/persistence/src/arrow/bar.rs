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
    data::bar::{Bar, BarType},
    types::{price::Price, quantity::Quantity},
};

use super::DecodeDataFromRecordBatch;
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for Bar {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("open", DataType::Int64, false),
            Field::new("high", DataType::Int64, false),
            Field::new("low", DataType::Int64, false),
            Field::new("close", DataType::Int64, false),
            Field::new("volume", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
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

impl EncodeToRecordBatch for Bar {
    fn encode_batch(metadata: &HashMap<String, String>, data: &[Self]) -> RecordBatch {
        // Create array builders
        let mut open_builder = Int64Array::builder(data.len());
        let mut high_builder = Int64Array::builder(data.len());
        let mut low_builder = Int64Array::builder(data.len());
        let mut close_builder = Int64Array::builder(data.len());
        let mut volume_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        // Iterate over data
        for bar in data {
            open_builder.append_value(bar.open.raw);
            high_builder.append_value(bar.high.raw);
            low_builder.append_value(bar.low.raw);
            close_builder.append_value(bar.close.raw);
            volume_builder.append_value(bar.volume.raw);
            ts_event_builder.append_value(bar.ts_event);
            ts_init_builder.append_value(bar.ts_init);
        }

        // Build arrays
        let open_array = open_builder.finish();
        let high_array = high_builder.finish();
        let low_array = low_builder.finish();
        let close_array = close_builder.finish();
        let volume_array = volume_builder.finish();
        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

        // Build record batch
        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(open_array),
                Arc::new(high_array),
                Arc::new(low_array),
                Arc::new(close_array),
                Arc::new(volume_array),
                Arc::new(ts_event_array),
                Arc::new(ts_init_array),
            ],
        )
        .unwrap()
    }
}

impl DecodeFromRecordBatch for Bar {
    fn decode_batch(metadata: &HashMap<String, String>, record_batch: RecordBatch) -> Vec<Self> {
        // Parse and validate metadata
        let (bar_type, price_precision, size_precision) = parse_metadata(metadata);

        // Extract field value arrays
        let cols = record_batch.columns();
        let open_values = cols[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let high_values = cols[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let low_values = cols[2].as_any().downcast_ref::<Int64Array>().unwrap();
        let close_values = cols[3].as_any().downcast_ref::<Int64Array>().unwrap();
        let volume_values = cols[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = cols[5].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = cols[6].as_any().downcast_ref::<UInt64Array>().unwrap();

        // Construct iterator of values from arrays
        let values = open_values
            .into_iter()
            .zip(high_values.iter())
            .zip(low_values.iter())
            .zip(close_values.iter())
            .zip(volume_values.iter())
            .zip(ts_event_values.iter())
            .zip(ts_init_values.iter())
            .map(
                |((((((open, high), low), close), volume), ts_event), ts_init)| Self {
                    bar_type,
                    open: Price::from_raw(open.unwrap(), price_precision),
                    high: Price::from_raw(high.unwrap(), price_precision),
                    low: Price::from_raw(low.unwrap(), price_precision),
                    close: Price::from_raw(close.unwrap(), price_precision),
                    volume: Quantity::from_raw(volume.unwrap(), size_precision),
                    ts_event: ts_event.unwrap(),
                    ts_init: ts_init.unwrap(),
                },
            );

        values.collect()
    }
}

impl DecodeDataFromRecordBatch for Bar {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Vec<Data> {
        let bars: Vec<Self> = Self::decode_batch(metadata, record_batch);
        bars.into_iter().map(Data::from).collect()
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
        let bar_type = BarType::from_str("AAPL.NASDAQ-1-MINUTE-LAST-INTERNAL").unwrap();
        let metadata = Bar::get_metadata(&bar_type, 2, 0);
        let schema = Bar::get_schema(Some(metadata.clone()));
        let expected_fields = vec![
            Field::new("open", DataType::Int64, false),
            Field::new("high", DataType::Int64, false),
            Field::new("low", DataType::Int64, false),
            Field::new("close", DataType::Int64, false),
            Field::new("volume", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[test]
    fn test_get_schema_map() {
        let schema_map = Bar::get_schema_map();
        let mut expected_map = HashMap::new();
        expected_map.insert("open".to_string(), "Int64".to_string());
        expected_map.insert("high".to_string(), "Int64".to_string());
        expected_map.insert("low".to_string(), "Int64".to_string());
        expected_map.insert("close".to_string(), "Int64".to_string());
        expected_map.insert("volume".to_string(), "UInt64".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[test]
    fn test_encode_batch() {
        let bar_type = BarType::from_str("AAPL.NASDAQ-1-MINUTE-LAST-INTERNAL").unwrap();
        let metadata = Bar::get_metadata(&bar_type, 2, 0);

        let bar1 = Bar::new(
            bar_type,
            Price::new(100.10, 2),
            Price::new(102.00, 2),
            Price::new(100.00, 2),
            Price::new(101.00, 2),
            Quantity::from(1100),
            1,
            3,
        );
        let bar2 = Bar::new(
            bar_type,
            Price::new(100.00, 2),
            Price::new(100.00, 2),
            Price::new(100.00, 2),
            Price::new(100.10, 2),
            Quantity::from(1110),
            2,
            4,
        );

        let data = vec![bar1, bar2];
        let record_batch = Bar::encode_batch(&metadata, &data);

        let columns = record_batch.columns();
        let open_values = columns[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let high_values = columns[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let low_values = columns[2].as_any().downcast_ref::<Int64Array>().unwrap();
        let close_values = columns[3].as_any().downcast_ref::<Int64Array>().unwrap();
        let volume_values = columns[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = columns[5].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[6].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 7);
        assert_eq!(open_values.len(), 2);
        assert_eq!(open_values.value(0), 100_100_000_000);
        assert_eq!(open_values.value(1), 100_000_000_000);
        assert_eq!(high_values.len(), 2);
        assert_eq!(high_values.value(0), 102_000_000_000);
        assert_eq!(high_values.value(1), 100_000_000_000);
        assert_eq!(low_values.len(), 2);
        assert_eq!(low_values.value(0), 100_000_000_000);
        assert_eq!(low_values.value(1), 100_000_000_000);
        assert_eq!(close_values.len(), 2);
        assert_eq!(close_values.value(0), 101_000_000_000);
        assert_eq!(close_values.value(1), 100_100_000_000);
        assert_eq!(volume_values.len(), 2);
        assert_eq!(volume_values.value(0), 1_100_000_000_000);
        assert_eq!(volume_values.value(1), 1_110_000_000_000);
        assert_eq!(ts_event_values.len(), 2);
        assert_eq!(ts_event_values.value(0), 1);
        assert_eq!(ts_event_values.value(1), 2);
        assert_eq!(ts_init_values.len(), 2);
        assert_eq!(ts_init_values.value(0), 3);
        assert_eq!(ts_init_values.value(1), 4);
    }

    #[test]
    fn test_decode_batch() {
        let bar_type = BarType::from_str("AAPL.NASDAQ-1-MINUTE-LAST-INTERNAL").unwrap();
        let metadata = Bar::get_metadata(&bar_type, 2, 0);

        let open = Int64Array::from(vec![100_100_000_000, 10_000_000_000]);
        let high = Int64Array::from(vec![102_000_000_000, 10_000_000_000]);
        let low = Int64Array::from(vec![100_000_000_000, 10_000_000_000]);
        let close = Int64Array::from(vec![101_000_000_000, 10_010_000_000]);
        let volume = UInt64Array::from(vec![11_000_000_000, 10_000_000_000]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = RecordBatch::try_new(
            Bar::get_schema(Some(metadata.clone())).into(),
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
