// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
    array::{Decimal128Array, StringArray, StringBuilder, StringViewArray, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
#[cfg(test)]
use nautilus_model::identifiers::InstrumentId;
use nautilus_model::{data::TradeTick, enums::AggressorSide, identifiers::TradeId};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_IDENTIFIER, decode_required_decimal_price,
    decode_required_decimal_quantity, decode_required_timestamp, enum_dictionary_array,
    enum_dictionary_data_type, extract_column, extract_column_string, fixed_decimal_data_type,
    identifier_array_from_display, parse_metadata, required_price_decimal_array,
    required_quantity_decimal_array,
};
#[cfg(test)]
use super::{KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for TradeTick {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("price", fixed_decimal_data_type(), true),
            Field::new("size", fixed_decimal_data_type(), true),
            Field::new("aggressor_side", enum_dictionary_data_type(), false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for TradeTick {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut trade_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for tick in data.iter().map(std::borrow::Borrow::borrow) {
            trade_id_builder.append_value(tick.trade_id.to_string());
            ts_event_builder.append_value(tick.ts_event.as_u64());
            ts_init_builder.append_value(tick.ts_init.as_u64());
        }

        let price_array = Arc::new(required_price_decimal_array(
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|tick| tick.price.raw),
            "price",
        )?);
        let size_array = Arc::new(required_quantity_decimal_array(
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|tick| tick.size.raw),
            "size",
        )?);
        let aggressor_side_array = Arc::new(enum_dictionary_array(
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|tick| tick.aggressor_side),
        )?);
        let trade_id_array = Arc::new(trade_id_builder.finish());
        let ts_event_array = Arc::new(ts_event_builder.finish());
        let ts_init_array = Arc::new(ts_init_builder.finish());

        crate::arrow::record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                price_array,
                size_array,
                aggressor_side_array,
                trade_id_array,
                ts_event_array,
                ts_init_array,
                Arc::new(identifier_array_from_display(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|tick| tick.instrument_id),
                )),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(
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
        let record_batch = crate::arrow::record_batch_with_u64_timestamps(&record_batch)?;
        let record_batch = &record_batch;
        let cols = record_batch.columns();

        let price_values =
            extract_column::<Decimal128Array>(cols, "price", 0, fixed_decimal_data_type())?;

        let size_values =
            extract_column::<Decimal128Array>(cols, "size", 1, fixed_decimal_data_type())?;

        let aggressor_side_values = extract_column_string(cols, "aggressor_side", 2)?;
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
                .enumerate()
                .map(|(i, id)| {
                    id.map(TradeId::from).ok_or_else(|| {
                        EncodingError::ParseError("trade_id", format!("NULL value at row {i}"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            extract_column::<StringArray>(cols, "trade_id", 3, DataType::Utf8)?
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    id.map(TradeId::from).ok_or_else(|| {
                        EncodingError::ParseError("trade_id", format!("NULL value at row {i}"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        };

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|i| {
                let price =
                    decode_required_decimal_price(price_values, price_precision, "price", i)?;
                let size =
                    decode_required_decimal_quantity(size_values, size_precision, "size", i)?;
                let aggressor_side_value = aggressor_side_values.value(i);
                let aggressor_side =
                    AggressorSide::from_str(aggressor_side_value).map_err(|e| {
                        EncodingError::ParseError(stringify!(AggressorSide), e.to_string())
                    })?;
                let trade_id = trade_id_values[i];
                let ts_event = decode_required_timestamp(ts_event_values, "ts_event", i)?;
                let ts_init = decode_required_timestamp(ts_init_values, "ts_init", i)?;

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Array, Decimal128Array, TimestampNanosecondArray, UInt64Array};
    use nautilus_model::types::{
        Price, Quantity, fixed::FIXED_SCALAR, price::PriceRaw, quantity::QuantityRaw,
    };
    use rstest::rstest;

    use super::*;
    use crate::arrow::{get_raw_price, get_raw_quantity};

    #[rstest]
    fn test_get_schema() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);
        let schema = TradeTick::get_schema(Some(metadata.clone()));

        let mut expected_fields = Vec::with_capacity(7);

        expected_fields.push(Field::new("price", fixed_decimal_data_type(), true));

        expected_fields.extend(vec![
            Field::new("size", fixed_decimal_data_type(), true),
            Field::new("aggressor_side", enum_dictionary_data_type(), false),
            Field::new("trade_id", DataType::Utf8, false),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
        ]);

        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = TradeTick::get_schema_map();
        let mut expected_map = HashMap::new();

        let precision_bytes = "Decimal128(38, 16)".to_string();
        expected_map.insert("price".to_string(), precision_bytes.clone());
        expected_map.insert("size".to_string(), precision_bytes);
        expected_map.insert(
            "aggressor_side".to_string(),
            "Dictionary(Int8, Utf8)".to_string(),
        );
        expected_map.insert("trade_id".to_string(), "Utf8".to_string());
        expected_map.insert(
            "ts_event".to_string(),
            "Timestamp(Nanosecond, Some(\"UTC\"))".to_string(),
        );
        expected_map.insert(
            "ts_init".to_string(),
            "Timestamp(Nanosecond, Some(\"UTC\"))".to_string(),
        );
        expected_map.insert(KEY_IDENTIFIER.to_string(), "Utf8".to_string());
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
            aggressor_side: AggressorSide::Buy,
            trade_id: TradeId::new("1"),
            ts_event: 1.into(),
            ts_init: 3.into(),
        };

        let tick2 = TradeTick {
            instrument_id,
            price: Price::from("100.50"),
            size: Quantity::from(500),
            aggressor_side: AggressorSide::Sell,
            trade_id: TradeId::new("2"),
            ts_event: 2.into(),
            ts_init: 4.into(),
        };

        let data = vec![tick1, tick2];
        let record_batch = TradeTick::encode_batch(&metadata, &data).unwrap();
        let columns = record_batch.columns();

        let price_values = columns[0]
            .as_any()
            .downcast_ref::<Decimal128Array>()
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
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        assert_eq!(
            get_raw_quantity(size_values.value(0)),
            (1000.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(
            get_raw_quantity(size_values.value(1)),
            (500.0 * FIXED_SCALAR) as QuantityRaw
        );

        let aggressor_side_values = extract_column_string(columns, "aggressor_side", 2).unwrap();
        let trade_id_values = columns[3].as_any().downcast_ref::<StringArray>().unwrap();
        let ts_event_values = columns[4]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_values = columns[5]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(columns.len(), 7);
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
        assert_eq!(aggressor_side_values.value(0), "BUY");
        assert_eq!(aggressor_side_values.value(1), "SELL");
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

        let raw_price1 = (100.00 * FIXED_SCALAR) as PriceRaw;
        let raw_price2 = (101.00 * FIXED_SCALAR) as PriceRaw;
        let price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &raw_price1.to_le_bytes(),
            &raw_price2.to_le_bytes(),
        ]);

        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((1000.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
            &((900.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let aggressor_side =
            enum_dictionary_array([AggressorSide::NoAggressor, AggressorSide::Buy]).unwrap();
        let trade_id = StringArray::from(vec!["1", "2"]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&TradeTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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
        assert_eq!(decoded_data[0].price, Price::from_raw(raw_price1, 2));
        assert_eq!(decoded_data[1].price, Price::from_raw(raw_price2, 2));
    }

    #[rstest]
    fn test_decode_batch_null_trade_id_returns_error() {
        use arrow::datatypes::Field;

        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let raw_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((1000.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let aggressor_side = enum_dictionary_array([AggressorSide::NoAggressor]).unwrap();

        let trade_id: StringArray = vec![None::<&str>].into();
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        // Create schema with nullable trade_id to simulate external data source
        let fields = vec![
            Field::new("price", fixed_decimal_data_type(), false),
            Field::new("size", fixed_decimal_data_type(), false),
            Field::new("aggressor_side", enum_dictionary_data_type(), false),
            Field::new("trade_id", DataType::Utf8, true), // nullable
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
        ];
        let schema = Schema::new_with_metadata(fields, metadata.clone());

        let record_batch = crate::arrow::record_batch_with_timestamps(
            schema.into(),
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

        let result = TradeTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("NULL value at row 0"),
            "Expected NULL error, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_invalid_price_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let invalid_price: PriceRaw = PriceRaw::MAX - 1000;
        let price = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &invalid_price.to_le_bytes(),
        ]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((1000.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let aggressor_side = enum_dictionary_array([AggressorSide::NoAggressor]).unwrap();
        let trade_id = StringArray::from(vec!["1"]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&TradeTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

        let result = TradeTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("price") && err.to_string().contains("row 0"),
            "Expected price error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_invalid_size_returns_error() {
        use nautilus_model::types::{fixed::FIXED_PRECISION, quantity::QUANTITY_RAW_MAX};

        let instrument_id = InstrumentId::from("AAPL.XNAS");
        // Decode the size at full precision so the out-of-range raw value bypasses the
        // precision-0 correction, which would otherwise round it back within the bound.
        let metadata = TradeTick::get_metadata(&instrument_id, 2, FIXED_PRECISION);

        let raw_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);

        let invalid_size = QUANTITY_RAW_MAX + 1;
        let size =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&invalid_size.to_le_bytes()]);
        let aggressor_side = enum_dictionary_array([AggressorSide::NoAggressor]).unwrap();
        let trade_id = StringArray::from(vec!["1"]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&TradeTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

        let result = TradeTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("size") && err.to_string().contains("row 0"),
            "Expected size error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_invalid_aggressor_side_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let raw_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((1000.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);

        let aggressor_side = enum_dictionary_array(["INVALID"]).unwrap();
        let trade_id = StringArray::from(vec!["1"]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&TradeTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

        let result = TradeTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("AggressorSide"),
            "Expected AggressorSide error, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_instrument_id_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = TradeTick::get_metadata(&instrument_id, 2, 0);
        metadata.remove(KEY_INSTRUMENT_ID);

        let raw_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((1000.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let aggressor_side = enum_dictionary_array([AggressorSide::NoAggressor]).unwrap();
        let trade_id = StringArray::from(vec!["1"]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&TradeTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

        let result = TradeTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("instrument_id"),
            "Expected missing instrument_id error, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_price_precision_returns_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut metadata = TradeTick::get_metadata(&instrument_id, 2, 0);
        metadata.remove(KEY_PRICE_PRECISION);

        let raw_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let price =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&raw_price.to_le_bytes()]);
        let size = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((1000.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let aggressor_side = enum_dictionary_array([AggressorSide::NoAggressor]).unwrap();
        let trade_id = StringArray::from(vec!["1"]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&TradeTick::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

        let result = TradeTick::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("price_precision"),
            "Expected missing price_precision error, was: {err}"
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let metadata = TradeTick::get_metadata(&instrument_id, 2, 0);

        let tick1 = TradeTick {
            instrument_id,
            price: Price::from("100.10"),
            size: Quantity::from(1000),
            aggressor_side: AggressorSide::Buy,
            trade_id: TradeId::new("trade-123"),
            ts_event: 1_000_000_000.into(),
            ts_init: 1_000_000_001.into(),
        };

        let tick2 = TradeTick {
            instrument_id,
            price: Price::from("100.50"),
            size: Quantity::from(500),
            aggressor_side: AggressorSide::Sell,
            trade_id: TradeId::new("trade-456"),
            ts_event: 2_000_000_000.into(),
            ts_init: 2_000_000_001.into(),
        };

        let original = vec![tick1, tick2];
        let record_batch = TradeTick::encode_batch(&metadata, &original).unwrap();
        let decoded = TradeTick::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.instrument_id, orig.instrument_id);
            assert_eq!(dec.price, orig.price);
            assert_eq!(dec.size, orig.size);
            assert_eq!(dec.aggressor_side, orig.aggressor_side);
            assert_eq!(dec.trade_id, orig.trade_id);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
