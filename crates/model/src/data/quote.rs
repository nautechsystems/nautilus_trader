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

//! A `QuoteTick` data type representing a top-of-book state.

use std::{cmp, collections::HashMap, fmt::Display, hash::Hash};

use derive_builder::Builder;
use indexmap::IndexMap;
use nautilus_core::{
    UnixNanos,
    correctness::{FAILED, check_equal_u8},
    serialization::Serializable,
};
use serde::{Deserialize, Serialize};

use super::HasTsInit;
use crate::{
    enums::PriceType,
    identifiers::InstrumentId,
    types::{
        Price, Quantity,
        fixed::{FIXED_PRECISION, FIXED_SIZE_BINARY},
    },
};

/// Represents a quote tick in a market.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Builder)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct QuoteTick {
    /// The quotes instrument ID.
    pub instrument_id: InstrumentId,
    /// The top-of-book bid price.
    pub bid_price: Price,
    /// The top-of-book ask price.
    pub ask_price: Price,
    /// The top-of-book bid size.
    pub bid_size: Quantity,
    /// The top-of-book ask size.
    pub ask_size: Quantity,
    /// UNIX timestamp (nanoseconds) when the quote event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl QuoteTick {
    /// Creates a new [`QuoteTick`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `bid_price.precision` does not equal `ask_price.precision`.
    /// - `bid_size.precision` does not equal `ask_size.precision`.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_equal_u8(
            bid_price.precision,
            ask_price.precision,
            "bid_price.precision",
            "ask_price.precision",
        )?;
        check_equal_u8(
            bid_size.precision,
            ask_size.precision,
            "bid_size.precision",
            "ask_size.precision",
        )?;
        Ok(Self {
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        })
    }

    /// Creates a new [`QuoteTick`] instance.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `bid_price.precision` does not equal `ask_price.precision`.
    /// - `bid_size.precision` does not equal `ask_size.precision`.
    pub fn new(
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new_checked(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
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
        metadata.insert("bid_price".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_price".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("bid_size".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ask_size".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }

    /// Returns the [`Price`] for this quote depending on the given `price_type`.
    ///
    /// # Panics
    ///
    /// Panics if an unsupported `price_type` is provided.
    #[must_use]
    pub fn extract_price(&self, price_type: PriceType) -> Price {
        match price_type {
            PriceType::Bid => self.bid_price,
            PriceType::Ask => self.ask_price,
            PriceType::Mid => Price::from_raw(
                (self.bid_price.raw + self.ask_price.raw) / 2,
                cmp::min(self.bid_price.precision + 1, FIXED_PRECISION),
            ),
            _ => panic!("Cannot extract with price type {price_type}"),
        }
    }

    /// Returns the [`Quantity`] for this quote depending on the given `price_type`.
    ///
    /// # Panics
    ///
    /// Panics if an unsupported `price_type` is provided.
    #[must_use]
    pub fn extract_size(&self, price_type: PriceType) -> Quantity {
        match price_type {
            PriceType::Bid => self.bid_size,
            PriceType::Ask => self.ask_size,
            PriceType::Mid => Quantity::from_raw(
                (self.bid_size.raw + self.ask_size.raw) / 2,
                cmp::min(self.bid_size.precision + 1, FIXED_PRECISION),
            ),
            _ => panic!("Cannot extract with price type {price_type}"),
        }
    }
}

impl Display for QuoteTick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id,
            self.bid_price,
            self.ask_price,
            self.bid_size,
            self.ask_size,
            self.ts_event,
        )
    }
}

impl Serializable for QuoteTick {}

impl HasTsInit for QuoteTick {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {

    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::QuoteTickBuilder;
    use crate::{
        data::{HasTsInit, QuoteTick, stubs::quote_ethusdt_binance},
        enums::PriceType,
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };

    fn create_test_quote() -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from("EURUSD.SIM"),
            Price::from("1.0500"),
            Price::from("1.0505"),
            Quantity::from("100000"),
            Quantity::from("75000"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        )
    }

    #[rstest]
    fn test_quote_tick_new() {
        let quote = create_test_quote();

        assert_eq!(quote.instrument_id, InstrumentId::from("EURUSD.SIM"));
        assert_eq!(quote.bid_price, Price::from("1.0500"));
        assert_eq!(quote.ask_price, Price::from("1.0505"));
        assert_eq!(quote.bid_size, Quantity::from("100000"));
        assert_eq!(quote.ask_size, Quantity::from("75000"));
        assert_eq!(quote.ts_event, UnixNanos::from(1_000_000_000));
        assert_eq!(quote.ts_init, UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_quote_tick_new_checked_valid() {
        let result = QuoteTick::new_checked(
            InstrumentId::from("GBPUSD.SIM"),
            Price::from("1.2500"),
            Price::from("1.2505"),
            Quantity::from("50000"),
            Quantity::from("60000"),
            UnixNanos::from(500_000_000),
            UnixNanos::from(1_500_000_000),
        );

        assert!(result.is_ok());
        let quote = result.unwrap();
        assert_eq!(quote.instrument_id, InstrumentId::from("GBPUSD.SIM"));
        assert_eq!(quote.bid_price, Price::from("1.2500"));
        assert_eq!(quote.ask_price, Price::from("1.2505"));
    }

    #[rstest]
    #[should_panic(
        expected = "'bid_price.precision' u8 of 4 was not equal to 'ask_price.precision' u8 of 5"
    )]
    fn test_quote_tick_new_with_precision_mismatch_panics() {
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
        let bid_price = Price::from("10000.0000"); // Precision: 4
        let ask_price = Price::from("10000.00100"); // Precision: 5 (mismatch)
        let bid_size = Quantity::from("1.000000");
        let ask_size = Quantity::from("1.000000");
        let ts_event = UnixNanos::from(0);
        let ts_init = UnixNanos::from(1);

        let _ = QuoteTick::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        );
    }

    #[rstest]
    fn test_quote_tick_new_checked_with_precision_mismatch_error() {
        let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
        let bid_price = Price::from("10000.0000");
        let ask_price = Price::from("10000.0010");
        let bid_size = Quantity::from("10.000000"); // Precision: 6
        let ask_size = Quantity::from("10.0000000"); // Precision: 7 (mismatch)
        let ts_event = UnixNanos::from(0);
        let ts_init = UnixNanos::from(1);

        let result = QuoteTick::new_checked(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(
            "'bid_size.precision' u8 of 6 was not equal to 'ask_size.precision' u8 of 7"
        ));
    }

    #[rstest]
    fn test_quote_tick_builder() {
        let quote = QuoteTickBuilder::default()
            .instrument_id(InstrumentId::from("BTCUSD.CRYPTO"))
            .bid_price(Price::from("50000.00"))
            .ask_price(Price::from("50001.00"))
            .bid_size(Quantity::from("0.50"))
            .ask_size(Quantity::from("0.75"))
            .ts_event(UnixNanos::from(3_000_000_000))
            .ts_init(UnixNanos::from(4_000_000_000))
            .build()
            .unwrap();

        assert_eq!(quote.instrument_id, InstrumentId::from("BTCUSD.CRYPTO"));
        assert_eq!(quote.bid_price, Price::from("50000.00"));
        assert_eq!(quote.ask_price, Price::from("50001.00"));
        assert_eq!(quote.bid_size, Quantity::from("0.50"));
        assert_eq!(quote.ask_size, Quantity::from("0.75"));
        assert_eq!(quote.ts_event, UnixNanos::from(3_000_000_000));
        assert_eq!(quote.ts_init, UnixNanos::from(4_000_000_000));
    }

    #[rstest]
    fn test_get_metadata() {
        let instrument_id = InstrumentId::from("EURUSD.SIM");
        let metadata = QuoteTick::get_metadata(&instrument_id, 5, 8);

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
        let fields = QuoteTick::get_fields();

        assert_eq!(fields.len(), 6);

        #[cfg(feature = "high-precision")]
        {
            assert_eq!(
                fields.get("bid_price"),
                Some(&"FixedSizeBinary(16)".to_string())
            );
            assert_eq!(
                fields.get("ask_price"),
                Some(&"FixedSizeBinary(16)".to_string())
            );
            assert_eq!(
                fields.get("bid_size"),
                Some(&"FixedSizeBinary(16)".to_string())
            );
            assert_eq!(
                fields.get("ask_size"),
                Some(&"FixedSizeBinary(16)".to_string())
            );
        }
        #[cfg(not(feature = "high-precision"))]
        {
            assert_eq!(
                fields.get("bid_price"),
                Some(&"FixedSizeBinary(8)".to_string())
            );
            assert_eq!(
                fields.get("ask_price"),
                Some(&"FixedSizeBinary(8)".to_string())
            );
            assert_eq!(
                fields.get("bid_size"),
                Some(&"FixedSizeBinary(8)".to_string())
            );
            assert_eq!(
                fields.get("ask_size"),
                Some(&"FixedSizeBinary(8)".to_string())
            );
        }

        assert_eq!(fields.get("ts_event"), Some(&"UInt64".to_string()));
        assert_eq!(fields.get("ts_init"), Some(&"UInt64".to_string()));
    }

    #[rstest]
    #[case(PriceType::Bid, Price::from("10000.0000"))]
    #[case(PriceType::Ask, Price::from("10001.0000"))]
    #[case(PriceType::Mid, Price::from("10000.5000"))]
    fn test_extract_price(
        #[case] input: PriceType,
        #[case] expected: Price,
        quote_ethusdt_binance: QuoteTick,
    ) {
        let quote = quote_ethusdt_binance;
        let result = quote.extract_price(input);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(PriceType::Bid, Quantity::from("1.00000000"))]
    #[case(PriceType::Ask, Quantity::from("1.00000000"))]
    #[case(PriceType::Mid, Quantity::from("1.00000000"))]
    fn test_extract_size(
        #[case] input: PriceType,
        #[case] expected: Quantity,
        quote_ethusdt_binance: QuoteTick,
    ) {
        let quote = quote_ethusdt_binance;
        let result = quote.extract_size(input);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[should_panic(expected = "Cannot extract with price type LAST")]
    fn test_extract_price_invalid_type() {
        let quote = create_test_quote();
        let _ = quote.extract_price(PriceType::Last);
    }

    #[rstest]
    #[should_panic(expected = "Cannot extract with price type LAST")]
    fn test_extract_size_invalid_type() {
        let quote = create_test_quote();
        let _ = quote.extract_size(PriceType::Last);
    }

    #[rstest]
    fn test_quote_tick_has_ts_init() {
        let quote = create_test_quote();
        assert_eq!(quote.ts_init(), UnixNanos::from(2_000_000_000));
    }

    #[rstest]
    fn test_quote_tick_display() {
        let quote = create_test_quote();
        let display_str = format!("{quote}");

        assert!(display_str.contains("EURUSD.SIM"));
        assert!(display_str.contains("1.0500"));
        assert!(display_str.contains("1.0505"));
        assert!(display_str.contains("100000"));
        assert!(display_str.contains("75000"));
        assert!(display_str.contains("1000000000"));
    }

    #[rstest]
    fn test_quote_tick_with_zero_prices() {
        let quote = QuoteTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("0.0000"),
            Price::from("0.0000"),
            Quantity::from("1000.0000"),
            Quantity::from("1000.0000"),
            UnixNanos::from(0),
            UnixNanos::from(0),
        );

        assert!(quote.bid_price.is_zero());
        assert!(quote.ask_price.is_zero());
        assert_eq!(quote.ts_event, UnixNanos::from(0));
        assert_eq!(quote.ts_init, UnixNanos::from(0));
    }

    #[rstest]
    fn test_quote_tick_with_max_values() {
        let quote = QuoteTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("999999.9999"),
            Price::from("999999.9999"),
            Quantity::from("999999999.9999"),
            Quantity::from("999999999.9999"),
            UnixNanos::from(u64::MAX),
            UnixNanos::from(u64::MAX),
        );

        assert_eq!(quote.ts_event, UnixNanos::from(u64::MAX));
        assert_eq!(quote.ts_init, UnixNanos::from(u64::MAX));
    }

    #[rstest]
    fn test_extract_mid_price_precision() {
        let quote = QuoteTick::new(
            InstrumentId::from("TEST.SIM"),
            Price::from("1.00"),
            Price::from("1.02"),
            Quantity::from("100.00"),
            Quantity::from("100.00"),
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(2_000_000_000),
        );

        let mid_price = quote.extract_price(PriceType::Mid);
        let mid_size = quote.extract_size(PriceType::Mid);

        assert_eq!(mid_price, Price::from("1.010"));
        assert_eq!(mid_size, Quantity::from("100.000"));
    }

    #[rstest]
    fn test_to_string(quote_ethusdt_binance: QuoteTick) {
        let quote = quote_ethusdt_binance;
        assert_eq!(
            quote.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,10001.0000,1.00000000,1.00000000,0"
        );
    }
}
