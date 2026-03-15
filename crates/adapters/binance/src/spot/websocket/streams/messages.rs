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
//! The handler emits venue-specific types via [`BinanceSpotWsMessage`].
//! Data client layers convert these to Nautilus domain types.

use nautilus_network::websocket::WebSocketClient;
use serde::{Deserialize, Serialize};

use crate::common::enums::BinanceWsMethod;
pub use crate::spot::sbe::stream::{
    BestBidAskStreamEvent, DepthDiffStreamEvent, DepthSnapshotStreamEvent, PriceLevel, Trade,
    TradesStreamEvent,
};

/// Output message from the Spot WebSocket streams handler.
///
/// Contains venue-specific SBE-decoded event types. The data client layer
/// converts these to Nautilus domain types using parse functions with
/// instrument context.
#[derive(Debug, Clone)]
pub enum BinanceSpotWsMessage {
    /// Trade stream events (SBE decoded).
    Trades(TradesStreamEvent),
    /// Best bid/ask stream event (SBE decoded).
    BestBidAsk(BestBidAskStreamEvent),
    /// Depth snapshot stream event (SBE decoded).
    DepthSnapshot(DepthSnapshotStreamEvent),
    /// Depth diff stream event (SBE decoded).
    DepthDiff(DepthDiffStreamEvent),
    /// Raw binary message (unhandled SBE template).
    RawBinary(Vec<u8>),
    /// Raw JSON message (unhandled text frame).
    RawJson(serde_json::Value),
    /// Error from the server.
    Error(BinanceWsErrorMsg),
    /// WebSocket reconnected.
    Reconnected,
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
pub enum BinanceSpotWsStreamsCommand {
    /// Set the WebSocket client after connection.
    SetClient(WebSocketClient),
    /// Disconnect and clean up.
    Disconnect,
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
