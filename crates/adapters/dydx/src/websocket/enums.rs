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

//! Enums for dYdX WebSocket operations, channels, and message types.

use std::collections::HashMap;

use nautilus_model::{
    data::{Data, OrderBookDeltas},
    events::AccountState,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strum::{AsRefStr, Display, EnumString, FromRepr};

use super::{
    error::DydxWebSocketError,
    messages::{
        DydxOraclePriceMarket, DydxWsChannelBatchDataMsg, DydxWsChannelDataMsg, DydxWsConnectedMsg,
        DydxWsSubaccountsChannelData, DydxWsSubaccountsSubscribed, DydxWsSubscriptionMsg,
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
    /// Subscribe to a channel.
    Subscribe,
    /// Unsubscribe from a channel.
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
    /// Unknown/unrecognized channel type.
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
    /// Channel data update.
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

/// High level message emitted by the dYdX WebSocket client.
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
    /// Channel data update.
    ChannelData(DydxWsChannelDataMsg),
    /// Batch of channel data updates.
    ChannelBatchData(DydxWsChannelBatchDataMsg),
    /// Block height update from chain.
    BlockHeight(u64),
    /// Error received from the venue or client lifecycle.
    Error(DydxWebSocketError),
    /// Raw message payload that does not yet have a typed representation.
    Raw(Value),
    /// Notification that the underlying connection reconnected.
    Reconnected,
    /// Explicit pong event (text-based heartbeat acknowledgement).
    Pong,
}

/// Nautilus domain message emitted after parsing dYdX WebSocket events.
///
/// This enum contains fully-parsed Nautilus domain objects ready for consumption
/// by the Python layer without additional processing.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Market data (trades, quotes, bars).
    Data(Vec<Data>),
    /// Order book deltas.
    Deltas(Box<OrderBookDeltas>),
    /// Order status reports from subaccount stream.
    Order(Box<OrderStatusReport>),
    /// Fill reports from subaccount stream.
    Fill(Box<FillReport>),
    /// Position status reports from subaccount stream.
    Position(Box<PositionStatusReport>),
    /// Account state updates from subaccount stream.
    AccountState(Box<AccountState>),
    /// Raw subaccount subscription with full state (for execution client parsing).
    SubaccountSubscribed(Box<DydxWsSubaccountsSubscribed>),
    /// Raw subaccounts channel data (orders/fills) for execution client parsing.
    SubaccountsChannelData(Box<DydxWsSubaccountsChannelData>),
    /// Oracle price updates from markets channel (for execution client).
    OraclePrices(HashMap<String, DydxOraclePriceMarket>),
    /// Block height update from chain.
    BlockHeight(u64),
    /// Error message.
    Error(DydxWebSocketError),
    /// Reconnection notification.
    Reconnected,
}
