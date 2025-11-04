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

//! Error types for Lighter HTTP client.

use thiserror::Error;

/// Errors that can occur when using the Lighter HTTP client.
#[derive(Error, Debug)]
pub enum LighterHttpError {
    /// HTTP request error.
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    /// JSON parsing error.
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),

    /// Authentication error.
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// API error response.
    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },

    /// Rate limit exceeded.
    #[error("Rate limit exceeded, retry after {retry_after:?} seconds")]
    RateLimit { retry_after: Option<u64> },

    /// Invalid parameters.
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    /// Other error.
    #[error("{0}")]
    Other(String),
}

/// Result type for Lighter HTTP operations.
pub type LighterHttpResult<T> = Result<T, LighterHttpError>;
