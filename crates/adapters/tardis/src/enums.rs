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

use nautilus_model::identifiers::Venue;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};
use ustr::Ustr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display, FromRepr, EnumString)]
#[serde(rename_all = "lowercase")]
/// The instrument type for the symbol.
pub enum InstrumentType {
    Spot,
    Perpetual,
    Future,
    Option,
    Combo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display, FromRepr, EnumString)]
#[serde(rename_all = "lowercase")]
/// The type of option.
pub enum OptionType {
    Call,
    Put,
}

/// The aggressor side of the trade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display, FromRepr, EnumString)]
#[serde(rename_all = "lowercase")]
pub enum TradeSide {
    Buy,
    Sell,
    Unknown,
}

/// The bar kind.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Display, FromRepr, EnumString)]
#[serde(rename_all = "lowercase")]
pub enum BarKind {
    Time,
    Volume,
    Tick,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Display,
    AsRefStr,
    EnumIter,
    EnumString,
    FromRepr,
)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
/// Represents a crypto exchange.
/// See <https://api.tardis.dev/v1/exchanges> for all supported exchanges.
pub enum Exchange {
    Ascendex,
    Binance,
    BinanceDelivery,
    BinanceDex,
    BinanceFutures,
    BinanceJersey,
    BinanceOptions,
    BinanceUs,
    Bitfinex,
    BitfinexDerivatives,
    Bitflyer,
    Bitmex,
    Bitnomial,
    Bitstamp,
    BlockchainCom,
    Bybit,
    BybitOptions,
    BybitSpot,
    Coinbase,
    Coinflex,
    CryptoCom,
    CryptoComDerivatives,
    Cryptofacilities,
    Delta,
    Deribit,
    Dydx,
    Ftx,
    FtxUs,
    GateIo,
    GateIoFutures,
    Gemini,
    Hitbtc,
    Huobi,
    HuobiDm,
    HuobiDmLinearSwap,
    HuobiDmOptions,
    HuobiDmSwap,
    Kraken,
    Kucoin,
    Mango,
    Okcoin,
    Okex,
    OkexFutures,
    OkexOptions,
    OkexSwap,
    Phemex,
    Poloniex,
    Serum,
    StarAtlas,
    Upbit,
    WooX,
}

impl Exchange {
    #[must_use]
    pub fn from_venue_str(s: &str) -> Vec<Self> {
        let s = s.to_ascii_uppercase();
        match s.as_str() {
            "ASCENDEX" => vec![Self::Ascendex],
            "BINANCE" => vec![
                Self::Binance,
                Self::BinanceDex,
                Self::BinanceFutures,
                Self::BinanceJersey,
                Self::BinanceOptions,
            ],
            "BINANCE_DELIVERY" => vec![Self::BinanceDelivery],
            "BINANCE_US" => vec![Self::BinanceUs],
            "BITFINEX" => vec![Self::Bitfinex, Self::BitfinexDerivatives],
            "BITFLYER" => vec![Self::Bitflyer],
            "BITMEX" => vec![Self::Bitmex],
            "BITNOMIAL" => vec![Self::Bitnomial],
            "BITSTAMP" => vec![Self::Bitstamp],
            "BLOCKCHAIN_COM" => vec![Self::BlockchainCom],
            "BYBIT" => vec![Self::Bybit, Self::BybitOptions, Self::BybitSpot],
            "COINBASE" => vec![Self::Coinbase],
            "COINFLEX" => vec![Self::Coinflex],
            "CRYPTO_COM" => vec![Self::CryptoCom, Self::CryptoComDerivatives],
            "CRYPTOFACILITIES" => vec![Self::Cryptofacilities],
            "DELTA" => vec![Self::Delta],
            "DERIBIT" => vec![Self::Deribit],
            "DYDX" => vec![Self::Dydx],
            "FTX" => vec![Self::Ftx, Self::FtxUs],
            "GATE_IO" => vec![Self::GateIo, Self::GateIoFutures],
            "GEMINI" => vec![Self::Gemini],
            "HITBTC" => vec![Self::Hitbtc],
            "HUOBI" => vec![
                Self::Huobi,
                Self::HuobiDm,
                Self::HuobiDmLinearSwap,
                Self::HuobiDmOptions,
            ],
            "HUOBI_DELIVERY" => vec![Self::HuobiDmSwap],
            "KRAKEN" => vec![Self::Kraken],
            "KUCOIN" => vec![Self::Kucoin],
            "MANGO" => vec![Self::Mango],
            "OKCOIN" => vec![Self::Okcoin],
            "OKEX" => vec![
                Self::Okex,
                Self::OkexFutures,
                Self::OkexOptions,
                Self::OkexSwap,
            ],
            "PHEMEX" => vec![Self::Phemex],
            "POLONIEX" => vec![Self::Poloniex],
            "SERUM" => vec![Self::Serum],
            "STARATLAS" => vec![Self::StarAtlas],
            "UPBIT" => vec![Self::Upbit],
            "WOO_X" => vec![Self::WooX],
            _ => Vec::new(),
        }
    }

    #[must_use]
    pub const fn as_venue_str(&self) -> &str {
        match self {
            Self::Ascendex => "ASCENDEX",
            Self::Binance => "BINANCE",
            Self::BinanceDelivery => "BINANCE_DELIVERY",
            Self::BinanceDex => "BINANCE",
            Self::BinanceFutures => "BINANCE",
            Self::BinanceJersey => "BINANCE",
            Self::BinanceOptions => "BINANCE",
            Self::BinanceUs => "BINANCE_US",
            Self::Bitfinex => "BITFINEX",
            Self::BitfinexDerivatives => "BITFINEX",
            Self::Bitflyer => "BITFLYER",
            Self::Bitmex => "BITMEX",
            Self::Bitnomial => "BITNOMIAL",
            Self::Bitstamp => "BITSTAMP",
            Self::BlockchainCom => "BLOCKCHAIN_COM",
            Self::Bybit => "BYBIT",
            Self::BybitOptions => "BYBIT",
            Self::BybitSpot => "BYBIT",
            Self::Coinbase => "COINBASE",
            Self::Coinflex => "COINFLEX",
            Self::CryptoCom => "CRYPTO_COM",
            Self::CryptoComDerivatives => "CRYPTO_COM",
            Self::Cryptofacilities => "CRYPTOFACILITIES",
            Self::Delta => "DELTA",
            Self::Deribit => "DERIBIT",
            Self::Dydx => "DYDX",
            Self::Ftx => "FTX",
            Self::FtxUs => "FTX",
            Self::GateIo => "GATE_IO",
            Self::GateIoFutures => "GATE_IO",
            Self::Gemini => "GEMINI",
            Self::Hitbtc => "HITBTC",
            Self::Huobi => "HUOBI",
            Self::HuobiDm => "HUOBI",
            Self::HuobiDmLinearSwap => "HUOBI",
            Self::HuobiDmOptions => "HUOBI",
            Self::HuobiDmSwap => "HUOBI_DELIVERY",
            Self::Kraken => "KRAKEN",
            Self::Kucoin => "KUCOIN",
            Self::Mango => "MANGO",
            Self::Okcoin => "OKCOIN",
            Self::Okex => "OKEX",
            Self::OkexFutures => "OKEX",
            Self::OkexOptions => "OKEX",
            Self::OkexSwap => "OKEX",
            Self::Phemex => "PHEMEX",
            Self::Poloniex => "POLONIEX",
            Self::Serum => "SERUM",
            Self::StarAtlas => "STARATLAS",
            Self::Upbit => "UPBIT",
            Self::WooX => "WOO_X",
        }
    }

    #[must_use]
    pub fn as_venue(&self) -> Venue {
        Venue::from_ustr_unchecked(Ustr::from(self.as_venue_str()))
    }
}
