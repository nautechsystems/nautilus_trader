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

//! Defines instrument definitions for the trading domain models.

pub mod crypto_future;
pub mod crypto_perpetual;
pub mod currency_pair;
pub mod equity;
pub mod futures_contract;
pub mod futures_spread;
pub mod options_contract;
pub mod options_spread;
pub mod synthetic;

#[cfg(feature = "stubs")]
pub mod stubs;

use nautilus_core::nanos::UnixNanos;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use ustr::Ustr;

use self::{
    crypto_future::CryptoFuture, crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair,
    equity::Equity, futures_contract::FuturesContract, futures_spread::FuturesSpread,
    options_contract::OptionsContract, options_spread::OptionsSpread,
};
use crate::{
    enums::{AssetClass, InstrumentClass, OptionKind},
    identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue},
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};

#[derive(Clone, Debug)]
pub enum InstrumentAny {
    CryptoFuture(CryptoFuture),
    CryptoPerpetual(CryptoPerpetual),
    CurrencyPair(CurrencyPair),
    Equity(Equity),
    FuturesContract(FuturesContract),
    FuturesSpread(FuturesSpread),
    OptionsContract(OptionsContract),
    OptionsSpread(OptionsSpread),
}

impl InstrumentAny {
    #[must_use]
    pub fn id(&self) -> InstrumentId {
        match self {
            Self::CryptoFuture(inst) => inst.id,
            Self::CryptoPerpetual(inst) => inst.id,
            Self::CurrencyPair(inst) => inst.id,
            Self::Equity(inst) => inst.id,
            Self::FuturesContract(inst) => inst.id,
            Self::FuturesSpread(inst) => inst.id,
            Self::OptionsContract(inst) => inst.id,
            Self::OptionsSpread(inst) => inst.id,
        }
    }

    #[must_use]
    pub fn base_currency(&self) -> Option<Currency> {
        match self {
            Self::CryptoFuture(inst) => inst.base_currency(),
            Self::CryptoPerpetual(inst) => inst.base_currency(),
            Self::CurrencyPair(inst) => inst.base_currency(),
            Self::Equity(inst) => inst.base_currency(),
            Self::FuturesContract(inst) => inst.base_currency(),
            Self::FuturesSpread(inst) => inst.base_currency(),
            Self::OptionsContract(inst) => inst.base_currency(),
            Self::OptionsSpread(inst) => inst.base_currency(),
        }
    }

    #[must_use]
    pub fn quote_currency(&self) -> Currency {
        match self {
            Self::CryptoFuture(inst) => inst.quote_currency(),
            Self::CryptoPerpetual(inst) => inst.quote_currency(),
            Self::CurrencyPair(inst) => inst.quote_currency(),
            Self::Equity(inst) => inst.quote_currency(),
            Self::FuturesContract(inst) => inst.quote_currency(),
            Self::FuturesSpread(inst) => inst.quote_currency(),
            Self::OptionsContract(inst) => inst.quote_currency(),
            Self::OptionsSpread(inst) => inst.quote_currency(),
        }
    }

    #[must_use]
    pub fn settlement_currency(&self) -> Currency {
        match self {
            Self::CryptoFuture(inst) => inst.settlement_currency(),
            Self::CryptoPerpetual(inst) => inst.settlement_currency(),
            Self::CurrencyPair(inst) => inst.settlement_currency(),
            Self::Equity(inst) => inst.settlement_currency(),
            Self::FuturesContract(inst) => inst.settlement_currency(),
            Self::FuturesSpread(inst) => inst.settlement_currency(),
            Self::OptionsContract(inst) => inst.settlement_currency(),
            Self::OptionsSpread(inst) => inst.settlement_currency(),
        }
    }

    #[must_use]
    pub fn is_inverse(&self) -> bool {
        match self {
            Self::CryptoFuture(inst) => inst.is_inverse(),
            Self::CryptoPerpetual(inst) => inst.is_inverse(),
            Self::CurrencyPair(inst) => inst.is_inverse(),
            Self::Equity(inst) => inst.is_inverse(),
            Self::FuturesContract(inst) => inst.is_inverse(),
            Self::FuturesSpread(inst) => inst.is_inverse(),
            Self::OptionsContract(inst) => inst.is_inverse(),
            Self::OptionsSpread(inst) => inst.is_inverse(),
        }
    }

    #[must_use]
    pub fn price_precision(&self) -> u8 {
        match self {
            Self::CryptoFuture(inst) => inst.price_precision(),
            Self::CryptoPerpetual(inst) => inst.price_precision(),
            Self::CurrencyPair(inst) => inst.price_precision(),
            Self::Equity(inst) => inst.price_precision(),
            Self::FuturesContract(inst) => inst.price_precision(),
            Self::FuturesSpread(inst) => inst.price_precision(),
            Self::OptionsContract(inst) => inst.price_precision(),
            Self::OptionsSpread(inst) => inst.price_precision(),
        }
    }

    #[must_use]
    pub fn size_precision(&self) -> u8 {
        match self {
            Self::CryptoFuture(inst) => inst.size_precision(),
            Self::CryptoPerpetual(inst) => inst.size_precision(),
            Self::CurrencyPair(inst) => inst.size_precision(),
            Self::Equity(inst) => inst.size_precision(),
            Self::FuturesContract(inst) => inst.size_precision(),
            Self::FuturesSpread(inst) => inst.size_precision(),
            Self::OptionsContract(inst) => inst.size_precision(),
            Self::OptionsSpread(inst) => inst.size_precision(),
        }
    }

    #[must_use]
    pub fn price_increment(&self) -> Price {
        match self {
            Self::CryptoFuture(inst) => inst.price_increment(),
            Self::CryptoPerpetual(inst) => inst.price_increment(),
            Self::CurrencyPair(inst) => inst.price_increment(),
            Self::Equity(inst) => inst.price_increment(),
            Self::FuturesContract(inst) => inst.price_increment(),
            Self::FuturesSpread(inst) => inst.price_increment(),
            Self::OptionsContract(inst) => inst.price_increment(),
            Self::OptionsSpread(inst) => inst.price_increment(),
        }
    }

    #[must_use]
    pub fn size_increment(&self) -> Quantity {
        match self {
            Self::CryptoFuture(inst) => inst.size_increment(),
            Self::CryptoPerpetual(inst) => inst.size_increment(),
            Self::CurrencyPair(inst) => inst.size_increment(),
            Self::Equity(inst) => inst.size_increment(),
            Self::FuturesContract(inst) => inst.size_increment(),
            Self::FuturesSpread(inst) => inst.size_increment(),
            Self::OptionsContract(inst) => inst.size_increment(),
            Self::OptionsSpread(inst) => inst.size_increment(),
        }
    }

    pub fn make_price(&self, value: f64) -> anyhow::Result<Price> {
        match self {
            Self::CryptoFuture(inst) => inst.make_price(value),
            Self::CryptoPerpetual(inst) => inst.make_price(value),
            Self::CurrencyPair(inst) => inst.make_price(value),
            Self::Equity(inst) => inst.make_price(value),
            Self::FuturesContract(inst) => inst.make_price(value),
            Self::FuturesSpread(inst) => inst.make_price(value),
            Self::OptionsContract(inst) => inst.make_price(value),
            Self::OptionsSpread(inst) => inst.make_price(value),
        }
    }

    pub fn make_qty(&self, value: f64) -> anyhow::Result<Quantity> {
        match self {
            Self::CryptoFuture(inst) => inst.make_qty(value),
            Self::CryptoPerpetual(inst) => inst.make_qty(value),
            Self::CurrencyPair(inst) => inst.make_qty(value),
            Self::Equity(inst) => inst.make_qty(value),
            Self::FuturesContract(inst) => inst.make_qty(value),
            Self::FuturesSpread(inst) => inst.make_qty(value),
            Self::OptionsContract(inst) => inst.make_qty(value),
            Self::OptionsSpread(inst) => inst.make_qty(value),
        }
    }

    #[must_use]
    pub fn calculate_notional_value(
        &self,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> Money {
        match self {
            Self::CryptoFuture(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::CryptoPerpetual(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::CurrencyPair(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::Equity(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::FuturesContract(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::FuturesSpread(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::OptionsContract(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::OptionsSpread(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
        }
    }

    // #[deprecated(since = "0.21.0", note = "Will be removed in a future version")]
    #[must_use]
    pub fn maker_fee(&self) -> Decimal {
        match self {
            Self::CryptoFuture(inst) => inst.maker_fee(),
            Self::CryptoPerpetual(inst) => inst.maker_fee(),
            Self::CurrencyPair(inst) => inst.maker_fee(),
            Self::Equity(inst) => inst.maker_fee(),
            Self::FuturesContract(inst) => inst.maker_fee(),
            Self::FuturesSpread(inst) => inst.maker_fee(),
            Self::OptionsContract(inst) => inst.maker_fee(),
            Self::OptionsSpread(inst) => inst.maker_fee(),
        }
    }

    // #[deprecated(since = "0.21.0", note = "Will be removed in a future version")]
    #[must_use]
    pub fn taker_fee(&self) -> Decimal {
        match self {
            Self::CryptoFuture(inst) => inst.taker_fee(),
            Self::CryptoPerpetual(inst) => inst.taker_fee(),
            Self::CurrencyPair(inst) => inst.taker_fee(),
            Self::Equity(inst) => inst.taker_fee(),
            Self::FuturesContract(inst) => inst.taker_fee(),
            Self::FuturesSpread(inst) => inst.taker_fee(),
            Self::OptionsContract(inst) => inst.taker_fee(),
            Self::OptionsSpread(inst) => inst.taker_fee(),
        }
    }
}

impl PartialEq for InstrumentAny {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

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
    fn isin(&self) -> Option<Ustr>;
    fn option_kind(&self) -> Option<OptionKind>;
    fn exchange(&self) -> Option<Ustr>;
    fn strike_price(&self) -> Option<Price>;
    fn activation_ns(&self) -> Option<UnixNanos>;
    fn expiration_ns(&self) -> Option<UnixNanos>;
    fn is_inverse(&self) -> bool;
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

    /// Creates a new `Price` from the given `value` with the correct price precision for the instrument.
    fn make_price(&self, value: f64) -> anyhow::Result<Price> {
        Price::new(value, self.price_precision())
    }

    /// Creates a new `Quantity` from the given `value` with the correct size precision for the instrument.
    fn make_qty(&self, value: f64) -> anyhow::Result<Quantity> {
        Quantity::new(value, self.size_precision())
    }

    /// Calculates the notional value from the given parameters.
    /// The `use_quote_for_inverse` flag is only applicable for inverse instruments.
    ///
    /// # Panics
    ///
    /// If instrument is inverse and not `use_quote_for_inverse`, with no base currency.
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

        Money::new(amount, currency).unwrap() // TODO: Handle error properly
    }

    /// Returns the equivalent quantity of the base asset.
    fn calculate_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        let value = quantity.as_f64() * (1.0 / last_px.as_f64());
        Quantity::new(value, self.size_precision()).unwrap() // TODO: Handle error properly
    }
}
