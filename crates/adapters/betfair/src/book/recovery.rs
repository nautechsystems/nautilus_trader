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

//! Subscription and recovery tasks for Betfair books.
//!
//! The tasks here write gated market subscriptions, wait for initial market images, and supply
//! the Betfair replacement operation and error classification to the shared [`BookRecovery`]
//! runner. The runner owns image waits, backoff, the retry budget, and the retry ceiling that
//! follows it. A successful write alone does not complete recovery: the tracker must accept an
//! image.
//!
//! A replacement reissues the market subscription without clocks, and Betfair answers with a
//! fresh image of every subscribed market. An attempt joins a subscription written within the
//! snapshot timeout whose image has not started instead, so markets that need recovery together
//! share one image. With snapshot deadlines disabled, every attempt writes. Tasks claim ownership through [`BookSyncTracker`], and
//! cancellation stops obsolete work after replacement or shutdown.

use std::sync::Arc;

use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_live::{
    book::{
        recovery::BookRecovery as Recovery,
        snapshot::{SnapshotGate, snapshot_expired},
    },
    task::TaskSpawner,
};

use super::sync::BookSyncTracker;
use crate::stream::{
    client::BetfairStreamClient,
    error::{BetfairStreamError, should_retry_stream_error},
};

pub(crate) type BookRecovery = Recovery<BetfairStreamError>;

/// Subscribes a market's books and recovers them when the market image is missing.
///
/// `subscribe` queues the market subscription before this returns, so subscriptions reach the
/// venue in call order, and opens the gate it receives once the write is queued. The market
/// suppresses book changes until then, and the task claims recovery when the write fails or no
/// image arrives within `snapshot_timeout`.
pub(crate) fn spawn_subscription_task(
    market_id: String,
    subscribe: impl FnOnce(&SnapshotGate) -> Result<(), BetfairStreamError>,
    book_sync: BookSyncTracker,
    stream_client: Arc<BetfairStreamClient>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let gate = SnapshotGate::default();
    gate.lock().close();
    let cancel = book_sync.record_subscription(&market_id, Instant::now(), gate.clone());

    // A subscription write always requests an image of every market, so it never joins
    let written =
        book_sync.request_image(Duration::ZERO, Instant::now(), &gate, || subscribe(&gate));

    let guard = cancel.clone().drop_guard();
    let shutdown = tasks.cancellation_token();
    let spawner = tasks.clone();
    let tracker = book_sync.clone();
    let task_market_id = market_id.clone();

    let subscription = async move {
        let _guard = guard;

        match written {
            Ok(()) => {
                if !snapshot_expired(&cancel, snapshot_timeout).await {
                    return;
                }

                log::warn!(
                    "Initial market image missing for {market_id}; requesting a fresh image"
                );
            }
            // The market stays registered, so recovery writes the subscription again
            Err(e) => {
                log::warn!(
                    "Market subscription write failed for {market_id}: {e}; \
                     requesting a fresh image"
                );
            }
        }

        if let Some(recovery) = book_sync.claim_subscription_recovery(&market_id, &cancel) {
            spawn_recovery_task(
                market_id,
                recovery,
                book_sync,
                stream_client,
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
        // A market whose write never runs would suppress every change
        tracker.remove(&task_market_id);
        log::debug!(
            "Skipping market subscription for {task_market_id} after Betfair shutdown began: {e}"
        );
    }
}

/// Starts recovery for `market_id` unless another owner holds its book.
pub(crate) fn start_recovery(
    market_id: &str,
    book_sync: &BookSyncTracker,
    stream_client: Arc<BetfairStreamClient>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let Some(recovery) = book_sync.claim_recovery(market_id) else {
        return;
    };

    spawn_recovery_task(
        market_id.to_string(),
        recovery,
        book_sync.clone(),
        stream_client,
        snapshot_timeout,
        tasks,
    );
}

fn spawn_recovery_task(
    market_id: String,
    recovery: Arc<BookRecovery>,
    book_sync: BookSyncTracker,
    stream_client: Arc<BetfairStreamClient>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();
    let recovery_guard = recovery.cancellation.clone().drop_guard();
    let task_market_id = market_id.clone();

    let task = async move {
        let _recovery_guard = recovery_guard;

        tokio::select! {
            biased;
            () = shutdown.cancelled() => recovery.cancellation.cancel(),
            () = recovery.run(
                &market_id,
                snapshot_timeout,
                |_attempt_cancel, gate| std::future::ready(request_image(
                    &book_sync,
                    &stream_client,
                    snapshot_timeout,
                    &gate,
                )),
                should_retry_stream_error,
                BetfairStreamError::Timeout,
                || BetfairStreamError::Timeout(format!(
                    "Market image timed out after {}ms",
                    snapshot_timeout.as_millis(),
                )),
            ) => {}
        }
    };

    // A refused task drops the guard, which cancels the claimed episode
    if let Err(e) = tasks.spawn(task) {
        log::debug!(
            "Skipping book recovery for {task_market_id} after Betfair shutdown began: {e}"
        );
    }
}

fn request_image(
    book_sync: &BookSyncTracker,
    stream_client: &BetfairStreamClient,
    snapshot_timeout: Duration,
    gate: &SnapshotGate,
) -> Result<(), BetfairStreamError> {
    book_sync.request_image(snapshot_timeout, Instant::now(), gate, || {
        stream_client.request_market_image(gate)
    })
}
