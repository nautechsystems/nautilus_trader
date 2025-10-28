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
    array::{
        Array, FixedSizeBinaryArray, FixedSizeBinaryBuilder, UInt8Array, UInt32Array, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        depth::{DEPTH10_LEN, OrderBookDepth10},
        order::BookOrder,
    },
    enums::OrderSide,
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

fn get_field_data() -> Vec<(&'static str, DataType)> {
    vec![
        ("bid_price", DataType::FixedSizeBinary(PRECISION_BYTES)),
        ("ask_price", DataType::FixedSizeBinary(PRECISION_BYTES)),
        ("bid_size", DataType::FixedSizeBinary(PRECISION_BYTES)),
        ("ask_size", DataType::FixedSizeBinary(PRECISION_BYTES)),
        ("bid_count", DataType::UInt32),
        ("ask_count", DataType::UInt32),
    ]
}

impl ArrowSchemaProvider for OrderBookDepth10 {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let mut fields = Vec::new();
        let field_data = get_field_data();

        // Schema is of the form:
        // bid_price_0, bid_price_1, ..., bid_price_9, ask_price_0, ask_price_1
        for (name, data_type) in field_data {
            for i in 0..DEPTH10_LEN {
                fields.push(Field::new(format!("{name}_{i}"), data_type.clone(), false));
            }
        }

        fields.push(Field::new("flags", DataType::UInt8, false));
        fields.push(Field::new("sequence", DataType::UInt64, false));
        fields.push(Field::new("ts_event", DataType::UInt64, false));
        fields.push(Field::new("ts_init", DataType::UInt64, false));

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
            bid_price_builders.push(FixedSizeBinaryBuilder::with_capacity(
                data.len(),
                PRECISION_BYTES,
            ));
            ask_price_builders.push(FixedSizeBinaryBuilder::with_capacity(
                data.len(),
                PRECISION_BYTES,
            ));
            bid_size_builders.push(FixedSizeBinaryBuilder::with_capacity(
                data.len(),
                PRECISION_BYTES,
            ));
            ask_size_builders.push(FixedSizeBinaryBuilder::with_capacity(
                data.len(),
                PRECISION_BYTES,
            ));
            bid_count_builders.push(UInt32Array::builder(data.len()));
            ask_count_builders.push(UInt32Array::builder(data.len()));
        }

        let mut flags_builder = UInt8Array::builder(data.len());
        let mut sequence_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for depth in data {
            for i in 0..DEPTH10_LEN {
                bid_price_builders[i]
                    .append_value(depth.bids[i].price.raw.to_le_bytes())
                    .unwrap();
                ask_price_builders[i]
                    .append_value(depth.asks[i].price.raw.to_le_bytes())
                    .unwrap();
                bid_size_builders[i]
                    .append_value(depth.bids[i].size.raw.to_le_bytes())
                    .unwrap();
                ask_size_builders[i]
                    .append_value(depth.asks[i].size.raw.to_le_bytes())
                    .unwrap();
                bid_count_builders[i].append_value(depth.bid_counts[i]);
                ask_count_builders[i].append_value(depth.ask_counts[i]);
            }

            flags_builder.append_value(depth.flags);
            sequence_builder.append_value(depth.sequence);
            ts_event_builder.append_value(depth.ts_event.as_u64());
            ts_init_builder.append_value(depth.ts_init.as_u64());
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

        let flags_array = Arc::new(flags_builder.finish()) as Arc<dyn Array>;
        let sequence_array = Arc::new(sequence_builder.finish()) as Arc<dyn Array>;
        let ts_event_array = Arc::new(ts_event_builder.finish()) as Arc<dyn Array>;
        let ts_init_array = Arc::new(ts_init_builder.finish()) as Arc<dyn Array>;

        let mut columns = Vec::new();
        columns.extend(bid_price_arrays);
        columns.extend(ask_price_arrays);
        columns.extend(bid_size_arrays);
        columns.extend(ask_size_arrays);
        columns.extend(bid_count_arrays);
        columns.extend(ask_count_arrays);
        columns.push(flags_array);
        columns.push(sequence_array);
        columns.push(ts_event_array);
        columns.push(ts_init_array);

        RecordBatch::try_new(Self::get_schema(Some(metadata.clone())).into(), columns)
    }

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(
            &self.instrument_id,
            self.bids[0].price.precision,
            self.bids[0].size.precision,
        )
    }
}

impl DecodeFromRecordBatch for OrderBookDepth10 {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        let mut bid_prices = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_prices = Vec::with_capacity(DEPTH10_LEN);
        let mut bid_sizes = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_sizes = Vec::with_capacity(DEPTH10_LEN);
        let mut bid_counts = Vec::with_capacity(DEPTH10_LEN);
        let mut ask_counts = Vec::with_capacity(DEPTH10_LEN);

        macro_rules! extract_depth_column {
            ($array:ty, $name:literal, $i:expr, $offset:expr, $type:expr) => {
                extract_column::<$array>(cols, concat!($name, "_", stringify!($i)), $offset, $type)?
            };
        }

        for i in 0..DEPTH10_LEN {
            bid_prices.push(extract_depth_column!(
                FixedSizeBinaryArray,
                "bid_price",
                i,
                i,
                DataType::FixedSizeBinary(PRECISION_BYTES)
            ));
            ask_prices.push(extract_depth_column!(
                FixedSizeBinaryArray,
                "ask_price",
                i,
                DEPTH10_LEN + i,
                DataType::FixedSizeBinary(PRECISION_BYTES)
            ));
            bid_sizes.push(extract_depth_column!(
                FixedSizeBinaryArray,
                "bid_size",
                i,
                2 * DEPTH10_LEN + i,
                DataType::FixedSizeBinary(PRECISION_BYTES)
            ));
            ask_sizes.push(extract_depth_column!(
                FixedSizeBinaryArray,
                "ask_size",
                i,
                3 * DEPTH10_LEN + i,
                DataType::FixedSizeBinary(PRECISION_BYTES)
            ));
            bid_counts.push(extract_depth_column!(
                UInt32Array,
                "bid_count",
                i,
                4 * DEPTH10_LEN + i,
                DataType::UInt32
            ));
            ask_counts.push(extract_depth_column!(
                UInt32Array,
                "ask_count",
                i,
                5 * DEPTH10_LEN + i,
                DataType::UInt32
            ));
        }

        for i in 0..DEPTH10_LEN {
            if bid_prices[i].value_length() != PRECISION_BYTES {
                return Err(EncodingError::ParseError(
                    "bid_price",
                    format!(
                        "Invalid value length at index {i}: expected {PRECISION_BYTES}, found {}",
                        bid_prices[i].value_length()
                    ),
                ));
            }
            if ask_prices[i].value_length() != PRECISION_BYTES {
                return Err(EncodingError::ParseError(
                    "ask_price",
                    format!(
                        "Invalid value length at index {i}: expected {PRECISION_BYTES}, found {}",
                        ask_prices[i].value_length()
                    ),
                ));
            }
            if bid_sizes[i].value_length() != PRECISION_BYTES {
                return Err(EncodingError::ParseError(
                    "bid_size",
                    format!(
                        "Invalid value length at index {i}: expected {PRECISION_BYTES}, found {}",
                        bid_sizes[i].value_length()
                    ),
                ));
            }
            if ask_sizes[i].value_length() != PRECISION_BYTES {
                return Err(EncodingError::ParseError(
                    "ask_size",
                    format!(
                        "Invalid value length at index {i}: expected {PRECISION_BYTES}, found {}",
                        ask_sizes[i].value_length()
                    ),
                ));
            }
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
            .map(|row| {
                let mut bids = [BookOrder::default(); DEPTH10_LEN];
                let mut asks = [BookOrder::default(); DEPTH10_LEN];
                let mut bid_count_arr = [0u32; DEPTH10_LEN];
                let mut ask_count_arr = [0u32; DEPTH10_LEN];

                for i in 0..DEPTH10_LEN {
                    bids[i] = BookOrder::new(
                        OrderSide::Buy,
                        Price::from_raw(get_raw_price(bid_prices[i].value(row)), price_precision),
                        Quantity::from_raw(
                            get_raw_quantity(bid_sizes[i].value(row)),
                            size_precision,
                        ),
                        0, // Order id always zero
                    );
                    asks[i] = BookOrder::new(
                        OrderSide::Sell,
                        Price::from_raw(get_raw_price(ask_prices[i].value(row)), price_precision),
                        Quantity::from_raw(
                            get_raw_quantity(ask_sizes[i].value(row)),
                            size_precision,
                        ),
                        0, // Order id always zero
                    );
                    bid_count_arr[i] = bid_counts[i].value(row);
                    ask_count_arr[i] = ask_counts[i].value(row);
                }

                Ok(Self {
                    instrument_id,
                    bids,
                    asks,
                    bid_counts: bid_count_arr,
                    ask_counts: ask_count_arr,
                    flags: flags.value(row),
                    sequence: sequence.value(row),
                    ts_event: ts_event.value(row).into(),
                    ts_init: ts_init.value(row).into(),
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
    use arrow::datatypes::{DataType, Field};
    use nautilus_model::{
        data::stubs::stub_depth10,
        types::{fixed::FIXED_SCALAR, price::PriceRaw, quantity::QuantityRaw},
    };
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = OrderBookDepth10::get_metadata(&instrument_id, 2, 0);
        let schema = OrderBookDepth10::get_schema(Some(metadata));

        let mut group_count = 0;
        let field_data = get_field_data();
        for (name, data_type) in field_data {
            for i in 0..DEPTH10_LEN {
                let field = schema.field(i + group_count * DEPTH10_LEN).clone();
                assert_eq!(
                    field,
                    Field::new(format!("{name}_{i}"), data_type.clone(), false)
                );
            }

            group_count += 1;
        }

        let flags_field = schema.field(group_count * DEPTH10_LEN).clone();
        assert_eq!(flags_field, Field::new("flags", DataType::UInt8, false));
        let sequence_field = schema.field(group_count * DEPTH10_LEN + 1).clone();
        assert_eq!(
            sequence_field,
            Field::new("sequence", DataType::UInt64, false)
        );
        let ts_event_field = schema.field(group_count * DEPTH10_LEN + 2).clone();
        assert_eq!(
            ts_event_field,
            Field::new("ts_event", DataType::UInt64, false)
        );
        let ts_init_field = schema.field(group_count * DEPTH10_LEN + 3).clone();
        assert_eq!(
            ts_init_field,
            Field::new("ts_init", DataType::UInt64, false)
        );

        assert_eq!(schema.metadata()["instrument_id"], "AAPL.XNAS");
        assert_eq!(schema.metadata()["price_precision"], "2");
        assert_eq!(schema.metadata()["size_precision"], "0");
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = OrderBookDepth10::get_schema_map();

        let field_data = get_field_data();
        for (name, data_type) in field_data {
            for i in 0..DEPTH10_LEN {
                let field = schema_map.get(&format!("{name}_{i}")).map(String::as_str);
                assert_eq!(field, Some(format!("{data_type:?}").as_str()));
            }
        }

        assert_eq!(schema_map.get("flags").map(String::as_str), Some("UInt8"));
        assert_eq!(
            schema_map.get("sequence").map(String::as_str),
            Some("UInt64")
        );
        assert_eq!(
            schema_map.get("ts_event").map(String::as_str),
            Some("UInt64")
        );
        assert_eq!(
            schema_map.get("ts_init").map(String::as_str),
            Some("UInt64")
        );
    }

    #[rstest]
    fn test_encode_batch(stub_depth10: OrderBookDepth10) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let price_precision = 2;
        let metadata = OrderBookDepth10::get_metadata(&instrument_id, price_precision, 0);

        let data = vec![stub_depth10];
        let record_batch = OrderBookDepth10::encode_batch(&metadata, &data).unwrap();
        let columns = record_batch.columns();

        assert_eq!(columns.len(), DEPTH10_LEN * 6 + 4);

        // Extract and test bid prices
        let bid_prices: Vec<_> = (0..DEPTH10_LEN)
            .map(|i| {
                columns[i]
                    .as_any()
                    .downcast_ref::<FixedSizeBinaryArray>()
                    .unwrap()
            })
            .collect();

        let expected_bid_prices: Vec<f64> =
            vec![99.0, 98.0, 97.0, 96.0, 95.0, 94.0, 93.0, 92.0, 91.0, 90.0];

        for (i, bid_price) in bid_prices.iter().enumerate() {
            assert_eq!(bid_price.len(), 1);
            assert_eq!(
                get_raw_price(bid_price.value(0)),
                (expected_bid_prices[i] * FIXED_SCALAR) as PriceRaw
            );
            assert_eq!(
                Price::from_raw(get_raw_price(bid_price.value(0)), price_precision).as_f64(),
                expected_bid_prices[i]
            );
        }

        // Extract and test ask prices
        let ask_prices: Vec<_> = (0..DEPTH10_LEN)
            .map(|i| {
                columns[DEPTH10_LEN + i]
                    .as_any()
                    .downcast_ref::<FixedSizeBinaryArray>()
                    .unwrap()
            })
            .collect();

        let expected_ask_prices: Vec<f64> = vec![
            100.0, 101.0, 102.0, 103.0, 104.0, 105.0, 106.0, 107.0, 108.0, 109.0,
        ];

        for (i, ask_price) in ask_prices.iter().enumerate() {
            assert_eq!(ask_price.len(), 1);
            assert_eq!(
                get_raw_price(ask_price.value(0)),
                (expected_ask_prices[i] * FIXED_SCALAR) as PriceRaw
            );
            assert_eq!(
                Price::from_raw(get_raw_price(ask_price.value(0)), price_precision).as_f64(),
                expected_ask_prices[i]
            );
        }

        // Extract and test bid sizes
        let bid_sizes: Vec<_> = (0..DEPTH10_LEN)
            .map(|i| {
                columns[2 * DEPTH10_LEN + i]
                    .as_any()
                    .downcast_ref::<FixedSizeBinaryArray>()
                    .unwrap()
            })
            .collect();

        for (i, bid_size) in bid_sizes.iter().enumerate() {
            assert_eq!(bid_size.len(), 1);
            assert_eq!(
                get_raw_quantity(bid_size.value(0)),
                ((100.0 * FIXED_SCALAR * (i + 1) as f64) as QuantityRaw)
            );
        }

        // Extract and test ask sizes
        let ask_sizes: Vec<_> = (0..DEPTH10_LEN)
            .map(|i| {
                columns[3 * DEPTH10_LEN + i]
                    .as_any()
                    .downcast_ref::<FixedSizeBinaryArray>()
                    .unwrap()
            })
            .collect();

        for (i, ask_size) in ask_sizes.iter().enumerate() {
            assert_eq!(ask_size.len(), 1);
            assert_eq!(
                get_raw_quantity(ask_size.value(0)),
                ((100.0 * FIXED_SCALAR * ((i + 1) as f64)) as QuantityRaw)
            );
        }

        // Extract and test bid counts
        let bid_counts: Vec<_> = (0..DEPTH10_LEN)
            .map(|i| {
                columns[4 * DEPTH10_LEN + i]
                    .as_any()
                    .downcast_ref::<UInt32Array>()
                    .unwrap()
            })
            .collect();

        for count_values in bid_counts {
            assert_eq!(count_values.len(), 1);
            assert_eq!(count_values.value(0), 1);
        }

        // Extract and test ask counts
        let ask_counts: Vec<_> = (0..DEPTH10_LEN)
            .map(|i| {
                columns[5 * DEPTH10_LEN + i]
                    .as_any()
                    .downcast_ref::<UInt32Array>()
                    .unwrap()
            })
            .collect();

        for count_values in ask_counts {
            assert_eq!(count_values.len(), 1);
            assert_eq!(count_values.value(0), 1);
        }

        // Test remaining fields
        let flags_values = columns[6 * DEPTH10_LEN]
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        let sequence_values = columns[6 * DEPTH10_LEN + 1]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_event_values = columns[6 * DEPTH10_LEN + 2]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_init_values = columns[6 * DEPTH10_LEN + 3]
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

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
