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

use nautilus_model::identifiers::Venue;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString, FromRepr};
use ustr::Ustr;

/// CryptoHFTData exchange identifiers.
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
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum CryptoHFTDataExchange {
    BinanceSpot,
    BinanceFutures,
    BybitSpot,
    Bybit,
    KrakenSpot,
    KrakenDerivatives,
    OkxSpot,
    OkxFutures,
    BitgetSpot,
    BitgetFutures,
    HyperliquidSpot,
    HyperliquidFutures,
    Lighter,
    AsterFutures,
    Bitmex,
}

impl CryptoHFTDataExchange {
    /// Returns the CHD API exchange string.
    #[must_use]
    pub fn as_chd_str(&self) -> &'static str {
        match self {
            Self::BinanceSpot => "binance_spot",
            Self::BinanceFutures => "binance_futures",
            Self::BybitSpot => "bybit_spot",
            Self::Bybit => "bybit",
            Self::KrakenSpot => "kraken_spot",
            Self::KrakenDerivatives => "kraken_derivatives",
            Self::OkxSpot => "okx_spot",
            Self::OkxFutures => "okx_futures",
            Self::BitgetSpot => "bitget_spot",
            Self::BitgetFutures => "bitget_futures",
            Self::HyperliquidSpot => "hyperliquid_spot",
            Self::HyperliquidFutures => "hyperliquid_futures",
            Self::Lighter => "lighter",
            Self::AsterFutures => "aster_futures",
            Self::Bitmex => "bitmex",
        }
    }

    /// Returns the Nautilus venue used for generated instrument identifiers.
    #[must_use]
    pub fn as_venue_str(&self) -> &'static str {
        match self {
            Self::BinanceSpot | Self::BinanceFutures => "BINANCE",
            Self::BybitSpot | Self::Bybit => "BYBIT",
            Self::KrakenSpot | Self::KrakenDerivatives => "KRAKEN",
            Self::OkxSpot | Self::OkxFutures => "OKEX",
            Self::BitgetSpot | Self::BitgetFutures => "BITGET",
            Self::HyperliquidSpot | Self::HyperliquidFutures => "HYPERLIQUID",
            Self::Lighter => "LIGHTER",
            Self::AsterFutures => "ASTER",
            Self::Bitmex => "BITMEX",
        }
    }

    /// Returns the Nautilus venue used for generated instrument identifiers.
    #[must_use]
    pub fn as_venue(&self) -> Venue {
        Venue::from_ustr_unchecked(Ustr::from(self.as_venue_str()))
    }
}

/// CryptoHFTData dataset identifiers.
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
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum CryptoHFTDataType {
    Klines,
    Trades,
    Orderbook,
    Ticker,
    MarkPrice,
    OpenInterest,
    Liquidations,
}

impl CryptoHFTDataType {
    /// Returns the CHD API data type string.
    #[must_use]
    pub fn as_chd_str(&self) -> &'static str {
        match self {
            Self::Klines => "klines",
            Self::Trades => "trades",
            Self::Orderbook => "orderbook",
            Self::Ticker => "ticker",
            Self::MarkPrice => "mark_price",
            Self::OpenInterest => "open_interest",
            Self::Liquidations => "liquidations",
        }
    }
}

/// Handling policy when CHD order book update IDs have gaps.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
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
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum GapPolicy {
    /// Return an error when a sequence gap is detected.
    #[default]
    Error,
    /// Log a warning and continue decoding.
    Warn,
    /// Skip rows in the gapped update.
    Skip,
}
