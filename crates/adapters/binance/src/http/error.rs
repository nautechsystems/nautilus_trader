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

//! Binance HTTP error types.

use std::fmt::{self, Display};

use nautilus_network::http::error::HttpClientError;

/// Binance HTTP client error type.
#[derive(Debug)]
pub enum BinanceHttpError {
    /// Missing API credentials for authenticated request.
    MissingCredentials,
    /// Binance API returned an error response.
    BinanceError {
        /// Binance error code.
        code: i64,
        /// Error message from Binance.
        message: String,
    },
    /// JSON parsing or serialization error.
    JsonError(String),
    /// Request validation error.
    ValidationError(String),
    /// Network or connection error.
    NetworkError(String),
    /// Request timed out.
    Timeout(String),
    /// Request was canceled.
    Canceled(String),
    /// Unexpected HTTP status code.
    UnexpectedStatus {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
    },
}

impl Display for BinanceHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::BinanceError { code, message } => {
                write!(f, "Binance error {code}: {message}")
            }
            Self::JsonError(msg) => write!(f, "JSON error: {msg}"),
            Self::ValidationError(msg) => write!(f, "Validation error: {msg}"),
            Self::NetworkError(msg) => write!(f, "Network error: {msg}"),
            Self::Timeout(msg) => write!(f, "Timeout: {msg}"),
            Self::Canceled(msg) => write!(f, "Canceled: {msg}"),
            Self::UnexpectedStatus { status, body } => {
                write!(f, "Unexpected status {status}: {body}")
            }
        }
    }
}

impl std::error::Error for BinanceHttpError {}

impl From<serde_json::Error> for BinanceHttpError {
    fn from(err: serde_json::Error) -> Self {
        Self::JsonError(err.to_string())
    }
}

impl From<anyhow::Error> for BinanceHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<HttpClientError> for BinanceHttpError {
    fn from(err: HttpClientError) -> Self {
        match err {
            HttpClientError::TimeoutError(msg) => Self::Timeout(msg),
            HttpClientError::InvalidProxy(msg) | HttpClientError::ClientBuildError(msg) => {
                Self::NetworkError(msg)
            }
            HttpClientError::Error(msg) => Self::NetworkError(msg),
        }
    }
}

/// Result type for Binance HTTP operations.
pub type BinanceHttpResult<T> = Result<T, BinanceHttpError>;
