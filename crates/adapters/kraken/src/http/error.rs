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

//! Error types for Kraken HTTP client operations.

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum KrakenHttpError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("API error: {}", format_api_errors(.0))]
    ApiError(Vec<String>),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Missing credentials")]
    MissingCredentials,
}

/// Formats API error messages, handling empty error arrays.
fn format_api_errors(errors: &[String]) -> String {
    if errors.is_empty() {
        "unknown error (empty error list)".to_string()
    } else {
        errors.join(", ")
    }
}

impl From<anyhow::Error> for KrakenHttpError {
    fn from(err: anyhow::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}
