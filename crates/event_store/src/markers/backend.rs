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

//! Durable backend trait and in-memory backend for the data marker sidecar.

use std::fmt::Debug;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{
    error::EventStoreError,
    manifest::{RunId, RunStatus},
    markers::{DataClass, DataCursorSnapshot, HiFiMarker, MarkerGap, StreamDictEntry, StreamSlot},
};

/// Per-run manifest for a data marker sidecar file.
///
/// Mirrors [`RunManifest`](crate::manifest::RunManifest) for the marker sidecar: it links the
/// marker file to its run, records the enabled data classes and whether high-fidelity capture is
/// active, tracks per-table counts, and carries the sealed lifecycle status. It is a distinct
/// struct, not a `RunManifest`, because the marker file is sealed independently of the entry run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkerManifest {
    /// The id of the run this marker file belongs to.
    pub run_id: RunId,
    /// The data classes enabled for marker capture on this run.
    pub enabled_classes: Vec<DataClass>,
    /// Whether high-fidelity per-record markers are recorded for this run.
    pub high_fidelity: bool,
    /// The number of cursor snapshots recorded.
    pub snapshot_count: u64,
    /// The number of high-fidelity markers recorded.
    pub hifi_count: u64,
    /// The number of marker gaps recorded.
    pub gap_count: u64,
    /// The number of distinct stream dictionary entries recorded.
    pub dict_count: u64,
    /// The lifecycle state of this marker file.
    pub status: RunStatus,
}

impl MarkerManifest {
    /// Returns `true` once `status` is anything other than [`RunStatus::Running`].
    #[must_use]
    pub const fn is_sealed(&self) -> bool {
        !matches!(self.status, RunStatus::Running)
    }
}

/// A durable backend for the data marker sidecar.
///
/// Backends persist cursor snapshots, high-fidelity markers, gaps, and the slot dictionary for
/// one open run at a time, mirroring the [`EventStore`](crate::backend::EventStore) trait for the
/// marker file. Each durable record is appended with its precomputed integrity hash so a verifier
/// can recompute and detect tampering. The marker path never blocks trading, so overflow is
/// recorded as a [`MarkerGap`] by the writer rather than failing the caller here.
pub trait MarkerBackend: Debug + Send {
    /// Opens a run for marker capture with the supplied manifest.
    ///
    /// The status is normalized to [`RunStatus::Running`] and the per-table counts are zeroed, so
    /// a caller can pass a freshly built or reused manifest without pre-clearing it.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] for unclassified backend failures.
    fn open_run(&mut self, manifest: MarkerManifest) -> Result<(), EventStoreError>;

    /// Appends a cursor snapshot with its precomputed integrity hash.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Closed`] when the run is sealed, and
    /// [`EventStoreError::Backend`] when no run is open or for unclassified backend failures.
    fn append_snapshot(
        &mut self,
        snapshot: &DataCursorSnapshot,
        hash: [u8; 32],
    ) -> Result<(), EventStoreError>;

    /// Appends a high-fidelity marker with its precomputed integrity hash.
    ///
    /// # Errors
    ///
    /// See [`append_snapshot`](Self::append_snapshot).
    fn append_hifi(&mut self, marker: &HiFiMarker, hash: [u8; 32]) -> Result<(), EventStoreError>;

    /// Appends a gap covering a dropped range of marker sequences, with its precomputed integrity
    /// hash.
    ///
    /// # Errors
    ///
    /// See [`append_snapshot`](Self::append_snapshot).
    fn append_gap(&mut self, gap: &MarkerGap, hash: [u8; 32]) -> Result<(), EventStoreError>;

    /// Records a slot dictionary entry with its precomputed integrity hash, write-once by slot.
    ///
    /// A second call for an already-recorded slot is ignored, so the first observed
    /// `slot -> (data_cls, identifier)` mapping wins and cannot be remapped.
    ///
    /// # Errors
    ///
    /// See [`append_snapshot`](Self::append_snapshot).
    fn put_dict(&mut self, entry: &StreamDictEntry, hash: [u8; 32]) -> Result<(), EventStoreError>;

    /// Scans all cursor snapshots in ascending `marker_seq` order.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open or for unclassified backend
    /// failures.
    fn scan_snapshots(&self) -> Result<Vec<DataCursorSnapshot>, EventStoreError>;

    /// Scans all high-fidelity markers in ascending `marker_seq` order.
    ///
    /// # Errors
    ///
    /// See [`scan_snapshots`](Self::scan_snapshots).
    fn scan_hifi(&self) -> Result<Vec<HiFiMarker>, EventStoreError>;

    /// Scans all recorded gaps in ascending `from_marker_seq` order.
    ///
    /// # Errors
    ///
    /// See [`scan_snapshots`](Self::scan_snapshots).
    fn scan_gaps(&self) -> Result<Vec<MarkerGap>, EventStoreError>;

    /// Scans all slot dictionary entries in ascending slot order.
    ///
    /// # Errors
    ///
    /// See [`scan_snapshots`](Self::scan_snapshots).
    fn scan_dict(&self) -> Result<Vec<StreamDictEntry>, EventStoreError>;

    /// Seals the open run with the given terminal status.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when `status` is [`RunStatus::Running`] or no run is
    /// open, and [`EventStoreError::Closed`] when the run is already sealed.
    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError>;

    /// Returns the current marker manifest.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open.
    fn manifest(&self) -> Result<MarkerManifest, EventStoreError>;
}

/// In-memory implementation of [`MarkerBackend`].
///
/// Stores snapshots, high-fidelity markers, and gaps as `Vec` tables, and the slot dictionary as
/// an [`IndexMap`] keyed by slot for write-once semantics. Each record's integrity hash is
/// retained alongside it so the verifier can recompute and compare. The `scan_*` methods return
/// records in their documented sort order regardless of append order, matching the keyed
/// persistent backend. One instance owns at most one open run at a time; opening a new run
/// replaces the previous one (markers never block boot). Used by writer and reader unit tests and
/// by the `cfg(madsim)` simulation lane.
#[derive(Debug, Default)]
pub struct MemoryMarkerBackend {
    state: Option<RunState>,
}

#[derive(Debug)]
struct RunState {
    manifest: MarkerManifest,
    snapshots: Vec<(DataCursorSnapshot, [u8; 32])>,
    hifi: Vec<(HiFiMarker, [u8; 32])>,
    gaps: Vec<(MarkerGap, [u8; 32])>,
    dict: IndexMap<StreamSlot, (StreamDictEntry, [u8; 32])>,
}

impl MemoryMarkerBackend {
    /// Creates a new empty [`MemoryMarkerBackend`] with no run open.
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

    fn writable_state(&mut self) -> Result<&mut RunState, EventStoreError> {
        let state = self.state_mut()?;
        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }
        Ok(state)
    }
}

impl MarkerBackend for MemoryMarkerBackend {
    fn open_run(&mut self, mut manifest: MarkerManifest) -> Result<(), EventStoreError> {
        // Markers never block boot, so a previous unsealed run is replaced rather than surfacing
        // CrashedPredecessor the way the entry backend does.
        manifest.status = RunStatus::Running;
        manifest.snapshot_count = 0;
        manifest.hifi_count = 0;
        manifest.gap_count = 0;
        manifest.dict_count = 0;

        self.state = Some(RunState {
            manifest,
            snapshots: Vec::new(),
            hifi: Vec::new(),
            gaps: Vec::new(),
            dict: IndexMap::new(),
        });
        Ok(())
    }

    fn append_snapshot(
        &mut self,
        snapshot: &DataCursorSnapshot,
        hash: [u8; 32],
    ) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        state.snapshots.push((snapshot.clone(), hash));
        state.manifest.snapshot_count = state.snapshots.len() as u64;
        Ok(())
    }

    fn append_hifi(&mut self, marker: &HiFiMarker, hash: [u8; 32]) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        state.hifi.push((marker.clone(), hash));
        state.manifest.hifi_count = state.hifi.len() as u64;
        Ok(())
    }

    fn append_gap(&mut self, gap: &MarkerGap, hash: [u8; 32]) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        state.gaps.push((gap.clone(), hash));
        state.manifest.gap_count = state.gaps.len() as u64;
        Ok(())
    }

    fn put_dict(&mut self, entry: &StreamDictEntry, hash: [u8; 32]) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        // Write-once by slot: the first observed mapping wins, so a later re-put cannot remap a
        // slot to a different class or identifier. The count tracks distinct slots, so a duplicate
        // re-put leaves it unchanged.
        state
            .dict
            .entry(entry.slot)
            .or_insert_with(|| (entry.clone(), hash));
        state.manifest.dict_count = state.dict.len() as u64;
        Ok(())
    }

    fn scan_snapshots(&self) -> Result<Vec<DataCursorSnapshot>, EventStoreError> {
        let mut out: Vec<DataCursorSnapshot> = self
            .state()?
            .snapshots
            .iter()
            .map(|(snapshot, _)| snapshot.clone())
            .collect();
        out.sort_by_key(|snapshot| snapshot.marker_seq);
        Ok(out)
    }

    fn scan_hifi(&self) -> Result<Vec<HiFiMarker>, EventStoreError> {
        let mut out: Vec<HiFiMarker> = self
            .state()?
            .hifi
            .iter()
            .map(|(marker, _)| marker.clone())
            .collect();
        out.sort_by_key(|marker| marker.marker_seq);
        Ok(out)
    }

    fn scan_gaps(&self) -> Result<Vec<MarkerGap>, EventStoreError> {
        let mut out: Vec<MarkerGap> = self
            .state()?
            .gaps
            .iter()
            .map(|(gap, _)| gap.clone())
            .collect();
        out.sort_by_key(|gap| gap.from_marker_seq);
        Ok(out)
    }

    fn scan_dict(&self) -> Result<Vec<StreamDictEntry>, EventStoreError> {
        let mut out: Vec<StreamDictEntry> = self
            .state()?
            .dict
            .values()
            .map(|(entry, _)| entry.clone())
            .collect();
        out.sort_by_key(|entry| entry.slot);
        Ok(out)
    }

    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
        let state = self.state_mut()?;

        // Running is not a terminal state; accepting it would leave the manifest unsealed while
        // returning Ok, so subsequent appends would not see Closed.
        if matches!(status, RunStatus::Running) {
            return Err(EventStoreError::Backend(
                "seal status must be a terminal state, was Running".to_string(),
            ));
        }

        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }

        state.manifest.status = status;
        Ok(())
    }

    fn manifest(&self) -> Result<MarkerManifest, EventStoreError> {
        Ok(self.state()?.manifest.clone())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::{fixture, rstest};

    use super::*;
    use crate::markers::{
        MarkerGapReason, StreamCursor, compute_dict_hash, compute_gap_hash, compute_hifi_hash,
        compute_marker_hash,
    };

    fn manifest() -> MarkerManifest {
        MarkerManifest {
            run_id: "1700000000-aaaa1111".to_string(),
            enabled_classes: vec![DataClass::Quote, DataClass::Trade],
            high_fidelity: false,
            snapshot_count: 0,
            hifi_count: 0,
            gap_count: 0,
            dict_count: 0,
            status: RunStatus::Running,
        }
    }

    fn snapshot(marker_seq: u64, event_seq_before: u64) -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq,
            event_seq_before,
            ts_init: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
            advanced: vec![StreamCursor {
                slot: 0,
                ts_init_hi: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
                count: marker_seq,
            }],
        }
    }

    fn hifi(marker_seq: u64) -> HiFiMarker {
        HiFiMarker {
            marker_seq,
            event_seq_before: 10,
            slot: 0,
            ts_event: UnixNanos::from(1),
            ts_init: UnixNanos::from(2),
            same_ts_ordinal: 0,
            record_fingerprint: [0u8; 32],
        }
    }

    #[fixture]
    fn open_backend() -> MemoryMarkerBackend {
        let mut backend = MemoryMarkerBackend::new();
        backend.open_run(manifest()).expect("open run");
        backend
    }

    #[rstest]
    fn append_and_scan_snapshots_in_seq_order(mut open_backend: MemoryMarkerBackend) {
        let s1 = snapshot(1, 10);
        let s2 = snapshot(2, 20);
        open_backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append 1");
        open_backend
            .append_snapshot(&s2, compute_marker_hash(&s2))
            .expect("append 2");

        let scanned = open_backend.scan_snapshots().expect("scan");

        assert_eq!(scanned, vec![s1, s2]);
    }

    #[rstest]
    fn scans_return_marker_seq_order_regardless_of_append_order(
        mut open_backend: MemoryMarkerBackend,
    ) {
        let s2 = snapshot(2, 20);
        let s1 = snapshot(1, 10);
        let m2 = hifi(4);
        let m1 = hifi(3);
        // Appended out of order: the backend must still scan in ascending marker_seq.
        open_backend
            .append_snapshot(&s2, compute_marker_hash(&s2))
            .expect("snap 2");
        open_backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("snap 1");
        open_backend
            .append_hifi(&m2, compute_hifi_hash(&m2))
            .expect("hifi 4");
        open_backend
            .append_hifi(&m1, compute_hifi_hash(&m1))
            .expect("hifi 3");

        assert_eq!(open_backend.scan_snapshots().expect("snaps"), vec![s1, s2]);
        assert_eq!(open_backend.scan_hifi().expect("hifi"), vec![m1, m2]);
    }

    #[rstest]
    fn append_and_scan_hifi(mut open_backend: MemoryMarkerBackend) {
        let m1 = hifi(1);
        let m2 = hifi(2);
        open_backend
            .append_hifi(&m1, compute_hifi_hash(&m1))
            .expect("hifi 1");
        open_backend
            .append_hifi(&m2, compute_hifi_hash(&m2))
            .expect("hifi 2");

        let scanned = open_backend.scan_hifi().expect("scan hifi");

        assert_eq!(scanned, vec![m1, m2]);
    }

    #[rstest]
    fn dict_entries_are_write_once_by_slot(mut open_backend: MemoryMarkerBackend) {
        let first = StreamDictEntry {
            slot: 0,
            data_cls: DataClass::Quote,
            identifier: "ETHUSDT.BINANCE".to_string(),
        };
        // A second put for slot 0 must not remap the slot to a different class/identifier.
        let remap = StreamDictEntry {
            slot: 0,
            data_cls: DataClass::Trade,
            identifier: "BTCUSDT.BINANCE".to_string(),
        };
        let other = StreamDictEntry {
            slot: 1,
            data_cls: DataClass::Trade,
            identifier: "BTCUSDT.BINANCE".to_string(),
        };

        open_backend
            .put_dict(&first, compute_dict_hash(&first))
            .expect("put 0");
        open_backend
            .put_dict(&remap, compute_dict_hash(&remap))
            .expect("re-put 0");
        open_backend
            .put_dict(&other, compute_dict_hash(&other))
            .expect("put 1");

        let dict = open_backend.scan_dict().expect("scan dict");

        assert_eq!(dict, vec![first, other]);
        // The duplicate re-put of slot 0 is not double-counted.
        assert_eq!(open_backend.manifest().expect("manifest").dict_count, 2);
    }

    #[rstest]
    fn append_gap_and_scan_gaps(mut open_backend: MemoryMarkerBackend) {
        let g1 = MarkerGap {
            from_marker_seq: 5,
            to_marker_seq: 9,
            reason: MarkerGapReason::Overflow,
        };
        let g2 = MarkerGap {
            from_marker_seq: 20,
            to_marker_seq: 20,
            reason: MarkerGapReason::WriterClosed,
        };
        open_backend
            .append_gap(&g1, compute_gap_hash(&g1))
            .expect("gap 1");
        open_backend
            .append_gap(&g2, compute_gap_hash(&g2))
            .expect("gap 2");

        let gaps = open_backend.scan_gaps().expect("scan gaps");

        assert_eq!(gaps, vec![g1, g2]);
    }

    #[rstest]
    #[case::ended(RunStatus::Ended)]
    #[case::crashed_recovered(RunStatus::CrashedRecovered)]
    #[case::quarantined(RunStatus::Quarantined)]
    fn seal_sets_manifest_status(mut open_backend: MemoryMarkerBackend, #[case] status: RunStatus) {
        open_backend.seal(status).expect("seal");

        let m = open_backend.manifest().expect("manifest");
        assert_eq!(m.status, status);
        assert!(m.is_sealed());
    }

    #[rstest]
    fn open_run_normalizes_status_and_zeroes_counts() {
        let mut backend = MemoryMarkerBackend::new();
        let mut stale = manifest();
        stale.status = RunStatus::Ended;
        stale.snapshot_count = 99;
        stale.hifi_count = 99;
        stale.gap_count = 99;
        stale.dict_count = 99;

        backend.open_run(stale).expect("open");

        let opened = backend.manifest().expect("manifest");
        assert_eq!(opened.status, RunStatus::Running);
        assert_eq!(opened.snapshot_count, 0);
        assert_eq!(opened.hifi_count, 0);
        assert_eq!(opened.gap_count, 0);
        assert_eq!(opened.dict_count, 0);
        // Enabled classes and mode survive the open.
        assert_eq!(
            opened.enabled_classes,
            vec![DataClass::Quote, DataClass::Trade]
        );
        assert!(!opened.high_fidelity);
    }

    #[rstest]
    fn manifest_counts_track_appends(mut open_backend: MemoryMarkerBackend) {
        let snap = snapshot(1, 10);
        let marker = hifi(2);
        let gap = MarkerGap {
            from_marker_seq: 3,
            to_marker_seq: 4,
            reason: MarkerGapReason::Overflow,
        };
        let entry = StreamDictEntry {
            slot: 0,
            data_cls: DataClass::Quote,
            identifier: "ETHUSDT.BINANCE".to_string(),
        };

        open_backend
            .append_snapshot(&snap, compute_marker_hash(&snap))
            .expect("snap");
        open_backend
            .append_hifi(&marker, compute_hifi_hash(&marker))
            .expect("hifi");
        open_backend
            .append_gap(&gap, compute_gap_hash(&gap))
            .expect("gap");
        open_backend
            .put_dict(&entry, compute_dict_hash(&entry))
            .expect("dict");

        let m = open_backend.manifest().expect("manifest");
        assert_eq!(m.snapshot_count, 1);
        assert_eq!(m.hifi_count, 1);
        assert_eq!(m.gap_count, 1);
        assert_eq!(m.dict_count, 1);
    }

    #[rstest]
    fn append_after_seal_returns_closed(mut open_backend: MemoryMarkerBackend) {
        open_backend.seal(RunStatus::Ended).expect("seal");
        let snap = snapshot(1, 10);

        let err = open_backend
            .append_snapshot(&snap, compute_marker_hash(&snap))
            .expect_err("must reject");

        assert!(matches!(err, EventStoreError::Closed));
    }

    #[rstest]
    fn seal_rejects_running_status(mut open_backend: MemoryMarkerBackend) {
        let err = open_backend
            .seal(RunStatus::Running)
            .expect_err("must reject");

        match err {
            EventStoreError::Backend(msg) => assert!(msg.contains("Running"), "msg was: {msg}"),
            other => panic!("expected Backend, was {other:?}"),
        }
        assert!(!open_backend.manifest().expect("manifest").is_sealed());
    }

    #[rstest]
    fn re_seal_returns_closed(mut open_backend: MemoryMarkerBackend) {
        open_backend.seal(RunStatus::Ended).expect("seal");

        let err = open_backend
            .seal(RunStatus::Quarantined)
            .expect_err("re-seal");

        assert!(matches!(err, EventStoreError::Closed));
    }

    #[rstest]
    #[case::append_snapshot("append_snapshot")]
    #[case::append_hifi("append_hifi")]
    #[case::append_gap("append_gap")]
    #[case::put_dict("put_dict")]
    #[case::scan_snapshots("scan_snapshots")]
    #[case::scan_hifi("scan_hifi")]
    #[case::scan_gaps("scan_gaps")]
    #[case::scan_dict("scan_dict")]
    #[case::seal("seal")]
    #[case::manifest("manifest")]
    fn methods_error_when_no_run_open(#[case] op: &str) {
        let mut backend = MemoryMarkerBackend::new();
        let snap = snapshot(1, 10);
        let marker = hifi(1);
        let gap = MarkerGap {
            from_marker_seq: 1,
            to_marker_seq: 2,
            reason: MarkerGapReason::Overflow,
        };
        let entry = StreamDictEntry {
            slot: 0,
            data_cls: DataClass::Quote,
            identifier: "ETHUSDT.BINANCE".to_string(),
        };

        let err = match op {
            "append_snapshot" => backend
                .append_snapshot(&snap, compute_marker_hash(&snap))
                .unwrap_err(),
            "append_hifi" => backend
                .append_hifi(&marker, compute_hifi_hash(&marker))
                .unwrap_err(),
            "append_gap" => backend
                .append_gap(&gap, compute_gap_hash(&gap))
                .unwrap_err(),
            "put_dict" => backend
                .put_dict(&entry, compute_dict_hash(&entry))
                .unwrap_err(),
            "scan_snapshots" => backend.scan_snapshots().unwrap_err(),
            "scan_hifi" => backend.scan_hifi().unwrap_err(),
            "scan_gaps" => backend.scan_gaps().unwrap_err(),
            "scan_dict" => backend.scan_dict().unwrap_err(),
            "seal" => backend.seal(RunStatus::Ended).unwrap_err(),
            "manifest" => backend.manifest().unwrap_err(),
            _ => unreachable!(),
        };

        match err {
            EventStoreError::Backend(msg) => assert!(msg.contains("no run open"), "msg was: {msg}"),
            other => panic!("expected Backend, was {other:?}"),
        }
    }
}
