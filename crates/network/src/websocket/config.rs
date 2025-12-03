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

//! Configuration for WebSocket client connections.
//!
//! # Reconnection Strategy
//!
//! The default configuration uses unlimited reconnection attempts (`reconnect_max_attempts: None`).
//! This is intentional for trading systems because:
//! - Venues may be down for extended periods but eventually recover.
//! - Exponential backoff already prevents resource waste.
//! - Automatic recovery can be useful when manual intervention is not desirable.
//!
//! Use `Some(n)` primarily for testing, development, or non-critical connections.

use std::fmt::Debug;

use super::types::{MessageHandler, PingHandler};

/// Configuration for WebSocket client connections.
///
/// # Connection Modes
///
/// The `message_handler` field determines the connection mode:
///
/// ## Handler Mode (`message_handler: Some(...)`)
///
/// - Use with [`crate::websocket::WebSocketClient::connect`].
/// - Client spawns internal task to read messages and call handler.
/// - Supports automatic reconnection with exponential backoff.
/// - Reconnection config fields (`reconnect_*`) are active.
/// - Best for long-lived connections, Python bindings, callback-based APIs.
///
/// ## Stream Mode (`message_handler: None`)
///
/// - Use with [`crate::websocket::WebSocketClient::connect_stream`].
/// - Returns a [`MessageReader`](super::types::MessageReader) stream for the caller to read from.
/// - **Does NOT support automatic reconnection** (reader owned by caller).
/// - Reconnection config fields are ignored.
/// - On disconnect, client transitions to CLOSED state and caller must manually reconnect.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct WebSocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The default headers.
    pub headers: Vec<(String, String)>,
    /// The function to handle incoming messages.
    ///
    /// - `Some(handler)`: Handler mode with automatic reconnection (use with `connect`).
    /// - `None`: Stream mode without automatic reconnection (use with `connect_stream`).
    ///
    /// See [`WebSocketConfig`] docs for detailed explanation of modes.
    pub message_handler: Option<MessageHandler>,
    /// The optional heartbeat interval (seconds).
    pub heartbeat: Option<u64>,
    /// The optional heartbeat message.
    pub heartbeat_msg: Option<String>,
    /// The handler for incoming pings.
    pub ping_handler: Option<PingHandler>,
    /// The timeout (milliseconds) for reconnection attempts.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_timeout_ms: Option<u64>,
    /// The initial reconnection delay (milliseconds) for reconnects.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_delay_initial_ms: Option<u64>,
    /// The maximum reconnect delay (milliseconds) for exponential backoff.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_delay_max_ms: Option<u64>,
    /// The exponential backoff factor for reconnection delays.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_backoff_factor: Option<f64>,
    /// The maximum jitter (milliseconds) added to reconnection delays.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    pub reconnect_jitter_ms: Option<u64>,
    /// The maximum number of reconnection attempts before giving up.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    /// - `None`: Unlimited reconnection attempts (default, recommended for production).
    /// - `Some(n)`: After n failed attempts, transition to CLOSED state.
    pub reconnect_max_attempts: Option<u32>,
}

impl Debug for WebSocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WebSocketConfig))
            .field("url", &self.url)
            .field("headers", &self.headers)
            .field(
                "message_handler",
                &self.message_handler.as_ref().map(|_| "<function>"),
            )
            .field("heartbeat", &self.heartbeat)
            .field("heartbeat_msg", &self.heartbeat_msg)
            .field(
                "ping_handler",
                &self.ping_handler.as_ref().map(|_| "<function>"),
            )
            .field("reconnect_timeout_ms", &self.reconnect_timeout_ms)
            .field(
                "reconnect_delay_initial_ms",
                &self.reconnect_delay_initial_ms,
            )
            .field("reconnect_delay_max_ms", &self.reconnect_delay_max_ms)
            .field("reconnect_backoff_factor", &self.reconnect_backoff_factor)
            .field("reconnect_jitter_ms", &self.reconnect_jitter_ms)
            .field("reconnect_max_attempts", &self.reconnect_max_attempts)
            .finish()
    }
}

impl Clone for WebSocketConfig {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            headers: self.headers.clone(),
            message_handler: self.message_handler.clone(),
            heartbeat: self.heartbeat,
            heartbeat_msg: self.heartbeat_msg.clone(),
            ping_handler: self.ping_handler.clone(),
            reconnect_timeout_ms: self.reconnect_timeout_ms,
            reconnect_delay_initial_ms: self.reconnect_delay_initial_ms,
            reconnect_delay_max_ms: self.reconnect_delay_max_ms,
            reconnect_backoff_factor: self.reconnect_backoff_factor,
            reconnect_jitter_ms: self.reconnect_jitter_ms,
            reconnect_max_attempts: self.reconnect_max_attempts,
        }
    }
}
