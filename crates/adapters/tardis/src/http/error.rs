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

use serde::Deserialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
pub(crate) struct TardisErrorResponse {
    pub code: u64,
    pub message: String,
}

/// HTTP errors for the Tardis HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An HTTP request failed at the transport level.
    #[error("HTTP request failed: {0}")]
    Request(String),

    /// The Tardis API returned an error response.
    #[error("Tardis API error [{code}]: {message}")]
    ApiError {
        /// HTTP status code.
        status: u16,
        /// Tardis error code.
        code: u64,
        /// Tardis error message.
        message: String,
    },

    /// Failed to deserialize the JSON response body.
    #[error("Failed to parse response body as JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    /// Failed to parse the response into a Tardis domain type.
    #[error("Failed to parse response as Tardis type: {0}")]
    ResponseParse(String),
}
