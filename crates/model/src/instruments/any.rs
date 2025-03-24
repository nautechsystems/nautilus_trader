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

use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

use super::{
    Instrument, betting::BettingInstrument, binary_option::BinaryOption,
    crypto_future::CryptoFuture, crypto_option::CryptoOption, crypto_perpetual::CryptoPerpetual,
    currency_pair::CurrencyPair, equity::Equity, futures_contract::FuturesContract,
    futures_spread::FuturesSpread, option_contract::OptionContract, option_spread::OptionSpread,
};
use crate::types::{Price, Quantity};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[enum_dispatch(Instrument)]
pub enum InstrumentAny {
    Betting(BettingInstrument),
    BinaryOption(BinaryOption),
    CryptoFuture(CryptoFuture),
    CryptoOption(CryptoOption),
    CryptoPerpetual(CryptoPerpetual),
    CurrencyPair(CurrencyPair),
    Equity(Equity),
    FuturesContract(FuturesContract),
    FuturesSpread(FuturesSpread),
    OptionContract(OptionContract),
    OptionSpread(OptionSpread),
}

// TODO: Probably move this to the `Instrument` trait too
impl InstrumentAny {
    #[must_use]
    pub fn get_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        match self {
            Self::Betting(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::BinaryOption(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoFuture(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoOption(inst) => inst.calculate_base_quantity(quantity, last_px),
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
