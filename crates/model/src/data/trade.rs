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

//! A `TradeTick` data type representing a single trade in a market.

use std::{collections::HashMap, fmt::Display, hash::Hash};

use derive_builder::Builder;
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, correctness::FAILED, serialization::Serializable};
use serde::{Deserialize, Serialize};

use super::GetTsInit;
use crate::{
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity, fixed::FIXED_SIZE_BINARY, quantity::check_positive_quantity},
};

/// Represents a trade tick in a market.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Builder)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
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
    /// UNIX timestamp (nanoseconds) when the trade event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl TradeTick {
    /// Creates a new [`TradeTick`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// This function returns an error:
    /// - If `size` is not positive (> 0).
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_positive_quantity(size, stringify!(size))?;

        Ok(Self {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`TradeTick`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `size` is not positive (> 0).
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
        Self::new_checked(
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        )
        .expect(FAILED)
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
        metadata.insert("price".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("size".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("aggressor_side".to_string(), "UInt8".to_string());
        metadata.insert("trade_id".to_string(), "Utf8".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

impl Display for TradeTick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

impl GetTsInit for TradeTick {
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
    use pyo3::{IntoPyObjectExt, Python};
    use rstest::rstest;

    use crate::{
        data::{TradeTick, stubs::stub_trade_ethusdt_buyer},
        enums::AggressorSide,
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };

    #[cfg(feature = "high-precision")] // TODO: Add 64-bit precision version of test
    #[rstest]
    #[should_panic(expected = "invalid `Quantity` for 'size' not positive, was 0")]
    fn test_trade_tick_new_with_zero_size_panics() {
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
        let price = Price::from("10000.00");
        let zero_size = Quantity::from(0);
        let aggressor_side = AggressorSide::Buyer;
        let trade_id = TradeId::from("123456789");
        let ts_event = UnixNanos::from(0);
        let ts_init = UnixNanos::from(1);

        let _ = TradeTick::new(
            instrument_id,
            price,
            zero_size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        );
    }

    #[rstest]
    fn test_trade_tick_new_checked_with_zero_size_error() {
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
        let price = Price::from("10000.00");
        let zero_size = Quantity::from(0);
        let aggressor_side = AggressorSide::Buyer;
        let trade_id = TradeId::from("123456789");
        let ts_event = UnixNanos::from(0);
        let ts_init = UnixNanos::from(1);

        let result = TradeTick::new_checked(
            instrument_id,
            price,
            zero_size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        );

        assert!(result.is_err());
    }

    #[rstest]
    fn test_to_string(stub_trade_ethusdt_buyer: TradeTick) {
        let trade = stub_trade_ethusdt_buyer;
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
    fn test_from_pyobject(stub_trade_ethusdt_buyer: TradeTick) {
        pyo3::prepare_freethreaded_python();
        let trade = stub_trade_ethusdt_buyer;

        Python::with_gil(|py| {
            let tick_pyobject = trade.into_py_any(py).unwrap();
            let parsed_tick = TradeTick::from_pyobject(tick_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_tick, trade);
        });
    }

    #[rstest]
    fn test_json_serialization(stub_trade_ethusdt_buyer: TradeTick) {
        let trade = stub_trade_ethusdt_buyer;
        let serialized = trade.as_json_bytes().unwrap();
        let deserialized = TradeTick::from_json_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, trade);
    }

    #[rstest]
    fn test_msgpack_serialization(stub_trade_ethusdt_buyer: TradeTick) {
        let trade = stub_trade_ethusdt_buyer;
        let serialized = trade.as_msgpack_bytes().unwrap();
        let deserialized = TradeTick::from_msgpack_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, trade);
    }
}
