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

//! Display-mode Arrow encoder for [`OrderBookDelta`].

use std::sync::Arc;

use arrow::{
    array::{
        Float64Builder, StringBuilder, TimestampNanosecondBuilder, UInt8Builder, UInt64Builder,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{data::OrderBookDelta, enums::BookAction};

use super::{
    float64_field, price_to_f64, quantity_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`OrderBookDelta`].
#[must_use]
pub fn deltas_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        utf8_field("action", false),
        utf8_field("side", false),
        float64_field("price", false),
        float64_field("size", false),
        utf8_field("order_id", false),
        Field::new("flags", DataType::UInt8, false),
        Field::new("sequence", DataType::UInt64, false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

/// Encodes order book deltas as a display-friendly Arrow [`RecordBatch`].
///
/// Prices and sizes render as `Float64`, action and side render as `Utf8`
/// via their `Display` implementations, and `order_id` becomes `Utf8` so
/// numerically large IDs survive display in dashboards. Mixed-instrument
/// batches are supported. Precision is lost on the conversion to `f64`;
/// use [`crate::arrow::book_deltas_to_arrow_record_batch_bytes`] for catalog
/// storage.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_deltas(data: &[OrderBookDelta]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id_builder = StringBuilder::new();
    let mut action_builder = StringBuilder::new();
    let mut side_builder = StringBuilder::new();
    let mut price_builder = Float64Builder::with_capacity(data.len());
    let mut size_builder = Float64Builder::with_capacity(data.len());
    let mut order_id_builder = StringBuilder::new();
    let mut flags_builder = UInt8Builder::with_capacity(data.len());
    let mut sequence_builder = UInt64Builder::with_capacity(data.len());
    let mut ts_event_builder = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init_builder = TimestampNanosecondBuilder::with_capacity(data.len());

    for delta in data {
        instrument_id_builder.append_value(delta.instrument_id.to_string());
        action_builder.append_value(format!("{}", delta.action));
        side_builder.append_value(format!("{}", delta.order.side));

        // A `Clear` delta carries a `NULL_ORDER` (zero price/size) and has no
        // meaningful order to render; emit `NaN` so dashboards show empty
        // cells rather than a phantom zero-priced order.
        if delta.action == BookAction::Clear {
            price_builder.append_value(f64::NAN);
            size_builder.append_value(f64::NAN);
        } else {
            price_builder.append_value(price_to_f64(&delta.order.price));
            size_builder.append_value(quantity_to_f64(&delta.order.size));
        }
        order_id_builder.append_value(delta.order.order_id.to_string());
        flags_builder.append_value(delta.flags);
        sequence_builder.append_value(delta.sequence);
        ts_event_builder.append_value(unix_nanos_to_i64(delta.ts_event.as_u64()));
        ts_init_builder.append_value(unix_nanos_to_i64(delta.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(deltas_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(action_builder.finish()),
            Arc::new(side_builder.finish()),
            Arc::new(price_builder.finish()),
            Arc::new(size_builder.finish()),
            Arc::new(order_id_builder.finish()),
            Arc::new(flags_builder.finish()),
            Arc::new(sequence_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{
            Array, Float64Array, StringArray, TimestampNanosecondArray, UInt8Array, UInt64Array,
        },
        datatypes::TimeUnit,
    };
    use nautilus_model::{
        data::order::BookOrder,
        enums::{BookAction, OrderSide},
        identifiers::InstrumentId,
        types::{Price, Quantity, price::PRICE_UNDEF, quantity::QUANTITY_UNDEF},
    };
    use rstest::rstest;

    use super::*;

    fn make_delta(
        instrument_id: &str,
        action: BookAction,
        side: OrderSide,
        price: &str,
        order_id: u64,
        sequence: u64,
        ts: u64,
    ) -> OrderBookDelta {
        OrderBookDelta {
            instrument_id: InstrumentId::from(instrument_id),
            action,
            order: BookOrder {
                side,
                price: Price::from(price),
                size: Quantity::from(100),
                order_id,
            },
            flags: 0,
            sequence,
            ts_event: ts.into(),
            ts_init: (ts + 1).into(),
        }
    }

    #[rstest]
    fn test_encode_deltas_schema() {
        let batch = encode_deltas(&[]).unwrap();
        let fields = batch.schema().fields().clone();
        assert_eq!(fields.len(), 10);
        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[1].name(), "action");
        assert_eq!(fields[1].data_type(), &DataType::Utf8);
        assert_eq!(fields[2].name(), "side");
        assert_eq!(fields[2].data_type(), &DataType::Utf8);
        assert_eq!(fields[3].name(), "price");
        assert_eq!(fields[3].data_type(), &DataType::Float64);
        assert_eq!(fields[4].name(), "size");
        assert_eq!(fields[4].data_type(), &DataType::Float64);
        assert_eq!(fields[5].name(), "order_id");
        assert_eq!(fields[5].data_type(), &DataType::Utf8);
        assert_eq!(fields[6].name(), "flags");
        assert_eq!(fields[6].data_type(), &DataType::UInt8);
        assert_eq!(fields[7].name(), "sequence");
        assert_eq!(fields[7].data_type(), &DataType::UInt64);
        assert_eq!(fields[8].name(), "ts_event");
        assert_eq!(
            fields[8].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[9].name(), "ts_init");
    }

    #[rstest]
    fn test_encode_deltas_values() {
        let deltas = vec![
            make_delta(
                "AAPL.XNAS",
                BookAction::Add,
                OrderSide::Buy,
                "100.10",
                1,
                10,
                1_000,
            ),
            make_delta(
                "AAPL.XNAS",
                BookAction::Update,
                OrderSide::Sell,
                "100.20",
                2,
                11,
                2_000,
            ),
        ];
        let batch = encode_deltas(&deltas).unwrap();

        assert_eq!(batch.num_rows(), 2);

        let action_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let side_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let price_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let size_col = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let order_id_col = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let flags_col = batch
            .column(6)
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        let sequence_col = batch
            .column(7)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_event_col = batch
            .column(8)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(action_col.value(0), format!("{}", BookAction::Add));
        assert_eq!(action_col.value(1), format!("{}", BookAction::Update));
        assert_eq!(side_col.value(0), format!("{}", OrderSide::Buy));
        assert_eq!(side_col.value(1), format!("{}", OrderSide::Sell));
        assert!((price_col.value(0) - 100.10).abs() < 1e-9);
        assert!((price_col.value(1) - 100.20).abs() < 1e-9);
        assert!((size_col.value(0) - 100.0).abs() < 1e-9);
        assert_eq!(order_id_col.value(0), "1");
        assert_eq!(order_id_col.value(1), "2");
        assert_eq!(flags_col.value(0), 0);
        assert_eq!(sequence_col.value(0), 10);
        assert_eq!(sequence_col.value(1), 11);
        assert_eq!(ts_event_col.value(0), 1_000);
    }

    #[rstest]
    fn test_encode_deltas_empty() {
        let batch = encode_deltas(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[rstest]
    fn test_encode_deltas_live_clear_renders_as_nan() {
        let clear = OrderBookDelta::clear(InstrumentId::from("AAPL.XNAS"), 1, 1.into(), 2.into());

        let batch = encode_deltas(&[clear]).unwrap();
        let price_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let size_col = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();

        assert!(
            price_col.value(0).is_nan(),
            "live clear price should be NaN"
        );
        assert!(size_col.value(0).is_nan(), "live clear size should be NaN");
    }

    #[rstest]
    fn test_encode_deltas_clear_sentinels_render_as_nan() {
        let clear = OrderBookDelta {
            instrument_id: InstrumentId::from("AAPL.XNAS"),
            action: BookAction::Clear,
            order: BookOrder {
                side: OrderSide::NoOrderSide,
                price: Price::from_raw(PRICE_UNDEF, 0),
                size: Quantity::from_raw(QUANTITY_UNDEF, 0),
                order_id: 0,
            },
            flags: 0,
            sequence: 1,
            ts_event: 1.into(),
            ts_init: 2.into(),
        };

        let batch = encode_deltas(&[clear]).unwrap();
        let price_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let size_col = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();

        assert!(price_col.value(0).is_nan(), "clear price should be NaN");
        assert!(size_col.value(0).is_nan(), "clear size should be NaN");
    }

    #[rstest]
    fn test_encode_deltas_mixed_instruments() {
        let deltas = vec![
            make_delta(
                "AAPL.XNAS",
                BookAction::Add,
                OrderSide::Buy,
                "100.10",
                1,
                1,
                1,
            ),
            make_delta(
                "MSFT.XNAS",
                BookAction::Add,
                OrderSide::Sell,
                "250.00",
                2,
                1,
                2,
            ),
        ];
        let batch = encode_deltas(&deltas).unwrap();
        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(instrument_id_col.value(0), "AAPL.XNAS");
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
