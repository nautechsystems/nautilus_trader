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

use anyhow::ensure;
use enum_dispatch::enum_dispatch;
use nautilus_core::UnixNanos;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{Value, json};
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

#[allow(clippy::missing_errors_doc)]
#[allow(clippy::too_many_arguments)]
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

    if let Some(pi) = price_increment {
        check_positive!(pi.as_f64(), "price_increment not positive");
        ensure!(
            pi.precision as i32 == price_precision,
            "price_precision != price_increment.precision"
        );
    }
    if let Some(l) = lot_size {
        check_positive!(l.as_f64(), "lot_size not positive");
    }
    if let Some(q) = max_quantity {
        check_positive!(q.as_f64(), "max_quantity not positive");
    }
    if let Some(q) = min_quantity {
        ensure!(q.as_f64() >= 0.0, "min_quantity negative");
    }
    if let Some(n) = max_notional {
        check_positive!(n.as_f64(), "max_notional not positive");
    }
    if let Some(n) = min_notional {
        ensure!(n.as_f64() >= 0.0, "min_notional negative");
    }
    if let Some(p) = max_price {
        check_positive!(p.as_f64(), "max_price not positive");
    }
    if let Some(p) = min_price {
        ensure!(p.as_f64() >= 0.0, "min_price negative");
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
            .map(|c| c != self.settlement_currency())
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
        let inc = 10f64.powi(-(self.size_precision() as i32));
        let rounded = if round_down.unwrap_or(false) {
            (value / inc).floor() * inc
        } else {
            let factor = 10f64.powi(self.size_precision() as i32);
            let tmp = value * factor;
            let floored = tmp.floor();
            let rem = tmp - floored;
            let half_even = if rem > 0.5 {
                floored + 1.0
            } else if rem < 0.5 || (rem == 0.5 && (floored as u64) % 2 == 0) {
                floored
            } else {
                floored + 1.0
            };
            half_even / factor
        };
        if value > 0.0 && rounded < inc * 0.1 {
            panic!("value rounded to zero for quantity");
        }
        Quantity::new(rounded, self.size_precision())
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
                let amt = quantity.as_f64() * self.multiplier().as_f64() * (1.0 / price.as_f64());
                let ccy = self
                    .base_currency()
                    .expect("inverse instrument without base currency");
                Money::new(amt, ccy)
            }
        } else {
            let amt = quantity.as_f64() * self.multiplier().as_f64() * price.as_f64();
            Money::new(amt, self.quote_currency())
        }
    }

    /// Returns the equivalent quantity of the base asset.
    fn calculate_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        Quantity::new(quantity.as_f64() / last_px.as_f64(), self.size_precision())
    }

    fn next_bid_price(&self, value: f64, n: i32) -> Option<Price> {
        let px = if let Some(scheme) = self.tick_scheme() {
            scheme.next_bid_price(value, n, self.price_precision())?
        } else {
            let inc = self.price_increment().as_f64();
            if inc <= 0.0 {
                return None;
            }
            let base = (value / inc).floor() * inc;
            Price::new(base - (n as f64) * inc, self.price_precision())
        };
        if self.min_price().is_some_and(|min| px < min)
            || self.max_price().is_some_and(|max| px > max)
        {
            return None;
        }
        Some(px)
    }

    fn next_ask_price(&self, value: f64, n: i32) -> Option<Price> {
        let px = if let Some(scheme) = self.tick_scheme() {
            scheme.next_ask_price(value, n, self.price_precision())?
        } else {
            let inc = self.price_increment().as_f64();
            if inc <= 0.0 {
                return None;
            }
            let base = (value / inc).ceil() * inc;
            Price::new(base + (n as f64) * inc, self.price_precision())
        };
        if self.min_price().is_some_and(|min| px < min)
            || self.max_price().is_some_and(|max| px > max)
        {
            return None;
        }
        Some(px)
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

    fn to_dict(&self) -> Value {
        json!({
            "id": self.id().to_string(),
            "raw_symbol": self.raw_symbol().to_string(),
            "venue": self.venue().to_string(),
            "asset_class": self.asset_class() as u8,
            "instrument_class": self.instrument_class() as u8,
            "price_precision": self.price_precision(),
            "size_precision": self.size_precision(),
        })
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
    fn make_qty_rounding(currency_pair_btcusdt: CurrencyPair) {
        assert_eq!(
            currency_pair_btcusdt.make_qty(1.5, None).to_string(),
            "1.500000"
        );
        assert_eq!(
            currency_pair_btcusdt.make_qty(2.5, None).to_string(),
            "2.500000"
        );
        assert_eq!(
            currency_pair_btcusdt.make_qty(1.2345678, None).to_string(),
            "1.234568"
        );
    }

    #[rstest]
    fn make_qty_round_down(currency_pair_btcusdt: CurrencyPair) {
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(1.2345678, Some(true))
                .to_string(),
            "1.234567"
        );
        assert_eq!(
            currency_pair_btcusdt
                .make_qty(1.9999999, Some(true))
                .to_string(),
            "1.999999"
        );
    }

    #[rstest]
    fn make_qty_precision(currency_pair_ethusdt: CurrencyPair) {
        assert_eq!(
            currency_pair_ethusdt.make_qty(1.2345678, None).to_string(),
            "1.23457"
        );
        assert_eq!(
            currency_pair_ethusdt
                .make_qty(1.2345678, Some(true))
                .to_string(),
            "1.23456"
        );
    }

    #[rstest]
    #[should_panic]
    fn make_qty_rounds_to_zero(currency_pair_btcusdt: CurrencyPair) {
        currency_pair_btcusdt.make_qty(1e-12, None);
    }

    #[rstest]
    fn notional_linear(currency_pair_btcusdt: CurrencyPair) {
        let qty = currency_pair_btcusdt.make_qty(2.0, None);
        let px = currency_pair_btcusdt.make_price(10_000.0);
        let notional = currency_pair_btcusdt.calculate_notional_value(qty, px, None);
        let expected = Money::new(20_000.0, currency_pair_btcusdt.quote_currency());
        assert_eq!(notional, expected);
    }

    #[rstest]
    fn tick_navigation(currency_pair_btcusdt: CurrencyPair) {
        let start = 10_000.1234;
        let bid0 = currency_pair_btcusdt.next_bid_price(start, 0).unwrap();
        let bid1 = currency_pair_btcusdt.next_bid_price(start, 1).unwrap();
        assert!(bid1 < bid0);
        let asks = currency_pair_btcusdt.next_ask_prices(start, 3);
        assert_eq!(asks.len(), 3);
        assert!(asks[0] > bid0);
    }

    #[rstest]
    fn json_roundtrip(currency_pair_btcusdt: CurrencyPair) {
        let dict = currency_pair_btcusdt.to_dict();
        let id_in_json = dict.get("id").unwrap().as_str().unwrap();
        assert_eq!(id_in_json, currency_pair_btcusdt.id().to_string());
    }

    #[rstest]
    #[should_panic]
    fn validate_negative_max_qty() {
        let qty = Quantity::new(0.0, 0);
        validate_instrument_common(
            2,
            2,
            &Quantity::new(0.01, 2),
            &Quantity::new(1.0, 0),
            dec!(0),
            dec!(0),
            None,
            None,
            Some(&qty),
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    }
}
