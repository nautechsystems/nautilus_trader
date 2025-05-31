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

//! Instrument definitions for the trading domain model.

pub mod any;
pub mod betting;
pub mod binary_option;
pub mod crypto_future;
pub mod crypto_option;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod futures_spread;
pub mod option_contract;
pub mod option_spread;
pub mod synthetic;

#[cfg(feature = "stubs")]
pub mod stubs;

use enum_dispatch::enum_dispatch;
use nautilus_core::UnixNanos;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

// Re-exports
pub use crate::instruments::{
    any::InstrumentAny, betting::BettingInstrument, binary_option::BinaryOption,
    crypto_future::CryptoFuture, crypto_option::CryptoOption, crypto_perpetual::CryptoPerpetual,
    currency_pair::CurrencyPair, equity::Equity, futures_contract::FuturesContract,
    futures_spread::FuturesSpread, option_contract::OptionContract, option_spread::OptionSpread,
    synthetic::SyntheticInstrument,
};
use crate::{
    enums::{AssetClass, InstrumentClass, OptionKind},
    identifiers::{InstrumentId, Symbol, Venue},
    types::{Currency, Money, Price, Quantity},
};

#[enum_dispatch]
pub trait Instrument: 'static + Send {
    fn into_any(self) -> InstrumentAny;
    fn id(&self) -> InstrumentId;
    fn symbol(&self) -> Symbol {
        self.id().symbol
    }
    fn venue(&self) -> Venue {
        self.id().venue
    }
    fn raw_symbol(&self) -> Symbol;
    fn asset_class(&self) -> AssetClass;
    fn instrument_class(&self) -> InstrumentClass;
    fn underlying(&self) -> Option<Ustr>;
    fn base_currency(&self) -> Option<Currency>;
    fn quote_currency(&self) -> Currency;
    fn settlement_currency(&self) -> Currency;
    fn cost_currency(&self) -> Currency {
        if self.is_inverse() {
            self.base_currency()
                .expect("Inverse instruments must have a base currency")
        } else {
            self.quote_currency()
        }
    }
    fn isin(&self) -> Option<Ustr>;
    fn option_kind(&self) -> Option<OptionKind>;
    fn exchange(&self) -> Option<Ustr>;
    fn strike_price(&self) -> Option<Price>;
    fn activation_ns(&self) -> Option<UnixNanos>;
    fn expiration_ns(&self) -> Option<UnixNanos>;
    fn is_inverse(&self) -> bool;
    fn is_quanto(&self) -> bool {
        if let Some(base_currency) = self.base_currency() {
            self.settlement_currency() != base_currency
        } else {
            false
        }
    }
    fn price_precision(&self) -> u8;
    fn size_precision(&self) -> u8;
    fn price_increment(&self) -> Price;
    fn size_increment(&self) -> Quantity;
    fn multiplier(&self) -> Quantity;
    fn lot_size(&self) -> Option<Quantity>;
    fn max_quantity(&self) -> Option<Quantity>;
    fn min_quantity(&self) -> Option<Quantity>;
    fn max_notional(&self) -> Option<Money>;
    fn min_notional(&self) -> Option<Money>;
    fn max_price(&self) -> Option<Price>;
    fn min_price(&self) -> Option<Price>;
    fn margin_init(&self) -> Decimal {
        dec!(0) // Temporary until separate fee models
    }

    fn margin_maint(&self) -> Decimal {
        dec!(0) // Temporary until separate fee models
    }

    fn maker_fee(&self) -> Decimal {
        dec!(0) // Temporary until separate fee models
    }

    fn taker_fee(&self) -> Decimal {
        dec!(0) // Temporary until separate fee models
    }
    fn ts_event(&self) -> UnixNanos;
    fn ts_init(&self) -> UnixNanos;

    /// Creates a new [`Price`] from the given `value` with the correct price precision for the instrument.
    fn make_price(&self, value: f64) -> Price {
        Price::new(value, self.price_precision())
    }

    /// Creates a new [`Quantity`] from the given `value` with the correct size precision for the instrument.
    fn make_qty(&self, value: f64, round_down: Option<bool>) -> Quantity {
        if round_down.unwrap_or(false) {
            // Round down to the nearest valid increment
            let increment = 10f64.powi(-i32::from(self.size_precision()));
            let rounded_value = (value / increment).floor() * increment;
            Quantity::new(rounded_value, self.size_precision())
        } else {
            // Use standard rounding behavior (banker's rounding)
            Quantity::new(value, self.size_precision())
        }
    }

    /// Calculates the notional value from the given parameters.
    /// The `use_quote_for_inverse` flag is only applicable for inverse instruments.
    ///
    /// # Panics
    ///
    /// This function panics if instrument is inverse and not `use_quote_for_inverse`, with no base currency.
    fn calculate_notional_value(
        &self,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> Money {
        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        let (amount, currency) = if self.is_inverse() {
            if use_quote_for_inverse {
                (quantity.as_f64(), self.quote_currency())
            } else {
                let amount =
                    quantity.as_f64() * self.multiplier().as_f64() * (1.0 / price.as_f64());
                let currency = self
                    .base_currency()
                    .expect("Error: no base currency for notional calculation");
                (amount, currency)
            }
        } else {
            let amount = quantity.as_f64() * self.multiplier().as_f64() * price.as_f64();
            let currency = self.quote_currency();
            (amount, currency)
        };

        Money::new(amount, currency)
    }

    /// Returns the equivalent quantity of the base asset.
    fn calculate_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        let value = quantity.as_f64() * (1.0 / last_px.as_f64());
        Quantity::new(value, self.size_precision())
    }
}

pub const EXPIRING_INSTRUMENT_TYPES: [InstrumentClass; 4] = [
    InstrumentClass::Future,
    InstrumentClass::FuturesSpread,
    InstrumentClass::Option,
    InstrumentClass::OptionSpread,
];

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::instruments::{CurrencyPair, Instrument, stubs::*};

    #[rstest]
    fn test_make_qty_standard_rounding(currency_pair_btcusdt: CurrencyPair) {
        assert_eq!(
            currency_pair_btcusdt.make_qty(1.5, None).to_string(),
            "1.500000"
        ); // 1.5 -> 1.500000
        assert_eq!(
            currency_pair_btcusdt.make_qty(2.5, None).to_string(),
            "2.500000"
        ); // 2.5 -> 2.500000 (banker's rounds to even)
        assert_eq!(
            currency_pair_btcusdt.make_qty(1.2345678, None).to_string(),
            "1.234568"
        ); // 1.2345678 -> 1.234568 (rounds to precision)
    }

    #[rstest]
    fn test_make_qty_round_down(currency_pair_btcusdt: CurrencyPair) {
        assert_eq!(
            currency_pair_btcusdt.make_qty(1.5, Some(true)).to_string(),
            "1.500000"
        ); // 1.5 -> 1.500000
        assert_eq!(
            currency_pair_btcusdt.make_qty(2.5, Some(true)).to_string(),
            "2.500000"
        ); // 2.5 -> 2.500000
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(1.2345678, Some(true))
                .to_string(),
            "1.234567"
        ); // 1.2345678 -> 1.234567 (rounds down)
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(1.9999999, Some(true))
                .to_string(),
            "1.999999"
        ); // 1.9999999 -> 1.999999 (rounds down)
    }

    #[rstest]
    fn test_make_qty_boundary_cases(currency_pair_btcusdt: CurrencyPair) {
        // The instrument has size_precision=6, so increment = 0.000001
        let increment = 0.000001;

        // Testing behavior near increment boundaries
        let value_just_above = 1.0 + (increment * 1.1);
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(value_just_above, Some(true))
                .to_string(),
            "1.000001"
        ); // Should round down to 1.000001
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(value_just_above, None)
                .to_string(),
            "1.000001"
        ); // Standard rounding should be 1.000001

        // Test with a value that should differ between round modes
        let value_half_increment = 1.0000015;
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(value_half_increment, Some(true))
                .to_string(),
            "1.000001"
        ); // Should round down to 1.000001
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(value_half_increment, None)
                .to_string(),
            "1.000002"
        ); // Standard rounding should be 1.000002
    }

    #[rstest]
    fn test_make_qty_zero_value(currency_pair_btcusdt: CurrencyPair) {
        // Zero should remain zero with both rounding methods
        assert_eq!(
            currency_pair_btcusdt.make_qty(0.0, None).to_string(),
            "0.000000"
        );
        assert_eq!(
            currency_pair_btcusdt.make_qty(0.0, Some(true)).to_string(),
            "0.000000"
        );
    }

    #[rstest]
    fn test_make_qty_different_precision(currency_pair_ethusdt: CurrencyPair) {
        // ethusdt has size_precision=5
        assert_eq!(
            currency_pair_ethusdt.make_qty(1.2345678, None).to_string(),
            "1.23457"
        ); // 1.2345678 -> 1.23457 (standard rounding)
        assert_eq!(
            currency_pair_ethusdt
                .make_qty(1.2345678, Some(true))
                .to_string(),
            "1.23456"
        ); // 1.2345678 -> 1.23456 (rounds down)
    }
}
