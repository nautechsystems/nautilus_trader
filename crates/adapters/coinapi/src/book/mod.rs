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

//! Order book synchronization and recovery for CoinAPI `book` and `book_l3` streams.
//!
//! - [`sync`] owns sequence validation, snapshot acceptance, and recovery state transitions.
//! - `orders` keeps the resting orders an L3 stream changes by identifier.
//! - `recovery` restarts a book's stream through the shared retry runner.
//!
//! CoinAPI numbers each (type, symbol) stream from one per connection and sends a snapshot when
//! the pair enters the session scope. It replays nothing for a repeated scope, so recovery takes
//! the pair out of the scope and restores it, and the venue stream delivers the snapshot.
//! Fixed-depth `book5`, `book20`, and `book50` messages each list complete top levels, so they need
//! no synchronization.

pub mod sync;

pub(crate) mod orders;
pub(crate) mod recovery;

use crate::websocket::error::Error as WsError;

/// Error from a CoinAPI book recovery attempt, classified for the shared retry runner.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CoinApiBookError {
    /// A later attempt can resolve the failure.
    #[error("{0}")]
    Retryable(String),
    /// Another attempt cannot resolve the failure.
    #[error("{0}")]
    Permanent(String),
}

impl CoinApiBookError {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}

impl From<WsError> for CoinApiBookError {
    fn from(error: WsError) -> Self {
        match error {
            WsError::Validation(_) => Self::Permanent(error.to_string()),
            WsError::Transport(_) | WsError::Send(_) => Self::Retryable(error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_network::{TransportError, error::SendError};
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        WsError::Validation("book pair is not subscribed".into()),
        CoinApiBookError::Permanent(
            "Invalid CoinAPI WebSocket request: book pair is not subscribed".into()
        ),
        false
    )]
    #[case(
        WsError::Transport(TransportError::ConnectionReset),
        CoinApiBookError::Retryable("connection reset".into()),
        true
    )]
    #[case(
        WsError::Send(SendError::BrokenPipe("writer closed".into())),
        CoinApiBookError::Retryable("send failed: broken pipe (writer closed)".into()),
        true
    )]
    fn websocket_errors_preserve_context_and_retry_classification(
        #[case] error: WsError,
        #[case] expected: CoinApiBookError,
        #[case] retryable: bool,
    ) {
        let mapped = CoinApiBookError::from(error);

        assert_eq!(mapped, expected);
        assert_eq!(mapped.is_retryable(), retryable);
    }
}
