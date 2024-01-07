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
    array::{Int64Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::bar::{Bar, BarType},
    types::{price::Price, quantity::Quantity},
};

use super::{
    extract_column, DecodeDataFromRecordBatch, EncodingError, KEY_BAR_TYPE, KEY_PRICE_PRECISION,
    KEY_SIZE_PRECISION,
};
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

fn parse_metadata(metadata: &HashMap<String, String>) -> Result<(BarType, u8, u8), EncodingError> {
    let bar_type_str = metadata
        .get(KEY_BAR_TYPE)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_BAR_TYPE))?;
    let bar_type = BarType::from_str(bar_type_str)
        .map_err(|e| EncodingError::ParseError(KEY_BAR_TYPE, e.to_string()))?;

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

    Ok((bar_type, price_precision, size_precision))
}

impl EncodeToRecordBatch for Bar {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut open_builder = Int64Array::builder(data.len());
        let mut high_builder = Int64Array::builder(data.len());
        let mut low_builder = Int64Array::builder(data.len());
        let mut close_builder = Int64Array::builder(data.len());
        let mut volume_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for bar in data {
            open_builder.append_value(bar.open.raw);
            high_builder.append_value(bar.high.raw);
            low_builder.append_value(bar.low.raw);
            close_builder.append_value(bar.close.raw);
            volume_builder.append_value(bar.volume.raw);
            ts_event_builder.append_value(bar.ts_event);
            ts_init_builder.append_value(bar.ts_init);
        }

        let open_array = open_builder.finish();
        let high_array = high_builder.finish();
        let low_array = low_builder.finish();
        let close_array = close_builder.finish();
        let volume_array = volume_builder.finish();
        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

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
    }
}

impl DecodeFromRecordBatch for Bar {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (bar_type, price_precision, size_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        let open_values = extract_column::<Int64Array>(cols, "open", 0, DataType::Int64)?;
        let high_values = extract_column::<Int64Array>(cols, "high", 1, DataType::Int64)?;
        let low_values = extract_column::<Int64Array>(cols, "low", 2, DataType::Int64)?;
        let close_values = extract_column::<Int64Array>(cols, "close", 3, DataType::Int64)?;
        let volume_values = extract_column::<UInt64Array>(cols, "volume", 4, DataType::UInt64)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 5, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 6, DataType::UInt64)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|i| {
                let open = Price::from_raw(open_values.value(i), price_precision).unwrap();
                let high = Price::from_raw(high_values.value(i), price_precision).unwrap();
                let low = Price::from_raw(low_values.value(i), price_precision).unwrap();
                let close = Price::from_raw(close_values.value(i), price_precision).unwrap();
                let volume = Quantity::from_raw(volume_values.value(i), size_precision).unwrap();
                let ts_event = ts_event_values.value(i);
                let ts_init = ts_init_values.value(i);

                Ok(Self {
                    bar_type,
                    open,
                    high,
                    low,
                    close,
                    volume,
                    ts_event,
                    ts_init,
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for Bar {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let bars: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(bars.into_iter().map(Data::from).collect())
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
        let bar_type = BarType::from_str("AAPL.XNAS-1-MINUTE-LAST-INTERNAL").unwrap();
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

    #[rstest]
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

    #[rstest]
    fn test_encode_batch() {
        let bar_type = BarType::from_str("AAPL.XNAS-1-MINUTE-LAST-INTERNAL").unwrap();
        let metadata = Bar::get_metadata(&bar_type, 2, 0);

        let bar1 = Bar::new(
            bar_type,
            Price::from("100.10"),
            Price::from("102.00"),
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from(1100),
            1,
            3,
        );
        let bar2 = Bar::new(
            bar_type,
            Price::from("100.00"),
            Price::from("100.00"),
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(1110),
            2,
            4,
        );

        let data = vec![bar1, bar2];
        let record_batch = Bar::encode_batch(&metadata, &data).unwrap();

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

    #[rstest]
    fn test_decode_batch() {
        let bar_type = BarType::from_str("AAPL.XNAS-1-MINUTE-LAST-INTERNAL").unwrap();
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

        let decoded_data = Bar::decode_batch(&metadata, record_batch).unwrap();
        assert_eq!(decoded_data.len(), 2);
    }
}
