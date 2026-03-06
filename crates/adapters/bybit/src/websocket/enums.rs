//! Enumerations for Bybit WebSocket operations and channels.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// WebSocket operation type.
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
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum BybitWsOperation {
    /// Subscribe to topics.
    Subscribe,
    /// Unsubscribe from topics.
    Unsubscribe,
    /// Authenticate connection.
    Auth,
    /// Ping message.
    Ping,
    /// Pong message.
    Pong,
}

/// Private authenticated channel types.
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
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum BybitWsPrivateChannel {
    /// Order updates.
    Order,
    /// Execution/fill updates.
    Execution,
    /// Position updates.
    Position,
    /// Wallet/balance updates.
    Wallet,
}

/// Public channel types.
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
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum BybitWsPublicChannel {
    /// Order book updates.
    #[serde(rename = "orderbook")]
    #[strum(serialize = "orderbook")]
    OrderBook,
    /// Public trades.
    #[serde(rename = "publicTrade")]
    #[strum(serialize = "publicTrade")]
    PublicTrade,
    /// Trade updates.
    Trade,
    /// Kline/candlestick updates.
    Kline,
    /// Ticker updates.
    Tickers,
}
