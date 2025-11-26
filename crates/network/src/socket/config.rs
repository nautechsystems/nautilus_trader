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

//! Socket configuration.
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

use tokio_tungstenite::tungstenite::stream::Mode;

use super::types::TcpMessageHandler;

/// Configuration for TCP socket connection.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.network")
)]
pub struct SocketConfig {
    /// The URL to connect to.
    pub url: String,
    /// The connection mode {Plain, TLS}.
    pub mode: Mode,
    /// The sequence of bytes which separates lines.
    pub suffix: Vec<u8>,
    /// The optional function to handle incoming messages.
    pub message_handler: Option<TcpMessageHandler>,
    /// The optional heartbeat with period and beat message.
    pub heartbeat: Option<(u64, Vec<u8>)>,
    /// The timeout (milliseconds) for reconnection attempts.
    pub reconnect_timeout_ms: Option<u64>,
    /// The initial reconnection delay (milliseconds) for reconnects.
    pub reconnect_delay_initial_ms: Option<u64>,
    /// The maximum reconnect delay (milliseconds) for exponential backoff.
    pub reconnect_delay_max_ms: Option<u64>,
    /// The exponential backoff factor for reconnection delays.
    pub reconnect_backoff_factor: Option<f64>,
    /// The maximum jitter (milliseconds) added to reconnection delays.
    pub reconnect_jitter_ms: Option<u64>,
    /// The maximum number of initial connection attempts (default: 5).
    pub connection_max_retries: Option<u32>,
    /// The maximum number of reconnection attempts before giving up.
    /// - `None`: Unlimited reconnection attempts (default, recommended for production).
    /// - `Some(n)`: After n failed attempts, transition to CLOSED state.
    pub reconnect_max_attempts: Option<u32>,
    /// The path to the certificates directory.
    pub certs_dir: Option<String>,
}

impl Debug for SocketConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SocketConfig))
            .field("url", &self.url)
            .field("mode", &self.mode)
            .field("suffix", &self.suffix)
            .field(
                "message_handler",
                &self.message_handler.as_ref().map(|_| "<function>"),
            )
            .field("heartbeat", &self.heartbeat)
            .field("reconnect_timeout_ms", &self.reconnect_timeout_ms)
            .field(
                "reconnect_delay_initial_ms",
                &self.reconnect_delay_initial_ms,
            )
            .field("reconnect_delay_max_ms", &self.reconnect_delay_max_ms)
            .field("reconnect_backoff_factor", &self.reconnect_backoff_factor)
            .field("reconnect_jitter_ms", &self.reconnect_jitter_ms)
            .field("connection_max_retries", &self.connection_max_retries)
            .field("reconnect_max_attempts", &self.reconnect_max_attempts)
            .field("certs_dir", &self.certs_dir)
            .finish()
    }
}

impl Clone for SocketConfig {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            mode: self.mode,
            suffix: self.suffix.clone(),
            message_handler: self.message_handler.clone(),
            heartbeat: self.heartbeat.clone(),
            reconnect_timeout_ms: self.reconnect_timeout_ms,
            reconnect_delay_initial_ms: self.reconnect_delay_initial_ms,
            reconnect_delay_max_ms: self.reconnect_delay_max_ms,
            reconnect_backoff_factor: self.reconnect_backoff_factor,
            reconnect_jitter_ms: self.reconnect_jitter_ms,
            connection_max_retries: self.connection_max_retries,
            reconnect_max_attempts: self.reconnect_max_attempts,
            certs_dir: self.certs_dir.clone(),
        }
    }
}
