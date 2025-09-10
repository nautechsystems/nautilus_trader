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

//! Enumerations for Delta Exchange WebSocket integration.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Represents WebSocket connection states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display)]
pub enum ConnectionState {
    /// Connection is being established.
    Connecting,
    /// Connection is established and ready.
    Connected,
    /// Connection is being closed.
    Disconnecting,
    /// Connection is closed.
    Disconnected,
    /// Connection failed and is being retried.
    Reconnecting,
    /// Connection permanently failed.
    Failed,
}

/// Represents WebSocket operations for subscription management.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum WsOperation {
    /// Subscribe to channels.
    Subscribe,
    /// Unsubscribe from channels.
    Unsubscribe,
    /// Authentication message.
    Auth,
}

/// Represents Delta Exchange WebSocket channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum DeltaExchangeWsChannel {
    // Public channels
    /// Version 2 ticker data.
    #[serde(rename = "v2_ticker")]
    V2Ticker,
    /// Level 1 order book (best bid/ask).
    #[serde(rename = "l1_orderbook")]
    L1Orderbook,
    /// Level 2 order book (full depth).
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

impl DeltaExchangeWsChannel {
    /// Check if the channel requires authentication.
    pub fn requires_auth(&self) -> bool {
        matches!(
            self,
            Self::Margins
                | Self::Positions
                | Self::Orders
                | Self::UserTrades
                | Self::V2UserTrades
                | Self::PortfolioMargins
                | Self::MmpTrigger
        )
    }

    /// Check if the channel is public.
    pub fn is_public(&self) -> bool {
        !self.requires_auth()
    }

    /// Get all public channels.
    pub fn public_channels() -> Vec<Self> {
        vec![
            Self::V2Ticker,
            Self::L1Orderbook,
            Self::L2Orderbook,
            Self::L2Updates,
            Self::AllTrades,
            Self::MarkPrice,
            Self::Candlesticks,
            Self::SpotPrice,
            Self::V2SpotPrice,
            Self::Spot30mtwapPrice,
            Self::FundingRate,
            Self::ProductUpdates,
            Self::Announcements,
        ]
    }

    /// Get all private channels.
    pub fn private_channels() -> Vec<Self> {
        vec![
            Self::Margins,
            Self::Positions,
            Self::Orders,
            Self::UserTrades,
            Self::V2UserTrades,
            Self::PortfolioMargins,
            Self::MmpTrigger,
        ]
    }
}

/// Represents WebSocket message types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WsMessageType {
    /// Subscription confirmation.
    Subscriptions,
    /// Authentication response.
    Auth,
    /// Data update message.
    Update,
    /// Snapshot message.
    Snapshot,
    /// Error message.
    Error,
    /// Heartbeat/ping message.
    Ping,
    /// Pong response.
    Pong,
}

/// Represents subscription states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display)]
pub enum SubscriptionState {
    /// Subscription request sent, waiting for confirmation.
    Pending,
    /// Subscription confirmed and active.
    Active,
    /// Unsubscription request sent.
    Unsubscribing,
    /// Subscription failed.
    Failed,
    /// Subscription inactive.
    Inactive,
}

/// Represents reconnection strategies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Display)]
pub enum ReconnectionStrategy {
    /// No automatic reconnection.
    None,
    /// Immediate reconnection.
    Immediate,
    /// Exponential backoff reconnection.
    ExponentialBackoff,
    /// Fixed interval reconnection.
    FixedInterval,
}

/// Represents message priority levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Display)]
pub enum MessagePriority {
    /// Low priority messages (market data).
    Low = 0,
    /// Normal priority messages.
    Normal = 1,
    /// High priority messages (order updates).
    High = 2,
    /// Critical priority messages (authentication, errors).
    Critical = 3,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_auth_requirements() {
        assert!(DeltaExchangeWsChannel::Orders.requires_auth());
        assert!(DeltaExchangeWsChannel::Positions.requires_auth());
        assert!(DeltaExchangeWsChannel::UserTrades.requires_auth());
        assert!(DeltaExchangeWsChannel::Margins.requires_auth());

        assert!(!DeltaExchangeWsChannel::V2Ticker.requires_auth());
        assert!(!DeltaExchangeWsChannel::L2Orderbook.requires_auth());
        assert!(!DeltaExchangeWsChannel::AllTrades.requires_auth());
    }

    #[test]
    fn test_channel_public_check() {
        assert!(DeltaExchangeWsChannel::V2Ticker.is_public());
        assert!(DeltaExchangeWsChannel::L2Updates.is_public());
        assert!(DeltaExchangeWsChannel::MarkPrice.is_public());

        assert!(!DeltaExchangeWsChannel::Orders.is_public());
        assert!(!DeltaExchangeWsChannel::Positions.is_public());
        assert!(!DeltaExchangeWsChannel::UserTrades.is_public());
    }

    #[test]
    fn test_public_channels_list() {
        let public_channels = DeltaExchangeWsChannel::public_channels();
        assert!(!public_channels.is_empty());
        assert!(public_channels.contains(&DeltaExchangeWsChannel::V2Ticker));
        assert!(public_channels.contains(&DeltaExchangeWsChannel::L2Orderbook));
        assert!(!public_channels.contains(&DeltaExchangeWsChannel::Orders));
    }

    #[test]
    fn test_private_channels_list() {
        let private_channels = DeltaExchangeWsChannel::private_channels();
        assert!(!private_channels.is_empty());
        assert!(private_channels.contains(&DeltaExchangeWsChannel::Orders));
        assert!(private_channels.contains(&DeltaExchangeWsChannel::Positions));
        assert!(!private_channels.contains(&DeltaExchangeWsChannel::V2Ticker));
    }

    #[test]
    fn test_connection_state_display() {
        assert_eq!(ConnectionState::Connecting.to_string(), "Connecting");
        assert_eq!(ConnectionState::Connected.to_string(), "Connected");
        assert_eq!(ConnectionState::Disconnected.to_string(), "Disconnected");
    }

    #[test]
    fn test_message_priority_ordering() {
        assert!(MessagePriority::Critical > MessagePriority::High);
        assert!(MessagePriority::High > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
    }

    #[test]
    fn test_ws_operation_serialization() {
        let subscribe = WsOperation::Subscribe;
        let json = serde_json::to_string(&subscribe).unwrap();
        assert_eq!(json, "\"subscribe\"");

        let unsubscribe: WsOperation = serde_json::from_str("\"unsubscribe\"").unwrap();
        assert_eq!(unsubscribe, WsOperation::Unsubscribe);
    }
}
