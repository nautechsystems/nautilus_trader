// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
    hash::Hash,
    str::FromStr,
};

use indexmap::IndexMap;
use nautilus_core::{python::to_pyvalue_err, serialization::Serializable, time::UnixNanos};
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};

use super::order::{BookOrder, OrderId, NULL_ORDER};
use crate::{
    enums::{BookAction, FromU8, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

/// Represents a single change/delta in an order book.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")]
pub struct OrderBookDelta {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The order book delta action.
    pub action: BookAction,
    /// The order to apply.
    pub order: BookOrder,
    /// A combination of packet end with matching engine status.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// The UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl OrderBookDelta {
    #[allow(clippy::too_many_arguments)]
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
        Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
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

    /// Returns the field map for the type, for use with arrow schemas.
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("action".to_string(), "UInt8".to_string());
        metadata.insert("side".to_string(), "UInt8".to_string());
        metadata.insert("price".to_string(), "Int64".to_string());
        metadata.insert("size".to_string(), "UInt64".to_string());
        metadata.insert("order_id".to_string(), "UInt64".to_string());
        metadata.insert("flags".to_string(), "UInt8".to_string());
        metadata.insert("sequence".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }

    /// Create a new [`OrderBookDelta`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &PyAny) -> PyResult<Self> {
        let instrument_id_obj: &PyAny = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id = InstrumentId::from_str(instrument_id_str)
            .map_err(to_pyvalue_err)
            .unwrap();

        let action_obj: &PyAny = obj.getattr("action")?.extract()?;
        let action_u8 = action_obj.getattr("value")?.extract()?;
        let action = BookAction::from_u8(action_u8).unwrap();

        let flags: u8 = obj.getattr("flags")?.extract()?;
        let sequence: u64 = obj.getattr("sequence")?.extract()?;
        let ts_event: UnixNanos = obj.getattr("ts_event")?.extract()?;
        let ts_init: UnixNanos = obj.getattr("ts_init")?.extract()?;

        let order_pyobject = obj.getattr("order")?;
        let order: BookOrder = if order_pyobject.is_none() {
            NULL_ORDER
        } else {
            let side_obj: &PyAny = order_pyobject.getattr("side")?.extract()?;
            let side_u8 = side_obj.getattr("value")?.extract()?;
            let side = OrderSide::from_u8(side_u8).unwrap();

            let price_py: &PyAny = order_pyobject.getattr("price")?;
            let price_raw: i64 = price_py.getattr("raw")?.extract()?;
            let price_prec: u8 = price_py.getattr("precision")?.extract()?;
            let price = Price::from_raw(price_raw, price_prec).map_err(to_pyvalue_err)?;

            let size_py: &PyAny = order_pyobject.getattr("size")?;
            let size_raw: u64 = size_py.getattr("raw")?.extract()?;
            let size_prec: u8 = size_py.getattr("precision")?.extract()?;
            let size = Quantity::from_raw(size_raw, size_prec).map_err(to_pyvalue_err)?;

            let order_id: OrderId = order_pyobject.getattr("order_id")?.extract()?;
            BookOrder {
                side,
                price,
                size,
                order_id,
            }
        };

        Ok(Self::new(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        ))
    }
}

impl Serializable for OrderBookDelta {}

impl Display for OrderBookDelta {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use super::*;
    use crate::{
        identifiers::instrument_id::InstrumentId,
        types::{price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn stub_delta() -> OrderBookDelta {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price, size, order_id);
        OrderBookDelta::new(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, *};
    use crate::{
        enums::OrderSide,
        types::{price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_new() {
        let instrument_id = InstrumentId::from("AAPL.NASDAQ");
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
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
            ts_event,
            ts_init,
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
    fn test_display(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        assert_eq!(
            format!("{}", delta),
            "AAPL.NASDAQ,ADD,100.00,10,BUY,123456,0,1,1,2".to_string()
        );
    }

    #[rstest]
    fn test_json_serialization(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        let serialized = delta.as_json_bytes().unwrap();
        let deserialized = OrderBookDelta::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, delta);
    }

    #[rstest]
    fn test_msgpack_serialization(stub_delta: OrderBookDelta) {
        let delta = stub_delta;
        let serialized = delta.as_msgpack_bytes().unwrap();
        let deserialized = OrderBookDelta::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, delta);
    }
}
