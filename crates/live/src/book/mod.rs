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

//! Shared order book recovery state, sequence decisions, and snapshot coordination.
//!
//! - [`recovery`] owns recovery episodes, retry budgets, cancellation, and terminal failure state.
//! - [`snapshot`] coordinates subscription write confirmation with snapshot acceptance and deadlines.
//! - [`BookSequenceOutcome`] and [`BookSyncSignalKind`] describe validation decisions and monitoring
//!   signals without carrying venue-specific sequence fields or channel types.
//!
//! # Recovery Lifecycle
//!
//! The adapter validates incoming book frames and claims recovery through
//! [`BookRecoveryState`](recovery::BookRecoveryState). The recovery runner requests replacement
//! subscriptions through an adapter-supplied operation. A confirmed write opens the snapshot gate;
//! only an accepted fresh snapshot completes recovery. The adapter reports terminal failure through
//! the same recovery state, preventing obsolete work from failing a newer subscription.
//!
//! # Adapters
//!
//! Adapters retain sequence rules, cached levels, socket routing, wire commands, acknowledgement
//! correlation, and error classification. They keep state transitions under their existing lock or
//! owning task and retain an active recovery across reconnects so its retry budget is not reset.
//!
//! A monitoring signal does not itself start recovery; the adapter decides how to respond.

pub mod recovery;
pub mod snapshot;

use nautilus_common::live::dst::time::Duration;

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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn default_book_snapshot_timeout_is_ten_seconds() {
        assert_eq!(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS, 10);
    }
}
