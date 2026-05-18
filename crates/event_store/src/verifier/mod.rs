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

//! Off-trader verifier that proves a run file's integrity before the trader opens it.
//!
//! See `README.md` "Storage backend" and "Determinism contract" sections for the SPEC
//! posture: redb 4.x does not framewise-checksum data pages, so a `zero-tail` corruption
//! opens cleanly and panics on first read. The verifier therefore exercises every entry
//! and every stored index pair, accumulating findings so a single run produces one
//! actionable report rather than failing fast on the first hit. The supervisor runs the
//! verifier in an isolated process so a bad file aborts the verifier, not trading.
//!
//! Scope:
//!
//! - Walk every `seq` over `[1, high_watermark]` and recompute [`crate::EntryHash`].
//! - Detect gaps in the seq sequence (the SPEC's gap-detection idempotency primitive).
//! - Rebuild the `intent_id` sidecar index from headers and cross-check against the
//!   stored projection. For `client_order_id` and `venue_order_id` the verifier
//!   validates that every stored target seq still resolves to a clean entry; full
//!   payload-derived rebuild is deferred until the wrapper-type encoders land.
//! - Validate manifest invariants: `high_watermark` matches the durable last seq, the
//!   recorded `start_ts_init` and `end_ts_init` bracket the entry stream, and a sealed
//!   manifest's status is a terminal state.
//!
//! The library API stays narrow: a single [`Verifier`] type that owns a backend, a
//! [`VerifyReport`] structured for downstream operator tooling, and a [`VerifyError`]
//! reserved for failures that prevent the verifier from producing any report at all.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    path::Path,
};

use crate::{
    backend::{EventStore, IndexKind, RedbBackend},
    entry::EventStoreEntry,
    error::EventStoreError,
    manifest::{RunId, RunManifest, RunStatus},
};

/// Verifier over a single open run.
///
/// Constructed either by passing an already-open backend ([`Verifier::new`]) or by
/// opening a sealed redb file directly ([`Verifier::open_redb`]). The verifier never
/// mutates the backend; it walks the entry table and the secondary indices, then emits
/// a typed [`VerifyReport`].
pub struct Verifier {
    backend: Box<dyn EventStore>,
}

impl Debug for Verifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Verifier)).finish_non_exhaustive()
    }
}

impl Verifier {
    /// Wraps an already-open backend for read-only verification.
    #[must_use]
    pub fn new(backend: Box<dyn EventStore>) -> Self {
        Self { backend }
    }

    /// Opens a sealed redb run file at `<base_dir>/<instance_id>/<run_id>.redb` and
    /// wraps it for verification.
    ///
    /// Mirrors [`crate::backend::RedbBackend::open_sealed`]: only sealed files are
    /// accepted, since opening a still-`Running` file would race with a live writer
    /// and break the off-trader-process posture.
    ///
    /// # Errors
    ///
    /// Returns [`VerifyError::Backend`] when the underlying backend rejects the open
    /// (file missing, run still `Running`, header corruption).
    pub fn open_redb(
        base_dir: impl AsRef<Path>,
        instance_id: &str,
        run_id: &str,
    ) -> Result<Self, VerifyError> {
        let backend =
            RedbBackend::open_sealed(base_dir.as_ref().to_path_buf(), instance_id, run_id)?;
        Ok(Self {
            backend: Box::new(backend),
        })
    }

    /// Returns a reference to the wrapped backend.
    #[must_use]
    pub fn backend(&self) -> &dyn EventStore {
        self.backend.as_ref()
    }

    /// Performs a full integrity scan of the open run and returns the typed report.
    ///
    /// `verify` reads the manifest, walks every `seq` in `[1, high_watermark]`,
    /// rebuilds the intent-id sidecar index, cross-checks the stored client- and
    /// venue-order-id indices, and validates manifest invariants. Hash mismatches,
    /// gaps, index drift, and manifest mismatches surface as [`VerifyFinding`]s on the
    /// returned report; only failures that prevent the verifier from producing a
    /// report at all surface as [`VerifyError`].
    ///
    /// # Errors
    ///
    /// Returns [`VerifyError::Backend`] when the backend refuses a read-side
    /// operation (no run open, disk pressure, manifest decode failure).
    pub fn verify(&self) -> Result<VerifyReport, VerifyError> {
        let manifest = self.backend.manifest()?;
        let high_watermark = self.backend.high_watermark()?;

        let mut findings = Vec::new();
        let scan = self.scan_entries(high_watermark, &mut findings)?;

        self.cross_check_indices(&scan, &mut findings)?;
        validate_manifest(&manifest, high_watermark, &scan, &mut findings);

        Ok(VerifyReport {
            run_id: manifest.run_id.clone(),
            status: manifest.status,
            high_watermark,
            entries_scanned: scan.scanned,
            findings,
        })
    }

    fn scan_entries(
        &self,
        high_watermark: u64,
        findings: &mut Vec<VerifyFinding>,
    ) -> Result<EntryScan, VerifyError> {
        let mut scanned: u64 = 0;
        let mut min_ts: Option<u64> = None;
        let mut max_ts: Option<u64> = None;
        let mut intent_index: BTreeMap<String, u64> = BTreeMap::new();
        let mut clean_seqs: BTreeSet<u64> = BTreeSet::new();
        let mut corrupted_seqs: BTreeSet<u64> = BTreeSet::new();
        let mut gap_cursor: Option<u64> = None;

        for seq in 1..=high_watermark {
            match self.backend.scan_seq(seq) {
                Ok(Some(entry)) => {
                    flush_pending_gap(seq, &mut gap_cursor, findings);

                    // The recomputed hash check inside scan_seq covers the entry
                    // contents, but the entry's embedded seq is one of those
                    // contents: a row whose value is moved or duplicated under a
                    // different table key still hashes correctly. Cross-check the
                    // table key against the embedded seq so the verifier catches
                    // that class of corruption rather than reporting a clean run.
                    if entry.seq != seq {
                        findings.push(VerifyFinding::SeqMismatch {
                            table_key: seq,
                            embedded_seq: entry.seq,
                        });
                        corrupted_seqs.insert(seq);
                        scanned += 1;
                        continue;
                    }
                    record_entry(&entry, &mut min_ts, &mut max_ts, &mut intent_index);
                    clean_seqs.insert(seq);
                    scanned += 1;
                }
                Ok(None) | Err(EventStoreError::Gap { .. }) => {
                    extend_pending_gap(seq, &mut gap_cursor);
                }
                Err(EventStoreError::HashMismatch { seq: bad }) => {
                    flush_pending_gap(seq, &mut gap_cursor, findings);
                    findings.push(VerifyFinding::HashMismatch { seq: bad });
                    corrupted_seqs.insert(seq);
                    scanned += 1;
                }
                Err(other) => return Err(VerifyError::Backend(other)),
            }
        }

        flush_pending_gap(high_watermark + 1, &mut gap_cursor, findings);

        Ok(EntryScan {
            scanned,
            min_ts,
            max_ts,
            intent_index,
            clean_seqs,
            corrupted_seqs,
        })
    }

    fn cross_check_indices(
        &self,
        scan: &EntryScan,
        findings: &mut Vec<VerifyFinding>,
    ) -> Result<(), VerifyError> {
        let stored_intent: BTreeMap<String, u64> = self
            .backend
            .iter_index_keys(IndexKind::IntentId)?
            .into_iter()
            .collect();
        diff_index(
            IndexKind::IntentId,
            &scan.intent_index,
            &stored_intent,
            findings,
        );

        for kind in [IndexKind::ClientOrderId, IndexKind::VenueOrderId] {
            for (key, stored_seq) in self.backend.iter_index_keys(kind)? {
                let drift = classify_target(stored_seq, scan);
                if let Some(drift) = drift {
                    findings.push(VerifyFinding::IndexDrift { kind, key, drift });
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
struct EntryScan {
    scanned: u64,
    min_ts: Option<u64>,
    max_ts: Option<u64>,
    intent_index: BTreeMap<String, u64>,
    clean_seqs: BTreeSet<u64>,
    corrupted_seqs: BTreeSet<u64>,
}

fn record_entry(
    entry: &EventStoreEntry,
    min_ts: &mut Option<u64>,
    max_ts: &mut Option<u64>,
    intent_index: &mut BTreeMap<String, u64>,
) {
    let ts = entry.ts_init.as_u64();
    *min_ts = Some(min_ts.map_or(ts, |cur| cur.min(ts)));
    *max_ts = Some(max_ts.map_or(ts, |cur| cur.max(ts)));

    if let Some(intent_id) = entry.headers.intent_id.as_ref() {
        intent_index
            .entry(intent_id.to_string())
            .or_insert(entry.seq);
    }
}

fn extend_pending_gap(seq: u64, gap_cursor: &mut Option<u64>) {
    if gap_cursor.is_none() {
        *gap_cursor = Some(seq);
    }
}

fn flush_pending_gap(
    next_seq: u64,
    gap_cursor: &mut Option<u64>,
    findings: &mut Vec<VerifyFinding>,
) {
    if let Some(start) = gap_cursor.take() {
        findings.push(VerifyFinding::Gap {
            range: GapRange {
                from: start,
                to: next_seq - 1,
            },
        });
    }
}

fn diff_index(
    kind: IndexKind,
    rebuilt: &BTreeMap<String, u64>,
    stored: &BTreeMap<String, u64>,
    findings: &mut Vec<VerifyFinding>,
) {
    for (key, rebuilt_seq) in rebuilt {
        match stored.get(key) {
            Some(stored_seq) if stored_seq == rebuilt_seq => {}
            Some(stored_seq) => findings.push(VerifyFinding::IndexDrift {
                kind,
                key: key.clone(),
                drift: IndexDrift::DivergentSeq {
                    stored_seq: *stored_seq,
                    rebuilt_seq: *rebuilt_seq,
                },
            }),
            None => findings.push(VerifyFinding::IndexDrift {
                kind,
                key: key.clone(),
                drift: IndexDrift::MissingFromStored {
                    rebuilt_seq: *rebuilt_seq,
                },
            }),
        }
    }

    for (key, stored_seq) in stored {
        if !rebuilt.contains_key(key) {
            findings.push(VerifyFinding::IndexDrift {
                kind,
                key: key.clone(),
                drift: IndexDrift::UnknownKey {
                    stored_seq: *stored_seq,
                },
            });
        }
    }
}

fn classify_target(stored_seq: u64, scan: &EntryScan) -> Option<IndexDrift> {
    if scan.clean_seqs.contains(&stored_seq) {
        None
    } else if scan.corrupted_seqs.contains(&stored_seq) {
        Some(IndexDrift::TargetCorrupted { stored_seq })
    } else {
        Some(IndexDrift::DanglingTarget { stored_seq })
    }
}

fn validate_manifest(
    manifest: &RunManifest,
    high_watermark: u64,
    scan: &EntryScan,
    findings: &mut Vec<VerifyFinding>,
) {
    if manifest.high_watermark != high_watermark {
        findings.push(VerifyFinding::ManifestMismatch {
            kind: ManifestField::HighWatermark,
            reason: format!(
                "manifest high_watermark {} disagrees with durable high_watermark {high_watermark}",
                manifest.high_watermark,
            ),
        });
    }

    if let Some(min_ts) = scan.min_ts
        && manifest.start_ts_init.as_u64() > min_ts
    {
        findings.push(VerifyFinding::ManifestMismatch {
            kind: ManifestField::StartTsInit,
            reason: format!(
                "manifest start_ts_init {} sits above earliest entry ts_init {min_ts}",
                manifest.start_ts_init.as_u64(),
            ),
        });
    }

    if manifest.is_sealed() {
        match (manifest.end_ts_init.map(|t| t.as_u64()), scan.max_ts) {
            (Some(stored), Some(observed)) if stored != observed => {
                findings.push(VerifyFinding::ManifestMismatch {
                    kind: ManifestField::EndTsInit,
                    reason: format!(
                        "manifest end_ts_init {stored} disagrees with last observed ts_init {observed}",
                    ),
                });
            }
            (None, Some(observed)) => findings.push(VerifyFinding::ManifestMismatch {
                kind: ManifestField::EndTsInit,
                reason: format!(
                    "sealed manifest is missing end_ts_init while entries up to ts_init {observed} exist",
                ),
            }),
            (Some(stored), None) => findings.push(VerifyFinding::ManifestMismatch {
                kind: ManifestField::EndTsInit,
                reason: format!(
                    "sealed manifest carries end_ts_init {stored} despite empty entry table",
                ),
            }),
            _ => {}
        }
    }
}

/// The structured report produced by [`Verifier::verify`].
///
/// Operators key on [`VerifyReport::is_clean`] for the binary verdict and walk
/// [`VerifyReport::findings`] for the actionable items. The verifier never
/// quarantines on its own: that is the supervisor's call given the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// The id of the verified run, copied from the manifest.
    pub run_id: RunId,
    /// The lifecycle status the run carried at verification time.
    pub status: RunStatus,
    /// The durable high-watermark the verifier walked up to.
    pub high_watermark: u64,
    /// The number of `seq` slots the verifier successfully read (clean or hash-mismatched).
    pub entries_scanned: u64,
    /// Every integrity finding the verifier accumulated.
    pub findings: Vec<VerifyFinding>,
}

impl VerifyReport {
    /// Returns `true` when the verifier accumulated no findings.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// One actionable integrity finding from a verifier run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyFinding {
    /// The recomputed canonical hash of `seq` did not match the stored value.
    HashMismatch {
        /// The sequence number whose hash diverged.
        seq: u64,
    },
    /// One or more contiguous `seq` slots inside the high-watermark are missing.
    Gap {
        /// The inclusive range of missing seqs.
        range: GapRange,
    },
    /// The entry stored at table key `table_key` carries an `entry.seq` that
    /// disagrees with the key.
    ///
    /// The canonical hash hashes `entry.seq` rather than the table key, so a row
    /// whose bytes were moved or duplicated under a different key still passes the
    /// hash check. The verifier surfaces the divergence so that class of
    /// corruption never reads as clean.
    SeqMismatch {
        /// The redb table key (the slot the verifier was reading).
        table_key: u64,
        /// The seq embedded inside the decoded entry value.
        embedded_seq: u64,
    },
    /// A stored sidecar index entry diverges from the projection rebuilt from the
    /// entry table.
    IndexDrift {
        /// Which sidecar index the finding applies to.
        kind: IndexKind,
        /// The stringified key inside that index.
        key: String,
        /// The kind of drift observed.
        drift: IndexDrift,
    },
    /// A manifest field disagrees with the entry table or violates a sealed-state
    /// invariant.
    ManifestMismatch {
        /// Which manifest field the finding applies to.
        kind: ManifestField,
        /// Operator-readable explanation of the mismatch.
        reason: String,
    },
}

/// An inclusive `[from, to]` range of missing seqs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GapRange {
    /// First missing seq.
    pub from: u64,
    /// Last missing seq.
    pub to: u64,
}

/// The kind of drift observed for a sidecar index key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndexDrift {
    /// The verifier rebuilt this key from the entry table but no stored index entry
    /// exists for it.
    MissingFromStored {
        /// The seq the verifier recomputed for this key.
        rebuilt_seq: u64,
    },
    /// The stored index entry points at a different seq than the rebuild produced.
    DivergentSeq {
        /// The seq currently recorded in the stored index.
        stored_seq: u64,
        /// The seq the verifier recomputed for this key.
        rebuilt_seq: u64,
    },
    /// A stored index key the verifier could not match against its rebuilt projection.
    ///
    /// For `intent_id` this means the entry table contains no header carrying the
    /// stored key. For `client_order_id` and `venue_order_id` the rebuild is not yet
    /// payload-aware (wrapper-type encoders are deferred), so this variant is not
    /// emitted for those kinds; their target reachability is reported as
    /// [`Self::DanglingTarget`] and [`Self::TargetCorrupted`] instead.
    UnknownKey {
        /// The seq the stored index recorded.
        stored_seq: u64,
    },
    /// The stored index points at a seq that does not exist inside the high-watermark.
    DanglingTarget {
        /// The seq the stored index recorded.
        stored_seq: u64,
    },
    /// The stored index points at a seq whose entry failed the hash check.
    TargetCorrupted {
        /// The seq the stored index recorded.
        stored_seq: u64,
    },
}

/// A manifest field flagged by [`VerifyFinding::ManifestMismatch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManifestField {
    /// `manifest.high_watermark` does not match the durable last seq.
    HighWatermark,
    /// `manifest.start_ts_init` sits above the earliest observed `ts_init`.
    StartTsInit,
    /// `manifest.end_ts_init` does not bracket the entry stream as expected for a
    /// sealed run.
    EndTsInit,
}

/// Errors that prevent the verifier from producing a report at all.
///
/// Findings on a successful report cover the operator's actionable surface; this
/// type captures the verifier's own failure modes (no run open, disk pressure on a
/// read, manifest header damage that prevents loading the manifest).
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// A backend operation refused service before the verifier could produce a report.
    #[error("backend access failed: {0}")]
    Backend(#[from] EventStoreError),
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
        backend::{AppendEntry, IndexKey, MemoryBackend, ScanDirection},
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

    fn append_with_headers(
        seq: u64,
        ts_init: u64,
        headers: Headers,
        index_keys: Vec<IndexKey>,
    ) -> AppendEntry {
        AppendEntry::new(build_entry(seq, headers, ts_init), index_keys)
    }

    /// Test-only wrapper that delegates every call to an inner backend except the
    /// manifest, which it returns verbatim, and (optionally) the high-watermark.
    /// Lets unit tests drive manifest-mismatch and trailing-gap findings the
    /// public `MemoryBackend` API would normalize away on seal.
    struct ManifestOverrideBackend {
        inner: MemoryBackend,
        manifest_override: RunManifest,
        high_watermark_override: Option<u64>,
    }

    impl ManifestOverrideBackend {
        fn new(inner: MemoryBackend, manifest_override: RunManifest) -> Self {
            Self {
                inner,
                manifest_override,
                high_watermark_override: None,
            }
        }

        fn with_high_watermark(mut self, hwm: u64) -> Self {
            self.high_watermark_override = Some(hwm);
            self
        }
    }

    impl EventStore for ManifestOverrideBackend {
        fn open_run(&mut self, m: RunManifest) -> Result<(), EventStoreError> {
            self.inner.open_run(m)
        }

        fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
            self.inner.append_batch(entries)
        }

        fn scan_range(
            &self,
            from: u64,
            to: u64,
            direction: ScanDirection,
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            self.inner.scan_range(from, to, direction)
        }

        fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            self.inner.scan_seq(seq)
        }

        fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
            self.inner.lookup(kind, key)
        }

        fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            self.inner.iter_index_keys(kind)
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.inner.seal(status)
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            Ok(self.manifest_override.clone())
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            if let Some(hwm) = self.high_watermark_override {
                return Ok(hwm);
            }
            self.inner.high_watermark()
        }
    }

    #[fixture]
    fn open_backend() -> MemoryBackend {
        let mut backend = MemoryBackend::new();
        backend
            .open_run(manifest("1700000000-aaaa1111"))
            .expect("open run");
        backend
    }

    fn verifier_for(backend: MemoryBackend) -> Verifier {
        Verifier::new(Box::new(backend))
    }

    #[rstest]
    fn clean_run_reports_no_findings(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
                append_with(3, 12, Vec::new()),
            ])
            .expect("append");
        open_backend.seal(RunStatus::Ended).expect("seal");

        let report = verifier_for(open_backend).verify().expect("verify");

        // Lock the canonical clean case to zero findings exactly: any spurious
        // additional finding must fail this test rather than slip past is_clean()
        // matchers in the more targeted suites below.
        assert!(report.is_clean(), "findings was: {:?}", report.findings);
        assert_eq!(report.findings.len(), 0);
        assert_eq!(report.high_watermark, 3);
        assert_eq!(report.entries_scanned, 3);
        assert_eq!(report.status, RunStatus::Ended);
    }

    #[rstest]
    fn empty_run_reports_no_findings(mut open_backend: MemoryBackend) {
        open_backend.seal(RunStatus::Ended).expect("seal");

        let report = verifier_for(open_backend).verify().expect("verify");

        assert!(report.is_clean(), "findings was: {:?}", report.findings);
        assert_eq!(report.entries_scanned, 0);
    }

    #[rstest]
    fn hash_mismatch_surfaces_per_seq(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
        let mut tampered = build_entry(2, Headers::empty(), 11);
        tampered.payload = Bytes::from_static(b"\xFF");
        open_backend
            .append_batch(&[AppendEntry::without_indices(tampered)])
            .expect("append");
        open_backend
            .append_batch(&[append_with(3, 12, Vec::new())])
            .expect("append");

        let report = verifier_for(open_backend).verify().expect("verify");

        assert!(
            report
                .findings
                .iter()
                .any(|f| matches!(f, VerifyFinding::HashMismatch { seq: 2 })),
            "findings was: {:?}",
            report.findings,
        );
        assert_eq!(report.entries_scanned, 3);
        assert_eq!(report.high_watermark, 3);
    }

    #[rstest]
    fn multiple_hash_mismatches_all_surface(mut open_backend: MemoryBackend) {
        // Confirms the verifier walks past hash mismatches instead of bailing on the
        // first hit: seq=2 and seq=4 are both tampered, and both must appear in the
        // report.
        for seq in 1..=4u64 {
            let mut entry = build_entry(seq, Headers::empty(), 10 + seq);
            if seq == 2 || seq == 4 {
                entry.payload = Bytes::from_static(b"\xFF");
            }
            open_backend
                .append_batch(&[AppendEntry::without_indices(entry)])
                .expect("append");
        }

        let report = verifier_for(open_backend).verify().expect("verify");

        let mismatch_seqs: Vec<u64> = report
            .findings
            .iter()
            .filter_map(|f| match f {
                VerifyFinding::HashMismatch { seq } => Some(*seq),
                _ => None,
            })
            .collect();
        assert_eq!(mismatch_seqs, vec![2, 4]);
    }

    #[rstest]
    fn intent_id_drift_divergent_seq() {
        // Stored intent index points at seq=1 (where the encoder emitted the key) but
        // only seq=2 carries the matching header. Rebuild yields {intent: 2}; stored
        // holds {intent: 1}; DivergentSeq surfaces.
        let intent = UUID4::new();
        let headers = Headers {
            intent_id: Some(intent),
            ..Headers::empty()
        };
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-drift")).expect("open run");
        backend
            .append_batch(&[
                append_with(
                    1,
                    10,
                    vec![IndexKey::new(IndexKind::IntentId, intent.to_string())],
                ),
                append_with_headers(2, 11, headers, Vec::new()),
            ])
            .expect("append");
        backend.seal(RunStatus::Ended).expect("seal");

        let report = verifier_for(backend).verify().expect("verify");

        let drift = report
            .findings
            .iter()
            .find(|f| matches!(f, VerifyFinding::IndexDrift { .. }))
            .unwrap_or_else(|| panic!("expected IndexDrift, was {:?}", report.findings));

        match drift {
            VerifyFinding::IndexDrift {
                kind: IndexKind::IntentId,
                key,
                drift:
                    IndexDrift::DivergentSeq {
                        stored_seq,
                        rebuilt_seq,
                    },
            } => {
                assert_eq!(*key, intent.to_string());
                assert_eq!(*stored_seq, 1);
                assert_eq!(*rebuilt_seq, 2);
            }
            other => panic!("unexpected drift, was {other:?}"),
        }
    }

    #[rstest]
    fn intent_id_unknown_stored_key() {
        // Stored intent index carries a key whose entry has no intent_id header. The
        // verifier rebuilds an empty intent map and surfaces the orphan as
        // UnknownKey drift.
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-orphan")).expect("open run");
        let key = "intent-orphan".to_string();
        backend
            .append_batch(&[append_with(
                1,
                10,
                vec![IndexKey::new(IndexKind::IntentId, key.clone())],
            )])
            .expect("append");

        let report = verifier_for(backend).verify().expect("verify");

        let drift = report
            .findings
            .iter()
            .find(|f| matches!(f, VerifyFinding::IndexDrift { .. }))
            .unwrap_or_else(|| panic!("expected IndexDrift, was {:?}", report.findings));

        match drift {
            VerifyFinding::IndexDrift {
                kind: IndexKind::IntentId,
                key: drift_key,
                drift: IndexDrift::UnknownKey { stored_seq: 1 },
            } => assert_eq!(*drift_key, key),
            other => panic!("unexpected drift, was {other:?}"),
        }
    }

    #[rstest]
    fn intent_id_missing_from_stored_drift() {
        // Entry carries an intent header, but no stored index entry exists for that
        // key. Rebuild yields {intent: 1}, stored is empty, so MissingFromStored
        // surfaces.
        let intent = UUID4::new();
        let headers = Headers {
            intent_id: Some(intent),
            ..Headers::empty()
        };
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-missing")).expect("open run");
        backend
            .append_batch(&[append_with_headers(1, 10, headers, Vec::new())])
            .expect("append");

        let report = verifier_for(backend).verify().expect("verify");

        assert!(
            report.findings.iter().any(|f| matches!(
                f,
                VerifyFinding::IndexDrift {
                    kind: IndexKind::IntentId,
                    drift: IndexDrift::MissingFromStored { rebuilt_seq: 1 },
                    ..
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn client_order_id_index_clean_when_target_resolves(mut open_backend: MemoryBackend) {
        open_backend
            .append_batch(&[AppendEntry::new(
                build_entry(1, Headers::empty(), 10),
                vec![IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string())],
            )])
            .expect("append");
        open_backend.seal(RunStatus::Ended).expect("seal");

        let report = verifier_for(open_backend).verify().expect("verify");

        assert!(report.is_clean(), "findings was: {:?}", report.findings);
    }

    #[rstest]
    #[case::client_order_id(IndexKind::ClientOrderId)]
    #[case::venue_order_id(IndexKind::VenueOrderId)]
    fn entity_index_target_corrupted_drift(
        mut open_backend: MemoryBackend,
        #[case] kind: IndexKind,
    ) {
        // Stored entity-index entry points at seq=1 whose stored hash no longer
        // matches the recomputed hash. The verifier must surface TargetCorrupted
        // for both ClientOrderId and VenueOrderId so a drop of either kind from
        // the cross-check loop fails this test.
        let mut tampered = build_entry(1, Headers::empty(), 10);
        tampered.payload = Bytes::from_static(b"\xFF");
        open_backend
            .append_batch(&[AppendEntry::new(
                tampered,
                vec![IndexKey::new(kind, "K-1".to_string())],
            )])
            .expect("append");

        let report = verifier_for(open_backend).verify().expect("verify");

        assert!(
            report.findings.iter().any(|f| matches!(
                f,
                VerifyFinding::IndexDrift {
                    kind: drift_kind,
                    drift: IndexDrift::TargetCorrupted { stored_seq: 1 },
                    ..
                } if *drift_kind == kind
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    fn find_manifest_mismatch(findings: &[VerifyFinding], target: ManifestField) -> &str {
        findings
            .iter()
            .find_map(|f| match f {
                VerifyFinding::ManifestMismatch { kind, reason } if *kind == target => {
                    Some(reason.as_str())
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("expected ManifestMismatch({target:?}), findings was: {findings:?}")
            })
    }

    #[rstest]
    fn manifest_high_watermark_drift() {
        // Real durable hwm is 1, but the manifest reports 99. Verifier must surface
        // a HighWatermark mismatch whose reason carries both values so a swap of
        // observed and stored sides would fail this test.
        let mut inner = MemoryBackend::new();
        inner.open_run(manifest("run-hwm")).expect("open run");
        inner
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
        inner.seal(RunStatus::Ended).expect("seal");

        let mut stale = inner.manifest().expect("manifest");
        stale.high_watermark = 99;
        let backend = ManifestOverrideBackend::new(inner, stale);

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");
        let reason = find_manifest_mismatch(&report.findings, ManifestField::HighWatermark);

        assert!(reason.contains("99"), "reason was: {reason}");
        assert!(reason.contains('1'), "reason was: {reason}");
    }

    #[rstest]
    fn manifest_end_ts_init_drift_when_sealed() {
        // Real durable max ts_init is 25, but the sealed manifest's end_ts_init
        // claims 99. The reason must surface both values; without that assertion,
        // a min/max swap inside record_entry would still pass.
        let mut inner = MemoryBackend::new();
        inner.open_run(manifest("run-end-ts")).expect("open run");
        inner
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 25, Vec::new()),
            ])
            .expect("append");
        inner.seal(RunStatus::Ended).expect("seal");

        let mut drifted = inner.manifest().expect("manifest");
        drifted.end_ts_init = Some(UnixNanos::from(99));
        let backend = ManifestOverrideBackend::new(inner, drifted);

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");
        let reason = find_manifest_mismatch(&report.findings, ManifestField::EndTsInit);

        assert!(reason.contains("99"), "reason was: {reason}");
        assert!(reason.contains("25"), "reason was: {reason}");
    }

    #[rstest]
    fn manifest_end_ts_init_missing_when_sealed_with_entries() {
        // Sealed manifest forgot to record end_ts_init while the entry stream is
        // non-empty: validate_manifest's (None, Some) arm must fire and the
        // reason must carry the observed last ts_init.
        let mut inner = MemoryBackend::new();
        inner
            .open_run(manifest("run-end-ts-missing"))
            .expect("open run");
        inner
            .append_batch(&[append_with(1, 42, Vec::new())])
            .expect("append");
        inner.seal(RunStatus::Ended).expect("seal");

        let mut drifted = inner.manifest().expect("manifest");
        drifted.end_ts_init = None;
        let backend = ManifestOverrideBackend::new(inner, drifted);

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");
        let reason = find_manifest_mismatch(&report.findings, ManifestField::EndTsInit);

        assert!(reason.contains("missing"), "reason was: {reason}");
        assert!(reason.contains("42"), "reason was: {reason}");
    }

    #[rstest]
    fn manifest_end_ts_init_set_on_sealed_empty_run() {
        // Sealed manifest carries end_ts_init even though the entry table is
        // empty: validate_manifest's (Some, None) arm must fire and the reason
        // must carry the spurious stored value.
        let mut inner = MemoryBackend::new();
        inner
            .open_run(manifest("run-end-ts-empty"))
            .expect("open run");
        inner.seal(RunStatus::Ended).expect("seal");

        let mut drifted = inner.manifest().expect("manifest");
        drifted.end_ts_init = Some(UnixNanos::from(77));
        let backend = ManifestOverrideBackend::new(inner, drifted);

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");
        let reason = find_manifest_mismatch(&report.findings, ManifestField::EndTsInit);

        assert!(reason.contains("77"), "reason was: {reason}");
        assert!(reason.contains("empty"), "reason was: {reason}");
    }

    #[rstest]
    fn manifest_start_ts_init_drift() {
        // Earliest entry ts_init is 10, but the manifest's start_ts_init is 50.
        // Reason must carry both values so a flipped comparison or wrong-side
        // formatting fails the test.
        let mut inner = MemoryBackend::new();
        inner.open_run(manifest("run-start-ts")).expect("open run");
        inner
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 25, Vec::new()),
            ])
            .expect("append");
        inner.seal(RunStatus::Ended).expect("seal");

        let mut drifted = inner.manifest().expect("manifest");
        drifted.start_ts_init = UnixNanos::from(50);
        let backend = ManifestOverrideBackend::new(inner, drifted);

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");
        let reason = find_manifest_mismatch(&report.findings, ManifestField::StartTsInit);

        assert!(reason.contains("50"), "reason was: {reason}");
        assert!(reason.contains("10"), "reason was: {reason}");
    }

    #[rstest]
    fn trailing_gap_surfaces_when_last_seqs_missing() {
        // Inner backend holds seqs 1..=3, but both the manifest and the advertised
        // high-watermark claim 5. The verifier must walk to seq=5, find seqs 4-5
        // missing, and emit a single trailing GapRange{4,5}. Removing the
        // `flush_pending_gap(high_watermark + 1, ...)` call after the loop would
        // drop this finding entirely.
        let mut inner = MemoryBackend::new();
        inner
            .open_run(manifest("run-trailing-gap"))
            .expect("open run");
        inner
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
                append_with(3, 12, Vec::new()),
            ])
            .expect("append");
        inner.seal(RunStatus::Ended).expect("seal");

        let mut drifted = inner.manifest().expect("manifest");
        drifted.high_watermark = 5;
        // Advertise hwm=5 on both sides so the HighWatermark mismatch path stays
        // quiet and the test pins only the trailing-gap behavior.
        let backend = ManifestOverrideBackend::new(inner, drifted).with_high_watermark(5);

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");

        let gaps: Vec<GapRange> = report
            .findings
            .iter()
            .filter_map(|f| match f {
                VerifyFinding::Gap { range } => Some(*range),
                _ => None,
            })
            .collect();
        assert_eq!(gaps, vec![GapRange { from: 4, to: 5 }]);
        assert_eq!(report.entries_scanned, 3);
        assert_eq!(report.high_watermark, 5);
    }

    /// Test backend that rewrites a single `scan_seq` result so the value's
    /// embedded seq disagrees with the requested table key. Lets the unit suite
    /// exercise the redb-only "row moved under wrong key" corruption class
    /// without setting up a real on-disk file.
    struct SeqRewriteBackend {
        inner: MemoryBackend,
        target_key: u64,
        substitute: EventStoreEntry,
    }

    impl EventStore for SeqRewriteBackend {
        fn open_run(&mut self, m: RunManifest) -> Result<(), EventStoreError> {
            self.inner.open_run(m)
        }
        fn append_batch(&mut self, e: &[AppendEntry]) -> Result<u64, EventStoreError> {
            self.inner.append_batch(e)
        }
        fn scan_range(
            &self,
            from: u64,
            to: u64,
            direction: ScanDirection,
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            self.inner.scan_range(from, to, direction)
        }
        fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            if seq == self.target_key {
                return Ok(Some(self.substitute.clone()));
            }
            self.inner.scan_seq(seq)
        }
        fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
            self.inner.lookup(kind, key)
        }
        fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            self.inner.iter_index_keys(kind)
        }
        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.inner.seal(status)
        }
        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            self.inner.manifest()
        }
        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            self.inner.high_watermark()
        }
    }

    #[rstest]
    fn seq_mismatch_surfaces_when_row_value_disagrees_with_key() {
        // Row at table_key=2 holds the bytes of an entry whose embedded seq is 99.
        // The hash recomputes correctly (because the hash covers entry.seq=99),
        // so scan_seq returns Ok(Some(entry)) without raising HashMismatch. The
        // verifier must catch the key/embedded-seq divergence rather than mark
        // the slot clean.
        let mut inner = MemoryBackend::new();
        inner
            .open_run(manifest("run-seq-mismatch"))
            .expect("open run");
        inner
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
                append_with(3, 12, Vec::new()),
            ])
            .expect("append");
        inner.seal(RunStatus::Ended).expect("seal");

        let substitute = build_entry(99, Headers::empty(), 11);
        let backend = SeqRewriteBackend {
            inner,
            target_key: 2,
            substitute,
        };

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");

        assert!(
            report.findings.iter().any(|f| matches!(
                f,
                VerifyFinding::SeqMismatch {
                    table_key: 2,
                    embedded_seq: 99,
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn seq_mismatch_marks_target_corrupted_for_dependent_indices() {
        // Same row corruption as above, but the stored client_order_id index
        // points at the rewritten slot. The slot must be classified as corrupted
        // so the index drift surfaces TargetCorrupted rather than silently
        // accepting the lookup.
        let mut inner = MemoryBackend::new();
        inner
            .open_run(manifest("run-seq-mismatch-idx"))
            .expect("open run");
        inner
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                AppendEntry::new(
                    build_entry(2, Headers::empty(), 11),
                    vec![IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string())],
                ),
            ])
            .expect("append");
        inner.seal(RunStatus::Ended).expect("seal");

        let substitute = build_entry(99, Headers::empty(), 11);
        let backend = SeqRewriteBackend {
            inner,
            target_key: 2,
            substitute,
        };

        let report = Verifier::new(Box::new(backend)).verify().expect("verify");

        assert!(
            report.findings.iter().any(|f| matches!(
                f,
                VerifyFinding::IndexDrift {
                    kind: IndexKind::ClientOrderId,
                    drift: IndexDrift::TargetCorrupted { stored_seq: 2 },
                    ..
                }
            )),
            "findings was: {:?}",
            report.findings,
        );
    }

    #[rstest]
    fn verify_propagates_no_run_open_as_error() {
        let backend = MemoryBackend::new();
        let verifier = Verifier::new(Box::new(backend));

        let err = verifier.verify().expect_err("must fail");

        match err {
            VerifyError::Backend(EventStoreError::Backend(msg)) => {
                assert!(msg.contains("no run open"), "msg was: {msg}");
            }
            VerifyError::Backend(other) => {
                panic!("expected Backend(no run open), was {other:?}")
            }
        }
    }
}
