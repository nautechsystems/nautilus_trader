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

//! Data structures for Deribit WebSocket JSON-RPC messages.

use nautilus_model::{
    data::{Data, OrderBookDeltas},
    instruments::InstrumentAny,
};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::enums::{DeribitBookAction, DeribitBookMsgType, DeribitHeartbeatType};
pub use crate::common::rpc::{DeribitJsonRpcError, DeribitJsonRpcRequest, DeribitJsonRpcResponse};
use crate::websocket::error::DeribitWsError;

/// JSON-RPC subscription notification from Deribit.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitSubscriptionNotification<T> {
    /// JSON-RPC version.
    pub jsonrpc: String,
    /// Method name (always "subscription").
    pub method: String,
    /// Subscription parameters containing channel and data.
    pub params: DeribitSubscriptionParams<T>,
}

/// Subscription notification parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitSubscriptionParams<T> {
    /// Channel name (e.g., "trades.BTC-PERPETUAL.raw").
    pub channel: String,
    /// Channel-specific data.
    pub data: T,
}

/// Authentication request parameters for client_signature grant.
#[derive(Debug, Clone, Serialize)]
pub struct DeribitAuthParams {
    /// Grant type (client_signature for HMAC auth).
    pub grant_type: String,
    /// Client ID (API key).
    pub client_id: String,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// HMAC-SHA256 signature.
    pub signature: String,
    /// Random nonce.
    pub nonce: String,
    /// Data string (empty for WebSocket auth).
    pub data: String,
    /// Optional scope for session-based authentication.
    /// Use "session:name" for persistent session auth (allows skipping access_token in private requests).
    /// Use "connection" (default) for per-connection auth (requires access_token in each private request).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// Token refresh request parameters.
#[derive(Debug, Clone, Serialize)]
pub struct DeribitRefreshTokenParams {
    /// Grant type (always "refresh_token").
    pub grant_type: String,
    /// The refresh token obtained from authentication.
    pub refresh_token: String,
}

/// Authentication response result.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitAuthResult {
    /// Access token.
    pub access_token: String,
    /// Token expiration time in seconds.
    pub expires_in: u64,
    /// Refresh token.
    pub refresh_token: String,
    /// Granted scope.
    pub scope: String,
    /// Token type (bearer).
    pub token_type: String,
    /// Enabled features.
    #[serde(default)]
    pub enabled_features: Vec<String>,
}

/// Subscription request parameters.
#[derive(Debug, Clone, Serialize)]
pub struct DeribitSubscribeParams {
    /// List of channels to subscribe to.
    pub channels: Vec<String>,
}

/// Subscription response result.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitSubscribeResult(pub Vec<String>);

/// Heartbeat enable request parameters.
#[derive(Debug, Clone, Serialize)]
pub struct DeribitHeartbeatParams {
    /// Heartbeat interval in seconds (minimum 10).
    pub interval: u64,
}

/// Heartbeat notification data.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitHeartbeatData {
    /// Heartbeat type.
    #[serde(rename = "type")]
    pub heartbeat_type: DeribitHeartbeatType,
}

/// Trade data from trades.{instrument}.raw channel.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitTradeMsg {
    /// Trade ID.
    pub trade_id: String,
    /// Instrument name.
    pub instrument_name: Ustr,
    /// Trade price.
    pub price: f64,
    /// Trade amount (contracts).
    pub amount: f64,
    /// Trade direction ("buy" or "sell").
    pub direction: String,
    /// Trade timestamp in milliseconds.
    pub timestamp: u64,
    /// Trade sequence number.
    pub trade_seq: u64,
    /// Tick direction (0-3).
    pub tick_direction: i8,
    /// Index price at trade time.
    pub index_price: f64,
    /// Mark price at trade time.
    pub mark_price: f64,
    /// IV (for options).
    pub iv: Option<f64>,
    /// Liquidation indicator.
    pub liquidation: Option<String>,
    /// Combo trade ID (if part of combo).
    pub combo_trade_id: Option<i64>,
    /// Block trade ID.
    pub block_trade_id: Option<String>,
    /// Combo ID.
    pub combo_id: Option<String>,
}

/// Order book data from book.{instrument}.raw channel.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitBookMsg {
    /// Message type (snapshot or change).
    #[serde(rename = "type")]
    pub msg_type: DeribitBookMsgType,
    /// Instrument name.
    pub instrument_name: Ustr,
    /// Timestamp in milliseconds.
    pub timestamp: u64,
    /// Change ID for sequence tracking.
    pub change_id: u64,
    /// Previous change ID (for delta validation).
    pub prev_change_id: Option<u64>,
    /// Bid levels: [action, price, amount] where action is "new" for snapshot, "new"/"change"/"delete" for change.
    pub bids: Vec<Vec<serde_json::Value>>,
    /// Ask levels: [action, price, amount] where action is "new" for snapshot, "new"/"change"/"delete" for change.
    pub asks: Vec<Vec<serde_json::Value>>,
}

/// Parsed order book level.
#[derive(Debug, Clone)]
pub struct DeribitBookLevel {
    /// Price level.
    pub price: f64,
    /// Amount at this level.
    pub amount: f64,
    /// Action for delta updates.
    pub action: Option<DeribitBookAction>,
}

/// Ticker data from ticker.{instrument}.raw channel.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitTickerMsg {
    /// Instrument name.
    pub instrument_name: Ustr,
    /// Timestamp in milliseconds.
    pub timestamp: u64,
    /// Best bid price.
    pub best_bid_price: Option<f64>,
    /// Best bid amount.
    pub best_bid_amount: Option<f64>,
    /// Best ask price.
    pub best_ask_price: Option<f64>,
    /// Best ask amount.
    pub best_ask_amount: Option<f64>,
    /// Last trade price.
    pub last_price: Option<f64>,
    /// Mark price.
    pub mark_price: f64,
    /// Index price.
    pub index_price: f64,
    /// Open interest.
    pub open_interest: f64,
    /// Current funding rate (perpetuals).
    pub current_funding: Option<f64>,
    /// Funding 8h rate (perpetuals).
    pub funding_8h: Option<f64>,
    /// Settlement price (expired instruments).
    pub settlement_price: Option<f64>,
    /// 24h volume.
    pub volume: Option<f64>,
    /// 24h volume in USD.
    pub volume_usd: Option<f64>,
    /// 24h high.
    pub high: Option<f64>,
    /// 24h low.
    pub low: Option<f64>,
    /// 24h price change.
    pub price_change: Option<f64>,
    /// State of the instrument.
    pub state: String,
    // Options-specific fields
    /// Greeks (options).
    pub greeks: Option<DeribitGreeks>,
    /// Underlying price (options).
    pub underlying_price: Option<f64>,
    /// Underlying index (options).
    pub underlying_index: Option<String>,
}

/// Greeks for options.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitGreeks {
    pub delta: f64,
    pub gamma: f64,
    pub vega: f64,
    pub theta: f64,
    pub rho: f64,
}

/// Quote data from quote.{instrument} channel.
#[derive(Debug, Clone, Deserialize)]
pub struct DeribitQuoteMsg {
    /// Instrument name.
    pub instrument_name: Ustr,
    /// Timestamp in milliseconds.
    pub timestamp: u64,
    /// Best bid price.
    pub best_bid_price: f64,
    /// Best bid amount.
    pub best_bid_amount: f64,
    /// Best ask price.
    pub best_ask_price: f64,
    /// Best ask amount.
    pub best_ask_amount: f64,
}

/// Raw Deribit WebSocket message variants.
#[derive(Debug, Clone)]
pub enum DeribitWsMessage {
    /// JSON-RPC response to a request.
    Response(DeribitJsonRpcResponse<serde_json::Value>),
    /// Subscription notification (trade, book, ticker data).
    Notification(DeribitSubscriptionNotification<serde_json::Value>),
    /// Heartbeat message.
    Heartbeat(DeribitHeartbeatData),
    /// JSON-RPC error.
    Error(DeribitJsonRpcError),
    /// Reconnection event (internal).
    Reconnected,
}

/// Deribit WebSocket error for external consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeribitWebSocketError {
    /// Error code from Deribit.
    pub code: i64,
    /// Error message.
    pub message: String,
    /// Timestamp when error occurred.
    pub timestamp: u64,
}

impl From<DeribitJsonRpcError> for DeribitWebSocketError {
    fn from(err: DeribitJsonRpcError) -> Self {
        Self {
            code: err.code,
            message: err.message,
            timestamp: 0,
        }
    }
}

/// Normalized Nautilus domain message after parsing.
#[derive(Debug, Clone)]
pub enum NautilusWsMessage {
    /// Market data (trades, bars, quotes).
    Data(Vec<Data>),
    /// Order book deltas.
    Deltas(OrderBookDeltas),
    /// Instrument definition update.
    Instrument(Box<InstrumentAny>),
    /// Error from venue.
    Error(DeribitWsError),
    /// Unhandled/raw message for debugging.
    Raw(serde_json::Value),
    /// Reconnection completed.
    Reconnected,
    /// Authentication succeeded with tokens.
    Authenticated(Box<DeribitAuthResult>),
}

/// Parses a raw JSON message into a DeribitWsMessage.
///
/// # Errors
///
/// Returns an error if JSON parsing fails or the message format is unrecognized.
pub fn parse_raw_message(text: &str) -> Result<DeribitWsMessage, DeribitWsError> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| DeribitWsError::Json(e.to_string()))?;

    // Check for subscription notification (has "method": "subscription")
    if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
        if method == "subscription" {
            let notification: DeribitSubscriptionNotification<serde_json::Value> =
                serde_json::from_value(value).map_err(|e| DeribitWsError::Json(e.to_string()))?;
            return Ok(DeribitWsMessage::Notification(notification));
        }
        // Check for heartbeat
        if method == "heartbeat"
            && let Some(params) = value.get("params")
        {
            let heartbeat: DeribitHeartbeatData = serde_json::from_value(params.clone())
                .map_err(|e| DeribitWsError::Json(e.to_string()))?;
            return Ok(DeribitWsMessage::Heartbeat(heartbeat));
        }
    }

    // Check for JSON-RPC response (has "id" field)
    if value.get("id").is_some() {
        // Check for error response
        if value.get("error").is_some() {
            let response: DeribitJsonRpcResponse<serde_json::Value> =
                serde_json::from_value(value.clone())
                    .map_err(|e| DeribitWsError::Json(e.to_string()))?;
            if let Some(err) = response.error {
                return Ok(DeribitWsMessage::Error(err));
            }
        }
        // Success response
        let response: DeribitJsonRpcResponse<serde_json::Value> =
            serde_json::from_value(value).map_err(|e| DeribitWsError::Json(e.to_string()))?;
        return Ok(DeribitWsMessage::Response(response));
    }

    // Fallback: try to parse as generic response
    let response: DeribitJsonRpcResponse<serde_json::Value> =
        serde_json::from_value(value).map_err(|e| DeribitWsError::Json(e.to_string()))?;
    Ok(DeribitWsMessage::Response(response))
}

/// Extracts the instrument name from a channel string.
///
/// For example: "trades.BTC-PERPETUAL.raw" -> "BTC-PERPETUAL"
pub fn extract_instrument_from_channel(channel: &str) -> Option<&str> {
    let parts: Vec<&str> = channel.split('.').collect();
    if parts.len() >= 2 {
        Some(parts[1])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_parse_subscription_notification() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": "trades.BTC-PERPETUAL.raw",
                "data": [{"trade_id": "123", "price": 50000.0}]
            }
        }"#;

        let msg = parse_raw_message(json).unwrap();
        assert!(matches!(msg, DeribitWsMessage::Notification(_)));
    }

    #[rstest]
    fn test_parse_response() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "result": ["trades.BTC-PERPETUAL.raw"],
            "testnet": true,
            "usIn": 1234567890,
            "usOut": 1234567891,
            "usDiff": 1
        }"#;

        let msg = parse_raw_message(json).unwrap();
        assert!(matches!(msg, DeribitWsMessage::Response(_)));
    }

    #[rstest]
    fn test_parse_error_response() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": 10028,
                "message": "too_many_requests"
            }
        }"#;

        let msg = parse_raw_message(json).unwrap();
        assert!(matches!(msg, DeribitWsMessage::Error(_)));
    }

    #[rstest]
    fn test_extract_instrument_from_channel() {
        assert_eq!(
            extract_instrument_from_channel("trades.BTC-PERPETUAL.raw"),
            Some("BTC-PERPETUAL")
        );
        assert_eq!(
            extract_instrument_from_channel("book.ETH-25DEC25.raw"),
            Some("ETH-25DEC25")
        );
        assert_eq!(extract_instrument_from_channel("platform_state"), None);
    }
}
