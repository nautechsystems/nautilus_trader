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

//! Bootstrap replay for restoring cache state after a cache-owned snapshot.
//!
//! This module is deliberately state-only: it consumes event-store entries, decodes the
//! cache-affecting payloads, and mutates [`nautilus_common::cache::Cache`] directly. It
//! does not publish to the live message bus, send commands, invoke adapters, or submit
//! entries back into the event store.

use std::fmt::Display;

use nautilus_common::cache::Cache;
use nautilus_model::{
    enums::OmsType,
    events::{AccountState, OrderEventAny, OrderFilled, OrderInitialized, PositionAdjusted},
    orders::OrderAny,
    position::Position,
};
use serde::de::DeserializeOwned;

use crate::{
    backend::EventStore,
    capture::builtins::{
        PAYLOAD_TYPE_ACCOUNT_STATE, PAYLOAD_TYPE_ORDER_ACCEPTED,
        PAYLOAD_TYPE_ORDER_CANCEL_REJECTED, PAYLOAD_TYPE_ORDER_CANCELED, PAYLOAD_TYPE_ORDER_DENIED,
        PAYLOAD_TYPE_ORDER_EMULATED, PAYLOAD_TYPE_ORDER_EXPIRED, PAYLOAD_TYPE_ORDER_FILLED,
        PAYLOAD_TYPE_ORDER_INITIALIZED, PAYLOAD_TYPE_ORDER_MODIFY_REJECTED,
        PAYLOAD_TYPE_ORDER_PENDING_CANCEL, PAYLOAD_TYPE_ORDER_PENDING_UPDATE,
        PAYLOAD_TYPE_ORDER_REJECTED, PAYLOAD_TYPE_ORDER_RELEASED, PAYLOAD_TYPE_ORDER_SUBMITTED,
        PAYLOAD_TYPE_ORDER_TRIGGERED, PAYLOAD_TYPE_ORDER_UPDATED, PAYLOAD_TYPE_POSITION_ADJUSTED,
    },
    entry::EventStoreEntry,
    error::EventStoreError,
    reader::{EventStoreReader, SnapshotReplayPlan},
    snapshot::SnapshotAnchor,
};

/// Summary of a cache snapshot-tail replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheReplayReport {
    /// Replay bounds derived from the latest snapshot anchor.
    pub plan: SnapshotReplayPlan,
    /// Number of entries applied to cache state.
    pub applied_entries: usize,
    /// Number of event-store entries that do not have a cache replay rule yet.
    pub ignored_entries: usize,
}

/// Errors surfaced while restoring a cache snapshot tail.
#[derive(Debug, thiserror::Error)]
pub enum CacheReplayError {
    /// The event-store reader failed.
    #[error(transparent)]
    EventStore(#[from] EventStoreError),
    /// The caller-provided snapshot restore hook failed.
    #[error("restore cache snapshot {blob_ref}: {message}")]
    SnapshotRestore {
        /// Cache-owned snapshot blob reference.
        blob_ref: String,
        /// Error message returned by the restore hook.
        message: String,
    },
    /// The replay scan yielded an entry outside the derived restore bounds.
    #[error("entry seq {seq} is before replay start seq {from_seq}")]
    UnexpectedSeq {
        /// Entry sequence yielded by the scan.
        seq: u64,
        /// First sequence this replay is allowed to apply.
        from_seq: u64,
    },
    /// A captured payload failed to decode.
    #[error("decode seq {seq} payload_type {payload_type}: {message}")]
    Decode {
        /// Event-store sequence number.
        seq: u64,
        /// Captured payload type tag.
        payload_type: String,
        /// Decode error message.
        message: String,
    },
    /// Applying a decoded payload to the cache failed.
    #[error("apply seq {seq} payload_type {payload_type}: {message}")]
    Apply {
        /// Event-store sequence number.
        seq: u64,
        /// Captured payload type tag.
        payload_type: String,
        /// Apply error message.
        message: String,
    },
}

impl CacheReplayError {
    /// Builds a snapshot-restore error for `anchor`.
    #[must_use]
    pub fn snapshot_restore(anchor: &SnapshotAnchor, error: impl Display) -> Self {
        Self::SnapshotRestore {
            blob_ref: anchor.blob_ref.clone(),
            message: error.to_string(),
        }
    }
}

/// Replays the cache snapshot tail after the caller restores the cache-owned snapshot blob.
///
/// The restore hook runs before the tail iterator is consumed. When `anchor` is `Some`,
/// the hook should fetch and apply the cache-owned blob identified by
/// [`SnapshotAnchor::blob_ref`] and validate it against
/// [`SnapshotAnchor::content_hash`]. When `anchor` is `None`, restore starts from
/// event-store seq `1` and the hook may be a no-op.
///
/// This is a bootstrap path: it mutates cache state directly and never publishes replay
/// entries to the live message bus.
///
/// # Errors
///
/// Returns [`CacheReplayError::EventStore`] when the reader fails, `restore_snapshot`'s
/// error when the cache snapshot restore hook fails, [`CacheReplayError::Decode`] when
/// a supported payload cannot be decoded, and [`CacheReplayError::Apply`] when the
/// decoded payload cannot be applied to the cache.
pub fn restore_cache_snapshot_and_replay_tail<B, F>(
    cache: &mut Cache,
    reader: &EventStoreReader<B>,
    restore_snapshot: F,
) -> Result<CacheReplayReport, CacheReplayError>
where
    B: EventStore,
    F: FnOnce(&mut Cache, Option<&SnapshotAnchor>) -> Result<(), CacheReplayError>,
{
    let (plan, scan) = reader.scan_snapshot_replay_tail()?;
    restore_snapshot(cache, plan.anchor.as_ref())?;

    let mut applied_entries = 0;
    let mut ignored_entries = 0;

    for entry in scan {
        let entry = entry?;

        if entry.seq < plan.from_seq {
            return Err(CacheReplayError::UnexpectedSeq {
                seq: entry.seq,
                from_seq: plan.from_seq,
            });
        }

        if apply_cache_replay_entry(cache, &entry)? {
            applied_entries += 1;
        } else {
            ignored_entries += 1;
        }
    }

    Ok(CacheReplayReport {
        plan,
        applied_entries,
        ignored_entries,
    })
}

/// Replays the cache snapshot tail when the cache snapshot has already been restored.
///
/// This is a convenience wrapper for callers that load the cache-owned snapshot blob
/// before entering the event-store replay path.
///
/// # Errors
///
/// See [`restore_cache_snapshot_and_replay_tail`].
pub fn replay_cache_snapshot_tail<B>(
    cache: &mut Cache,
    reader: &EventStoreReader<B>,
) -> Result<CacheReplayReport, CacheReplayError>
where
    B: EventStore,
{
    restore_cache_snapshot_and_replay_tail(cache, reader, |_, _| Ok(()))
}

/// Applies one event-store entry to cache state when a replay rule exists.
///
/// Returns `Ok(true)` when the entry changed cache state and `Ok(false)` when the
/// payload is outside the current cache bootstrap replay surface.
///
/// # Errors
///
/// Returns [`CacheReplayError::Decode`] when a supported payload cannot be decoded and
/// [`CacheReplayError::Apply`] when the decoded payload cannot be applied to the cache.
pub fn apply_cache_replay_entry(
    cache: &mut Cache,
    entry: &EventStoreEntry,
) -> Result<bool, CacheReplayError> {
    match entry.payload_type.as_str() {
        PAYLOAD_TYPE_ACCOUNT_STATE => {
            let state = decode_payload::<AccountState>(entry)?;
            apply_result(entry, cache.update_account_state(&state))?;
        }
        PAYLOAD_TYPE_ORDER_INITIALIZED => {
            let event = decode_order_event::<OrderInitialized>(entry, OrderEventAny::Initialized)?;
            let order = OrderAny::from_events(vec![event]).map_err(|e| apply_error(entry, e))?;
            apply_result(entry, cache.add_order(order, None, None, false))?;
        }
        PAYLOAD_TYPE_ORDER_DENIED => {
            apply_order_event(cache, entry, OrderEventAny::Denied)?;
        }
        PAYLOAD_TYPE_ORDER_EMULATED => {
            apply_order_event(cache, entry, OrderEventAny::Emulated)?;
        }
        PAYLOAD_TYPE_ORDER_RELEASED => {
            apply_order_event(cache, entry, OrderEventAny::Released)?;
        }
        PAYLOAD_TYPE_ORDER_SUBMITTED => {
            apply_order_event(cache, entry, OrderEventAny::Submitted)?;
        }
        PAYLOAD_TYPE_ORDER_ACCEPTED => {
            apply_order_event(cache, entry, OrderEventAny::Accepted)?;
        }
        PAYLOAD_TYPE_ORDER_REJECTED => {
            apply_order_event(cache, entry, OrderEventAny::Rejected)?;
        }
        PAYLOAD_TYPE_ORDER_CANCELED => {
            apply_order_event(cache, entry, OrderEventAny::Canceled)?;
        }
        PAYLOAD_TYPE_ORDER_EXPIRED => {
            apply_order_event(cache, entry, OrderEventAny::Expired)?;
        }
        PAYLOAD_TYPE_ORDER_TRIGGERED => {
            apply_order_event(cache, entry, OrderEventAny::Triggered)?;
        }
        PAYLOAD_TYPE_ORDER_PENDING_UPDATE => {
            apply_order_event(cache, entry, OrderEventAny::PendingUpdate)?;
        }
        PAYLOAD_TYPE_ORDER_PENDING_CANCEL => {
            apply_order_event(cache, entry, OrderEventAny::PendingCancel)?;
        }
        PAYLOAD_TYPE_ORDER_MODIFY_REJECTED => {
            apply_order_event(cache, entry, OrderEventAny::ModifyRejected)?;
        }
        PAYLOAD_TYPE_ORDER_CANCEL_REJECTED => {
            apply_order_event(cache, entry, OrderEventAny::CancelRejected)?;
        }
        PAYLOAD_TYPE_ORDER_UPDATED => {
            apply_order_event(cache, entry, OrderEventAny::Updated)?;
        }
        PAYLOAD_TYPE_ORDER_FILLED => {
            let fill = decode_payload::<OrderFilled>(entry)?;
            let event = OrderEventAny::Filled(fill);
            apply_result(entry, cache.update_order(&event))?;
            apply_fill_to_position(cache, entry, &fill)?;
        }
        PAYLOAD_TYPE_POSITION_ADJUSTED => {
            let adjustment = decode_payload::<PositionAdjusted>(entry)?;
            apply_position_adjustment(cache, entry, adjustment)?;
        }
        _ => return Ok(false),
    }

    Ok(true)
}

fn apply_order_event<T>(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    wrap: impl FnOnce(T) -> OrderEventAny,
) -> Result<(), CacheReplayError>
where
    T: DeserializeOwned,
{
    let event = decode_order_event(entry, wrap)?;
    apply_result(entry, cache.update_order(&event))?;
    Ok(())
}

fn decode_order_event<T>(
    entry: &EventStoreEntry,
    wrap: impl FnOnce(T) -> OrderEventAny,
) -> Result<OrderEventAny, CacheReplayError>
where
    T: DeserializeOwned,
{
    Ok(wrap(decode_payload(entry)?))
}

fn apply_fill_to_position(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    fill: &OrderFilled,
) -> Result<(), CacheReplayError> {
    let Some(position_id) = fill.position_id else {
        return Ok(());
    };

    if let Some(mut position) = cache.position_owned(&position_id) {
        if position.trade_ids().contains(&fill.trade_id) {
            return Ok(());
        }

        position.apply(fill);
        apply_result(entry, cache.update_position(&position))?;
        return Ok(());
    }

    let Some(instrument) = cache.instrument(&fill.instrument_id).cloned() else {
        return Ok(());
    };

    let position = Position::new(&instrument, *fill);
    apply_result(entry, cache.add_position(&position, OmsType::Unspecified))?;
    Ok(())
}

fn apply_position_adjustment(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    adjustment: PositionAdjusted,
) -> Result<(), CacheReplayError> {
    let Some(mut position) = cache.position_owned(&adjustment.position_id) else {
        return Ok(());
    };

    position.apply_adjustment(adjustment);
    apply_result(entry, cache.update_position(&position))?;
    Ok(())
}

fn decode_payload<T>(entry: &EventStoreEntry) -> Result<T, CacheReplayError>
where
    T: DeserializeOwned,
{
    rmp_serde::from_slice(&entry.payload).map_err(|e| CacheReplayError::Decode {
        seq: entry.seq,
        payload_type: entry.payload_type.to_string(),
        message: e.to_string(),
    })
}

fn apply_result<T, E>(entry: &EventStoreEntry, result: Result<T, E>) -> Result<T, CacheReplayError>
where
    E: Display,
{
    result.map_err(|e| apply_error(entry, e))
}

fn apply_error(entry: &EventStoreEntry, error: impl Display) -> CacheReplayError {
    CacheReplayError::Apply {
        seq: entry.seq,
        payload_type: entry.payload_type.to_string(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::{any::Any, cell::Cell, rc::Rc};

    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_common::msgbus::{self, BusTap, Endpoint, MStr, Topic as BusTopic};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        accounts::AccountAny,
        enums::{OrderStatus, PositionAdjustmentType},
        events::{
            PositionEvent,
            account::stubs::{cash_account_state, cash_account_state_million_usd},
            order::spec::{
                OrderAcceptedSpec, OrderFilledSpec, OrderInitializedSpec, OrderSubmittedSpec,
            },
        },
        identifiers::{AccountId, PositionId},
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::Order,
        types::{Currency, Money},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        backend::{AppendEntry, MemoryBackend},
        capture::{
            builtins::{encode_order_event_any, encode_position_event},
            encode_account_state,
        },
        entry::Topic as EntryTopic,
        hash::compute_entry_hash,
        headers::Headers,
        manifest::{RegisteredComponents, RunManifest, RunStatus},
        snapshot::SnapshotAnchor,
    };

    fn manifest(run_id: &str) -> RunManifest {
        RunManifest {
            run_id: run_id.to_string(),
            parent_run_id: None,
            instance_id: "trader-001".to_string(),
            binary_hash: "deadbeef".to_string(),
            schema_version: 1,
            crate_versions: "feedface".to_string(),
            feature_flags: Vec::new(),
            adapter_versions: IndexMap::new(),
            config_hash: "cafebabe".to_string(),
            registered_components: RegisteredComponents::default(),
            seed: None,
            start_ts_init: UnixNanos::from(0),
            end_ts_init: None,
            high_watermark: 0,
            status: RunStatus::Running,
        }
    }

    fn append_payload(seq: u64, payload_type: &str, payload: Bytes) -> AppendEntry {
        let topic = EntryTopic::from("events.account.SIM");
        let ts = UnixNanos::from(seq);
        let headers = Headers::empty();
        let hash = compute_entry_hash(
            seq,
            ts,
            ts,
            topic.as_ref(),
            payload_type,
            &payload,
            &headers,
        );
        let entry = EventStoreEntry::new(
            hash,
            seq,
            headers,
            topic,
            Ustr::from(payload_type),
            payload,
            ts,
            ts,
        );
        AppendEntry::without_indices(entry)
    }

    fn append_account_state(seq: u64, state: &AccountState) -> AppendEntry {
        let encoded = encode_account_state(state).expect("encode account state");
        append_payload(seq, PAYLOAD_TYPE_ACCOUNT_STATE, encoded.payload)
    }

    fn append_order_event(seq: u64, event: &OrderEventAny) -> AppendEntry {
        let encoded = encode_order_event_any(event).expect("encode order event");
        let payload_type = encoded.payload_type.expect("order payload type");
        append_payload(seq, payload_type.as_str(), encoded.payload)
    }

    fn append_position_event(seq: u64, event: &PositionEvent) -> AppendEntry {
        let encoded = encode_position_event(event).expect("encode position event");
        let payload_type = encoded.payload_type.expect("position payload type");
        append_payload(seq, payload_type.as_str(), encoded.payload)
    }

    fn reader_with_entries(
        run_id: &str,
        entries: &[AppendEntry],
    ) -> EventStoreReader<MemoryBackend> {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest(run_id)).expect("open");
        backend.append_batch(entries).expect("append");
        EventStoreReader::new(backend)
    }

    fn reader_with_anchor(anchor_seq: u64) -> (EventStoreReader<MemoryBackend>, AccountState) {
        let anchored = cash_account_state();
        let replayed = cash_account_state_million_usd("200 USD", "0 USD", "200 USD");
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-replay")).expect("open");
        backend
            .append_batch(&[
                append_account_state(1, &anchored),
                append_account_state(2, &replayed),
            ])
            .expect("append");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(anchor_seq, "cache://account", "hash"))
            .expect("record anchor");
        (EventStoreReader::new(backend), replayed)
    }

    #[derive(Debug)]
    struct CountingTap {
        calls: Rc<Cell<usize>>,
    }

    impl CountingTap {
        fn new(calls: Rc<Cell<usize>>) -> Self {
            Self { calls }
        }

        fn increment(&self) {
            self.calls.set(self.calls.get() + 1);
        }
    }

    impl BusTap for CountingTap {
        fn on_publish(&self, _topic: MStr<BusTopic>, _message: &dyn Any) {
            self.increment();
        }

        fn on_send(&self, _endpoint: MStr<Endpoint>, _message: &dyn Any) {
            self.increment();
        }
    }

    struct BusTapGuard;

    impl Drop for BusTapGuard {
        fn drop(&mut self) {
            msgbus::clear_bus_tap();
        }
    }

    #[rstest]
    fn replay_restores_snapshot_before_applying_tail() {
        let (reader, replayed) = reader_with_anchor(1);
        let mut cache = Cache::default();
        let restored = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
        let restored_id = restored.account_id;

        let report =
            restore_cache_snapshot_and_replay_tail(&mut cache, &reader, |cache, anchor| {
                assert_eq!(anchor.expect("anchor").high_watermark, 1);
                let account = AccountAny::from_events(std::slice::from_ref(&restored))
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))?;
                cache
                    .add_account(account)
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))
            })
            .expect("replay");

        let account = cache.account_owned(&restored_id).expect("account restored");
        let events = account.events();

        assert_eq!(report.plan.from_seq, 2);
        assert_eq!(report.applied_entries, 1);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(events, vec![restored, replayed]);
    }

    #[rstest]
    fn replay_does_not_apply_entries_at_or_below_anchor_watermark() {
        let (reader, _) = reader_with_anchor(2);
        let mut cache = Cache::default();
        let restored = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
        let restored_id = restored.account_id;

        let report =
            restore_cache_snapshot_and_replay_tail(&mut cache, &reader, |cache, anchor| {
                assert_eq!(anchor.expect("anchor").high_watermark, 2);
                let account = AccountAny::from_events(std::slice::from_ref(&restored))
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))?;
                cache
                    .add_account(account)
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))
            })
            .expect("replay");

        let account = cache.account_owned(&restored_id).expect("account restored");

        assert!(report.plan.is_empty());
        assert_eq!(report.applied_entries, 0);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(account.events(), vec![restored]);
    }

    #[rstest]
    fn replay_from_start_applies_account_state_without_bus_publish() {
        let state = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
        let account_id = AccountId::from("SIM-001");
        let bus_calls = Rc::new(Cell::new(0));
        msgbus::set_bus_tap(Rc::new(CountingTap::new(Rc::clone(&bus_calls))));
        let _guard = BusTapGuard;
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-replay")).expect("open");
        backend
            .append_batch(&[append_account_state(1, &state)])
            .expect("append");
        let reader = EventStoreReader::new(backend);
        let mut cache = Cache::default();

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let account = cache.account_owned(&account_id).expect("account replayed");

        assert_eq!(report.plan.anchor, None);
        assert_eq!(report.plan.from_seq, 1);
        assert_eq!(report.applied_entries, 1);
        assert_eq!(bus_calls.get(), 0);
        assert_eq!(account.last_event(), Some(state));
        assert_eq!(account.base_currency(), Some(Currency::USD()));
    }

    #[rstest]
    fn unsupported_payload_is_ignored() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-replay")).expect("open");
        backend
            .append_batch(&[append_payload(
                1,
                "RunStarted",
                Bytes::copy_from_slice(UUID4::new().to_string().as_bytes()),
            )])
            .expect("append");
        let reader = EventStoreReader::new(backend);
        let mut cache = Cache::default();

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");

        assert_eq!(report.applied_entries, 0);
        assert_eq!(report.ignored_entries, 1);
    }

    #[rstest]
    fn order_fill_replay_updates_order_and_creates_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let initialized = OrderInitializedSpec::builder()
            .instrument_id(instrument.id())
            .build();
        let client_order_id = initialized.client_order_id;
        let submitted = OrderSubmittedSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .build();
        let accepted = OrderAcceptedSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .account_id(submitted.account_id)
            .build();
        let filled = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(accepted.venue_order_id)
            .account_id(submitted.account_id)
            .position_id(position_id)
            .commission(Money::from("1 USD"))
            .build();
        let filled_event = OrderEventAny::Filled(filled);
        let reader = reader_with_entries(
            "run-order-replay",
            &[
                append_order_event(1, &OrderEventAny::Initialized(initialized)),
                append_order_event(2, &OrderEventAny::Submitted(submitted)),
                append_order_event(3, &OrderEventAny::Accepted(accepted)),
                append_order_event(4, &filled_event),
            ],
        );
        let mut cache = Cache::default();
        cache.add_instrument(instrument).expect("add instrument");

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let order = cache.order_owned(&client_order_id).expect("order replayed");
        let position = cache
            .position_owned(&position_id)
            .expect("position replayed");

        assert_eq!(report.applied_entries, 4);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.event_count(), 4);
        assert_eq!(order.last_event(), &filled_event);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.last_event(), Some(filled));
        assert_eq!(position.trade_ids(), vec![filled.trade_id]);
        assert_eq!(position.commissions(), vec![Money::from("1 USD")]);
    }

    #[rstest]
    fn position_adjustment_replay_updates_existing_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .build();
        let position = Position::new(&instrument, fill);
        let adjustment = PositionAdjusted::new(
            fill.trader_id,
            fill.strategy_id,
            fill.instrument_id,
            position_id,
            fill.account_id,
            PositionAdjustmentType::Funding,
            None,
            Some(Money::from("2 USD")),
            Some(Ustr::from("funding")),
            UUID4::new(),
            UnixNanos::from(10),
            UnixNanos::from(11),
        );
        let entry = append_position_event(1, &PositionEvent::PositionAdjusted(adjustment)).entry;
        let mut cache = Cache::default();
        cache
            .add_position(&position, OmsType::Unspecified)
            .expect("seed position");

        let applied = apply_cache_replay_entry(&mut cache, &entry).expect("apply");
        let position = cache
            .position_owned(&position_id)
            .expect("position updated");

        assert!(applied);
        assert_eq!(position.adjustments, vec![adjustment]);
        assert_eq!(position.realized_pnl, Some(Money::from("2 USD")));
        assert_eq!(position.ts_last, adjustment.ts_event);
    }

    #[rstest]
    fn duplicate_position_fill_is_not_applied_twice() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .commission(Money::from("1 USD"))
            .build();
        let position = Position::new(&instrument, fill);
        let entry = append_order_event(1, &OrderEventAny::Filled(fill)).entry;
        let mut cache = Cache::default();
        cache
            .add_position(&position, OmsType::Unspecified)
            .expect("seed position");

        apply_fill_to_position(&mut cache, &entry, &fill).expect("apply fill");
        let position = cache
            .position_owned(&position_id)
            .expect("position updated");

        assert_eq!(position.event_count(), 1);
        assert_eq!(position.trade_ids(), vec![fill.trade_id]);
        assert_eq!(position.commissions(), vec![Money::from("1 USD")]);
    }

    #[rstest]
    fn corrupt_supported_payload_returns_decode_error() {
        let reader = reader_with_entries(
            "run-decode-error",
            &[append_payload(
                1,
                PAYLOAD_TYPE_ACCOUNT_STATE,
                Bytes::copy_from_slice(&[0xc1]),
            )],
        );
        let mut cache = Cache::default();

        let err = replay_cache_snapshot_tail(&mut cache, &reader).expect_err("decode error");

        match err {
            CacheReplayError::Decode {
                seq, payload_type, ..
            } => {
                assert_eq!(seq, 1);
                assert_eq!(payload_type, PAYLOAD_TYPE_ACCOUNT_STATE);
            }
            other => panic!("expected Decode, was {other:?}"),
        }
    }

    #[rstest]
    fn missing_order_event_returns_apply_error() {
        let submitted = OrderSubmittedSpec::builder().build();
        let reader = reader_with_entries(
            "run-apply-error",
            &[append_order_event(1, &OrderEventAny::Submitted(submitted))],
        );
        let mut cache = Cache::default();

        let err = replay_cache_snapshot_tail(&mut cache, &reader).expect_err("apply error");

        match err {
            CacheReplayError::Apply {
                seq,
                payload_type,
                message,
            } => {
                assert_eq!(seq, 1);
                assert_eq!(payload_type, PAYLOAD_TYPE_ORDER_SUBMITTED);
                assert!(
                    message.contains("not found"),
                    "message should include cache apply failure: {message}",
                );
            }
            other => panic!("expected Apply, was {other:?}"),
        }
    }
}
