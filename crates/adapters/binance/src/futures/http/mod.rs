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

//! Binance Futures HTTP client with JSON encoding.

pub mod client;
pub mod error;
pub mod models;
pub mod query;

pub use client::{
    BinanceFundingRate, BinanceFuturesHttpClient, BinanceFuturesInstrument, BinanceOpenInterest,
    BinanceRawFuturesHttpClient, FundingRateParams, ListenKeyResponse, MarkPriceParams,
    OpenInterestParams,
};
pub use error::{BinanceFuturesHttpError, BinanceFuturesHttpResult};
pub use models::{
    BinanceBookTicker, BinanceFuturesAsset, BinanceFuturesCoinExchangeInfo,
    BinanceFuturesCoinSymbol, BinanceFuturesMarkPrice, BinanceFuturesOrder,
    BinanceFuturesTicker24hr, BinanceFuturesUsdExchangeInfo, BinanceFuturesUsdSymbol,
    BinanceOrderBook, BinancePriceTicker, BinanceServerTime,
};
pub use query::{
    BinanceBookTickerParams, BinanceDepthParams, BinanceFundingRateParams,
    BinanceIncomeHistoryParams, BinanceMarkPriceParams, BinanceOpenInterestParams,
    BinanceOpenOrdersParams, BinanceOrderQueryParams, BinancePositionRiskParams,
    BinanceTicker24hrParams, BinanceUserTradesParams,
};
