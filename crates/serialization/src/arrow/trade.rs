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
        FixedSizeBinaryArray, FixedSizeBinaryBuilder, StringArray, StringBuilder, StringViewArray,
        UInt8Array, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity, fixed::PRECISION_BYTES},
};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION,
    KEY_SIZE_PRECISION, extract_column, get_raw_price, get_raw_quantity,
};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for TradeTick {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("price", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("size", DataType::FixedSizeBinary(PRECISION_BYTES), false),
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

impl EncodeToRecordBatch for TradeTick {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut price_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut size_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);

        let mut aggressor_side_builder = UInt8Array::builder(data.len());
        let mut trade_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for tick in data {
            price_builder
                .append_value(tick.price.raw.to_le_bytes())
                .unwrap();
            size_builder
                .append_value(tick.size.raw.to_le_bytes())
                .unwrap();
            aggressor_side_builder.append_value(tick.aggressor_side as u8);
            trade_id_builder.append_value(tick.trade_id.to_string());
            ts_event_builder.append_value(tick.ts_event.as_u64());
            ts_init_builder.append_value(tick.ts_init.as_u64());
        }

        let price_array = Arc::new(price_builder.finish());
        let size_array = Arc::new(size_builder.finish());
        let aggressor_side_array = Arc::new(aggressor_side_builder.finish());
        let trade_id_array = Arc::new(trade_id_builder.finish());
        let ts_event_array = Arc::new(ts_event_builder.finish());
        let ts_init_array = Arc::new(ts_init_builder.finish());

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                price_array,
                size_array,
                aggressor_side_array,
                trade_id_array,
                ts_event_array,
                ts_init_array,
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        TradeTick::get_metadata(
            &self.instrument_id,
            self.price.precision,
            self.size.precision,
        )
    }
}

impl DecodeFromRecordBatch for TradeTick {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (instrument_id, price_precision, size_precision) = parse_metadata(metadata)?;
        let cols = record_batch.columns();

        let price_values = extract_column::<FixedSizeBinaryArray>(
            cols,
            "price",
            0,
            DataType::FixedSizeBinary(PRECISION_BYTES),
        )?;

        let size_values = extract_column::<FixedSizeBinaryArray>(
            cols,
            "size",
            1,
            DataType::FixedSizeBinary(PRECISION_BYTES),
        )?;
        let aggressor_side_values =
            extract_column::<UInt8Array>(cols, "aggressor_side", 2, DataType::UInt8)?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 4, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 5, DataType::UInt64)?;

        // Datafusion reads trade_ids as StringView
        let trade_id_values: Vec<TradeId> = if record_batch
            .schema()
            .field_with_name("trade_id")?
            .data_type()
            == &DataType::Utf8View
        {
            extract_column::<StringViewArray>(cols, "trade_id", 3, DataType::Utf8View)?
                .iter()
                .map(|id| TradeId::from(id.unwrap()))
                .collect()
        } else {
            extract_column::<StringArray>(cols, "trade_id", 3, DataType::Utf8)?
                .iter()
                .map(|id| TradeId::from(id.unwrap()))
                .collect()
        };

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|i| {
                let price = Price::from_raw(get_raw_price(price_values.value(i)), price_precision);

                let size =
                    Quantity::from_raw(get_raw_quantity(size_values.value(i)), size_precision);
                let aggressor_side_value = aggressor_side_values.value(i);
                let aggressor_side = AggressorSide::from_repr(aggressor_side_value as usize)
                    .ok_or_else(|| {
                        EncodingError::ParseError(
                            stringify!(AggressorSide),
                            format!("Invalid enum value, was {aggressor_side_value}"),
                        )
                    })?;
                let trade_id = trade_id_values[i];
                let ts_event = ts_event_values.value(i).into();
                let ts_init = ts_init_values.value(i).into();

                Ok(Self {
                    instrument_id,
                    price,
                    size,
                    aggressor_side,
                    trade_id,
                    ts_event,
                    ts_init,
                })
            })
            .collect();

        result
    }
}

impl DecodeDataFromRecordBatch for TradeTick {
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
    use std::sync::Arc;

    use arrow::{
        array::{Array, FixedSizeBinaryArray, UInt8Array, UInt64Array},
        record_batch::RecordBatch,
    };
    use nautilus_model::types::{fixed::FIXED_SCALAR, price::PriceRaw, quantity::QuantityRaw};
    use rstest::rstest;

    use super::*;
    use crate::arrow::{get_raw_price, get_raw_quantity};

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);
        let schema = TradeTick::get_schema(Some(metadata.clone()));

        let mut expected_fields = Vec::with_capacity(6);

        expected_fields.push(Field::new(
            "price",
            DataType::FixedSizeBinary(PRECISION_BYTES),
            false,
        ));

        expected_fields.extend(vec![
            Field::new("size", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("aggressor_side", DataType::UInt8, false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ]);

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = TradeTick::get_schema_map();
        let mut expected_map = HashMap::new();

        let precision_bytes = format!("FixedSizeBinary({PRECISION_BYTES})");
        expected_map.insert("price".to_string(), precision_bytes.clone());
        expected_map.insert("size".to_string(), precision_bytes);
        expected_map.insert("aggressor_side".to_string(), "UInt8".to_string());
        expected_map.insert("trade_id".to_string(), "Utf8".to_string());
        expected_map.insert("ts_event".to_string(), "UInt64".to_string());
        expected_map.insert("ts_init".to_string(), "UInt64".to_string());
        assert_eq!(schema_map, expected_map);
    }

    #[rstest]
    fn test_encode_trade_tick() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let tick1 = TradeTick {
            instrument_id,
            price: Price::from("100.10"),
            size: Quantity::from(1000),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("1"),
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let tick2 = TradeTick {
            instrument_id,
            price: Price::from("100.50"),
            size: Quantity::from(500),
            aggressor_side: AggressorSide::Seller,
            trade_id: TradeId::new("2"),
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![tick1, tick2];
        let record_batch = TradeTick::encode_batch(&metadata, &data).unwrap();
        let columns = record_batch.columns();

        let price_values = columns[0]
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .unwrap();
        assert_eq!(
            get_raw_price(price_values.value(0)),
            (100.10 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(price_values.value(1)),
            (100.50 * FIXED_SCALAR) as PriceRaw
        );

        let size_values = columns[1]
            .as_any()
            .downcast_ref::<FixedSizeBinaryArray>()
            .unwrap();
        assert_eq!(
            get_raw_quantity(size_values.value(0)),
            (1000.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(
            get_raw_quantity(size_values.value(1)),
            (500.0 * FIXED_SCALAR) as QuantityRaw
        );

        let aggressor_side_values = columns[2].as_any().downcast_ref::<UInt8Array>().unwrap();
        let trade_id_values = columns[3].as_any().downcast_ref::<StringArray>().unwrap();
        let ts_event_values = columns[4].as_any().downcast_ref::<UInt64Array>().unwrap();
        let ts_init_values = columns[5].as_any().downcast_ref::<UInt64Array>().unwrap();

        assert_eq!(columns.len(), 6);
        assert_eq!(size_values.len(), 2);
        assert_eq!(
            get_raw_quantity(size_values.value(0)),
            (1000.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(
            get_raw_quantity(size_values.value(1)),
            (500.0 * FIXED_SCALAR) as QuantityRaw
        );
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

    #[rstest]
    fn test_decode_batch() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let price = FixedSizeBinaryArray::from(vec![
            &(1_000_000_000_000 as PriceRaw).to_le_bytes(),
            &(1_010_000_000_000 as PriceRaw).to_le_bytes(),
        ]);

        let size = FixedSizeBinaryArray::from(vec![
            &((1000.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
            &((900.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
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

        let decoded_data = TradeTick::decode_batch(&metadata, record_batch).unwrap();
        assert_eq!(decoded_data.len(), 2);
        assert_eq!(decoded_data[0].price, Price::from_raw(1_000_000_000_000, 2));
        assert_eq!(decoded_data[1].price, Price::from_raw(1_010_000_000_000, 2));
    }
}
