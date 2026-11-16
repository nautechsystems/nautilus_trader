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

//! Recovery tasks and replacement subscriptions for Coinbase books.
//!
//! The tasks here monitor snapshot deadlines and supply the Coinbase replacement operation and
//! error classification to the shared [`BookRecovery`] runner. The runner owns snapshot waits,
//! backoff, the retry budget, and the retry ceiling that follows it. A successful write alone does
//! not complete recovery: the tracker must accept a snapshot.
//!
//! Tasks claim ownership through [`BookSyncTracker`], which keeps shared state transitions under
//! its lock. Cancellation stops obsolete work after unsubscribe, replacement, or shutdown.

use std::{future::Future, sync::Arc};

use nautilus_common::live::dst::time::{self, Duration, Instant};
use nautilus_core::AtomicMap;
use nautilus_live::{book::recovery::BookRecovery as Recovery, task::TaskSpawner};
use nautilus_model::identifiers::InstrumentId;
use ustr::Ustr;

use super::sync::BookSyncTracker;
use crate::websocket::{client::CoinbaseWebSocketClient, error::CoinbaseWsError};

pub(crate) type BookRecovery = Recovery<CoinbaseWsError>;

/// Spawns a one-shot monitor that recovers every book whose armed snapshot deadline expires.
pub(crate) fn spawn_recovery_monitor(
    book_sync: BookSyncTracker,
    ws: CoinbaseWebSocketClient,
    book_products: Arc<AtomicMap<InstrumentId, Ustr>>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let task_cancel = tasks.cancellation_token();
    let spawner = tasks.clone();

    spawn_task(tasks, async move {
        tokio::select! {
            biased;
            () = task_cancel.cancelled() => {}
            () = time::sleep(snapshot_timeout) => {
                let expired = book_sync.take_expired_snapshots(&book_products, Instant::now());

                for signal in expired {
                    signal.log();
                    start_recovery(
                        signal.instrument_id,
                        &book_products,
                        &book_sync,
                        &ws,
                        snapshot_timeout,
                        &spawner,
                    );
                }
            }
        }
    });
}

pub(crate) fn start_recovery(
    instrument_id: InstrumentId,
    book_products: &AtomicMap<InstrumentId, Ustr>,
    book_sync: &BookSyncTracker,
    ws: &CoinbaseWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let Some(product_id) = book_products.get_cloned(&instrument_id) else {
        return;
    };

    let Some(recovery) = book_sync.claim_recovery(instrument_id) else {
        return;
    };

    spawn_recovery_task(
        instrument_id,
        product_id,
        recovery,
        ws.clone(),
        snapshot_timeout,
        tasks,
    );
}

pub(crate) fn spawn_recovery_task(
    instrument_id: InstrumentId,
    product_id: Ustr,
    recovery: Arc<BookRecovery>,
    ws: CoinbaseWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();

    // A refused task drops this guard, so the claimed episode cannot keep the book owned
    let recovery_guard = recovery.cancellation.clone().drop_guard();

    spawn_task(tasks, async move {
        let _recovery_guard = recovery_guard;

        tokio::select! {
            biased;
            () = shutdown.cancelled() => recovery.cancellation.cancel(),
            () = recovery.run(
                instrument_id,
                snapshot_timeout,
                |attempt_cancel, gate| ws.resubscribe_book(product_id, attempt_cancel, gate),
                is_retryable_error,
                CoinbaseWsError::Timeout,
                || CoinbaseWsError::Timeout(format!(
                    "book snapshot not received within {}ms",
                    snapshot_timeout.as_millis(),
                )),
            ) => {}
        }
    });
}

pub(crate) fn spawn_task<F>(tasks: &TaskSpawner, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let cancel = tasks.cancellation_token();

    if let Err(e) = tasks.spawn(async move {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {}
            () = fut => {}
        }
    }) {
        log::debug!("Skipping book task after Coinbase shutdown began: {e}");
    }
}

fn is_retryable_error(error: &CoinbaseWsError) -> bool {
    matches!(
        error,
        CoinbaseWsError::TransportSend(_)
            | CoinbaseWsError::ChannelSend(_)
            | CoinbaseWsError::Connection(_)
            | CoinbaseWsError::Timeout(_)
    )
}
