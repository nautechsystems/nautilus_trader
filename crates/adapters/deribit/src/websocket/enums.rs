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

//! Enumerations for Deribit WebSocket channels and operations.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// Deribit data stream update intervals.
///
/// Controls how frequently updates are sent for subscribed channels.
/// Raw updates require authentication while aggregated updates are public.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    AsRefStr,
    EnumIter,
    EnumString,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub enum DeribitUpdateInterval {
    /// Raw updates - immediate delivery of each event.
    /// Requires authentication.
    Raw,
    /// Aggregated updates every 100 milliseconds (default).
    #[default]
    Ms100,
    /// Aggregated updates every 2 ticks.
    Agg2,
}

impl DeribitUpdateInterval {
    /// Returns the string representation for Deribit channel subscription.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Ms100 => "100ms",
            Self::Agg2 => "agg2",
        }
    }

    /// Returns whether this interval requires authentication.
    #[must_use]
    pub const fn requires_auth(&self) -> bool {
        matches!(self, Self::Raw)
    }
}

impl std::fmt::Display for DeribitUpdateInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Deribit WebSocket public data channels.
///
/// Channels follow the format: `{channel_type}.{instrument_or_currency}.{interval}`
#[derive(
    Clone,
    Copy,
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
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub enum DeribitWsChannel {
    // Public Market Data Channels
    /// Raw trade stream: `trades.{instrument}.raw`
    Trades,
    /// Order book updates: `book.{instrument}.{group}.{depth}.{interval}`
    Book,
    /// Ticker updates: `ticker.{instrument}.{interval}`
    Ticker,
    /// Quote updates (best bid/ask): `quote.{instrument}`
    Quote,
    /// Index price: `deribit_price_index.{currency}`
    PriceIndex,
    /// Price ranking: `deribit_price_ranking.{currency}`
    PriceRanking,
    /// Volatility index: `deribit_volatility_index.{currency}`
    VolatilityIndex,
    /// Estimated expiration price: `estimated_expiration_price.{currency}`
    EstimatedExpirationPrice,
    /// Perpetual interest rate: `perpetual.{instrument}.{interval}`
    Perpetual,
    /// Mark price options: `markprice.options.{currency}`
    MarkPriceOptions,
    /// Platform state: `platform_state`
    PlatformState,
    /// Announcements: `announcements`
    Announcements,
    /// Chart trades: `chart.trades.{instrument}.{resolution}`
    ChartTrades,

    // Private User Channels (for future execution support)
    /// User orders: `user.orders.{instrument}.{interval}`
    UserOrders,
    /// User trades/fills: `user.trades.{instrument}.{interval}`
    UserTrades,
    /// User portfolio: `user.portfolio.{currency}`
    UserPortfolio,
    /// User changes (combined orders/trades/positions): `user.changes.{instrument}.{interval}`
    UserChanges,
    /// User access log: `user.access_log`
    UserAccessLog,
}

impl DeribitWsChannel {
    /// Formats the channel name for subscription with the given instrument or currency.
    ///
    /// Returns the full channel string for Deribit subscription.
    ///
    /// # Arguments
    ///
    /// * `instrument_or_currency` - The instrument name (e.g., "BTC-PERPETUAL") or currency (e.g., "BTC")
    /// * `interval` - Optional update interval. Defaults to `Ms100` (100ms) if not specified.
    ///
    /// # Note
    ///
    /// `Raw` subscriptions require authentication. Use `Ms100` for public/unauthenticated access.
    #[must_use]
    pub fn format_channel(
        &self,
        instrument_or_currency: &str,
        interval: Option<DeribitUpdateInterval>,
    ) -> String {
        let interval_str = interval.unwrap_or_default().as_str();
        match self {
            Self::Trades => format!("trades.{instrument_or_currency}.{interval_str}"),
            Self::Book => format!("book.{instrument_or_currency}.{interval_str}"),
            Self::Ticker => format!("ticker.{instrument_or_currency}.{interval_str}"),
            Self::Quote => format!("quote.{instrument_or_currency}"),
            Self::PriceIndex => format!("deribit_price_index.{instrument_or_currency}"),
            Self::PriceRanking => format!("deribit_price_ranking.{instrument_or_currency}"),
            Self::VolatilityIndex => format!("deribit_volatility_index.{instrument_or_currency}"),
            Self::EstimatedExpirationPrice => {
                format!("estimated_expiration_price.{instrument_or_currency}")
            }
            Self::Perpetual => format!("perpetual.{instrument_or_currency}.{interval_str}"),
            Self::MarkPriceOptions => format!("markprice.options.{instrument_or_currency}"),
            Self::PlatformState => "platform_state".to_string(),
            Self::Announcements => "announcements".to_string(),
            Self::ChartTrades => format!("chart.trades.{instrument_or_currency}.{interval_str}"),
            Self::UserOrders => format!("user.orders.{instrument_or_currency}.{interval_str}"),
            Self::UserTrades => format!("user.trades.{instrument_or_currency}.{interval_str}"),
            Self::UserPortfolio => format!("user.portfolio.{instrument_or_currency}"),
            Self::UserChanges => format!("user.changes.{instrument_or_currency}.{interval_str}"),
            Self::UserAccessLog => "user.access_log".to_string(),
        }
    }

    /// Parses a channel string to extract the channel type.
    ///
    /// Returns the channel enum variant if recognized.
    #[must_use]
    pub fn from_channel_string(channel: &str) -> Option<Self> {
        if channel.starts_with("trades.") {
            Some(Self::Trades)
        } else if channel.starts_with("book.") {
            Some(Self::Book)
        } else if channel.starts_with("ticker.") {
            Some(Self::Ticker)
        } else if channel.starts_with("quote.") {
            Some(Self::Quote)
        } else if channel.starts_with("deribit_price_index.") {
            Some(Self::PriceIndex)
        } else if channel.starts_with("deribit_price_ranking.") {
            Some(Self::PriceRanking)
        } else if channel.starts_with("deribit_volatility_index.") {
            Some(Self::VolatilityIndex)
        } else if channel.starts_with("estimated_expiration_price.") {
            Some(Self::EstimatedExpirationPrice)
        } else if channel.starts_with("perpetual.") {
            Some(Self::Perpetual)
        } else if channel.starts_with("markprice.options.") {
            Some(Self::MarkPriceOptions)
        } else if channel == "platform_state" {
            Some(Self::PlatformState)
        } else if channel == "announcements" {
            Some(Self::Announcements)
        } else if channel.starts_with("chart.trades.") {
            Some(Self::ChartTrades)
        } else if channel.starts_with("user.orders.") {
            Some(Self::UserOrders)
        } else if channel.starts_with("user.trades.") {
            Some(Self::UserTrades)
        } else if channel.starts_with("user.portfolio.") {
            Some(Self::UserPortfolio)
        } else if channel.starts_with("user.changes.") {
            Some(Self::UserChanges)
        } else if channel == "user.access_log" {
            Some(Self::UserAccessLog)
        } else {
            None
        }
    }

    /// Returns whether this is a private (authenticated) channel.
    #[must_use]
    pub const fn is_private(&self) -> bool {
        matches!(
            self,
            Self::UserOrders
                | Self::UserTrades
                | Self::UserPortfolio
                | Self::UserChanges
                | Self::UserAccessLog
        )
    }
}

/// Deribit JSON-RPC WebSocket methods.
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
pub enum DeribitWsMethod {
    // Public methods
    /// Subscribe to public channels.
    #[serde(rename = "public/subscribe")]
    #[strum(serialize = "public/subscribe")]
    PublicSubscribe,
    /// Unsubscribe from public channels.
    #[serde(rename = "public/unsubscribe")]
    #[strum(serialize = "public/unsubscribe")]
    PublicUnsubscribe,
    /// Authenticate with API credentials.
    #[serde(rename = "public/auth")]
    #[strum(serialize = "public/auth")]
    PublicAuth,
    /// Enable heartbeat mechanism.
    #[serde(rename = "public/set_heartbeat")]
    #[strum(serialize = "public/set_heartbeat")]
    SetHeartbeat,
    /// Disable heartbeat mechanism.
    #[serde(rename = "public/disable_heartbeat")]
    #[strum(serialize = "public/disable_heartbeat")]
    DisableHeartbeat,
    /// Test connectivity (used for heartbeat response).
    #[serde(rename = "public/test")]
    #[strum(serialize = "public/test")]
    Test,
    /// Hello/handshake message.
    #[serde(rename = "public/hello")]
    #[strum(serialize = "public/hello")]
    Hello,
    /// Get server time.
    #[serde(rename = "public/get_time")]
    #[strum(serialize = "public/get_time")]
    GetTime,

    // Private methods (for future execution support)
    /// Subscribe to private channels.
    #[serde(rename = "private/subscribe")]
    #[strum(serialize = "private/subscribe")]
    PrivateSubscribe,
    /// Unsubscribe from private channels.
    #[serde(rename = "private/unsubscribe")]
    #[strum(serialize = "private/unsubscribe")]
    PrivateUnsubscribe,
    /// Logout and close session.
    #[serde(rename = "private/logout")]
    #[strum(serialize = "private/logout")]
    Logout,
}

impl DeribitWsMethod {
    /// Returns the JSON-RPC method string.
    #[must_use]
    pub fn as_method_str(&self) -> &'static str {
        match self {
            Self::PublicSubscribe => "public/subscribe",
            Self::PublicUnsubscribe => "public/unsubscribe",
            Self::PublicAuth => "public/auth",
            Self::SetHeartbeat => "public/set_heartbeat",
            Self::DisableHeartbeat => "public/disable_heartbeat",
            Self::Test => "public/test",
            Self::Hello => "public/hello",
            Self::GetTime => "public/get_time",
            Self::PrivateSubscribe => "private/subscribe",
            Self::PrivateUnsubscribe => "private/unsubscribe",
            Self::Logout => "private/logout",
        }
    }
}

/// Deribit order book update action types.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DeribitBookAction {
    /// New price level added.
    #[serde(rename = "new")]
    New,
    /// Existing price level changed.
    #[serde(rename = "change")]
    Change,
    /// Price level removed.
    #[serde(rename = "delete")]
    Delete,
}

/// Deribit order book message type.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DeribitBookMsgType {
    /// Full order book snapshot.
    #[serde(rename = "snapshot")]
    Snapshot,
    /// Incremental update.
    #[serde(rename = "change")]
    Change,
}

/// Deribit heartbeat types.
#[derive(
    Clone, Debug, Display, PartialEq, Eq, Hash, AsRefStr, EnumString, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DeribitHeartbeatType {
    /// Server heartbeat notification.
    #[serde(rename = "heartbeat")]
    Heartbeat,
    /// Server requesting client response.
    #[serde(rename = "test_request")]
    TestRequest,
}
