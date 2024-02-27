// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
};

use indexmap::IndexMap;
use nautilus_core::{serialization::Serializable, time::UnixNanos};
use serde::{Deserialize, Serialize};

use crate::{
    enums::AggressorSide,
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    types::{price::Price, quantity::Quantity},
};

/// Represents a single trade tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct TradeTick {
    /// The trade instrument ID.
    pub instrument_id: InstrumentId,
    /// The traded price.
    pub price: Price,
    /// The traded size.
    pub size: Quantity,
    /// The trade aggressor side.
    pub aggressor_side: AggressorSide,
    /// The trade match ID (assigned by the venue).
    pub trade_id: TradeId,
    /// The UNIX timestamp (nanoseconds) when the tick event occurred.
    pub ts_event: UnixNanos,
    ///  The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl TradeTick {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
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
        metadata.insert("price".to_string(), "Int64".to_string());
        metadata.insert("size".to_string(), "UInt64".to_string());
        metadata.insert("aggressor_side".to_string(), "UInt8".to_string());
        metadata.insert("trade_id".to_string(), "Utf8".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

impl Display for TradeTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id,
            self.price,
            self.size,
            self.aggressor_side,
            self.trade_id,
            self.ts_event,
        )
    }
}

impl Serializable for TradeTick {}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "stubs")]
pub mod stubs {
    use rstest::fixture;

    use crate::{
        data::trade::TradeTick,
        enums::AggressorSide,
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
        types::{price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn stub_trade_tick_ethusdt_buyer() -> TradeTick {
        TradeTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            price: Price::from("10000.0000"),
            size: Quantity::from("1.00000000"),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789").unwrap(),
            ts_event: 0,
            ts_init: 1,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::serialization::Serializable;
    use pyo3::{IntoPy, Python};
    use rstest::rstest;

    use super::stubs::*;
    use crate::{data::trade::TradeTick, enums::AggressorSide};

    #[rstest]
    fn test_to_string(stub_trade_tick_ethusdt_buyer: TradeTick) {
        let trade = stub_trade_tick_ethusdt_buyer;
        assert_eq!(
            trade.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,1.00000000,BUYER,123456789,0"
        );
    }

    #[rstest]
    fn test_deserialize_raw_string() {
        let raw_string = r#"{
            "type": "TradeTick",
            "instrument_id": "ETHUSDT-PERP.BINANCE",
            "price": "10000.0000",
            "size": "1.00000000",
            "aggressor_side": "BUYER",
            "trade_id": "123456789",
            "ts_event": 0,
            "ts_init": 1
        }"#;

        let trade: TradeTick = serde_json::from_str(raw_string).unwrap();

        assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
    }

    #[rstest]
    fn test_from_pyobject(stub_trade_tick_ethusdt_buyer: TradeTick) {
        pyo3::prepare_freethreaded_python();
        let trade = stub_trade_tick_ethusdt_buyer;

        Python::with_gil(|py| {
            let tick_pyobject = trade.into_py(py);
            let parsed_tick = TradeTick::from_pyobject(tick_pyobject.as_ref(py)).unwrap();
            assert_eq!(parsed_tick, trade);
        });
    }

    #[rstest]
    fn test_json_serialization(stub_trade_tick_ethusdt_buyer: TradeTick) {
        let trade = stub_trade_tick_ethusdt_buyer;
        let serialized = trade.as_json_bytes().unwrap();
        let deserialized = TradeTick::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, trade);
    }

    #[rstest]
    fn test_msgpack_serialization(stub_trade_tick_ethusdt_buyer: TradeTick) {
        let trade = stub_trade_tick_ethusdt_buyer;
        let serialized = trade.as_msgpack_bytes().unwrap();
        let deserialized = TradeTick::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, trade);
    }
}
