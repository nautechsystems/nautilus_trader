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
    array::{Array, Int64Array, UInt32Array, UInt64Array, UInt8Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        depth::{OrderBookDepth10, DEPTH10_LEN},
        order::BookOrder,
    },
    enums::OrderSide,
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

use super::{
    extract_column, DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION,
};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for OrderBookDepth10 {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("bid_price_0", DataType::Int64, false),
            Field::new("bid_price_1", DataType::Int64, false),
            Field::new("bid_price_2", DataType::Int64, false),
            Field::new("bid_price_3", DataType::Int64, false),
            Field::new("bid_price_4", DataType::Int64, false),
            Field::new("bid_price_5", DataType::Int64, false),
            Field::new("bid_price_6", DataType::Int64, false),
            Field::new("bid_price_7", DataType::Int64, false),
            Field::new("bid_price_8", DataType::Int64, false),
            Field::new("bid_price_9", DataType::Int64, false),
            Field::new("ask_price_0", DataType::Int64, false),
            Field::new("ask_price_1", DataType::Int64, false),
            Field::new("ask_price_2", DataType::Int64, false),
            Field::new("ask_price_3", DataType::Int64, false),
            Field::new("ask_price_4", DataType::Int64, false),
            Field::new("ask_price_5", DataType::Int64, false),
            Field::new("ask_price_6", DataType::Int64, false),
            Field::new("ask_price_7", DataType::Int64, false),
            Field::new("ask_price_8", DataType::Int64, false),
            Field::new("ask_price_9", DataType::Int64, false),
            Field::new("bid_size_0", DataType::UInt64, false),
            Field::new("bid_size_1", DataType::UInt64, false),
            Field::new("bid_size_2", DataType::UInt64, false),
            Field::new("bid_size_3", DataType::UInt64, false),
            Field::new("bid_size_4", DataType::UInt64, false),
            Field::new("bid_size_5", DataType::UInt64, false),
            Field::new("bid_size_6", DataType::UInt64, false),
            Field::new("bid_size_7", DataType::UInt64, false),
            Field::new("bid_size_8", DataType::UInt64, false),
            Field::new("bid_size_9", DataType::UInt64, false),
            Field::new("ask_size_0", DataType::UInt64, false),
            Field::new("ask_size_1", DataType::UInt64, false),
            Field::new("ask_size_2", DataType::UInt64, false),
            Field::new("ask_size_3", DataType::UInt64, false),
            Field::new("ask_size_4", DataType::UInt64, false),
            Field::new("ask_size_5", DataType::UInt64, false),
            Field::new("ask_size_6", DataType::UInt64, false),
            Field::new("ask_size_7", DataType::UInt64, false),
            Field::new("ask_size_8", DataType::UInt64, false),
            Field::new("ask_size_9", DataType::UInt64, false),
            Field::new("bid_count_0", DataType::UInt32, false),
            Field::new("bid_count_1", DataType::UInt32, false),
            Field::new("bid_count_2", DataType::UInt32, false),
            Field::new("bid_count_3", DataType::UInt32, false),
            Field::new("bid_count_4", DataType::UInt32, false),
            Field::new("bid_count_5", DataType::UInt32, false),
            Field::new("bid_count_6", DataType::UInt32, false),
            Field::new("bid_count_7", DataType::UInt32, false),
            Field::new("bid_count_8", DataType::UInt32, false),
            Field::new("bid_count_9", DataType::UInt32, false),
            Field::new("ask_count_0", DataType::UInt32, false),
            Field::new("ask_count_1", DataType::UInt32, false),
            Field::new("ask_count_2", DataType::UInt32, false),
            Field::new("ask_count_3", DataType::UInt32, false),
            Field::new("ask_count_4", DataType::UInt32, false),
            Field::new("ask_count_5", DataType::UInt32, false),
            Field::new("ask_count_6", DataType::UInt32, false),
            Field::new("ask_count_7", DataType::UInt32, false),
            Field::new("ask_count_8", DataType::UInt32, false),
            Field::new("ask_count_9", DataType::UInt32, false),
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

impl EncodeToRecordBatch for OrderBookDepth10 {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut bid_price_builders = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_price_builders = Vec::with_capacity(DEPTH10_LEN);
        let mut bid_size_builders = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_size_builders = Vec::with_capacity(DEPTH10_LEN);
        let mut bid_count_builders = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_count_builders = Vec::with_capacity(DEPTH10_LEN);

        for _ in 0..DEPTH10_LEN {
            bid_price_builders.push(Int64Array::builder(data.len()));
            ask_price_builders.push(Int64Array::builder(data.len()));
            bid_size_builders.push(UInt64Array::builder(data.len()));
            ask_size_builders.push(UInt64Array::builder(data.len()));
            bid_count_builders.push(UInt32Array::builder(data.len()));
            ask_count_builders.push(UInt32Array::builder(data.len()));
        }

        let mut flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for depth in data {
            for i in 0..DEPTH10_LEN {
                bid_price_builders[i].append_value(depth.bids[i].price.raw);
                ask_price_builders[i].append_value(depth.asks[i].price.raw);
                bid_size_builders[i].append_value(depth.bids[i].size.raw);
                ask_size_builders[i].append_value(depth.asks[i].size.raw);
                bid_count_builders[i].append_value(depth.bid_counts[i]);
                ask_count_builders[i].append_value(depth.ask_counts[i]);
            }

            flags_builder.append_value(depth.flags);
            sequence_builder.append_value(depth.sequence);
            ts_event_builder.append_value(depth.ts_event);
            ts_init_builder.append_value(depth.ts_init);
        }

        let bid_price_arrays = bid_price_builders
            .into_iter()
            .map(|mut b| Arc::new(b.finish()) as Arc<dyn Array>)
            .collect::<Vec<_>>();
        let ask_price_arrays = ask_price_builders
            .into_iter()
            .map(|mut b| Arc::new(b.finish()) as Arc<dyn Array>)
            .collect::<Vec<_>>();
        let bid_size_arrays = bid_size_builders
            .into_iter()
            .map(|mut b| Arc::new(b.finish()) as Arc<dyn Array>)
            .collect::<Vec<_>>();
        let ask_size_arrays = ask_size_builders
            .into_iter()
            .map(|mut b| Arc::new(b.finish()) as Arc<dyn Array>)
            .collect::<Vec<_>>();
        let bid_count_arrays = bid_count_builders
            .into_iter()
            .map(|mut b| Arc::new(b.finish()) as Arc<dyn Array>)
            .collect::<Vec<_>>();
        let ask_count_arrays = ask_count_builders
            .into_iter()
            .map(|mut b| Arc::new(b.finish()) as Arc<dyn Array>)
            .collect::<Vec<_>>();

        let flags_array = Arc::new(flags_builder.finish());
        let sequence_array = Arc::new(sequence_builder.finish());
        let ts_event_array = Arc::new(ts_event_builder.finish());
        let ts_init_array = Arc::new(ts_init_builder.finish());

        let mut columns = Vec::new();
        columns.extend_from_slice(&bid_price_arrays);
        columns.extend_from_slice(&ask_price_arrays);
        columns.extend_from_slice(&bid_size_arrays);
        columns.extend_from_slice(&ask_size_arrays);
        columns.extend_from_slice(&bid_count_arrays);
        columns.extend_from_slice(&ask_count_arrays);
        columns.push(flags_array);
        columns.push(sequence_array);
        columns.push(ts_event_array);
        columns.push(ts_init_array);

        RecordBatch::try_new(Self::get_schema(Some(metadata.clone())).into(), columns)
    }
}

impl DecodeFromRecordBatch for OrderBookDepth10 {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        let bid_price_col_names = [
            "bid_price_0",
            "bid_price_1",
            "bid_price_2",
            "bid_price_3",
            "bid_price_4",
            "bid_price_5",
            "bid_price_6",
            "bid_price_7",
            "bid_price_8",
            "bid_price_9",
        ];

        let ask_price_col_names = [
            "ask_price_0",
            "ask_price_1",
            "ask_price_2",
            "ask_price_3",
            "ask_price_4",
            "ask_price_5",
            "ask_price_6",
            "ask_price_7",
            "ask_price_8",
            "ask_price_9",
        ];

        let bid_size_col_names = [
            "bid_size_0",
            "bid_size_1",
            "bid_size_2",
            "bid_size_3",
            "bid_size_4",
            "bid_size_5",
            "bid_size_6",
            "bid_size_7",
            "bid_size_8",
            "bid_size_9",
        ];

        let ask_size_col_names = [
            "ask_size_0",
            "ask_size_1",
            "ask_size_2",
            "ask_size_3",
            "ask_size_4",
            "ask_size_5",
            "ask_size_6",
            "ask_size_7",
            "ask_size_8",
            "ask_size_9",
        ];

        let bid_count_col_names = [
            "bid_count_0",
            "bid_count_1",
            "bid_count_2",
            "bid_count_3",
            "bid_count_4",
            "bid_count_5",
            "bid_count_6",
            "bid_count_7",
            "bid_count_8",
            "bid_count_9",
        ];

        let ask_count_col_names = [
            "ask_count_0",
            "ask_count_1",
            "ask_count_2",
            "ask_count_3",
            "ask_count_4",
            "ask_count_5",
            "ask_count_6",
            "ask_count_7",
            "ask_count_8",
            "ask_count_9",
        ];

        let mut bid_prices = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_prices = Vec::with_capacity(DEPTH10_LEN);
        let mut bid_sizes = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_sizes = Vec::with_capacity(DEPTH10_LEN);
        let mut bid_counts = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_counts = Vec::with_capacity(DEPTH10_LEN);

        for i in 0..DEPTH10_LEN {
            bid_prices.push(extract_column::<Int64Array>(
                cols,
                bid_price_col_names[i],
                i,
                DataType::Int64,
            )?);
            ask_prices.push(extract_column::<Int64Array>(
                cols,
                ask_price_col_names[i],
                DEPTH10_LEN + i,
                DataType::Int64,
            )?);
            bid_sizes.push(extract_column::<UInt64Array>(
                cols,
                bid_size_col_names[i],
                2 * DEPTH10_LEN + i,
                DataType::UInt64,
            )?);
            ask_sizes.push(extract_column::<UInt64Array>(
                cols,
                ask_size_col_names[i],
                3 * DEPTH10_LEN + i,
                DataType::UInt64,
            )?);
            bid_counts.push(extract_column::<UInt32Array>(
                cols,
                bid_count_col_names[i],
                4 * DEPTH10_LEN + i,
                DataType::UInt32,
            )?);
            ask_counts.push(extract_column::<UInt32Array>(
                cols,
                ask_count_col_names[i],
                5 * DEPTH10_LEN + i,
                DataType::UInt32,
            )?);
        }

        let flags = extract_column::<UInt8Array>(cols, "flags", 6 * DEPTH10_LEN, DataType::UInt8)?;
        let sequence =
            extract_column::<UInt64Array>(cols, "sequence", 6 * DEPTH10_LEN + 1, DataType::UInt64)?;
        let ts_event =
            extract_column::<UInt64Array>(cols, "ts_event", 6 * DEPTH10_LEN + 2, DataType::UInt64)?;
        let ts_init =
            extract_column::<UInt64Array>(cols, "ts_init", 6 * DEPTH10_LEN + 3, DataType::UInt64)?;

        // Map record batch rows to vector of OrderBookDepth10
        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|i| {
                let mut bids = [BookOrder::default(); DEPTH10_LEN];
                let mut asks = [BookOrder::default(); DEPTH10_LEN];
                let mut bid_count_arr = [0u32; DEPTH10_LEN];
                let mut ask_count_arr = [0u32; DEPTH10_LEN];

                for j in 0..DEPTH10_LEN {
                    bids[j] = BookOrder::new(
                        OrderSide::Buy,
                        Price::from_raw(bid_prices[j].value(i), price_precision).unwrap(),
                        Quantity::from_raw(bid_sizes[j].value(i), size_precision).unwrap(),
                        0, // Order ID always zero
                    );

                    asks[j] = BookOrder::new(
                        OrderSide::Sell,
                        Price::from_raw(ask_prices[j].value(i), price_precision).unwrap(),
                        Quantity::from_raw(ask_sizes[j].value(i), size_precision).unwrap(),
                        0, // Order ID always zero
                    );
                    bid_count_arr[j] = bid_counts[j].value(i);
                    ask_count_arr[j] = ask_counts[j].value(i);
                }

                Ok(Self {
                    instrument_id,
                    bids,
                    asks,
                    bid_counts: bid_count_arr,
                    ask_counts: ask_count_arr,
                    flags: flags.value(i),
                    sequence: sequence.value(i),
                    ts_event: ts_event.value(i),
                    ts_init: ts_init.value(i),
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for OrderBookDepth10 {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let depths: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(depths.into_iter().map(Data::from).collect())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {

    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use nautilus_model::data::depth::stubs::stub_depth10;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDepth10::get_metadata(&instrument_id, 2, 0);
        let schema = OrderBookDepth10::get_schema(Some(metadata.clone()));
        let expected_fields = vec![
            Field::new("bid_price_0", DataType::Int64, false),
            Field::new("bid_price_1", DataType::Int64, false),
            Field::new("bid_price_2", DataType::Int64, false),
            Field::new("bid_price_3", DataType::Int64, false),
            Field::new("bid_price_4", DataType::Int64, false),
            Field::new("bid_price_5", DataType::Int64, false),
            Field::new("bid_price_6", DataType::Int64, false),
            Field::new("bid_price_7", DataType::Int64, false),
            Field::new("bid_price_8", DataType::Int64, false),
            Field::new("bid_price_9", DataType::Int64, false),
            Field::new("ask_price_0", DataType::Int64, false),
            Field::new("ask_price_1", DataType::Int64, false),
            Field::new("ask_price_2", DataType::Int64, false),
            Field::new("ask_price_3", DataType::Int64, false),
            Field::new("ask_price_4", DataType::Int64, false),
            Field::new("ask_price_5", DataType::Int64, false),
            Field::new("ask_price_6", DataType::Int64, false),
            Field::new("ask_price_7", DataType::Int64, false),
            Field::new("ask_price_8", DataType::Int64, false),
            Field::new("ask_price_9", DataType::Int64, false),
            Field::new("bid_size_0", DataType::UInt64, false),
            Field::new("bid_size_1", DataType::UInt64, false),
            Field::new("bid_size_2", DataType::UInt64, false),
            Field::new("bid_size_3", DataType::UInt64, false),
            Field::new("bid_size_4", DataType::UInt64, false),
            Field::new("bid_size_5", DataType::UInt64, false),
            Field::new("bid_size_6", DataType::UInt64, false),
            Field::new("bid_size_7", DataType::UInt64, false),
            Field::new("bid_size_8", DataType::UInt64, false),
            Field::new("bid_size_9", DataType::UInt64, false),
            Field::new("ask_size_0", DataType::UInt64, false),
            Field::new("ask_size_1", DataType::UInt64, false),
            Field::new("ask_size_2", DataType::UInt64, false),
            Field::new("ask_size_3", DataType::UInt64, false),
            Field::new("ask_size_4", DataType::UInt64, false),
            Field::new("ask_size_5", DataType::UInt64, false),
            Field::new("ask_size_6", DataType::UInt64, false),
            Field::new("ask_size_7", DataType::UInt64, false),
            Field::new("ask_size_8", DataType::UInt64, false),
            Field::new("ask_size_9", DataType::UInt64, false),
            Field::new("bid_count_0", DataType::UInt32, false),
            Field::new("bid_count_1", DataType::UInt32, false),
            Field::new("bid_count_2", DataType::UInt32, false),
            Field::new("bid_count_3", DataType::UInt32, false),
            Field::new("bid_count_4", DataType::UInt32, false),
            Field::new("bid_count_5", DataType::UInt32, false),
            Field::new("bid_count_6", DataType::UInt32, false),
            Field::new("bid_count_7", DataType::UInt32, false),
            Field::new("bid_count_8", DataType::UInt32, false),
            Field::new("bid_count_9", DataType::UInt32, false),
            Field::new("ask_count_0", DataType::UInt32, false),
            Field::new("ask_count_1", DataType::UInt32, false),
            Field::new("ask_count_2", DataType::UInt32, false),
            Field::new("ask_count_3", DataType::UInt32, false),
            Field::new("ask_count_4", DataType::UInt32, false),
            Field::new("ask_count_5", DataType::UInt32, false),
            Field::new("ask_count_6", DataType::UInt32, false),
            Field::new("ask_count_7", DataType::UInt32, false),
            Field::new("ask_count_8", DataType::UInt32, false),
            Field::new("ask_count_9", DataType::UInt32, false),
            Field::new("flags", DataType::UInt8, false),
            Field::new("sequence", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
        assert_eq!(schema.metadata()["instrument_id"], "AAPL.XNAS");
        assert_eq!(schema.metadata()["price_precision"], "2");
        assert_eq!(schema.metadata()["size_precision"], "0");
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = OrderBookDepth10::get_schema_map();
        let mut expected_map = HashMap::new();
        expected_map.insert("bid_price_0".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_1".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_2".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_3".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_4".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_5".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_6".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_7".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_8".to_string(), "Int64".to_string());
        expected_map.insert("bid_price_9".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_0".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_1".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_2".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_3".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_4".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_5".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_6".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_7".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_8".to_string(), "Int64".to_string());
        expected_map.insert("ask_price_9".to_string(), "Int64".to_string());
        expected_map.insert("bid_size_0".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_1".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_2".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_3".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_4".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_5".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_6".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_7".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_8".to_string(), "UInt64".to_string());
        expected_map.insert("bid_size_9".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_0".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_1".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_2".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_3".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_4".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_5".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_6".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_7".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_8".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size_9".to_string(), "UInt64".to_string());
        expected_map.insert("bid_count_0".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_1".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_2".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_3".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_4".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_5".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_6".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_7".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_8".to_string(), "UInt32".to_string());
        expected_map.insert("bid_count_9".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_0".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_1".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_2".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_3".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_4".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_5".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_6".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_7".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_8".to_string(), "UInt32".to_string());
        expected_map.insert("ask_count_9".to_string(), "UInt32".to_string());
        expected_map.insert("flags".to_string(), "UInt8".to_string());
        expected_map.insert("sequence".to_string(), "UInt64".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[rstest]
    fn test_encode_batch(stub_depth10: OrderBookDepth10) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDepth10::get_metadata(&instrument_id, 2, 0);

        let data = vec![stub_depth10];
        let record_batch = OrderBookDepth10::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();

        let bid_price_0_values = columns[0].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_1_values = columns[1].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_2_values = columns[2].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_3_values = columns[3].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_4_values = columns[4].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_5_values = columns[5].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_6_values = columns[6].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_7_values = columns[7].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_8_values = columns[8].as_any().downcast_ref::<Int64Array>().unwrap();
        let bid_price_9_values = columns[9].as_any().downcast_ref::<Int64Array>().unwrap();

        let ask_price_0_values = columns[10].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_1_values = columns[11].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_2_values = columns[12].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_3_values = columns[13].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_4_values = columns[14].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_5_values = columns[15].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_6_values = columns[16].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_7_values = columns[17].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_8_values = columns[18].as_any().downcast_ref::<Int64Array>().unwrap();
        let ask_price_9_values = columns[19].as_any().downcast_ref::<Int64Array>().unwrap();

        let bid_size_0_values = columns[20].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_1_values = columns[21].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_2_values = columns[22].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_3_values = columns[23].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_4_values = columns[24].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_5_values = columns[25].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_6_values = columns[26].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_7_values = columns[27].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_8_values = columns[28].as_any().downcast_ref::<UInt64Array>().unwrap();
        let bid_size_9_values = columns[29].as_any().downcast_ref::<UInt64Array>().unwrap();

        let ask_size_0_values = columns[30].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_1_values = columns[31].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_2_values = columns[32].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_3_values = columns[33].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_4_values = columns[34].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_5_values = columns[35].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_6_values = columns[36].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_7_values = columns[37].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_8_values = columns[38].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_9_values = columns[39].as_any().downcast_ref::<UInt64Array>().unwrap();

        let bid_counts_0_values = columns[40].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_1_values = columns[41].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_2_values = columns[42].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_3_values = columns[43].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_4_values = columns[44].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_5_values = columns[45].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_6_values = columns[46].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_7_values = columns[47].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_8_values = columns[48].as_any().downcast_ref::<UInt32Array>().unwrap();
        let bid_counts_9_values = columns[49].as_any().downcast_ref::<UInt32Array>().unwrap();

        let ask_counts_0_values = columns[50].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_1_values = columns[51].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_2_values = columns[52].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_3_values = columns[53].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_4_values = columns[54].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_5_values = columns[55].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_6_values = columns[56].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_7_values = columns[57].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_8_values = columns[58].as_any().downcast_ref::<UInt32Array>().unwrap();
        let ask_counts_9_values = columns[59].as_any().downcast_ref::<UInt32Array>().unwrap();

        let flags_values = columns[60].as_any().downcast_ref::<UInt8Array>().unwrap();
        let sequence_values = columns[61].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = columns[62].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[63].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 64);

        assert_eq!(bid_price_0_values.len(), 1);
        assert_eq!(bid_price_1_values.len(), 1);
        assert_eq!(bid_price_2_values.len(), 1);
        assert_eq!(bid_price_3_values.len(), 1);
        assert_eq!(bid_price_4_values.len(), 1);
        assert_eq!(bid_price_5_values.len(), 1);
        assert_eq!(bid_price_6_values.len(), 1);
        assert_eq!(bid_price_7_values.len(), 1);
        assert_eq!(bid_price_8_values.len(), 1);
        assert_eq!(bid_price_9_values.len(), 1);
        assert_eq!(bid_price_0_values.value(0), 99_000_000_000);
        assert_eq!(bid_price_1_values.value(0), 98_000_000_000);
        assert_eq!(bid_price_2_values.value(0), 97_000_000_000);
        assert_eq!(bid_price_3_values.value(0), 96_000_000_000);
        assert_eq!(bid_price_4_values.value(0), 95_000_000_000);
        assert_eq!(bid_price_5_values.value(0), 94_000_000_000);
        assert_eq!(bid_price_6_values.value(0), 93_000_000_000);
        assert_eq!(bid_price_7_values.value(0), 92_000_000_000);
        assert_eq!(bid_price_8_values.value(0), 91_000_000_000);
        assert_eq!(bid_price_9_values.value(0), 90_000_000_000);

        assert_eq!(ask_price_0_values.len(), 1);
        assert_eq!(ask_price_1_values.len(), 1);
        assert_eq!(ask_price_2_values.len(), 1);
        assert_eq!(ask_price_3_values.len(), 1);
        assert_eq!(ask_price_4_values.len(), 1);
        assert_eq!(ask_price_5_values.len(), 1);
        assert_eq!(ask_price_6_values.len(), 1);
        assert_eq!(ask_price_7_values.len(), 1);
        assert_eq!(ask_price_8_values.len(), 1);
        assert_eq!(ask_price_9_values.len(), 1);
        assert_eq!(ask_price_0_values.value(0), 100_000_000_000);
        assert_eq!(ask_price_1_values.value(0), 101_000_000_000);
        assert_eq!(ask_price_2_values.value(0), 102_000_000_000);
        assert_eq!(ask_price_3_values.value(0), 103_000_000_000);
        assert_eq!(ask_price_4_values.value(0), 104_000_000_000);
        assert_eq!(ask_price_5_values.value(0), 105_000_000_000);
        assert_eq!(ask_price_6_values.value(0), 106_000_000_000);
        assert_eq!(ask_price_7_values.value(0), 107_000_000_000);
        assert_eq!(ask_price_8_values.value(0), 108_000_000_000);
        assert_eq!(ask_price_9_values.value(0), 109_000_000_000);

        assert_eq!(bid_size_0_values.len(), 1);
        assert_eq!(bid_size_1_values.len(), 1);
        assert_eq!(bid_size_2_values.len(), 1);
        assert_eq!(bid_size_3_values.len(), 1);
        assert_eq!(bid_size_4_values.len(), 1);
        assert_eq!(bid_size_5_values.len(), 1);
        assert_eq!(bid_size_6_values.len(), 1);
        assert_eq!(bid_size_7_values.len(), 1);
        assert_eq!(bid_size_8_values.len(), 1);
        assert_eq!(bid_size_9_values.len(), 1);
        assert_eq!(bid_size_0_values.value(0), 100_000_000_000);
        assert_eq!(bid_size_1_values.value(0), 200_000_000_000);
        assert_eq!(bid_size_2_values.value(0), 300_000_000_000);
        assert_eq!(bid_size_3_values.value(0), 400_000_000_000);
        assert_eq!(bid_size_4_values.value(0), 500_000_000_000);
        assert_eq!(bid_size_5_values.value(0), 600_000_000_000);
        assert_eq!(bid_size_6_values.value(0), 700_000_000_000);
        assert_eq!(bid_size_7_values.value(0), 800_000_000_000);
        assert_eq!(bid_size_8_values.value(0), 900_000_000_000);
        assert_eq!(bid_size_9_values.value(0), 1_000_000_000_000);

        assert_eq!(ask_size_0_values.len(), 1);
        assert_eq!(ask_size_1_values.len(), 1);
        assert_eq!(ask_size_2_values.len(), 1);
        assert_eq!(ask_size_3_values.len(), 1);
        assert_eq!(ask_size_4_values.len(), 1);
        assert_eq!(ask_size_5_values.len(), 1);
        assert_eq!(ask_size_6_values.len(), 1);
        assert_eq!(ask_size_7_values.len(), 1);
        assert_eq!(ask_size_8_values.len(), 1);
        assert_eq!(ask_size_9_values.len(), 1);
        assert_eq!(ask_size_0_values.value(0), 100_000_000_000);
        assert_eq!(ask_size_1_values.value(0), 200_000_000_000);
        assert_eq!(ask_size_2_values.value(0), 300_000_000_000);
        assert_eq!(ask_size_3_values.value(0), 400_000_000_000);
        assert_eq!(ask_size_4_values.value(0), 500_000_000_000);
        assert_eq!(ask_size_5_values.value(0), 600_000_000_000);
        assert_eq!(ask_size_6_values.value(0), 700_000_000_000);
        assert_eq!(ask_size_7_values.value(0), 800_000_000_000);
        assert_eq!(ask_size_8_values.value(0), 900_000_000_000);
        assert_eq!(ask_size_9_values.value(0), 1_000_000_000_000);

        assert_eq!(bid_counts_0_values.len(), 1);
        assert_eq!(bid_counts_1_values.len(), 1);
        assert_eq!(bid_counts_2_values.len(), 1);
        assert_eq!(bid_counts_3_values.len(), 1);
        assert_eq!(bid_counts_4_values.len(), 1);
        assert_eq!(bid_counts_5_values.len(), 1);
        assert_eq!(bid_counts_6_values.len(), 1);
        assert_eq!(bid_counts_7_values.len(), 1);
        assert_eq!(bid_counts_8_values.len(), 1);
        assert_eq!(bid_counts_9_values.len(), 1);
        assert_eq!(bid_counts_0_values.value(0), 1);
        assert_eq!(bid_counts_1_values.value(0), 1);
        assert_eq!(bid_counts_2_values.value(0), 1);
        assert_eq!(bid_counts_3_values.value(0), 1);
        assert_eq!(bid_counts_4_values.value(0), 1);
        assert_eq!(bid_counts_5_values.value(0), 1);
        assert_eq!(bid_counts_6_values.value(0), 1);
        assert_eq!(bid_counts_7_values.value(0), 1);
        assert_eq!(bid_counts_8_values.value(0), 1);
        assert_eq!(bid_counts_9_values.value(0), 1);

        assert_eq!(ask_counts_0_values.len(), 1);
        assert_eq!(ask_counts_1_values.len(), 1);
        assert_eq!(ask_counts_2_values.len(), 1);
        assert_eq!(ask_counts_3_values.len(), 1);
        assert_eq!(ask_counts_4_values.len(), 1);
        assert_eq!(ask_counts_5_values.len(), 1);
        assert_eq!(ask_counts_6_values.len(), 1);
        assert_eq!(ask_counts_7_values.len(), 1);
        assert_eq!(ask_counts_8_values.len(), 1);
        assert_eq!(ask_counts_9_values.len(), 1);
        assert_eq!(ask_counts_0_values.value(0), 1);
        assert_eq!(ask_counts_1_values.value(0), 1);
        assert_eq!(ask_counts_2_values.value(0), 1);
        assert_eq!(ask_counts_3_values.value(0), 1);
        assert_eq!(ask_counts_4_values.value(0), 1);
        assert_eq!(ask_counts_5_values.value(0), 1);
        assert_eq!(ask_counts_6_values.value(0), 1);
        assert_eq!(ask_counts_7_values.value(0), 1);
        assert_eq!(ask_counts_8_values.value(0), 1);
        assert_eq!(ask_counts_9_values.value(0), 1);

        assert_eq!(flags_values.len(), 1);
        assert_eq!(flags_values.value(0), 0);

        assert_eq!(sequence_values.len(), 1);
        assert_eq!(sequence_values.value(0), 0);

        assert_eq!(ts_event_values.len(), 1);
        assert_eq!(ts_event_values.value(0), 1);

        assert_eq!(ts_init_values.len(), 1);
        assert_eq!(ts_init_values.value(0), 2);
    }

    #[rstest]
    fn test_decode_batch(stub_depth10: OrderBookDepth10) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDepth10::get_metadata(&instrument_id, 2, 0);

        let data = vec![stub_depth10];
        let record_batch = OrderBookDepth10::encode_batch(&metadata, &data).unwrap();
        let decoded_data = OrderBookDepth10::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded_data.len(), 1);
    }
}
