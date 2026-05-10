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

/// Error type for Coinbase WebSocket operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum CoinbaseWsError {
    /// URL parsing failed.
    #[error("URL parsing failed: {0}")]
    UrlParsing(String),

    /// Message serialization failed.
    #[error("message serialization failed: {0}")]
    MessageSerialization(String),

    /// Message deserialization failed.
    #[error("message deserialization failed: {0}")]
    MessageDeserialization(String),

    /// WebSocket connection failed.
    #[error("WebSocket connection failed: {0}")]
    Connection(String),

    /// Channel send failed.
    #[error("channel send failed: {0}")]
    ChannelSend(String),

    /// Authentication failed.
    #[error("authentication failed: {0}")]
    Auth(String),
}

impl CoinbaseWsError {
    /// Returns true if the error is retryable (connection or channel failures).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Connection(_) | Self::ChannelSend(_))
    }
}
