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

//! An [`OrderBookDepth`] aggregated order book snapshot with runtime depth.

use std::{collections::HashMap, fmt::Display};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::{ARROW_TIMESTAMP_NANOSECOND, HasTsInit, order::BookOrder};
use crate::{
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{PRICE_ERROR, PRICE_UNDEF},
};

/// Number of levels in a standard depth-10 snapshot.
pub const DEPTH10_LEN: usize = 10;
/// Number of levels stored inline before a depth side spills to the heap.
pub const DEPTH_INLINE_LEN: usize = DEPTH10_LEN;

const DEPTH_SIDE_LIST: &str = "List(Struct(price: Decimal128(38, 16), size: Decimal128(38, 16), count: UInt32, order_id: UInt64))";

/// Represents one aggregated order book snapshot with any number of levels per side.
///
/// The plural name denotes the many levels in one snapshot. In contrast, [`super::OrderBookDeltas`]
/// is a container of multiple update events. Up to ten levels per side remain inline; deeper venue
/// snapshots spill transparently without changing the data type.
///
/// Per-level [`BookOrder::order_id`] values are retained when supplied by the venue.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "OrderBookDepthRaw")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct OrderBookDepth {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The bid orders for the depth update.
    pub bids: SmallVec<[BookOrder; DEPTH_INLINE_LEN]>,
    /// The ask orders for the depth update.
    pub asks: SmallVec<[BookOrder; DEPTH_INLINE_LEN]>,
    /// The count of bid orders per level for the depth update.
    pub bid_counts: SmallVec<[u32; DEPTH_INLINE_LEN]>,
    /// The count of ask orders per level for the depth update.
    pub ask_counts: SmallVec<[u32; DEPTH_INLINE_LEN]>,
    /// The record flags bit field, indicating event end and data information.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// UNIX timestamp (nanoseconds) when the book event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

#[derive(Deserialize)]
struct OrderBookDepthRaw {
    instrument_id: InstrumentId,
    bids: SmallVec<[BookOrder; DEPTH_INLINE_LEN]>,
    asks: SmallVec<[BookOrder; DEPTH_INLINE_LEN]>,
    bid_counts: SmallVec<[u32; DEPTH_INLINE_LEN]>,
    ask_counts: SmallVec<[u32; DEPTH_INLINE_LEN]>,
    flags: u8,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

impl TryFrom<OrderBookDepthRaw> for OrderBookDepth {
    type Error = anyhow::Error;

    fn try_from(value: OrderBookDepthRaw) -> Result<Self, Self::Error> {
        Self::new_checked(
            value.instrument_id,
            value.bids,
            value.asks,
            value.bid_counts,
            value.ask_counts,
            value.flags,
            value.sequence,
            value.ts_event,
            value.ts_init,
        )
    }
}

impl OrderBookDepth {
    /// Creates a new [`OrderBookDepth`] instance.
    ///
    /// # Panics
    ///
    /// Panics if either order side and its count vector have different lengths.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new<B, A, BC, AC>(
        instrument_id: InstrumentId,
        bids: B,
        asks: A,
        bid_counts: BC,
        ask_counts: AC,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self
    where
        B: IntoIterator<Item = BookOrder>,
        A: IntoIterator<Item = BookOrder>,
        BC: IntoIterator<Item = u32>,
        AC: IntoIterator<Item = u32>,
    {
        Self::new_checked(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .unwrap_or_else(|e| panic!("{e}"))
    }

    /// Creates a new [`OrderBookDepth`] instance after validating its levels.
    ///
    /// # Errors
    ///
    /// Returns an error if an order side and its count vector have different lengths, or if a
    /// retained level has the wrong side. Levels with no side, a non-positive or undefined size,
    /// or an undefined or error price are omitted with their count entries.
    #[expect(clippy::too_many_arguments)]
    pub fn new_checked<B, A, BC, AC>(
        instrument_id: InstrumentId,
        bids: B,
        asks: A,
        bid_counts: BC,
        ask_counts: AC,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self>
    where
        B: IntoIterator<Item = BookOrder>,
        A: IntoIterator<Item = BookOrder>,
        BC: IntoIterator<Item = u32>,
        AC: IntoIterator<Item = u32>,
    {
        let bids = bids
            .into_iter()
            .collect::<SmallVec<[BookOrder; DEPTH_INLINE_LEN]>>();
        let asks = asks
            .into_iter()
            .collect::<SmallVec<[BookOrder; DEPTH_INLINE_LEN]>>();
        let bid_counts = bid_counts
            .into_iter()
            .collect::<SmallVec<[u32; DEPTH_INLINE_LEN]>>();
        let ask_counts = ask_counts
            .into_iter()
            .collect::<SmallVec<[u32; DEPTH_INLINE_LEN]>>();
        anyhow::ensure!(
            bids.len() == bid_counts.len(),
            "bid order and count lengths must match"
        );
        anyhow::ensure!(
            asks.len() == ask_counts.len(),
            "ask order and count lengths must match"
        );
        let (bids, bid_counts): (
            SmallVec<[BookOrder; DEPTH_INLINE_LEN]>,
            SmallVec<[u32; DEPTH_INLINE_LEN]>,
        ) = bids
            .into_iter()
            .zip(bid_counts)
            .filter(|(order, _)| is_depth_level_present(order))
            .unzip();
        let (asks, ask_counts): (
            SmallVec<[BookOrder; DEPTH_INLINE_LEN]>,
            SmallVec<[u32; DEPTH_INLINE_LEN]>,
        ) = asks
            .into_iter()
            .zip(ask_counts)
            .filter(|(order, _)| is_depth_level_present(order))
            .unzip();
        anyhow::ensure!(
            bids.iter().all(|order| order.side == Some(OrderSide::Buy)),
            "bid levels must have Buy side"
        );
        anyhow::ensure!(
            asks.iter().all(|order| order.side == Some(OrderSide::Sell)),
            "ask levels must have Sell side"
        );

        Ok(Self {
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata.insert("size_precision".to_string(), size_precision.to_string());
        metadata
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[must_use]
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("bids".to_string(), DEPTH_SIDE_LIST.to_string());
        metadata.insert("asks".to_string(), DEPTH_SIDE_LIST.to_string());
        metadata.insert("flags".to_string(), "UInt8".to_string());
        metadata.insert("sequence".to_string(), "UInt64".to_string());
        metadata.insert(
            "ts_event".to_string(),
            ARROW_TIMESTAMP_NANOSECOND.to_string(),
        );
        metadata.insert(
            "ts_init".to_string(),
            ARROW_TIMESTAMP_NANOSECOND.to_string(),
        );
        metadata
    }
}

/// Returns whether an order represents a populated depth level.
#[must_use]
pub fn is_depth_level_present(order: &BookOrder) -> bool {
    order.side.is_some()
        && order.price.raw != PRICE_UNDEF
        && order.price.raw != PRICE_ERROR
        && order.size.is_positive()
}

// TODO: Exact format for Debug and Display TBD
impl Display for OrderBookDepth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},flags={},sequence={},ts_event={},ts_init={}",
            self.instrument_id, self.flags, self.sequence, self.ts_event, self.ts_init
        )
    }
}

impl Serializable for OrderBookDepth {}

impl HasTsInit for OrderBookDepth {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

/// Temporary source-compatible alias for the former fixed-depth type name.
pub type OrderBookDepth10 = OrderBookDepth;

#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    use rstest::rstest;
    use serde_json;

    use super::*;
    use crate::{
        data::{
            order::{BookOrder, NULL_ORDER},
            stubs::*,
        },
        enums::OrderSide,
        types::{Price, QUANTITY_UNDEF, Quantity, price::PriceRaw},
    };

    fn create_test_book_order(
        side: OrderSide,
        price: &str,
        size: &str,
        order_id: u64,
    ) -> BookOrder {
        BookOrder::new(side, Price::from(price), Quantity::from(size), order_id)
    }

    fn create_test_depth10() -> OrderBookDepth {
        let instrument_id = InstrumentId::from("EURUSD.SIM");

        // Create bid orders (descending prices)
        let bids = [
            create_test_book_order(OrderSide::Buy, "1.0500", "100000", 1),
            create_test_book_order(OrderSide::Buy, "1.0499", "150000", 2),
            create_test_book_order(OrderSide::Buy, "1.0498", "200000", 3),
            create_test_book_order(OrderSide::Buy, "1.0497", "125000", 4),
            create_test_book_order(OrderSide::Buy, "1.0496", "175000", 5),
            create_test_book_order(OrderSide::Buy, "1.0495", "100000", 6),
            create_test_book_order(OrderSide::Buy, "1.0494", "225000", 7),
            create_test_book_order(OrderSide::Buy, "1.0493", "150000", 8),
            create_test_book_order(OrderSide::Buy, "1.0492", "300000", 9),
            create_test_book_order(OrderSide::Buy, "1.0491", "175000", 10),
        ];

        // Create ask orders (ascending prices)
        let asks = [
            create_test_book_order(OrderSide::Sell, "1.0501", "100000", 11),
            create_test_book_order(OrderSide::Sell, "1.0502", "125000", 12),
            create_test_book_order(OrderSide::Sell, "1.0503", "150000", 13),
            create_test_book_order(OrderSide::Sell, "1.0504", "175000", 14),
            create_test_book_order(OrderSide::Sell, "1.0505", "200000", 15),
            create_test_book_order(OrderSide::Sell, "1.0506", "100000", 16),
            create_test_book_order(OrderSide::Sell, "1.0507", "250000", 17),
            create_test_book_order(OrderSide::Sell, "1.0508", "125000", 18),
            create_test_book_order(OrderSide::Sell, "1.0509", "300000", 19),
            create_test_book_order(OrderSide::Sell, "1.0510", "175000", 20),
        ];

        let bid_counts = [1, 2, 1, 3, 1, 2, 1, 4, 1, 2];
        let ask_counts = [2, 1, 3, 1, 2, 1, 4, 1, 2, 3];

        OrderBookDepth::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            32,                             // flags
            12345,                          // sequence
            UnixNanos::from(1_000_000_000), // ts_event
            UnixNanos::from(2_000_000_000), // ts_init
        )
    }

    fn create_empty_depth10() -> OrderBookDepth {
        let instrument_id = InstrumentId::from("EMPTY.TEST");

        // Create empty orders with zero prices and quantities
        let empty_bid = create_test_book_order(OrderSide::Buy, "0.0", "0", 0);
        let empty_ask = create_test_book_order(OrderSide::Sell, "0.0", "0", 0);

        OrderBookDepth::new(
            instrument_id,
            [empty_bid; DEPTH10_LEN],
            [empty_ask; DEPTH10_LEN],
            [0; DEPTH10_LEN],
            [0; DEPTH10_LEN],
            0,
            0,
            UnixNanos::from(0),
            UnixNanos::from(0),
        )
    }

    #[rstest]
    fn test_order_book_depths_new() {
        let depth = create_test_depth10();

        assert_eq!(depth.instrument_id, InstrumentId::from("EURUSD.SIM"));
        assert_eq!(depth.bids.len(), DEPTH10_LEN);
        assert_eq!(depth.asks.len(), DEPTH10_LEN);
        assert_eq!(depth.bid_counts.len(), DEPTH10_LEN);
        assert_eq!(depth.ask_counts.len(), DEPTH10_LEN);
        assert_eq!(depth.flags, 32);
        assert_eq!(depth.sequence, 12345);
        assert_eq!(depth.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(depth.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    #[case::price(true)]
    #[case::size(false)]
    fn test_depths_drop_partial_undefined_level(#[case] price_undefined: bool) {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let bid = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.23"),
            Quantity::from("100.00"),
            1,
        );
        let ask = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.24"),
            Quantity::from("100.00"),
            2,
        );
        let mut asks = [ask; DEPTH10_LEN];
        if price_undefined {
            asks[1].price = Price::from_raw(PRICE_UNDEF, 0);
        } else {
            asks[1].size = Quantity::from_raw(QUANTITY_UNDEF, 0);
        }
        let depth = OrderBookDepth::new(
            instrument_id,
            [bid; DEPTH10_LEN],
            asks,
            [1; DEPTH10_LEN],
            [1; DEPTH10_LEN],
            0,
            1,
            1.into(),
            1.into(),
        );
        let expected = OrderBookDepth::new(
            instrument_id,
            [bid; DEPTH10_LEN],
            [ask; DEPTH10_LEN - 1],
            [1; DEPTH10_LEN],
            [1; DEPTH10_LEN - 1],
            0,
            1,
            1.into(),
            1.into(),
        );

        assert_eq!(depth, expected);
    }

    #[rstest]
    fn test_order_book_depths_new_checked_rejects_mismatched_counts() {
        let result = OrderBookDepth::new_checked(
            InstrumentId::from("EURUSD.SIM"),
            Vec::<BookOrder>::new(),
            Vec::<BookOrder>::new(),
            vec![1],
            Vec::<u32>::new(),
            0,
            0,
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "bid order and count lengths must match",
        );
    }

    #[rstest]
    fn test_order_book_depths_new_checked_rejects_sell_bid() {
        let result = OrderBookDepth::new_checked(
            InstrumentId::from("EURUSD.SIM"),
            [create_test_book_order(
                OrderSide::Sell,
                "1.0500",
                "100000",
                1,
            )],
            [],
            [1],
            [],
            0,
            0,
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "bid levels must have Buy side",
        );
    }

    #[rstest]
    fn test_order_book_depths_new_checked_rejects_buy_ask() {
        let result = OrderBookDepth::new_checked(
            InstrumentId::from("EURUSD.SIM"),
            [],
            [create_test_book_order(
                OrderSide::Buy,
                "1.0501",
                "100000",
                2,
            )],
            [],
            [1],
            0,
            0,
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "ask levels must have Sell side",
        );
    }

    #[rstest]
    #[case(PRICE_UNDEF)]
    #[case(PRICE_ERROR)]
    fn test_depth_level_with_sentinel_price_is_absent(#[case] raw: PriceRaw) {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from_raw(raw, 0),
            Quantity::from("1"),
            1,
        );

        assert!(!is_depth_level_present(&order));
    }

    #[rstest]
    #[case(PRICE_UNDEF)]
    #[case(PRICE_ERROR)]
    fn test_order_book_depths_new_checked_drops_sentinel_price(#[case] raw: PriceRaw) {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from_raw(raw, 0),
            Quantity::from("1"),
            1,
        );

        let depth = OrderBookDepth::new_checked(
            InstrumentId::from("EURUSD.SIM"),
            [order],
            [],
            [7],
            [],
            0,
            0,
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
        .unwrap();

        assert!(depth.bids.is_empty());
        assert!(depth.bid_counts.is_empty());
    }

    #[rstest]
    fn test_order_book_depths_deserialize_rejects_mismatched_counts() {
        let mut value = serde_json::to_value(create_test_depth10()).unwrap();
        value["bid_counts"].as_array_mut().unwrap().pop();

        let error = serde_json::from_value::<OrderBookDepth>(value).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("bid order and count lengths must match")
        );
    }

    #[rstest]
    fn test_order_book_depths_deserialize_drops_legacy_padding_and_zero_size_levels() {
        let mut legacy = create_test_depth10();
        legacy.bids[1] = NULL_ORDER;
        legacy.bids[2].size = Quantity::zero(legacy.bids[2].size.precision);
        let payload = serde_json::to_string(&legacy).unwrap();

        let decoded = serde_json::from_str::<OrderBookDepth>(&payload).unwrap();
        let bid_order_ids = decoded
            .bids
            .iter()
            .map(|order| order.order_id)
            .collect::<Vec<_>>();

        assert_eq!(bid_order_ids, vec![1, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(decoded.bid_counts.as_slice(), &[1, 3, 1, 2, 1, 4, 1, 2]);
        assert_eq!(decoded.asks, legacy.asks);
        assert_eq!(decoded.ask_counts, legacy.ask_counts);
        assert_eq!(decoded.flags, legacy.flags);
        assert_eq!(decoded.sequence, legacy.sequence);
        assert_eq!(decoded.ts_event, legacy.ts_event);
        assert_eq!(decoded.ts_init, legacy.ts_init);
    }

    #[rstest]
    fn test_order_book_depths_msgpack_deserialize_drops_legacy_padding_and_zero_size_levels() {
        let mut legacy = create_test_depth10();
        legacy.bids[1] = NULL_ORDER;
        legacy.bids[2].size = Quantity::zero(legacy.bids[2].size.precision);
        let payload = rmp_serde::to_vec_named(&legacy).unwrap();

        let decoded = rmp_serde::from_slice::<OrderBookDepth>(&payload).unwrap();

        assert_eq!(
            decoded
                .bids
                .iter()
                .map(|order| order.order_id)
                .collect::<Vec<_>>(),
            vec![1, 4, 5, 6, 7, 8, 9, 10],
        );
        assert_eq!(decoded.bid_counts.as_slice(), &[1, 3, 1, 2, 1, 4, 1, 2]);
        assert_eq!(decoded.asks, legacy.asks);
        assert_eq!(decoded.ask_counts, legacy.ask_counts);
        assert_eq!(decoded.flags, legacy.flags);
        assert_eq!(decoded.sequence, legacy.sequence);
        assert_eq!(decoded.ts_event, legacy.ts_event);
        assert_eq!(decoded.ts_init, legacy.ts_init);
    }

    #[rstest]
    fn test_order_book_depths_new_with_all_parameters() {
        let instrument_id = InstrumentId::from("GBPUSD.SIM");
        let bid = create_test_book_order(OrderSide::Buy, "1.2500", "50000", 1);
        let ask = create_test_book_order(OrderSide::Sell, "1.2501", "75000", 2);
        let flags = 64u8;
        let sequence = 999u64;
        let ts_event = UnixNanos::from(5_000_000_000);
        let ts_init = UnixNanos::from(6_000_000_000);

        let depth = OrderBookDepth::new(
            instrument_id,
            [bid; DEPTH10_LEN],
            [ask; DEPTH10_LEN],
            [5; DEPTH10_LEN],
            [3; DEPTH10_LEN],
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert_eq!(depth.instrument_id, instrument_id);
        assert_eq!(depth.bids[0], bid);
        assert_eq!(depth.asks[0], ask);
        assert_eq!(depth.bid_counts[0], 5);
        assert_eq!(depth.ask_counts[0], 3);
        assert_eq!(depth.flags, flags);
        assert_eq!(depth.sequence, sequence);
        assert_eq!(depth.ts_event, ts_event);
        assert_eq!(depth.ts_init, ts_init);
    }

    #[rstest]
    fn test_order_book_depths_lengths() {
        let depth = create_test_depth10();

        // The legacy depth-10 fixture retains all ten levels.
        assert_eq!(depth.bids.len(), 10);
        assert_eq!(depth.asks.len(), 10);
        assert_eq!(depth.bid_counts.len(), 10);
        assert_eq!(depth.ask_counts.len(), 10);
    }

    #[rstest]
    fn test_order_book_depths_indexing() {
        let depth = create_test_depth10();

        // Test first and last elements of each array
        assert_eq!(depth.bids[0].price, Price::from("1.0500"));
        assert_eq!(depth.bids[9].price, Price::from("1.0491"));
        assert_eq!(depth.asks[0].price, Price::from("1.0501"));
        assert_eq!(depth.asks[9].price, Price::from("1.0510"));
        assert_eq!(depth.bid_counts[0], 1);
        assert_eq!(depth.bid_counts[9], 2);
        assert_eq!(depth.ask_counts[0], 2);
        assert_eq!(depth.ask_counts[9], 3);
    }

    #[rstest]
    fn test_order_book_depths_bid_ask_ordering() {
        let depth = create_test_depth10();

        // Verify bid prices are in descending order (highest to lowest)
        for i in 0..9 {
            assert!(
                depth.bids[i].price >= depth.bids[i + 1].price,
                "Bid prices should be in descending order: {} >= {}",
                depth.bids[i].price,
                depth.bids[i + 1].price
            );
        }

        // Verify ask prices are in ascending order (lowest to highest)
        for i in 0..9 {
            assert!(
                depth.asks[i].price <= depth.asks[i + 1].price,
                "Ask prices should be in ascending order: {} <= {}",
                depth.asks[i].price,
                depth.asks[i + 1].price
            );
        }

        // Verify bid-ask spread (best bid < best ask)
        assert!(
            depth.bids[0].price < depth.asks[0].price,
            "Best bid {} should be less than best ask {}",
            depth.bids[0].price,
            depth.asks[0].price
        );
    }

    #[rstest]
    fn test_order_book_depths_clone() {
        let depth1 = create_test_depth10();
        let depth2 = depth1.clone();

        assert_eq!(depth1.instrument_id, depth2.instrument_id);
        assert_eq!(depth1.bids, depth2.bids);
        assert_eq!(depth1.asks, depth2.asks);
        assert_eq!(depth1.bid_counts, depth2.bid_counts);
        assert_eq!(depth1.ask_counts, depth2.ask_counts);
        assert_eq!(depth1.flags, depth2.flags);
        assert_eq!(depth1.sequence, depth2.sequence);
        assert_eq!(depth1.ts_event, depth2.ts_event);
        assert_eq!(depth1.ts_init, depth2.ts_init);
    }

    #[rstest]
    fn test_order_book_depths_inline_and_spilled_storage() {
        let inline = create_test_depth10();
        let bid = inline.bids[0];
        let ask = inline.asks[0];
        let spilled = OrderBookDepth::new(
            inline.instrument_id,
            vec![bid; 25],
            vec![ask; 25],
            vec![1; 25],
            vec![1; 25],
            inline.flags,
            inline.sequence,
            inline.ts_event,
            inline.ts_init,
        );

        assert!(!inline.bids.spilled());
        assert!(!inline.asks.spilled());
        assert!(!inline.bid_counts.spilled());
        assert!(!inline.ask_counts.spilled());
        assert!(spilled.bids.spilled());
        assert!(spilled.asks.spilled());
        assert!(spilled.bid_counts.spilled());
        assert!(spilled.ask_counts.spilled());
        assert_eq!(spilled.bids.len(), 25);
        assert_eq!(spilled.asks.len(), 25);
    }

    #[rstest]
    fn test_order_book_depths_debug() {
        let depth = create_test_depth10();
        let debug_str = format!("{depth:?}");

        assert!(debug_str.contains("OrderBookDepth"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("flags: 32"));
        assert!(debug_str.contains("sequence: 12345"));
    }

    #[rstest]
    fn test_order_book_depths_partial_eq() {
        let depth1 = create_test_depth10();
        let depth2 = create_test_depth10();
        let depth3 = create_empty_depth10();

        assert_eq!(depth1, depth2); // Same data
        assert_ne!(depth1, depth3); // Different data
        assert_ne!(depth2, depth3); // Different data
    }

    #[rstest]
    fn test_order_book_depths_eq_consistency() {
        let depth1 = create_test_depth10();
        let depth2 = create_test_depth10();

        assert_eq!(depth1, depth2);
        assert_eq!(depth2, depth1); // Symmetry
        assert_eq!(depth1, depth1); // Reflexivity
    }

    #[rstest]
    fn test_order_book_depths_hash() {
        let depth1 = create_test_depth10();
        let depth2 = create_test_depth10();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        depth1.hash(&mut hasher1);
        depth2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish()); // Equal objects have equal hashes
    }

    #[rstest]
    fn test_order_book_depths_hash_different_objects() {
        let depth1 = create_test_depth10();
        let depth2 = create_empty_depth10();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        depth1.hash(&mut hasher1);
        depth2.hash(&mut hasher2);

        assert_ne!(hasher1.finish(), hasher2.finish()); // Different objects should have different hashes
    }

    #[rstest]
    fn test_order_book_depths_display() {
        let depth = create_test_depth10();
        let display_str = format!("{depth}");

        assert!(display_str.contains("EURUSD.SIM"));
        assert!(display_str.contains("flags=32"));
        assert!(display_str.contains("sequence=12345"));
        assert!(display_str.contains("ts_event=1000000000"));
        assert!(display_str.contains("ts_init=2000000000"));
    }

    #[rstest]
    fn test_order_book_depths_display_format() {
        let depth = create_test_depth10();
        let expected = "EURUSD.SIM,flags=32,sequence=12345,ts_event=1000000000,ts_init=2000000000";

        assert_eq!(format!("{depth}"), expected);
    }

    #[rstest]
    fn test_order_book_depths_serialization() {
        let depth = create_test_depth10();

        // Test JSON serialization
        let json = serde_json::to_string(&depth).unwrap();
        let deserialized: OrderBookDepth = serde_json::from_str(&json).unwrap();

        assert_eq!(depth, deserialized);
    }

    #[rstest]
    fn test_order_book_depths_serializable_trait() {
        fn assert_serializable<T: Serializable>(_: &T) {}

        let depth = create_test_depth10();

        // Verify Serializable trait is implemented (compile-time check)
        assert_serializable(&depth);
    }

    #[rstest]
    fn test_order_book_depths_has_ts_init() {
        let depth = create_test_depth10();

        assert_eq!(depth.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_book_depths_get_metadata() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let price_precision = 5u8;
        let size_precision = 0u8;

        let metadata =
            OrderBookDepth::get_metadata(&instrument_id, price_precision, size_precision);

        assert_eq!(
            metadata.get("instrument_id"),
            Some(&"EURUSD.SIM".to_string())
        );
        assert_eq!(metadata.get("price_precision"), Some(&"5".to_string()));
        assert_eq!(metadata.get("size_precision"), Some(&"0".to_string()));
        assert_eq!(metadata.len(), 3);
    }

    #[rstest]
    fn test_order_book_depths_get_fields() {
        let fields = OrderBookDepth::get_fields();

        assert_eq!(fields.get("bids"), Some(&DEPTH_SIDE_LIST.to_string()));
        assert_eq!(fields.get("asks"), Some(&DEPTH_SIDE_LIST.to_string()));
        assert_eq!(fields.get("flags"), Some(&"UInt8".to_string()));
        assert_eq!(fields.get("sequence"), Some(&"UInt64".to_string()));
        assert_eq!(
            fields.get("ts_event"),
            Some(&ARROW_TIMESTAMP_NANOSECOND.to_string())
        );
        assert_eq!(
            fields.get("ts_init"),
            Some(&ARROW_TIMESTAMP_NANOSECOND.to_string())
        );
        assert_eq!(fields.get("identifier"), None);
        assert_eq!(fields.len(), 6);
    }

    #[rstest]
    fn test_order_book_depths_get_fields_order() {
        let fields = OrderBookDepth::get_fields();
        let keys: Vec<&String> = fields.keys().collect();

        assert_eq!(
            keys,
            vec!["bids", "asks", "flags", "sequence", "ts_event", "ts_init",]
        );
    }

    #[rstest]
    fn test_order_book_depths_empty_values() {
        let depth = create_empty_depth10();

        assert_eq!(depth.instrument_id, InstrumentId::from("EMPTY.TEST"));
        assert_eq!(depth.flags, 0);
        assert_eq!(depth.sequence, 0);
        assert_eq!(depth.ts_event, UnixNanos::from(0));
        assert_eq!(depth.ts_init, UnixNanos::from(0));

        assert!(depth.bids.is_empty());
        assert!(depth.asks.is_empty());
        assert!(depth.bid_counts.is_empty());
        assert!(depth.ask_counts.is_empty());
    }

    #[rstest]
    fn test_order_book_depths_max_values() {
        let instrument_id = InstrumentId::from("MAX.TEST");
        let max_bid = create_test_book_order(OrderSide::Buy, "999999.99", "999999999", u64::MAX);
        let max_ask = create_test_book_order(OrderSide::Sell, "1000000.00", "999999999", u64::MAX);

        let depth = OrderBookDepth::new(
            instrument_id,
            [max_bid; DEPTH10_LEN],
            [max_ask; DEPTH10_LEN],
            [u32::MAX; DEPTH10_LEN],
            [u32::MAX; DEPTH10_LEN],
            u8::MAX,
            u64::MAX,
            UnixNanos::from(u64::MAX),
            UnixNanos::from(u64::MAX),
        );

        assert_eq!(depth.flags, u8::MAX);
        assert_eq!(depth.sequence, u64::MAX);
        assert_eq!(depth.ts_event, UnixNanos::from(u64::MAX));
        assert_eq!(depth.ts_init, UnixNanos::from(u64::MAX));

        for &count in &depth.bid_counts {
            assert_eq!(count, u32::MAX);
        }

        for &count in &depth.ask_counts {
            assert_eq!(count, u32::MAX);
        }
    }

    #[rstest]
    fn test_order_book_depths_different_instruments() {
        let instruments = [
            "EURUSD.SIM",
            "GBPUSD.SIM",
            "USDJPY.SIM",
            "AUDUSD.SIM",
            "USDCHF.SIM",
        ];

        for instrument_str in &instruments {
            let instrument_id = InstrumentId::from(*instrument_str);
            let bid = create_test_book_order(OrderSide::Buy, "1.0000", "100000", 1);
            let ask = create_test_book_order(OrderSide::Sell, "1.0001", "100000", 2);

            let depth = OrderBookDepth::new(
                instrument_id,
                [bid; DEPTH10_LEN],
                [ask; DEPTH10_LEN],
                [1; DEPTH10_LEN],
                [1; DEPTH10_LEN],
                0,
                1,
                UnixNanos::from(1_000_000_000),
                UnixNanos::from(2_000_000_000),
            );

            assert_eq!(depth.instrument_id, instrument_id);
            assert!(format!("{depth}").contains(instrument_str));
        }
    }

    #[rstest]
    fn test_order_book_depths_realistic_forex_spread() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");

        // Realistic EUR/USD spread with 0.1 pip spread
        let best_bid = create_test_book_order(OrderSide::Buy, "1.08500", "1000000", 1);
        let best_ask = create_test_book_order(OrderSide::Sell, "1.08501", "1000000", 2);

        let depth = OrderBookDepth::new(
            instrument_id,
            [best_bid; DEPTH10_LEN],
            [best_ask; DEPTH10_LEN],
            [5; DEPTH10_LEN], // Realistic order count
            [3; DEPTH10_LEN],
            16,                                         // Realistic flags
            123_456,                                    // Realistic sequence
            UnixNanos::from(1_672_531_200_000_000_000), // Jan 1, 2023 timestamp
            UnixNanos::from(1_672_531_200_000_100_000),
        );

        assert_eq!(depth.bids[0].price, Price::from("1.08500"));
        assert_eq!(depth.asks[0].price, Price::from("1.08501"));
        assert!(depth.bids[0].price < depth.asks[0].price); // Positive spread

        // Verify realistic quantities and counts
        assert_eq!(depth.bids[0].size, Quantity::from("1000000"));
        assert_eq!(depth.bid_counts[0], 5);
        assert_eq!(depth.ask_counts[0], 3);
    }

    #[rstest]
    fn test_order_book_depth10_with_stub(stub_depth10: OrderBookDepth) {
        let depth = stub_depth10;

        assert_eq!(depth.instrument_id, InstrumentId::from("AAPL.XNAS"));
        assert_eq!(depth.bids.len(), 10);
        assert_eq!(depth.asks.len(), 10);
        assert_eq!(depth.asks[9].price, Price::from("109.0"));
        assert_eq!(depth.asks[0].price, Price::from("100.0"));
        assert_eq!(depth.bids[0].price, Price::from("99.0"));
        assert_eq!(depth.bids[9].price, Price::from("90.0"));
        assert_eq!(depth.bid_counts.len(), 10);
        assert_eq!(depth.ask_counts.len(), 10);
        assert_eq!(depth.bid_counts[0], 1);
        assert_eq!(depth.ask_counts[0], 1);
        assert_eq!(depth.flags, 0);
        assert_eq!(depth.sequence, 0);
        assert_eq!(depth.ts_event, UnixNanos::from(1));
        assert_eq!(depth.ts_init, UnixNanos::from(2));
    }

    #[rstest]
    fn test_new(stub_depth10: OrderBookDepth) {
        let depth = stub_depth10;
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let flags = 0;
        let sequence = 0;
        let ts_event = 1;
        let ts_init = 2;

        assert_eq!(depth.instrument_id, instrument_id);
        assert_eq!(depth.bids.len(), 10);
        assert_eq!(depth.asks.len(), 10);
        assert_eq!(depth.asks[9].price, Price::from("109.0"));
        assert_eq!(depth.asks[0].price, Price::from("100.0"));
        assert_eq!(depth.bids[0].price, Price::from("99.0"));
        assert_eq!(depth.bids[9].price, Price::from("90.0"));
        assert_eq!(depth.bid_counts.len(), 10);
        assert_eq!(depth.ask_counts.len(), 10);
        assert_eq!(depth.bid_counts[0], 1);
        assert_eq!(depth.ask_counts[0], 1);
        assert_eq!(depth.flags, flags);
        assert_eq!(depth.sequence, sequence);
        assert_eq!(depth.ts_event, ts_event);
        assert_eq!(depth.ts_init, ts_init);
    }

    #[rstest]
    fn test_display(stub_depth10: OrderBookDepth) {
        let depth = stub_depth10;
        assert_eq!(
            format!("{depth}"),
            "AAPL.XNAS,flags=0,sequence=0,ts_event=1,ts_init=2".to_string()
        );
    }
}
