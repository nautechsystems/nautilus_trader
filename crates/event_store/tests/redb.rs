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

//! Integration tests for the [`RedbBackend`] against a tempdir-backed run file.
//!
//! Mirrors the in-memory backend's behavior matrix and exercises the on-disk specifics:
//! file layout, durability across `RedbBackend` instances (cross-process crash recovery),
//! and the manifest's status-driven open-time crash check.

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_event_store::{
    AppendEntry, EventStore, EventStoreEntry, EventStoreError, Headers, IndexKey, IndexKind,
    MemoryBackend, RedbBackend, RegisteredComponents, RunManifest, RunStatus, ScanDirection,
    SnapshotAnchor, Topic, compute_entry_hash,
};
use proptest::{prelude::*, test_runner::Config as ProptestConfig};
use rstest::rstest;
use tempfile::TempDir;
use ustr::Ustr;

const INSTANCE_ID: &str = "trader-001";

fn manifest(run_id: &str) -> RunManifest {
    RunManifest {
        run_id: run_id.to_string(),
        parent_run_id: None,
        instance_id: INSTANCE_ID.to_string(),
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

fn fresh_backend() -> (TempDir, RedbBackend) {
    let tmp = TempDir::new().expect("tempdir");
    let backend = RedbBackend::new(tmp.path());
    (tmp, backend)
}

fn open_backend() -> (TempDir, RedbBackend) {
    let (tmp, mut backend) = fresh_backend();
    backend
        .open_run(manifest("1700000000-aaaa1111"))
        .expect("open run");
    (tmp, backend)
}

#[rstest]
fn manifest_errors_when_no_run_open() {
    let (_tmp, backend) = fresh_backend();

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
    let (_tmp, mut backend) = fresh_backend();
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
fn open_run_creates_file_at_expected_path() {
    let (tmp, mut backend) = fresh_backend();
    backend
        .open_run(manifest("1700000000-aaaa1111"))
        .expect("open run");

    let expected = tmp
        .path()
        .join(INSTANCE_ID)
        .join("1700000000-aaaa1111.redb");
    assert!(
        expected.exists(),
        "expected redb file at {}",
        expected.display()
    );
    assert_eq!(backend.current_path().expect("path"), expected.as_path());
}

#[rstest]
fn open_run_normalizes_status_and_zeroes_progress() {
    let (_tmp, backend) = open_backend();
    let m = backend.manifest().expect("manifest");

    assert_eq!(m.status, RunStatus::Running);
    assert_eq!(m.high_watermark, 0);
    assert!(m.end_ts_init.is_none());
    assert_eq!(backend.high_watermark().expect("hwm"), 0);
}

#[rstest]
fn append_advances_high_watermark() {
    let (_tmp, mut backend) = open_backend();
    let batch = vec![
        append_with(1, 10, Vec::new()),
        append_with(2, 11, Vec::new()),
        append_with(3, 12, Vec::new()),
    ];

    let hwm = backend.append_batch(&batch).expect("append");

    assert_eq!(hwm, 3);
    assert_eq!(backend.high_watermark().expect("hwm"), 3);
    assert_eq!(backend.manifest().expect("m").high_watermark, 3);
}

#[rstest]
fn append_rejects_first_seq_not_at_watermark_plus_one() {
    let (_tmp, mut backend) = open_backend();
    let batch = vec![append_with(2, 10, Vec::new())];

    let err = backend.append_batch(&batch).expect_err("must reject");

    assert!(matches!(
        err,
        EventStoreError::OutOfOrder {
            high_watermark: 0,
            seq: 2,
        }
    ));
}

#[rstest]
fn append_rejects_within_batch_seq_gap() {
    let (_tmp, mut backend) = open_backend();
    let batch = vec![
        append_with(1, 10, Vec::new()),
        append_with(3, 11, Vec::new()),
    ];

    let err = backend.append_batch(&batch).expect_err("must reject");

    // Atomically rejected: durable hwm is still 0, not the within-batch cursor.
    assert!(matches!(
        err,
        EventStoreError::OutOfOrder {
            high_watermark: 0,
            seq: 3,
        }
    ));
    assert_eq!(backend.high_watermark().expect("hwm"), 0);
    // Failed batch must not have partially landed on disk either.
    assert!(backend.scan_seq(1).expect("scan").is_none());
}

#[rstest]
fn append_after_seal_returns_closed() {
    let (_tmp, mut backend) = open_backend();
    backend
        .append_batch(&[append_with(1, 10, Vec::new())])
        .expect("append");
    backend.seal(RunStatus::Ended).expect("seal");

    let err = backend
        .append_batch(&[append_with(2, 11, Vec::new())])
        .expect_err("must reject");

    assert!(matches!(err, EventStoreError::Closed));
}

#[rstest]
fn empty_batch_is_a_noop() {
    let (_tmp, mut backend) = open_backend();
    let hwm = backend.append_batch(&[]).expect("append");

    assert_eq!(hwm, 0);
    assert_eq!(backend.high_watermark().expect("hwm"), 0);
}

#[rstest]
fn snapshot_anchor_is_none_until_recorded() {
    let (_tmp, backend) = open_backend();

    assert!(
        backend
            .latest_snapshot_anchor()
            .expect("latest anchor")
            .is_none()
    );
}

#[rstest]
fn snapshot_anchor_round_trips_and_persists_for_sealed_reader() {
    let (tmp, mut backend) = open_backend();
    backend
        .append_batch(&[
            append_with(1, 10, Vec::new()),
            append_with(2, 11, Vec::new()),
        ])
        .expect("append");
    let anchor = SnapshotAnchor::new(2, "cache://snapshots/run-1/2", "blake3:abc");

    backend
        .record_snapshot_anchor(anchor.clone())
        .expect("record anchor");
    assert_eq!(
        backend.latest_snapshot_anchor().expect("latest anchor"),
        Some(anchor.clone()),
    );

    backend.seal(RunStatus::Ended).expect("seal");
    drop(backend);

    let reader = RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "1700000000-aaaa1111")
        .expect("open sealed");
    assert_eq!(
        reader.latest_snapshot_anchor().expect("latest anchor"),
        Some(anchor),
    );
}

#[rstest]
fn snapshot_anchor_rejects_watermark_past_durable_hwm() {
    let (_tmp, mut backend) = open_backend();

    let err = backend
        .record_snapshot_anchor(SnapshotAnchor::new(1, "blob", "hash"))
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
fn snapshot_anchor_rejects_backward_move() {
    let (_tmp, mut backend) = open_backend();
    backend
        .append_batch(&[
            append_with(1, 10, Vec::new()),
            append_with(2, 11, Vec::new()),
        ])
        .expect("append");
    backend
        .record_snapshot_anchor(SnapshotAnchor::new(2, "latest", "hash-latest"))
        .expect("record latest");

    let err = backend
        .record_snapshot_anchor(SnapshotAnchor::new(1, "older", "hash-older"))
        .expect_err("must reject");

    match err {
        EventStoreError::Backend(msg) => {
            assert!(msg.contains("older than latest anchor"), "msg was: {msg}",);
        }
        other => panic!("expected Backend, was {other:?}"),
    }
}

#[rstest]
fn snapshot_anchor_after_seal_returns_closed() {
    let (_tmp, mut backend) = open_backend();
    backend
        .append_batch(&[append_with(1, 10, Vec::new())])
        .expect("append");
    backend.seal(RunStatus::Ended).expect("seal");

    let err = backend
        .record_snapshot_anchor(SnapshotAnchor::new(1, "blob", "hash"))
        .expect_err("must reject");

    assert!(matches!(err, EventStoreError::Closed));
}

#[rstest]
fn latest_snapshot_anchor_returns_corrupted_when_anchor_bytes_are_garbled() {
    let run_id = "run-anchor-decode";
    let tmp = TempDir::new().expect("tempdir");
    let path = {
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest(run_id)).expect("open run");
        backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(1, "blob", "hash"))
            .expect("record anchor");
        backend.seal(RunStatus::Ended).expect("seal");
        backend.current_path().expect("path").to_path_buf()
    };

    {
        let snapshot_anchor: redb::TableDefinition<&str, &[u8]> =
            redb::TableDefinition::new("snapshot_anchor");
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(snapshot_anchor).expect("open table");
            table
                .insert("latest", b"\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF".as_slice())
                .expect("overwrite latest snapshot anchor");
        }
        txn.commit().expect("commit overwrite");
    }

    let reader = RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, run_id).expect("open sealed");
    let err = reader
        .latest_snapshot_anchor()
        .expect_err("must flag corruption");

    match err {
        EventStoreError::Corrupted(msg) => {
            assert!(msg.contains("decode snapshot anchor"), "msg was: {msg}");
        }
        other => panic!("expected Corrupted, was {other:?}"),
    }
}

#[rstest]
fn scan_seq_returns_committed_entry() {
    let (_tmp, mut backend) = open_backend();
    backend
        .append_batch(&[
            append_with(1, 10, Vec::new()),
            append_with(2, 11, Vec::new()),
        ])
        .expect("append");

    let entry = backend.scan_seq(2).expect("scan").expect("present");

    assert_eq!(entry.seq, 2);
    assert_eq!(entry.ts_init, UnixNanos::from(11));
}

#[rstest]
fn scan_seq_returns_none_outside_watermark() {
    let (_tmp, mut backend) = open_backend();
    backend
        .append_batch(&[append_with(1, 10, Vec::new())])
        .expect("append");

    assert!(backend.scan_seq(0).expect("scan").is_none());
    assert!(backend.scan_seq(2).expect("scan").is_none());
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
    #[case] from: u64,
    #[case] to: u64,
    #[case] direction: ScanDirection,
    #[case] expected: Vec<u64>,
) {
    let (_tmp, mut backend) = open_backend();
    backend
        .append_batch(&[
            append_with(1, 10, Vec::new()),
            append_with(2, 11, Vec::new()),
            append_with(3, 12, Vec::new()),
        ])
        .expect("append");

    let seqs: Vec<u64> = backend
        .scan_range(from, to, direction)
        .expect("scan")
        .into_iter()
        .map(|e| e.seq)
        .collect();

    assert_eq!(seqs, expected);
}

#[rstest]
fn lookup_records_first_occurrence_per_kind() {
    let (_tmp, mut backend) = open_backend();
    let intent = "intent-1".to_string();
    let cl_ord = "O-1".to_string();
    let venue = "V-1".to_string();
    backend
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
                    IndexKey::new(IndexKind::IntentId, intent.clone()),
                    IndexKey::new(IndexKind::ClientOrderId, cl_ord.clone()),
                    IndexKey::new(IndexKind::VenueOrderId, venue.clone()),
                ],
            ),
        ])
        .expect("append");

    assert_eq!(
        backend
            .lookup(IndexKind::IntentId, &intent)
            .expect("lookup"),
        Some(1),
    );
    assert_eq!(
        backend
            .lookup(IndexKind::ClientOrderId, &cl_ord)
            .expect("lookup"),
        Some(1),
    );
    assert_eq!(
        backend
            .lookup(IndexKind::VenueOrderId, &venue)
            .expect("lookup"),
        Some(1),
    );
    assert!(
        backend
            .lookup(IndexKind::IntentId, "missing")
            .expect("lookup")
            .is_none(),
    );
}

#[rstest]
fn within_entry_duplicate_keys_resolve_to_first_seq() {
    let (_tmp, mut backend) = open_backend();
    let key = "O-1".to_string();
    backend
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
        backend
            .lookup(IndexKind::ClientOrderId, &key)
            .expect("lookup"),
        Some(1),
    );
}

#[rstest]
fn lookup_isolates_keys_by_kind() {
    let (_tmp, mut backend) = open_backend();
    let key = "shared".to_string();
    backend
        .append_batch(&[AppendEntry::new(
            build_entry(1, Headers::empty(), 10),
            vec![IndexKey::new(IndexKind::ClientOrderId, key.clone())],
        )])
        .expect("append");

    assert_eq!(
        backend
            .lookup(IndexKind::ClientOrderId, &key)
            .expect("lookup"),
        Some(1),
    );
    assert!(
        backend
            .lookup(IndexKind::VenueOrderId, &key)
            .expect("lookup")
            .is_none(),
    );
}

#[rstest]
#[case::ended(RunStatus::Ended)]
#[case::crashed_recovered(RunStatus::CrashedRecovered)]
#[case::quarantined(RunStatus::Quarantined)]
fn seal_stamps_end_ts_and_blocks_re_seal(#[case] status: RunStatus) {
    let (_tmp, mut backend) = open_backend();
    backend
        .append_batch(&[
            append_with(1, 10, Vec::new()),
            append_with(2, 25, Vec::new()),
            append_with(3, 17, Vec::new()),
        ])
        .expect("append");

    backend.seal(status).expect("seal");

    let m = backend.manifest().expect("manifest");
    assert_eq!(m.status, status);
    assert_eq!(m.high_watermark, 3);
    assert_eq!(m.end_ts_init, Some(UnixNanos::from(25)));

    let err = backend.seal(RunStatus::Ended).expect_err("re-seal");
    assert!(matches!(err, EventStoreError::Closed));
}

#[rstest]
fn seal_rejects_running_status() {
    let (_tmp, mut backend) = open_backend();
    let err = backend.seal(RunStatus::Running).expect_err("must reject");

    match err {
        EventStoreError::Backend(msg) => {
            assert!(msg.contains("Running"), "msg was: {msg}");
        }
        other => panic!("expected Backend, was {other:?}"),
    }
    assert!(!backend.manifest().expect("manifest").is_sealed());
    backend
        .append_batch(&[append_with(1, 10, Vec::new())])
        .expect("append");
}

#[rstest]
fn seal_with_no_entries_leaves_end_ts_unset() {
    let (_tmp, mut backend) = open_backend();
    backend.seal(RunStatus::Ended).expect("seal");

    let m = backend.manifest().expect("manifest");
    assert_eq!(m.status, RunStatus::Ended);
    assert!(m.end_ts_init.is_none());
    assert_eq!(m.high_watermark, 0);
}

#[rstest]
fn reopening_running_run_returns_crashed_predecessor_in_same_backend() {
    let (_tmp, mut backend) = fresh_backend();
    backend.open_run(manifest("run-1")).expect("open 1");
    backend
        .append_batch(&[append_with(1, 10, Vec::new())])
        .expect("append");

    let err = backend.open_run(manifest("run-2")).expect_err("must flag");
    assert!(matches!(err, EventStoreError::CrashedPredecessor));

    assert!(
        backend.scan_seq(1).expect("scan").is_some(),
        "predecessor entry must survive failed open_run",
    );

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
    let (_tmp, mut backend) = fresh_backend();
    backend.open_run(manifest("run-1")).expect("open 1");
    backend.seal(RunStatus::Ended).expect("seal");

    backend.open_run(manifest("run-2")).expect("open 2");
    assert_eq!(
        backend.manifest().expect("manifest").run_id,
        "run-2".to_string(),
    );
}

#[rstest]
fn cross_backend_crash_recovery_replays_progress() {
    // Simulates the cross-process crash path: backend writes entries, drops without
    // sealing, a new backend instance opens the same run id and surfaces it as a
    // crashed predecessor with the durable high-watermark and max ts_init recovered
    // from the redb file.
    let tmp = TempDir::new().expect("tempdir");
    {
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest("run-1")).expect("open 1");
        backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 25, Vec::new()),
                append_with(3, 17, Vec::new()),
            ])
            .expect("append");
        // Backend dropped here without sealing.
    }

    {
        let mut recovered = RedbBackend::new(tmp.path());
        let err = recovered
            .open_run(manifest("run-1"))
            .expect_err("must flag crashed predecessor");
        assert!(matches!(err, EventStoreError::CrashedPredecessor));

        assert_eq!(recovered.high_watermark().expect("hwm"), 3);
        let pre_seal = recovered.manifest().expect("manifest");
        assert_eq!(pre_seal.status, RunStatus::Running);
        assert_eq!(pre_seal.high_watermark, 3);

        recovered.seal(RunStatus::CrashedRecovered).expect("seal");
        let sealed = recovered.manifest().expect("manifest");
        assert_eq!(sealed.status, RunStatus::CrashedRecovered);
        assert_eq!(sealed.high_watermark, 3);
        assert_eq!(sealed.end_ts_init, Some(UnixNanos::from(25)));
        // recovered drops here, releasing the redb file lock so the next backend
        // can open the same path without DatabaseAlreadyOpen.
    }

    // After sealing, a brand new backend opening the same file sees a sealed manifest
    // and rejects the open with a Backend error rather than CrashedPredecessor.
    let mut after = RedbBackend::new(tmp.path());
    let err = after.open_run(manifest("run-1")).expect_err("must reject");

    match err {
        EventStoreError::Backend(msg) => {
            assert!(msg.contains("already sealed"), "msg was: {msg}");
            assert!(msg.contains("run-1.redb"), "msg was: {msg}");
        }
        other => panic!("expected Backend, was {other:?}"),
    }
}

#[rstest]
fn cross_backend_seal_persists_to_disk() {
    let tmp = TempDir::new().expect("tempdir");
    {
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest("run-1")).expect("open 1");
        backend
            .append_batch(&[append_with(1, 10, Vec::new())])
            .expect("append");
        backend.seal(RunStatus::Ended).expect("seal");
    }

    // Open a second backend instance with a fresh run id; the prior file remains on
    // disk, but the new run gets its own .redb file.
    let mut backend = RedbBackend::new(tmp.path());
    backend.open_run(manifest("run-2")).expect("open 2");
    assert_eq!(backend.high_watermark().expect("hwm"), 0);

    let prior = tmp.path().join(INSTANCE_ID).join("run-1.redb");
    assert!(prior.exists(), "sealed predecessor file must persist");
}

#[rstest]
fn scan_recomputes_hash_and_quarantines_on_mismatch() {
    // Trust the writer: the entry's `entry_hash` field is what gets stored. By
    // tampering with the payload before append, the stored hash no longer matches the
    // recomputed hash, and scan must surface HashMismatch rather than the corrupted
    // row.
    let (_tmp, mut backend) = open_backend();
    let mut tampered = build_entry(1, Headers::empty(), 10);
    tampered.payload = Bytes::from_static(b"\xFF\xFF");
    backend
        .append_batch(&[AppendEntry::without_indices(tampered)])
        .expect("append");

    assert!(matches!(
        backend.scan_seq(1),
        Err(EventStoreError::HashMismatch { seq: 1 }),
    ));
    assert!(matches!(
        backend.scan_range(1, 1, ScanDirection::Forward),
        Err(EventStoreError::HashMismatch { seq: 1 }),
    ));
}

#[rstest]
fn scan_seq_returns_gap_for_missing_in_watermark_row() {
    // Manufacture corruption: append three entries, drop the backend without sealing,
    // then open the redb file directly and remove seq=2 so the entries table has a
    // hole inside the durable watermark. Reopening via the backend surfaces the run
    // as a crashed predecessor (high_watermark recovered from `last()`), and any read
    // of seq=2 must report a Gap rather than silently returning None.
    let tmp = TempDir::new().expect("tempdir");
    let path = {
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest("run-gap")).expect("open run");
        backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
                append_with(3, 12, Vec::new()),
            ])
            .expect("append");
        backend.current_path().expect("path").to_path_buf()
    };

    {
        let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            table.remove(2_u64).expect("remove seq 2");
        }
        txn.commit().expect("commit removal");
    }

    let mut recovered = RedbBackend::new(tmp.path());
    let err = recovered
        .open_run(manifest("run-gap"))
        .expect_err("must flag crashed predecessor");
    assert!(matches!(err, EventStoreError::CrashedPredecessor));

    // high_watermark recovered from redb's `last()` is still 3 even though seq=2 is gone.
    assert_eq!(recovered.high_watermark().expect("hwm"), 3);

    match recovered.scan_seq(2) {
        Err(EventStoreError::Gap {
            prev: 1,
            next: 3,
            missing: 2,
        }) => {}
        other => panic!("expected Gap{{prev:1,next:3,missing:2}}, was {other:?}"),
    }

    match recovered.scan_range(1, 3, ScanDirection::Forward) {
        Err(EventStoreError::Gap {
            prev: 1,
            next: 3,
            missing: 2,
        }) => {}
        other => panic!("expected Gap{{prev:1,next:3,missing:2}}, was {other:?}"),
    }
}

#[rstest]
fn scan_range_reports_gap_at_tail_when_iter_ends_early() {
    // Tail gap branch: the iterator runs out of rows before reaching `hi`, but
    // `high_watermark` is still high because rows exist *beyond* the requested
    // window. To trigger this, append seqs 1..=5, remove 3 and 4 (keep 5 so
    // last() still returns 5), then scan [1, 4]: redb yields {1, 2}, then the
    // iter ends at expected=3 with hi=4 still unmet.
    let tmp = TempDir::new().expect("tempdir");
    let path = {
        let mut backend = RedbBackend::new(tmp.path());
        backend
            .open_run(manifest("run-tail-gap"))
            .expect("open run");
        backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
                append_with(3, 12, Vec::new()),
                append_with(4, 13, Vec::new()),
                append_with(5, 14, Vec::new()),
            ])
            .expect("append");
        backend.current_path().expect("path").to_path_buf()
    };

    {
        let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            table.remove(3_u64).expect("remove seq 3");
            table.remove(4_u64).expect("remove seq 4");
        }
        txn.commit().expect("commit removal");
    }

    let mut recovered = RedbBackend::new(tmp.path());
    let err = recovered
        .open_run(manifest("run-tail-gap"))
        .expect_err("must flag crashed predecessor");
    assert!(matches!(err, EventStoreError::CrashedPredecessor));

    // last() = 5, so the recovered high_watermark stays at 5 even though the
    // middle rows are gone.
    assert_eq!(recovered.high_watermark().expect("hwm"), 5);

    match recovered.scan_range(1, 4, ScanDirection::Forward) {
        Err(EventStoreError::Gap {
            prev: 2,
            next: 5,
            missing: 3,
        }) => {}
        other => panic!("expected Gap{{prev:2,next:5,missing:3}}, was {other:?}"),
    }
}

#[rstest]
fn append_extracts_no_indices_when_keys_empty() {
    let (_tmp, mut backend) = open_backend();
    let headers = Headers {
        intent_id: Some(UUID4::new()),
        ..Headers::empty()
    };
    backend
        .append_batch(&[AppendEntry::without_indices(build_entry(1, headers, 10))])
        .expect("append");

    assert!(
        backend
            .lookup(IndexKind::IntentId, "any")
            .expect("lookup")
            .is_none(),
    );
}

#[rstest]
fn parity_with_memory_backend_for_indices() {
    // Sanity check that the redb backend's index visibility matches the memory backend
    // for the same input: guards against a silent storage divergence (e.g. lookup
    // returning a stringly-similar but non-identical match).
    let (_tmp, mut redb) = open_backend();
    let mut memory = MemoryBackend::new();
    memory
        .open_run(manifest("1700000000-aaaa1111"))
        .expect("memory open");

    let intent = "intent-1".to_string();
    let cl_ord = "O-1".to_string();
    let key_set = vec![
        IndexKey::new(IndexKind::IntentId, intent.clone()),
        IndexKey::new(IndexKind::ClientOrderId, cl_ord.clone()),
    ];

    let batch = vec![AppendEntry::new(
        build_entry(1, Headers::empty(), 10),
        key_set,
    )];
    redb.append_batch(&batch).expect("redb append");
    memory.append_batch(&batch).expect("memory append");

    for kind in [
        IndexKind::IntentId,
        IndexKind::ClientOrderId,
        IndexKind::VenueOrderId,
    ] {
        let key = match kind {
            IndexKind::IntentId => &intent,
            IndexKind::ClientOrderId => &cl_ord,
            IndexKind::VenueOrderId => &intent,
        };
        assert_eq!(
            redb.lookup(kind, key).expect("redb lookup"),
            memory.lookup(kind, key).expect("memory lookup"),
            "mismatch for kind {kind:?}",
        );
    }
}

#[rstest]
fn scan_returns_corrupted_when_entry_bytes_are_garbled() {
    // Garbled bincode payload at a known seq must surface as Corrupted, not Backend
    // or Gap. Drives the decode->Corrupted classification on both scan paths.
    let tmp = TempDir::new().expect("tempdir");
    let path = {
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest("run-decode")).expect("open run");
        backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
                append_with(3, 12, Vec::new()),
            ])
            .expect("append");
        backend.current_path().expect("path").to_path_buf()
    };

    {
        let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            // Replace seq=2's bytes with something that is not a valid bincode envelope.
            table
                .insert(2_u64, b"\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF".as_slice())
                .expect("overwrite seq 2");
        }
        txn.commit().expect("commit overwrite");
    }

    let mut recovered = RedbBackend::new(tmp.path());
    let err = recovered
        .open_run(manifest("run-decode"))
        .expect_err("must flag crashed predecessor");
    // compute_progress walks every entry to recover max ts_init; a malformed row
    // surfaces there before we ever reach the scan paths.
    match err {
        EventStoreError::Corrupted(msg) => {
            assert!(msg.contains("decode entry on load"), "msg was: {msg}",);
        }
        other => panic!("expected Corrupted from compute_progress, was {other:?}"),
    }
}

#[rstest]
fn open_run_returns_corrupted_when_manifest_key_missing() {
    // Manifest table present but the "current" key has been cleared. open_run
    // must surface this as Corrupted (the run file existed, so the missing key
    // is structural damage), not as a fresh-run open or a Backend error.
    let tmp = TempDir::new().expect("tempdir");
    let path = {
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest("run-no-mk")).expect("open run");
        backend.current_path().expect("path").to_path_buf()
    };

    {
        let manifest_table: redb::TableDefinition<&str, &[u8]> =
            redb::TableDefinition::new("manifest");
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(manifest_table).expect("open table");
            table.remove("current").expect("remove current key");
        }
        txn.commit().expect("commit removal");
    }

    let mut recovered = RedbBackend::new(tmp.path());
    let err = recovered
        .open_run(manifest("run-no-mk"))
        .expect_err("must flag corruption");

    match err {
        EventStoreError::Corrupted(msg) => {
            assert!(msg.contains("missing manifest"), "msg was: {msg}");
        }
        other => panic!("expected Corrupted, was {other:?}"),
    }
}

#[rstest]
fn open_run_returns_corrupted_for_table_type_mismatch() {
    // Hand-built file where the "manifest" table exists with the wrong value
    // type (`u64` instead of `&[u8]`). redb returns TableTypeMismatch on the
    // first read; our map_table_err must classify it as Corrupted, not Backend.
    let tmp = TempDir::new().expect("tempdir");
    let dir = tmp.path().join(INSTANCE_ID);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("run-typemismatch.redb");

    {
        let manifest_wrong: redb::TableDefinition<&str, u64> =
            redb::TableDefinition::new("manifest");
        let db = redb::Database::create(&path).expect("create redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(manifest_wrong).expect("open table");
            table.insert("current", 42_u64).expect("insert");
        }
        txn.commit().expect("commit");
    }

    let mut backend = RedbBackend::new(tmp.path());
    let err = backend
        .open_run(manifest("run-typemismatch"))
        .expect_err("must flag corruption");

    match err {
        EventStoreError::Corrupted(_) => {}
        other => panic!("expected Corrupted from TableTypeMismatch, was {other:?}"),
    }
}

fn build_payload_entry(seq: u64, ts_init: u64, payload: &[u8]) -> EventStoreEntry {
    let topic: Topic = "prop.topic".into();
    let payload_type = Ustr::from("PropPayload");
    let payload_bytes = Bytes::copy_from_slice(payload);
    let headers = Headers::empty();
    let ts_init = UnixNanos::from(ts_init);
    let ts_publish = UnixNanos::from(ts_init.as_u64().saturating_add(1));
    let hash = compute_entry_hash(
        seq,
        ts_init,
        ts_publish,
        topic.as_ref(),
        payload_type.as_str(),
        &payload_bytes,
        &headers,
    );

    EventStoreEntry::new(
        hash,
        seq,
        headers,
        topic,
        payload_type,
        payload_bytes,
        ts_init,
        ts_publish,
    )
}

proptest! {
    // Each case spawns a tempdir + redb file, so cap the run rather than rely on
    // proptest's default 256 cases. 32 is enough to surface non-deterministic
    // edge cases without the test taking minutes.
    #![proptest_config(ProptestConfig {
        cases: 32,
        ..ProptestConfig::default()
    })]

    /// For any batch of valid entries, append-then-scan returns the same payload
    /// bytes and hashes in seq order, and high_watermark equals the batch length.
    #[rstest]
    fn prop_append_then_scan_roundtrip(
        payloads in proptest::collection::vec(
            proptest::collection::vec(any::<u8>(), 0..32),
            1..16,
        ),
        ts_inits in proptest::collection::vec(any::<u64>(), 1..16),
    ) {
        let n = payloads.len().min(ts_inits.len());
        let mut appends = Vec::with_capacity(n);

        for i in 0..n {
            let seq = u64::try_from(i + 1).expect("seq fits");
            let entry = build_payload_entry(seq, ts_inits[i], &payloads[i]);
            appends.push(AppendEntry::without_indices(entry));
        }

        let tmp = TempDir::new().expect("tempdir");
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest("prop-run")).expect("open");
        let hwm = backend.append_batch(&appends).expect("append");

        let total = u64::try_from(n).expect("len fits");
        prop_assert_eq!(hwm, total);
        prop_assert_eq!(backend.high_watermark().expect("hwm"), total);

        let scanned = backend
            .scan_range(1, total, ScanDirection::Forward)
            .expect("scan");
        prop_assert_eq!(scanned.len(), n);

        for (got, expected) in scanned.iter().zip(appends.iter()) {
            prop_assert_eq!(got.seq, expected.entry.seq);
            prop_assert_eq!(got.payload.as_ref(), expected.entry.payload.as_ref());
            prop_assert_eq!(got.entry_hash, expected.entry.entry_hash);
            prop_assert_eq!(got.ts_init, expected.entry.ts_init);
        }

        // Reverse scan returns the same entries in reverse order.
        let reversed = backend
            .scan_range(1, total, ScanDirection::Reverse)
            .expect("reverse scan");
        let reversed_seqs: Vec<u64> = reversed.iter().map(|e| e.seq).collect();
        let mut expected_reversed: Vec<u64> = (1..=total).collect();
        expected_reversed.reverse();
        prop_assert_eq!(reversed_seqs, expected_reversed);
    }
}

#[rstest]
fn iter_index_keys_enumerates_first_write_wins_pairs() {
    // RedbBackend pins the same iter_index_keys contract MemoryBackend has: the
    // verifier's cross-check depends on enumerating every (key, seq) under each
    // sidecar table, with first-write-wins on duplicate keys and per-kind
    // isolation. Direct coverage so a backend regression cannot hide behind the
    // verifier's transitive use.
    let (_tmp, mut backend) = open_backend();
    backend
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
                    IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string()),
                    IndexKey::new(IndexKind::ClientOrderId, "O-2".to_string()),
                ],
            ),
        ])
        .expect("append");

    let mut client = backend
        .iter_index_keys(IndexKind::ClientOrderId)
        .expect("iter");
    client.sort();
    assert_eq!(
        client,
        vec![("O-1".to_string(), 1u64), ("O-2".to_string(), 2u64)],
    );

    let venue = backend
        .iter_index_keys(IndexKind::VenueOrderId)
        .expect("iter");
    assert_eq!(venue, vec![("V-1".to_string(), 1u64)]);

    assert!(
        backend
            .iter_index_keys(IndexKind::IntentId)
            .expect("iter")
            .is_empty(),
    );
}

#[rstest]
fn iter_index_keys_errors_when_no_run_open() {
    let (_tmp, backend) = fresh_backend();

    match backend.iter_index_keys(IndexKind::IntentId) {
        Err(EventStoreError::Backend(msg)) => {
            assert!(msg.contains("no run open"), "msg was: {msg}");
        }
        other => panic!("expected Backend, was {other:?}"),
    }
}
