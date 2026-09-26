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

//! Shared order book synchronization, recovery, and conformance checks.
//!
//! - [`sync`] holds the per-book synchronization lifecycle and its contract.
//! - [`recovery`] owns recovery episodes, the retry budget, and the retry ceiling that follows it.
//! - [`snapshot`] coordinates subscription write confirmation with snapshot acceptance and deadlines.
//! - [`BookSequenceOutcome`] and [`BookSyncSignal`] describe validation decisions and monitoring
//!   signals without carrying venue-specific sequence fields or channel types.
//! - `conformance` (test support) checks emitted book output against the book stream contract.
//!
//! # Recovery Lifecycle
//!
//! The adapter validates incoming book frames and marks gaps through [`BookSync`](sync::BookSync).
//! The recovery runner requests replacement snapshots through an adapter-supplied operation until
//! the adapter accepts one or the episode is cancelled; a book never ends in a terminal failure
//! state. A confirmed write opens the snapshot gate; only an accepted fresh snapshot completes
//! recovery.
//!
//! # Adapters
//!
//! Adapters retain sequence rules, venue positions, buffers, socket routing, wire commands,
//! acknowledgement correlation, and error classification. They keep [`BookSync`](sync::BookSync)
//! under their existing lock or owning task.
//!
//! A monitoring signal does not itself start recovery; the adapter decides how to respond.

pub mod recovery;
pub mod snapshot;
pub mod sync;

#[cfg(any(test, feature = "test-support"))]
pub mod conformance;

use nautilus_common::live::dst::time::Duration;
use nautilus_model::identifiers::InstrumentId;

/// Default wait for an initial, post-reconnect, or recovery order book snapshot, in seconds.
///
/// Adapters use this as their `book_snapshot_timeout_secs` default; the config
/// value remains tunable per deployment.
pub const DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS: u64 = 10;

/// Decision from an adapter's book sequence validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookSequenceOutcome {
    /// Process the book snapshot or incremental update.
    Accept,
    /// Discard the frame without starting another recovery.
    Suppress,
    /// Discard the frame and request a fresh snapshot.
    Recover,
}

/// Condition reported while monitoring book synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookSyncSignalKind {
    /// No accepted book update arrived within the stale-feed threshold.
    Stale { elapsed: Duration },
    /// The expected book snapshot did not arrive before its deadline.
    SnapshotMissing,
}

/// A monitoring signal for one book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookSyncSignal {
    pub instrument_id: InstrumentId,
    pub kind: BookSyncSignalKind,
}

impl BookSyncSignal {
    /// Logs the signal as a warning.
    pub fn log(&self) {
        let instrument_id = self.instrument_id;

        match self.kind {
            BookSyncSignalKind::Stale { elapsed } => {
                log::warn!(
                    "Book feed stale for {instrument_id}: no update for {:.3}s",
                    elapsed.as_secs_f64()
                );
            }
            BookSyncSignalKind::SnapshotMissing => {
                log::warn!("Book snapshot not received for {instrument_id} after recovery request");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn default_book_snapshot_timeout_is_ten_seconds() {
        assert_eq!(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS, 10);
    }
}
