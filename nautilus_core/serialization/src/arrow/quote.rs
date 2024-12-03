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

use arrow::{
    array::{FixedSizeBinaryArray, FixedSizeBinaryBuilder, Int64Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::quote::QuoteTick,
    identifiers::InstrumentId,
    types::{price::Price, price::PriceRaw, quantity::Quantity},
};

use super::{
    extract_column, get_raw_price, DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION
};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for QuoteTick {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let mut fields = Vec::with_capacity(6);

        // Add price fields with appropriate type based on precision
        #[cfg(not(feature = "high_precision"))]
        {
            fields.push(Field::new("bid_price", DataType::Int64, false));
            fields.push(Field::new("ask_price", DataType::Int64, false));
        }
        #[cfg(feature = "high_precision")]
        {
            fields.push(Field::new(
                "bid_price",
                DataType::FixedSizeBinary(16),
                false,
            ));
            fields.push(Field::new(
                "ask_price",
                DataType::FixedSizeBinary(16),
                false,
            ));
        }

        // Add remaining fields (unchanged)
        fields.extend(vec![
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ]);

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

impl EncodeToRecordBatch for QuoteTick {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        #[cfg(not(feature = "high_precision"))]
        let mut bid_price_builder = Int64Array::builder(data.len());
        #[cfg(not(feature = "high_precision"))]
        let mut ask_price_builder = Int64Array::builder(data.len());
        #[cfg(feature = "high_precision")]
        let mut bid_price_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), 16);
        #[cfg(feature = "high_precision")]
        let mut ask_price_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), 16);

        let mut bid_size_builder = UInt64Array::builder(data.len());
        let mut ask_size_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for quote in data {
            #[cfg(not(feature = "high_precision"))]
            {
                bid_price_builder.append_value(quote.bid_price.raw);
                ask_price_builder.append_value(quote.ask_price.raw);
            }
            #[cfg(feature = "high_precision")]
            {
                bid_price_builder
                    .append_value(quote.bid_price.raw.to_le_bytes())
                    .unwrap();
                ask_price_builder
                    .append_value(quote.ask_price.raw.to_le_bytes())
                    .unwrap();
            }
            bid_size_builder.append_value(quote.bid_size.raw);
            ask_size_builder.append_value(quote.ask_size.raw);
            ts_event_builder.append_value(quote.ts_event.as_u64());
            ts_init_builder.append_value(quote.ts_init.as_u64());
        }

        let bid_price_array = Arc::new(bid_price_builder.finish());
        let ask_price_array = Arc::new(ask_price_builder.finish());
        let bid_size_array = Arc::new(bid_size_builder.finish());
        let ask_size_array = Arc::new(ask_size_builder.finish());
        let ts_event_array = Arc::new(ts_event_builder.finish());
        let ts_init_array = Arc::new(ts_init_builder.finish());

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                bid_price_array,
                ask_price_array,
                bid_size_array,
                ask_size_array,
                ts_event_array,
                ts_init_array,
            ],
        )
    }
}

impl DecodeFromRecordBatch for QuoteTick {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        #[cfg(not(feature = "high_precision"))]
        let (bid_price_values, ask_price_values) = {
            let bid_price_values =
                extract_column::<Int64Array>(cols, "bid_price", 0, DataType::Int64)?;
            let ask_price_values =
                extract_column::<Int64Array>(cols, "ask_price", 1, DataType::Int64)?;
            (bid_price_values, ask_price_values)
        };

        #[cfg(feature = "high_precision")]
        let (bid_price_values, ask_price_values) = {
            let bid_price_values = extract_column::<FixedSizeBinaryArray>(
                cols,
                "bid_price",
                0,
                DataType::FixedSizeBinary(16),
            )?;
            let ask_price_values = extract_column::<FixedSizeBinaryArray>(
                cols,
                "ask_price",
                1,
                DataType::FixedSizeBinary(16),
            )?;
            (bid_price_values, ask_price_values)
        };

        let bid_size_values = extract_column::<UInt64Array>(cols, "bid_size", 2, DataType::UInt64)?;
        let ask_size_values = extract_column::<UInt64Array>(cols, "ask_size", 3, DataType::UInt64)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 4, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 5, DataType::UInt64)?;

        #[cfg(feature = "high_precision")]
        {
            assert_eq!(
                bid_price_values.value_length(),
                16,
                "High precision uses 128 bit/16 byte value"
            );
            assert_eq!(
                ask_price_values.value_length(),
                16,
                "High precision uses 128 bit/16 byte value"
            );
        }

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|row| {
                #[cfg(not(feature = "high_precision"))]
                let (bid_price, ask_price) = (
                    Price::from_raw(bid_price_values.value(row), price_precision),
                    Price::from_raw(ask_price_values.value(row), price_precision),
                );

                #[cfg(feature = "high_precision")]
                let (bid_price, ask_price) = (
                    Price::from_raw(
                        get_raw_price(bid_price_values.value(row).try_into().unwrap()),
                        price_precision,
                    ),
                    Price::from_raw(
                        get_raw_price(ask_price_values.value(row).try_into().unwrap()),
                        price_precision,
                    ),
                );

                Ok(Self {
                    instrument_id,
                    bid_price,
                    ask_price,
                    bid_size: Quantity::from_raw(bid_size_values.value(row), size_precision),
                    ask_size: Quantity::from_raw(ask_size_values.value(row), size_precision),
                    ts_event: ts_event_values.value(row).into(),
                    ts_init: ts_init_values.value(row).into(),
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for QuoteTick {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let ticks: Vec<Self> = Self::decode_batch(metadata, record_batch)?;
        Ok(ticks.into_iter().map(Data::from).collect())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use arrow::record_batch::RecordBatch;
    use nautilus_model::types::fixed::FIXED_HIGH_PRECISION_SCALAR;
    use rstest::rstest;

    use crate::arrow::get_raw_price;

    use super::*;

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        let schema = QuoteTick::get_schema(Some(metadata.clone()));

        let mut expected_fields = Vec::with_capacity(6);

        #[cfg(not(feature = "high_precision"))]
        {
            expected_fields.push(Field::new("bid_price", DataType::Int64, false));
            expected_fields.push(Field::new("ask_price", DataType::Int64, false));
        }
        #[cfg(feature = "high_precision")]
        {
            expected_fields.push(Field::new(
                "bid_price",
                DataType::FixedSizeBinary(16),
                false,
            ));
            expected_fields.push(Field::new(
                "ask_price",
                DataType::FixedSizeBinary(16),
                false,
            ));
        }

        expected_fields.extend(vec![
            Field::new("bid_size", DataType::UInt64, false),
            Field::new("ask_size", DataType::UInt64, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ]);

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let arrow_schema = QuoteTick::get_schema_map();
        let mut expected_map = HashMap::new();

        #[cfg(not(feature = "high_precision"))]
        {
            expected_map.insert("bid_price".to_string(), "Int64".to_string());
            expected_map.insert("ask_price".to_string(), "Int64".to_string());
        }
        #[cfg(feature = "high_precision")]
        {
            expected_map.insert("bid_price".to_string(), "FixedSizeBinary(16)".to_string());
            expected_map.insert("ask_price".to_string(), "FixedSizeBinary(16)".to_string());
        }

        expected_map.insert("bid_size".to_string(), "UInt64".to_string());
        expected_map.insert("ask_size".to_string(), "UInt64".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(arrow_schema, expected_map);
    }

    #[rstest]
    fn test_encode_quote_tick() {
        // Create test data
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let tick1 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.10"),
            ask_price: Price::from("101.50"),
            bid_size: Quantity::from(1000),
            ask_size: Quantity::from(500),
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let tick2 = QuoteTick {
            instrument_id,
            bid_price: Price::from("100.75"),
            ask_price: Price::from("100.20"),
            bid_size: Quantity::from(750),
            ask_size: Quantity::from(300),
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![tick1, tick2];
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);
        let record_batch = QuoteTick::encode_batch(&metadata, &data).unwrap();

        // Verify the encoded data
        let columns = record_batch.columns();

        #[cfg(not(feature = "high_precision"))]
        {
            let bid_price_values = columns[0].as_any().downcast_ref::<Int64Array>().unwrap();
            let ask_price_values = columns[1].as_any().downcast_ref::<Int64Array>().unwrap();
            assert_eq!(bid_price_values.value(0), 100_100_000_000);
            assert_eq!(bid_price_values.value(1), 100_750_000_000);
            assert_eq!(ask_price_values.value(0), 101_500_000_000);
            assert_eq!(ask_price_values.value(1), 100_200_000_000);
        }

        #[cfg(feature = "high_precision")]
        {
            let bid_price_values = columns[0]
                .as_any()
                .downcast_ref::<FixedSizeBinaryArray>()
                .unwrap();
            let ask_price_values = columns[1]
                .as_any()
                .downcast_ref::<FixedSizeBinaryArray>()
                .unwrap();
            assert_eq!(
                get_raw_price(bid_price_values.value(0).try_into().unwrap()),
                (100.10 * FIXED_HIGH_PRECISION_SCALAR) as i128
            );
            assert_eq!(
                get_raw_price(bid_price_values.value(1).try_into().unwrap()),
                (100.75 * FIXED_HIGH_PRECISION_SCALAR) as i128
            );
            assert_eq!(
                get_raw_price(ask_price_values.value(0).try_into().unwrap()),
                (101.50 * FIXED_HIGH_PRECISION_SCALAR) as i128
            );
            assert_eq!(
                get_raw_price(ask_price_values.value(1).try_into().unwrap()),
                (100.20 * FIXED_HIGH_PRECISION_SCALAR) as i128
            );
        }

        let bid_size_values = columns[2].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ask_size_values = columns[3].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_event_values = columns[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 6);
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
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = QuoteTick::get_metadata(&instrument_id, 2, 0);

        #[cfg(not(feature = "high_precision"))]
        let (bid_price, ask_price) = (
            Int64Array::from(vec![10000, 9900]),
            Int64Array::from(vec![10100, 10000]),
        );

        #[cfg(feature = "high_precision")]
        let (bid_price, ask_price) = (
            FixedSizeBinaryArray::from(vec![
                &(10000 as PriceRaw).to_le_bytes(),
                &(9900 as PriceRaw).to_le_bytes(),
            ]),
            FixedSizeBinaryArray::from(vec![
                &(10100 as PriceRaw).to_le_bytes(),
                &(10000 as PriceRaw).to_le_bytes(),
            ]),
        );

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

        let decoded_data = QuoteTick::decode_batch(&metadata, record_batch).unwrap();
        assert_eq!(decoded_data.len(), 2);

        // Verify decoded values
        assert_eq!(decoded_data[0].bid_price, Price::from_raw(10000, 2));
        assert_eq!(decoded_data[0].ask_price, Price::from_raw(10100, 2));
        assert_eq!(decoded_data[1].bid_price, Price::from_raw(9900, 2));
        assert_eq!(decoded_data[1].ask_price, Price::from_raw(10000, 2));
    }
}
