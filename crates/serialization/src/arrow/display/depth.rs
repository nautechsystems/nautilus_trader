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

//! Nested display-mode Arrow encoding for [`OrderBookDepth`].

use std::sync::Arc;

use arrow::{
    array::{StringBuilder, TimestampNanosecondBuilder, UInt8Builder, UInt64Builder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::data::OrderBookDepth;

use super::{price_to_f64, quantity_to_f64, unix_nanos_to_i64};
use crate::arrow::{
    depth_display::{DepthSideBuilder, schema},
    timestamp_data_type,
};

/// Returns the nested depth display schema.
///
/// The compatibility name does not limit the number of levels.
#[must_use]
pub fn depth10_schema() -> Schema {
    schema()
}

/// Encodes every level into nested `bids` and `asks` lists.
///
/// Each level contains a display price and size, count, and exact integer order ID.
/// Empty sides remain empty lists. Mixed instruments and depths share one schema.
/// Prices and sizes use `Float64`, with nulls for undefined values; use raw catalog
/// output when exact decimal values are required. The compatibility name does not
/// impose a ten-level limit.
///
/// # Errors
///
/// Returns an error if Arrow cannot construct the batch or list offsets overflow.
pub fn encode_depth10(data: &[OrderBookDepth]) -> Result<RecordBatch, ArrowError> {
    let mut instruments = StringBuilder::new();
    let mut bids = DepthSideBuilder::new();
    let mut asks = DepthSideBuilder::new();
    let mut flags = UInt8Builder::with_capacity(data.len());
    let mut sequence = UInt64Builder::with_capacity(data.len());
    let mut ts_event =
        TimestampNanosecondBuilder::with_capacity(data.len()).with_data_type(timestamp_data_type());
    let mut ts_init =
        TimestampNanosecondBuilder::with_capacity(data.len()).with_data_type(timestamp_data_type());

    for depth in data {
        instruments.append_value(depth.instrument_id.to_string());
        for (side, orders, counts) in [
            (&mut bids, &depth.bids, &depth.bid_counts),
            (&mut asks, &depth.asks, &depth.ask_counts),
        ] {
            if orders.len() != counts.len() {
                return Err(ArrowError::InvalidArgumentError(
                    "Depth orders and counts must have equal lengths".to_string(),
                ));
            }

            for (order, count) in orders.iter().zip(counts) {
                side.prices.append_option(
                    (!order.price.is_undefined()).then(|| price_to_f64(&order.price)),
                );
                side.sizes.append_option(
                    (!order.size.is_undefined()).then(|| quantity_to_f64(&order.size)),
                );
                side.counts.append_value(*count);
                side.order_ids.append_value(order.order_id);
            }
            side.finish_row()?;
        }
        flags.append_value(depth.flags);
        sequence.append_value(depth.sequence);
        ts_event.append_value(unix_nanos_to_i64(depth.ts_event.as_u64()));
        ts_init.append_value(unix_nanos_to_i64(depth.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(depth10_schema()),
        vec![
            Arc::new(instruments.finish()),
            Arc::new(bids.finish()?),
            Arc::new(asks.finish()?),
            Arc::new(flags.finish()),
            Arc::new(sequence.finish()),
            Arc::new(ts_event.finish()),
            Arc::new(ts_init.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{
            Array, Float64Array, ListArray, StringArray, StructArray, TimestampNanosecondArray,
            UInt8Array, UInt32Array, UInt64Array,
        },
        datatypes::{DataType, Field, Fields, TimeUnit},
    };
    use nautilus_model::{
        data::BookOrder,
        enums::OrderSide,
        identifiers::InstrumentId,
        types::{PRICE_UNDEF, Price, QUANTITY_UNDEF, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_depth_display_schema_and_empty_batch() {
        let batch = encode_depth10(&[]).unwrap();
        let levels: Fields = vec![
            Field::new("price", DataType::Float64, true),
            Field::new("size", DataType::Float64, true),
            Field::new("count", DataType::UInt32, false),
            Field::new("order_id", DataType::UInt64, false),
        ]
        .into();
        let side = DataType::List(Arc::new(Field::new(
            "item",
            DataType::Struct(levels),
            false,
        )));
        let expected = Schema::new(vec![
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("bids", side.clone(), false),
            Field::new("asks", side, false),
            Field::new("flags", DataType::UInt8, false),
            Field::new("sequence", DataType::UInt64, false),
            Field::new(
                "ts_event",
                DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
                false,
            ),
            Field::new(
                "ts_init",
                DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
                false,
            ),
        ]);

        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.schema().as_ref(), &expected);
        assert_eq!(depth10_schema(), expected);
    }

    #[rstest]
    fn test_depth_display_preserves_mixed_depths_and_every_field() {
        let data = [
            depth("AAPL.XNAS", 0, 3, 1),
            depth("MSFT.XNAS", 5, 0, 2),
            depth("NVDA.XNAS", 25, 27, 3),
        ];
        let batch = encode_depth10(&data).unwrap();
        let ids = batch
            .column_by_name("instrument_id")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let flags = batch
            .column_by_name("flags")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt8Array>()
            .unwrap();
        let sequence = batch
            .column_by_name("sequence")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let ts_event = batch
            .column_by_name("ts_event")
            .unwrap()
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let ts_init = batch
            .column_by_name("ts_init")
            .unwrap()
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();

        assert_eq!(batch.num_rows(), 3);
        assert_eq!(batch.schema().as_ref(), &depth10_schema());

        for (row, (instrument, bid_len, ask_len, seed)) in [
            ("AAPL.XNAS", 0, 3, 1_u32),
            ("MSFT.XNAS", 5, 0, 2),
            ("NVDA.XNAS", 25, 27, 3),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(ids.value(row), instrument);
            assert_eq!(flags.value(row), u8::try_from(seed).unwrap());
            assert_eq!(sequence.value(row), u64::from(seed) + 30);
            assert_eq!(ts_event.value(row), i64::from(seed) + 40);
            assert_eq!(ts_init.value(row), i64::from(seed) + 50);

            for (name, len, offset) in [("bids", bid_len, 0_u32), ("asks", ask_len, 100)] {
                let list = batch
                    .column_by_name(name)
                    .unwrap()
                    .as_any()
                    .downcast_ref::<ListArray>()
                    .unwrap();
                let values = list.value(row);
                let levels = values.as_any().downcast_ref::<StructArray>().unwrap();
                let prices = levels
                    .column_by_name("price")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .unwrap();
                let sizes = levels
                    .column_by_name("size")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .unwrap();
                let counts = levels
                    .column_by_name("count")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<UInt32Array>()
                    .unwrap();
                let order_ids = levels
                    .column_by_name("order_id")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<UInt64Array>()
                    .unwrap();
                assert!(!list.is_null(row));
                assert_eq!(levels.len(), len);
                assert_eq!(levels.null_count(), 0);

                for i in 0..len {
                    let n = seed + offset + u32::try_from(i).unwrap();
                    assert_eq!(prices.value(i), f64::from(n) + 0.25);
                    assert_eq!(sizes.value(i), f64::from(n) + 0.5);
                    assert_eq!(counts.value(i), n + 10);
                    assert_eq!(order_ids.value(i), u64::MAX - u64::from(n));
                }
            }
        }
    }

    #[rstest]
    fn test_depth_display_undefined_fields_are_null() {
        let mut data = depth("AAPL.XNAS", 1, 1, 1);
        data.bids[0].price = Price::from_raw(PRICE_UNDEF, 0);
        data.asks[0].size = Quantity::from_raw(QUANTITY_UNDEF, 0);
        let batch = encode_depth10(&[data]).unwrap();
        for (name, field) in [("bids", "price"), ("asks", "size")] {
            let list = batch
                .column_by_name(name)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let values = list.value(0);
            let levels = values.as_any().downcast_ref::<StructArray>().unwrap();
            assert_eq!(levels.len(), 1);
            assert!(levels.column_by_name(field).unwrap().is_null(0));
        }
    }

    fn depth(instrument: &str, bids: usize, asks: usize, seed: u32) -> OrderBookDepth {
        let side = |len, offset, side| {
            let orders: Vec<_> = (0..len)
                .map(|i| {
                    let n = seed + offset + u32::try_from(i).unwrap();
                    BookOrder::new(
                        side,
                        format!("{n}.25").parse().unwrap(),
                        format!("{n}.5").parse().unwrap(),
                        u64::MAX - u64::from(n),
                    )
                })
                .collect();
            let counts: Vec<_> = (0..len)
                .map(|i| seed + offset + u32::try_from(i).unwrap() + 10)
                .collect();
            (orders, counts)
        };
        let (bids, bid_counts) = side(bids, 0, OrderSide::Buy);
        let (asks, ask_counts) = side(asks, 100, OrderSide::Sell);
        OrderBookDepth::new_checked(
            InstrumentId::from(instrument),
            bids,
            asks,
            bid_counts,
            ask_counts,
            u8::try_from(seed).unwrap(),
            u64::from(seed) + 30,
            (u64::from(seed) + 40).into(),
            (u64::from(seed) + 50).into(),
        )
        .unwrap()
    }
}
