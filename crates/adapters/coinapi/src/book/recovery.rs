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

//! Recovery tasks that restart CoinAPI book streams.
//!
//! Each attempt takes the book's pair out of the session scope and restores it through
//! [`CoinApiWebSocketClient::resubscribe`], and the restarted stream delivers the snapshot. The
//! shared [`BookRecovery`](Recovery) runner owns snapshot waits, backoff, the retry budget, and the
//! retry ceiling that follows it. A completed write alone does not complete recovery: the tracker
//! must accept a snapshot. A restart that pauses the symbol's other channels also discards its
//! retained OHLCV periods, which could otherwise close without the paused updates: once when the
//! pause begins, in case the restart is cancelled, and again once the restored scope is written.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nautilus_common::live::dst::time::{self, Duration, Instant};
use nautilus_live::{
    book::{recovery::BookRecovery as Recovery, snapshot::SnapshotGate},
    task::TaskSpawner,
};
use nautilus_model::identifiers::InstrumentId;
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use super::{CoinApiBookError, sync::BookSyncTracker};
use crate::{
    data::OhlcvCache,
    websocket::{client::CoinApiWebSocketClient, messages::Subscription},
};

pub(crate) type BookRecovery = Recovery<CoinApiBookError>;

/// Spawns a one-shot monitor that recovers every book whose armed snapshot deadline expires.
pub(crate) fn spawn_recovery_monitor(
    tracker: BookSyncTracker,
    ws: Arc<CoinApiWebSocketClient>,
    ohlcv: Arc<Mutex<OhlcvCache>>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let cancel = tasks.cancellation_token();
    let spawner = tasks.clone();

    let task = async move {
        tokio::select! {
            biased;
            () = cancel.cancelled() => {}
            () = time::sleep(snapshot_timeout) => {
                for signal in tracker.take_expired_snapshots(Instant::now()) {
                    signal.log();
                    start_recovery(
                        signal.instrument_id,
                        &tracker,
                        &ws,
                        &ohlcv,
                        snapshot_timeout,
                        &spawner,
                    );
                }
            }
        }
    };

    if let Err(e) = tasks.spawn(task) {
        log::warn!("Skipping book snapshot monitor after shutdown began: {e}");
    }
}

pub(crate) fn start_recovery(
    instrument_id: InstrumentId,
    tracker: &BookSyncTracker,
    ws: &Arc<CoinApiWebSocketClient>,
    ohlcv: &Arc<Mutex<OhlcvCache>>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    if let Some((recovery, subscription)) = tracker.claim_recovery(instrument_id) {
        spawn_recovery_task(
            instrument_id,
            subscription,
            recovery,
            Arc::clone(ws),
            Arc::clone(ohlcv),
            snapshot_timeout,
            tasks,
        );
    }
}

pub(crate) fn spawn_recovery_task(
    instrument_id: InstrumentId,
    subscription: Subscription,
    recovery: Arc<BookRecovery>,
    ws: Arc<CoinApiWebSocketClient>,
    ohlcv: Arc<Mutex<OhlcvCache>>,
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
                |attempt_cancel, gate| restart(&ws, &ohlcv, subscription, attempt_cancel, gate),
                CoinApiBookError::is_retryable,
                CoinApiBookError::Permanent,
                || CoinApiBookError::Retryable("book snapshot deadline expired".to_string()),
            ) => {}
        }
    };

    if let Err(e) = tasks.spawn(task) {
        log::warn!("Skipping book recovery for {instrument_id} after shutdown began: {e}");
    }
}

async fn restart(
    ws: &CoinApiWebSocketClient,
    ohlcv: &Mutex<OhlcvCache>,
    subscription: Subscription,
    cancel: CancellationToken,
    gate: SnapshotGate,
) -> Result<(), CoinApiBookError> {
    let discard = || {
        ohlcv
            .lock()
            .retain(|(symbol_id, _), _| *symbol_id != subscription.symbol_id);
    };

    let paused = AtomicBool::new(false);

    tokio::select! {
        biased;
        () = cancel.cancelled() => {
            Err(CoinApiBookError::Retryable("book recovery attempt cancelled".to_string()))
        }
        result = ws.resubscribe(subscription, || {
            discard();
            paused.store(true, Ordering::Relaxed);
        }) => {
            result?;

            // Updates sent before the removal took effect can restore a period during the pause
            if paused.load(Ordering::Relaxed) {
                discard();
            }

            gate.open();
            Ok(())
        }
    }
}
