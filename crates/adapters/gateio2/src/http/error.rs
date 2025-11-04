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

//! HTTP client error types.

use std::fmt;

/// Result type for Gate.io HTTP operations.
pub type GateioHttpResult<T> = Result<T, GateioHttpError>;

/// Errors that can occur during Gate.io HTTP operations.
#[derive(Debug)]
pub enum GateioHttpError {
    /// HTTP request error
    HttpError(String),
    /// JSON parsing error
    JsonError(String),
    /// API error from Gate.io
    ApiError { label: String, message: String },
    /// Authentication error
    AuthError(String),
    /// Rate limit exceeded
    RateLimitError(String),
    /// Invalid request
    InvalidRequest(String),
    /// Other error
    Other(String),
}

impl fmt::Display for GateioHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HttpError(msg) => write!(f, "HTTP error: {}", msg),
            Self::JsonError(msg) => write!(f, "JSON error: {}", msg),
            Self::ApiError { label, message } => {
                write!(f, "API error ({}): {}", label, message)
            }
            Self::AuthError(msg) => write!(f, "Authentication error: {}", msg),
            Self::RateLimitError(msg) => write!(f, "Rate limit error: {}", msg),
            Self::InvalidRequest(msg) => write!(f, "Invalid request: {}", msg),
            Self::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for GateioHttpError {}

impl From<anyhow::Error> for GateioHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err.to_string())
    }
}

impl From<serde_json::Error> for GateioHttpError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}
