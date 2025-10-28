// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! An `OrderBookDepth10` aggregated top-of-book data type with a fixed depth of 10 levels per side.

use std::{collections::HashMap, fmt::Display};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};

use super::{HasTsInit, order::BookOrder};
use crate::{identifiers::InstrumentId, types::fixed::FIXED_SIZE_BINARY};

pub const DEPTH10_LEN: usize = 10;

/// Represents an aggregated order book update with a fixed depth of 10 levels per side.
///
/// This structure is specifically designed for scenarios where a snapshot of the top 10 bid and
/// ask levels in an order book is needed. It differs from `OrderBookDelta` or `OrderBookDeltas`
/// in its fixed-depth nature and is optimized for cases where a full depth representation is not
/// required or practical.
///
/// Note: This type is not compatible with `OrderBookDelta` or `OrderBookDeltas` due to
/// its specialized structure and limited depth use case.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBookDepth10 {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The bid orders for the depth update.
    pub bids: [BookOrder; DEPTH10_LEN],
    /// The ask orders for the depth update.
    pub asks: [BookOrder; DEPTH10_LEN],
    /// The count of bid orders per level for the depth update.
    pub bid_counts: [u32; DEPTH10_LEN],
    /// The count of ask orders per level for the depth update.
    pub ask_counts: [u32; DEPTH10_LEN],
    /// The record flags bit field, indicating event end and data information.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// UNIX timestamp (nanoseconds) when the book event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl OrderBookDepth10 {
    /// Creates a new [`OrderBookDepth10`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        bids: [BookOrder; DEPTH10_LEN],
        asks: [BookOrder; DEPTH10_LEN],
        bid_counts: [u32; DEPTH10_LEN],
        ask_counts: [u32; DEPTH10_LEN],
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
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
        metadata.insert("bid_price_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_price_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_0".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_1".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_2".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_3".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_4".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_5".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_6".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_7".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_8".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size_9".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_count_0".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_1".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_2".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_3".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_4".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_5".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_6".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_7".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_8".to_string(), "UInt32".to_string());
        metadata.insert("bid_count_9".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_0".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_1".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_2".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_3".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_4".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_5".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_6".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_7".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_8".to_string(), "UInt32".to_string());
        metadata.insert("ask_count_9".to_string(), "UInt32".to_string());
        metadata.insert("flags".to_string(), "UInt8".to_string());
        metadata.insert("sequence".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

// TODO: Exact format for Debug and Display TBD
impl Display for OrderBookDepth10 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},flags={},sequence={},ts_event={},ts_init={}",
            self.instrument_id, self.flags, self.sequence, self.ts_event, self.ts_init
        )
    }
}

impl Serializable for OrderBookDepth10 {}

impl HasTsInit for OrderBookDepth10 {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
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
        data::{order::BookOrder, stubs::*},
        enums::OrderSide,
        types::{Price, Quantity},
    };

    fn create_test_book_order(
        side: OrderSide,
        price: &str,
        size: &str,
        order_id: u64,
    ) -> BookOrder {
        BookOrder::new(side, Price::from(price), Quantity::from(size), order_id)
    }

    fn create_test_depth10() -> OrderBookDepth10 {
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

        OrderBookDepth10::new(
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

    fn create_empty_depth10() -> OrderBookDepth10 {
        let instrument_id = InstrumentId::from("EMPTY.TEST");

        // Create empty orders with zero prices and quantities
        let empty_bid = create_test_book_order(OrderSide::Buy, "0.0", "0", 0);
        let empty_ask = create_test_book_order(OrderSide::Sell, "0.0", "0", 0);

        OrderBookDepth10::new(
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
    fn test_order_book_depth10_new() {
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
    fn test_order_book_depth10_new_with_all_parameters() {
        let instrument_id = InstrumentId::from("GBPUSD.SIM");
        let bid = create_test_book_order(OrderSide::Buy, "1.2500", "50000", 1);
        let ask = create_test_book_order(OrderSide::Sell, "1.2501", "75000", 2);
        let flags = 64u8;
        let sequence = 999u64;
        let ts_event = UnixNanos::from(5_000_000_000);
        let ts_init = UnixNanos::from(6_000_000_000);

        let depth = OrderBookDepth10::new(
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
    fn test_order_book_depth10_fixed_array_sizes() {
        let depth = create_test_depth10();

        // Verify arrays are exactly DEPTH10_LEN (10)
        assert_eq!(depth.bids.len(), 10);
        assert_eq!(depth.asks.len(), 10);
        assert_eq!(depth.bid_counts.len(), 10);
        assert_eq!(depth.ask_counts.len(), 10);

        // Verify DEPTH10_LEN constant
        assert_eq!(DEPTH10_LEN, 10);
    }

    #[rstest]
    fn test_order_book_depth10_array_indexing() {
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
    fn test_order_book_depth10_bid_ask_ordering() {
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
    fn test_order_book_depth10_clone() {
        let depth1 = create_test_depth10();
        let depth2 = depth1;

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
    fn test_order_book_depth10_copy() {
        let depth1 = create_test_depth10();
        let depth2 = depth1;

        // Verify Copy trait by modifying one and ensuring the other is unchanged
        // Since we're using Copy, this should work without explicit clone
        assert_eq!(depth1, depth2);
    }

    #[rstest]
    fn test_order_book_depth10_debug() {
        let depth = create_test_depth10();
        let debug_str = format!("{depth:?}");

        assert!(debug_str.contains("OrderBookDepth10"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("flags: 32"));
        assert!(debug_str.contains("sequence: 12345"));
    }

    #[rstest]
    fn test_order_book_depth10_partial_eq() {
        let depth1 = create_test_depth10();
        let depth2 = create_test_depth10();
        let depth3 = create_empty_depth10();

        assert_eq!(depth1, depth2); // Same data
        assert_ne!(depth1, depth3); // Different data
        assert_ne!(depth2, depth3); // Different data
    }

    #[rstest]
    fn test_order_book_depth10_eq_consistency() {
        let depth1 = create_test_depth10();
        let depth2 = create_test_depth10();

        assert_eq!(depth1, depth2);
        assert_eq!(depth2, depth1); // Symmetry
        assert_eq!(depth1, depth1); // Reflexivity
    }

    #[rstest]
    fn test_order_book_depth10_hash() {
        let depth1 = create_test_depth10();
        let depth2 = create_test_depth10();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        depth1.hash(&mut hasher1);
        depth2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish()); // Equal objects have equal hashes
    }

    #[rstest]
    fn test_order_book_depth10_hash_different_objects() {
        let depth1 = create_test_depth10();
        let depth2 = create_empty_depth10();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        depth1.hash(&mut hasher1);
        depth2.hash(&mut hasher2);

        assert_ne!(hasher1.finish(), hasher2.finish()); // Different objects should have different hashes
    }

    #[rstest]
    fn test_order_book_depth10_display() {
        let depth = create_test_depth10();
        let display_str = format!("{depth}");

        assert!(display_str.contains("EURUSD.SIM"));
        assert!(display_str.contains("flags=32"));
        assert!(display_str.contains("sequence=12345"));
        assert!(display_str.contains("ts_event=1000000000"));
        assert!(display_str.contains("ts_init=2000000000"));
    }

    #[rstest]
    fn test_order_book_depth10_display_format() {
        let depth = create_test_depth10();
        let expected = "EURUSD.SIM,flags=32,sequence=12345,ts_event=1000000000,ts_init=2000000000";

        assert_eq!(format!("{depth}"), expected);
    }

    #[rstest]
    fn test_order_book_depth10_serialization() {
        let depth = create_test_depth10();

        // Test JSON serialization
        let json = serde_json::to_string(&depth).unwrap();
        let deserialized: OrderBookDepth10 = serde_json::from_str(&json).unwrap();

        assert_eq!(depth, deserialized);
    }

    #[rstest]
    fn test_order_book_depth10_serializable_trait() {
        let depth = create_test_depth10();

        // Verify Serializable trait is implemented (compile-time check)
        fn assert_serializable<T: Serializable>(_: &T) {}
        assert_serializable(&depth);
    }

    #[rstest]
    fn test_order_book_depth10_has_ts_init() {
        let depth = create_test_depth10();

        assert_eq!(depth.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_book_depth10_get_metadata() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let price_precision = 5u8;
        let size_precision = 0u8;

        let metadata =
            OrderBookDepth10::get_metadata(&instrument_id, price_precision, size_precision);

        assert_eq!(
            metadata.get("instrument_id"),
            Some(&"EURUSD.SIM".to_string())
        );
        assert_eq!(metadata.get("price_precision"), Some(&"5".to_string()));
        assert_eq!(metadata.get("size_precision"), Some(&"0".to_string()));
        assert_eq!(metadata.len(), 3);
    }

    #[rstest]
    fn test_order_book_depth10_get_fields() {
        let fields = OrderBookDepth10::get_fields();

        // Verify all 10 bid and ask price fields
        for i in 0..10 {
            assert_eq!(
                fields.get(&format!("bid_price_{i}")),
                Some(&FIXED_SIZE_BINARY.to_string())
            );
            assert_eq!(
                fields.get(&format!("ask_price_{i}")),
                Some(&FIXED_SIZE_BINARY.to_string())
            );
        }

        // Verify all 10 bid and ask size fields
        for i in 0..10 {
            assert_eq!(
                fields.get(&format!("bid_size_{i}")),
                Some(&FIXED_SIZE_BINARY.to_string())
            );
            assert_eq!(
                fields.get(&format!("ask_size_{i}")),
                Some(&FIXED_SIZE_BINARY.to_string())
            );
        }

        // Verify all 10 bid and ask count fields
        for i in 0..10 {
            assert_eq!(
                fields.get(&format!("bid_count_{i}")),
                Some(&"UInt32".to_string())
            );
            assert_eq!(
                fields.get(&format!("ask_count_{i}")),
                Some(&"UInt32".to_string())
            );
        }

        // Verify metadata fields
        assert_eq!(fields.get("flags"), Some(&"UInt8".to_string()));
        assert_eq!(fields.get("sequence"), Some(&"UInt64".to_string()));
        assert_eq!(fields.get("ts_event"), Some(&"UInt64".to_string()));
        assert_eq!(fields.get("ts_init"), Some(&"UInt64".to_string()));

        // Verify total field count:
        // 10 bid_price + 10 ask_price + 10 bid_size + 10 ask_size + 10 bid_count + 10 ask_count + 4 metadata = 64
        assert_eq!(fields.len(), 64);
    }

    #[rstest]
    fn test_order_book_depth10_get_fields_order() {
        let fields = OrderBookDepth10::get_fields();
        let keys: Vec<&String> = fields.keys().collect();

        // Verify the ordering of fields matches expectations
        assert_eq!(keys[0], "bid_price_0");
        assert_eq!(keys[9], "bid_price_9");
        assert_eq!(keys[10], "ask_price_0");
        assert_eq!(keys[19], "ask_price_9");
        assert_eq!(keys[20], "bid_size_0");
        assert_eq!(keys[29], "bid_size_9");
        assert_eq!(keys[30], "ask_size_0");
        assert_eq!(keys[39], "ask_size_9");
        assert_eq!(keys[40], "bid_count_0");
        assert_eq!(keys[41], "bid_count_1");
    }

    #[rstest]
    fn test_order_book_depth10_empty_values() {
        let depth = create_empty_depth10();

        assert_eq!(depth.instrument_id, InstrumentId::from("EMPTY.TEST"));
        assert_eq!(depth.flags, 0);
        assert_eq!(depth.sequence, 0);
        assert_eq!(depth.ts_event, UnixNanos::from(0));
        assert_eq!(depth.ts_init, UnixNanos::from(0));

        // Verify all orders have zero prices and quantities
        for bid in &depth.bids {
            assert_eq!(bid.price, Price::from("0.0"));
            assert_eq!(bid.size, Quantity::from("0"));
            assert_eq!(bid.order_id, 0);
        }

        for ask in &depth.asks {
            assert_eq!(ask.price, Price::from("0.0"));
            assert_eq!(ask.size, Quantity::from("0"));
            assert_eq!(ask.order_id, 0);
        }

        // Verify all counts are zero
        for &count in &depth.bid_counts {
            assert_eq!(count, 0);
        }

        for &count in &depth.ask_counts {
            assert_eq!(count, 0);
        }
    }

    #[rstest]
    fn test_order_book_depth10_max_values() {
        let instrument_id = InstrumentId::from("MAX.TEST");
        let max_bid = create_test_book_order(OrderSide::Buy, "999999.99", "999999999", u64::MAX);
        let max_ask = create_test_book_order(OrderSide::Sell, "1000000.00", "999999999", u64::MAX);

        let depth = OrderBookDepth10::new(
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
    fn test_order_book_depth10_different_instruments() {
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

            let depth = OrderBookDepth10::new(
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
    fn test_order_book_depth10_realistic_forex_spread() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");

        // Realistic EUR/USD spread with 0.1 pip spread
        let best_bid = create_test_book_order(OrderSide::Buy, "1.08500", "1000000", 1);
        let best_ask = create_test_book_order(OrderSide::Sell, "1.08501", "1000000", 2);

        let depth = OrderBookDepth10::new(
            instrument_id,
            [best_bid; DEPTH10_LEN],
            [best_ask; DEPTH10_LEN],
            [5; DEPTH10_LEN], // Realistic order count
            [3; DEPTH10_LEN],
            16,                                         // Realistic flags
            123456,                                     // Realistic sequence
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
    fn test_order_book_depth10_with_stub(stub_depth10: OrderBookDepth10) {
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
    fn test_new(stub_depth10: OrderBookDepth10) {
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
    fn test_display(stub_depth10: OrderBookDepth10) {
        let depth = stub_depth10;
        assert_eq!(
            format!("{depth}"),
            "AAPL.XNAS,flags=0,sequence=0,ts_event=1,ts_init=2".to_string()
        );
    }
}
