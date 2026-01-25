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

//! Binance Spot WebSocket API message types.
//!
//! This module defines:
//! - [`HandlerCommand`]: Commands sent from the client to the handler.
//! - [`NautilusWsApiMessage`]: Output messages emitted by the handler to the client.
//! - Request/response structures for the Binance WebSocket API.

use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};

use crate::spot::http::{
    models::{BinanceCancelOrderResponse, BinanceNewOrderResponse},
    query::{CancelOrderParams, CancelReplaceOrderParams, NewOrderParams},
};

/// Commands sent from the outer client to the inner handler.
///
/// The handler runs in a dedicated Tokio task and processes these commands
/// to perform WebSocket API operations (request/response pattern).
#[allow(
    missing_debug_implementations,
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Set the WebSocket client after connection.
    SetClient(WebSocketClient),
    /// Disconnect and clean up.
    Disconnect,
    /// Place a new order.
    PlaceOrder {
        /// Request ID for correlation.
        id: String,
        /// Order parameters.
        params: NewOrderParams,
    },
    /// Cancel an order.
    CancelOrder {
        /// Request ID for correlation.
        id: String,
        /// Cancel parameters.
        params: CancelOrderParams,
    },
    /// Cancel and replace an order atomically.
    CancelReplaceOrder {
        /// Request ID for correlation.
        id: String,
        /// Cancel-replace parameters.
        params: CancelReplaceOrderParams,
    },
    /// Cancel all open orders for a symbol.
    CancelAllOrders {
        /// Request ID for correlation.
        id: String,
        /// Symbol to cancel all orders for.
        symbol: String,
    },
}

/// Normalized output message from the WebSocket API handler.
///
/// These messages are emitted by the handler and consumed by the client
/// for routing to callers or the execution engine.
#[derive(Debug, Clone)]
pub enum NautilusWsApiMessage {
    /// Connection established.
    Connected,
    /// Session authenticated successfully.
    Authenticated,
    /// Connection was re-established after disconnect.
    Reconnected,
    /// Order accepted by venue.
    OrderAccepted {
        /// Request ID for correlation.
        request_id: String,
        /// Order response from venue.
        response: BinanceNewOrderResponse,
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
        response: BinanceCancelOrderResponse,
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
    /// Cancel-replace response (new order after cancel).
    CancelReplaceAccepted {
        /// Request ID for correlation.
        request_id: String,
        /// Cancel response.
        cancel_response: BinanceCancelOrderResponse,
        /// New order response.
        new_order_response: BinanceNewOrderResponse,
    },
    /// Cancel-replace rejected.
    CancelReplaceRejected {
        /// Request ID for correlation.
        request_id: String,
        /// Error code from venue.
        code: i32,
        /// Error message from venue.
        msg: String,
    },
    /// All orders canceled for a symbol.
    AllOrdersCanceled {
        /// Request ID for correlation.
        request_id: String,
        /// Canceled order responses.
        responses: Vec<BinanceCancelOrderResponse>,
    },
    /// Error from venue or network.
    Error(String),
}

/// Metadata for a pending request.
///
/// Stored in the handler to match responses to their originating requests.
#[derive(Debug, Clone)]
pub enum RequestMeta {
    /// Pending order placement.
    PlaceOrder,
    /// Pending order cancellation.
    CancelOrder,
    /// Pending cancel-replace.
    CancelReplaceOrder,
    /// Pending cancel-all.
    CancelAllOrders,
}

/// WebSocket API request wrapper.
///
/// Requests are sent as JSON text frames, responses come back as SBE binary.
#[derive(Debug, Clone, Serialize)]
pub struct WsApiRequest {
    /// Unique request ID for correlation.
    pub id: String,
    /// API method name (e.g., "order.place").
    pub method: String,
    /// Request parameters.
    pub params: serde_json::Value,
}

impl WsApiRequest {
    /// Create a new WebSocket API request.
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

/// WebSocket API error response (JSON).
#[derive(Debug, Clone, Deserialize)]
pub struct WsApiErrorResponse {
    /// Error code from venue.
    pub code: i32,
    /// Error message from venue.
    pub msg: String,
    /// Request ID if available.
    pub id: Option<String>,
}

/// WebSocket API method names.
pub mod method {
    /// Place a new order.
    pub const ORDER_PLACE: &str = "order.place";
    /// Cancel an order.
    pub const ORDER_CANCEL: &str = "order.cancel";
    /// Cancel and replace an order.
    pub const ORDER_CANCEL_REPLACE: &str = "order.cancelReplace";
    /// Cancel all open orders for a symbol.
    pub const OPEN_ORDERS_CANCEL_ALL: &str = "openOrders.cancelAll";
    /// Session logon.
    pub const SESSION_LOGON: &str = "session.logon";
    /// Session status.
    pub const SESSION_STATUS: &str = "session.status";
    /// Session logout.
    pub const SESSION_LOGOUT: &str = "session.logout";
}
