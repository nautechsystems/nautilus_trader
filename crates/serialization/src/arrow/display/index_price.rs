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

//! Display-mode Arrow encoder for [`IndexPriceUpdate`].

use std::sync::Arc;

use arrow::{
    array::{Float64Builder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::IndexPriceUpdate;

use super::{float64_field, price_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field};

/// Returns the display-mode Arrow schema for [`IndexPriceUpdate`].
#[must_use]
pub fn index_prices_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("value", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

/// Encodes index price updates as a display-friendly Arrow [`RecordBatch`].
///
/// Emits a `Float64` `value` column, a `Utf8` `instrument_id` column, and
/// `Timestamp(Nanosecond)` columns for event and init times. Mixed-instrument
/// batches are supported. Precision is lost on the conversion to `f64`; use
/// [`crate::arrow::index_prices_to_arrow_record_batch_bytes`] for catalog storage.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_index_prices(data: &[IndexPriceUpdate]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id_builder = StringBuilder::new();
    let mut value_builder = Float64Builder::with_capacity(data.len());
    let mut ts_event_builder = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init_builder = TimestampNanosecondBuilder::with_capacity(data.len());

    for update in data {
        instrument_id_builder.append_value(update.instrument_id.to_string());
        value_builder.append_value(price_to_f64(&update.value));
        ts_event_builder.append_value(unix_nanos_to_i64(update.ts_event.as_u64()));
        ts_init_builder.append_value(unix_nanos_to_i64(update.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(index_prices_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(value_builder.finish()),
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
    use nautilus_model::{identifiers::InstrumentId, types::Price};
    use rstest::rstest;

    use super::*;

    fn make_update(instrument_id: &str, value: &str, ts: u64) -> IndexPriceUpdate {
        IndexPriceUpdate {
            instrument_id: InstrumentId::from(instrument_id),
            value: Price::from(value),
            ts_event: ts.into(),
            ts_init: (ts + 1).into(),
        }
    }

    #[rstest]
    fn test_encode_index_prices_schema() {
        let batch = encode_index_prices(&[]).unwrap();
        let fields = batch.schema().fields().clone();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[1].name(), "value");
        assert_eq!(fields[1].data_type(), &DataType::Float64);
        assert_eq!(fields[2].name(), "ts_event");
        assert_eq!(
            fields[2].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[3].name(), "ts_init");
    }

    #[rstest]
    fn test_encode_index_prices_values() {
        let updates = vec![
            make_update("BTC-USDT.BINANCE", "50200.00", 1_000),
            make_update("BTC-USDT.BINANCE", "50300.00", 2_000),
        ];
        let batch = encode_index_prices(&updates).unwrap();

        assert_eq!(batch.num_rows(), 2);

        let value_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ts_event_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert!((value_col.value(0) - 50_200.00).abs() < 1e-9);
        assert!((value_col.value(1) - 50_300.00).abs() < 1e-9);
        assert_eq!(ts_event_col.value(0), 1_000);
    }

    #[rstest]
    fn test_encode_index_prices_empty() {
        let batch = encode_index_prices(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[rstest]
    fn test_encode_index_prices_mixed_instruments() {
        let updates = vec![
            make_update("BTC-USDT.BINANCE", "50200.00", 1),
            make_update("ETH-USDT.BINANCE", "2500.00", 2),
        ];
        let batch = encode_index_prices(&updates).unwrap();
        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "BTC-USDT.BINANCE");
        assert_eq!(instrument_id_col.value(1), "ETH-USDT.BINANCE");
    }
}
