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

//! CoinAPI protocol and request failures.

use nautilus_network::{http::HttpClientError, retry::RetryError};

/// A failure at the CoinAPI REST boundary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Transport(#[from] HttpClientError),
    #[error("CoinAPI HTTP status {status}")]
    Http {
        status: u16,
        retry_after_secs: Option<u64>,
    },
    #[error("CoinAPI HTTP status {status}: {message}")]
    Venue {
        status: u16,
        message: String,
        retry_after_secs: Option<u64>,
    },
    #[error("CoinAPI response parsing failed: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("Invalid CoinAPI request: {0}")]
    Validation(String),
    #[error("Unexpected CoinAPI response: {0}")]
    Response(String),
    #[error("CoinAPI credit budget exhausted")]
    CreditBudget,
    #[error("CoinAPI retry control: {0}")]
    Retry(RetryError),
}

impl Error {
    /// Returns whether a read request may retry this failure.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(
                HttpClientError::TransportError(_) | HttpClientError::TimeoutError(_),
            )
            | Self::Retry(RetryError::OperationTimeout { .. }) => true,
            Self::Http { status, .. } | Self::Venue { status, .. } => {
                *status == 429 || matches!(status, 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }

    pub(super) fn retry_delay(&self) -> Option<std::time::Duration> {
        match self {
            Self::Http {
                retry_after_secs, ..
            }
            | Self::Venue {
                retry_after_secs, ..
            } => retry_after_secs.map(std::time::Duration::from_secs),
            _ => None,
        }
    }
}
