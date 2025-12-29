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

//! Binance Spot HTTP error types.

use std::fmt::{self, Display};

use nautilus_network::http::error::HttpClientError;

/// Binance Spot HTTP client error type.
#[derive(Debug)]
pub enum BinanceSpotHttpError {
    /// Missing API credentials for authenticated request.
    MissingCredentials,
    /// Binance API returned an error response.
    BinanceError {
        /// Binance error code.
        code: i64,
        /// Error message from Binance.
        message: String,
    },
    /// SBE decode error.
    SbeDecodeError(SbeDecodeError),
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
        /// Response body (hex encoded for SBE).
        body: String,
    },
}

impl Display for BinanceSpotHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCredentials => write!(f, "Missing API credentials"),
            Self::BinanceError { code, message } => {
                write!(f, "Binance error {code}: {message}")
            }
            Self::SbeDecodeError(err) => write!(f, "SBE decode error: {err}"),
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

impl std::error::Error for BinanceSpotHttpError {}

impl From<SbeDecodeError> for BinanceSpotHttpError {
    fn from(err: SbeDecodeError) -> Self {
        Self::SbeDecodeError(err)
    }
}

impl From<anyhow::Error> for BinanceSpotHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<HttpClientError> for BinanceSpotHttpError {
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

/// Result type for Binance Spot HTTP operations.
pub type BinanceSpotHttpResult<T> = Result<T, BinanceSpotHttpError>;

/// SBE decode error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SbeDecodeError {
    /// Buffer too short to decode expected data.
    BufferTooShort { expected: usize, actual: usize },
    /// Schema ID mismatch.
    SchemaMismatch { expected: u16, actual: u16 },
    /// Schema version mismatch.
    VersionMismatch { expected: u16, actual: u16 },
    /// Unknown template ID.
    UnknownTemplateId(u16),
    /// Group count exceeds safety limit.
    GroupSizeTooLarge { count: u32, max: u32 },
    /// Invalid UTF-8 in string field.
    InvalidUtf8,
}

impl Display for SbeDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort { expected, actual } => {
                write!(
                    f,
                    "Buffer too short: expected {expected} bytes, got {actual}"
                )
            }
            Self::SchemaMismatch { expected, actual } => {
                write!(f, "Schema ID mismatch: expected {expected}, got {actual}")
            }
            Self::VersionMismatch { expected, actual } => {
                write!(
                    f,
                    "Schema version mismatch: expected {expected}, got {actual}"
                )
            }
            Self::UnknownTemplateId(id) => write!(f, "Unknown template ID: {id}"),
            Self::GroupSizeTooLarge { count, max } => {
                write!(f, "Group size {count} exceeds maximum {max}")
            }
            Self::InvalidUtf8 => write!(f, "Invalid UTF-8 in string field"),
        }
    }
}

impl std::error::Error for SbeDecodeError {}
