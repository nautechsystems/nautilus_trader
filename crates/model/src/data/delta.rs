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

//! An `OrderBookDelta` data type intended to carry book state information.

use std::{collections::HashMap, fmt::Display, hash::Hash};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, correctness::FAILED, serialization::Serializable};
use serde::{Deserialize, Serialize};

use super::{
    HasTsInit,
    order::{BookOrder, NULL_ORDER},
};
use crate::{
    enums::{BookAction, RecordFlag},
    identifiers::InstrumentId,
    types::{fixed::FIXED_SIZE_BINARY, quantity::check_positive_quantity},
};

/// Represents a single change/delta in an order book.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OrderBookDelta {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The order book delta action.
    pub action: BookAction,
    /// The order to apply.
    pub order: BookOrder,
    /// The record flags bit field indicating event end and data information.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// UNIX timestamp (nanoseconds) when the book event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl OrderBookDelta {
    /// Creates a new [`OrderBookDelta`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `action` is [`BookAction::Add`] or [`BookAction::Update`] and `size` is not positive (> 0).
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        if matches!(action, BookAction::Add | BookAction::Update) {
            check_positive_quantity(order.size, stringify!(order.size))?;
        }

        Ok(Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`OrderBookDelta`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `action` is [`BookAction::Add`] or [`BookAction::Update`] and `size` is not positive (> 0).
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
        .expect(FAILED)
    }

    /// Creates a new [`OrderBookDelta`] instance with a `Clear` action and NULL order.
    #[must_use]
    pub fn clear(
        instrument_id: InstrumentId,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            action: BookAction::Clear,
            order: NULL_ORDER,
            flags: RecordFlag::F_SNAPSHOT as u8,
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
        metadata.insert("action".to_string(), "UInt8".to_string());
        metadata.insert("side".to_string(), "UInt8".to_string());
        metadata.insert("price".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("size".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("order_id".to_string(), "UInt64".to_string());
        metadata.insert("flags".to_string(), "UInt8".to_string());
        metadata.insert("sequence".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

impl Display for OrderBookDelta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.instrument_id,
            self.action,
            self.order,
            self.flags,
            self.sequence,
            self.ts_event,
            self.ts_init
        )
    }
}

impl Serializable for OrderBookDelta {}

impl HasTsInit for OrderBookDelta {
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

    use nautilus_core::{
        UnixNanos,
        serialization::{
            Serializable,
            msgpack::{FromMsgPack, ToMsgPack},
        },
    };
    use rstest::rstest;

    use crate::{
        data::{BookOrder, HasTsInit, OrderBookDelta, stubs::*},
        enums::{BookAction, OrderSide, RecordFlag},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };

    fn create_test_delta() -> OrderBookDelta {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.0500"),
            Quantity::from("100000"),
            12345,
        );
        OrderBookDelta::new(
            InstrumentId::from("EURUSD.SIM"),
            BookAction::Add,
            order,
            0,
            123,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    #[rstest]
    fn test_order_book_delta_new() {
        let delta = create_test_delta();

        assert_eq!(delta.instrument_id, InstrumentId::from("EURUSD.SIM"));
        assert_eq!(delta.action, BookAction::Add);
        assert_eq!(delta.order.side, OrderSide::Buy);
        assert_eq!(delta.order.price, Price::from("1.0500"));
        assert_eq!(delta.order.size, Quantity::from("100000"));
        assert_eq!(delta.order.order_id, 12345);
        assert_eq!(delta.flags, 0);
        assert_eq!(delta.sequence, 123);
        assert_eq!(delta.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(delta.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_book_delta_new_checked_valid() {
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.0505"),
            Quantity::from("50000"),
            67890,
        );
        let result = OrderBookDelta::new_checked(
            InstrumentId::from("GBPUSD.SIM"),
            BookAction::Update,
            order,
            16,
            456,
            UnixNanos::from(500_000_000),
            UnixNanos::from(1_500_000_000),
        );

        assert!(result.is_ok());
        let delta = result.unwrap();
        assert_eq!(delta.instrument_id, InstrumentId::from("GBPUSD.SIM"));
        assert_eq!(delta.action, BookAction::Update);
        assert_eq!(delta.order.side, OrderSide::Sell);
        assert_eq!(delta.flags, 16);
    }

    #[rstest]
    fn test_order_book_delta_new_with_zero_size_panics() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let zero_size = Quantity::from(0);
        let side = OrderSide::Buy;
        let order_id = 123_456;
        let flags = 0;
        let sequence = 1;
        let ts_event = UnixNanos::from(0);
        let ts_init = UnixNanos::from(1);

        let order = BookOrder::new(side, price, zero_size, order_id);

        let result = std::panic::catch_unwind(|| {
            let _ = OrderBookDelta::new(
                instrument_id,
                action,
                order,
                flags,
                sequence,
                ts_event,
                ts_init,
            );
        });
        assert!(result.is_err());
    }

    #[rstest]
    fn test_order_book_delta_new_checked_with_zero_size_error() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let zero_size = Quantity::from(0);
        let side = OrderSide::Buy;
        let order_id = 123_456;
        let flags = 0;
        let sequence = 1;
        let ts_event = UnixNanos::from(0);
        let ts_init = UnixNanos::from(1);

        let order = BookOrder::new(side, price, zero_size, order_id);

        let result = OrderBookDelta::new_checked(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid `Quantity` for 'order.size' not positive")
        );
    }

    #[rstest]
    fn test_order_book_delta_new_checked_delete_with_zero_size_ok() {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from(0),
            123_456,
        );
        let result = OrderBookDelta::new_checked(
            InstrumentId::from("TEST.SIM"),
            BookAction::Delete,
            order,
            0,
            1,
            UnixNanos::from(0),
            UnixNanos::from(1),
        );

        assert!(result.is_ok());
    }

    #[rstest]
    fn test_order_book_delta_clear() {
        let instrument_id = InstrumentId::from("BTCUSD.CRYPTO");
        let sequence = 999;
        let ts_event = UnixNanos::from(3_000_000_000);
        let ts_init = UnixNanos::from(4_000_000_000);

        let delta = OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init);

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, BookAction::Clear);
        assert!(delta.order.price.is_zero());
        assert!(delta.order.size.is_zero());
        assert_eq!(delta.order.side, OrderSide::NoOrderSide);
        assert_eq!(delta.order.order_id, 0);
        assert_eq!(delta.flags, RecordFlag::F_SNAPSHOT as u8);
        assert_eq!(delta.sequence, sequence);
        assert_eq!(delta.ts_event, ts_event);
        assert_eq!(delta.ts_init, ts_init);
    }

    #[rstest]
    fn test_get_metadata() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let metadata = OrderBookDelta::get_metadata(&instrument_id, 5, 8);

        assert_eq!(metadata.len(), 3);
        assert_eq!(
            metadata.get("instrument_id"),
            Some(&"EURUSD.SIM".to_string())
        );
        assert_eq!(metadata.get("price_precision"), Some(&"5".to_string()));
        assert_eq!(metadata.get("size_precision"), Some(&"8".to_string()));
    }

    #[rstest]
    fn test_get_fields() {
        let fields = OrderBookDelta::get_fields();

        assert_eq!(fields.len(), 9);
        assert_eq!(fields.get("action"), Some(&"UInt8".to_string()));
        assert_eq!(fields.get("side"), Some(&"UInt8".to_string()));

        #[cfg(feature = "high-precision")]
        {
            assert_eq!(
                fields.get("price"),
                Some(&"FixedSizeBinary(16)".to_string())
            );
            assert_eq!(fields.get("size"), Some(&"FixedSizeBinary(16)".to_string()));
        }
        #[cfg(not(feature = "high-precision"))]
        {
            assert_eq!(fields.get("price"), Some(&"FixedSizeBinary(8)".to_string()));
            assert_eq!(fields.get("size"), Some(&"FixedSizeBinary(8)".to_string()));
        }

        assert_eq!(fields.get("order_id"), Some(&"UInt64".to_string()));
        assert_eq!(fields.get("flags"), Some(&"UInt8".to_string()));
        assert_eq!(fields.get("sequence"), Some(&"UInt64".to_string()));
        assert_eq!(fields.get("ts_event"), Some(&"UInt64".to_string()));
        assert_eq!(fields.get("ts_init"), Some(&"UInt64".to_string()));
    }

    #[rstest]
    #[case(BookAction::Add)]
    #[case(BookAction::Update)]
    #[case(BookAction::Delete)]
    #[case(BookAction::Clear)]
    fn test_order_book_delta_with_different_actions(#[case] action: BookAction) {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            if matches!(action, BookAction::Delete | BookAction::Clear) {
                Quantity::from(0)
            } else {
                Quantity::from("1000")
            },
            123_456,
        );

        let result = if matches!(action, BookAction::Clear) {
            Ok(OrderBookDelta::clear(
                InstrumentId::from("TEST.SIM"),
                1,
                UnixNanos::from(1_000_000_000),
                UnixNanos::from(2_000_000_000),
            ))
        } else {
            OrderBookDelta::new_checked(
                InstrumentId::from("TEST.SIM"),
                action,
                order,
                0,
                1,
                UnixNanos::from(1_000_000_000),
                UnixNanos::from(2_000_000_000),
            )
        };

        assert!(result.is_ok());
        let delta = result.unwrap();
        assert_eq!(delta.action, action);
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_order_book_delta_with_different_sides(#[case] side: OrderSide) {
        let order = BookOrder::new(side, Price::from("100.00"), Quantity::from("1000"), 123_456);

        let delta = OrderBookDelta::new(
            InstrumentId::from("TEST.SIM"),
            BookAction::Add,
            order,
            0,
            1,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        assert_eq!(delta.order.side, side);
    }

    #[rstest]
    fn test_order_book_delta_has_ts_init() {
        let delta = create_test_delta();
        assert_eq!(delta.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_order_book_delta_display() {
        let delta = create_test_delta();
        let display_str = format!("{delta}");

        assert!(display_str.contains("EURUSD.SIM"));
        assert!(display_str.contains("ADD"));
        assert!(display_str.contains("BUY"));
        assert!(display_str.contains("1.0500"));
        assert!(display_str.contains("100000"));
        assert!(display_str.contains("12345"));
        assert!(display_str.contains("123"));
    }

    #[rstest]
    fn test_order_book_delta_with_zero_timestamps() {
        let order = BookOrder::new(
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from("1000"),
            123_456,
        );
        let delta = OrderBookDelta::new(
            InstrumentId::from("TEST.SIM"),
            BookAction::Add,
            order,
            0,
            0,
            UnixNanos::from(0),
            UnixNanos::from(0),
        );

        assert_eq!(delta.sequence, 0);
        assert_eq!(delta.ts_event, UnixNanos::from(0));
        assert_eq!(delta.ts_init, UnixNanos::from(0));
    }

    #[rstest]
    fn test_order_book_delta_with_max_values() {
        let order = BookOrder::new(
            OrderSide::Sell,
            Price::from("999999.9999"),
            Quantity::from("999999999.9999"),
            u64::MAX,
        );
        let delta = OrderBookDelta::new(
            InstrumentId::from("TEST.SIM"),
            BookAction::Update,
            order,
            u8::MAX,
            u64::MAX,
            UnixNanos::from(u64::MAX),
            UnixNanos::from(u64::MAX),
        );

        assert_eq!(delta.flags, u8::MAX);
        assert_eq!(delta.sequence, u64::MAX);
        assert_eq!(delta.order.order_id, u64::MAX);
        assert_eq!(delta.ts_event, UnixNanos::from(u64::MAX));
        assert_eq!(delta.ts_init, UnixNanos::from(u64::MAX));
    }

    #[rstest]
    fn test_new() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123_456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price, size, order_id);

        let delta = OrderBookDelta::new(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event.into(),
            ts_init.into(),
        );

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, action);
        assert_eq!(delta.order.price, price);
        assert_eq!(delta.order.size, size);
        assert_eq!(delta.order.side, side);
        assert_eq!(delta.order.order_id, order_id);
        assert_eq!(delta.flags, flags);
        assert_eq!(delta.sequence, sequence);
        assert_eq!(delta.ts_event, ts_event);
        assert_eq!(delta.ts_init, ts_init);
    }

    #[rstest]
    fn test_clear() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let sequence = 1;
        let ts_event = 2;
        let ts_init = 3;

        let delta = OrderBookDelta::clear(instrument_id, sequence, ts_event.into(), ts_init.into());

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, BookAction::Clear);
        assert!(delta.order.price.is_zero());
        assert!(delta.order.size.is_zero());
        assert_eq!(delta.order.side, OrderSide::NoOrderSide);
        assert_eq!(delta.order.order_id, 0);
        assert_eq!(delta.flags, 32);
        assert_eq!(delta.sequence, sequence);
        assert_eq!(delta.ts_event, ts_event);
        assert_eq!(delta.ts_init, ts_init);
    }

    #[rstest]
    fn test_order_book_delta_hash() {
        let delta1 = create_test_delta();
        let delta2 = create_test_delta();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        delta1.hash(&mut hasher1);
        delta2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_order_book_delta_hash_different_deltas() {
        let delta1 = create_test_delta();
        let order2 = BookOrder::new(
            OrderSide::Sell,
            Price::from("1.0505"),
            Quantity::from("50000"),
            67890,
        );
        let delta2 = OrderBookDelta::new(
            InstrumentId::from("EURUSD.SIM"),
            BookAction::Add,
            order2,
            0,
            123,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        delta1.hash(&mut hasher1);
        delta2.hash(&mut hasher2);

        assert_ne!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_order_book_delta_partial_eq() {
        let delta1 = create_test_delta();
        let delta2 = create_test_delta();

        // Test equality
        assert_eq!(delta1, delta2);

        // Test inequality with different instrument
        let order3 = BookOrder::new(
            OrderSide::Buy,
            Price::from("1.0500"),
            Quantity::from("100000"),
            12345,
        );
        let delta3 = OrderBookDelta::new(
            InstrumentId::from("GBPUSD.SIM"),
            BookAction::Add,
            order3,
            0,
            123,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        assert_ne!(delta1, delta3);
    }

    #[rstest]
    fn test_order_book_delta_clone() {
        let delta1 = create_test_delta();
        let delta2 = delta1;

        assert_eq!(delta1, delta2);
        assert_eq!(delta1.instrument_id, delta2.instrument_id);
        assert_eq!(delta1.action, delta2.action);
        assert_eq!(delta1.order, delta2.order);
        assert_eq!(delta1.flags, delta2.flags);
        assert_eq!(delta1.sequence, delta2.sequence);
        assert_eq!(delta1.ts_event, delta2.ts_event);
        assert_eq!(delta1.ts_init, delta2.ts_init);
    }

    #[rstest]
    fn test_order_book_delta_debug() {
        let delta = create_test_delta();
        let debug_str = format!("{delta:?}");

        assert!(debug_str.contains("OrderBookDelta"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("Add"));
        assert!(debug_str.contains("BUY"));
        assert!(debug_str.contains("1.0500"));
    }

    #[rstest]
    fn test_order_book_delta_serialization() {
        let delta = create_test_delta();

        let json = serde_json::to_string(&delta).unwrap();
        let deserialized: OrderBookDelta = serde_json::from_str(&json).unwrap();

        assert_eq!(delta, deserialized);
    }

    #[rstest]
    fn test_json_serialization(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        let serialized = delta.to_json_bytes().unwrap();
        let deserialized = OrderBookDelta::from_json_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, delta);
    }

    #[rstest]
    fn test_msgpack_serialization(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        let serialized = delta.to_msgpack_bytes().unwrap();
        let deserialized = OrderBookDelta::from_msgpack_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, delta);
    }
}
