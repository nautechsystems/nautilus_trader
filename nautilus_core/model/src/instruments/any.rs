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

use nautilus_core::nanos::UnixNanos;
use rust_decimal::Decimal;

use super::{
    crypto_future::CryptoFuture, crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair,
    equity::Equity, futures_contract::FuturesContract, futures_spread::FuturesSpread,
    options_contract::OptionsContract, options_spread::OptionsSpread, Instrument,
};
use crate::{
    enums::InstrumentClass,
    identifiers::InstrumentId,
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
    /// Consumes the `OrderAny` enum and returns the underlying order as a boxed trait.
    #[must_use]
    pub fn into_instrument(self) -> Box<dyn Instrument> {
        match self {
            Self::CryptoFuture(inst) => Box::new(inst),
            Self::CryptoPerpetual(inst) => Box::new(inst),
            Self::CurrencyPair(inst) => Box::new(inst),
            Self::Equity(inst) => Box::new(inst),
            Self::FuturesContract(inst) => Box::new(inst),
            Self::FuturesSpread(inst) => Box::new(inst),
            Self::OptionsContract(inst) => Box::new(inst),
            Self::OptionsSpread(inst) => Box::new(inst),
        }
    }

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

    #[must_use]
    pub fn multiplier(&self) -> Quantity {
        match self {
            Self::CryptoFuture(inst) => inst.multiplier(),
            Self::CryptoPerpetual(inst) => inst.multiplier(),
            Self::CurrencyPair(inst) => inst.multiplier(),
            Self::Equity(inst) => inst.multiplier(),
            Self::FuturesContract(inst) => inst.multiplier(),
            Self::FuturesSpread(inst) => inst.multiplier(),
            Self::OptionsContract(inst) => inst.multiplier(),
            Self::OptionsSpread(inst) => inst.multiplier(),
        }
    }

    #[must_use]
    pub fn instrument_class(&self) -> InstrumentClass {
        match self {
            Self::CryptoFuture(inst) => inst.instrument_class(),
            Self::CryptoPerpetual(inst) => inst.instrument_class(),
            Self::CurrencyPair(inst) => inst.instrument_class(),
            Self::Equity(inst) => inst.instrument_class(),
            Self::FuturesContract(inst) => inst.instrument_class(),
            Self::FuturesSpread(inst) => inst.instrument_class(),
            Self::OptionsContract(inst) => inst.instrument_class(),
            Self::OptionsSpread(inst) => inst.instrument_class(),
        }
    }

    #[must_use]
    pub fn activation_ns(&self) -> Option<UnixNanos> {
        match self {
            Self::CryptoFuture(inst) => inst.activation_ns(),
            Self::CryptoPerpetual(inst) => inst.activation_ns(),
            Self::CurrencyPair(inst) => inst.activation_ns(),
            Self::Equity(inst) => inst.activation_ns(),
            Self::FuturesContract(inst) => inst.activation_ns(),
            Self::FuturesSpread(inst) => inst.activation_ns(),
            Self::OptionsContract(inst) => inst.activation_ns(),
            Self::OptionsSpread(inst) => inst.activation_ns(),
        }
    }

    #[must_use]
    pub fn expiration_ns(&self) -> Option<UnixNanos> {
        match self {
            Self::CryptoFuture(inst) => inst.expiration_ns(),
            Self::CryptoPerpetual(inst) => inst.expiration_ns(),
            Self::CurrencyPair(inst) => inst.expiration_ns(),
            Self::Equity(inst) => inst.expiration_ns(),
            Self::FuturesContract(inst) => inst.expiration_ns(),
            Self::FuturesSpread(inst) => inst.expiration_ns(),
            Self::OptionsContract(inst) => inst.expiration_ns(),
            Self::OptionsSpread(inst) => inst.expiration_ns(),
        }
    }

    pub fn make_price(&self, value: f64) -> Price {
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

    pub fn make_qty(&self, value: f64) -> Quantity {
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
