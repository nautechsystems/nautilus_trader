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

//! Adapter-level error types aggregating HTTP and Machine Server errors.

/// Adapter-level error aggregating HTTP and WebSocket errors.
#[derive(Debug, thiserror::Error)]
pub enum TardisError {
    /// An HTTP API error.
    #[error("HTTP error: {0}")]
    Http(#[from] crate::http::error::Error),

    /// A Machine Server WebSocket error.
    #[error("Machine error: {0}")]
    Machine(#[from] crate::machine::Error),
}

impl TardisError {
    /// Returns `true` if the error is likely transient and the operation can be
    /// retried.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(crate::http::error::Error::ApiError { status, .. }) => {
                *status == 429 || *status >= 500
            }
            Self::Http(crate::http::error::Error::Request(_)) => true,
            Self::Machine(crate::machine::Error::ConnectFailed(_)) => true,
            Self::Machine(crate::machine::Error::ConnectionClosed { .. }) => true,
            _ => false,
        }
    }
}
