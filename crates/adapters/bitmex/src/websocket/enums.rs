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

//! Enumerations used when parsing BitMEX WebSocket payloads.

use nautilus_model::enums::{AggressorSide, BookAction, OrderSide};
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Side of an order or trade.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum BitmexSide {
    /// Buy side of the trade/order.
    Buy,
    /// Sell side of the trade/order.
    Sell,
}

impl BitmexSide {
    /// Converts the BitMEX side into a Nautilus order side.
    #[must_use]
    pub const fn as_order_side(&self) -> OrderSide {
        match self {
            Self::Buy => OrderSide::Buy,
            Self::Sell => OrderSide::Sell,
        }
    }
    /// Converts the BitMEX side into a Nautilus aggressor side.
    #[must_use]
    pub const fn as_aggressor_side(&self) -> AggressorSide {
        match self {
            Self::Buy => AggressorSide::Buyer,
            Self::Sell => AggressorSide::Seller,
        }
    }
}

impl From<BitmexSide> for crate::common::enums::BitmexSide {
    fn from(side: BitmexSide) -> Self {
        match side {
            BitmexSide::Buy => Self::Buy,
            BitmexSide::Sell => Self::Sell,
        }
    }
}

/// Direction of price tick relative to previous trade.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
pub enum BitmexTickDirection {
    /// Price higher than previous trade.
    PlusTick,
    /// Price lower than previous trade.
    MinusTick,
    /// Price equal to previous trade, which was higher than the trade before it.
    ZeroPlusTick,
    /// Price equal to previous trade, which was lower than the trade before it.
    ZeroMinusTick,
}

/// Trading instrument state.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum BitmexInstrumentState {
    /// Instrument is available for trading.
    Open,
    /// Instrument is not currently trading.
    Closed,
    /// Instrument is in settlement.
    Settling,
    /// Instrument is not listed.
    Unlisted,
}

/// Action type for table data messages.
#[derive(
    Copy,
    Clone,
    Debug,
    Display,
    PartialEq,
    Eq,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum BitmexAction {
    /// Initial snapshot of table data.
    Partial,
    /// New data inserted.
    Insert,
    /// Update to existing data.
    Update,
    /// Existing data deleted.
    Delete,
}

impl BitmexAction {
    /// Maps a table action into the corresponding order book action.
    #[must_use]
    pub const fn as_book_action(&self) -> BookAction {
        match self {
            Self::Partial => BookAction::Add,
            Self::Insert => BookAction::Add,
            Self::Update => BookAction::Update,
            Self::Delete => BookAction::Delete,
        }
    }
}

/// Operation type for WebSocket commands.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum BitmexWsOperation {
    /// Subscribe to one or more topics.
    Subscribe,
    /// Unsubscribe from one or more topics.
    Unsubscribe,
}

/// Authentication action types for WebSocket commands.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum BitmexWsAuthAction {
    /// Submit API key for authentication (legacy, deprecated).
    AuthKey,
    /// Submit API key with expires for authentication (recommended).
    AuthKeyExpires,
    /// Cancel all orders after n seconds.
    CancelAllAfter,
}

/// Represents possible WebSocket topics that can be subscribed to.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum BitmexWsTopic {
    /// Site announcements.
    Announcement,
    /// Trollbox chat.
    Chat,
    /// Statistics of connected users/bots.
    Connected,
    /// Updates of swap funding rates.
    Funding,
    /// Instrument updates including mark and index prices.
    Instrument,
    /// Daily insurance fund updates.
    Insurance,
    /// Liquidation orders as they're entered into the book.
    Liquidation,
    /// Settlement price updates.
    Settlement,
    /// Full level 2 orderbook.
    OrderBookL2,
    /// Top 25 levels of level 2 orderbook.
    #[serde(rename = "orderBookL2_25")]
    #[strum(to_string = "orderBookL2_25")]
    OrderBookL2_25,
    /// Top 10 levels using traditional full book push.
    OrderBook10,
    /// System announcements.
    PublicNotifications,
    /// Top level of the book.
    Quote,
    /// 1-minute quote bins.
    QuoteBin1m,
    /// 5-minute quote bins.
    QuoteBin5m,
    /// 1-hour quote bins.
    QuoteBin1h,
    /// 1-day quote bins.
    QuoteBin1d,
    /// Live trades.
    Trade,
    /// 1-minute trade bins.
    TradeBin1m,
    /// 5-minute trade bins.
    TradeBin5m,
    /// 1-hour trade bins.
    TradeBin1h,
    /// 1-day trade bins.
    TradeBin1d,
}

/// Represents authenticated WebSocket channels for account updates.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, AsRefStr, EnumIter, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum BitmexWsAuthChannel {
    /// Order updates for the authenticated account.
    Order,
    /// Execution/fill updates for the authenticated account.
    Execution,
    /// Position updates for the authenticated account.
    Position,
    /// Margin updates for the authenticated account.
    Margin,
    /// Wallet updates for the authenticated account.
    Wallet,
}
