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

//! Display-mode Arrow encoder for [`OrderBookDepth10`].

use std::sync::Arc;

use arrow::{
    array::{
        ArrayRef, Float64Builder, StringBuilder, TimestampNanosecondBuilder, UInt8Builder,
        UInt32Builder, UInt64Builder,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::depth::{DEPTH10_LEN, OrderBookDepth10};

use super::{
    float64_field, price_to_f64, quantity_to_f64, timestamp_field, unix_nanos_to_i64, utf8_field,
};

/// Returns the display-mode Arrow schema for [`OrderBookDepth10`].
///
/// Column order: `instrument_id`, then all `bid_price_{0..N}`,
/// `ask_price_{0..N}`, `bid_size_{0..N}`, `ask_size_{0..N}`,
/// `bid_count_{0..N}`, `ask_count_{0..N}`, then `flags`, `sequence`,
/// `ts_event`, `ts_init`.
#[must_use]
pub fn depth10_schema() -> Schema {
    let mut fields = Vec::with_capacity(1 + 6 * DEPTH10_LEN + 4);
    fields.push(utf8_field("instrument_id", false));

    for i in 0..DEPTH10_LEN {
        fields.push(float64_field(&format!("bid_price_{i}"), false));
    }

    for i in 0..DEPTH10_LEN {
        fields.push(float64_field(&format!("ask_price_{i}"), false));
    }

    for i in 0..DEPTH10_LEN {
        fields.push(float64_field(&format!("bid_size_{i}"), false));
    }

    for i in 0..DEPTH10_LEN {
        fields.push(float64_field(&format!("ask_size_{i}"), false));
    }

    for i in 0..DEPTH10_LEN {
        fields.push(Field::new(
            format!("bid_count_{i}"),
            DataType::UInt32,
            false,
        ));
    }

    for i in 0..DEPTH10_LEN {
        fields.push(Field::new(
            format!("ask_count_{i}"),
            DataType::UInt32,
            false,
        ));
    }

    fields.push(Field::new("flags", DataType::UInt8, false));
    fields.push(Field::new("sequence", DataType::UInt64, false));
    fields.push(timestamp_field("ts_event", false));
    fields.push(timestamp_field("ts_init", false));

    Schema::new(fields)
}

/// Encodes depth-10 snapshots as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Float64` columns per level for prices and sizes, `UInt32` columns
/// per level for counts, a `Utf8` `instrument_id` column, and
/// `Timestamp(Nanosecond)` columns for event and init times. Mixed-instrument
/// batches are supported. Precision is lost on the conversion to `f64`; use
/// [`crate::arrow::book_depth10_to_arrow_record_batch_bytes`] for catalog
/// storage.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_depth10(data: &[OrderBookDepth10]) -> Result<RecordBatch, ArrowError> {
    let mut instrument_id_builder = StringBuilder::new();
    let mut bid_price_builders: Vec<Float64Builder> = (0..DEPTH10_LEN)
        .map(|_| Float64Builder::with_capacity(data.len()))
        .collect();
    let mut ask_price_builders: Vec<Float64Builder> = (0..DEPTH10_LEN)
        .map(|_| Float64Builder::with_capacity(data.len()))
        .collect();
    let mut bid_size_builders: Vec<Float64Builder> = (0..DEPTH10_LEN)
        .map(|_| Float64Builder::with_capacity(data.len()))
        .collect();
    let mut ask_size_builders: Vec<Float64Builder> = (0..DEPTH10_LEN)
        .map(|_| Float64Builder::with_capacity(data.len()))
        .collect();
    let mut bid_count_builders: Vec<UInt32Builder> = (0..DEPTH10_LEN)
        .map(|_| UInt32Builder::with_capacity(data.len()))
        .collect();
    let mut ask_count_builders: Vec<UInt32Builder> = (0..DEPTH10_LEN)
        .map(|_| UInt32Builder::with_capacity(data.len()))
        .collect();
    let mut flags_builder = UInt8Builder::with_capacity(data.len());
    let mut sequence_builder = UInt64Builder::with_capacity(data.len());
    let mut ts_event_builder = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init_builder = TimestampNanosecondBuilder::with_capacity(data.len());

    for depth in data {
        instrument_id_builder.append_value(depth.instrument_id.to_string());
        for i in 0..DEPTH10_LEN {
            bid_price_builders[i].append_value(price_to_f64(&depth.bids[i].price));
            ask_price_builders[i].append_value(price_to_f64(&depth.asks[i].price));
            bid_size_builders[i].append_value(quantity_to_f64(&depth.bids[i].size));
            ask_size_builders[i].append_value(quantity_to_f64(&depth.asks[i].size));
            bid_count_builders[i].append_value(depth.bid_counts[i]);
            ask_count_builders[i].append_value(depth.ask_counts[i]);
        }
        flags_builder.append_value(depth.flags);
        sequence_builder.append_value(depth.sequence);
        ts_event_builder.append_value(unix_nanos_to_i64(depth.ts_event.as_u64()));
        ts_init_builder.append_value(unix_nanos_to_i64(depth.ts_init.as_u64()));
    }

    let mut columns: Vec<ArrayRef> = Vec::with_capacity(1 + 6 * DEPTH10_LEN + 4);
    columns.push(Arc::new(instrument_id_builder.finish()));

    for mut b in bid_price_builders {
        columns.push(Arc::new(b.finish()));
    }

    for mut b in ask_price_builders {
        columns.push(Arc::new(b.finish()));
    }

    for mut b in bid_size_builders {
        columns.push(Arc::new(b.finish()));
    }

    for mut b in ask_size_builders {
        columns.push(Arc::new(b.finish()));
    }

    for mut b in bid_count_builders {
        columns.push(Arc::new(b.finish()));
    }

    for mut b in ask_count_builders {
        columns.push(Arc::new(b.finish()));
    }

    columns.push(Arc::new(flags_builder.finish()));
    columns.push(Arc::new(sequence_builder.finish()));
    columns.push(Arc::new(ts_event_builder.finish()));
    columns.push(Arc::new(ts_init_builder.finish()));

    RecordBatch::try_new(Arc::new(depth10_schema()), columns)
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{
            Array, Float64Array, StringArray, TimestampNanosecondArray, UInt8Array, UInt32Array,
            UInt64Array,
        },
        datatypes::TimeUnit,
    };
    use nautilus_model::{
        data::{order::BookOrder, stubs::stub_depth10},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_encode_depth10_schema() {
        let batch = encode_depth10(&[]).unwrap();
        let fields = batch.schema().fields().clone();

        let expected_len = 1 + 6 * DEPTH10_LEN + 4;
        assert_eq!(fields.len(), expected_len);

        assert_eq!(fields[0].name(), "instrument_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);

        for i in 0..DEPTH10_LEN {
            assert_eq!(fields[1 + i].name(), &format!("bid_price_{i}"));
            assert_eq!(fields[1 + i].data_type(), &DataType::Float64);
        }

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                fields[1 + DEPTH10_LEN + i].name(),
                &format!("ask_price_{i}")
            );
        }

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                fields[1 + 2 * DEPTH10_LEN + i].name(),
                &format!("bid_size_{i}")
            );
        }

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                fields[1 + 3 * DEPTH10_LEN + i].name(),
                &format!("ask_size_{i}")
            );
        }

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                fields[1 + 4 * DEPTH10_LEN + i].name(),
                &format!("bid_count_{i}")
            );
            assert_eq!(
                fields[1 + 4 * DEPTH10_LEN + i].data_type(),
                &DataType::UInt32
            );
        }

        for i in 0..DEPTH10_LEN {
            assert_eq!(
                fields[1 + 5 * DEPTH10_LEN + i].name(),
                &format!("ask_count_{i}")
            );
        }

        let trailer_start = 1 + 6 * DEPTH10_LEN;
        assert_eq!(fields[trailer_start].name(), "flags");
        assert_eq!(fields[trailer_start].data_type(), &DataType::UInt8);
        assert_eq!(fields[trailer_start + 1].name(), "sequence");
        assert_eq!(fields[trailer_start + 1].data_type(), &DataType::UInt64);
        assert_eq!(fields[trailer_start + 2].name(), "ts_event");
        assert_eq!(
            fields[trailer_start + 2].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
        assert_eq!(fields[trailer_start + 3].name(), "ts_init");
    }

    #[rstest]
    fn test_encode_depth10_values(stub_depth10: OrderBookDepth10) {
        let data = vec![stub_depth10];
        let batch = encode_depth10(&data).unwrap();

        assert_eq!(batch.num_rows(), 1);

        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(
            instrument_id_col.value(0),
            stub_depth10.instrument_id.to_string()
        );

        let bid_price_0 = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((bid_price_0.value(0) - stub_depth10.bids[0].price.as_f64()).abs() < 1e-9);

        let ask_price_0 = batch
            .column(1 + DEPTH10_LEN)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((ask_price_0.value(0) - stub_depth10.asks[0].price.as_f64()).abs() < 1e-9);

        let bid_size_0 = batch
            .column(1 + 2 * DEPTH10_LEN)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!((bid_size_0.value(0) - stub_depth10.bids[0].size.as_f64()).abs() < 1e-9);

        let bid_count_0 = batch
            .column(1 + 4 * DEPTH10_LEN)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();
        assert_eq!(bid_count_0.value(0), stub_depth10.bid_counts[0]);

        let trailer_start = 1 + 6 * DEPTH10_LEN;
        let flags_col = batch
            .column(trailer_start)
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        let sequence_col = batch
            .column(trailer_start + 1)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_event_col = batch
            .column(trailer_start + 2)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(flags_col.value(0), stub_depth10.flags);
        assert_eq!(sequence_col.value(0), stub_depth10.sequence);
        assert_eq!(ts_event_col.value(0), stub_depth10.ts_event.as_u64() as i64);
    }

    #[rstest]
    fn test_encode_depth10_multi_row_values(stub_depth10: OrderBookDepth10) {
        // Guards against row-indexing bugs in the wide depth10 schema by
        // placing distinct values at the same level across two rows and
        // asserting each row-column independently.
        let row0 = stub_depth10;
        let mut row1 = stub_depth10;
        row1.bids[0] = BookOrder::new(
            row1.bids[0].side,
            Price::from("200.00"),
            Quantity::from(250),
            row1.bids[0].order_id,
        );
        row1.asks[0] = BookOrder::new(
            row1.asks[0].side,
            Price::from("201.00"),
            Quantity::from(350),
            row1.asks[0].order_id,
        );
        row1.bid_counts[0] = 42;
        row1.ask_counts[0] = 43;

        let batch = encode_depth10(&[row0, row1]).unwrap();
        assert_eq!(batch.num_rows(), 2);

        let bid_price_0 = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ask_price_0 = batch
            .column(1 + DEPTH10_LEN)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let bid_size_0 = batch
            .column(1 + 2 * DEPTH10_LEN)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ask_size_0 = batch
            .column(1 + 3 * DEPTH10_LEN)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let bid_count_0 = batch
            .column(1 + 4 * DEPTH10_LEN)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();
        let ask_count_0 = batch
            .column(1 + 5 * DEPTH10_LEN)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();

        assert!((bid_price_0.value(0) - row0.bids[0].price.as_f64()).abs() < 1e-9);
        assert!((bid_price_0.value(1) - 200.00).abs() < 1e-9);
        assert!((ask_price_0.value(0) - row0.asks[0].price.as_f64()).abs() < 1e-9);
        assert!((ask_price_0.value(1) - 201.00).abs() < 1e-9);
        assert!((bid_size_0.value(0) - row0.bids[0].size.as_f64()).abs() < 1e-9);
        assert!((bid_size_0.value(1) - 250.0).abs() < 1e-9);
        assert!((ask_size_0.value(0) - row0.asks[0].size.as_f64()).abs() < 1e-9);
        assert!((ask_size_0.value(1) - 350.0).abs() < 1e-9);
        assert_eq!(bid_count_0.value(0), row0.bid_counts[0]);
        assert_eq!(bid_count_0.value(1), 42);
        assert_eq!(ask_count_0.value(0), row0.ask_counts[0]);
        assert_eq!(ask_count_0.value(1), 43);
    }

    #[rstest]
    fn test_encode_depth10_empty() {
        let batch = encode_depth10(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
    }

    #[rstest]
    fn test_encode_depth10_mixed_instruments(stub_depth10: OrderBookDepth10) {
        let mut other = stub_depth10;
        other.instrument_id = InstrumentId::from("MSFT.XNAS");

        let data = vec![stub_depth10, other];
        let batch = encode_depth10(&data).unwrap();
        assert_eq!(batch.num_rows(), 2);

        let instrument_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(
            instrument_id_col.value(0),
            stub_depth10.instrument_id.to_string()
        );
        assert_eq!(instrument_id_col.value(1), "MSFT.XNAS");
    }
}
