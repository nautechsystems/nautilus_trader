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

//! Display-mode Arrow encoder for [`QuoteTick`].

use std::sync::Arc;

use arrow::{
    array::{Float64Builder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::QuoteTick;

use super::{
    float64_field, price_to_f64, quantity_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`QuoteTick`].
#[must_use]
pub fn quotes_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("bid_price", false),
        float64_field("ask_price", false),
        float64_field("bid_size", false),
        float64_field("ask_size", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

/// Encodes quotes as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Float64` columns for prices and sizes, a `Utf8` `instrument_id`
/// column, and `Timestamp(Nanosecond)` columns for event and init times.
/// Mixed-instrument batches are supported. Precision is lost in the
/// conversion to `f64`; use [`crate::arrow::quotes_to_arrow_record_batch_bytes`]
/// for catalog storage.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_quotes(data: &[QuoteTick]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id_builder = StringBuilder::new();
    let mut bid_price_builder = Float64Builder::with_capacity(data.len());
    let mut ask_price_builder = Float64Builder::with_capacity(data.len());
    let mut bid_size_builder = Float64Builder::with_capacity(data.len());
    let mut ask_size_builder = Float64Builder::with_capacity(data.len());
    let mut ts_event_builder = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init_builder = TimestampNanosecondBuilder::with_capacity(data.len());

    for quote in data {
        instrument_id_builder.append_value(quote.instrument_id.to_string());
        bid_price_builder.append_value(price_to_f64(&quote.bid_price));
        ask_price_builder.append_value(price_to_f64(&quote.ask_price));
        bid_size_builder.append_value(quantity_to_f64(&quote.bid_size));
        ask_size_builder.append_value(quantity_to_f64(&quote.ask_size));
        ts_event_builder.append_value(unix_nanos_to_i64(quote.ts_event.as_u64()));
        ts_init_builder.append_value(unix_nanos_to_i64(quote.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(quotes_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(bid_price_builder.finish()),
            Arc::new(ask_price_builder.finish()),
            Arc::new(bid_size_builder.finish()),
            Arc::new(ask_size_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Array, Float64Array, StringArray, TimestampNanosecondArray},
        datatypes::{DataType, TimeUnit},
    };
    use nautilus_model::{
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn make_quote(instrument_id: &str, bid: &str, ask: &str, ts: u64) -> QuoteTick {
        QuoteTick {
            instrument_id: InstrumentId::from(instrument_id),
            bid_price: Price::from(bid),
            ask_price: Price::from(ask),
            bid_size: Quantity::from(1_000),
            ask_size: Quantity::from(500),
            ts_event: ts.into(),
            ts_init: (ts + 1).into(),
        }
    }

    #[rstest]
    fn test_encode_quotes_schema() {
        let quotes = vec![make_quote("AAPL.XNAS", "100.10", "100.20", 1)];
        let batch = encode_quotes(&quotes).unwrap();

        let schema = batch.schema();
        let fields = schema.fields();
        assert_eq!(fields.len(), 7);
        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[1].name(), "bid_price");
        assert_eq!(fields[1].data_type(), &DataType::Float64);
        assert_eq!(fields[2].name(), "ask_price");
        assert_eq!(fields[2].data_type(), &DataType::Float64);
        assert_eq!(fields[3].name(), "bid_size");
        assert_eq!(fields[3].data_type(), &DataType::Float64);
        assert_eq!(fields[4].name(), "ask_size");
        assert_eq!(fields[4].data_type(), &DataType::Float64);
        assert_eq!(fields[5].name(), "ts_event");
        assert_eq!(
            fields[5].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[6].name(), "ts_init");
        assert_eq!(
            fields[6].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
    }

    #[rstest]
    fn test_encode_quotes_values() {
        let quotes = vec![
            make_quote("AAPL.XNAS", "100.10", "100.20", 1_000_000_000),
            make_quote("AAPL.XNAS", "100.15", "100.25", 2_000_000_000),
        ];
        let batch = encode_quotes(&quotes).unwrap();

        assert_eq!(batch.num_rows(), 2);

        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let bid_price_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ask_price_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let bid_size_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ask_size_col = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ts_event_col = batch
            .column(5)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_col = batch
            .column(6)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "AAPL.XNAS");
        assert!((bid_price_col.value(0) - 100.10).abs() < 1e-9);
        assert!((bid_price_col.value(1) - 100.15).abs() < 1e-9);
        assert!((ask_price_col.value(0) - 100.20).abs() < 1e-9);
        assert!((ask_price_col.value(1) - 100.25).abs() < 1e-9);
        assert!((bid_size_col.value(0) - 1_000.0).abs() < 1e-9);
        assert!((ask_size_col.value(0) - 500.0).abs() < 1e-9);
        assert_eq!(ts_event_col.value(0), 1_000_000_000);
        assert_eq!(ts_event_col.value(1), 2_000_000_000);
        assert_eq!(ts_init_col.value(0), 1_000_000_001);
        assert_eq!(ts_init_col.value(1), 2_000_000_001);
    }

    #[rstest]
    fn test_encode_quotes_empty() {
        let batch = encode_quotes(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.schema().fields().len(), 7);
    }

    #[rstest]
    fn test_encode_quotes_mixed_instruments() {
        let quotes = vec![
            make_quote("AAPL.XNAS", "100.10", "100.20", 1),
            make_quote("MSFT.XNAS", "250.00", "250.05", 2),
        ];
        let batch = encode_quotes(&quotes).unwrap();

        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
