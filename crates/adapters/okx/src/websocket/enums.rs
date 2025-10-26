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

//! Enumerations representing OKX WebSocket message metadata and codes.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Error types that can be returned by the WebSocket API.
#[derive(
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum OKXWsErrorType {
    /// General error.
    Error,
    /// Error during subscription.
    SubscriptionError,
    /// Error during unsubscription.
    UnsubscriptionError,
    /// Authentication failure.
    AuthenticationError,
    /// Rate limit exceeded.
    RateLimit,
}

/// Operation type for WebSocket commands.
#[derive(
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum OKXWsOperation {
    /// Subscribes to one or more topics.
    Subscribe,
    /// Unsubscribe from one or more topics.
    Unsubscribe,
    /// Place a new order.
    Order,
    /// Cancel an existing order.
    CancelOrder,
    /// Amend an existing order.
    AmendOrder,
    /// Place multiple orders.
    BatchOrders,
    /// Cancel multiple orders.
    BatchCancelOrders,
    /// Amend multiple orders.
    BatchAmendOrders,
    /// Mass cancel all orders for an instrument.
    MassCancel,
    /// Place a new algo order.
    OrderAlgo,
    /// Cancel algo orders.
    CancelAlgos,
}

#[derive(
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum OKXSubscriptionEvent {
    Subscribe,
    Unsubscribe,
}

#[derive(
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum OKXWsChannel {
    // Public Channels
    #[serde(rename = "instruments")]
    Instruments,
    #[serde(rename = "tickers")]
    Tickers,
    #[serde(rename = "open-interest")]
    OpenInterest,
    #[serde(rename = "trades")]
    Trades,
    #[serde(rename = "trades-all")]
    TradesAll,
    #[serde(rename = "estimated-price")]
    EstimatedPrice,
    #[serde(rename = "mark-price")]
    MarkPrice,
    #[serde(rename = "price-limit")]
    PriceLimit,
    #[serde(rename = "books")]
    Books, // Default depth
    #[serde(rename = "books5")]
    Books5,
    #[serde(rename = "books50-l2-tbt")]
    Books50Tbt,
    #[serde(rename = "books-l2-tbt")]
    BooksTbt,
    #[serde(rename = "bbo-tbt")]
    BboTbt,
    #[serde(rename = "opt-summary")]
    OptionSummary,
    #[serde(rename = "funding-rate")]
    FundingRate,
    #[serde(rename = "index-tickers")]
    IndexTickers,
    #[serde(rename = "status")]
    Status,
    // Private Channels
    #[serde(rename = "account")]
    Account,
    // #[display(fmt = "positions")]
    // Positions,
    // #[display(fmt = "balance_and_position")]
    // BalanceAndPosition,
    #[serde(rename = "orders")]
    Orders,
    #[serde(rename = "fills")]
    Fills,
    #[serde(rename = "orders-algo")]
    OrdersAlgo,
    // #[display(fmt = "algo-advance")]
    // AlgoAdvance,
    // #[display(fmt = "liquidation-warning")]
    // LiquidationWarning,
    // #[display(fmt = "account-greeks")]
    // AccountGreeks,
    // #[display(fmt = "rfqs")]
    // Rfqs,
    // #[serde(rename = "quotes")]
    // Quotes,
    // #[display(fmt = "structure-block-trades")]
    // StructuredBlock,
    // #[display(fmt = "spreads")]
    // Spreads,
    //
    // // Trading Channels
    // #[display(fmt = "orders-spot-margin")]
    // SpotMarginOrders,
    // #[display(fmt = "orders-futures")]
    // FuturesOrders,
    // #[display(fmt = "orders-swap")]
    // SwapOrders,
    // #[display(fmt = "orders-option")]
    // OptionOrders,
    //
    // // Business Channels
    // #[display(fmt = "deposit-info")]
    // DepositInfo,
    // #[display(fmt = "withdrawal-info")]
    // WithdrawalInfo,

    // Candlesticks
    #[serde(rename = "candle1s")]
    Candle1Second,
    #[serde(rename = "candle1m")]
    Candle1Minute,
    #[serde(rename = "candle3m")]
    Candle3Minute,
    #[serde(rename = "candle5m")]
    Candle5Minute,
    #[serde(rename = "candle15m")]
    Candle15Minute,
    #[serde(rename = "candle30m")]
    Candle30Minute,
    #[serde(rename = "candle1H")]
    Candle1Hour,
    #[serde(rename = "candle2H")]
    Candle2Hour,
    #[serde(rename = "candle4H")]
    Candle4Hour,
    #[serde(rename = "candle6H")]
    Candle6Hour,
    #[serde(rename = "candle12H")]
    Candle12Hour,
    #[serde(rename = "candle1D")]
    Candle1Day,
    #[serde(rename = "candle2D")]
    Candle2Day,
    #[serde(rename = "candle3D")]
    Candle3Day,
    #[serde(rename = "candle5D")]
    Candle5Day,
    #[serde(rename = "candle1W")]
    Candle1Week,
    #[serde(rename = "candle1M")]
    Candle1Month,
    #[serde(rename = "candle3M")]
    Candle3Month,
    #[serde(rename = "candle6M")]
    Candle6Month,
    #[serde(rename = "candle1Y")]
    Candle1Year,

    // Mark Price Candlesticks
    #[serde(rename = "mark-price-candle1s")]
    MarkPriceCandle1Second,
    #[serde(rename = "mark-price-candle1m")]
    MarkPriceCandle1Minute,
    #[serde(rename = "mark-price-candle3m")]
    MarkPriceCandle3Minute,
    #[serde(rename = "mark-price-candle5m")]
    MarkPriceCandle5Minute,
    #[serde(rename = "mark-price-candle15m")]
    MarkPriceCandle15Minute,
    #[serde(rename = "mark-price-candle30m")]
    MarkPriceCandle30Minute,
    #[serde(rename = "mark-price-candle1H")]
    MarkPriceCandle1Hour,
    #[serde(rename = "mark-price-candle2H")]
    MarkPriceCandle2Hour,
    #[serde(rename = "mark-price-candle4H")]
    MarkPriceCandle4Hour,
    #[serde(rename = "mark-price-candle6H")]
    MarkPriceCandle6Hour,
    #[serde(rename = "mark-price-candle12H")]
    MarkPriceCandle12Hour,
    #[serde(rename = "mark-price-candle1D")]
    MarkPriceCandle1Day,
    #[serde(rename = "mark-price-candle2D")]
    MarkPriceCandle2Day,
    #[serde(rename = "mark-price-candle3D")]
    MarkPriceCandle3Day,
    #[serde(rename = "mark-price-candle5D")]
    MarkPriceCandle5Day,
    #[serde(rename = "mark-price-candle1W")]
    MarkPriceCandle1Week,
    #[serde(rename = "mark-price-candle1M")]
    MarkPriceCandle1Month,
    #[serde(rename = "mark-price-candle3M")]
    MarkPriceCandle3Month,
}
