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
    array::{FixedSizeBinaryArray, FixedSizeBinaryBuilder, StringBuilder, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{Data, bar::BarType, custom::CustomData},
    types::fixed::PRECISION_BYTES,
};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION, decode_price, decode_quantity, extract_column,
    extract_column_string, validate_precision_bytes,
};
use rust_decimal::Decimal;

use crate::common::bar::BinanceBar;

const KEY_BAR_TYPE: &str = "bar_type";

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

impl ArrowSchemaProvider for BinanceBar {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        // Uses FixedSizeBinary for Price/Quantity (consistent with core Bar),
        // and Utf8 for Decimal fields (no binary convention for rust_decimal::Decimal).
        let fields = vec![
            Field::new("open", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("high", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("low", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("close", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("volume", DataType::FixedSizeBinary(PRECISION_BYTES), false),
            Field::new("quote_volume", DataType::Utf8, false),
            Field::new("count", DataType::UInt64, false),
            Field::new("taker_buy_base_volume", DataType::Utf8, false),
            Field::new("taker_buy_quote_volume", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for BinanceBar {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut open_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut high_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut low_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut close_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut volume_builder = FixedSizeBinaryBuilder::with_capacity(data.len(), PRECISION_BYTES);
        let mut quote_volume_builder = StringBuilder::with_capacity(data.len(), data.len() * 20);
        let mut count_builder = UInt64Array::builder(data.len());
        let mut taker_buy_base_volume_builder =
            StringBuilder::with_capacity(data.len(), data.len() * 20);
        let mut taker_buy_quote_volume_builder =
            StringBuilder::with_capacity(data.len(), data.len() * 20);
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for bar in data {
            open_builder
                .append_value(bar.open.raw.to_le_bytes())
                .unwrap();
            high_builder
                .append_value(bar.high.raw.to_le_bytes())
                .unwrap();
            low_builder.append_value(bar.low.raw.to_le_bytes()).unwrap();
            close_builder
                .append_value(bar.close.raw.to_le_bytes())
                .unwrap();
            volume_builder
                .append_value(bar.volume.raw.to_le_bytes())
                .unwrap();
            quote_volume_builder.append_value(bar.quote_volume.to_string());
            count_builder.append_value(bar.count);
            taker_buy_base_volume_builder.append_value(bar.taker_buy_base_volume.to_string());
            taker_buy_quote_volume_builder.append_value(bar.taker_buy_quote_volume.to_string());
            ts_event_builder.append_value(bar.ts_event.as_u64());
            ts_init_builder.append_value(bar.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(open_builder.finish()),
                Arc::new(high_builder.finish()),
                Arc::new(low_builder.finish()),
                Arc::new(close_builder.finish()),
                Arc::new(volume_builder.finish()),
                Arc::new(quote_volume_builder.finish()),
                Arc::new(count_builder.finish()),
                Arc::new(taker_buy_base_volume_builder.finish()),
                Arc::new(taker_buy_quote_volume_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = Self::get_metadata(&self.bar_type);
        metadata.insert(
            KEY_PRICE_PRECISION.to_string(),
            self.open.precision.to_string(),
        );
        metadata.insert(
            KEY_SIZE_PRECISION.to_string(),
            self.volume.precision.to_string(),
        );
        metadata
    }
}

/// Encodes a vector of [`BinanceBar`] into an Arrow `RecordBatch`.
///
/// # Errors
///
/// Returns an error if `data` is empty or encoding fails.
#[expect(clippy::missing_panics_doc)] // Guarded by empty check
pub fn binance_bar_to_arrow_record_batch(
    data: &[BinanceBar],
) -> Result<RecordBatch, EncodingError> {
    if data.is_empty() {
        return Err(EncodingError::EmptyData);
    }

    let first = data
        .first()
        .expect("Chunk should have at least one element to encode");
    let metadata = first.metadata();
    BinanceBar::encode_batch(&metadata, data).map_err(EncodingError::ArrowError)
}

/// Decodes a `RecordBatch` into a vector of [`BinanceBar`].
///
/// # Errors
///
/// Returns an `EncodingError` if decoding fails.
pub fn decode_binance_bar_batch(
    metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<Vec<BinanceBar>, EncodingError> {
    let (bar_type, price_precision, size_precision) = parse_metadata(metadata)?;
    let cols = record_batch.columns();

    let open_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "open",
        0,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let high_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "high",
        1,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let low_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "low",
        2,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let close_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "close",
        3,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let volume_values = extract_column::<FixedSizeBinaryArray>(
        cols,
        "volume",
        4,
        DataType::FixedSizeBinary(PRECISION_BYTES),
    )?;
    let quote_volume_values = extract_column_string(cols, "quote_volume", 5)?;
    let count_values = extract_column::<UInt64Array>(cols, "count", 6, DataType::UInt64)?;
    let taker_buy_base_volume_values = extract_column_string(cols, "taker_buy_base_volume", 7)?;
    let taker_buy_quote_volume_values = extract_column_string(cols, "taker_buy_quote_volume", 8)?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 9, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 10, DataType::UInt64)?;

    validate_precision_bytes(open_values, "open")?;
    validate_precision_bytes(high_values, "high")?;
    validate_precision_bytes(low_values, "low")?;
    validate_precision_bytes(close_values, "close")?;
    validate_precision_bytes(volume_values, "volume")?;

    (0..record_batch.num_rows())
        .map(|row| {
            let open = decode_price(open_values.value(row), price_precision, "open", row)?;
            let high = decode_price(high_values.value(row), price_precision, "high", row)?;
            let low = decode_price(low_values.value(row), price_precision, "low", row)?;
            let close = decode_price(close_values.value(row), price_precision, "close", row)?;
            let volume = decode_quantity(volume_values.value(row), size_precision, "volume", row)?;

            let quote_volume = Decimal::from_str(quote_volume_values.value(row))
                .map_err(|e| EncodingError::ParseError("quote_volume", e.to_string()))?;
            let taker_buy_base_volume = Decimal::from_str(taker_buy_base_volume_values.value(row))
                .map_err(|e| EncodingError::ParseError("taker_buy_base_volume", e.to_string()))?;
            let taker_buy_quote_volume =
                Decimal::from_str(taker_buy_quote_volume_values.value(row)).map_err(|e| {
                    EncodingError::ParseError("taker_buy_quote_volume", e.to_string())
                })?;

            Ok(BinanceBar::new(
                bar_type,
                open,
                high,
                low,
                close,
                volume,
                quote_volume,
                count_values.value(row),
                taker_buy_base_volume,
                taker_buy_quote_volume,
                ts_event_values.value(row).into(),
                ts_init_values.value(row).into(),
            ))
        })
        .collect()
}

impl DecodeDataFromRecordBatch for BinanceBar {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        let items = decode_binance_bar_batch(metadata, &record_batch)?;
        Ok(items
            .into_iter()
            .map(|item| Data::Custom(CustomData::from_arc(Arc::new(item))))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::{Price, Quantity};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn stub_binance_bar() -> BinanceBar {
        BinanceBar::new(
            BarType::from("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            Price::from("0.01634790"),
            Price::from("0.01640000"),
            Price::from("0.01575800"),
            Price::from("0.01577100"),
            Quantity::from("148976.11427815"),
            dec!(2434.19055334),
            100,
            dec!(1756.87402397),
            dec!(28.46694368),
            1_650_000_000_000_000_000u64.into(),
            1_650_000_000_000_000_000u64.into(),
        )
    }

    #[rstest]
    fn test_get_schema() {
        let schema = BinanceBar::get_schema(None);
        assert_eq!(schema.fields().len(), 11);
        assert_eq!(schema.field(0).name(), "open");
        assert_eq!(schema.field(5).name(), "quote_volume");
        assert_eq!(schema.field(5).data_type(), &DataType::Utf8);
        assert_eq!(schema.field(6).name(), "count");
        assert_eq!(schema.field(6).data_type(), &DataType::UInt64);
    }

    #[rstest]
    fn test_encode_decode_round_trip() {
        let bar = stub_binance_bar();
        let metadata = bar.metadata();
        let data = vec![bar.clone()];

        let record_batch = BinanceBar::encode_batch(&metadata, &data).unwrap();
        let decoded = decode_binance_bar_batch(&metadata, &record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0], bar);
    }

    #[rstest]
    fn test_encode_decode_multiple_bars() {
        let bar1 = stub_binance_bar();
        let bar2 = BinanceBar::new(
            BarType::from("BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL"),
            Price::from("0.01700000"),
            Price::from("0.01710000"),
            Price::from("0.01690000"),
            Price::from("0.01695000"),
            Quantity::from("50000.00000000"),
            dec!(1000.00000000),
            50,
            dec!(500.00000000),
            dec!(10.00000000),
            1_650_000_060_000_000_000u64.into(),
            1_650_000_060_000_000_000u64.into(),
        );

        let metadata = bar1.metadata();
        let data = vec![bar1.clone(), bar2.clone()];

        let record_batch = BinanceBar::encode_batch(&metadata, &data).unwrap();
        let decoded = decode_binance_bar_batch(&metadata, &record_batch).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0], bar1);
        assert_eq!(decoded[1], bar2);
    }

    #[rstest]
    fn test_decode_data_batch_returns_custom_data() {
        let bar = stub_binance_bar();
        let metadata = bar.metadata();
        let data = vec![bar];

        let record_batch = BinanceBar::encode_batch(&metadata, &data).unwrap();
        let decoded = BinanceBar::decode_data_batch(&metadata, record_batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert!(matches!(decoded[0], Data::Custom(_)));
    }
}
