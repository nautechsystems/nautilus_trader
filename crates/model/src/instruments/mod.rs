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

use std::{fmt::Display, str::FromStr};

use enum_dispatch::enum_dispatch;
use nautilus_core::{
    UnixNanos,
    correctness::{check_equal_u8, check_positive_decimal, check_predicate_true},
};
use rust_decimal::{Decimal, RoundingStrategy, prelude::*};
use rust_decimal_macros::dec;
use ustr::Ustr;

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
    types::{
        Currency, Money, Price, Quantity, money::check_positive_money, price::check_positive_price,
        quantity::check_positive_quantity,
    },
};

#[allow(clippy::missing_errors_doc, clippy::too_many_arguments)]
pub fn validate_instrument_common(
    price_precision: u8,
    size_precision: u8,
    size_increment: Quantity,
    multiplier: Quantity,
    margin_init: Decimal,
    margin_maint: Decimal,
    price_increment: Option<Price>,
    lot_size: Option<Quantity>,
    max_quantity: Option<Quantity>,
    min_quantity: Option<Quantity>,
    max_notional: Option<Money>,
    min_notional: Option<Money>,
    max_price: Option<Price>,
    min_price: Option<Price>,
) -> anyhow::Result<()> {
    check_positive_quantity(size_increment, "size_increment")?;
    check_equal_u8(
        size_increment.precision,
        size_precision,
        "size_increment.precision",
        "size_precision",
    )?;
    check_positive_quantity(multiplier, "multiplier")?;
    check_positive_decimal(margin_init, "margin_init")?;
    check_positive_decimal(margin_maint, "margin_maint")?;

    if let Some(price_increment) = price_increment {
        check_positive_price(price_increment, "price_increment")?;
        check_equal_u8(
            price_increment.precision,
            price_precision,
            "price_increment.precision",
            "price_precision",
        )?;
    }

    if let Some(lot) = lot_size {
        check_positive_quantity(lot, "lot_size")?;
    }

    if let Some(quantity) = max_quantity {
        check_positive_quantity(quantity, "max_quantity")?;
    }

    if let Some(quantity) = min_quantity {
        check_positive_quantity(quantity, "max_quantity")?;
    }

    if let Some(notional) = max_notional {
        check_positive_money(notional, "max_notional")?;
    }

    if let Some(notional) = min_notional {
        check_positive_money(notional, "min_notional")?;
    }

    if let Some(max_price) = max_price {
        check_positive_price(max_price, "max_price")?;
        check_equal_u8(
            max_price.precision,
            price_precision,
            "max_price.precision",
            "price_precision",
        )?;
    }
    if let Some(min_price) = min_price {
        check_positive_price(min_price, "min_price")?;
        check_equal_u8(
            min_price.precision,
            price_precision,
            "min_price.precision",
            "price_precision",
        )?;
    }

    if let (Some(min), Some(max)) = (min_price, max_price) {
        check_predicate_true(min.raw <= max.raw, "min_price exceeds max_price")?;
    }

    Ok(())
}

pub trait TickSchemeRule: Display {
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price>;
    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price>;
}

#[derive(Clone, Copy, Debug)]
pub struct FixedTickScheme {
    tick: f64,
}

impl PartialEq for FixedTickScheme {
    fn eq(&self, other: &Self) -> bool {
        self.tick == other.tick
    }
}
impl Eq for FixedTickScheme {}

impl FixedTickScheme {
    #[allow(clippy::missing_errors_doc)]
    pub fn new(tick: f64) -> anyhow::Result<Self> {
        check_predicate_true(tick > 0.0, "tick must be positive")?;
        Ok(Self { tick })
    }
}

impl TickSchemeRule for FixedTickScheme {
    #[inline(always)]
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        let base = (value / self.tick).floor() * self.tick;
        Some(Price::new(base - (n as f64) * self.tick, precision))
    }

    #[inline(always)]
    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        let base = (value / self.tick).ceil() * self.tick;
        Some(Price::new(base + (n as f64) * self.tick, precision))
    }
}

impl Display for FixedTickScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FIXED")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickScheme {
    Fixed(FixedTickScheme),
    Crypto,
}

impl TickSchemeRule for TickScheme {
    #[inline(always)]
    fn next_bid_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        match self {
            Self::Fixed(scheme) => scheme.next_bid_price(value, n, precision),
            Self::Crypto => {
                let increment: f64 = 0.01;
                let base = (value / increment).floor() * increment;
                Some(Price::new(base - (n as f64) * increment, precision))
            }
        }
    }

    #[inline(always)]
    fn next_ask_price(&self, value: f64, n: i32, precision: u8) -> Option<Price> {
        match self {
            Self::Fixed(scheme) => scheme.next_ask_price(value, n, precision),
            Self::Crypto => {
                let increment: f64 = 0.01;
                let base = (value / increment).ceil() * increment;
                Some(Price::new(base + (n as f64) * increment, precision))
            }
        }
    }
}

impl Display for TickScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fixed(_) => write!(f, "FIXED"),
            Self::Crypto => write!(f, "CRYPTO_0_01"),
        }
    }
}

impl FromStr for TickScheme {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_uppercase().as_str() {
            "FIXED" => Ok(Self::Fixed(FixedTickScheme::new(1.0)?)),
            "CRYPTO_0_01" => Ok(Self::Crypto),
            _ => anyhow::bail!("unknown tick scheme {s}"),
        }
    }
}

#[enum_dispatch]
pub trait Instrument: 'static + Send {
    fn tick_scheme(&self) -> Option<&dyn TickSchemeRule> {
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

    /// # Panics
    ///
    /// Panics if the instrument is inverse and does not have a base currency.
    fn cost_currency(&self) -> Currency {
        if self.is_inverse() {
            self.base_currency()
                .expect("inverse instrument without base_currency")
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
            .is_some_and(|currency| currency != self.settlement_currency())
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

    fn _min_price_increment_precision(&self) -> u8 {
        self.price_increment().precision
    }

    /// # Errors
    ///
    /// Returns an error if the value is not finite or cannot be converted to a `Price`.
    #[inline(always)]
    fn try_make_price(&self, value: f64) -> anyhow::Result<Price> {
        check_predicate_true(value.is_finite(), "non-finite value passed to make_price")?;
        let precision = self
            .price_precision()
            .min(self._min_price_increment_precision()) as u32;
        let decimal_value = Decimal::from_f64_retain(value)
            .ok_or_else(|| anyhow::anyhow!("non-finite value passed to make_price"))?;
        let rounded_decimal =
            decimal_value.round_dp_with_strategy(precision, RoundingStrategy::MidpointNearestEven);
        let rounded = rounded_decimal
            .to_f64()
            .ok_or_else(|| anyhow::anyhow!("Decimal out of f64 range in make_price"))?;
        Ok(Price::new(rounded, self.price_precision()))
    }

    fn make_price(&self, value: f64) -> Price {
        self.try_make_price(value).unwrap()
    }

    /// # Errors
    ///
    /// Returns an error if the value is not finite or cannot be converted to a `Quantity`.
    #[inline(always)]
    fn try_make_qty(&self, value: f64, round_down: Option<bool>) -> anyhow::Result<Quantity> {
        let precision_u8 = self.size_precision();
        let precision = precision_u8 as u32;
        let decimal_value = Decimal::from_f64_retain(value)
            .ok_or_else(|| anyhow::anyhow!("non-finite value passed to make_qty"))?;
        let rounded_decimal = if round_down.unwrap_or(false) {
            decimal_value.round_dp_with_strategy(precision, RoundingStrategy::ToZero)
        } else {
            decimal_value.round_dp_with_strategy(precision, RoundingStrategy::MidpointNearestEven)
        };
        let rounded = rounded_decimal
            .to_f64()
            .ok_or_else(|| anyhow::anyhow!("Decimal out of f64 range in make_qty"))?;
        let increment = 10f64.powi(-(precision_u8 as i32));
        if value > 0.0 && rounded < increment * 0.1 {
            anyhow::bail!("value rounded to zero for quantity");
        }
        Ok(Quantity::new(rounded, precision_u8))
    }

    fn make_qty(&self, value: f64, round_down: Option<bool>) -> Quantity {
        self.try_make_qty(value, round_down).unwrap()
    }

    /// # Errors
    ///
    /// Returns an error if the quantity or price is not finite or cannot be converted to a `Quantity`.
    fn try_calculate_base_quantity(
        &self,
        quantity: Quantity,
        last_price: Price,
    ) -> anyhow::Result<Quantity> {
        check_predicate_true(
            quantity.as_f64().is_finite(),
            "non-finite quantity passed to calculate_base_quantity",
        )?;
        check_predicate_true(
            last_price.as_f64().is_finite(),
            "non-finite price passed to calculate_base_quantity",
        )?;
        let quantity_decimal = Decimal::from_f64_retain(quantity.as_f64()).ok_or_else(|| {
            anyhow::anyhow!("non-finite quantity passed to calculate_base_quantity")
        })?;
        let price_decimal = Decimal::from_f64_retain(last_price.as_f64())
            .ok_or_else(|| anyhow::anyhow!("non-finite price passed to calculate_base_quantity"))?;
        let value_decimal = (quantity_decimal / price_decimal).round_dp_with_strategy(
            self.size_precision().into(),
            RoundingStrategy::MidpointNearestEven,
        );
        let rounded = value_decimal.to_f64().ok_or_else(|| {
            anyhow::anyhow!("Decimal out of f64 range in calculate_base_quantity")
        })?;
        Ok(Quantity::new(rounded, self.size_precision()))
    }

    fn calculate_base_quantity(&self, quantity: Quantity, last_price: Price) -> Quantity {
        self.try_calculate_base_quantity(quantity, last_price)
            .unwrap()
    }

    /// # Panics
    ///
    /// Panics if the instrument is inverse and does not have a base currency.
    #[inline(always)]
    fn calculate_notional_value(
        &self,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> Money {
        let use_quote_inverse = use_quote_for_inverse.unwrap_or(false);
        if self.is_inverse() {
            if use_quote_inverse {
                Money::new(quantity.as_f64(), self.quote_currency())
            } else {
                let amount =
                    quantity.as_f64() * self.multiplier().as_f64() * (1.0 / price.as_f64());
                let currency = self
                    .base_currency()
                    .expect("inverse instrument without base_currency");
                Money::new(amount, currency)
            }
        } else if self.is_quanto() {
            let amount = quantity.as_f64() * self.multiplier().as_f64() * price.as_f64();
            Money::new(amount, self.settlement_currency())
        } else {
            let amount = quantity.as_f64() * self.multiplier().as_f64() * price.as_f64();
            Money::new(amount, self.quote_currency())
        }
    }

    #[inline(always)]
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

    #[inline(always)]
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

    #[inline]
    fn next_bid_prices(&self, value: f64, n: usize) -> Vec<Price> {
        let mut prices = Vec::with_capacity(n);
        for i in 0..n {
            if let Some(price) = self.next_bid_price(value, i as i32) {
                prices.push(price);
            } else {
                break;
            }
        }
        prices
    }

    #[inline]
    fn next_ask_prices(&self, value: f64, n: usize) -> Vec<Price> {
        let mut prices = Vec::with_capacity(n);
        for i in 0..n {
            if let Some(price) = self.next_ask_price(value, i as i32) {
                prices.push(price);
            } else {
                break;
            }
        }
        prices
    }
}

impl Display for CurrencyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id='{}', tick_scheme='{}', price_precision={}, size_precision={}, \
price_increment={}, size_increment={}, multiplier={}, margin_init={}, margin_maint={})",
            stringify!(CurrencyPair),
            self.id,
            self.tick_scheme()
                .map_or_else(|| "None".into(), |s| s.to_string()),
            self.price_precision(),
            self.size_precision(),
            self.price_increment(),
            self.size_increment(),
            self.multiplier(),
            self.margin_init(),
            self.margin_maint(),
        )
    }
}

pub const EXPIRING_INSTRUMENT_TYPES: [InstrumentClass; 4] = [
    InstrumentClass::Future,
    InstrumentClass::FuturesSpread,
    InstrumentClass::Option,
    InstrumentClass::OptionSpread,
];

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use proptest::prelude::*;
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::{instruments::stubs::*, types::Money};

    pub fn default_price_increment(precision: u8) -> Price {
        let step = 10f64.powi(-(precision as i32));
        Price::new(step, precision)
    }

    #[rstest]
    fn default_increment_precision() {
        let inc = default_price_increment(2);
        assert_eq!(inc, Price::new(0.01, 2));
    }

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
        let start = 10_000.123_4;
        let bid_0 = currency_pair_btcusdt.next_bid_price(start, 0).unwrap();
        let bid_1 = currency_pair_btcusdt.next_bid_price(start, 1).unwrap();
        assert!(bid_1 < bid_0);
        let asks = currency_pair_btcusdt.next_ask_prices(start, 3);
        assert_eq!(asks.len(), 3);
        assert!(asks[0] > bid_0);
    }

    #[rstest]
    #[should_panic]
    fn validate_negative_margin_init() {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);

        validate_instrument_common(
            2,
            2,              // size_precision
            size_increment, // size_increment
            multiplier,     // multiplier
            dec!(-0.01),    // margin_init
            dec!(0.01),     // margin_maint
            None,           // price_increment
            None,           // lot_size
            None,           // max_quantity
            None,           // min_quantity
            None,           // max_notional
            None,           // min_notional
            None,           // max_price
            None,           // min_price
        )
        .unwrap();
    }

    #[rstest]
    #[should_panic]
    fn validate_negative_margin_maint() {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);

        validate_instrument_common(
            2,
            2,              // size_precision
            size_increment, // size_increment
            multiplier,     // multiplier
            dec!(0.01),     // margin_init
            dec!(-0.01),    // margin_maint
            None,           // price_increment
            None,           // lot_size
            None,           // max_quantity
            None,           // min_quantity
            None,           // max_notional
            None,           // min_notional
            None,           // max_price
            None,           // min_price
        )
        .unwrap();
    }

    #[rstest]
    #[should_panic]
    fn validate_negative_max_qty() {
        let quantity = Quantity::new(0.0, 0);
        validate_instrument_common(
            2,
            2,
            Quantity::new(0.01, 2),
            Quantity::new(1.0, 0),
            dec!(0),
            dec!(0),
            None,
            None,
            Some(quantity),
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
            size_increment,
            multiplier,
            dec!(0),
            dec!(0),
            Some(price_increment),
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
            size_increment,
            multiplier,
            dec!(0),
            dec!(0),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(max_price),
            Some(min_price),
        )
        .unwrap();
    }

    #[rstest]
    fn validate_instrument_common_ok() {
        let res = validate_instrument_common(
            2,
            4,
            Quantity::new(0.0001, 4),
            Quantity::new(1.0, 0),
            dec!(0.02),
            dec!(0.01),
            Some(Price::new(0.01, 2)),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert!(matches!(res, Ok(())));
    }

    #[rstest]
    #[should_panic]
    fn validate_multiple_errors() {
        validate_instrument_common(
            2,
            2,
            Quantity::new(-0.01, 2),
            Quantity::new(0.0, 0),
            dec!(0),
            dec!(0),
            None,
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
    #[case(1.234_999_9, false, "1.235000")]
    #[case(1.234_999_9, true, "1.234999")]
    fn make_qty_boundary(
        currency_pair_btcusdt: CurrencyPair,
        #[case] input: f64,
        #[case] round_down: bool,
        #[case] expected: &str,
    ) {
        let quantity = currency_pair_btcusdt.make_qty(input, Some(round_down));
        assert_eq!(quantity.to_string(), expected);
    }

    #[rstest]
    fn fixed_tick_multiple_steps() {
        let scheme = FixedTickScheme::new(1.0).unwrap();
        let bid = scheme.next_bid_price(10.0, 2, 1).unwrap();
        let ask = scheme.next_ask_price(10.0, 3, 1).unwrap();
        assert_eq!(bid, Price::new(8.0, 1));
        assert_eq!(ask, Price::new(13.0, 1));
    }

    #[rstest]
    #[case(1.234_999, 1.23)]
    #[case(1.235, 1.24)]
    #[case(1.235_001, 1.24)]
    fn make_price_rounding_parity(
        currency_pair_btcusdt: CurrencyPair,
        #[case] input: f64,
        #[case] expected: f64,
    ) {
        let price = currency_pair_btcusdt.make_price(input);
        assert!((price.as_f64() - expected).abs() < 1e-9);
    }

    #[rstest]
    fn make_price_half_even_parity(currency_pair_btcusdt: CurrencyPair) {
        let rounding_precision = std::cmp::min(
            currency_pair_btcusdt.price_precision(),
            currency_pair_btcusdt._min_price_increment_precision(),
        );
        let step = 10f64.powi(-(rounding_precision as i32));
        let base_even_multiple = 42.0;
        let base_value = step * base_even_multiple;
        let delta = step / 2000.0;
        let value_below = base_value + 0.5 * step - delta;
        let value_exact = base_value + 0.5 * step;
        let value_above = base_value + 0.5 * step + delta;
        let price_below = currency_pair_btcusdt.make_price(value_below);
        let price_exact = currency_pair_btcusdt.make_price(value_exact);
        let price_above = currency_pair_btcusdt.make_price(value_above);
        assert_eq!(price_below, price_exact);
        assert_ne!(price_exact, price_above);
    }

    #[rstest]
    fn tick_scheme_round_trip() {
        let scheme = TickScheme::from_str("CRYPTO_0_01").unwrap();
        assert_eq!(scheme.to_string(), "CRYPTO_0_01");
    }

    #[rstest]
    fn is_quanto_flag(ethbtc_quanto: CryptoFuture) {
        assert!(ethbtc_quanto.is_quanto());
    }

    #[rstest]
    fn notional_quanto(ethbtc_quanto: CryptoFuture) {
        let quantity = ethbtc_quanto.make_qty(5.0, None);
        let price = ethbtc_quanto.make_price(0.036);
        let notional = ethbtc_quanto.calculate_notional_value(quantity, price, None);
        let expected = Money::new(0.18, ethbtc_quanto.settlement_currency());
        assert_eq!(notional, expected);
    }

    #[rstest]
    fn notional_inverse_base(xbtusd_inverse_perp: CryptoPerpetual) {
        let quantity = xbtusd_inverse_perp.make_qty(100.0, None);
        let price = xbtusd_inverse_perp.make_price(50_000.0);
        let notional = xbtusd_inverse_perp.calculate_notional_value(quantity, price, Some(false));
        let expected = Money::new(
            100.0 * xbtusd_inverse_perp.multiplier().as_f64() * (1.0 / 50_000.0),
            xbtusd_inverse_perp.base_currency().unwrap(),
        );
        assert_eq!(notional, expected);
    }

    #[rstest]
    fn notional_inverse_quote_use_quote(xbtusd_inverse_perp: CryptoPerpetual) {
        let quantity = xbtusd_inverse_perp.make_qty(100.0, None);
        let price = xbtusd_inverse_perp.make_price(50_000.0);
        let notional = xbtusd_inverse_perp.calculate_notional_value(quantity, price, Some(true));
        let expected = Money::new(100.0, xbtusd_inverse_perp.quote_currency());
        assert_eq!(notional, expected);
    }

    #[rstest]
    #[should_panic]
    fn validate_non_positive_max_price() {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);
        let max_price = Price::new(0.0, 2);
        validate_instrument_common(
            2,
            2,
            size_increment,
            multiplier,
            dec!(0),
            dec!(0),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(max_price),
            None,
        )
        .unwrap();
    }

    #[rstest]
    #[should_panic]
    fn validate_non_positive_max_notional(currency_pair_btcusdt: CurrencyPair) {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);
        let max_notional = Money::new(0.0, currency_pair_btcusdt.quote_currency());
        validate_instrument_common(
            2,
            2,
            size_increment,
            multiplier,
            dec!(0),
            dec!(0),
            None,
            None,
            None,
            None,
            Some(max_notional),
            None,
            None,
            None,
        )
        .unwrap();
    }

    #[rstest]
    #[should_panic]
    fn validate_price_increment_min_price_precision_mismatch() {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);
        let price_increment = Price::new(0.01, 2);
        let min_price = Price::new(1.0, 3);
        validate_instrument_common(
            2,
            2,
            size_increment,
            multiplier,
            dec!(0),
            dec!(0),
            Some(price_increment),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(min_price),
        )
        .unwrap();
    }

    #[rstest]
    #[should_panic]
    fn validate_negative_min_notional(currency_pair_btcusdt: CurrencyPair) {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);
        let min_notional = Money::new(-1.0, currency_pair_btcusdt.quote_currency());
        let max_notional = Money::new(1.0, currency_pair_btcusdt.quote_currency());
        validate_instrument_common(
            2,
            2,
            size_increment,
            multiplier,
            dec!(0),
            dec!(0),
            None,
            None,
            None,
            None,
            Some(max_notional),
            Some(min_notional),
            None,
            None,
        )
        .unwrap();
    }

    #[rstest]
    #[case::dp0(Decimal::new(1_000, 0), Decimal::new(2, 0), 500.0)]
    #[case::dp1(Decimal::new(10_000, 1), Decimal::new(2, 0), 500.0)]
    #[case::dp2(Decimal::new(100_000, 2), Decimal::new(2, 0), 500.0)]
    #[case::dp3(Decimal::new(1_000_000, 3), Decimal::new(2, 0), 500.0)]
    #[case::dp4(Decimal::new(10_000_000, 4), Decimal::new(2, 0), 500.0)]
    #[case::dp5(Decimal::new(100_000_000, 5), Decimal::new(2, 0), 500.0)]
    #[case::dp6(Decimal::new(1_000_000_000, 6), Decimal::new(2, 0), 500.0)]
    #[case::dp7(Decimal::new(10_000_000_000, 7), Decimal::new(2, 0), 500.0)]
    #[case::dp8(Decimal::new(100_000_000_000, 8), Decimal::new(2, 0), 500.0)]
    fn base_qty_rounding(
        currency_pair_btcusdt: CurrencyPair,
        #[case] q: Decimal,
        #[case] px: Decimal,
        #[case] expected: f64,
    ) {
        let qty = Quantity::new(q.to_f64().unwrap(), 8);
        let price = Price::new(px.to_f64().unwrap(), 8);
        let base = currency_pair_btcusdt.calculate_base_quantity(qty, price);
        assert!((base.as_f64() - expected).abs() < 1e-9);
    }

    proptest! {
        #[rstest]
        fn make_price_qty_fuzz(input in 0.0001f64..1e8) {
            let instrument = currency_pair_btcusdt();
            let price = instrument.make_price(input);
            prop_assert!(price.as_f64().is_finite());
            let quantity = instrument.make_qty(input, None);
            prop_assert!(quantity.as_f64().is_finite());
        }
    }

    #[rstest]
    fn tick_walk_limits_btcusdt_ask(currency_pair_btcusdt: CurrencyPair) {
        if let Some(max_price) = currency_pair_btcusdt.max_price() {
            assert!(
                currency_pair_btcusdt
                    .next_ask_price(max_price.as_f64(), 1)
                    .is_none()
            );
        }
    }

    #[rstest]
    fn tick_walk_limits_ethusdt_ask(currency_pair_ethusdt: CurrencyPair) {
        if let Some(max_price) = currency_pair_ethusdt.max_price() {
            assert!(
                currency_pair_ethusdt
                    .next_ask_price(max_price.as_f64(), 1)
                    .is_none()
            );
        }
    }

    #[rstest]
    fn tick_walk_limits_btcusdt_bid(currency_pair_btcusdt: CurrencyPair) {
        if let Some(min_price) = currency_pair_btcusdt.min_price() {
            assert!(
                currency_pair_btcusdt
                    .next_bid_price(min_price.as_f64(), 1)
                    .is_none()
            );
        }
    }

    #[rstest]
    fn tick_walk_limits_ethusdt_bid(currency_pair_ethusdt: CurrencyPair) {
        if let Some(min_price) = currency_pair_ethusdt.min_price() {
            assert!(
                currency_pair_ethusdt
                    .next_bid_price(min_price.as_f64(), 1)
                    .is_none()
            );
        }
    }

    #[rstest]
    fn tick_walk_limits_quanto_ask(ethbtc_quanto: CryptoFuture) {
        if let Some(max_price) = ethbtc_quanto.max_price() {
            assert!(
                ethbtc_quanto
                    .next_ask_price(max_price.as_f64(), 1)
                    .is_none()
            );
        }
    }

    #[rstest]
    #[case(0.999_999, false)]
    #[case(0.999_999, true)]
    #[case(1.000_000_1, false)]
    #[case(1.000_000_1, true)]
    #[case(1.234_5, false)]
    #[case(1.234_5, true)]
    #[case(2.345_5, false)]
    #[case(2.345_5, true)]
    #[case(0.000_999_999, false)]
    #[case(0.000_999_999, true)]
    fn quantity_rounding_grid(
        currency_pair_btcusdt: CurrencyPair,
        #[case] input: f64,
        #[case] round_down: bool,
    ) {
        let qty = currency_pair_btcusdt.make_qty(input, Some(round_down));
        assert!(qty.as_f64().is_finite());
    }

    #[rstest]
    fn pyo3_failure_tick_scheme_unknown() {
        assert!(TickScheme::from_str("UNKNOWN").is_err());
    }

    #[rstest]
    fn pyo3_failure_fixed_tick_zero() {
        assert!(FixedTickScheme::new(0.0).is_err());
    }

    #[rstest]
    fn pyo3_failure_validate_price_increment_max_price_precision_mismatch() {
        let size_increment = Quantity::new(0.01, 2);
        let multiplier = Quantity::new(1.0, 0);
        let price_increment = Price::new(0.01, 2);
        let max_price = Price::new(1.0, 3);
        let res = validate_instrument_common(
            2,
            2,
            size_increment,
            multiplier,
            dec!(0),
            dec!(0),
            Some(price_increment),
            None,
            None,
            None,
            None,
            None,
            Some(max_price),
            None,
        );
        assert!(res.is_err());
    }

    #[rstest]
    #[case::dp9(Decimal::new(1_000_000_000_000, 9), Decimal::new(2, 0), 500.0)]
    #[case::dp10(Decimal::new(10_000_000_000_000, 10), Decimal::new(2, 0), 500.0)]
    #[case::dp11(Decimal::new(100_000_000_000_000, 11), Decimal::new(2, 0), 500.0)]
    #[case::dp12(Decimal::new(1_000_000_000_000_000, 12), Decimal::new(2, 0), 500.0)]
    #[case::dp13(Decimal::new(10_000_000_000_000_000, 13), Decimal::new(2, 0), 500.0)]
    #[case::dp14(Decimal::new(100_000_000_000_000_000, 14), Decimal::new(2, 0), 500.0)]
    #[case::dp15(Decimal::new(1_000_000_000_000_000_000, 15), Decimal::new(2, 0), 500.0)]
    #[case::dp16(
        Decimal::from_i128_with_scale(10_000_000_000_000_000_000i128, 16),
        Decimal::new(2, 0),
        500.0
    )]
    #[case::dp17(
        Decimal::from_i128_with_scale(100_000_000_000_000_000_000i128, 17),
        Decimal::new(2, 0),
        500.0
    )]
    fn base_qty_rounding_high_dp(
        currency_pair_btcusdt: CurrencyPair,
        #[case] q: Decimal,
        #[case] px: Decimal,
        #[case] expected: f64,
    ) {
        let qty = Quantity::new(q.to_f64().unwrap(), 8);
        let price = Price::new(px.to_f64().unwrap(), 8);
        let base = currency_pair_btcusdt.calculate_base_quantity(qty, price);
        assert!((base.as_f64() - expected).abs() < 1e-9);
    }

    #[rstest]
    fn check_positive_money_ok(currency_pair_btcusdt: CurrencyPair) {
        let money = Money::new(100.0, currency_pair_btcusdt.quote_currency());
        assert!(check_positive_money(money, "money").is_ok());
    }

    #[rstest]
    #[should_panic]
    fn check_positive_money_zero(currency_pair_btcusdt: CurrencyPair) {
        let money = Money::new(0.0, currency_pair_btcusdt.quote_currency());
        check_positive_money(money, "money").unwrap();
    }

    #[rstest]
    #[should_panic]
    fn check_positive_money_negative(currency_pair_btcusdt: CurrencyPair) {
        let money = Money::new(-0.01, currency_pair_btcusdt.quote_currency());
        check_positive_money(money, "money").unwrap();
    }
}
