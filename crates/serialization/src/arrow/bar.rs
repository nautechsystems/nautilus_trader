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
    array::{Decimal128Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::{Bar, BarType};

use super::{
    DecodeDataFromRecordBatch, EncodingError, KEY_BAR_TYPE, KEY_IDENTIFIER, KEY_PRICE_PRECISION,
    KEY_SIZE_PRECISION, decode_required_decimal_price, decode_required_decimal_quantity,
    decode_required_timestamp, extract_column, fixed_decimal_data_type,
    identifier_array_from_display, required_price_decimal_array, required_quantity_decimal_array,
};
use crate::arrow::{ArrowSchemaProvider, Data, DecodeFromRecordBatch, EncodeToRecordBatch};

impl ArrowSchemaProvider for Bar {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("open", fixed_decimal_data_type(), true),
            Field::new("high", fixed_decimal_data_type(), true),
            Field::new("low", fixed_decimal_data_type(), true),
            Field::new("close", fixed_decimal_data_type(), true),
            Field::new("volume", fixed_decimal_data_type(), true),
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
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for bar in data.iter().map(std::borrow::Borrow::borrow) {
            ts_event_builder.append_value(bar.ts_event.as_u64());
            ts_init_builder.append_value(bar.ts_init.as_u64());
        }

        let ts_event_array = ts_event_builder.finish();
        let ts_init_array = ts_init_builder.finish();

        crate::arrow::record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(required_price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.open.raw),
                    "open",
                )?),
                Arc::new(required_price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.high.raw),
                    "high",
                )?),
                Arc::new(required_price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.low.raw),
                    "low",
                )?),
                Arc::new(required_price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.close.raw),
                    "close",
                )?),
                Arc::new(required_quantity_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.volume.raw),
                    "volume",
                )?),
                Arc::new(ts_event_array),
                Arc::new(ts_init_array),
                Arc::new(identifier_array_from_display(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.bar_type),
                )),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        Self::get_metadata(&self.bar_type, self.open.precision, self.volume.precision)
    }
}

impl DecodeFromRecordBatch for Bar {
    fn decode_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let (bar_type, price_precision, size_precision) = parse_metadata(metadata)?;
        let record_batch = crate::arrow::record_batch_with_u64_timestamps(&record_batch)?;
        let record_batch = &record_batch;
        let cols = record_batch.columns();

        let open_values =
            extract_column::<Decimal128Array>(cols, "open", 0, fixed_decimal_data_type())?;
        let high_values =
            extract_column::<Decimal128Array>(cols, "high", 1, fixed_decimal_data_type())?;
        let low_values =
            extract_column::<Decimal128Array>(cols, "low", 2, fixed_decimal_data_type())?;
        let close_values =
            extract_column::<Decimal128Array>(cols, "close", 3, fixed_decimal_data_type())?;
        let volume_values =
            extract_column::<Decimal128Array>(cols, "volume", 4, fixed_decimal_data_type())?;
        let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 5, DataType::UInt64)?;
        let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 6, DataType::UInt64)?;

        let result: Result<Vec<Self>, EncodingError> = (0..record_batch.num_rows())
            .map(|i| {
                let open = decode_required_decimal_price(open_values, price_precision, "open", i)?;
                let high = decode_required_decimal_price(high_values, price_precision, "high", i)?;
                let low = decode_required_decimal_price(low_values, price_precision, "low", i)?;
                let close =
                    decode_required_decimal_price(close_values, price_precision, "close", i)?;
                let volume =
                    decode_required_decimal_quantity(volume_values, size_precision, "volume", i)?;
                let ts_event = decode_required_timestamp(ts_event_values, "ts_event", i)?;
                let ts_init = decode_required_timestamp(ts_init_values, "ts_init", i)?;

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::array::{Array, TimestampNanosecondArray};
    use nautilus_model::types::{
        Price, Quantity, fixed::FIXED_SCALAR, price::PriceRaw, quantity::QuantityRaw,
    };
    use rstest::rstest;

    use super::*;
    use crate::arrow::{get_raw_price, get_raw_quantity};

    #[rstest]
    fn test_get_schema() {
        let bar_type = BarType::from_str("AAPL.XNAS-1-MINUTE-LAST-INTERNAL").unwrap();
        let metadata = Bar::get_metadata(&bar_type, 2, 0);
        let schema = Bar::get_schema(Some(metadata.clone()));
        let expected_fields = vec![
            Field::new("open", fixed_decimal_data_type(), true),
            Field::new("high", fixed_decimal_data_type(), true),
            Field::new("low", fixed_decimal_data_type(), true),
            Field::new("close", fixed_decimal_data_type(), true),
            Field::new("volume", fixed_decimal_data_type(), true),
            Field::new("ts_event", crate::arrow::timestamp_data_type(), false),
            Field::new("ts_init", crate::arrow::timestamp_data_type(), false),
            Field::new(KEY_IDENTIFIER, DataType::Utf8, true),
        ];
        let expected_schema = Schema::new_with_metadata(expected_fields, metadata);
        assert_eq!(schema, expected_schema);
    }

    #[rstest]
    fn test_get_schema_map() {
        let schema_map = Bar::get_schema_map();
        let mut expected_map = HashMap::new();
        let fixed_size_binary = "Decimal128(38, 16)".to_string();
        expected_map.insert("open".to_string(), fixed_size_binary.clone());
        expected_map.insert("high".to_string(), fixed_size_binary.clone());
        expected_map.insert("low".to_string(), fixed_size_binary.clone());
        expected_map.insert("close".to_string(), fixed_size_binary.clone());
        expected_map.insert("volume".to_string(), fixed_size_binary);
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
            1.into(),
            3.into(),
        );
        let bar2 = Bar::new(
            bar_type,
            Price::from("100.00"),
            Price::from("100.10"),
            Price::from("100.00"),
            Price::from("100.10"),
            Quantity::from(1110),
            2.into(),
            4.into(),
        );

        let data = vec![bar1, bar2];
        let record_batch = Bar::encode_batch(&metadata, &data).unwrap();

        let columns = record_batch.columns();
        let open_values = columns[0]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let high_values = columns[1]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let low_values = columns[2]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let close_values = columns[3]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let volume_values = columns[4]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap();
        let ts_event_values = columns[5]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_values = columns[6]
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(columns.len(), 8);
        assert_eq!(open_values.len(), 2);
        assert_eq!(
            get_raw_price(open_values.value(0)),
            (100.10 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(open_values.value(1)),
            (100.00 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(high_values.len(), 2);
        assert_eq!(
            get_raw_price(high_values.value(0)),
            (102.00 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(high_values.value(1)),
            (100.10 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(low_values.len(), 2);
        assert_eq!(
            get_raw_price(low_values.value(0)),
            (100.00 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(low_values.value(1)),
            (100.00 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(close_values.len(), 2);
        assert_eq!(
            get_raw_price(close_values.value(0)),
            (101.00 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(
            get_raw_price(close_values.value(1)),
            (100.10 * FIXED_SCALAR) as PriceRaw
        );
        assert_eq!(volume_values.len(), 2);
        assert_eq!(
            get_raw_quantity(volume_values.value(0)),
            (1100.0 * FIXED_SCALAR) as QuantityRaw
        );
        assert_eq!(
            get_raw_quantity(volume_values.value(1)),
            (1110.0 * FIXED_SCALAR) as QuantityRaw
        );
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

        let open = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.10 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((10.00 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let high = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((102.00 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((10.00 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let low = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.00 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((10.00 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let close = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((101.00 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
            &((10.01 * FIXED_SCALAR) as PriceRaw).to_le_bytes(),
        ]);
        let volume = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((11.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
            &((10.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1, 2]);
        let ts_init = UInt64Array::from(vec![3, 4]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&Bar::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

    #[rstest]
    fn test_decode_batch_invalid_price_returns_error() {
        let bar_type = BarType::from_str("AAPL.XNAS-1-MINUTE-LAST-INTERNAL").unwrap();
        let metadata = Bar::get_metadata(&bar_type, 2, 0);

        let invalid_price: PriceRaw = PriceRaw::MAX - 1000;
        let valid_price = (100.00 * FIXED_SCALAR) as PriceRaw;

        let open = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &invalid_price.to_le_bytes(),
        ]);
        let high =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let low =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let close =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let volume = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&Bar::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

        let result = Bar::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("open") && err.to_string().contains("row 0"),
            "Expected open error at row 0, was: {err}"
        );
    }

    #[rstest]
    fn test_decode_batch_missing_bar_type_returns_error() {
        let bar_type = BarType::from_str("AAPL.XNAS-1-MINUTE-LAST-INTERNAL").unwrap();
        let mut metadata = Bar::get_metadata(&bar_type, 2, 0);

        let valid_price = (100.00 * FIXED_SCALAR) as PriceRaw;
        let open =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let high =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let low =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let close =
            crate::arrow::test_support::decimal_array_from_bytes(vec![&valid_price.to_le_bytes()]);
        let volume = crate::arrow::test_support::decimal_array_from_bytes(vec![
            &((100.0 * FIXED_SCALAR) as QuantityRaw).to_le_bytes(),
        ]);
        let ts_event = UInt64Array::from(vec![1]);
        let ts_init = UInt64Array::from(vec![2]);

        let record_batch = crate::arrow::record_batch_with_timestamps(
            crate::arrow::schema_without_identifier_column(&Bar::get_schema(Some(
                metadata.clone(),
            )))
            .into(),
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

        metadata.remove(KEY_BAR_TYPE);

        let result = Bar::decode_batch(&metadata, record_batch);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("bar_type"),
            "Expected missing bar_type error, was: {err}"
        );
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let bar_type = BarType::from_str("AAPL.XNAS-1-MINUTE-LAST-INTERNAL").unwrap();
        let metadata = Bar::get_metadata(&bar_type, 2, 0);

        let bar1 = Bar::new(
            bar_type,
            Price::from("100.10"),
            Price::from("102.00"),
            Price::from("100.00"),
            Price::from("101.00"),
            Quantity::from(1100),
            1_000_000_000.into(),
            1_000_000_001.into(),
        );

        let bar2 = Bar::new(
            bar_type,
            Price::from("101.00"),
            Price::from("103.00"),
            Price::from("100.50"),
            Price::from("102.50"),
            Quantity::from(2200),
            2_000_000_000.into(),
            2_000_000_001.into(),
        );

        let original = vec![bar1, bar2];
        let record_batch = Bar::encode_batch(&metadata, &original).unwrap();
        let decoded = Bar::decode_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), original.len());
        for (orig, dec) in original.iter().zip(decoded.iter()) {
            assert_eq!(dec.bar_type, orig.bar_type);
            assert_eq!(dec.open, orig.open);
            assert_eq!(dec.high, orig.high);
            assert_eq!(dec.low, orig.low);
            assert_eq!(dec.close, orig.close);
            assert_eq!(dec.volume, orig.volume);
            assert_eq!(dec.ts_event, orig.ts_event);
            assert_eq!(dec.ts_init, orig.ts_init);
        }
    }
}
