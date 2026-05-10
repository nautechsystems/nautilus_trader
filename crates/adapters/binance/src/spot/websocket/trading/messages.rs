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
//! - [`BinanceSpotWsTradingCommand`]: Commands sent from the client to the handler.
//! - [`BinanceSpotWsTradingMessage`]: Output messages emitted by the handler to the client.
//! - Request/response structures for the Binance Spot WebSocket Trading API.

use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};

use super::user_data::{
    BinanceSpotAccountPositionMsg, BinanceSpotBalanceUpdateMsg, BinanceSpotExecutionReport,
};
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
pub enum BinanceSpotWsTradingCommand {
    /// Sets the WebSocket client after connection.
    SetClient(WebSocketClient),
    /// Disconnects and cleans up.
    Disconnect,
    /// Places a new order.
    PlaceOrder {
        /// Request ID for correlation.
        id: String,
        /// Order parameters.
        params: NewOrderParams,
    },
    /// Cancels an order.
    CancelOrder {
        /// Request ID for correlation.
        id: String,
        /// Cancel parameters.
        params: CancelOrderParams,
    },
    /// Cancels and replaces an order atomically.
    CancelReplaceOrder {
        /// Request ID for correlation.
        id: String,
        /// Cancel-replace parameters.
        params: CancelReplaceOrderParams,
    },
    /// Cancels all open orders for a symbol.
    CancelAllOrders {
        /// Request ID for correlation.
        id: String,
        /// Symbol to cancel all orders for.
        symbol: String,
    },
    /// Authenticates the WebSocket session via `session.logon`.
    SessionLogon,
    /// Subscribes to the user data stream via `userDataStream.subscribe`.
    SubscribeUserData,
}

/// Normalized output message from the WebSocket API handler.
///
/// These messages are emitted by the handler and consumed by the client
/// for routing to callers or the execution engine.
#[derive(Debug, Clone)]
pub enum BinanceSpotWsTradingMessage {
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
    /// User data stream subscribed.
    UserDataSubscribed {
        /// Subscription ID from Binance.
        subscription_id: String,
    },
    /// Order execution report from user data stream.
    ExecutionReport(Box<BinanceSpotExecutionReport>),
    /// Account position update from user data stream.
    AccountPosition(BinanceSpotAccountPositionMsg),
    /// Balance update from user data stream.
    BalanceUpdate(BinanceSpotBalanceUpdateMsg),
    /// Error from venue or network.
    Error(String),
}

/// Metadata for a pending request.
///
/// Stored in the handler to match responses to their originating requests.
#[derive(Debug, Clone, Copy)]
pub enum BinanceSpotWsTradingRequestMeta {
    /// Pending order placement.
    PlaceOrder,
    /// Pending order cancellation.
    CancelOrder,
    /// Pending cancel-replace.
    CancelReplaceOrder,
    /// Pending cancel-all.
    CancelAllOrders,
    /// Pending session logon.
    SessionLogon,
    /// Pending user data subscription.
    SubscribeUserData,
}

/// WebSocket API request wrapper.
///
/// Requests are sent as JSON text frames, responses come back as SBE binary.
#[derive(Debug, Clone, Serialize)]
pub struct BinanceSpotWsTradingRequest {
    /// Unique request ID for correlation.
    pub id: String,
    /// API method name (e.g., "order.place").
    pub method: String,
    /// Request parameters.
    pub params: serde_json::Value,
}

impl BinanceSpotWsTradingRequest {
    /// Creates a new WebSocket API request.
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
pub struct BinanceSpotWsTradingResponseError {
    /// Error code from venue.
    pub code: i32,
    /// Error message from venue.
    pub msg: String,
    /// Request ID if available.
    pub id: Option<String>,
}

/// WebSocket API method names.
pub mod method {
    /// Places a new order.
    pub const ORDER_PLACE: &str = "order.place";
    /// Cancels an order.
    pub const ORDER_CANCEL: &str = "order.cancel";
    /// Cancels and replaces an order.
    pub const ORDER_CANCEL_REPLACE: &str = "order.cancelReplace";
    /// Cancels all open orders for a symbol.
    pub const OPEN_ORDERS_CANCEL_ALL: &str = "openOrders.cancelAll";
    /// Initiates session logon.
    pub const SESSION_LOGON: &str = "session.logon";
    /// Queries session status.
    pub const SESSION_STATUS: &str = "session.status";
    /// Initiates session logout.
    pub const SESSION_LOGOUT: &str = "session.logout";
}
