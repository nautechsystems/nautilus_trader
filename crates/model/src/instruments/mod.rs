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

#[cfg(any(test, feature = "stubs"))]
pub mod stubs;

use anyhow::ensure;
use enum_dispatch::enum_dispatch;
use nautilus_core::UnixNanos;
use rust_decimal::{Decimal, RoundingStrategy, prelude::*};
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

macro_rules! check_positive {
    ($val:expr, $msg:expr) => {
        ensure!($val > 0.0, $msg);
    };
}

#[allow(clippy::missing_errors_doc, clippy::too_many_arguments)]
pub fn validate_instrument_common(
    price_precision: i32,
    size_precision: i32,
    size_increment: &Quantity,
    multiplier: &Quantity,
    margin_init: Decimal,
    margin_maint: Decimal,
    price_increment: Option<&Price>,
    lot_size: Option<&Quantity>,
    max_quantity: Option<&Quantity>,
    min_quantity: Option<&Quantity>,
    max_notional: Option<&Money>,
    min_notional: Option<&Money>,
    max_price: Option<&Price>,
    min_price: Option<&Price>,
) -> anyhow::Result<()> {
    ensure!(price_precision >= 0, "price_precision negative");
    ensure!(size_precision >= 0, "size_precision negative");
    check_positive!(size_increment.as_f64(), "size_increment not positive");
    ensure!(
        size_increment.precision as i32 == size_precision,
        "size_precision != size_increment.precision"
    );
    check_positive!(multiplier.as_f64(), "multiplier not positive");
    ensure!(margin_init >= dec!(0), "margin_init negative");
    ensure!(margin_maint >= dec!(0), "margin_maint negative");

    if let Some(increment) = price_increment {
        check_positive!(increment.as_f64(), "price_increment not positive");
        ensure!(
            increment.precision as i32 == price_precision,
            "price_precision != price_increment.precision"
        );
    }
    if let Some(lot) = lot_size {
        check_positive!(lot.as_f64(), "lot_size not positive");
    }
    if let Some(quantity) = max_quantity {
        check_positive!(quantity.as_f64(), "max_quantity not positive");
    }
    if let Some(quantity) = min_quantity {
        ensure!(quantity.as_f64() >= 0.0, "min_quantity negative");
    }
    if let Some(notional) = max_notional {
        check_positive!(notional.as_f64(), "max_notional not positive");
    }
    if let Some(notional) = min_notional {
        ensure!(notional.as_f64() >= 0.0, "min_notional negative");
    }
    if let Some(price) = max_price {
        ensure!(
            price.precision as i32 == price_precision,
            "price_precision != max_price.precision"
        );
    }
    if let Some(price) = min_price {
        ensure!(
            price.precision as i32 == price_precision,
            "price_precision != min_price.precision"
        );
    }
    if let (Some(min), Some(max)) = (min_price, max_price) {
        ensure!(min.as_f64() <= max.as_f64(), "min_price exceeds max_price");
    }
    Ok(())
}

pub trait TickScheme {
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price>;
    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price>;
}

#[derive(Debug)]
pub struct FixedTickScheme {
    tick: f64,
}

impl FixedTickScheme {
    #[allow(clippy::missing_errors_doc)]
    pub fn new(tick: f64) -> anyhow::Result<Self> {
        ensure!(tick > 0.0, "tick must be positive");
        Ok(Self { tick })
    }
}

impl TickScheme for FixedTickScheme {
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        let base = (value / self.tick).floor() * self.tick;
        Some(Price::new(base - (n as f64) * self.tick, precision))
    }

    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        let base = (value / self.tick).ceil() * self.tick;
        Some(Price::new(base + (n as f64) * self.tick, precision))
    }
}

#[enum_dispatch]
pub trait Instrument: 'static + Send {
    fn tick_scheme(&self) -> Option<&FixedTickScheme> {
        None
    }

    fn into_any(self) -> InstrumentAny
    where
        Self: Sized,
        InstrumentAny: From<Self>,
    {
        self.into()
    }

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
                .expect("inverse instrument without base currency")
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
        self.base_currency()
            .map(|currency| currency != self.settlement_currency())
            .unwrap_or(false)
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
        dec!(0)
    }
    fn margin_maint(&self) -> Decimal {
        dec!(0)
    }
    fn maker_fee(&self) -> Decimal {
        dec!(0)
    }
    fn taker_fee(&self) -> Decimal {
        dec!(0)
    }

    fn ts_event(&self) -> UnixNanos;
    fn ts_init(&self) -> UnixNanos;

    /// Creates a new [`Price`] from the given `value` with the correct price precision for the instrument.
    fn make_price(&self, value: f64) -> Price {
        let factor = 10f64.powi(self.price_increment().precision as i32);
        let rounded = (value * factor).round() / factor;
        Price::new(rounded, self.price_precision())
    }

    /// Creates a new [`Quantity`] from the given `value` with the correct size precision for the instrument.
    fn make_qty(&self, value: f64, round_down: Option<bool>) -> Quantity {
        let precision_u8 = self.size_precision();
        let precision = precision_u8 as u32;
        let decimal_value = Decimal::from_f64_retain(value).expect("non-finite");
        let rounded_decimal = if round_down.unwrap_or(false) {
            decimal_value.round_dp_with_strategy(precision, RoundingStrategy::ToZero)
        } else {
            decimal_value.round_dp_with_strategy(precision, RoundingStrategy::MidpointNearestEven)
        };
        let rounded = rounded_decimal.to_f64().expect("out of range");
        let increment = 10f64.powi(-(precision_u8 as i32));
        if value > 0.0 && rounded < increment * 0.1 {
            panic!("value rounded to zero for quantity");
        }
        Quantity::new(rounded, precision_u8)
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
        let use_quote = use_quote_for_inverse.unwrap_or(false);
        if self.is_inverse() {
            if use_quote {
                Money::new(quantity.as_f64(), self.quote_currency())
            } else {
                let amount =
                    quantity.as_f64() * self.multiplier().as_f64() * (1.0 / price.as_f64());
                let currency = self
                    .base_currency()
                    .expect("inverse instrument without base currency");
                Money::new(amount, currency)
            }
        } else {
            let amount = quantity.as_f64() * self.multiplier().as_f64() * price.as_f64();
            Money::new(amount, self.quote_currency())
        }
    }

    /// Returns the equivalent quantity of the base asset.
    fn calculate_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        Quantity::new(quantity.as_f64() / last_px.as_f64(), self.size_precision())
    }

    fn next_bid_price(&self, value: f64, n: i32) -> Option<Price> {
        let price = if let Some(scheme) = self.tick_scheme() {
            scheme.next_bid_price(value, n, self.price_precision())?
        } else {
            let increment = self.price_increment().as_f64().abs();
            if increment == 0.0 {
                return None;
            }
            let base = (value / increment).floor() * increment;
            Price::new(base - (n as f64) * increment, self.price_precision())
        };
        if self.min_price().is_some_and(|min| price < min)
            || self.max_price().is_some_and(|max| price > max)
        {
            return None;
        }
        Some(price)
    }

    fn next_ask_price(&self, value: f64, n: i32) -> Option<Price> {
        let price = if let Some(scheme) = self.tick_scheme() {
            scheme.next_ask_price(value, n, self.price_precision())?
        } else {
            let increment = self.price_increment().as_f64().abs();
            if increment == 0.0 {
                return None;
            }
            let base = (value / increment).ceil() * increment;
            Price::new(base + (n as f64) * increment, self.price_precision())
        };
        if self.min_price().is_some_and(|min| price < min)
            || self.max_price().is_some_and(|max| price > max)
        {
            return None;
        }
        Some(price)
    }

    fn next_bid_prices(&self, value: f64, n: usize) -> Vec<Price> {
        (0..n)
            .filter_map(|i| self.next_bid_price(value, i as i32))
            .collect()
    }

    fn next_ask_prices(&self, value: f64, n: usize) -> Vec<Price> {
        (0..n)
            .filter_map(|i| self.next_ask_price(value, i as i32))
            .collect()
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

    use super::*;
    use crate::{instruments::stubs::*, types::Money};

    #[rstest]
    #[case(1.5, "1.500000")]
    #[case(2.5, "2.500000")]
    #[case(1.2345678, "1.234568")]
    #[case(0.000123, "0.000123")]
    #[case(99999.999999, "99999.999999")]
    fn make_qty_rounding(
        currency_pair_btcusdt: CurrencyPair,
        #[case] input: f64,
        #[case] expected: &str,
    ) {
        assert_eq!(
            currency_pair_btcusdt.make_qty(input, None).to_string(),
            expected
        );
    }

    #[rstest]
    #[case(1.2345678, "1.234567")]
    #[case(1.9999999, "1.999999")]
    #[case(0.00012345, "0.000123")]
    #[case(10.9999999, "10.999999")]
    fn make_qty_round_down(
        currency_pair_btcusdt: CurrencyPair,
        #[case] input: f64,
        #[case] expected: &str,
    ) {
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(input, Some(true))
                .to_string(),
            expected
        );
    }

    #[rstest]
    #[case(1.2345678, "1.23457")]
    #[case(2.3456781, "2.34568")]
    #[case(0.00001, "0.00001")]
    fn make_qty_precision(
        currency_pair_ethusdt: CurrencyPair,
        #[case] input: f64,
        #[case] expected: &str,
    ) {
        assert_eq!(
            currency_pair_ethusdt.make_qty(input, None).to_string(),
            expected
        );
    }

    #[rstest]
    #[case(1.2345675, "1.234568")]
    #[case(1.2345665, "1.234566")]
    fn make_qty_half_even(
        currency_pair_btcusdt: CurrencyPair,
        #[case] input: f64,
        #[case] expected: &str,
    ) {
        assert_eq!(
            currency_pair_btcusdt.make_qty(input, None).to_string(),
            expected
        );
    }

    #[rstest]
    #[should_panic]
    fn make_qty_rounds_to_zero(currency_pair_btcusdt: CurrencyPair) {
        currency_pair_btcusdt.make_qty(1e-12, None);
    }

    #[rstest]
    fn notional_linear(currency_pair_btcusdt: CurrencyPair) {
        let quantity = currency_pair_btcusdt.make_qty(2.0, None);
        let price = currency_pair_btcusdt.make_price(10_000.0);
        let notional = currency_pair_btcusdt.calculate_notional_value(quantity, price, None);
        let expected = Money::new(20_000.0, currency_pair_btcusdt.quote_currency());
        assert_eq!(notional, expected);
    }

    #[rstest]
    fn tick_navigation(currency_pair_btcusdt: CurrencyPair) {
        let start = 10_000.1234;
        let bid_0 = currency_pair_btcusdt.next_bid_price(start, 0).unwrap();
        let bid_1 = currency_pair_btcusdt.next_bid_price(start, 1).unwrap();
        assert!(bid_1 < bid_0);
        let asks = currency_pair_btcusdt.next_ask_prices(start, 3);
        assert_eq!(asks.len(), 3);
        assert!(asks[0] > bid_0);
    }

    #[rstest]
    #[should_panic]
    fn validate_negative_max_qty() {
        let quantity = Quantity::new(0.0, 0);
        validate_instrument_common(
            2,
            2,
            &Quantity::new(0.01, 2),
            &Quantity::new(1.0, 0),
            dec!(0),
            dec!(0),
            None,
            None,
            Some(&quantity),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    }

    #[rstest]
    fn make_price_negative_rounding(currency_pair_ethusdt: CurrencyPair) {
        let price = currency_pair_ethusdt.make_price(-123.456_789);
        assert!(price.as_f64() < 0.0);
    }

    #[rstest]
    fn base_quantity_linear(currency_pair_btcusdt: CurrencyPair) {
        let quantity = currency_pair_btcusdt.make_qty(2.0, None);
        let price = currency_pair_btcusdt.make_price(10_000.0);
        let base = currency_pair_btcusdt.calculate_base_quantity(quantity, price);
        assert_eq!(base.to_string(), "0.000200");
    }

    #[rstest]
    fn fixed_tick_scheme_prices() {
        let scheme = FixedTickScheme::new(0.5).unwrap();
        let bid = scheme.next_bid_price(10.3, 0, 2).unwrap();
        let ask = scheme.next_ask_price(10.3, 0, 2).unwrap();
        assert!(bid < ask);
    }

    #[rstest]
    #[should_panic]
    fn fixed_tick_negative() {
        FixedTickScheme::new(-0.01).unwrap();
    }

    #[rstest]
    fn next_bid_prices_sequence(currency_pair_btcusdt: CurrencyPair) {
        let start = 10_000.0;
        let bids = currency_pair_btcusdt.next_bid_prices(start, 5);
        assert_eq!(bids.len(), 5);
        for i in 1..bids.len() {
            assert!(bids[i] < bids[i - 1]);
        }
    }

    #[rstest]
    fn next_ask_prices_sequence(currency_pair_btcusdt: CurrencyPair) {
        let start = 10_000.0;
        let asks = currency_pair_btcusdt.next_ask_prices(start, 5);
        assert_eq!(asks.len(), 5);
        for i in 1..asks.len() {
            assert!(asks[i] > asks[i - 1]);
        }
    }

    #[rstest]
    fn fixed_tick_boundary() {
        let scheme = FixedTickScheme::new(0.5).unwrap();
        let price = scheme.next_bid_price(10.5, 0, 2).unwrap();
        assert_eq!(price, Price::new(10.5, 2));
    }

    #[rstest]
    #[should_panic]
    fn validate_price_increment_precision_mismatch() {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);
        let price_increment = Price::new(0.001, 3);
        validate_instrument_common(
            2,
            2,
            &size_increment,
            &multiplier,
            dec!(0),
            dec!(0),
            Some(&price_increment),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    }

    #[rstest]
    #[should_panic]
    fn validate_min_price_exceeds_max_price() {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);
        let min_price = Price::new(10.0, 2);
        let max_price = Price::new(5.0, 2);
        validate_instrument_common(
            2,
            2,
            &size_increment,
            &multiplier,
            dec!(0),
            dec!(0),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&max_price),
            Some(&min_price),
        )
        .unwrap();
    }
}
