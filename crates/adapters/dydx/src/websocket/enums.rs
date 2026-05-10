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

//! Enums for dYdX WebSocket operations, channels, and message types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{AsRefStr, Display, EnumString, FromRepr};

use super::{
    error::DydxWebSocketError,
    messages::{
        DydxCandle, DydxMarketsContents, DydxOrderbookContents, DydxOrderbookSnapshotContents,
        DydxTradeContents, DydxWsConnectedMsg, DydxWsSubaccountsChannelData,
        DydxWsSubaccountsSubscribed, DydxWsSubscriptionMsg,
    },
};

/// WebSocket operation types for dYdX.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DydxWsOperation {
    /// Subscribes to a channel.
    Subscribe,
    /// Unsubscribes from a channel.
    Unsubscribe,
    /// Ping keepalive message.
    Ping,
    /// Pong response to ping.
    Pong,
}

/// dYdX WebSocket channel identifiers.
///
/// # References
///
/// <https://docs.dydx.trade/developers/indexer/websockets>
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DydxWsChannel {
    /// Market data for all markets.
    #[serde(rename = "v4_markets")]
    #[strum(serialize = "v4_markets")]
    Markets,
    /// Trade stream for specific market.
    #[serde(rename = "v4_trades")]
    #[strum(serialize = "v4_trades")]
    Trades,
    /// Order book snapshots and updates.
    #[serde(rename = "v4_orderbook")]
    #[strum(serialize = "v4_orderbook")]
    Orderbook,
    /// Candlestick/kline data.
    #[serde(rename = "v4_candles")]
    #[strum(serialize = "v4_candles")]
    Candles,
    /// Subaccount updates (orders, fills, positions).
    #[serde(rename = "v4_subaccounts")]
    #[strum(serialize = "v4_subaccounts")]
    Subaccounts,
    /// Parent subaccount updates (for isolated positions).
    #[serde(rename = "v4_parent_subaccounts")]
    #[strum(serialize = "v4_parent_subaccounts")]
    ParentSubaccounts,
    /// Block height updates from chain.
    #[serde(rename = "v4_block_height")]
    #[strum(serialize = "v4_block_height")]
    BlockHeight,
    /// Unknown/unrecognized channel type (default when field is missing).
    #[default]
    #[serde(other)]
    #[strum(to_string = "unknown")]
    Unknown,
}

impl DydxWsChannel {
    /// Returns `true` if this is a private channel requiring authentication.
    #[must_use]
    pub const fn is_private(&self) -> bool {
        matches!(self, Self::Subaccounts | Self::ParentSubaccounts)
    }

    /// Returns `true` if this is a public channel.
    #[must_use]
    pub const fn is_public(&self) -> bool {
        !self.is_private()
    }

    /// Returns `true` if this is an unknown/unrecognized channel type.
    #[must_use]
    pub const fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }
}

/// WebSocket message types for dYdX.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Display,
    AsRefStr,
    EnumString,
    FromRepr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DydxWsMessageType {
    /// Connection established.
    Connected,
    /// Subscription confirmed.
    Subscribed,
    /// Unsubscription confirmed.
    Unsubscribed,
    /// Channel data update (default for missing type field).
    #[default]
    ChannelData,
    /// Batch channel data update.
    ChannelBatchData,
    /// Error message.
    Error,
    /// Unknown/unrecognized message type.
    #[serde(other)]
    #[strum(to_string = "unknown")]
    Unknown,
}

/// Control messages for the fallback parsing path.
///
/// Channel data is handled directly via `DydxWsFeedMessage` in `handle_feed_message()`.
#[derive(Debug, Clone)]
pub enum DydxWsMessage {
    /// Subscription acknowledgement.
    Subscribed(DydxWsSubscriptionMsg),
    /// Unsubscription acknowledgement.
    Unsubscribed(DydxWsSubscriptionMsg),
    /// Subaccounts subscription with initial account state.
    SubaccountsSubscribed(DydxWsSubaccountsSubscribed),
    /// Connected acknowledgement with connection_id.
    Connected(DydxWsConnectedMsg),
    /// Error received from the venue or client lifecycle.
    Error(DydxWebSocketError),
    /// Raw message payload that does not yet have a typed representation.
    Raw(Value),
    /// Notification that the underlying connection reconnected.
    Reconnected,
    /// Explicit pong event (text-based heartbeat acknowledgement).
    Pong,
}

/// Venue-specific message emitted by the handler to consumers.
///
/// The handler deserializes raw WebSocket JSON into these typed variants
/// without converting to Nautilus domain types. Consumers (data client,
/// execution client, Python bindings) perform the final conversion using
/// their own instrument caches.
#[derive(Debug, Clone)]
pub enum DydxWsOutputMessage {
    /// Trade data for a market.
    Trades {
        id: String,
        contents: DydxTradeContents,
    },
    /// Order book snapshot (initial subscription).
    OrderbookSnapshot {
        id: String,
        contents: DydxOrderbookSnapshotContents,
    },
    /// Order book delta update.
    OrderbookUpdate {
        id: String,
        contents: DydxOrderbookContents,
    },
    /// Order book batch update (multiple deltas).
    OrderbookBatch {
        id: String,
        updates: Vec<DydxOrderbookContents>,
    },
    /// Candle data for a market.
    Candles { id: String, contents: DydxCandle },
    /// Markets channel data (oracle prices, trading, instrument status).
    Markets(DydxMarketsContents),
    /// Subaccount subscription with initial account state.
    SubaccountSubscribed(Box<DydxWsSubaccountsSubscribed>),
    /// Subaccount channel data (orders, fills).
    SubaccountsChannelData(Box<DydxWsSubaccountsChannelData>),
    /// Block height update from chain.
    BlockHeight { height: u64, time: DateTime<Utc> },
    /// Error from the venue or handler.
    Error(DydxWebSocketError),
    /// Reconnection notification.
    Reconnected,
}
