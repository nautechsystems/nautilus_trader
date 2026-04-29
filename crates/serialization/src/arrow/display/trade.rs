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

//! Display-mode Arrow encoder for [`TradeTick`].

use std::sync::Arc;

use arrow::{
    array::{Float64Builder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::TradeTick;

use super::{
    float64_field, price_to_f64, quantity_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`TradeTick`].
#[must_use]
pub fn trades_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("price", false),
        float64_field("size", false),
        utf8_field("aggressor_side", false),
        utf8_field("trade_id", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

/// Encodes trades as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Float64` columns for price and size, `Utf8` columns for the
/// instrument ID, aggressor side, and trade ID, and `Timestamp(Nanosecond)`
/// columns for event and init times. Mixed-instrument batches are supported.
/// Precision is lost on the conversion to `f64`; use
/// [`crate::arrow::trades_to_arrow_record_batch_bytes`] for catalog storage.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_trades(data: &[TradeTick]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id_builder = StringBuilder::new();
    let mut price_builder = Float64Builder::with_capacity(data.len());
    let mut size_builder = Float64Builder::with_capacity(data.len());
    let mut aggressor_side_builder = StringBuilder::new();
    let mut trade_id_builder = StringBuilder::new();
    let mut ts_event_builder = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init_builder = TimestampNanosecondBuilder::with_capacity(data.len());

    for trade in data {
        instrument_id_builder.append_value(trade.instrument_id.to_string());
        price_builder.append_value(price_to_f64(&trade.price));
        size_builder.append_value(quantity_to_f64(&trade.size));
        aggressor_side_builder.append_value(format!("{}", trade.aggressor_side));
        trade_id_builder.append_value(trade.trade_id.to_string());
        ts_event_builder.append_value(unix_nanos_to_i64(trade.ts_event.as_u64()));
        ts_init_builder.append_value(unix_nanos_to_i64(trade.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(trades_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(price_builder.finish()),
            Arc::new(size_builder.finish()),
            Arc::new(aggressor_side_builder.finish()),
            Arc::new(trade_id_builder.finish()),
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
        enums::AggressorSide,
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn make_trade(
        instrument_id: &str,
        price: &str,
        aggressor_side: AggressorSide,
        trade_id: &str,
        ts: u64,
    ) -> TradeTick {
        TradeTick {
            instrument_id: InstrumentId::from(instrument_id),
            price: Price::from(price),
            size: Quantity::from(1_000),
            aggressor_side,
            trade_id: TradeId::new(trade_id),
            ts_event: ts.into(),
            ts_init: (ts + 1).into(),
        }
    }

    #[rstest]
    fn test_encode_trades_schema() {
        let batch = encode_trades(&[]).unwrap();
        let fields = batch.schema().fields().clone();
        assert_eq!(fields.len(), 7);
        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[1].name(), "price");
        assert_eq!(fields[1].data_type(), &DataType::Float64);
        assert_eq!(fields[2].name(), "size");
        assert_eq!(fields[2].data_type(), &DataType::Float64);
        assert_eq!(fields[3].name(), "aggressor_side");
        assert_eq!(fields[3].data_type(), &DataType::Utf8);
        assert_eq!(fields[4].name(), "trade_id");
        assert_eq!(fields[4].data_type(), &DataType::Utf8);
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
    fn test_encode_trades_values() {
        let trades = vec![
            make_trade("AAPL.XNAS", "100.10", AggressorSide::Buyer, "T-1", 1_000),
            make_trade("AAPL.XNAS", "100.20", AggressorSide::Seller, "T-2", 2_000),
        ];
        let batch = encode_trades(&trades).unwrap();

        assert_eq!(batch.num_rows(), 2);

        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let price_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let size_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let aggressor_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let trade_id_col = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
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
        assert!((price_col.value(0) - 100.10).abs() < 1e-9);
        assert!((price_col.value(1) - 100.20).abs() < 1e-9);
        assert!((size_col.value(0) - 1_000.0).abs() < 1e-9);
        assert_eq!(aggressor_col.value(0), format!("{}", AggressorSide::Buyer));
        assert_eq!(aggressor_col.value(1), format!("{}", AggressorSide::Seller));
        assert_eq!(trade_id_col.value(0), "T-1");
        assert_eq!(trade_id_col.value(1), "T-2");
        assert_eq!(ts_event_col.value(0), 1_000);
        assert_eq!(ts_init_col.value(1), 2_001);
    }

    #[rstest]
    fn test_encode_trades_empty() {
        let batch = encode_trades(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[rstest]
    fn test_encode_trades_mixed_instruments() {
        let trades = vec![
            make_trade("AAPL.XNAS", "100.10", AggressorSide::Buyer, "A-1", 1),
            make_trade("MSFT.XNAS", "250.00", AggressorSide::Seller, "M-1", 2),
        ];
        let batch = encode_trades(&trades).unwrap();
        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
