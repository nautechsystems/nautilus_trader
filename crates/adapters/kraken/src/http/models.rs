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

//! Re-exports of HTTP API models for backwards compatibility.
//!
//! Models are now organized by product type:
//! - Spot models: [`super::spot::models`]
//! - Futures models: [`super::futures::models`]

// Re-export Spot models
// Re-export Futures models
pub use super::{
    futures::models::{
        CancelledOrder, FuturesBatchOrderResponse, FuturesCancelAllOrdersResponse,
        FuturesCancelAllStatus, FuturesCancelOrderResponse, FuturesCancelStatus, FuturesCandle,
        FuturesCandlesResponse, FuturesEditOrderResponse, FuturesEditStatus, FuturesFill,
        FuturesFillsResponse, FuturesInstrument, FuturesInstrumentsResponse, FuturesMarginLevel,
        FuturesOpenOrder, FuturesOpenOrdersResponse, FuturesOpenPositionsResponse,
        FuturesOrderEvent, FuturesOrderEventsResponse, FuturesPosition, FuturesPublicExecution,
        FuturesPublicExecutionElement, FuturesPublicExecutionEvent, FuturesPublicExecutionWrapper,
        FuturesPublicExecutionsResponse, FuturesPublicOrder, FuturesSendOrderResponse,
        FuturesSendStatus, FuturesTicker, FuturesTickersResponse,
    },
    spot::models::{
        AssetPairInfo, AssetPairsResponse, KrakenResponse, OhlcData, OhlcResponse, OrderBookData,
        OrderBookLevel, OrderBookResponse, OrderDescription, ServerTime, SpotAddOrderResponse,
        SpotCancelOrderResponse, SpotClosedOrdersResult, SpotEditOrderResponse,
        SpotOpenOrdersResult, SpotOrder, SpotTrade, SpotTradesHistoryResult, SystemStatus,
        TickerInfo, TickerResponse, TradeData, TradesResponse, WebSocketToken,
    },
};
