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

use thiserror::Error;

/// Errors specific to the Bullet adapter.
#[derive(Debug, Error)]
pub enum BulletError {
    /// Credential loading failed.
    #[error("credential error: {0}")]
    Credential(String),

    /// HTTP request failed.
    #[error("HTTP error: {0}")]
    Http(String),

    /// WebSocket error.
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// Transaction signing or serialization failed.
    #[error("signing error: {0}")]
    Signing(String),

    /// The signed transaction was rejected due to a stale chain hash;
    /// the caller should refresh the chain data and retry.
    #[error("transaction outdated: re-fetch chain data and re-sign")]
    TransactionOutdated,

    /// An API error response from the Bullet exchange.
    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },

    /// Symbol not found in the instrument cache.
    #[error("unknown symbol: {0}")]
    UnknownSymbol(String),

    /// Conversion or parsing failed.
    #[error("parse error: {0}")]
    Parse(String),

    /// Configuration problem.
    #[error("config error: {0}")]
    Config(String),
}

impl From<serde_json::Error> for BulletError {
    fn from(e: serde_json::Error) -> Self {
        Self::Parse(e.to_string())
    }
}

impl From<borsh::io::Error> for BulletError {
    fn from(e: borsh::io::Error) -> Self {
        Self::Signing(e.to_string())
    }
}
