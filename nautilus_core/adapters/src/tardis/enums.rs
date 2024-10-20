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

use serde::{Deserialize, Serialize};

use super::message::{
    BarMsg, BookChangeMsg, BookSnapshotMsg, DerivativeTickerMsg, DisconnectMsg, TradeMsg,
};

/// A Tardis Machine Server message type.
#[allow(missing_docs)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum WsMessage {
    Trade(TradeMsg),
    BookChange(BookChangeMsg),
    BookSnapshot(BookSnapshotMsg),
    DerivativeTicker(DerivativeTickerMsg),
    Bar(BarMsg),
    Disconnect(DisconnectMsg),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// The type of the symbol eg. Spot, Perpetual, Future, Option.
pub enum SymbolType {
    Spot,
    Perpetual,
    Future,
    Option,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
/// The type of an option symbol.
pub enum OptionType {
    Call,
    Put,
}

/// The aggressor side of the trade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TradeSide {
    Buy,
    Sell,
    Unknown,
}

/// The kind of bar.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BarKind {
    Time,
    Volume,
    Tick,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
