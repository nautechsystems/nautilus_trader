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

//! Display-mode Arrow encoder for [`InstrumentClose`].

use std::sync::Arc;

use arrow::{
    array::{Float64Builder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::InstrumentClose;

use super::{float64_field, price_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field};

/// Returns the display-mode Arrow schema for [`InstrumentClose`].
#[must_use]
pub fn instrument_closes_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("close_price", false),
        utf8_field("close_type", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

/// Encodes instrument closes as a display-friendly Arrow [`RecordBatch`].
///
/// Emits a `Float64` `close_price` column, `Utf8` columns for the instrument
/// ID and close type, and `Timestamp(Nanosecond)` columns for event and init
/// times. Mixed-instrument batches are supported. Precision is lost on the
/// conversion to `f64`; use
/// [`crate::arrow::instrument_closes_to_arrow_record_batch_bytes`] for catalog
/// storage.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_instrument_closes(data: &[InstrumentClose]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id_builder = StringBuilder::new();
    let mut close_price_builder = Float64Builder::with_capacity(data.len());
    let mut close_type_builder = StringBuilder::new();
    let mut ts_event_builder = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init_builder = TimestampNanosecondBuilder::with_capacity(data.len());

    for close in data {
        instrument_id_builder.append_value(close.instrument_id.to_string());
        close_price_builder.append_value(price_to_f64(&close.close_price));
        close_type_builder.append_value(format!("{}", close.close_type));
        ts_event_builder.append_value(unix_nanos_to_i64(close.ts_event.as_u64()));
        ts_init_builder.append_value(unix_nanos_to_i64(close.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(instrument_closes_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(close_price_builder.finish()),
            Arc::new(close_type_builder.finish()),
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
    use nautilus_model::{enums::InstrumentCloseType, identifiers::InstrumentId, types::Price};
    use rstest::rstest;

    use super::*;

    fn make_close(
        instrument_id: &str,
        price: &str,
        close_type: InstrumentCloseType,
        ts: u64,
    ) -> InstrumentClose {
        InstrumentClose {
            instrument_id: InstrumentId::from(instrument_id),
            close_price: Price::from(price),
            close_type,
            ts_event: ts.into(),
            ts_init: (ts + 1).into(),
        }
    }

    #[rstest]
    fn test_encode_instrument_closes_schema() {
        let batch = encode_instrument_closes(&[]).unwrap();
        let fields = batch.schema().fields().clone();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[1].name(), "close_price");
        assert_eq!(fields[1].data_type(), &DataType::Float64);
        assert_eq!(fields[2].name(), "close_type");
        assert_eq!(fields[2].data_type(), &DataType::Utf8);
        assert_eq!(fields[3].name(), "ts_event");
        assert_eq!(
            fields[3].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[4].name(), "ts_init");
    }

    #[rstest]
    fn test_encode_instrument_closes_values() {
        let closes = vec![
            make_close(
                "AAPL.XNAS",
                "150.50",
                InstrumentCloseType::EndOfSession,
                1_000,
            ),
            make_close(
                "AAPL.XNAS",
                "151.25",
                InstrumentCloseType::ContractExpired,
                2_000,
            ),
        ];
        let batch = encode_instrument_closes(&closes).unwrap();

        assert_eq!(batch.num_rows(), 2);

        let close_price_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let close_type_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let ts_event_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert!((close_price_col.value(0) - 150.50).abs() < 1e-9);
        assert!((close_price_col.value(1) - 151.25).abs() < 1e-9);
        assert_eq!(
            close_type_col.value(0),
            format!("{}", InstrumentCloseType::EndOfSession)
        );
        assert_eq!(
            close_type_col.value(1),
            format!("{}", InstrumentCloseType::ContractExpired)
        );
        assert_eq!(ts_event_col.value(0), 1_000);
    }

    #[rstest]
    fn test_encode_instrument_closes_empty() {
        let batch = encode_instrument_closes(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[rstest]
    fn test_encode_instrument_closes_mixed_instruments() {
        let closes = vec![
            make_close("AAPL.XNAS", "150.50", InstrumentCloseType::EndOfSession, 1),
            make_close("MSFT.XNAS", "300.00", InstrumentCloseType::EndOfSession, 2),
        ];
        let batch = encode_instrument_closes(&closes).unwrap();
        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
