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

//! Subscription and recovery tasks for Bybit books.
//!
//! The tasks here write gated order book subscriptions, monitor snapshot deadlines, and supply
//! Bybit replacement operations and error classification to the shared [`BookRecovery`] runner.
//! The runner owns snapshot waits, backoff, the retry budget, and the retry ceiling that follows
//! it. A successful send alone does not complete recovery: the tracker must accept a snapshot.
//!
//! A replacement unsubscribes and resubscribes the book's topic on the socket for its product
//! type, and the venue answers with a fresh snapshot. Tasks claim ownership through
//! [`BookSyncTracker`], which keeps shared state transitions under its lock. Cancellation stops
//! obsolete work after unsubscribe, replacement, or shutdown.

use std::sync::Arc;

use nautilus_common::live::dst::time::{self, Duration, Instant};
use nautilus_core::AtomicMap;
use nautilus_live::{
    book::{
        recovery::BookRecovery as Recovery,
        snapshot::{SnapshotGate, snapshot_expired},
    },
    task::TaskSpawner,
};
use nautilus_model::identifiers::InstrumentId;
use tokio_util::sync::CancellationToken;

use super::sync::BookSyncTracker;
use crate::{
    common::enums::BybitProductType,
    websocket::{
        client::BybitWebSocketClient,
        error::{BybitWsError, should_retry_bybit_error},
    },
};

pub(crate) type BookRecovery = Recovery<BybitWsError>;

/// Subscribes an order book delta book and recovers it when its initial snapshot is missing.
///
/// The book suppresses frames until the subscription write completes, and the task claims
/// recovery when the write fails or no snapshot arrives within `snapshot_timeout`. A request that
/// never reached the connection releases the book instead.
#[allow(
    clippy::too_many_arguments,
    reason = "subscription needs shared adapter state"
)]
pub(crate) fn spawn_subscription_task(
    instrument_id: InstrumentId,
    depth: u32,
    book_depths: Arc<AtomicMap<InstrumentId, u32>>,
    book_sync: BookSyncTracker,
    ws_client: BybitWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let gate = SnapshotGate::default();
    gate.lock().close();
    book_depths.insert(instrument_id, depth);
    let cancel = book_sync.record_subscription(instrument_id, Instant::now(), gate.clone());
    let guard = cancel.clone().drop_guard();
    let shutdown = tasks.cancellation_token();
    let spawner = tasks.clone();
    let depths = Arc::clone(&book_depths);
    let tracker = book_sync.clone();

    let subscription = async move {
        let _guard = guard;
        let result = ws_client
            .subscribe_book(instrument_id, depth, cancel.clone(), gate)
            .await;

        match result {
            Ok(()) => {
                if !snapshot_expired(&cancel, snapshot_timeout).await {
                    return;
                }

                log::warn!(
                    "Initial book snapshot missing for {instrument_id}; requesting a fresh snapshot"
                );
            }
            // The topic stays registered, so recovery writes the subscription again
            Err(BybitWsError::Transport(e)) => {
                log::warn!(
                    "Initial book subscription write failed for {instrument_id}: {e}; \
                     requesting a fresh snapshot"
                );
            }
            Err(e) => {
                release_subscription(
                    instrument_id,
                    depth,
                    &cancel,
                    &book_depths,
                    &book_sync,
                    &ws_client,
                )
                .await;
                log::error!("Order book delta subscription failed for {instrument_id}: {e}");
                return;
            }
        }

        if let Some(recovery) = book_sync.claim_subscription_recovery(instrument_id, &cancel) {
            spawn_recovery_task(
                instrument_id,
                depth,
                recovery,
                ws_client,
                snapshot_timeout,
                &spawner,
            );
        }
    };

    let task = async move {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {}
            () = subscription => {}
        }
    };

    if let Err(e) = tasks.spawn(task) {
        // A book whose write never runs would suppress every frame
        tracker.remove(instrument_id);
        depths.remove(&instrument_id);
        log::debug!(
            "Skipping book subscription for {instrument_id} after Bybit shutdown began: {e}"
        );
    }
}

// Releases a failed initial subscription's transport reference and registration, unless an
// unsubscribe or resubscribe already replaced the book
async fn release_subscription(
    instrument_id: InstrumentId,
    depth: u32,
    cancel: &CancellationToken,
    book_depths: &AtomicMap<InstrumentId, u32>,
    book_sync: &BookSyncTracker,
    ws_client: &BybitWebSocketClient,
) {
    if !book_sync.remove_subscription(instrument_id, cancel) {
        return;
    }

    if let Err(e) = ws_client.unsubscribe_orderbook(instrument_id, depth).await {
        log::warn!("Failed to unsubscribe after orderbook subscription error: {e}");
    }

    book_depths.rcu(|depths| {
        if depths.get(&instrument_id) == Some(&depth) {
            depths.remove(&instrument_id);
        }
    });
}

/// Restarts every book of `product_type` after its socket reconnects, arming snapshot deadlines
/// that a one-shot monitor enforces.
///
/// A zero `snapshot_timeout` arms nothing, so reconnected books wait for the replayed
/// subscriptions to deliver snapshots.
pub(crate) fn reset_on_reconnect(
    product_type: BybitProductType,
    book_depths: &Arc<AtomicMap<InstrumentId, u32>>,
    book_sync: &BookSyncTracker,
    ws_client: &BybitWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    book_sync.reset_on_reconnect(product_type);

    if snapshot_timeout.is_zero() {
        return;
    }

    if book_sync.seed_pending_snapshots(product_type, snapshot_timeout, Instant::now()) > 0 {
        spawn_recovery_monitor(
            Arc::clone(book_depths),
            book_sync.clone(),
            ws_client.clone(),
            product_type,
            snapshot_timeout,
            tasks,
        );
    }
}

// Recovers every book of `product_type` whose armed snapshot deadline expires, once
fn spawn_recovery_monitor(
    book_depths: Arc<AtomicMap<InstrumentId, u32>>,
    book_sync: BookSyncTracker,
    ws_client: BybitWebSocketClient,
    product_type: BybitProductType,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let task_cancel = tasks.cancellation_token();
    let spawner = tasks.clone();

    let monitor = async move {
        tokio::select! {
            biased;
            () = task_cancel.cancelled() => {}
            () = time::sleep(snapshot_timeout) => {
                let expired = book_sync.take_expired_snapshots(product_type, Instant::now());

                for signal in expired {
                    signal.log();
                    start_recovery(
                        signal.instrument_id,
                        &book_depths,
                        &book_sync,
                        &ws_client,
                        snapshot_timeout,
                        &spawner,
                    );
                }
            }
        }
    };

    if let Err(e) = tasks.spawn(monitor) {
        log::debug!("Skipping book recovery monitor after Bybit shutdown began: {e}");
    }
}

/// Starts recovery for `instrument_id` unless another owner holds the book.
pub(crate) fn start_recovery(
    instrument_id: InstrumentId,
    book_depths: &AtomicMap<InstrumentId, u32>,
    book_sync: &BookSyncTracker,
    ws_client: &BybitWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let Some(depth) = book_depths.get_cloned(&instrument_id) else {
        return;
    };

    let Some(recovery) = book_sync.claim_recovery(instrument_id) else {
        return;
    };

    spawn_recovery_task(
        instrument_id,
        depth,
        recovery,
        ws_client.clone(),
        snapshot_timeout,
        tasks,
    );
}

fn spawn_recovery_task(
    instrument_id: InstrumentId,
    depth: u32,
    recovery: Arc<BookRecovery>,
    ws_client: BybitWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();
    let recovery_guard = recovery.cancellation.clone().drop_guard();

    let task = async move {
        let _recovery_guard = recovery_guard;

        tokio::select! {
            biased;
            () = shutdown.cancelled() => recovery.cancellation.cancel(),
            () = recovery.run(
                instrument_id,
                snapshot_timeout,
                |attempt_cancel, gate| ws_client.resubscribe_book(
                    instrument_id, depth, attempt_cancel, gate,
                ),
                should_retry_bybit_error,
                BybitWsError::ClientError,
                || BybitWsError::ClientError(format!(
                    "Book snapshot timed out after {}ms",
                    snapshot_timeout.as_millis(),
                )),
            ) => {}
        }
    };

    // A refused task drops the guard, which cancels the claimed episode
    if let Err(e) = tasks.spawn(task) {
        log::debug!("Skipping book recovery for {instrument_id} after Bybit shutdown began: {e}");
    }
}
