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

//! Reader for the event store.
//!
//! The reader composes over the locked [`EventStore`] trait. It owns one backend instance,
//! exposes range scans as a chunked iterator, single-`seq` lookups, secondary-index
//! lookups, and manifest access. The backend the reader receives may be a still-open run
//! (the writer's backend) or a sealed run produced via
//! [`crate::backend::RedbBackend::open_sealed`].
//!
//! Run iteration across a base directory lives on the redb backend itself
//! ([`crate::backend::RedbBackend::list_runs`]) because it depends on the on-disk file
//! layout; the in-memory backend has no analog and reads its single open run in place.

use std::{collections::VecDeque, fmt::Debug};

use crate::{
    backend::{EventStore, IndexKind, ScanDirection},
    entry::EventStoreEntry,
    error::EventStoreError,
    manifest::RunManifest,
    snapshot::SnapshotAnchor,
};

/// Default number of entries materialized per chunked `scan_range` call.
///
/// Chosen so a forensics scan of a multi-million-entry run keeps the live working set
/// bounded while amortizing the per-call transaction overhead. Tune through
/// [`EventStoreReader::scan_range_chunked`] when a workload prefers different bounds.
pub const DEFAULT_SCAN_CHUNK_SIZE: u64 = 1_024;

/// Replay bounds derived from the latest cache snapshot anchor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotReplayPlan {
    /// Latest cache snapshot anchor, or `None` when restore must replay from the start.
    pub anchor: Option<SnapshotAnchor>,
    /// First event-store seq to replay after the cache snapshot restore.
    pub from_seq: u64,
    /// Current durable high-watermark for the run.
    pub to_seq: u64,
}

impl SnapshotReplayPlan {
    /// Returns whether there are no entries to replay for this plan.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.from_seq > self.to_seq
    }
}

/// Read-only handle over an [`EventStore`] backend.
///
/// The reader is the canonical entry point for read-only replay, audit, and verifier
/// scans: it never mutates the backend (no `append_batch` surface) and it tolerates
/// running and sealed backends uniformly.
#[derive(Debug)]
pub struct EventStoreReader<B> {
    backend: B,
}

impl<B: EventStore> EventStoreReader<B> {
    /// Wraps `backend` for read-only access.
    #[must_use]
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Returns the manifest of the open run.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open.
    pub fn manifest(&self) -> Result<RunManifest, EventStoreError> {
        self.backend.manifest()
    }

    /// Returns the largest `seq` durably acknowledged for the open run.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open.
    pub fn high_watermark(&self) -> Result<u64, EventStoreError> {
        self.backend.high_watermark()
    }

    /// Reads a single entry by `seq`.
    ///
    /// Returns `None` when `seq == 0` or `seq` exceeds the current high-watermark.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::HashMismatch`] when the recomputed entry hash diverges
    /// from the stored value, [`EventStoreError::Gap`] when the row is missing inside
    /// the high-watermark, and [`EventStoreError::Backend`] for unclassified backend
    /// failures.
    pub fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
        self.backend.scan_seq(seq)
    }

    /// Looks up the first `seq` recorded under the given index key.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] for unclassified backend failures.
    pub fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
        self.backend.lookup(kind, key)
    }

    /// Returns the latest snapshot anchor for the run.
    ///
    /// Returns `Ok(None)` when no snapshot anchor has been recorded yet.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open or the backend does not
    /// support snapshot anchors, and [`EventStoreError::Corrupted`] when a stored anchor
    /// cannot decode.
    pub fn latest_snapshot_anchor(&self) -> Result<Option<SnapshotAnchor>, EventStoreError> {
        self.backend.latest_snapshot_anchor()
    }

    /// Builds the restore replay bounds from the latest snapshot anchor.
    ///
    /// Restore callers fetch and validate the cache-owned snapshot blob first, then
    /// replay entries in `[from_seq, to_seq]`. When an anchor exists, `from_seq` is
    /// `anchor.high_watermark + 1`; without an anchor, restore replays from seq `1`.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open or the backend does not
    /// support snapshot anchors, and [`EventStoreError::Corrupted`] when the stored
    /// anchor points past the durable high-watermark.
    pub fn snapshot_replay_plan(&self) -> Result<SnapshotReplayPlan, EventStoreError> {
        let anchor = self.latest_snapshot_anchor()?;
        let to_seq = self.high_watermark()?;
        let from_seq = match anchor.as_ref() {
            Some(anchor) if anchor.high_watermark > to_seq => {
                return Err(EventStoreError::Corrupted(format!(
                    "snapshot anchor high_watermark {} exceeds durable high_watermark {to_seq}",
                    anchor.high_watermark,
                )));
            }
            Some(anchor) => anchor.high_watermark.saturating_add(1),
            None => 1,
        };

        Ok(SnapshotReplayPlan {
            anchor,
            from_seq,
            to_seq,
        })
    }

    /// Scans the forward replay tail after the latest snapshot anchor.
    ///
    /// This pairs [`Self::snapshot_replay_plan`] with the actual event iterator used by
    /// restore: entries start at `anchor.high_watermark + 1` when an anchor exists, or
    /// at seq `1` when no cache snapshot has been anchored.
    ///
    /// # Errors
    ///
    /// See [`Self::snapshot_replay_plan`].
    pub fn scan_snapshot_replay_tail(
        &self,
    ) -> Result<(SnapshotReplayPlan, RangeScan<'_>), EventStoreError> {
        let plan = self.snapshot_replay_plan()?;
        let scan = self.scan_range(plan.from_seq, plan.to_seq, ScanDirection::Forward);
        Ok((plan, scan))
    }

    /// Scans entries by `seq` over the inclusive range `[from, to]`.
    ///
    /// The returned iterator pulls [`DEFAULT_SCAN_CHUNK_SIZE`] entries at a time from the
    /// backend so a multi-million-entry forensics scan keeps the working set bounded
    /// while still amortizing per-transaction overhead. The iterator yields one entry
    /// per call; backend errors surface as `Some(Err(...))` and terminate the scan.
    #[must_use]
    pub fn scan_range(&self, from: u64, to: u64, direction: ScanDirection) -> RangeScan<'_> {
        RangeScan::new(&self.backend, from, to, direction, DEFAULT_SCAN_CHUNK_SIZE)
    }

    /// Variant of [`Self::scan_range`] with a caller-chosen chunk size.
    ///
    /// `chunk_size == 0` is normalized to `1`; the reader never asks the backend for a
    /// degenerate empty window because the chunk window is the only progress signal the
    /// iterator advances on.
    #[must_use]
    pub fn scan_range_chunked(
        &self,
        from: u64,
        to: u64,
        direction: ScanDirection,
        chunk_size: u64,
    ) -> RangeScan<'_> {
        RangeScan::new(&self.backend, from, to, direction, chunk_size.max(1))
    }

    /// Returns the underlying backend, consuming the reader.
    #[must_use]
    pub fn into_inner(self) -> B {
        self.backend
    }

    /// Returns a reference to the underlying backend.
    #[must_use]
    pub const fn backend(&self) -> &B {
        &self.backend
    }
}

/// Lazy iterator over a `seq` range, materialized in chunks.
///
/// Created by [`EventStoreReader::scan_range`] and
/// [`EventStoreReader::scan_range_chunked`]. The iterator owns no transaction lifetime:
/// each chunk opens a fresh [`EventStore::scan_range`] call against the backend, so a
/// long-running scan is not held open against a writer's commit cadence.
pub struct RangeScan<'a> {
    backend: &'a dyn EventStore,
    direction: ScanDirection,
    chunk_size: u64,
    cursor: u64,
    end: u64,
    buffer: VecDeque<EventStoreEntry>,
    has_more: bool,
    // Reverse scans must clamp their starting cursor against the durable
    // high-watermark on the first fetch. Without that step, a `to` value
    // above the watermark (a forensics caller passing an open upper bound,
    // or simply `to = u64::MAX`) makes the first chunk lie wholly above
    // the durable rows; the backend clips that chunk to an empty Vec and
    // the iterator would terminate before reading the rows below the
    // watermark. Forward scans are not affected: an empty forward chunk
    // genuinely means we have walked past the watermark.
    reverse_clamped: bool,
}

impl Debug for RangeScan<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RangeScan))
            .field("direction", &self.direction)
            .field("chunk_size", &self.chunk_size)
            .field("cursor", &self.cursor)
            .field("end", &self.end)
            .field("buffered", &self.buffer.len())
            .field("has_more", &self.has_more)
            .field("reverse_clamped", &self.reverse_clamped)
            .finish()
    }
}

impl<'a> RangeScan<'a> {
    fn new(
        backend: &'a dyn EventStore,
        from: u64,
        to: u64,
        direction: ScanDirection,
        chunk_size: u64,
    ) -> Self {
        // Mirror the backend's empty-range conventions so the iterator never makes a
        // first call into the backend for a degenerate window. `from == 0` is reserved
        // (seq is 1-based); `from > to` is an empty range.
        let valid = from != 0 && from <= to;
        let (cursor, end) = if valid {
            match direction {
                ScanDirection::Forward => (from, to),
                ScanDirection::Reverse => (to, from),
            }
        } else {
            (0, 0)
        };

        Self {
            backend,
            direction,
            chunk_size: chunk_size.max(1),
            cursor,
            end,
            buffer: VecDeque::new(),
            has_more: valid,
            reverse_clamped: false,
        }
    }

    fn fetch_chunk(&mut self) -> Option<Result<(), EventStoreError>> {
        if !self.has_more {
            return None;
        }

        if matches!(self.direction, ScanDirection::Reverse) && !self.reverse_clamped {
            match self.backend.high_watermark() {
                Ok(hwm) => {
                    if hwm == 0 || hwm < self.end {
                        self.has_more = false;
                        return Some(Ok(()));
                    }
                    self.cursor = self.cursor.min(hwm);
                    self.reverse_clamped = true;
                }
                Err(e) => {
                    self.has_more = false;
                    return Some(Err(e));
                }
            }
        }
        let (chunk_lo, chunk_hi) = match self.direction {
            ScanDirection::Forward => {
                let lo = self.cursor;
                let hi = lo
                    .saturating_add(self.chunk_size)
                    .saturating_sub(1)
                    .min(self.end);
                (lo, hi)
            }
            ScanDirection::Reverse => {
                let hi = self.cursor;
                let lo = hi
                    .saturating_sub(self.chunk_size.saturating_sub(1))
                    .max(self.end);
                (lo, hi)
            }
        };

        match self.backend.scan_range(chunk_lo, chunk_hi, self.direction) {
            Ok(entries) => {
                if entries.is_empty() {
                    // The backend clipped the window to its high-watermark or the run is
                    // shorter than the requested range; either way no further chunks
                    // will yield rows.
                    self.has_more = false;
                    return Some(Ok(()));
                }

                match self.direction {
                    ScanDirection::Forward => {
                        if chunk_hi >= self.end {
                            self.has_more = false;
                        } else {
                            self.cursor = chunk_hi + 1;
                        }
                    }
                    ScanDirection::Reverse => {
                        if chunk_lo <= self.end {
                            self.has_more = false;
                        } else {
                            self.cursor = chunk_lo - 1;
                        }
                    }
                }
                self.buffer.extend(entries);
                Some(Ok(()))
            }
            Err(e) => {
                self.has_more = false;
                Some(Err(e))
            }
        }
    }
}

impl Iterator for RangeScan<'_> {
    type Item = Result<EventStoreEntry, EventStoreError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(entry) = self.buffer.pop_front() {
                return Some(Ok(entry));
            }

            match self.fetch_chunk() {
                Some(Ok(())) => {}
                Some(Err(e)) => return Some(Err(e)),
                None => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_core::UnixNanos;
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::*;
    use crate::{
        backend::{AppendEntry, IndexKey, MemoryBackend},
        compute_entry_hash,
        entry::Topic,
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

    fn build_entry(seq: u64, ts_init: u64) -> EventStoreEntry {
        let topic: Topic = "exec.command.SubmitOrder".into();
        let payload_type = Ustr::from("SubmitOrder");
        let payload = Bytes::from_static(b"\x01\x02\x03\x04");
        let headers = Headers::empty();
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
        AppendEntry::new(build_entry(seq, ts_init), index_keys)
    }

    fn populated(count: u64) -> EventStoreReader<MemoryBackend> {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-reader")).expect("open run");
        let batch: Vec<AppendEntry> = (1..=count)
            .map(|seq| append_with(seq, 100 + seq, Vec::new()))
            .collect();
        backend.append_batch(&batch).expect("append");
        EventStoreReader::new(backend)
    }

    #[derive(Debug)]
    struct AnchorPastWatermarkBackend;

    impl EventStore for AnchorPastWatermarkBackend {
        fn open_run(&mut self, _manifest: RunManifest) -> Result<(), EventStoreError> {
            Ok(())
        }

        fn append_batch(&mut self, _entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
            Ok(1)
        }

        fn scan_range(
            &self,
            _from: u64,
            _to: u64,
            _direction: ScanDirection,
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            Ok(Vec::new())
        }

        fn scan_seq(&self, _seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            Ok(None)
        }

        fn lookup(&self, _kind: IndexKind, _key: &str) -> Result<Option<u64>, EventStoreError> {
            Ok(None)
        }

        fn iter_index_keys(&self, _kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            Ok(Vec::new())
        }

        fn record_snapshot_anchor(
            &mut self,
            _anchor: SnapshotAnchor,
        ) -> Result<(), EventStoreError> {
            Ok(())
        }

        fn latest_snapshot_anchor(&self) -> Result<Option<SnapshotAnchor>, EventStoreError> {
            Ok(Some(SnapshotAnchor::new(
                2,
                "cache://snapshots/run-reader/2",
                "blake3:abc",
            )))
        }

        fn seal(&mut self, _status: RunStatus) -> Result<(), EventStoreError> {
            Ok(())
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            Ok(manifest("run-anchor-past-watermark"))
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            Ok(1)
        }
    }

    #[fixture]
    fn reader_with_three() -> EventStoreReader<MemoryBackend> {
        populated(3)
    }

    #[rstest]
    fn manifest_delegates_to_backend(reader_with_three: EventStoreReader<MemoryBackend>) {
        let m = reader_with_three.manifest().expect("manifest");

        assert_eq!(m.run_id, "run-reader");
        assert_eq!(m.high_watermark, 3);
    }

    #[rstest]
    fn high_watermark_delegates_to_backend(reader_with_three: EventStoreReader<MemoryBackend>) {
        assert_eq!(reader_with_three.high_watermark().expect("hwm"), 3);
    }

    #[rstest]
    fn latest_snapshot_anchor_delegates_to_backend() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-anchor")).expect("open run");
        backend
            .append_batch(&[append_with(1, 101, Vec::new())])
            .expect("append");
        let anchor = SnapshotAnchor::new(1, "cache://snapshots/run-anchor/1", "blake3:abc");
        backend
            .record_snapshot_anchor(anchor.clone())
            .expect("record anchor");
        let reader = EventStoreReader::new(backend);

        assert_eq!(
            reader.latest_snapshot_anchor().expect("latest anchor"),
            Some(anchor),
        );
    }

    #[rstest]
    fn snapshot_replay_plan_without_anchor_replays_from_start(
        reader_with_three: EventStoreReader<MemoryBackend>,
    ) {
        let plan = reader_with_three
            .snapshot_replay_plan()
            .expect("snapshot replay plan");

        assert_eq!(
            plan,
            SnapshotReplayPlan {
                anchor: None,
                from_seq: 1,
                to_seq: 3,
            },
        );
        assert!(!plan.is_empty());
    }

    #[rstest]
    fn snapshot_replay_plan_with_anchor_starts_after_anchor_watermark() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-anchor")).expect("open run");
        backend
            .append_batch(&[
                append_with(1, 101, Vec::new()),
                append_with(2, 102, Vec::new()),
                append_with(3, 103, Vec::new()),
            ])
            .expect("append");
        let anchor = SnapshotAnchor::new(2, "cache://snapshots/run-anchor/2", "blake3:abc");
        backend
            .record_snapshot_anchor(anchor.clone())
            .expect("record anchor");
        let reader = EventStoreReader::new(backend);

        let plan = reader.snapshot_replay_plan().expect("snapshot replay plan");

        assert_eq!(
            plan,
            SnapshotReplayPlan {
                anchor: Some(anchor),
                from_seq: 3,
                to_seq: 3,
            },
        );
        assert!(!plan.is_empty());
    }

    #[rstest]
    fn snapshot_replay_plan_rejects_anchor_past_watermark() {
        let reader = EventStoreReader::new(AnchorPastWatermarkBackend);
        let err = reader
            .snapshot_replay_plan()
            .expect_err("anchor past watermark must fail");

        match err {
            EventStoreError::Corrupted(msg) => {
                assert!(
                    msg.contains("exceeds durable high_watermark"),
                    "msg was: {msg}",
                );
            }
            other => panic!("expected Corrupted, was {other:?}"),
        }
    }

    #[rstest]
    fn scan_snapshot_replay_tail_yields_entries_after_anchor() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-anchor")).expect("open run");
        backend
            .append_batch(&[
                append_with(1, 101, Vec::new()),
                append_with(2, 102, Vec::new()),
                append_with(3, 103, Vec::new()),
            ])
            .expect("append");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(
                2,
                "cache://snapshots/run-anchor/2",
                "blake3:abc",
            ))
            .expect("record anchor");
        let reader = EventStoreReader::new(backend);

        let (plan, scan) = reader
            .scan_snapshot_replay_tail()
            .expect("snapshot replay tail");
        let seqs: Vec<_> = scan.map(|entry| entry.expect("entry").seq).collect();

        assert_eq!(plan.from_seq, 3);
        assert_eq!(seqs, vec![3]);
    }

    #[rstest]
    fn scan_snapshot_replay_tail_is_empty_when_anchor_matches_watermark() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-anchor")).expect("open run");
        backend
            .append_batch(&[
                append_with(1, 101, Vec::new()),
                append_with(2, 102, Vec::new()),
            ])
            .expect("append");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(
                2,
                "cache://snapshots/run-anchor/2",
                "blake3:abc",
            ))
            .expect("record anchor");
        let reader = EventStoreReader::new(backend);

        let (plan, scan) = reader
            .scan_snapshot_replay_tail()
            .expect("snapshot replay tail");
        let seqs: Vec<_> = scan.map(|entry| entry.expect("entry").seq).collect();

        assert_eq!(plan.from_seq, 3);
        assert_eq!(plan.to_seq, 2);
        assert!(plan.is_empty());
        assert!(seqs.is_empty());
    }

    #[rstest]
    fn scan_seq_returns_committed_entry(reader_with_three: EventStoreReader<MemoryBackend>) {
        let entry = reader_with_three
            .scan_seq(2)
            .expect("scan")
            .expect("present");

        assert_eq!(entry.seq, 2);
        assert_eq!(entry.ts_init, UnixNanos::from(102));
    }

    #[rstest]
    fn scan_seq_returns_none_outside_watermark(reader_with_three: EventStoreReader<MemoryBackend>) {
        assert!(reader_with_three.scan_seq(0).expect("scan").is_none());
        assert!(reader_with_three.scan_seq(99).expect("scan").is_none());
    }

    #[rstest]
    fn lookup_finds_recorded_index_key() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-lookup")).expect("open run");
        backend
            .append_batch(&[
                AppendEntry::new(
                    build_entry(1, 100),
                    vec![IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string())],
                ),
                AppendEntry::new(
                    build_entry(2, 101),
                    vec![IndexKey::new(IndexKind::VenueOrderId, "V-1".to_string())],
                ),
            ])
            .expect("append");
        let reader = EventStoreReader::new(backend);

        assert_eq!(
            reader
                .lookup(IndexKind::ClientOrderId, "O-1")
                .expect("lookup"),
            Some(1),
        );
        assert_eq!(
            reader
                .lookup(IndexKind::VenueOrderId, "V-1")
                .expect("lookup"),
            Some(2),
        );
        assert!(
            reader
                .lookup(IndexKind::ClientOrderId, "missing")
                .expect("lookup")
                .is_none(),
        );
    }

    #[rstest]
    fn scan_range_forward_yields_entries_in_order(
        reader_with_three: EventStoreReader<MemoryBackend>,
    ) {
        let seqs: Vec<u64> = reader_with_three
            .scan_range(1, 3, ScanDirection::Forward)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[rstest]
    fn scan_range_reverse_yields_entries_in_reverse(
        reader_with_three: EventStoreReader<MemoryBackend>,
    ) {
        let seqs: Vec<u64> = reader_with_three
            .scan_range(1, 3, ScanDirection::Reverse)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![3, 2, 1]);
    }

    #[rstest]
    fn scan_range_window_clips_to_request() {
        let reader = populated(10);

        let seqs: Vec<u64> = reader
            .scan_range(4, 7, ScanDirection::Forward)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![4, 5, 6, 7]);
    }

    #[rstest]
    fn scan_range_chunked_forward_walks_full_range() {
        // Seven entries with a chunk size of 2 forces the iterator to make four
        // backend calls (sizes 2, 2, 2, 1) and stitch them into a single forward
        // sequence without skipping seq 1, 4, or 7.
        let reader = populated(7);

        let seqs: Vec<u64> = reader
            .scan_range_chunked(1, 7, ScanDirection::Forward, 2)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[rstest]
    fn scan_range_chunked_reverse_walks_full_range() {
        // Mirror of the forward case: chunk size 2 over seven entries must yield
        // descending [7, 6, 5, 4, 3, 2, 1] without dropping the chunk-boundary seqs.
        let reader = populated(7);

        let seqs: Vec<u64> = reader
            .scan_range_chunked(1, 7, ScanDirection::Reverse, 2)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![7, 6, 5, 4, 3, 2, 1]);
    }

    #[rstest]
    fn scan_range_clips_to_high_watermark() {
        // Requesting beyond the watermark must terminate without a Gap error: the
        // backend reports the empty tail by returning an empty Vec, and the iterator
        // honors that as end-of-stream.
        let reader = populated(3);

        let seqs: Vec<u64> = reader
            .scan_range_chunked(1, 99, ScanDirection::Forward, 2)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[rstest]
    fn scan_range_reverse_clips_to_high_watermark() {
        // Reverse scans whose `to` sits more than one chunk above the high-watermark
        // must still walk the rows below the watermark. Without the up-front cursor
        // clamp, the backend would clip the first chunk to an empty Vec (entirely
        // above the watermark) and the iterator would terminate before yielding any
        // entry: regression coverage for the open `to` forensics call site.
        let reader = populated(3);

        let seqs: Vec<u64> = reader
            .scan_range_chunked(1, 99, ScanDirection::Reverse, 2)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![3, 2, 1]);
    }

    #[rstest]
    fn scan_range_reverse_with_to_at_u64_max() {
        // Belt-and-braces: even an open upper bound at `u64::MAX` must yield the
        // durable rows. Demonstrates the clamp guards against pathological inputs
        // (a defensive caller, a debug REPL, or a max-sentinel) without spinning.
        let reader = populated(3);

        let seqs: Vec<u64> = reader
            .scan_range_chunked(1, u64::MAX, ScanDirection::Reverse, 1)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![3, 2, 1]);
    }

    #[rstest]
    fn scan_range_reverse_above_watermark_yields_nothing() {
        // The reverse range sits entirely above the high-watermark; the iterator
        // must terminate cleanly with zero entries instead of stepping forever.
        let reader = populated(3);

        let seqs: Vec<u64> = reader
            .scan_range(10, 20, ScanDirection::Reverse)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert!(seqs.is_empty(), "seqs was: {seqs:?}");
    }

    #[rstest]
    #[case::inverted(5, 1, ScanDirection::Forward)]
    #[case::zero_from(0, 5, ScanDirection::Forward)]
    #[case::inverted_reverse(5, 1, ScanDirection::Reverse)]
    fn scan_range_empty_bounds_yield_no_entries(
        #[case] from: u64,
        #[case] to: u64,
        #[case] direction: ScanDirection,
    ) {
        let reader = populated(3);

        let seqs: Vec<u64> = reader
            .scan_range(from, to, direction)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert!(seqs.is_empty(), "seqs was: {seqs:?}");
    }

    #[rstest]
    fn scan_range_propagates_hash_mismatch_error() {
        // The MemoryBackend's tampered-payload path returns HashMismatch on scan; the
        // iterator must surface that error and then stop yielding rather than mask the
        // failure as end-of-stream.
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-tamper")).expect("open run");
        let mut tampered = build_entry(1, 100);
        tampered.payload = Bytes::from_static(b"\xFF\xFF");
        backend
            .append_batch(&[AppendEntry::without_indices(tampered)])
            .expect("append");
        let reader = EventStoreReader::new(backend);

        let mut iter = reader.scan_range(1, 1, ScanDirection::Forward);
        let first = iter.next().expect("first item");

        match first {
            Err(EventStoreError::HashMismatch { seq: 1 }) => {}
            other => panic!("expected HashMismatch, was {other:?}"),
        }
        assert!(
            iter.next().is_none(),
            "iterator must terminate after surfacing the error",
        );
    }

    #[rstest]
    fn scan_range_chunk_size_zero_normalizes_to_one() {
        // A zero chunk size must not deadlock: it normalizes to 1 so progress is
        // guaranteed even under a pathological caller.
        let reader = populated(3);

        let seqs: Vec<u64> = reader
            .scan_range_chunked(1, 3, ScanDirection::Forward, 0)
            .map(|r| r.expect("entry").seq)
            .collect();

        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[rstest]
    fn into_inner_returns_backend(reader_with_three: EventStoreReader<MemoryBackend>) {
        let backend = reader_with_three.into_inner();

        assert_eq!(backend.high_watermark().expect("hwm"), 3);
    }

    #[rstest]
    fn lookup_uses_distinct_kinds() {
        // Same string under two IndexKinds must return None for the kind that didn't
        // record it; the reader's lookup path must not collapse kinds.
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-kinds")).expect("open run");
        let shared_key = "shared-key".to_string();
        backend
            .append_batch(&[AppendEntry::new(
                build_entry(1, 100),
                vec![IndexKey::new(IndexKind::ClientOrderId, shared_key.clone())],
            )])
            .expect("append");
        let reader = EventStoreReader::new(backend);

        assert_eq!(
            reader
                .lookup(IndexKind::ClientOrderId, &shared_key)
                .expect("lookup"),
            Some(1),
        );
        assert!(
            reader
                .lookup(IndexKind::VenueOrderId, &shared_key)
                .expect("lookup")
                .is_none(),
        );
    }
}
