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

//! Defines the error structures and enumerations for the Coinbase International integration.
//!
//! This module includes data types for deserializing exchange errors from Coinbase International
//! (`CoinbaseIntxErrorResponse`, `CoinbaseIntxErrorMessage`), as well as a higher-level typed
//! error enum (`CoinbaseIntxHttpError`) that represents the various failure states in
//! the client (e.g., missing credentials, network errors, unexpected status
//! codes, etc.).

use nautilus_network::http::HttpClientError;
use reqwest::StatusCode;
use serde::Deserialize;
use thiserror::Error;

/// Represents the JSON structure of an error response returned by the Coinbase API.
#[derive(Clone, Debug, Deserialize)]
pub struct CoinbaseIntxErrorResponse {
    /// The top-level error object included in the Coinbase error response.
    pub error: CoinbaseIntxErrorMessage,
}

/// Contains the specific error details provided by the Coinbase API.
#[derive(Clone, Debug, Deserialize)]
pub struct CoinbaseIntxErrorMessage {
    /// A human-readable explanation of the error condition.
    pub message: String,
    /// A short identifier or category for the error, as returned by Coinbase.
    pub name: String,
}

#[derive(Deserialize)]
pub(crate) struct ErrorBody {
    pub title: Option<String>,
    pub error: Option<String>,
}

/// A typed error enumeration for the Coinbase HTTP client.
#[derive(Debug, Error)]
pub enum CoinbaseIntxHttpError {
    /// Error variant when credentials are missing but the request is authenticated.
    #[error("Missing credentials for authenticated request")]
    MissingCredentials,
    /// Errors returned directly by Coinbase (non-zero code).
    #[error("{error_code}: {message}")]
    CoinbaseError { error_code: String, message: String },
    /// Failure during JSON serialization/deserialization.
    #[error("JSON error: {0}")]
    JsonError(String),
    /// Wrapping the underlying HttpClientError from the network crate.
    #[error("Network error: {0}")]
    HttpClientError(#[from] HttpClientError),
    /// Any unknown HTTP status or unexpected response from Coinbase.
    #[error("Unexpected HTTP status code {status}: {body}")]
    UnexpectedStatus { status: StatusCode, body: String },
}
