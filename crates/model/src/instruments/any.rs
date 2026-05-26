// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

use super::{
    Instrument, betting::BettingInstrument, binary_option::BinaryOption, cfd::Cfd,
    commodity::Commodity, crypto_future::CryptoFuture, crypto_futures_spread::CryptoFuturesSpread,
    crypto_option::CryptoOption, crypto_option_spread::CryptoOptionSpread,
    crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair, equity::Equity,
    futures_contract::FuturesContract, futures_spread::FuturesSpread,
    index_instrument::IndexInstrument, option_contract::OptionContract,
    option_spread::OptionSpread, perpetual_contract::PerpetualContract,
    tokenized_asset::TokenizedAsset,
};
use crate::types::{Price, Quantity};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[enum_dispatch(Instrument)]
pub enum InstrumentAny {
    Betting(BettingInstrument),
    BinaryOption(BinaryOption),
    Cfd(Cfd),
    Commodity(Commodity),
    CryptoFuture(CryptoFuture),
    CryptoFuturesSpread(CryptoFuturesSpread),
    CryptoOption(CryptoOption),
    CryptoOptionSpread(CryptoOptionSpread),
    CryptoPerpetual(CryptoPerpetual),
    CurrencyPair(CurrencyPair),
    Equity(Equity),
    FuturesContract(FuturesContract),
    FuturesSpread(FuturesSpread),
    IndexInstrument(IndexInstrument),
    OptionContract(OptionContract),
    OptionSpread(OptionSpread),
    PerpetualContract(PerpetualContract),
    TokenizedAsset(TokenizedAsset),
}

// TODO: Probably move this to the `Instrument` trait too
impl InstrumentAny {
    #[must_use]
    pub fn get_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        match self {
            Self::Betting(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::BinaryOption(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::Cfd(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::Commodity(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoFuture(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoFuturesSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoOption(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoOptionSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoPerpetual(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CurrencyPair(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::Equity(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::FuturesContract(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::FuturesSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::IndexInstrument(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::OptionContract(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::OptionSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::PerpetualContract(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::TokenizedAsset(inst) => inst.calculate_base_quantity(quantity, last_px),
        }
    }

    /// Returns true if the instrument is a spread instrument.
    #[must_use]
    pub fn is_spread(&self) -> bool {
        matches!(
            self,
            Self::FuturesSpread(_)
                | Self::OptionSpread(_)
                | Self::CryptoFuturesSpread(_)
                | Self::CryptoOptionSpread(_)
        )
    }
}

impl PartialEq for InstrumentAny {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl crate::data::HasTsInit for InstrumentAny {
    fn ts_init(&self) -> nautilus_core::UnixNanos {
        Instrument::ts_init(self)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::instruments::stubs::*;

    #[rstest]
    #[case::futures_spread(InstrumentAny::FuturesSpread(futures_spread_es()), true)]
    #[case::option_spread(InstrumentAny::OptionSpread(option_spread()), true)]
    #[case::crypto_futures_spread(
        InstrumentAny::CryptoFuturesSpread(crypto_futures_spread_btc_deribit()),
        true
    )]
    #[case::crypto_option_spread(
        InstrumentAny::CryptoOptionSpread(crypto_option_spread_btc_deribit()),
        true
    )]
    #[case::crypto_future(
        InstrumentAny::CryptoFuture(crypto_future_btcusdt(
            2,
            6,
            crate::types::Price::from("0.01"),
            crate::types::Quantity::from("0.000001"),
        )),
        false
    )]
    #[case::crypto_option(
        InstrumentAny::CryptoOption(crypto_option_btc_deribit(
            3,
            1,
            crate::types::Price::from("0.001"),
            crate::types::Quantity::from("0.1"),
        )),
        false
    )]
    fn test_is_spread(#[case] instrument: InstrumentAny, #[case] expected: bool) {
        assert_eq!(instrument.is_spread(), expected);
    }
}
