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

//! Display-mode Arrow encoder for [`Bar`].

use std::sync::Arc;

use arrow::{
    array::{Float64Builder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::Bar;

use super::{
    float64_field, price_to_f64, quantity_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`Bar`].
#[must_use]
pub fn bars_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        utf8_field("bar_type", false),
        float64_field("open", false),
        float64_field("high", false),
        float64_field("low", false),
        float64_field("close", false),
        float64_field("volume", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

/// Encodes bars as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Float64` columns for OHLCV values, `Utf8` columns for the
/// instrument ID and bar type, and `Timestamp(Nanosecond)` columns for
/// event and init times. Mixed-instrument batches are supported. Precision
/// is lost on the conversion to `f64`; use
/// [`crate::arrow::bars_to_arrow_record_batch_bytes`] for catalog storage.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_bars(data: &[Bar]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id_builder = StringBuilder::new();
    let mut bar_type_builder = StringBuilder::new();
    let mut open_builder = Float64Builder::with_capacity(data.len());
    let mut high_builder = Float64Builder::with_capacity(data.len());
    let mut low_builder = Float64Builder::with_capacity(data.len());
    let mut close_builder = Float64Builder::with_capacity(data.len());
    let mut volume_builder = Float64Builder::with_capacity(data.len());
    let mut ts_event_builder = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init_builder = TimestampNanosecondBuilder::with_capacity(data.len());

    for bar in data {
        instrument_id_builder.append_value(bar.instrument_id().to_string());
        bar_type_builder.append_value(bar.bar_type.to_string());
        open_builder.append_value(price_to_f64(&bar.open));
        high_builder.append_value(price_to_f64(&bar.high));
        low_builder.append_value(price_to_f64(&bar.low));
        close_builder.append_value(price_to_f64(&bar.close));
        volume_builder.append_value(quantity_to_f64(&bar.volume));
        ts_event_builder.append_value(unix_nanos_to_i64(bar.ts_event.as_u64()));
        ts_init_builder.append_value(unix_nanos_to_i64(bar.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(bars_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(bar_type_builder.finish()),
            Arc::new(open_builder.finish()),
            Arc::new(high_builder.finish()),
            Arc::new(low_builder.finish()),
            Arc::new(close_builder.finish()),
            Arc::new(volume_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use arrow::{
        array::{Array, Float64Array, StringArray, TimestampNanosecondArray},
        datatypes::{DataType, TimeUnit},
    };
    use nautilus_model::{
        data::BarType,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn make_bar(
        bar_type_str: &str,
        open: &str,
        high: &str,
        low: &str,
        close: &str,
        ts: u64,
    ) -> Bar {
        let bar_type = BarType::from_str(bar_type_str).unwrap();
        Bar::new(
            bar_type,
            Price::from(open),
            Price::from(high),
            Price::from(low),
            Price::from(close),
            Quantity::from(1_100),
            ts.into(),
            (ts + 1).into(),
        )
    }

    #[rstest]
    fn test_encode_bars_schema() {
        let batch = encode_bars(&[]).unwrap();
        let fields = batch.schema().fields().clone();
        assert_eq!(fields.len(), 9);
        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[1].name(), "bar_type");
        assert_eq!(fields[1].data_type(), &DataType::Utf8);
        assert_eq!(fields[2].name(), "open");
        assert_eq!(fields[2].data_type(), &DataType::Float64);
        assert_eq!(fields[3].name(), "high");
        assert_eq!(fields[4].name(), "low");
        assert_eq!(fields[5].name(), "close");
        assert_eq!(fields[6].name(), "volume");
        assert_eq!(fields[6].data_type(), &DataType::Float64);
        assert_eq!(fields[7].name(), "ts_event");
        assert_eq!(
            fields[7].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[8].name(), "ts_init");
    }

    #[rstest]
    fn test_encode_bars_values() {
        let bars = vec![
            make_bar(
                "AAPL.XNAS-1-MINUTE-LAST-INTERNAL",
                "100.10",
                "102.00",
                "100.00",
                "101.00",
                1_000,
            ),
            make_bar(
                "AAPL.XNAS-1-MINUTE-LAST-INTERNAL",
                "100.20",
                "102.00",
                "100.00",
                "101.00",
                2_000,
            ),
        ];
        let batch = encode_bars(&bars).unwrap();

        assert_eq!(batch.num_rows(), 2);

        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let bar_type_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let open_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let high_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let low_col = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let close_col = batch
            .column(5)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let volume_col = batch
            .column(6)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ts_event_col = batch
            .column(7)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init_col = batch
            .column(8)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(bar_type_col.value(0), "AAPL.XNAS-1-MINUTE-LAST-INTERNAL");
        assert!((open_col.value(0) - 100.10).abs() < 1e-9);
        assert!((open_col.value(1) - 100.20).abs() < 1e-9);
        assert!((high_col.value(0) - 102.00).abs() < 1e-9);
        assert!((low_col.value(0) - 100.00).abs() < 1e-9);
        assert!((close_col.value(0) - 101.00).abs() < 1e-9);
        assert!((volume_col.value(0) - 1_100.0).abs() < 1e-9);
        assert_eq!(ts_event_col.value(0), 1_000);
        assert_eq!(ts_init_col.value(1), 2_001);
    }

    #[rstest]
    fn test_encode_bars_empty() {
        let batch = encode_bars(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[rstest]
    fn test_encode_bars_mixed_instruments() {
        let bars = vec![
            make_bar(
                "AAPL.XNAS-1-MINUTE-LAST-INTERNAL",
                "100.10",
                "102.00",
                "100.00",
                "101.00",
                1,
            ),
            make_bar(
                "MSFT.XNAS-1-MINUTE-LAST-INTERNAL",
                "250.00",
                "251.00",
                "249.00",
                "250.50",
                2,
            ),
        ];
        let batch = encode_bars(&bars).unwrap();
        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
