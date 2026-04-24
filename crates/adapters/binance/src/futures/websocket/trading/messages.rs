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

//! Binance Futures WebSocket Trading API message types.
//!
//! This module defines:
//! - [`BinanceFuturesWsTradingCommand`]: Commands sent from the client to the handler.
//! - [`BinanceFuturesWsTradingMessage`]: Output messages emitted by the handler to the client.
//! - Request/response structures for the Binance Futures WebSocket Trading API.

use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};

use crate::futures::http::{
    models::BinanceFuturesOrder,
    query::{BinanceCancelOrderParams, BinanceModifyOrderParams, BinanceNewOrderParams},
};

/// Commands sent from the outer client to the inner handler.
///
/// The handler runs in a dedicated Tokio task and processes these commands
/// to perform WebSocket Trading API operations (JSON request/response pattern).
#[allow(
    missing_debug_implementations,
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum BinanceFuturesWsTradingCommand {
    /// Sets the WebSocket client after connection.
    SetClient(WebSocketClient),
    /// Disconnects and cleans up.
    Disconnect,
    /// Places a new order.
    PlaceOrder {
        /// Request ID for correlation.
        id: String,
        /// Order parameters.
        params: BinanceNewOrderParams,
    },
    /// Cancels an order.
    CancelOrder {
        /// Request ID for correlation.
        id: String,
        /// Cancel parameters.
        params: BinanceCancelOrderParams,
    },
    /// Modifies an order (in-place price/quantity amendment).
    ModifyOrder {
        /// Request ID for correlation.
        id: String,
        /// Modify parameters.
        params: BinanceModifyOrderParams,
    },
}

/// Normalized output message from the Futures WebSocket Trading API handler.
///
/// These messages are emitted by the handler and consumed by the client
/// for routing to callers or the execution engine.
#[derive(Debug, Clone)]
pub enum BinanceFuturesWsTradingMessage {
    /// Connection established.
    Connected,
    /// Connection was re-established after disconnect.
    Reconnected,
    /// Order accepted by venue.
    OrderAccepted {
        /// Request ID for correlation.
        request_id: String,
        /// Order response from venue.
        response: Box<BinanceFuturesOrder>,
    },
    /// Order rejected by venue.
    OrderRejected {
        /// Request ID for correlation.
        request_id: String,
        /// Error code from venue.
        code: i32,
        /// Error message from venue.
        msg: String,
    },
    /// Order canceled successfully.
    OrderCanceled {
        /// Request ID for correlation.
        request_id: String,
        /// Cancel response from venue.
        response: Box<BinanceFuturesOrder>,
    },
    /// Cancel rejected by venue.
    CancelRejected {
        /// Request ID for correlation.
        request_id: String,
        /// Error code from venue.
        code: i32,
        /// Error message from venue.
        msg: String,
    },
    /// Order modified successfully.
    OrderModified {
        /// Request ID for correlation.
        request_id: String,
        /// Modified order response from venue.
        response: Box<BinanceFuturesOrder>,
    },
    /// Modify rejected by venue.
    ModifyRejected {
        /// Request ID for correlation.
        request_id: String,
        /// Error code from venue.
        code: i32,
        /// Error message from venue.
        msg: String,
    },
    /// Error from venue or network.
    Error(String),
}

/// Metadata for a pending request.
///
/// Stored in the handler to match responses to their originating requests.
#[derive(Debug, Clone, Copy)]
pub enum BinanceFuturesWsTradingRequestMeta {
    /// Pending order placement.
    PlaceOrder,
    /// Pending order cancellation.
    CancelOrder,
    /// Pending order modification.
    ModifyOrder,
}

/// WebSocket Trading API request wrapper.
///
/// Requests are sent as JSON text frames, responses come back as JSON text.
#[derive(Debug, Clone, Serialize)]
pub struct BinanceFuturesWsTradingRequest {
    /// Unique request ID for correlation.
    pub id: String,
    /// API method name (e.g., "order.place").
    pub method: String,
    /// Request parameters.
    pub params: serde_json::Value,
}

impl BinanceFuturesWsTradingRequest {
    /// Creates a new WebSocket Trading API request.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        method: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

/// WebSocket Trading API response envelope.
///
/// Binance Futures WS API returns responses in this format.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesWsTradingResponse {
    /// Request ID for correlation.
    pub id: String,
    /// HTTP-like status code (200 for success).
    pub status: u16,
    /// Result payload (present on success).
    pub result: Option<serde_json::Value>,
    /// Rate limit information.
    #[serde(default, rename = "rateLimits")]
    pub rate_limits: Vec<serde_json::Value>,
    /// Error details (present on failure).
    pub error: Option<BinanceFuturesWsTradingResponseError>,
}

/// Error details within a WebSocket Trading API response.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceFuturesWsTradingResponseError {
    /// Error code from venue.
    pub code: i32,
    /// Error message from venue.
    pub msg: String,
}

/// WebSocket Trading API method names for Binance Futures.
pub mod method {
    /// Places a new order.
    pub const ORDER_PLACE: &str = "order.place";
    /// Cancels an order.
    pub const ORDER_CANCEL: &str = "order.cancel";
    /// Modifies an order (in-place amendment).
    pub const ORDER_MODIFY: &str = "order.modify";
}
