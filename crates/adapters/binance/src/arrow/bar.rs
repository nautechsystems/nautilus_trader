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
    array::{Array, Decimal128Array, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::{Data, bar::BarType, custom::CustomData};
use nautilus_serialization::arrow::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError,
    FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION,
    StringColumnRef, decimal_to_arrow, decode_decimal, decode_decimal_price,
    decode_decimal_quantity, extract_column, extract_decimal_column, fixed_decimal_data_type,
    price_decimal_array, quantity_decimal_array, record_batch_with_timestamps,
    record_batch_with_u64_timestamps, timestamp_data_type,
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
        let fields = vec![
            Field::new("open", fixed_decimal_data_type(), true),
            Field::new("high", fixed_decimal_data_type(), true),
            Field::new("low", fixed_decimal_data_type(), true),
            Field::new("close", fixed_decimal_data_type(), true),
            Field::new("volume", fixed_decimal_data_type(), true),
            Field::new("quote_volume", fixed_decimal_data_type(), false),
            Field::new("count", DataType::UInt64, false),
            Field::new("taker_buy_base_volume", fixed_decimal_data_type(), false),
            Field::new("taker_buy_quote_volume", fixed_decimal_data_type(), false),
            Field::new("ts_event", timestamp_data_type(), false),
            Field::new("ts_init", timestamp_data_type(), false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for BinanceBar {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        let mut count_builder = UInt64Array::builder(data.len());
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for bar in data.iter().map(std::borrow::Borrow::borrow) {
            count_builder.append_value(bar.count);
            ts_event_builder.append_value(bar.ts_event.as_u64());
            ts_init_builder.append_value(bar.ts_init.as_u64());
        }

        record_batch_with_timestamps(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.open.raw()),
                    "open",
                )?),
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.high.raw()),
                    "high",
                )?),
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.low.raw()),
                    "low",
                )?),
                Arc::new(price_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.close.raw()),
                    "close",
                )?),
                Arc::new(quantity_decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| bar.volume.raw()),
                    "volume",
                )?),
                Arc::new(decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| &bar.quote_volume),
                    "quote_volume",
                )?),
                Arc::new(count_builder.finish()),
                Arc::new(decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| &bar.taker_buy_base_volume),
                    "taker_buy_base_volume",
                )?),
                Arc::new(decimal_array(
                    data.iter()
                        .map(std::borrow::Borrow::borrow)
                        .map(|bar| &bar.taker_buy_quote_volume),
                    "taker_buy_quote_volume",
                )?),
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
    let record_batch = record_batch_with_u64_timestamps(record_batch)?;
    let cols = record_batch.columns();

    let open_values =
        extract_column::<Decimal128Array>(cols, "open", 0, fixed_decimal_data_type())?;
    let high_values =
        extract_column::<Decimal128Array>(cols, "high", 1, fixed_decimal_data_type())?;
    let low_values = extract_column::<Decimal128Array>(cols, "low", 2, fixed_decimal_data_type())?;
    let close_values =
        extract_column::<Decimal128Array>(cols, "close", 3, fixed_decimal_data_type())?;
    let volume_values =
        extract_column::<Decimal128Array>(cols, "volume", 4, fixed_decimal_data_type())?;
    let count_values = extract_column::<UInt64Array>(cols, "count", 6, DataType::UInt64)?;
    let ts_event_values = extract_column::<UInt64Array>(cols, "ts_event", 9, DataType::UInt64)?;
    let ts_init_values = extract_column::<UInt64Array>(cols, "ts_init", 10, DataType::UInt64)?;

    (0..record_batch.num_rows())
        .map(|row| {
            let open = decode_decimal_price(open_values, price_precision, "open", row)?;
            let high = decode_decimal_price(high_values, price_precision, "high", row)?;
            let low = decode_decimal_price(low_values, price_precision, "low", row)?;
            let close = decode_decimal_price(close_values, price_precision, "close", row)?;
            let volume = decode_decimal_quantity(volume_values, size_precision, "volume", row)?;
            let quote_volume = decode_decimal_column(&record_batch, "quote_volume", row)?;
            let taker_buy_base_volume =
                decode_decimal_column(&record_batch, "taker_buy_base_volume", row)?;
            let taker_buy_quote_volume =
                decode_decimal_column(&record_batch, "taker_buy_quote_volume", row)?;

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

fn decimal_array<'a>(
    values: impl IntoIterator<Item = &'a Decimal>,
    field: &'static str,
) -> Result<Decimal128Array, ArrowError> {
    let values = values
        .into_iter()
        .map(|value| decimal_to_arrow(value, field))
        .collect::<Result<Vec<_>, _>>()?;
    Decimal128Array::from(values)
        .with_precision_and_scale(FIXED_DECIMAL_PRECISION, FIXED_DECIMAL_SCALE)
}

fn decode_decimal_column(
    record_batch: &RecordBatch,
    field: &'static str,
    row: usize,
) -> Result<Decimal, EncodingError> {
    let index = record_batch.schema().index_of(field)?;
    let column = record_batch
        .columns()
        .get(index)
        .ok_or(EncodingError::MissingColumn(field, index))?;
    if column.data_type() == &fixed_decimal_data_type() {
        let values = extract_decimal_column(record_batch, field)?;
        return decode_decimal(values, field, row);
    }
    let values = StringColumnRef::try_from_array(column.as_ref()).ok_or_else(|| {
        EncodingError::ParseError(
            field,
            format!(
                "expected Decimal128(38, 16) or legacy string, was {}",
                column.data_type()
            ),
        )
    })?;

    if values.is_null(row) {
        return Err(EncodingError::ParseError(
            field,
            format!("row {row}: required decimal is null"),
        ));
    }
    Decimal::from_str(values.value(row))
        .map_err(|e| EncodingError::ParseError(field, format!("row {row}: {e}")))
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
    use arrow::array::StringArray;
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
        assert_eq!(schema.field(0).data_type(), &fixed_decimal_data_type());
        assert_eq!(schema.field(5).name(), "quote_volume");
        assert_eq!(schema.field(5).data_type(), &fixed_decimal_data_type());
        assert_eq!(schema.field(6).name(), "count");
        assert_eq!(schema.field(6).data_type(), &DataType::UInt64);
        assert_eq!(schema.field(9).data_type(), &timestamp_data_type());
        assert_eq!(schema.field(10).data_type(), &timestamp_data_type());
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

    #[rstest]
    fn test_decode_legacy_string_decimal_columns() {
        let bar = stub_binance_bar();
        let metadata = bar.metadata();
        let batch = legacy_string_batch(&bar, Some("2434.19055334"));

        let decoded = decode_binance_bar_batch(&metadata, &batch).unwrap();

        assert_eq!(decoded, vec![bar]);
    }

    #[rstest]
    fn test_decode_legacy_string_decimal_rejects_null() {
        let bar = stub_binance_bar();
        let metadata = bar.metadata();
        let batch = legacy_string_batch(&bar, None);

        let error = decode_binance_bar_batch(&metadata, &batch).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Error parsing `quote_volume`: row 0: required decimal is null",
        );
    }

    fn legacy_string_batch(bar: &BinanceBar, quote_volume: Option<&str>) -> RecordBatch {
        let metadata = bar.metadata();
        let batch = BinanceBar::encode_batch(&metadata, &[bar]).unwrap();
        let mut fields = batch
            .schema()
            .fields()
            .iter()
            .map(|field| field.as_ref().clone())
            .collect::<Vec<_>>();
        fields[5] = Field::new("quote_volume", DataType::Utf8, true);
        fields[7] = Field::new("taker_buy_base_volume", DataType::Utf8, false);
        fields[8] = Field::new("taker_buy_quote_volume", DataType::Utf8, false);
        let schema = Schema::new_with_metadata(fields, metadata);
        let mut columns = batch.columns().to_vec();
        columns[5] = Arc::new(StringArray::from(vec![quote_volume]));
        columns[7] = Arc::new(StringArray::from(vec![Some("1756.87402397")]));
        columns[8] = Arc::new(StringArray::from(vec![Some("28.46694368")]));

        RecordBatch::try_new(Arc::new(schema), columns).unwrap()
    }
}
