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

use thiserror::Error;
use tokio_tungstenite::tungstenite;

/// A typed error enumeration for the BitMEX WebSocket client.
#[derive(Debug, Error)]
pub enum BitmexWsError {
    /// Parsing error.
    #[error("Parsing error: {0}")]
    ParsingError(String),
    /// Errors returned directly by BitMEX (non-zero code).
    #[error("BitMEX error {error_name}: {message}")]
    BitmexError { error_name: String, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Client error.
    #[error("Client error: {0}")]
    ClientError(String),
    /// Authentication error.
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
    /// Subscription error.
    #[error("Subscription error: {0}")]
    SubscriptionError(String),
    /// WebSocket transport error.
    #[error("Tungstenite error: {0}")]
    TungsteniteError(#[from] tungstenite::Error),
}

impl From<serde_json::Error> for BitmexWsError {
    fn from(error: serde_json::Error) -> Self {
        BitmexWsError::JsonError(error.to_string())
    }
}
