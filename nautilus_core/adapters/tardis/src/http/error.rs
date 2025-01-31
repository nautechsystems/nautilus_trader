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
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Tardis API error [{code}]: {message}")]
    ApiError {
        status: u16,
        code: u64,
        message: String,
    },

    #[error("Failed to parse response body as JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Failed to parse response as Tardis type: {0}")]
    ResponseParse(String),
}
