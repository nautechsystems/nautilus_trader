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

//! Recovery tasks and replacement subscriptions for Polymarket books.
//!
//! The tasks here monitor snapshot deadlines and supply Polymarket replacement operations and
//! error classification to the shared [`BookRecovery`] runner. The runner owns snapshot waits,
//! backoff, the retry budget, and the retry ceiling that follows it. A successful send alone
//! does not complete recovery: the tracker must accept a snapshot.
//!
//! Tasks claim ownership through [`BookSyncTracker`], which keeps shared state transitions under
//! its lock. Cancellation stops obsolete work after unsubscribe, replacement, or shutdown. The
//! data client supplies the pool handle and task scope.

use std::sync::Arc;

use ahash::AHashSet;
use nautilus_common::live::dst::time::{self, Duration, Instant};
use nautilus_core::{AtomicMap, AtomicSet};
use nautilus_live::{book::recovery::BookRecovery as Recovery, task::TaskSpawner};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use super::sync::BookSyncTracker;
use crate::websocket::{
    error::PolymarketWsError, handler::CycleMarketOutcome, pool::PolymarketMarketPoolHandle,
};

pub(crate) type BookRecovery = Recovery<PolymarketWsError>;

/// Spawns a one-shot monitor that retries recovery for every listed instrument
/// whose armed snapshot deadline expires.
#[allow(
    clippy::too_many_arguments,
    reason = "monitor needs shared adapter state"
)]
pub(crate) fn spawn_recovery_monitor(
    book_sync: BookSyncTracker,
    pool: PolymarketMarketPoolHandle,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument_ids: Vec<InstrumentId>,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let task_cancel = tasks.cancellation_token();
    let spawner = tasks.clone();

    if let Err(e) = tasks.spawn(async move {
        tokio::select! {
            biased;
            () = task_cancel.cancelled() => {}
            () = time::sleep(snapshot_timeout) => {
                let filter = instrument_ids.into_iter().collect::<AHashSet<_>>();
                let expired = book_sync.take_expired_snapshots(&filter, Instant::now());

                for signal in expired {
                    signal.log();
                    let token_id = instruments
                        .get_cloned(&signal.instrument_id)
                        .map(|instrument| instrument.raw_symbol().as_str().to_string());
                    start_recovery(
                        signal.instrument_id,
                        token_id,
                        &active_delta_subs,
                        &book_sync,
                        &pool,
                        snapshot_timeout,
                        &spawner,
                    );
                }
            }
        }
    }) {
        log::debug!("Skipping Polymarket book recovery monitor after shutdown began: {e}");
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "recovery needs shared adapter state"
)]
pub(crate) fn start_recovery(
    instrument_id: InstrumentId,
    token_id: Option<String>,
    active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
    book_sync: &BookSyncTracker,
    pool: &PolymarketMarketPoolHandle,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    if !active_delta_subs.contains(&instrument_id) {
        return;
    }

    let Some(token_id) = token_id else {
        log::error!("No token available to recover book for {instrument_id}");
        return;
    };

    let Some(recovery) = book_sync.claim_recovery_if_subscribed(active_delta_subs, instrument_id)
    else {
        return;
    };

    spawn_recovery_task(
        instrument_id,
        token_id,
        recovery,
        pool.clone(),
        snapshot_timeout,
        tasks,
    );
}

pub(crate) fn spawn_recovery_task(
    instrument_id: InstrumentId,
    token_id: String,
    recovery: Arc<BookRecovery>,
    pool: PolymarketMarketPoolHandle,
    snapshot_timeout: Duration,
    tasks: &TaskSpawner,
) {
    let shutdown = tasks.cancellation_token();
    let recovery_guard = recovery.cancellation.clone().drop_guard();

    if let Err(e) = tasks.spawn(async move {
        let _recovery_guard = recovery_guard;

        tokio::select! {
            biased;
            () = shutdown.cancelled() => recovery.cancellation.cancel(),
            () = recovery.run(
                instrument_id,
                snapshot_timeout,
                |attempt_cancel, gate| {
                    let pool = pool.clone();
                    let token_id = token_id.clone();
                    async move {
                        if attempt_cancel.is_cancelled() {
                            return Err(PolymarketWsError::Client(
                                "Book recovery canceled".into(),
                            ));
                        }

                        let outcome = pool
                            .resubscribe_market(vec![token_id], &attempt_cancel, &gate)
                            .await
                            .map_err(|e| PolymarketWsError::Client(e.to_string()))?;
                        cycle_outcome_result(outcome)?;

                        Ok(())
                    }
                },
                is_retryable_error,
                PolymarketWsError::Client,
                || PolymarketWsError::OperationTimeout {
                    timeout_ms: snapshot_timeout.as_millis().min(u128::from(u64::MAX)) as u64,
                },
            ) => {}
        }
    }) {
        log::debug!("Skipping Polymarket book recovery task after shutdown began: {e}");
    }
}

fn is_retryable_error(error: &PolymarketWsError) -> bool {
    matches!(
        error,
        PolymarketWsError::Network(_)
            | PolymarketWsError::TransportSend(_)
            | PolymarketWsError::TungsteniteError(_)
            | PolymarketWsError::Connection(_)
            | PolymarketWsError::OperationTimeout { .. }
    )
}

fn cycle_outcome_result(outcome: CycleMarketOutcome) -> Result<(), PolymarketWsError> {
    match outcome {
        CycleMarketOutcome::Completed => Ok(()),
        CycleMarketOutcome::ConnectionChanged => Err(PolymarketWsError::Connection(
            "Market subscription cycle interrupted by reconnect".into(),
        )),
        CycleMarketOutcome::Cancelled => {
            Err(PolymarketWsError::Client("Book recovery canceled".into()))
        }
        CycleMarketOutcome::NotDesired => Err(PolymarketWsError::Client(
            "Market subscription no longer desired".into(),
        )),
        CycleMarketOutcome::SendFailed(e) => Err(PolymarketWsError::TransportSend(e)),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration as StdDuration;

    use nautilus_common::testing::wait_until_async;
    use nautilus_live::task::TaskGroup;
    use nautilus_network::error::SendError;
    use rstest::rstest;

    use super::*;
    use crate::websocket::pool::PolymarketMarketPoolHandle;

    fn instrument_id() -> InstrumentId {
        InstrumentId::from("0xCOND-0xTOKEN.POLYMARKET")
    }

    fn test_tasks() -> (TaskGroup, TaskSpawner) {
        let tasks = TaskGroup::new();
        let spawner = tasks.spawner().expect("open task group");
        (tasks, spawner)
    }

    #[rstest]
    fn start_recovery_ignores_unsubscribed_instrument() {
        let book_sync = BookSyncTracker::default();
        let active_delta_subs = Arc::new(AtomicSet::new());
        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = PolymarketMarketPoolHandle::test_single_shard(ws_tx, &[]);
        let (_tasks, spawner) = test_tasks();
        let instrument_id = instrument_id();

        start_recovery(
            instrument_id,
            Some("0xTOKEN".to_string()),
            &active_delta_subs,
            &book_sync,
            &pool,
            Duration::from_secs(10),
            &spawner,
        );

        assert!(!book_sync.book_gated(instrument_id));
        assert!(
            book_sync.claim_recovery(instrument_id).is_some(),
            "unsubscribed instrument must not consume recovery ownership"
        );
    }

    #[rstest]
    fn start_recovery_without_token_leaves_book_unowned() {
        let book_sync = BookSyncTracker::default();
        let active_delta_subs = Arc::new(AtomicSet::new());
        active_delta_subs.insert(instrument_id());
        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = PolymarketMarketPoolHandle::test_single_shard(ws_tx, &[]);
        let (tasks, spawner) = test_tasks();
        let instrument_id = instrument_id();

        start_recovery(
            instrument_id,
            None,
            &active_delta_subs,
            &book_sync,
            &pool,
            Duration::from_secs(10),
            &spawner,
        );

        // No owner is stranded: a later event can claim recovery, and a snapshot restores the book
        assert!(tasks.is_empty());
        assert!(
            book_sync.record_snapshot_if_subscribed(
                &active_delta_subs,
                instrument_id,
                Instant::now()
            ),
            "missing token must not suppress book output"
        );
        assert!(book_sync.claim_recovery(instrument_id).is_some());
    }

    #[rstest]
    #[tokio::test]
    async fn start_recovery_skips_when_recovery_owned() {
        let book_sync = BookSyncTracker::default();
        let active_delta_subs = Arc::new(AtomicSet::new());
        active_delta_subs.insert(instrument_id());
        let (ws_tx, mut ws_rx) = tokio::sync::mpsc::unbounded_channel();
        let pool = PolymarketMarketPoolHandle::test_single_shard(ws_tx, &["0xTOKEN"]);
        let (tasks, spawner) = test_tasks();
        let instrument_id = instrument_id();
        let owned = book_sync.claim_recovery(instrument_id).unwrap();

        start_recovery(
            instrument_id,
            Some("0xTOKEN".to_string()),
            &active_delta_subs,
            &book_sync,
            &pool,
            Duration::from_secs(10),
            &spawner,
        );

        assert!(
            tasks.is_empty(),
            "owned recovery must not spawn a duplicate task"
        );

        // Give a duplicate task a chance to send so the absence below is meaningful.
        tokio::time::sleep(StdDuration::from_millis(200)).await;

        assert!(
            ws_rx.try_recv().is_err(),
            "owned recovery must not resubscribe"
        );
        assert!(!owned.cancellation.is_cancelled());
        assert!(book_sync.claim_recovery(instrument_id).is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn recovery_task_keeps_ownership_after_non_retryable_failure() {
        let book_sync = BookSyncTracker::default();
        let active_delta_subs = AtomicSet::new();
        active_delta_subs.insert(instrument_id());
        let (ws_tx, _ws_rx) = tokio::sync::mpsc::unbounded_channel();
        // The token is unowned, so the replacement send fails fast with a
        // non-retryable error, moving the recovery straight to its ceiling.
        let pool = PolymarketMarketPoolHandle::test_single_shard(ws_tx, &[]);
        let (tasks, spawner) = test_tasks();
        let instrument_id = instrument_id();
        let recovery = book_sync.claim_recovery(instrument_id).unwrap();

        spawn_recovery_task(
            instrument_id,
            "0xTOKEN".to_string(),
            recovery.clone(),
            pool,
            Duration::from_secs(10),
            &spawner,
        );

        wait_until_async(
            || async { recovery.gate.lock().is_closed() },
            StdDuration::from_secs(5),
        )
        .await;
        tokio::time::sleep(StdDuration::from_millis(200)).await;

        let running = recovery.is_running();
        let second_owner = book_sync.claim_recovery(instrument_id);
        recovery.gate.open();
        let accepted = book_sync.record_snapshot_if_subscribed(
            &active_delta_subs,
            instrument_id,
            Instant::now(),
        );
        wait_until_async(|| async { tasks.all_finished() }, StdDuration::from_secs(5)).await;

        assert!(running, "a failed attempt must not end the recovery");
        assert!(second_owner.is_none());
        assert!(
            accepted,
            "a snapshot with the gate open completes the recovery"
        );
        assert!(recovery.is_accepted());
        assert!(!book_sync.book_gated(instrument_id));
    }

    #[rstest]
    #[case::network(PolymarketWsError::Network("down".to_string()), true)]
    #[case::transport_send(PolymarketWsError::TransportSend(SendError::Closed), true)]
    #[case::tungstenite(PolymarketWsError::TungsteniteError("e".to_string()), true)]
    #[case::connection(PolymarketWsError::Connection("e".to_string()), true)]
    #[case::operation_timeout(
        PolymarketWsError::OperationTimeout { timeout_ms: 10 },
        true
    )]
    #[case::url_parsing(PolymarketWsError::UrlParsing("e".to_string()), false)]
    #[case::message_serialization(
        PolymarketWsError::MessageSerialization("e".to_string()),
        false
    )]
    #[case::message_deserialization(
        PolymarketWsError::MessageDeserialization("e".to_string()),
        false
    )]
    #[case::authentication(PolymarketWsError::Authentication("e".to_string()), false)]
    #[case::channel_send(PolymarketWsError::ChannelSend("e".to_string()), false)]
    #[case::client(PolymarketWsError::Client("e".to_string()), false)]
    #[case::no_active_client(PolymarketWsError::NoActiveClient, false)]
    #[case::handler_unavailable(
        PolymarketWsError::HandlerUnavailable("e".to_string()),
        false
    )]
    fn recovery_retry_classification(#[case] error: PolymarketWsError, #[case] expected: bool) {
        assert_eq!(is_retryable_error(&error), expected);
    }
}
