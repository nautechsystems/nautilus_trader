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

use serde::{Deserialize, Serialize};

/// WebSocket transport backend selection.
///
/// Selection is runtime so multiple backends can compile side-by-side without
/// a `compile_error!` collision under `--all-features`.
///
/// `Tungstenite` supports custom HTTP upgrade headers on the WebSocket
/// handshake (see [`WebSocketConfig::headers`]). `Sockudo` is gated on the
/// `transport-sockudo` Cargo feature and uses a local HTTP/1.1 handshake helper
/// to pass the same upgrade headers through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportBackend {
    /// `tokio-tungstenite` backed transport (default).
    #[default]
    Tungstenite,
    /// `sockudo-ws` backed transport (gated on `transport-sockudo` feature).
    Sockudo,
}

/// Configuration for WebSocket client connections.
///
/// This struct contains only static configuration settings. Runtime callbacks
/// (message handler, ping handler) are passed separately to `connect()`.
///
/// # Connection Modes
///
/// ## Handler Mode
///
/// - Use with [`crate::websocket::WebSocketClient::connect`].
/// - Pass a message handler to `connect()` to receive messages via callback.
/// - Client spawns internal task to read messages and call handler.
/// - Supports automatic reconnection with exponential backoff.
/// - Reconnection config fields (`reconnect_*`) are active.
/// - Best for long-lived connections, Python bindings, callback-based APIs.
///
/// ## Stream Mode
///
/// - Use with [`crate::websocket::WebSocketClient::connect_stream`].
/// - Returns a [`MessageReader`](super::types::MessageReader) stream for the caller to read from.
/// - **Does NOT support automatic reconnection** (reader owned by caller).
/// - Reconnection config fields are ignored.
/// - On disconnect, client transitions to CLOSED state and caller must manually reconnect.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.network")
)]
#[allow(
    clippy::unsafe_derive_deserialize,
    reason = "PyO3-backed config still needs serde deserialization for strict config decoding"
)]
#[derive(Clone, Debug, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
pub struct WebSocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The default headers.
    #[serde(default)]
    #[builder(default)]
    pub headers: Vec<(String, String)>,
    /// The optional heartbeat interval (seconds).
    #[serde(default)]
    pub heartbeat: Option<u64>,
    /// The optional heartbeat message.
    #[serde(default)]
    pub heartbeat_msg: Option<String>,
    /// The timeout (milliseconds) for reconnection attempts.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    #[serde(default)]
    pub reconnect_timeout_ms: Option<u64>,
    /// The initial reconnection delay (milliseconds) for reconnects.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    #[serde(default)]
    pub reconnect_delay_initial_ms: Option<u64>,
    /// The maximum reconnect delay (milliseconds) for exponential backoff.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    #[serde(default)]
    pub reconnect_delay_max_ms: Option<u64>,
    /// The exponential backoff factor for reconnection delays.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    #[serde(default)]
    pub reconnect_backoff_factor: Option<f64>,
    /// The maximum jitter (milliseconds) added to reconnection delays.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    #[serde(default)]
    pub reconnect_jitter_ms: Option<u64>,
    /// The maximum number of reconnection attempts before giving up.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    /// - `None`: Unlimited reconnection attempts (default, recommended for production).
    /// - `Some(n)`: After n failed attempts, transition to CLOSED state.
    #[serde(default)]
    pub reconnect_max_attempts: Option<u32>,
    /// The idle timeout (milliseconds) for the read task.
    /// When set, the read task will break and trigger reconnection if no data
    /// is received within this duration. Useful for detecting silently dead
    /// connections where the server stops sending without closing.
    /// **Note**: Only applies to handler mode. Ignored in stream mode.
    #[serde(default)]
    pub idle_timeout_ms: Option<u64>,
    /// The transport backend to use for the WebSocket connection.
    ///
    /// Defaults to [`TransportBackend::Tungstenite`]. Selecting
    /// [`TransportBackend::Sockudo`] requires the `transport-sockudo` Cargo
    /// feature; otherwise `connect_with_server` returns an error. Both backends
    /// pass `headers` into the HTTP upgrade request.
    #[serde(default)]
    #[builder(default)]
    pub backend: TransportBackend,
    /// Optional forward proxy URL for the WebSocket connection.
    ///
    /// Routes the connection through an HTTP `CONNECT` tunnel. Accepts
    /// `http://` and `https://` schemes; SOCKS schemes are not yet supported.
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::WebSocketConfig;

    #[rstest]
    fn test_deserialize_websocket_config_rejects_unknown_field() {
        let config = json!({
            "url": "wss://example.com/ws",
            "unexpected": true,
        });

        let error = serde_json::from_value::<WebSocketConfig>(config).unwrap_err();

        assert!(error.to_string().contains("unknown field `unexpected`"));
    }
}
