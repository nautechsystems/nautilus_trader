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

//! Error types for Kraken WebSocket client operations.

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum KrakenWsError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Subscription error: {0}")]
    SubscriptionError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    #[error("JSON error: {0}")]
    JsonError(String),

    #[error("Channel error: {0}")]
    ChannelError(String),

    #[error("Disconnected: {0}")]
    Disconnected(String),

    #[error("Timeout: {0}")]
    Timeout(String),
}

impl From<serde_json::Error> for KrakenWsError {
    fn from(error: serde_json::Error) -> Self {
        Self::JsonError(error.to_string())
    }
}
