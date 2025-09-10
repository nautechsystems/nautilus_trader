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

//! Enumerations for Delta Exchange integration.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Represents the product types available on Delta Exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeltaExchangeProductType {
    /// Perpetual futures contracts.
    #[serde(rename = "perpetual_futures")]
    PerpetualFutures,
    /// Call options contracts.
    #[serde(rename = "call_options")]
    CallOptions,
    /// Put options contracts.
    #[serde(rename = "put_options")]
    PutOptions,
}

/// Represents order types supported by Delta Exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeltaExchangeOrderType {
    /// Limit order.
    #[serde(rename = "limit_order")]
    LimitOrder,
    /// Market order.
    #[serde(rename = "market_order")]
    MarketOrder,
    /// Stop loss order.
    #[serde(rename = "stop_loss_order")]
    StopLossOrder,
    /// Take profit order.
    #[serde(rename = "take_profit_order")]
    TakeProfitOrder,
}

/// Represents order sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeltaExchangeSide {
    /// Buy side.
    Buy,
    /// Sell side.
    Sell,
}

/// Represents time in force options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeltaExchangeTimeInForce {
    /// Good Till Cancel.
    #[serde(rename = "gtc")]
    Gtc,
    /// Immediate or Cancel.
    #[serde(rename = "ioc")]
    Ioc,
}

/// Represents order states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeltaExchangeOrderState {
    /// Order is open and active.
    Open,
    /// Order is pending execution.
    Pending,
    /// Order is closed/filled.
    Closed,
    /// Order is cancelled.
    Cancelled,
}

/// Represents trading states for products.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeltaExchangeTradingState {
    /// Normal trading operations.
    Operational,
    /// Market disrupted, cancel only mode.
    #[serde(rename = "disrupted_cancel_only")]
    DisruptedCancelOnly,
    /// Market disrupted, post only mode.
    #[serde(rename = "disrupted_post_only")]
    DisruptedPostOnly,
}

/// Represents asset status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "UPPERCASE")]
#[strum(serialize_all = "UPPERCASE")]
pub enum DeltaExchangeAssetStatus {
    /// Asset is active and tradeable.
    Active,
    /// Asset is inactive.
    Inactive,
}

/// Represents WebSocket channel types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeltaExchangeChannel {
    // Public channels
    /// Version 2 ticker data.
    #[serde(rename = "v2_ticker")]
    V2Ticker,
    /// Level 1 order book.
    #[serde(rename = "l1_orderbook")]
    L1Orderbook,
    /// Level 2 order book.
    #[serde(rename = "l2_orderbook")]
    L2Orderbook,
    /// Level 2 order book updates.
    #[serde(rename = "l2_updates")]
    L2Updates,
    /// All public trades.
    #[serde(rename = "all_trades")]
    AllTrades,
    /// Mark price updates.
    #[serde(rename = "mark_price")]
    MarkPrice,
    /// Candlestick data.
    Candlesticks,
    /// Spot price updates.
    #[serde(rename = "spot_price")]
    SpotPrice,
    /// Version 2 spot price.
    #[serde(rename = "v2/spot_price")]
    V2SpotPrice,
    /// 30-minute TWAP spot price.
    #[serde(rename = "spot_30mtwap_price")]
    Spot30mtwapPrice,
    /// Funding rate updates.
    #[serde(rename = "funding_rate")]
    FundingRate,
    /// Product updates (market disruptions, auctions).
    #[serde(rename = "product_updates")]
    ProductUpdates,
    /// System announcements.
    Announcements,

    // Private channels
    /// Margin/wallet updates.
    Margins,
    /// Position updates.
    Positions,
    /// Order updates.
    Orders,
    /// User trade updates.
    #[serde(rename = "user_trades")]
    UserTrades,
    /// Version 2 user trades (faster).
    #[serde(rename = "v2/user_trades")]
    V2UserTrades,
    /// Portfolio margin updates.
    #[serde(rename = "portfolio_margins")]
    PortfolioMargins,
    /// Market maker protection trigger.
    #[serde(rename = "mmp_trigger")]
    MmpTrigger,
}

/// Represents candlestick resolutions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum DeltaExchangeResolution {
    /// 1 minute.
    #[serde(rename = "1m")]
    OneMinute,
    /// 3 minutes.
    #[serde(rename = "3m")]
    ThreeMinutes,
    /// 5 minutes.
    #[serde(rename = "5m")]
    FiveMinutes,
    /// 15 minutes.
    #[serde(rename = "15m")]
    FifteenMinutes,
    /// 30 minutes.
    #[serde(rename = "30m")]
    ThirtyMinutes,
    /// 1 hour.
    #[serde(rename = "1h")]
    OneHour,
    /// 2 hours.
    #[serde(rename = "2h")]
    TwoHours,
    /// 4 hours.
    #[serde(rename = "4h")]
    FourHours,
    /// 6 hours.
    #[serde(rename = "6h")]
    SixHours,
    /// 12 hours.
    #[serde(rename = "12h")]
    TwelveHours,
    /// 1 day.
    #[serde(rename = "1d")]
    OneDay,
    /// 1 week.
    #[serde(rename = "1w")]
    OneWeek,
    /// 2 weeks.
    #[serde(rename = "2w")]
    TwoWeeks,
    /// 30 days.
    #[serde(rename = "30d")]
    ThirtyDays,
}

/// Represents order event types for WebSocket updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeltaExchangeOrderEventType {
    /// Order created.
    Create,
    /// Order updated.
    Update,
    /// Order deleted/cancelled.
    Delete,
    /// Order filled.
    Fill,
    /// Stop order updated.
    #[serde(rename = "stop_update")]
    StopUpdate,
    /// Stop order triggered.
    #[serde(rename = "stop_trigger")]
    StopTrigger,
    /// Stop order cancelled.
    #[serde(rename = "stop_cancel")]
    StopCancel,
    /// Order liquidated.
    Liquidation,
    /// Self trade occurred.
    #[serde(rename = "self_trade")]
    SelfTrade,
}

/// Represents position event types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeltaExchangePositionEventType {
    /// Position created.
    Create,
    /// Position updated.
    Update,
    /// Position deleted/closed.
    Delete,
    /// Auto top-up occurred.
    #[serde(rename = "auto_topup")]
    AutoTopup,
}
