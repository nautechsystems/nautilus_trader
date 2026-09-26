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

//! Order book synchronization and recovery for Binance diff depth streams.
//!
//! - [`sync`] owns buffered diffs, sequence validation, snapshot acceptance, and recovery state.
//! - [`recovery`] runs REST snapshot attempts through the shared [`nautilus_live::book`] runner.
//! - [`pacing`] holds snapshot requests to a share of the venue's request-weight budget.
//!
//! Binance seeds and recovers a book from a REST depth snapshot while the diff stream stays
//! subscribed, so a replacement attempt never resubscribes. The data clients route diff frames
//! through the tracker and start recovery when the tracker claims an episode.

pub(crate) mod pacing;
pub(crate) mod recovery;
pub(crate) mod sync;

/// Error from a Binance book snapshot attempt, classified for the shared retry runner.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum BinanceBookError {
    /// A later snapshot attempt can resolve the failure.
    #[error("{0}")]
    Retryable(String),
    /// Another snapshot attempt cannot resolve the failure.
    #[error("{0}")]
    Permanent(String),
}

impl BinanceBookError {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable(_))
    }
}
