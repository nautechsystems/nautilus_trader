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

//! HTTP client error types.

use std::error::Error;

/// Errors returned by the HTTP client.
///
/// Includes generic transport errors, timeouts, and proxy configuration errors.
#[derive(thiserror::Error, Debug)]
pub enum HttpClientError {
    #[error("HTTP error occurred: {0}")]
    Error(String),

    #[error("HTTP request timed out: {0}")]
    TimeoutError(String),

    #[error("Invalid proxy URL: {0}")]
    InvalidProxy(String),

    #[error("Failed to build HTTP client: {0}")]
    ClientBuildError(String),
}

impl From<reqwest::Error> for HttpClientError {
    fn from(source: reqwest::Error) -> Self {
        // reqwest's Display omits the actionable cause (DNS, refused, TLS),
        // which lives in the source chain, so walk and append it.
        let mut message = source.to_string();
        let mut cause: Option<&(dyn std::error::Error + 'static)> = source.source();
        while let Some(err) = cause {
            message.push_str(": ");
            message.push_str(&err.to_string());
            cause = err.source();
        }

        if source.is_timeout() {
            Self::TimeoutError(message)
        } else {
            Self::Error(message)
        }
    }
}

impl From<String> for HttpClientError {
    fn from(value: String) -> Self {
        Self::Error(value)
    }
}
