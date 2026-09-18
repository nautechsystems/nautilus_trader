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

//! Subscription write gates and pending snapshot lifetimes.
//!
//! - [`SnapshotGate`] prevents snapshot acceptance while a replacement subscription is being sent.
//! - [`PendingSnapshot`] owns the cancellation token, gate, and optional deadline for a pending
//!   snapshot. Removing it cancels the associated wait.
//! - [`snapshot_expired`] waits for a snapshot deadline or cancellation. A zero timeout disables
//!   monitoring.
//!
//! # Snapshot Lifecycle
//!
//! Gates start open. The adapter closes a gate before a guarded write and opens it only after the
//! intended connection confirms that write. Snapshot acceptance checks the gate under its lock.
//!
//! An adapter can keep an absolute deadline for monitoring or await [`snapshot_expired`] after
//! write confirmation. Accepting a snapshot or retiring its subscription drops the pending owner,
//! cancelling obsolete work.
//!
//! # Adapters
//!
//! The adapter verifies sequence validity and any subscription-generation correlation. The gate
//! alone cannot identify which request or connection produced a snapshot.
//!
//! Scheduling waits and starting recovery remain adapter responsibilities.

use std::sync::Arc;

use nautilus_common::live::dst::time::{self, Duration, Instant};
use parking_lot::{Mutex, MutexGuard};
use tokio_util::sync::CancellationToken;

/// Coordinates subscription sends with snapshot acceptance.
///
/// Clones share one gate, which is initially open.
#[derive(Debug, Clone, Default)]
pub struct SnapshotGate {
    closed: Arc<Mutex<bool>>,
}

impl SnapshotGate {
    /// Opens the gate after the subscription write completes.
    pub fn open(&self) {
        *self.closed.lock() = false;
    }

    /// Locks snapshot acceptance against a replacement send.
    #[must_use]
    pub fn lock(&self) -> SnapshotGateGuard<'_> {
        SnapshotGateGuard(self.closed.lock())
    }
}

/// Locked snapshot acceptance gate.
#[derive(Debug)]
pub struct SnapshotGateGuard<'a>(MutexGuard<'a, bool>);

impl SnapshotGateGuard<'_> {
    /// Suppresses snapshots until the replacement write completes.
    pub fn close(&mut self) {
        *self.0 = true;
    }

    /// Returns whether snapshot acceptance is suppressed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        *self.0
    }
}

/// Pending snapshot ownership, cancelled on acceptance, replacement, or removal.
#[derive(Debug)]
pub struct PendingSnapshot {
    pub deadline: Option<Instant>,
    pub cancel: CancellationToken,
    pub gate: SnapshotGate,
}

impl Drop for PendingSnapshot {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Returns whether the snapshot deadline expires before cancellation.
///
/// A zero timeout disables monitoring. Adapters cancel the token when accepting a snapshot
/// or retiring its subscription, so obsolete waits cannot start replacement work.
pub async fn snapshot_expired(cancel: &CancellationToken, timeout: Duration) -> bool {
    !timeout.is_zero() && time::timeout(timeout, cancel.cancelled()).await.is_err()
}
