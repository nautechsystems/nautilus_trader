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

//! WebSocket errors for Hyperliquid.

/// Errors that can occur during Hyperliquid WebSocket operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum HyperliquidWsError {
    #[error("URL parsing failed: {0}")]
    UrlParsing(String),

    #[error("Message serialization failed: {0}")]
    MessageSerialization(String),

    #[error("Message deserialization failed: {0}")]
    MessageDeserialization(String),

    #[error("WebSocket connection failed: {0}")]
    Connection(String),

    #[error("Channel send failed: {0}")]
    ChannelSend(String),

    #[error("Client error: {0}")]
    ClientError(String),

    #[error("Tungstenite error: {0}")]
    TungsteniteError(String),
}
