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

#[derive(
    Copy,
    Clone,
    Debug,
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
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
/// The instrument type for the symbol.
pub enum TardisInstrumentType {
    Spot,
    Perpetual,
    Future,
    Option,
    Combo,
}

#[derive(
    Copy,
    Clone,
    Debug,
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
#[serde(rename_all = "lowercase")]
/// The type of option.
pub enum TardisOptionType {
    Call,
    Put,
}

/// The aggressor side of the trade.
#[derive(
    Copy,
    Clone,
    Debug,
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
#[serde(rename_all = "lowercase")]
pub enum TardisTradeSide {
    Buy,
    Sell,
    Unknown,
}

/// The bar kind.
#[allow(missing_docs)]
#[derive(
    Copy,
    Clone,
    Debug,
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
#[serde(rename_all = "lowercase")]
pub enum TardisBarKind {
    Time,
    Volume,
    Tick,
}

#[derive(
    Copy,
    Clone,
    Debug,
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
pub enum TardisExchange {
    Ascendex,
    Binance,
    BinanceDelivery,
    BinanceDex,
    BinanceEuropeanOptions,
    BinanceFutures,
    BinanceJersey,
    BinanceOptions,
    BinanceUs,
    Bitfinex,
    BitfinexDerivatives,
    Bitflyer,
    Bitget,
    BitgetFutures,
    Bitmex,
    Bitnomial,
    Bitstamp,
    BlockchainCom,
    Bybit,
    BybitOptions,
    BybitSpot,
    Coinbase,
    CoinbaseInternational,
    Coinflex,
    CryptoCom,
    CryptoComDerivatives,
    Cryptofacilities,
    Delta,
    Deribit,
    Dydx,
    DydxV4,
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
    Hyperliquid,
    Kraken,
    Kucoin,
    KucoinFutures,
    Mango,
    Okcoin,
    Okex,
    OkexFutures,
    OkexOptions,
    OkexSpreads,
    OkexSwap,
    Phemex,
    Poloniex,
    Serum,
    StarAtlas,
    Upbit,
    WooX,
}

impl TardisExchange {
    /// Option-specific exchanges that should be filtered out when options are not requested.
    pub const OPTION_EXCHANGES: &'static [Self] = &[
        Self::BinanceOptions,
        Self::BinanceEuropeanOptions,
        Self::BybitOptions,
        Self::OkexOptions,
        Self::HuobiDmOptions,
    ];

    #[must_use]
    pub fn is_option_exchange(&self) -> bool {
        Self::OPTION_EXCHANGES.contains(self)
    }

    #[must_use]
    pub fn from_venue_str(s: &str) -> Vec<Self> {
        let s = s.to_ascii_uppercase();
        match s.as_str() {
            "ASCENDEX" => vec![Self::Ascendex],
            "BINANCE" => vec![
                Self::Binance,
                Self::BinanceDex,
                Self::BinanceEuropeanOptions,
                Self::BinanceFutures,
                Self::BinanceJersey,
                Self::BinanceOptions,
            ],
            "BINANCE_DELIVERY" => vec![Self::BinanceDelivery],
            "BINANCE_US" => vec![Self::BinanceUs],
            "BITFINEX" => vec![Self::Bitfinex, Self::BitfinexDerivatives],
            "BITFLYER" => vec![Self::Bitflyer],
            "BITGET" => vec![Self::Bitget, Self::BitgetFutures],
            "BITMEX" => vec![Self::Bitmex],
            "BITNOMIAL" => vec![Self::Bitnomial],
            "BITSTAMP" => vec![Self::Bitstamp],
            "BLOCKCHAIN_COM" => vec![Self::BlockchainCom],
            "BYBIT" => vec![Self::Bybit, Self::BybitOptions, Self::BybitSpot],
            "COINBASE" => vec![Self::Coinbase],
            "COINBASE_INTX" => vec![Self::CoinbaseInternational],
            "COINFLEX" => vec![Self::Coinflex],
            "CRYPTO_COM" => vec![Self::CryptoCom, Self::CryptoComDerivatives],
            "CRYPTOFACILITIES" => vec![Self::Cryptofacilities],
            "DELTA" => vec![Self::Delta],
            "DERIBIT" => vec![Self::Deribit],
            "DYDX" => vec![Self::Dydx],
            "DYDX_V4" => vec![Self::DydxV4],
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
            "HYPERLIQUID" => vec![Self::Hyperliquid],
            "KRAKEN" => vec![Self::Kraken],
            "KUCOIN" => vec![Self::Kucoin, Self::KucoinFutures],
            "MANGO" => vec![Self::Mango],
            "OKCOIN" => vec![Self::Okcoin],
            "OKEX" => vec![
                Self::Okex,
                Self::OkexFutures,
                Self::OkexOptions,
                Self::OkexSpreads,
                Self::OkexSwap,
            ],
            "PHEMEX" => vec![Self::Phemex],
            "POLONIEX" => vec![Self::Poloniex],
            "SERUM" => vec![Self::Serum],
            "STAR_ATLAS" => vec![Self::StarAtlas],
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
            Self::BinanceEuropeanOptions => "BINANCE",
            Self::BinanceFutures => "BINANCE",
            Self::BinanceJersey => "BINANCE",
            Self::BinanceOptions => "BINANCE",
            Self::BinanceUs => "BINANCE_US",
            Self::Bitfinex => "BITFINEX",
            Self::BitfinexDerivatives => "BITFINEX",
            Self::Bitflyer => "BITFLYER",
            Self::Bitget => "BITGET",
            Self::BitgetFutures => "BITGET",
            Self::Bitmex => "BITMEX",
            Self::Bitnomial => "BITNOMIAL",
            Self::Bitstamp => "BITSTAMP",
            Self::BlockchainCom => "BLOCKCHAIN_COM",
            Self::Bybit => "BYBIT",
            Self::BybitOptions => "BYBIT",
            Self::BybitSpot => "BYBIT",
            Self::Coinbase => "COINBASE",
            Self::CoinbaseInternational => "COINBASE_INTX",
            Self::Coinflex => "COINFLEX",
            Self::CryptoCom => "CRYPTO_COM",
            Self::CryptoComDerivatives => "CRYPTO_COM",
            Self::Cryptofacilities => "CRYPTOFACILITIES",
            Self::Delta => "DELTA",
            Self::Deribit => "DERIBIT",
            Self::Dydx => "DYDX",
            Self::DydxV4 => "DYDX_V4",
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
            Self::Hyperliquid => "HYPERLIQUID",
            Self::Kraken => "KRAKEN",
            Self::Kucoin => "KUCOIN",
            Self::KucoinFutures => "KUCOIN",
            Self::Mango => "MANGO",
            Self::Okcoin => "OKCOIN",
            Self::Okex => "OKEX",
            Self::OkexFutures => "OKEX",
            Self::OkexOptions => "OKEX",
            Self::OkexSpreads => "OKEX",
            Self::OkexSwap => "OKEX",
            Self::Phemex => "PHEMEX",
            Self::Poloniex => "POLONIEX",
            Self::Serum => "SERUM",
            Self::StarAtlas => "STAR_ATLAS",
            Self::Upbit => "UPBIT",
            Self::WooX => "WOO_X",
        }
    }

    #[must_use]
    pub fn as_venue(&self) -> Venue {
        Venue::from_ustr_unchecked(Ustr::from(self.as_venue_str()))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    #[rstest]
    fn test_exchange_to_venue_mapping() {
        for exchange in TardisExchange::iter() {
            let venue_str = exchange.as_venue_str();
            assert!(
                Venue::new_checked(venue_str).is_ok(),
                "Tardis exchange '{exchange:?}' maps to invalid Nautilus venue '{venue_str}'",
            );
        }
    }

    #[rstest]
    fn test_venue_to_exchange_mapping_bidirectional() {
        let test_venues = [
            "BINANCE",
            "BITMEX",
            "DERIBIT",
            "KRAKEN",
            "COINBASE",
            "BYBIT",
            "OKEX",
            "HUOBI",
            "GATE_IO",
            "KUCOIN",
            "BITFINEX",
            "GEMINI",
            "BITSTAMP",
            "ASCENDEX",
            "PHEMEX",
            "POLONIEX",
            "UPBIT",
            "WOO_X",
            "HYPERLIQUID",
            "CRYPTO_COM",
            "DYDX",
            "HITBTC",
        ];

        for venue_str in test_venues {
            let venue = Venue::new(venue_str);
            let exchanges = TardisExchange::from_venue_str(venue.as_str());

            for exchange in exchanges {
                assert_eq!(
                    exchange.as_venue_str(),
                    venue_str,
                    "Bidirectional mapping failed: Nautilus venue '{venue_str}' -> Tardis exchange '{exchange:?}' -> Nautilus venue '{}'",
                    exchange.as_venue_str()
                );
            }
        }
    }
}
