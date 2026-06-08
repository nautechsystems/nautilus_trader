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

//! Non-fatal integrity verifier for data marker sidecars.

use std::cmp::Ordering;

use ahash::AHashMap;
use nautilus_core::UnixNanos;

use crate::{
    error::EventStoreError,
    manifest::{RunId, RunStatus},
    markers::{
        DataCursorSnapshot, HiFiMarker, MarkerBackend, MarkerGap, MarkerManifest, StreamCursor,
        StreamDictEntry, StreamSlot, compute_dict_hash, compute_gap_hash, compute_hifi_hash,
        compute_marker_hash,
    },
};

/// Verifier for a single marker sidecar backend.
#[derive(Debug, Default)]
pub struct MarkerVerifier;

impl MarkerVerifier {
    /// Scans the marker sidecar and returns all non-fatal findings.
    ///
    /// `entry_high_watermark` comes from the sibling entry run and bounds marker
    /// `event_seq_before` values.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when the backend cannot scan a marker table or manifest.
    pub fn scan(
        backend: &dyn MarkerBackend,
        entry_high_watermark: u64,
    ) -> Result<MarkerVerifyReport, EventStoreError> {
        let manifest = backend.manifest()?;
        let mut findings = Vec::new();
        let snapshots = read_snapshots(backend, &mut findings)?;
        let hifi = read_hifi(backend, &mut findings)?;
        let gaps = read_gaps(backend, &mut findings)?;
        let dict = read_dict(backend, &mut findings)?;

        check_manifest_counts(&manifest, &snapshots, &hifi, &gaps, &dict, &mut findings);
        check_marker_sequence(&snapshots, &hifi, &gaps, &mut findings);
        check_event_seq(&snapshots, &hifi, entry_high_watermark, &mut findings);
        check_cursor_monotonicity(&snapshots, &mut findings);

        Ok(MarkerVerifyReport {
            run_id: manifest.run_id,
            status: manifest.status,
            snapshots_scanned: snapshots.len() as u64,
            hifi_scanned: hifi.len() as u64,
            gaps_scanned: gaps.len() as u64,
            dict_entries_scanned: dict.len() as u64,
            findings,
        })
    }
}

/// Structured report produced by [`MarkerVerifier`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerVerifyReport {
    /// The id of the run this marker sidecar belongs to.
    pub run_id: RunId,
    /// The marker sidecar lifecycle status at verification time.
    pub status: RunStatus,
    /// Number of cursor snapshots scanned.
    pub snapshots_scanned: u64,
    /// Number of high-fidelity markers scanned.
    pub hifi_scanned: u64,
    /// Number of marker gaps scanned.
    pub gaps_scanned: u64,
    /// Number of stream dictionary entries scanned.
    pub dict_entries_scanned: u64,
    /// Every marker-sidecar integrity finding.
    pub findings: Vec<MarkerFinding>,
}

impl MarkerVerifyReport {
    /// Returns `true` when no marker findings were accumulated.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Durable marker table kind used in hash findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerRecordKind {
    /// Cursor snapshot table.
    Snapshot,
    /// High-fidelity marker table.
    HiFi,
    /// Marker gap table.
    Gap,
    /// Stream dictionary table.
    Dict,
}

/// Marker manifest count field used in count-mismatch findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarkerCountKind {
    /// `snapshot_count`.
    Snapshot,
    /// `hifi_count`.
    HiFi,
    /// `gap_count`.
    Gap,
    /// `dict_count`.
    Dict,
}

/// One non-fatal integrity finding from a marker verification scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerFinding {
    /// A marker manifest count disagrees with the scanned table row count.
    ManifestCountMismatch {
        /// Which manifest count diverged.
        kind: MarkerCountKind,
        /// The count recorded in the marker manifest.
        manifest_count: u64,
        /// The count observed by scanning the marker table.
        scanned_count: u64,
    },
    /// One or more marker sequence values were neither present nor covered by a gap.
    MarkerSeqGap {
        /// First missing marker sequence.
        from_marker_seq: u64,
        /// Last missing marker sequence.
        to_marker_seq: u64,
    },
    /// A marker sequence value was covered more than once.
    MarkerSeqOverlap {
        /// First overlapped marker sequence.
        from_marker_seq: u64,
        /// Last overlapped marker sequence.
        to_marker_seq: u64,
    },
    /// A stored gap has an invalid inclusive range.
    InvalidMarkerGap {
        /// The gap's first marker sequence.
        from_marker_seq: u64,
        /// The gap's last marker sequence.
        to_marker_seq: u64,
    },
    /// `event_seq_before` decreased as `marker_seq` advanced.
    EventSeqRegressed {
        /// The marker sequence where the regression was observed.
        marker_seq: u64,
        /// The previous event sequence boundary.
        previous_event_seq_before: u64,
        /// The current event sequence boundary.
        event_seq_before: u64,
    },
    /// `event_seq_before` exceeded the sibling entry run high-watermark.
    EventSeqExceedsHighWatermark {
        /// The marker sequence carrying the invalid event boundary.
        marker_seq: u64,
        /// The invalid event sequence boundary.
        event_seq_before: u64,
        /// The sibling entry run high-watermark.
        high_watermark: u64,
    },
    /// A per-slot cursor count decreased between snapshots.
    CursorCountRegressed {
        /// The marker sequence carrying the regressed cursor.
        marker_seq: u64,
        /// The stream slot whose cursor regressed.
        slot: StreamSlot,
        /// The previous count for this slot.
        previous_count: u64,
        /// The current count for this slot.
        count: u64,
    },
    /// A per-slot highest `ts_init` decreased between snapshots.
    CursorTsInitRegressed {
        /// The marker sequence carrying the regressed cursor.
        marker_seq: u64,
        /// The stream slot whose cursor regressed.
        slot: StreamSlot,
        /// The previous highest `ts_init` for this slot.
        previous_ts_init_hi: UnixNanos,
        /// The current highest `ts_init` for this slot.
        ts_init_hi: UnixNanos,
    },
    /// A stored durable record hash did not match the recomputed canonical hash.
    HashMismatch {
        /// The table kind whose stored hash diverged.
        record: MarkerRecordKind,
        /// The marker sequence when the record kind carries one.
        marker_seq: Option<u64>,
        /// The stream slot when the record kind carries one.
        slot: Option<StreamSlot>,
    },
}

#[derive(Debug, Clone)]
struct ScannedRecord<T> {
    record: T,
}

#[derive(Debug, Clone, Copy)]
struct SequencedMarker {
    marker_seq: u64,
    event_seq_before: u64,
}

fn read_snapshots(
    backend: &dyn MarkerBackend,
    findings: &mut Vec<MarkerFinding>,
) -> Result<Vec<ScannedRecord<DataCursorSnapshot>>, EventStoreError> {
    if let Some(stored) = backend.scan_snapshot_records()? {
        let out = stored
            .into_iter()
            .map(|stored| {
                check_hash(
                    stored.hash,
                    compute_marker_hash(&stored.record),
                    MarkerRecordKind::Snapshot,
                    Some(stored.record.marker_seq),
                    None,
                    findings,
                );
                ScannedRecord {
                    record: stored.record,
                }
            })
            .collect();
        return Ok(out);
    }

    Ok(backend
        .scan_snapshots()?
        .into_iter()
        .map(|record| ScannedRecord { record })
        .collect())
}

fn read_hifi(
    backend: &dyn MarkerBackend,
    findings: &mut Vec<MarkerFinding>,
) -> Result<Vec<ScannedRecord<HiFiMarker>>, EventStoreError> {
    if let Some(stored) = backend.scan_hifi_records()? {
        let out = stored
            .into_iter()
            .map(|stored| {
                check_hash(
                    stored.hash,
                    compute_hifi_hash(&stored.record),
                    MarkerRecordKind::HiFi,
                    Some(stored.record.marker_seq),
                    Some(stored.record.slot),
                    findings,
                );
                ScannedRecord {
                    record: stored.record,
                }
            })
            .collect();
        return Ok(out);
    }

    Ok(backend
        .scan_hifi()?
        .into_iter()
        .map(|record| ScannedRecord { record })
        .collect())
}

fn read_gaps(
    backend: &dyn MarkerBackend,
    findings: &mut Vec<MarkerFinding>,
) -> Result<Vec<ScannedRecord<MarkerGap>>, EventStoreError> {
    if let Some(stored) = backend.scan_gap_records()? {
        let out = stored
            .into_iter()
            .map(|stored| {
                check_hash(
                    stored.hash,
                    compute_gap_hash(&stored.record),
                    MarkerRecordKind::Gap,
                    None,
                    None,
                    findings,
                );
                ScannedRecord {
                    record: stored.record,
                }
            })
            .collect();
        return Ok(out);
    }

    Ok(backend
        .scan_gaps()?
        .into_iter()
        .map(|record| ScannedRecord { record })
        .collect())
}

fn read_dict(
    backend: &dyn MarkerBackend,
    findings: &mut Vec<MarkerFinding>,
) -> Result<Vec<ScannedRecord<StreamDictEntry>>, EventStoreError> {
    if let Some(stored) = backend.scan_dict_records()? {
        let out = stored
            .into_iter()
            .map(|stored| {
                check_hash(
                    stored.hash,
                    compute_dict_hash(&stored.record),
                    MarkerRecordKind::Dict,
                    None,
                    Some(stored.record.slot),
                    findings,
                );
                ScannedRecord {
                    record: stored.record,
                }
            })
            .collect();
        return Ok(out);
    }

    Ok(backend
        .scan_dict()?
        .into_iter()
        .map(|record| ScannedRecord { record })
        .collect())
}

fn check_hash(
    stored_hash: [u8; 32],
    computed_hash: [u8; 32],
    record: MarkerRecordKind,
    marker_seq: Option<u64>,
    slot: Option<StreamSlot>,
    findings: &mut Vec<MarkerFinding>,
) {
    if stored_hash != computed_hash {
        findings.push(MarkerFinding::HashMismatch {
            record,
            marker_seq,
            slot,
        });
    }
}

fn check_manifest_counts(
    manifest: &MarkerManifest,
    snapshots: &[ScannedRecord<DataCursorSnapshot>],
    hifi: &[ScannedRecord<HiFiMarker>],
    gaps: &[ScannedRecord<MarkerGap>],
    dict: &[ScannedRecord<StreamDictEntry>],
    findings: &mut Vec<MarkerFinding>,
) {
    check_manifest_count(
        MarkerCountKind::Snapshot,
        manifest.snapshot_count,
        snapshots.len() as u64,
        findings,
    );
    check_manifest_count(
        MarkerCountKind::HiFi,
        manifest.hifi_count,
        hifi.len() as u64,
        findings,
    );
    check_manifest_count(
        MarkerCountKind::Gap,
        manifest.gap_count,
        gaps.len() as u64,
        findings,
    );
    check_manifest_count(
        MarkerCountKind::Dict,
        manifest.dict_count,
        dict.len() as u64,
        findings,
    );
}

fn check_manifest_count(
    kind: MarkerCountKind,
    manifest_count: u64,
    scanned_count: u64,
    findings: &mut Vec<MarkerFinding>,
) {
    if manifest_count != scanned_count {
        findings.push(MarkerFinding::ManifestCountMismatch {
            kind,
            manifest_count,
            scanned_count,
        });
    }
}

fn check_marker_sequence(
    snapshots: &[ScannedRecord<DataCursorSnapshot>],
    hifi: &[ScannedRecord<HiFiMarker>],
    gaps: &[ScannedRecord<MarkerGap>],
    findings: &mut Vec<MarkerFinding>,
) {
    let mut coverages = Vec::new();

    for snapshot in snapshots {
        coverages.push((snapshot.record.marker_seq, snapshot.record.marker_seq));
    }

    for marker in hifi {
        coverages.push((marker.record.marker_seq, marker.record.marker_seq));
    }

    for gap in gaps {
        if gap.record.from_marker_seq > gap.record.to_marker_seq {
            findings.push(MarkerFinding::InvalidMarkerGap {
                from_marker_seq: gap.record.from_marker_seq,
                to_marker_seq: gap.record.to_marker_seq,
            });
            continue;
        }
        coverages.push((gap.record.from_marker_seq, gap.record.to_marker_seq));
    }

    coverages.sort_unstable_by_key(|(from, to)| (*from, *to));
    let mut expected = 1_u64;

    for (from, to) in coverages {
        match from.cmp(&expected) {
            Ordering::Greater => {
                findings.push(MarkerFinding::MarkerSeqGap {
                    from_marker_seq: expected,
                    to_marker_seq: from - 1,
                });
                expected = to.saturating_add(1);
            }
            Ordering::Less => {
                findings.push(MarkerFinding::MarkerSeqOverlap {
                    from_marker_seq: from,
                    to_marker_seq: to.min(expected - 1),
                });

                if to >= expected {
                    expected = to.saturating_add(1);
                }
            }
            Ordering::Equal => {
                expected = to.saturating_add(1);
            }
        }
    }
}

fn check_event_seq(
    snapshots: &[ScannedRecord<DataCursorSnapshot>],
    hifi: &[ScannedRecord<HiFiMarker>],
    entry_high_watermark: u64,
    findings: &mut Vec<MarkerFinding>,
) {
    let mut markers = Vec::with_capacity(snapshots.len() + hifi.len());

    markers.extend(snapshots.iter().map(|snapshot| SequencedMarker {
        marker_seq: snapshot.record.marker_seq,
        event_seq_before: snapshot.record.event_seq_before,
    }));
    markers.extend(hifi.iter().map(|marker| SequencedMarker {
        marker_seq: marker.record.marker_seq,
        event_seq_before: marker.record.event_seq_before,
    }));
    markers.sort_unstable_by_key(|marker| marker.marker_seq);

    let mut previous_event_seq_before = None;

    for marker in markers {
        if marker.event_seq_before > entry_high_watermark {
            findings.push(MarkerFinding::EventSeqExceedsHighWatermark {
                marker_seq: marker.marker_seq,
                event_seq_before: marker.event_seq_before,
                high_watermark: entry_high_watermark,
            });
        }

        if let Some(previous) = previous_event_seq_before
            && marker.event_seq_before < previous
        {
            findings.push(MarkerFinding::EventSeqRegressed {
                marker_seq: marker.marker_seq,
                previous_event_seq_before: previous,
                event_seq_before: marker.event_seq_before,
            });
        }

        previous_event_seq_before = Some(marker.event_seq_before);
    }
}

fn check_cursor_monotonicity(
    snapshots: &[ScannedRecord<DataCursorSnapshot>],
    findings: &mut Vec<MarkerFinding>,
) {
    let mut ordered: Vec<&ScannedRecord<DataCursorSnapshot>> = snapshots.iter().collect();
    ordered.sort_unstable_by_key(|snapshot| snapshot.record.marker_seq);
    let mut cursors: AHashMap<StreamSlot, StreamCursor> = AHashMap::new();

    for snapshot in ordered {
        for cursor in &snapshot.record.advanced {
            if let Some(previous) = cursors.get(&cursor.slot) {
                if cursor.count < previous.count {
                    findings.push(MarkerFinding::CursorCountRegressed {
                        marker_seq: snapshot.record.marker_seq,
                        slot: cursor.slot,
                        previous_count: previous.count,
                        count: cursor.count,
                    });
                }

                if cursor.ts_init_hi < previous.ts_init_hi {
                    findings.push(MarkerFinding::CursorTsInitRegressed {
                        marker_seq: snapshot.record.marker_seq,
                        slot: cursor.slot,
                        previous_ts_init_hi: previous.ts_init_hi,
                        ts_init_hi: cursor.ts_init_hi,
                    });
                }
            }

            cursors.insert(cursor.slot, cursor.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{
        manifest::RunStatus,
        markers::{
            DataClass, DataCursorSnapshot, MarkerBackend, MarkerGap, MarkerGapReason,
            MarkerManifest, MemoryMarkerBackend, StreamCursor, StreamDictEntry, compute_gap_hash,
            compute_hifi_hash, compute_marker_hash,
        },
    };

    fn manifest() -> MarkerManifest {
        MarkerManifest {
            run_id: "1700000000-marker-verifier".to_string(),
            enabled_classes: vec![DataClass::Quote],
            high_fidelity: false,
            snapshot_count: 0,
            hifi_count: 0,
            gap_count: 0,
            dict_count: 0,
            status: RunStatus::Running,
        }
    }

    fn snapshot(
        marker_seq: u64,
        event_seq_before: u64,
        ts_init_hi: u64,
        count: u64,
    ) -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq,
            event_seq_before,
            ts_init: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
            advanced: vec![StreamCursor {
                slot: 0,
                ts_init_hi: UnixNanos::from(ts_init_hi),
                count,
            }],
        }
    }

    fn hifi(marker_seq: u64, event_seq_before: u64, slot: StreamSlot) -> HiFiMarker {
        HiFiMarker {
            marker_seq,
            event_seq_before,
            slot,
            ts_event: UnixNanos::from(1_700_000_000_000_000_100 + marker_seq),
            ts_init: UnixNanos::from(1_700_000_000_000_000_200 + marker_seq),
            same_ts_ordinal: 0,
            record_fingerprint: [7; 32],
        }
    }

    fn dict(slot: StreamSlot) -> StreamDictEntry {
        StreamDictEntry {
            slot,
            data_cls: DataClass::Quote,
            identifier: "ETHUSDT.BINANCE".to_string(),
        }
    }

    fn open_backend() -> MemoryMarkerBackend {
        let mut backend = MemoryMarkerBackend::new();
        backend.open_run(manifest()).expect("open run");
        backend
    }

    #[derive(Debug)]
    struct ManifestOverrideMarkerBackend {
        inner: MemoryMarkerBackend,
        manifest_override: MarkerManifest,
    }

    impl ManifestOverrideMarkerBackend {
        fn new(inner: MemoryMarkerBackend, manifest_override: MarkerManifest) -> Self {
            Self {
                inner,
                manifest_override,
            }
        }
    }

    impl MarkerBackend for ManifestOverrideMarkerBackend {
        fn open_run(&mut self, manifest: MarkerManifest) -> Result<(), EventStoreError> {
            self.inner.open_run(manifest)
        }

        fn append_snapshot(
            &mut self,
            snapshot: &DataCursorSnapshot,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.inner.append_snapshot(snapshot, hash)
        }

        fn append_hifi(
            &mut self,
            marker: &HiFiMarker,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.inner.append_hifi(marker, hash)
        }

        fn append_gap(&mut self, gap: &MarkerGap, hash: [u8; 32]) -> Result<(), EventStoreError> {
            self.inner.append_gap(gap, hash)
        }

        fn put_dict(
            &mut self,
            entry: &StreamDictEntry,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.inner.put_dict(entry, hash)
        }

        fn scan_snapshots(&self) -> Result<Vec<DataCursorSnapshot>, EventStoreError> {
            self.inner.scan_snapshots()
        }

        fn scan_snapshot_records(
            &self,
        ) -> Result<
            Option<Vec<crate::markers::StoredMarkerRecord<DataCursorSnapshot>>>,
            EventStoreError,
        > {
            self.inner.scan_snapshot_records()
        }

        fn scan_hifi(&self) -> Result<Vec<HiFiMarker>, EventStoreError> {
            self.inner.scan_hifi()
        }

        fn scan_hifi_records(
            &self,
        ) -> Result<Option<Vec<crate::markers::StoredMarkerRecord<HiFiMarker>>>, EventStoreError>
        {
            self.inner.scan_hifi_records()
        }

        fn scan_gaps(&self) -> Result<Vec<MarkerGap>, EventStoreError> {
            self.inner.scan_gaps()
        }

        fn scan_gap_records(
            &self,
        ) -> Result<Option<Vec<crate::markers::StoredMarkerRecord<MarkerGap>>>, EventStoreError>
        {
            self.inner.scan_gap_records()
        }

        fn scan_dict(&self) -> Result<Vec<StreamDictEntry>, EventStoreError> {
            self.inner.scan_dict()
        }

        fn scan_dict_records(
            &self,
        ) -> Result<Option<Vec<crate::markers::StoredMarkerRecord<StreamDictEntry>>>, EventStoreError>
        {
            self.inner.scan_dict_records()
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.inner.seal(status)
        }

        fn manifest(&self) -> Result<MarkerManifest, EventStoreError> {
            Ok(self.manifest_override.clone())
        }
    }

    #[rstest]
    fn contiguous_marker_seq_passes() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        let s2 = snapshot(2, 2, 200, 2);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_snapshot(&s2, compute_marker_hash(&s2))
            .expect("append s2");

        let report = MarkerVerifier::scan(&backend, 2).expect("scan");

        assert!(report.is_clean(), "findings was: {:?}", report.findings);
    }

    #[rstest]
    fn hifi_markers_participate_in_marker_seq_coverage() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        let m2 = hifi(2, 1, 0);
        let s3 = snapshot(3, 2, 200, 2);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_hifi(&m2, compute_hifi_hash(&m2))
            .expect("append hifi");
        backend
            .append_snapshot(&s3, compute_marker_hash(&s3))
            .expect("append s3");

        let report = MarkerVerifier::scan(&backend, 2).expect("scan");

        assert!(report.is_clean(), "findings was: {:?}", report.findings);
    }

    #[rstest]
    fn manifest_count_mismatch_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        let mut manifest_override = backend.manifest().expect("manifest");
        manifest_override.snapshot_count = 2;
        let backend = ManifestOverrideMarkerBackend::new(backend, manifest_override);

        let report = MarkerVerifier::scan(&backend, 1).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::ManifestCountMismatch {
                    kind: MarkerCountKind::Snapshot,
                    manifest_count: 2,
                    scanned_count: 1,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn missing_marker_seq_without_gap_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        let s3 = snapshot(3, 2, 200, 2);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_snapshot(&s3, compute_marker_hash(&s3))
            .expect("append s3");

        let report = MarkerVerifier::scan(&backend, 3).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::MarkerSeqGap {
                    from_marker_seq: 2,
                    to_marker_seq: 2,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn invalid_marker_gap_is_corrupt() {
        let mut backend = open_backend();
        let gap = MarkerGap {
            from_marker_seq: 4,
            to_marker_seq: 2,
            reason: MarkerGapReason::Overflow,
        };
        backend
            .append_gap(&gap, compute_gap_hash(&gap))
            .expect("append invalid gap");

        let report = MarkerVerifier::scan(&backend, 0).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::InvalidMarkerGap {
                    from_marker_seq: 4,
                    to_marker_seq: 2,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn overlapping_marker_coverage_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        let gap = MarkerGap {
            from_marker_seq: 1,
            to_marker_seq: 1,
            reason: MarkerGapReason::Overflow,
        };
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_gap(&gap, compute_gap_hash(&gap))
            .expect("append overlapping gap");

        let report = MarkerVerifier::scan(&backend, 1).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::MarkerSeqOverlap {
                    from_marker_seq: 1,
                    to_marker_seq: 1,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn non_monotonic_count_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 10);
        let s2 = snapshot(2, 2, 200, 9);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_snapshot(&s2, compute_marker_hash(&s2))
            .expect("append s2");

        let report = MarkerVerifier::scan(&backend, 2).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::CursorCountRegressed {
                    marker_seq: 2,
                    slot: 0,
                    previous_count: 10,
                    count: 9,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn non_monotonic_ts_init_hi_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 200, 1);
        let s2 = snapshot(2, 2, 199, 2);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_snapshot(&s2, compute_marker_hash(&s2))
            .expect("append s2");

        let report = MarkerVerifier::scan(&backend, 2).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::CursorTsInitRegressed {
                    marker_seq: 2,
                    slot: 0,
                    previous_ts_init_hi,
                    ts_init_hi,
                } if *previous_ts_init_hi == UnixNanos::from(200)
                    && *ts_init_hi == UnixNanos::from(199)
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn event_seq_before_regression_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 10, 100, 1);
        let s2 = snapshot(2, 9, 200, 2);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_snapshot(&s2, compute_marker_hash(&s2))
            .expect("append s2");

        let report = MarkerVerifier::scan(&backend, 10).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::EventSeqRegressed {
                    marker_seq: 2,
                    previous_event_seq_before: 10,
                    event_seq_before: 9,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn event_seq_before_exceeding_high_watermark_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 11, 100, 1);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");

        let report = MarkerVerifier::scan(&backend, 10).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::EventSeqExceedsHighWatermark {
                    marker_seq: 1,
                    event_seq_before: 11,
                    high_watermark: 10,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn non_snapshot_record_hash_mismatches_are_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        let m2 = hifi(2, 1, 0);
        let g3 = MarkerGap {
            from_marker_seq: 3,
            to_marker_seq: 3,
            reason: MarkerGapReason::Overflow,
        };
        let d0 = dict(0);
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_hifi(&m2, [0xBB; 32])
            .expect("append bad hifi hash");
        backend
            .append_gap(&g3, [0xCC; 32])
            .expect("append bad gap hash");
        backend
            .put_dict(&d0, [0xDD; 32])
            .expect("put bad dict hash");

        let report = MarkerVerifier::scan(&backend, 1).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::HashMismatch {
                    record: MarkerRecordKind::HiFi,
                    marker_seq: Some(2),
                    slot: Some(0),
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::HashMismatch {
                    record: MarkerRecordKind::Gap,
                    marker_seq: None,
                    slot: None,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::HashMismatch {
                    record: MarkerRecordKind::Dict,
                    marker_seq: None,
                    slot: Some(0),
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn bad_record_hash_is_corrupt() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        backend
            .append_snapshot(&s1, [0xAA; 32])
            .expect("append bad hash");

        let report = MarkerVerifier::scan(&backend, 1).expect("scan");

        assert!(
            report.findings.iter().any(|finding| matches!(
                finding,
                MarkerFinding::HashMismatch {
                    record: MarkerRecordKind::Snapshot,
                    marker_seq: Some(1),
                    slot: None,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn marker_gap_covers_missing_marker_seq() {
        let mut backend = open_backend();
        let s1 = snapshot(1, 1, 100, 1);
        let s3 = snapshot(3, 2, 200, 2);
        let gap = MarkerGap {
            from_marker_seq: 2,
            to_marker_seq: 2,
            reason: MarkerGapReason::Overflow,
        };
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_gap(&gap, compute_gap_hash(&gap))
            .expect("append gap");
        backend
            .append_snapshot(&s3, compute_marker_hash(&s3))
            .expect("append s3");

        let report = MarkerVerifier::scan(&backend, 3).expect("scan");

        assert!(report.is_clean(), "findings was: {:?}", report.findings);
    }
}
