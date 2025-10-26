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

use super::HasTsInit;
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
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl TradeTick {
    /// Creates a new [`TradeTick`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `size` is not positive (> 0).
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
    /// Panics if `size` is not positive (> 0).
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

impl HasTsInit for TradeTick {
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

    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::TradeTickBuilder;
    use crate::{
        data::{HasTsInit, TradeTick, stubs::stub_trade_ethusdt_buyer},
        enums::AggressorSide,
        identifiers::{InstrumentId, TradeId},
        types::{Price, Quantity},
    };

    fn create_test_trade() -> TradeTick {
        TradeTick::new(
            InstrumentId::from("EURUSD.SIM"),
            Price::from("1.0500"),
            Quantity::from("100000"),
            AggressorSide::Buyer,
            TradeId::from("T-001"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    #[rstest]
    fn test_trade_tick_new() {
        let trade = create_test_trade();

        assert_eq!(trade.instrument_id, InstrumentId::from("EURUSD.SIM"));
        assert_eq!(trade.price, Price::from("1.0500"));
        assert_eq!(trade.size, Quantity::from("100000"));
        assert_eq!(trade.aggressor_side, AggressorSide::Buyer);
        assert_eq!(trade.trade_id, TradeId::from("T-001"));
        assert_eq!(trade.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(trade.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_trade_tick_new_checked_valid() {
        let result = TradeTick::new_checked(
            InstrumentId::from("GBPUSD.SIM"),
            Price::from("1.2500"),
            Quantity::from("50000"),
            AggressorSide::Seller,
            TradeId::from("T-002"),
            UnixNanos::from(500_000_000),
            UnixNanos::from(1_500_000_000),
        );

        assert!(result.is_ok());
        let trade = result.unwrap();
        assert_eq!(trade.instrument_id, InstrumentId::from("GBPUSD.SIM"));
        assert_eq!(trade.price, Price::from("1.2500"));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
    }

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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid `Quantity` for 'size' not positive")
        );
    }

    #[rstest]
    fn test_trade_tick_builder() {
        let trade = TradeTickBuilder::default()
            .instrument_id(InstrumentId::from("BTCUSD.CRYPTO"))
            .price(Price::from("50000.00"))
            .size(Quantity::from("0.50"))
            .aggressor_side(AggressorSide::Seller)
            .trade_id(TradeId::from("T-999"))
            .ts_event(UnixNanos::from(3_000_000_000))
            .ts_init(UnixNanos::from(4_000_000_000))
            .build()
            .unwrap();

        assert_eq!(trade.instrument_id, InstrumentId::from("BTCUSD.CRYPTO"));
        assert_eq!(trade.price, Price::from("50000.00"));
        assert_eq!(trade.size, Quantity::from("0.50"));
        assert_eq!(trade.aggressor_side, AggressorSide::Seller);
        assert_eq!(trade.trade_id, TradeId::from("T-999"));
        assert_eq!(trade.ts_event, UnixNanos::from(3_000_000_000));
        assert_eq!(trade.ts_init, UnixNanos::from(4_000_000_000));
    }

    #[rstest]
    fn test_get_metadata() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let metadata = TradeTick::get_metadata(&instrument_id, 5, 8);

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
        let fields = TradeTick::get_fields();

        assert_eq!(fields.len(), 6);

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

        assert_eq!(fields.get("aggressor_side"), Some(&"UInt8".to_string()));
        assert_eq!(fields.get("trade_id"), Some(&"Utf8".to_string()));
        assert_eq!(fields.get("ts_event"), Some(&"UInt64".to_string()));
        assert_eq!(fields.get("ts_init"), Some(&"UInt64".to_string()));
    }

    #[rstest]
    #[case(AggressorSide::Buyer)]
    #[case(AggressorSide::Seller)]
    #[case(AggressorSide::NoAggressor)]
    fn test_trade_tick_with_different_aggressor_sides(#[case] aggressor_side: AggressorSide) {
        let trade = TradeTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("100.00"),
            Quantity::from("1000"),
            aggressor_side,
            TradeId::from("T-TEST"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        assert_eq!(trade.aggressor_side, aggressor_side);
    }

    #[rstest]
    fn test_trade_tick_hash() {
        let trade1 = create_test_trade();
        let trade2 = create_test_trade();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        trade1.hash(&mut hasher1);
        trade2.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_trade_tick_hash_different_trades() {
        let trade1 = create_test_trade();
        let mut trade2 = create_test_trade();
        trade2.price = Price::from("1.0501");

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        trade1.hash(&mut hasher1);
        trade2.hash(&mut hasher2);

        assert_ne!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_trade_tick_partial_eq() {
        let trade1 = create_test_trade();
        let trade2 = create_test_trade();
        let mut trade3 = create_test_trade();
        trade3.size = Quantity::from("80000");

        assert_eq!(trade1, trade2);
        assert_ne!(trade1, trade3);
    }

    #[rstest]
    fn test_trade_tick_clone() {
        let trade1 = create_test_trade();
        let trade2 = trade1;

        assert_eq!(trade1, trade2);
        assert_eq!(trade1.instrument_id, trade2.instrument_id);
        assert_eq!(trade1.price, trade2.price);
        assert_eq!(trade1.size, trade2.size);
        assert_eq!(trade1.aggressor_side, trade2.aggressor_side);
        assert_eq!(trade1.trade_id, trade2.trade_id);
        assert_eq!(trade1.ts_event, trade2.ts_event);
        assert_eq!(trade1.ts_init, trade2.ts_init);
    }

    #[rstest]
    fn test_trade_tick_debug() {
        let trade = create_test_trade();
        let debug_str = format!("{trade:?}");

        assert!(debug_str.contains("TradeTick"));
        assert!(debug_str.contains("EURUSD.SIM"));
        assert!(debug_str.contains("1.0500"));
        assert!(debug_str.contains("Buyer"));
        assert!(debug_str.contains("T-001"));
    }

    #[rstest]
    fn test_trade_tick_has_ts_init() {
        let trade = create_test_trade();
        assert_eq!(trade.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_trade_tick_display() {
        let trade = create_test_trade();
        let display_str = format!("{trade}");

        assert!(display_str.contains("EURUSD.SIM"));
        assert!(display_str.contains("1.0500"));
        assert!(display_str.contains("100000"));
        assert!(display_str.contains("BUYER"));
        assert!(display_str.contains("T-001"));
        assert!(display_str.contains("1000000000"));
    }

    #[rstest]
    fn test_trade_tick_serialization() {
        let trade = create_test_trade();

        let json = serde_json::to_string(&trade).unwrap();
        let deserialized: TradeTick = serde_json::from_str(&json).unwrap();

        assert_eq!(trade, deserialized);
    }

    #[rstest]
    fn test_trade_tick_with_zero_price() {
        let trade = TradeTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("0.0000"),
            Quantity::from("1000.0000"),
            AggressorSide::Buyer,
            TradeId::from("T-ZERO"),
            UnixNanos::from(0),
            UnixNanos::from(0),
        );

        assert!(trade.price.is_zero());
        assert_eq!(trade.ts_event, UnixNanos::from(0));
        assert_eq!(trade.ts_init, UnixNanos::from(0));
    }

    #[rstest]
    fn test_trade_tick_with_max_values() {
        let trade = TradeTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("999999.9999"),
            Quantity::from("999999999.9999"),
            AggressorSide::Seller,
            TradeId::from("T-MAX"),
            UnixNanos::from(u64::MAX),
            UnixNanos::from(u64::MAX),
        );

        assert_eq!(trade.ts_event, UnixNanos::from(u64::MAX));
        assert_eq!(trade.ts_init, UnixNanos::from(u64::MAX));
    }

    #[rstest]
    fn test_trade_tick_with_different_trade_ids() {
        let trade1 = TradeTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("100.00"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::from("TRADE-123"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        let trade2 = TradeTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("100.00"),
            Quantity::from("1000"),
            AggressorSide::Buyer,
            TradeId::from("TRADE-456"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        assert_ne!(trade1.trade_id, trade2.trade_id);
        assert_ne!(trade1, trade2);
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
        assert_eq!(
            trade.instrument_id,
            InstrumentId::from("ETHUSDT-PERP.BINANCE")
        );
        assert_eq!(trade.price, Price::from("10000.0000"));
        assert_eq!(trade.size, Quantity::from("1.00000000"));
        assert_eq!(trade.trade_id, TradeId::from("123456789"));
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_from_pyobject(stub_trade_ethusdt_buyer: TradeTick) {
        use pyo3::{IntoPyObjectExt, Python};

        let trade = stub_trade_ethusdt_buyer;

        Python::initialize();
        Python::attach(|py| {
            let tick_pyobject = trade.into_py_any(py).unwrap();
            let parsed_tick = TradeTick::from_pyobject(tick_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_tick, trade);
        });
    }
}
