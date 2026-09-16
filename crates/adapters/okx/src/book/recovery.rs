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

//! Bounded retries, snapshot waits, and cancellation for OKX book recovery.
//!
//! [`BookRecovery`] carries the cancellation token, snapshot outcome, and gate for one recovery
//! episode. The tasks here monitor snapshot deadlines, replace subscriptions, and retry transient
//! failures within attempt and elapsed-time limits. A successful send alone does not complete
//! recovery: the tracker must accept a snapshot.
//!
//! Tasks claim ownership and report failure through [`BookSyncTracker`], which keeps shared state
//! transitions under its lock. Cancellation stops obsolete work after unsubscribe, replacement,
//! or shutdown. The data client supplies the selected channel, transport, and task scope.

use std::sync::Arc;

use nautilus_common::live::dst::time::{self, Duration, Instant};
use nautilus_core::AtomicMap;
use nautilus_live::task::TaskSpawner;
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::retry::{RetryConfig, RetryManager};
use tokio_util::sync::CancellationToken;

use super::{
    BookChannelScope, BookRecoveryOutcome,
    sync::{BookSyncTracker, log_sync_signals},
};
use crate::{
    common::{consts::should_retry_error_code, enums::OKXBookChannel, task::spawn_task},
    websocket::{client::OKXWebSocketClient, error::OKXWsError, handler::SnapshotGate},
};

// Includes the initial attempt
const BOOK_RECOVERY_MAX_ATTEMPTS: u32 = 8;

#[derive(Debug)]
pub(crate) struct BookRecovery {
    pub(crate) cancellation: CancellationToken,
    pub(crate) outcome: tokio::sync::watch::Sender<BookRecoveryOutcome>,
    pub(crate) gate: SnapshotGate,
}

impl BookRecovery {
    pub(crate) fn begin_replacement(&self) -> bool {
        let mut gate = self.gate.lock();

        if self.cancellation.is_cancelled()
            || matches!(*self.outcome.borrow(), BookRecoveryOutcome::Accepted)
        {
            return false;
        }

        gate.close();
        true
    }
}

/// Spawns a one-shot monitor that retries recovery for every book instrument
/// whose armed snapshot deadline expires on this socket's channels.
pub(crate) fn spawn_recovery_monitor(
    book_sync: BookSyncTracker,
    recovery_ws: Option<OKXWebSocketClient>,
    book_channels: Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
    scope: BookChannelScope,
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
                let expired = book_sync.take_expired_snapshots(
                    &book_channels,
                    scope,
                    Instant::now(),
                );
                log_sync_signals(&expired);

                for signal in expired {
                    start_recovery(
                        signal.instrument_id,
                        &book_channels,
                        &book_sync,
                        recovery_ws.as_ref(),
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
    book_channels: &Arc<AtomicMap<InstrumentId, OKXBookChannel>>,
    book_sync: &BookSyncTracker,
    recovery_ws: Option<&OKXWebSocketClient>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let Some(channel) = book_channels.get_cloned(&instrument_id) else {
        return;
    };

    let Some(ws) = recovery_ws.cloned() else {
        book_sync.fail_recovery(instrument_id, None);
        log::error!("No websocket available to recover book for {instrument_id}");
        return;
    };

    let Some(recovery) = book_sync.claim_recovery(instrument_id) else {
        return;
    };

    spawn_recovery_task(
        instrument_id,
        channel,
        book_sync.clone(),
        recovery,
        ws,
        snapshot_timeout,
        tasks,
    );
}

pub(crate) fn spawn_recovery_task(
    instrument_id: InstrumentId,
    channel: OKXBookChannel,
    tracker: BookSyncTracker,
    recovery: Arc<BookRecovery>,
    ws: OKXWebSocketClient,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();
    let recovery_guard = recovery.cancellation.clone().drop_guard();

    spawn_task(tasks, async move {
        let _recovery_guard = recovery_guard;

        let manager = RetryManager::<OKXWsError>::new(RetryConfig {
            max_retries: BOOK_RECOVERY_MAX_ATTEMPTS - 1,
            initial_delay_ms: 1_000,
            max_delay_ms: 10_000,
            backoff_factor: 2.0,
            jitter_ms: 1_000,
            immediate_first: true,
            operation_timeout_ms: None,
            max_elapsed_ms: Some(180_000),
        });

        let result = tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                recovery.cancellation.cancel();
                return;
            }
            result = manager.invocation(
                "OKX book recovery",
                || async {
                    let mut outcome = recovery.outcome.subscribe();
                    if !recovery.begin_replacement() {
                        return Ok(());
                    }
                    recovery.outcome.send_if_modified(|outcome| {
                        if matches!(outcome, BookRecoveryOutcome::Rejected(_)) {
                            *outcome = BookRecoveryOutcome::Pending;
                            true
                        } else {
                            false
                        }
                    });
                    let attempt_cancel = recovery.cancellation.child_token();
                    let _attempt_guard = attempt_cancel.clone().drop_guard();
                    ws.resubscribe_book_channel(
                        instrument_id,
                        channel,
                        attempt_cancel,
                        recovery.gate.clone(),
                    ).await?;
                    let wait = async {
                        loop {
                            match outcome.borrow_and_update().clone() {
                                BookRecoveryOutcome::Accepted => return Ok(()),
                                BookRecoveryOutcome::Rejected(e) => return Err(e),
                                BookRecoveryOutcome::Pending => {}
                            }
                            outcome.changed().await.map_err(|e| OKXWsError::ClientError(e.to_string()))?;
                        }
                    };

                    if snapshot_timeout.is_zero() {
                        wait.await
                    } else {
                        time::timeout(snapshot_timeout, wait).await.unwrap_or_else(|_| {
                            Err(OKXWsError::OperationTimeout {
                                timeout_ms: snapshot_timeout.as_millis().min(u128::from(u64::MAX)) as u64,
                            })
                        })
                    }
                },
                is_retryable_error,
                |e| OKXWsError::ClientError(e.to_string()),
            ).cancellation_token(&recovery.cancellation).execute() => result,
        };

        if let Err(e) = result {
            if !recovery.cancellation.is_cancelled() {
                tracker.fail_recovery(instrument_id, Some(&recovery));
                log::error!(
                    "Book recovery failed for {instrument_id}; subscription retained, book output suppressed until reconnect or resubscribe: {e}"
                );
            }
        } else if matches!(*recovery.outcome.borrow(), BookRecoveryOutcome::Accepted) {
            log::info!("Book recovery completed for {instrument_id}");
        }
    });
}

fn is_retryable_error(error: &OKXWsError) -> bool {
    match error {
        OKXWsError::OkxError { error_code, .. } => is_retryable_code(error_code),
        OKXWsError::TransportSend(_)
        | OKXWsError::SendFailed(_)
        | OKXWsError::TungsteniteError(_)
        | OKXWsError::OperationTimeout { .. } => true,
        _ => false,
    }
}

pub(crate) fn is_retryable_code(code: &str) -> bool {
    matches!(code, "60014" | "64007") || should_retry_error_code(code)
}
