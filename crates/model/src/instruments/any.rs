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

use nautilus_core::UnixNanos;
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    betting::BettingInstrument, binary_option::BinaryOption, crypto_future::CryptoFuture,
    crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair, equity::Equity,
    futures_contract::FuturesContract, futures_spread::FuturesSpread,
    option_contract::OptionContract, option_spread::OptionSpread, Instrument,
};
use crate::{
    enums::InstrumentClass,
    identifiers::{InstrumentId, Symbol, Venue},
    types::{Currency, Money, Price, Quantity},
};

#[derive(Clone, Debug)]
pub enum InstrumentAny {
    Betting(BettingInstrument),
    BinaryOption(BinaryOption),
    CryptoFuture(CryptoFuture),
    CryptoPerpetual(CryptoPerpetual),
    CurrencyPair(CurrencyPair),
    Equity(Equity),
    FuturesContract(FuturesContract),
    FuturesSpread(FuturesSpread),
    OptionContract(OptionContract),
    OptionSpread(OptionSpread),
}

impl InstrumentAny {
    /// Consumes the `OrderAny` enum and returns the underlying order as a boxed trait.
    #[must_use]
    pub fn into_instrument(self) -> Box<dyn Instrument> {
        match self {
            Self::Betting(inst) => Box::new(inst),
            Self::BinaryOption(inst) => Box::new(inst),
            Self::CryptoFuture(inst) => Box::new(inst),
            Self::CryptoPerpetual(inst) => Box::new(inst),
            Self::CurrencyPair(inst) => Box::new(inst),
            Self::Equity(inst) => Box::new(inst),
            Self::FuturesContract(inst) => Box::new(inst),
            Self::FuturesSpread(inst) => Box::new(inst),
            Self::OptionContract(inst) => Box::new(inst),
            Self::OptionSpread(inst) => Box::new(inst),
        }
    }

    #[must_use]
    pub fn instrument_class(&self) -> InstrumentClass {
        match self {
            Self::Betting(inst) => inst.instrument_class(),
            Self::BinaryOption(inst) => inst.instrument_class(),
            Self::CryptoFuture(inst) => inst.instrument_class(),
            Self::CryptoPerpetual(inst) => inst.instrument_class(),
            Self::CurrencyPair(inst) => inst.instrument_class(),
            Self::Equity(inst) => inst.instrument_class(),
            Self::FuturesContract(inst) => inst.instrument_class(),
            Self::FuturesSpread(inst) => inst.instrument_class(),
            Self::OptionContract(inst) => inst.instrument_class(),
            Self::OptionSpread(inst) => inst.instrument_class(),
        }
    }

    #[must_use]
    pub fn id(&self) -> InstrumentId {
        match self {
            Self::Betting(inst) => inst.id,
            Self::BinaryOption(inst) => inst.id,
            Self::CryptoFuture(inst) => inst.id,
            Self::CryptoPerpetual(inst) => inst.id,
            Self::CurrencyPair(inst) => inst.id,
            Self::Equity(inst) => inst.id,
            Self::FuturesContract(inst) => inst.id,
            Self::FuturesSpread(inst) => inst.id,
            Self::OptionContract(inst) => inst.id,
            Self::OptionSpread(inst) => inst.id,
        }
    }

    #[must_use]
    pub fn symbol(&self) -> Symbol {
        match self {
            Self::Betting(inst) => inst.id.symbol,
            Self::BinaryOption(inst) => inst.id.symbol,
            Self::CryptoFuture(inst) => inst.id.symbol,
            Self::CryptoPerpetual(inst) => inst.id.symbol,
            Self::CurrencyPair(inst) => inst.id.symbol,
            Self::Equity(inst) => inst.id.symbol,
            Self::FuturesContract(inst) => inst.id.symbol,
            Self::FuturesSpread(inst) => inst.id.symbol,
            Self::OptionContract(inst) => inst.id.symbol,
            Self::OptionSpread(inst) => inst.id.symbol,
        }
    }

    #[must_use]
    pub fn venue(&self) -> Venue {
        match self {
            Self::Betting(inst) => inst.id.venue,
            Self::BinaryOption(inst) => inst.id.venue,
            Self::CryptoFuture(inst) => inst.id.venue,
            Self::CryptoPerpetual(inst) => inst.id.venue,
            Self::CurrencyPair(inst) => inst.id.venue,
            Self::Equity(inst) => inst.id.venue,
            Self::FuturesContract(inst) => inst.id.venue,
            Self::FuturesSpread(inst) => inst.id.venue,
            Self::OptionContract(inst) => inst.id.venue,
            Self::OptionSpread(inst) => inst.id.venue,
        }
    }

    #[must_use]
    pub fn raw_symbol(&self) -> Symbol {
        match self {
            Self::Betting(inst) => inst.raw_symbol(),
            Self::BinaryOption(inst) => inst.raw_symbol(),
            Self::CryptoFuture(inst) => inst.raw_symbol(),
            Self::CryptoPerpetual(inst) => inst.raw_symbol(),
            Self::CurrencyPair(inst) => inst.raw_symbol(),
            Self::Equity(inst) => inst.raw_symbol(),
            Self::FuturesContract(inst) => inst.raw_symbol(),
            Self::FuturesSpread(inst) => inst.raw_symbol(),
            Self::OptionContract(inst) => inst.raw_symbol(),
            Self::OptionSpread(inst) => inst.raw_symbol(),
        }
    }

    #[must_use]
    pub fn underlying(&self) -> Option<&Ustr> {
        match self {
            Self::Betting(_) => None,
            Self::BinaryOption(_) => None,
            Self::CryptoFuture(inst) => Some(&inst.underlying.code),
            Self::CryptoPerpetual(_) => None,
            Self::CurrencyPair(_) => None,
            Self::Equity(_) => None,
            Self::FuturesContract(inst) => Some(&inst.underlying),
            Self::FuturesSpread(inst) => Some(&inst.underlying),
            Self::OptionContract(inst) => Some(&inst.underlying),
            Self::OptionSpread(inst) => Some(&inst.underlying),
        }
    }

    #[must_use]
    pub fn base_currency(&self) -> Option<Currency> {
        match self {
            Self::Betting(inst) => inst.base_currency(),
            Self::BinaryOption(inst) => inst.base_currency(),
            Self::CryptoFuture(inst) => inst.base_currency(),
            Self::CryptoPerpetual(inst) => inst.base_currency(),
            Self::CurrencyPair(inst) => inst.base_currency(),
            Self::Equity(inst) => inst.base_currency(),
            Self::FuturesContract(inst) => inst.base_currency(),
            Self::FuturesSpread(inst) => inst.base_currency(),
            Self::OptionContract(inst) => inst.base_currency(),
            Self::OptionSpread(inst) => inst.base_currency(),
        }
    }

    #[must_use]
    pub fn quote_currency(&self) -> Currency {
        match self {
            Self::Betting(inst) => inst.quote_currency(),
            Self::BinaryOption(inst) => inst.quote_currency(),
            Self::CryptoFuture(inst) => inst.quote_currency(),
            Self::CryptoPerpetual(inst) => inst.quote_currency(),
            Self::CurrencyPair(inst) => inst.quote_currency(),
            Self::Equity(inst) => inst.quote_currency(),
            Self::FuturesContract(inst) => inst.quote_currency(),
            Self::FuturesSpread(inst) => inst.quote_currency(),
            Self::OptionContract(inst) => inst.quote_currency(),
            Self::OptionSpread(inst) => inst.quote_currency(),
        }
    }

    #[must_use]
    pub fn settlement_currency(&self) -> Currency {
        match self {
            Self::Betting(inst) => inst.settlement_currency(),
            Self::BinaryOption(inst) => inst.settlement_currency(),
            Self::CryptoFuture(inst) => inst.settlement_currency(),
            Self::CryptoPerpetual(inst) => inst.settlement_currency(),
            Self::CurrencyPair(inst) => inst.settlement_currency(),
            Self::Equity(inst) => inst.settlement_currency(),
            Self::FuturesContract(inst) => inst.settlement_currency(),
            Self::FuturesSpread(inst) => inst.settlement_currency(),
            Self::OptionContract(inst) => inst.settlement_currency(),
            Self::OptionSpread(inst) => inst.settlement_currency(),
        }
    }

    #[must_use]
    pub fn is_inverse(&self) -> bool {
        match self {
            Self::Betting(inst) => inst.is_inverse(),
            Self::BinaryOption(inst) => inst.is_inverse(),
            Self::CryptoFuture(inst) => inst.is_inverse(),
            Self::CryptoPerpetual(inst) => inst.is_inverse(),
            Self::CurrencyPair(inst) => inst.is_inverse(),
            Self::Equity(inst) => inst.is_inverse(),
            Self::FuturesContract(inst) => inst.is_inverse(),
            Self::FuturesSpread(inst) => inst.is_inverse(),
            Self::OptionContract(inst) => inst.is_inverse(),
            Self::OptionSpread(inst) => inst.is_inverse(),
        }
    }

    #[must_use]
    pub fn price_precision(&self) -> u8 {
        match self {
            Self::Betting(inst) => inst.price_precision(),
            Self::BinaryOption(inst) => inst.price_precision(),
            Self::CryptoFuture(inst) => inst.price_precision(),
            Self::CryptoPerpetual(inst) => inst.price_precision(),
            Self::CurrencyPair(inst) => inst.price_precision(),
            Self::Equity(inst) => inst.price_precision(),
            Self::FuturesContract(inst) => inst.price_precision(),
            Self::FuturesSpread(inst) => inst.price_precision(),
            Self::OptionContract(inst) => inst.price_precision(),
            Self::OptionSpread(inst) => inst.price_precision(),
        }
    }

    #[must_use]
    pub fn size_precision(&self) -> u8 {
        match self {
            Self::Betting(inst) => inst.size_precision(),
            Self::BinaryOption(inst) => inst.size_precision(),
            Self::CryptoFuture(inst) => inst.size_precision(),
            Self::CryptoPerpetual(inst) => inst.size_precision(),
            Self::CurrencyPair(inst) => inst.size_precision(),
            Self::Equity(inst) => inst.size_precision(),
            Self::FuturesContract(inst) => inst.size_precision(),
            Self::FuturesSpread(inst) => inst.size_precision(),
            Self::OptionContract(inst) => inst.size_precision(),
            Self::OptionSpread(inst) => inst.size_precision(),
        }
    }

    #[must_use]
    pub fn price_increment(&self) -> Price {
        match self {
            Self::Betting(inst) => inst.price_increment(),
            Self::BinaryOption(inst) => inst.price_increment(),
            Self::CryptoFuture(inst) => inst.price_increment(),
            Self::CryptoPerpetual(inst) => inst.price_increment(),
            Self::CurrencyPair(inst) => inst.price_increment(),
            Self::Equity(inst) => inst.price_increment(),
            Self::FuturesContract(inst) => inst.price_increment(),
            Self::FuturesSpread(inst) => inst.price_increment(),
            Self::OptionContract(inst) => inst.price_increment(),
            Self::OptionSpread(inst) => inst.price_increment(),
        }
    }

    #[must_use]
    pub fn size_increment(&self) -> Quantity {
        match self {
            Self::Betting(inst) => inst.size_increment(),
            Self::BinaryOption(inst) => inst.size_increment(),
            Self::CryptoFuture(inst) => inst.size_increment(),
            Self::CryptoPerpetual(inst) => inst.size_increment(),
            Self::CurrencyPair(inst) => inst.size_increment(),
            Self::Equity(inst) => inst.size_increment(),
            Self::FuturesContract(inst) => inst.size_increment(),
            Self::FuturesSpread(inst) => inst.size_increment(),
            Self::OptionContract(inst) => inst.size_increment(),
            Self::OptionSpread(inst) => inst.size_increment(),
        }
    }

    #[must_use]
    pub fn multiplier(&self) -> Quantity {
        match self {
            Self::Betting(inst) => inst.multiplier(),
            Self::BinaryOption(inst) => inst.multiplier(),
            Self::CryptoFuture(inst) => inst.multiplier(),
            Self::CryptoPerpetual(inst) => inst.multiplier(),
            Self::CurrencyPair(inst) => inst.multiplier(),
            Self::Equity(inst) => inst.multiplier(),
            Self::FuturesContract(inst) => inst.multiplier(),
            Self::FuturesSpread(inst) => inst.multiplier(),
            Self::OptionContract(inst) => inst.multiplier(),
            Self::OptionSpread(inst) => inst.multiplier(),
        }
    }

    #[must_use]
    pub fn activation_ns(&self) -> Option<UnixNanos> {
        match self {
            Self::Betting(inst) => inst.activation_ns(),
            Self::BinaryOption(inst) => inst.activation_ns(),
            Self::CryptoFuture(inst) => inst.activation_ns(),
            Self::CryptoPerpetual(inst) => inst.activation_ns(),
            Self::CurrencyPair(inst) => inst.activation_ns(),
            Self::Equity(inst) => inst.activation_ns(),
            Self::FuturesContract(inst) => inst.activation_ns(),
            Self::FuturesSpread(inst) => inst.activation_ns(),
            Self::OptionContract(inst) => inst.activation_ns(),
            Self::OptionSpread(inst) => inst.activation_ns(),
        }
    }

    #[must_use]
    pub fn expiration_ns(&self) -> Option<UnixNanos> {
        match self {
            Self::Betting(inst) => inst.expiration_ns(),
            Self::BinaryOption(inst) => inst.expiration_ns(),
            Self::CryptoFuture(inst) => inst.expiration_ns(),
            Self::CryptoPerpetual(inst) => inst.expiration_ns(),
            Self::CurrencyPair(inst) => inst.expiration_ns(),
            Self::Equity(inst) => inst.expiration_ns(),
            Self::FuturesContract(inst) => inst.expiration_ns(),
            Self::FuturesSpread(inst) => inst.expiration_ns(),
            Self::OptionContract(inst) => inst.expiration_ns(),
            Self::OptionSpread(inst) => inst.expiration_ns(),
        }
    }

    pub fn max_quantity(&self) -> Option<Quantity> {
        match self {
            Self::Betting(inst) => inst.max_quantity(),
            Self::BinaryOption(inst) => inst.max_quantity(),
            Self::CryptoFuture(inst) => inst.max_quantity(),
            Self::CryptoPerpetual(inst) => inst.max_quantity(),
            Self::CurrencyPair(inst) => inst.max_quantity(),
            Self::Equity(inst) => inst.max_quantity(),
            Self::FuturesContract(inst) => inst.max_quantity(),
            Self::FuturesSpread(inst) => inst.max_quantity(),
            Self::OptionContract(inst) => inst.max_quantity(),
            Self::OptionSpread(inst) => inst.max_quantity(),
        }
    }

    pub fn min_quantity(&self) -> Option<Quantity> {
        match self {
            Self::Betting(inst) => inst.min_quantity(),
            Self::BinaryOption(inst) => inst.min_quantity(),
            Self::CryptoFuture(inst) => inst.min_quantity(),
            Self::CryptoPerpetual(inst) => inst.min_quantity(),
            Self::CurrencyPair(inst) => inst.min_quantity(),
            Self::Equity(inst) => inst.min_quantity(),
            Self::FuturesContract(inst) => inst.min_quantity(),
            Self::FuturesSpread(inst) => inst.min_quantity(),
            Self::OptionContract(inst) => inst.min_quantity(),
            Self::OptionSpread(inst) => inst.min_quantity(),
        }
    }

    pub fn max_notional(&self) -> Option<Money> {
        match self {
            Self::Betting(inst) => inst.max_notional(),
            Self::BinaryOption(inst) => inst.max_notional(),
            Self::CryptoFuture(inst) => inst.max_notional(),
            Self::CryptoPerpetual(inst) => inst.max_notional(),
            Self::CurrencyPair(inst) => inst.max_notional(),
            Self::Equity(inst) => inst.max_notional(),
            Self::FuturesContract(inst) => inst.max_notional(),
            Self::FuturesSpread(inst) => inst.max_notional(),
            Self::OptionContract(inst) => inst.max_notional(),
            Self::OptionSpread(inst) => inst.max_notional(),
        }
    }

    pub fn min_notional(&self) -> Option<Money> {
        match self {
            Self::Betting(inst) => inst.min_notional(),
            Self::BinaryOption(inst) => inst.min_notional(),
            Self::CryptoFuture(inst) => inst.min_notional(),
            Self::CryptoPerpetual(inst) => inst.min_notional(),
            Self::CurrencyPair(inst) => inst.min_notional(),
            Self::Equity(inst) => inst.min_notional(),
            Self::FuturesContract(inst) => inst.min_notional(),
            Self::FuturesSpread(inst) => inst.min_notional(),
            Self::OptionContract(inst) => inst.min_notional(),
            Self::OptionSpread(inst) => inst.min_notional(),
        }
    }

    pub fn ts_event(&self) -> UnixNanos {
        match self {
            Self::Betting(inst) => inst.ts_event,
            Self::BinaryOption(inst) => inst.ts_event,
            Self::CryptoFuture(inst) => inst.ts_event,
            Self::CryptoPerpetual(inst) => inst.ts_event,
            Self::CurrencyPair(inst) => inst.ts_event,
            Self::Equity(inst) => inst.ts_event,
            Self::FuturesContract(inst) => inst.ts_event,
            Self::FuturesSpread(inst) => inst.ts_event,
            Self::OptionContract(inst) => inst.ts_event,
            Self::OptionSpread(inst) => inst.ts_event,
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Betting(inst) => inst.ts_init,
            Self::BinaryOption(inst) => inst.ts_init,
            Self::CryptoFuture(inst) => inst.ts_init,
            Self::CryptoPerpetual(inst) => inst.ts_init,
            Self::CurrencyPair(inst) => inst.ts_init,
            Self::Equity(inst) => inst.ts_init,
            Self::FuturesContract(inst) => inst.ts_init,
            Self::FuturesSpread(inst) => inst.ts_init,
            Self::OptionContract(inst) => inst.ts_init,
            Self::OptionSpread(inst) => inst.ts_init,
        }
    }

    pub fn make_price(&self, value: f64) -> Price {
        match self {
            Self::Betting(inst) => inst.make_price(value),
            Self::BinaryOption(inst) => inst.make_price(value),
            Self::CryptoFuture(inst) => inst.make_price(value),
            Self::CryptoPerpetual(inst) => inst.make_price(value),
            Self::CurrencyPair(inst) => inst.make_price(value),
            Self::Equity(inst) => inst.make_price(value),
            Self::FuturesContract(inst) => inst.make_price(value),
            Self::FuturesSpread(inst) => inst.make_price(value),
            Self::OptionContract(inst) => inst.make_price(value),
            Self::OptionSpread(inst) => inst.make_price(value),
        }
    }

    pub fn make_qty(&self, value: f64) -> Quantity {
        match self {
            Self::Betting(inst) => inst.make_qty(value),
            Self::BinaryOption(inst) => inst.make_qty(value),
            Self::CryptoFuture(inst) => inst.make_qty(value),
            Self::CryptoPerpetual(inst) => inst.make_qty(value),
            Self::CurrencyPair(inst) => inst.make_qty(value),
            Self::Equity(inst) => inst.make_qty(value),
            Self::FuturesContract(inst) => inst.make_qty(value),
            Self::FuturesSpread(inst) => inst.make_qty(value),
            Self::OptionContract(inst) => inst.make_qty(value),
            Self::OptionSpread(inst) => inst.make_qty(value),
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
            Self::Betting(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::BinaryOption(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
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
            Self::OptionContract(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
            Self::OptionSpread(inst) => {
                inst.calculate_notional_value(quantity, price, use_quote_for_inverse)
            }
        }
    }

    // #[deprecated(since = "0.21.0", note = "Will be removed in a future version")]
    #[must_use]
    pub fn maker_fee(&self) -> Decimal {
        match self {
            Self::Betting(inst) => inst.maker_fee(),
            Self::BinaryOption(inst) => inst.maker_fee(),
            Self::CryptoFuture(inst) => inst.maker_fee(),
            Self::CryptoPerpetual(inst) => inst.maker_fee(),
            Self::CurrencyPair(inst) => inst.maker_fee(),
            Self::Equity(inst) => inst.maker_fee(),
            Self::FuturesContract(inst) => inst.maker_fee(),
            Self::FuturesSpread(inst) => inst.maker_fee(),
            Self::OptionContract(inst) => inst.maker_fee(),
            Self::OptionSpread(inst) => inst.maker_fee(),
        }
    }

    // #[deprecated(since = "0.21.0", note = "Will be removed in a future version")]
    #[must_use]
    pub fn taker_fee(&self) -> Decimal {
        match self {
            Self::Betting(inst) => inst.taker_fee(),
            Self::BinaryOption(inst) => inst.taker_fee(),
            Self::CryptoFuture(inst) => inst.taker_fee(),
            Self::CryptoPerpetual(inst) => inst.taker_fee(),
            Self::CurrencyPair(inst) => inst.taker_fee(),
            Self::Equity(inst) => inst.taker_fee(),
            Self::FuturesContract(inst) => inst.taker_fee(),
            Self::FuturesSpread(inst) => inst.taker_fee(),
            Self::OptionContract(inst) => inst.taker_fee(),
            Self::OptionSpread(inst) => inst.taker_fee(),
        }
    }

    pub fn get_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        match self {
            Self::Betting(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::BinaryOption(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoFuture(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoPerpetual(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CurrencyPair(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::Equity(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::FuturesContract(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::FuturesSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::OptionContract(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::OptionSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
        }
    }
}

impl PartialEq for InstrumentAny {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
