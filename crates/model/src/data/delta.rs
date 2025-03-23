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
    GetTsInit,
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
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl OrderBookDelta {
    /// Creates a new [`OrderBookDelta`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// This function returns an error:
    /// - If `action` is [`BookAction::Add`] or [`BookAction::Update`] and `size` is not positive (> 0).
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
    /// This function panics:
    /// - If `action` is [`BookAction::Add`] or [`BookAction::Update`] and `size` is not positive (> 0).
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

impl GetTsInit for OrderBookDelta {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::{UnixNanos, serialization::Serializable};
    use rstest::rstest;

    use crate::{
        data::{BookOrder, OrderBookDelta, stubs::*},
        enums::{BookAction, OrderSide},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };

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
    fn test_display(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        assert_eq!(
            format!("{delta}"),
            "AAPL.XNAS,ADD,BUY,100.00,10,123456,0,1,1,2".to_string()
        );
    }

    #[rstest]
    fn test_json_serialization(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        let serialized = delta.as_json_bytes().unwrap();
        let deserialized = OrderBookDelta::from_json_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, delta);
    }

    #[rstest]
    fn test_msgpack_serialization(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        let serialized = delta.as_msgpack_bytes().unwrap();
        let deserialized = OrderBookDelta::from_msgpack_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, delta);
    }
}
