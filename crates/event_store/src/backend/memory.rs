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

//! In-memory [`EventStore`] backend used by writer and reader unit tests and by the
//! `cfg(madsim)` simulation backend.

use indexmap::IndexMap;
use nautilus_core::UnixNanos;

use crate::{
    backend::{AppendEntry, EventStore, IndexKey, IndexKind, ScanDirection},
    entry::EventStoreEntry,
    error::EventStoreError,
    manifest::{RunManifest, RunStatus},
    snapshot::{SnapshotAnchor, validate_new_anchor},
};

/// In-memory implementation of [`EventStore`].
///
/// Stores entries densely in a `Vec` keyed by `seq - 1` plus one [`IndexMap`] per
/// [`IndexKind`] for the sidecar indices. Hash recomputation on read is structurally
/// redundant (entries live in process memory) but kept for parity with persistent
/// backends so callers see uniform behavior.
///
/// One backend instance owns at most one open run at a time. Sealing the open run leaves
/// the manifest and entries readable until the next [`EventStore::open_run`] call replaces
/// them with a fresh run. Reopening while a `Running` run still exists returns
/// [`EventStoreError::CrashedPredecessor`] so callers exercise the same crash-recovery
/// path persistent backends surface on reopen.
#[derive(Debug, Default)]
pub struct MemoryBackend {
    state: Option<RunState>,
}

#[derive(Debug)]
struct RunState {
    manifest: RunManifest,
    entries: Vec<EventStoreEntry>,
    indices: Indices,
    snapshot_anchor: Option<SnapshotAnchor>,
    high_watermark: u64,
    max_ts_init: UnixNanos,
}

#[derive(Debug, Default)]
struct Indices {
    intent: IndexMap<String, u64>,
    client_order: IndexMap<String, u64>,
    venue_order: IndexMap<String, u64>,
}

impl Indices {
    fn map_for(&self, kind: IndexKind) -> &IndexMap<String, u64> {
        match kind {
            IndexKind::IntentId => &self.intent,
            IndexKind::ClientOrderId => &self.client_order,
            IndexKind::VenueOrderId => &self.venue_order,
        }
    }

    fn map_for_mut(&mut self, kind: IndexKind) -> &mut IndexMap<String, u64> {
        match kind {
            IndexKind::IntentId => &mut self.intent,
            IndexKind::ClientOrderId => &mut self.client_order,
            IndexKind::VenueOrderId => &mut self.venue_order,
        }
    }
}

impl MemoryBackend {
    /// Creates a new empty [`MemoryBackend`] with no run open.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> Result<&RunState, EventStoreError> {
        self.state
            .as_ref()
            .ok_or_else(|| EventStoreError::Backend("no run open".to_string()))
    }

    fn state_mut(&mut self) -> Result<&mut RunState, EventStoreError> {
        self.state
            .as_mut()
            .ok_or_else(|| EventStoreError::Backend("no run open".to_string()))
    }
}

impl EventStore for MemoryBackend {
    fn open_run(&mut self, mut manifest: RunManifest) -> Result<(), EventStoreError> {
        if let Some(state) = &self.state
            && !state.manifest.is_sealed()
        {
            return Err(EventStoreError::CrashedPredecessor);
        }

        manifest.status = RunStatus::Running;
        manifest.end_ts_init = None;
        manifest.high_watermark = 0;

        self.state = Some(RunState {
            manifest,
            entries: Vec::new(),
            indices: Indices::default(),
            snapshot_anchor: None,
            high_watermark: 0,
            max_ts_init: UnixNanos::default(),
        });
        Ok(())
    }

    fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
        let state = self.state_mut()?;

        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }

        if entries.is_empty() {
            return Ok(state.high_watermark);
        }

        for (expected, append) in (state.high_watermark + 1..).zip(entries.iter()) {
            if append.entry.seq != expected {
                // Batch is atomically rejected: report the durable high-watermark, not
                // the within-batch validation cursor, so callers that resync from this
                // value never skip entries that were never committed.
                return Err(EventStoreError::OutOfOrder {
                    high_watermark: state.high_watermark,
                    seq: append.entry.seq,
                });
            }
        }

        for append in entries {
            for IndexKey { kind, key } in &append.index_keys {
                state
                    .indices
                    .map_for_mut(*kind)
                    .entry(key.clone())
                    .or_insert(append.entry.seq);
            }

            if append.entry.ts_init > state.max_ts_init {
                state.max_ts_init = append.entry.ts_init;
            }
            state.high_watermark = append.entry.seq;
            state.entries.push(append.entry.clone());
        }

        state.manifest.high_watermark = state.high_watermark;
        Ok(state.high_watermark)
    }

    fn scan_range(
        &self,
        from: u64,
        to: u64,
        direction: ScanDirection,
    ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        let state = self.state()?;

        if from > to || from == 0 || state.entries.is_empty() {
            return Ok(Vec::new());
        }

        let lo = usize::try_from(from)
            .unwrap_or(usize::MAX)
            .saturating_sub(1);
        let hi = usize::try_from(to)
            .unwrap_or(usize::MAX)
            .min(state.entries.len());

        if lo >= hi {
            return Ok(Vec::new());
        }

        let slice = &state.entries[lo..hi];
        for entry in slice {
            if entry.recompute_hash() != entry.entry_hash {
                return Err(EventStoreError::HashMismatch { seq: entry.seq });
            }
        }

        let mut out: Vec<EventStoreEntry> = slice.to_vec();
        if matches!(direction, ScanDirection::Reverse) {
            out.reverse();
        }
        Ok(out)
    }

    fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
        let state = self.state()?;

        if seq == 0 || seq > state.high_watermark {
            return Ok(None);
        }

        let idx = usize::try_from(seq - 1)
            .map_err(|e| EventStoreError::Backend(format!("seq {seq} out of usize range: {e}")))?;
        let entry = &state.entries[idx];
        if entry.recompute_hash() != entry.entry_hash {
            return Err(EventStoreError::HashMismatch { seq });
        }
        Ok(Some(entry.clone()))
    }

    fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
        let state = self.state()?;
        Ok(state.indices.map_for(kind).get(key).copied())
    }

    fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
        let state = self.state()?;
        Ok(state
            .indices
            .map_for(kind)
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect())
    }

    fn record_snapshot_anchor(&mut self, anchor: SnapshotAnchor) -> Result<(), EventStoreError> {
        let state = self.state_mut()?;

        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }

        validate_new_anchor(
            &anchor,
            state.high_watermark,
            state.snapshot_anchor.as_ref(),
        )?;
        state.snapshot_anchor = Some(anchor);
        Ok(())
    }

    fn latest_snapshot_anchor(&self) -> Result<Option<SnapshotAnchor>, EventStoreError> {
        Ok(self.state()?.snapshot_anchor.clone())
    }

    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
        let state = self.state_mut()?;

        // `RunStatus::Running` is not a terminal state; accepting it would leave the
        // manifest unsealed (`is_sealed()` returns false) while still returning Ok,
        // so subsequent `append_batch` calls would not see `Closed`.
        if matches!(status, RunStatus::Running) {
            return Err(EventStoreError::Backend(
                "seal status must be a terminal state, was Running".to_string(),
            ));
        }

        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }

        state.manifest.status = status;
        state.manifest.high_watermark = state.high_watermark;
        if state.high_watermark > 0 {
            state.manifest.end_ts_init = Some(state.max_ts_init);
        }
        Ok(())
    }

    fn manifest(&self) -> Result<RunManifest, EventStoreError> {
        Ok(self.state()?.manifest.clone())
    }

    fn high_watermark(&self) -> Result<u64, EventStoreError> {
        Ok(self.state()?.high_watermark)
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_core::{UUID4, UnixNanos};
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::*;
    use crate::{
        compute_entry_hash,
        entry::{EventStoreEntry, Topic},
        headers::Headers,
        manifest::{RegisteredComponents, RunManifest, RunStatus},
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

    fn build_entry(seq: u64, headers: Headers, ts_init: u64) -> EventStoreEntry {
        let topic: Topic = "exec.command.SubmitOrder".into();
        let payload_type = Ustr::from("SubmitOrder");
        let payload = Bytes::from_static(b"\x01\x02\x03\x04");
        let ts_publish = UnixNanos::from(ts_init + 1);
        let ts_init = UnixNanos::from(ts_init);
        let hash = compute_entry_hash(
            seq,
            ts_init,
            ts_publish,
            topic.as_ref(),
            payload_type.as_str(),
            &payload,
            &headers,
        );

        EventStoreEntry::new(
            hash,
            seq,
            headers,
            topic,
            payload_type,
            payload,
            ts_init,
            ts_publish,
        )
    }

    fn append_with(seq: u64, ts_init: u64, index_keys: Vec<IndexKey>) -> AppendEntry {
        AppendEntry::new(build_entry(seq, Headers::empty(), ts_init), index_keys)
    }

    #[fixture]
    fn open_backend() -> MemoryBackend {
        let mut backend = MemoryBackend::new();
        backend
            .open_run(manifest("1700000000-aaaa1111"))
            .expect("open run");
        backend
    }

    #[rstest]
    fn manifest_errors_when_no_run_open() {
        let backend = MemoryBackend::new();

        match backend.manifest() {
            Err(EventStoreError::Backend(msg)) => {
                assert!(msg.contains("no run open"), "msg was: {msg}");
            }
            other => panic!("expected Backend, was {other:?}"),
        }

        match backend.high_watermark() {
            Err(EventStoreError::Backend(msg)) => {
                assert!(msg.contains("no run open"), "msg was: {msg}");
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    #[case::append_batch("append_batch")]
    #[case::scan_range("scan_range")]
    #[case::scan_seq("scan_seq")]
    #[case::lookup("lookup")]
    #[case::record_snapshot_anchor("record_snapshot_anchor")]
    #[case::latest_snapshot_anchor("latest_snapshot_anchor")]
    #[case::seal("seal")]
    fn methods_error_when_no_run_open(#[case] op: &str) {
        let mut backend = MemoryBackend::new();
        let err = match op {
            "append_batch" => backend.append_batch(&[]).unwrap_err(),
            "scan_range" => backend
                .scan_range(1, 1, ScanDirection::Forward)
                .unwrap_err(),
            "scan_seq" => backend.scan_seq(1).unwrap_err(),
            "lookup" => backend.lookup(IndexKind::IntentId, "k").unwrap_err(),
            "record_snapshot_anchor" => backend
                .record_snapshot_anchor(SnapshotAnchor::new(0, "blob", "hash"))
                .unwrap_err(),
            "latest_snapshot_anchor" => backend.latest_snapshot_anchor().unwrap_err(),
            "seal" => backend.seal(RunStatus::Ended).unwrap_err(),
            _ => unreachable!(),
        };

        match err {
            EventStoreError::Backend(msg) => {
                assert!(msg.contains("no run open"), "msg was: {msg}");
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    fn open_run_normalizes_status_and_zeroes_progress(open_backend: MemoryBackend) {
        let m = open_backend.manifest().expect("manifest");

        assert_eq!(m.status, RunStatus::Running);
        assert_eq!(m.high_watermark, 0);
        assert!(m.end_ts_init.is_none());
        assert_eq!(open_backend.high_watermark().expect("hwm"), 0);
    }

    #[rstest]
    fn append_advances_high_watermark(mut open_backend: MemoryBackend) {
        let batch = vec![
            append_with(1, 10, Vec::new()),
            append_with(2, 11, Vec::new()),
            append_with(3, 12, Vec::new()),
        ];

        let hwm = open_backend.append_batch(&batch).expect("append");

        assert_eq!(hwm, 3);
        assert_eq!(open_backend.high_watermark().expect("hwm"), 3);
        assert_eq!(open_backend.manifest().expect("m").high_watermark, 3);
    }

    #[rstest]
    fn append_rejects_first_seq_not_at_watermark_plus_one(mut open_backend: MemoryBackend) {
        let batch = vec![append_with(2, 10, Vec::new())];

        let err = open_backend.append_batch(&batch).expect_err("must reject");

        assert!(matches!(
            err,
            EventStoreError::OutOfOrder {
                high_watermark: 0,
                seq: 2,
            }
        ));
    }

    #[rstest]
    fn append_rejects_within_batch_seq_gap(mut open_backend: MemoryBackend) {
        let batch = vec![
            append_with(1, 10, Vec::new()),
            append_with(3, 11, Vec::new()),
        ];

        let err = open_backend.append_batch(&batch).expect_err("must reject");

        // Atomically rejected: durable hwm is still 0, not the within-batch cursor.
        assert!(matches!(
            err,
            EventStoreError::OutOfOrder {
                high_watermark: 0,
                seq: 3,
            }
        ));
        // Failed batch must not have partially landed.
        assert_eq!(open_backend.high_watermark().expect("hwm"), 0);
    }

    #[rstest]
    fn append_after_seal_returns_closed(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
        open_backend.seal(RunStatus::Ended).expect("seal");

        let err = open_backend
            .append_batch(&[append_with(2, 11, Vec::new())])
            .expect_err("must reject");

        assert!(matches!(err, EventStoreError::Closed));
    }

    #[rstest]
    fn empty_batch_is_a_noop(mut open_backend: MemoryBackend) {
        let hwm = open_backend.append_batch(&[]).expect("append");

        assert_eq!(hwm, 0);
        assert_eq!(open_backend.high_watermark().expect("hwm"), 0);
    }

    #[rstest]
    fn snapshot_anchor_is_none_until_recorded(open_backend: MemoryBackend) {
        assert!(
            open_backend
                .latest_snapshot_anchor()
                .expect("latest anchor")
                .is_none()
        );
    }

    #[rstest]
    fn snapshot_anchor_round_trips(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
        let anchor = SnapshotAnchor::new(1, "cache://snapshots/run-1/1", "blake3:abc");

        open_backend
            .record_snapshot_anchor(anchor.clone())
            .expect("record anchor");

        assert_eq!(
            open_backend
                .latest_snapshot_anchor()
                .expect("latest anchor"),
            Some(anchor),
        );
    }

    #[rstest]
    fn snapshot_anchor_rejects_watermark_past_durable_hwm(mut open_backend: MemoryBackend) {
        let anchor = SnapshotAnchor::new(1, "cache://snapshots/run-1/1", "blake3:abc");
        let err = open_backend
            .record_snapshot_anchor(anchor)
            .expect_err("must reject");

        match err {
            EventStoreError::Backend(msg) => {
                assert!(
                    msg.contains("exceeds durable high_watermark"),
                    "msg was: {msg}",
                );
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    fn snapshot_anchor_rejects_backward_move(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
            ])
            .expect("append");
        open_backend
            .record_snapshot_anchor(SnapshotAnchor::new(2, "latest", "hash-latest"))
            .expect("record latest");

        let err = open_backend
            .record_snapshot_anchor(SnapshotAnchor::new(1, "older", "hash-older"))
            .expect_err("must reject older anchor");

        match err {
            EventStoreError::Backend(msg) => {
                assert!(msg.contains("older than latest anchor"), "msg was: {msg}");
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    fn snapshot_anchor_after_seal_returns_closed(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
        open_backend.seal(RunStatus::Ended).expect("seal");

        let err = open_backend
            .record_snapshot_anchor(SnapshotAnchor::new(1, "blob", "hash"))
            .expect_err("must reject");

        assert!(matches!(err, EventStoreError::Closed));
    }

    #[rstest]
    fn scan_seq_returns_committed_entry(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
            ])
            .expect("append");

        let entry = open_backend.scan_seq(2).expect("scan").expect("present");

        assert_eq!(entry.seq, 2);
        assert_eq!(entry.ts_init, UnixNanos::from(11));
    }

    #[rstest]
    fn scan_seq_returns_none_outside_watermark(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");

        assert!(open_backend.scan_seq(0).expect("scan").is_none());
        assert!(open_backend.scan_seq(2).expect("scan").is_none());
    }

    #[rstest]
    #[case::forward_full(1, 3, ScanDirection::Forward, vec![1, 2, 3])]
    #[case::reverse_full(1, 3, ScanDirection::Reverse, vec![3, 2, 1])]
    #[case::forward_window(2, 3, ScanDirection::Forward, vec![2, 3])]
    #[case::reverse_window(2, 3, ScanDirection::Reverse, vec![3, 2])]
    #[case::clipped_to_watermark(2, 99, ScanDirection::Forward, vec![2, 3])]
    #[case::reverse_clipped(2, 99, ScanDirection::Reverse, vec![3, 2])]
    #[case::empty_inverted(3, 1, ScanDirection::Forward, vec![])]
    #[case::empty_zero(0, 0, ScanDirection::Forward, vec![])]
    fn scan_range_yields_expected_seqs(
        mut open_backend: MemoryBackend,
        #[case] from: u64,
        #[case] to: u64,
        #[case] direction: ScanDirection,
        #[case] expected: Vec<u64>,
    ) {
        open_backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
                append_with(3, 12, Vec::new()),
            ])
            .expect("append");

        let seqs: Vec<u64> = open_backend
            .scan_range(from, to, direction)
            .expect("scan")
            .into_iter()
            .map(|e| e.seq)
            .collect();

        assert_eq!(seqs, expected);
    }

    #[rstest]
    fn lookup_records_first_occurrence_per_kind(mut open_backend: MemoryBackend) {
        let intent = "intent-1".to_string();
        let cl_ord = "O-1".to_string();
        let venue = "V-1".to_string();
        open_backend
            .append_batch(&[
                AppendEntry::new(
                    build_entry(1, Headers::empty(), 10),
                    vec![
                        IndexKey::new(IndexKind::IntentId, intent.clone()),
                        IndexKey::new(IndexKind::ClientOrderId, cl_ord.clone()),
                        IndexKey::new(IndexKind::VenueOrderId, venue.clone()),
                    ],
                ),
                AppendEntry::new(
                    build_entry(2, Headers::empty(), 11),
                    vec![
                        // Same keys re-emitted: lookups must continue to point at seq=1.
                        IndexKey::new(IndexKind::IntentId, intent.clone()),
                        IndexKey::new(IndexKind::ClientOrderId, cl_ord.clone()),
                        IndexKey::new(IndexKind::VenueOrderId, venue.clone()),
                    ],
                ),
            ])
            .expect("append");

        assert_eq!(
            open_backend
                .lookup(IndexKind::IntentId, &intent)
                .expect("lookup"),
            Some(1),
        );
        assert_eq!(
            open_backend
                .lookup(IndexKind::ClientOrderId, &cl_ord)
                .expect("lookup"),
            Some(1),
        );
        assert_eq!(
            open_backend
                .lookup(IndexKind::VenueOrderId, &venue)
                .expect("lookup"),
            Some(1),
        );
        assert!(
            open_backend
                .lookup(IndexKind::IntentId, "missing")
                .expect("lookup")
                .is_none(),
        );
    }

    #[rstest]
    fn within_entry_duplicate_keys_resolve_to_first_seq(mut open_backend: MemoryBackend) {
        // First-write-wins applies within a single entry's index_keys vec as well
        // as across entries: a duplicate key (within entry 1) and a later entry's
        // re-emission (entry 2) both leave the lookup pointing at seq=1.
        let key = "O-1".to_string();
        open_backend
            .append_batch(&[
                AppendEntry::new(
                    build_entry(1, Headers::empty(), 10),
                    vec![
                        IndexKey::new(IndexKind::ClientOrderId, key.clone()),
                        IndexKey::new(IndexKind::ClientOrderId, key.clone()),
                    ],
                ),
                AppendEntry::new(
                    build_entry(2, Headers::empty(), 11),
                    vec![IndexKey::new(IndexKind::ClientOrderId, key.clone())],
                ),
            ])
            .expect("append");

        assert_eq!(
            open_backend
                .lookup(IndexKind::ClientOrderId, &key)
                .expect("lookup"),
            Some(1),
        );
    }

    #[rstest]
    fn lookup_isolates_keys_by_kind(mut open_backend: MemoryBackend) {
        // Same string under two different IndexKinds must not collide.
        let key = "shared".to_string();
        open_backend
            .append_batch(&[AppendEntry::new(
                build_entry(1, Headers::empty(), 10),
                vec![IndexKey::new(IndexKind::ClientOrderId, key.clone())],
            )])
            .expect("append");

        assert_eq!(
            open_backend
                .lookup(IndexKind::ClientOrderId, &key)
                .expect("lookup"),
            Some(1),
        );
        assert!(
            open_backend
                .lookup(IndexKind::VenueOrderId, &key)
                .expect("lookup")
                .is_none(),
        );
    }

    #[rstest]
    #[case::ended(RunStatus::Ended)]
    #[case::crashed_recovered(RunStatus::CrashedRecovered)]
    #[case::quarantined(RunStatus::Quarantined)]
    fn seal_stamps_end_ts_and_blocks_re_seal(
        mut open_backend: MemoryBackend,
        #[case] status: RunStatus,
    ) {
        open_backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 25, Vec::new()),
                append_with(3, 17, Vec::new()),
            ])
            .expect("append");

        open_backend.seal(status).expect("seal");

        let m = open_backend.manifest().expect("manifest");
        assert_eq!(m.status, status);
        assert_eq!(m.high_watermark, 3);
        // Highest ts_init across the run, not the last-arrived.
        assert_eq!(m.end_ts_init, Some(UnixNanos::from(25)));

        let err = open_backend.seal(RunStatus::Ended).expect_err("re-seal");
        assert!(matches!(err, EventStoreError::Closed));
    }

    #[rstest]
    fn seal_rejects_running_status(mut open_backend: MemoryBackend) {
        // Running is not a terminal state. Rejecting it keeps the seal contract
        // honest: a successful seal must make subsequent appends return Closed.
        let err = open_backend
            .seal(RunStatus::Running)
            .expect_err("must reject");

        match err {
            EventStoreError::Backend(msg) => {
                assert!(msg.contains("Running"), "msg was: {msg}");
            }
            other => panic!("expected Backend, was {other:?}"),
        }
        assert!(!open_backend.manifest().expect("manifest").is_sealed());
        // The run is still writeable.
        open_backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
    }

    #[rstest]
    fn seal_with_no_entries_leaves_end_ts_unset(mut open_backend: MemoryBackend) {
        open_backend.seal(RunStatus::Ended).expect("seal");

        let m = open_backend.manifest().expect("manifest");
        assert_eq!(m.status, RunStatus::Ended);
        assert!(m.end_ts_init.is_none());
        assert_eq!(m.high_watermark, 0);
    }

    #[rstest]
    fn reopening_running_run_returns_crashed_predecessor() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-1")).expect("open 1");
        backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");

        // Caller forgot to seal; the second open_run flags it for crash recovery.
        let err = backend.open_run(manifest("run-2")).expect_err("must flag");
        assert!(matches!(err, EventStoreError::CrashedPredecessor));

        // The failed open must preserve the predecessor's entries so the verifier
        // can scan them before the kernel decides CrashedRecovered vs Quarantined.
        assert!(
            backend.scan_seq(1).expect("scan").is_some(),
            "predecessor entry must survive failed open_run",
        );

        // After sealing the predecessor, a fresh open succeeds.
        backend.seal(RunStatus::CrashedRecovered).expect("seal");
        backend.open_run(manifest("run-2")).expect("open 2");
        assert_eq!(
            backend.manifest().expect("manifest").run_id,
            "run-2".to_string(),
        );
        assert_eq!(backend.high_watermark().expect("hwm"), 0);
    }

    #[rstest]
    fn reopening_after_clean_seal_succeeds() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-1")).expect("open 1");
        backend.seal(RunStatus::Ended).expect("seal");

        backend.open_run(manifest("run-2")).expect("open 2");
        assert_eq!(
            backend.manifest().expect("manifest").run_id,
            "run-2".to_string(),
        );
    }

    #[rstest]
    fn scan_recomputes_hash_and_quarantines_on_mismatch(mut open_backend: MemoryBackend) {
        // Tampered entry: payload doesn't match the stored entry_hash. Scans must
        // return HashMismatch rather than silently surfacing the corrupted row.
        let mut tampered = build_entry(1, Headers::empty(), 10);
        tampered.payload = Bytes::from_static(b"\xFF\xFF");
        open_backend
            .append_batch(&[AppendEntry::without_indices(tampered)])
            .expect("append");

        assert!(matches!(
            open_backend.scan_seq(1),
            Err(EventStoreError::HashMismatch { seq: 1 }),
        ));
        assert!(matches!(
            open_backend.scan_range(1, 1, ScanDirection::Forward),
            Err(EventStoreError::HashMismatch { seq: 1 }),
        ));
    }

    #[rstest]
    fn append_extracts_no_indices_when_keys_empty(mut open_backend: MemoryBackend) {
        // Backend treats AppendEntry::index_keys as the sole authority. Headers on
        // the entry are not auto-extracted; the writer/encoder is responsible.
        let headers = Headers {
            intent_id: Some(UUID4::new()),
            ..Headers::empty()
        };
        open_backend
            .append_batch(&[AppendEntry::without_indices(build_entry(1, headers, 10))])
            .expect("append");

        assert!(
            open_backend
                .lookup(IndexKind::IntentId, "any")
                .expect("lookup")
                .is_none(),
        );
    }

    #[rstest]
    fn iter_index_keys_enumerates_first_write_wins_pairs(mut open_backend: MemoryBackend) {
        // Walks every (key, seq) pair the verifier needs to cross-check the
        // sidecar indices: distinct kinds stay isolated, duplicate keys hold the
        // first-seen seq, and unrelated kinds return empty without leaking pairs
        // across kind boundaries.
        open_backend
            .append_batch(&[
                AppendEntry::new(
                    build_entry(1, Headers::empty(), 10),
                    vec![
                        IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string()),
                        IndexKey::new(IndexKind::VenueOrderId, "V-1".to_string()),
                    ],
                ),
                AppendEntry::new(
                    build_entry(2, Headers::empty(), 11),
                    vec![
                        // Re-emit O-1: first-write-wins must keep the seq=1 entry.
                        IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string()),
                        IndexKey::new(IndexKind::ClientOrderId, "O-2".to_string()),
                    ],
                ),
            ])
            .expect("append");

        let mut client = open_backend
            .iter_index_keys(IndexKind::ClientOrderId)
            .expect("iter");
        client.sort();
        assert_eq!(
            client,
            vec![("O-1".to_string(), 1u64), ("O-2".to_string(), 2u64)],
        );

        let venue = open_backend
            .iter_index_keys(IndexKind::VenueOrderId)
            .expect("iter");
        assert_eq!(venue, vec![("V-1".to_string(), 1u64)]);

        // No intent_id pairs were emitted; the iter must return an empty vec
        // rather than reusing the client/venue contents.
        assert!(
            open_backend
                .iter_index_keys(IndexKind::IntentId)
                .expect("iter")
                .is_empty(),
        );
    }

    #[rstest]
    fn iter_index_keys_errors_when_no_run_open() {
        let backend = MemoryBackend::new();

        match backend.iter_index_keys(IndexKind::IntentId) {
            Err(EventStoreError::Backend(msg)) => {
                assert!(msg.contains("no run open"), "msg was: {msg}");
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }
}
