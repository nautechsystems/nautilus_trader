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

//! Binance Spot WebSocket message types.
//!
//! This module defines:
//! - [`BinanceSpotWsMessage`]: Wrapper enum for handler output.
//! - [`NautilusSpotDataWsMessage`]: Market data messages for data clients.
//! - [`HandlerCommand`]: Commands sent from the client to the handler.
//! - Subscription request/response structures for the Binance WebSocket API.

use nautilus_model::{
    data::{Data, OrderBookDeltas},
    instruments::InstrumentAny,
};
use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};

use crate::common::enums::BinanceWsMethod;
// Re-export SBE stream types for convenience
pub use crate::common::sbe::stream::{
    BestBidAskStreamEvent, DepthDiffStreamEvent, DepthSnapshotStreamEvent, PriceLevel, Trade,
    TradesStreamEvent,
};

/// Output message from the Spot WebSocket handler.
#[derive(Debug, Clone)]
pub enum BinanceSpotWsMessage {
    /// Public market data message.
    Data(NautilusSpotDataWsMessage),
    /// Error from the server.
    Error(BinanceWsErrorMsg),
    /// WebSocket reconnected - subscriptions should be restored.
    Reconnected,
}

/// Market data message from Binance Spot WebSocket.
///
/// These are public messages that don't require authentication.
#[derive(Debug, Clone)]
pub enum NautilusSpotDataWsMessage {
    /// Market data (trades, quotes, bars).
    Data(Vec<Data>),
    /// Order book deltas.
    Deltas(OrderBookDeltas),
    /// Instrument definition update.
    Instrument(Box<InstrumentAny>),
    /// Raw binary message (unhandled SBE).
    RawBinary(Vec<u8>),
    /// Raw JSON message (unhandled).
    RawJson(serde_json::Value),
}

/// Binance WebSocket error message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinanceWsErrorMsg {
    /// Error code from Binance.
    pub code: i32,
    /// Error message from Binance.
    pub msg: String,
}

/// Commands sent from the outer client to the inner handler.
///
/// The handler runs in a dedicated Tokio task and processes these commands
/// to perform WebSocket operations.
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
    /// Initialize instrument cache with bulk data.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
    /// Subscribe to streams.
    Subscribe { streams: Vec<String> },
    /// Unsubscribe from streams.
    Unsubscribe { streams: Vec<String> },
}

/// Binance WebSocket subscription request.
#[derive(Debug, Clone, Serialize)]
pub struct BinanceWsSubscription {
    /// Request method.
    pub method: BinanceWsMethod,
    /// Stream names to subscribe/unsubscribe.
    pub params: Vec<String>,
    /// Request ID for correlation.
    pub id: u64,
}

impl BinanceWsSubscription {
    /// Create a subscribe request.
    #[must_use]
    pub fn subscribe(streams: Vec<String>, id: u64) -> Self {
        Self {
            method: BinanceWsMethod::Subscribe,
            params: streams,
            id,
        }
    }

    /// Create an unsubscribe request.
    #[must_use]
    pub fn unsubscribe(streams: Vec<String>, id: u64) -> Self {
        Self {
            method: BinanceWsMethod::Unsubscribe,
            params: streams,
            id,
        }
    }
}

/// Binance WebSocket subscription response.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceWsResponse {
    /// Result (null on success).
    pub result: Option<serde_json::Value>,
    /// Request ID for correlation.
    pub id: u64,
}

/// Binance WebSocket error response.
#[derive(Debug, Clone, Deserialize)]
pub struct BinanceWsErrorResponse {
    /// Error code.
    pub code: i32,
    /// Error message.
    pub msg: String,
    /// Request ID if available.
    pub id: Option<u64>,
}
