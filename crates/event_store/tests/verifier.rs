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

//! Integration tests for the off-trader [`Verifier`] against a tempdir-backed
//! [`RedbBackend`].
//!
//! Mirrors the production posture: the run file lives on disk, sealed by a writer that
//! has already exited. The verifier opens the sealed file via [`Verifier::open_redb`]
//! and proves the run is intact end-to-end. A companion test rebuilds the file with a
//! manufactured gap in the entry table to confirm the verifier surfaces it as a typed
//! finding rather than aborting the integrity scan.

use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    process::Command,
};

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use nautilus_event_store::{
    AppendEntry, DataClass, DataCursorSnapshot, EventStore, EventStoreEntry, GapRange, Headers,
    IndexKey, IndexKind, MarkerBackend, MarkerManifest, RedbBackend, RedbMarkerBackend,
    RegisteredComponents, RunManifest, RunStatus, SnapshotAnchor, StreamCursor, Topic, Verifier,
    VerifyFinding, codec, compute_entry_hash, compute_marker_hash,
};
use redb::ReadableTable;
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

fn build_entry(seq: u64, ts_init: u64) -> EventStoreEntry {
    let topic: Topic = "exec.command.SubmitOrder".into();
    let payload_type = Ustr::from("SubmitOrder");
    let payload = Bytes::from_static(b"\x01\x02\x03\x04");
    let headers = Headers::empty();
    let ts_publish = UnixNanos::from(ts_init + 1);
    let ts_init_ns = UnixNanos::from(ts_init);
    let hash = compute_entry_hash(
        seq,
        ts_init_ns,
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
        ts_init_ns,
        ts_publish,
    )
}

fn append_with(seq: u64, ts_init: u64, index_keys: Vec<IndexKey>) -> AppendEntry {
    AppendEntry::new(build_entry(seq, ts_init), index_keys)
}

fn write_sealed_run(tmp: &TempDir, run_id: &str) {
    let mut backend = RedbBackend::new(tmp.path());
    backend.open_run(manifest(run_id)).expect("open run");
    backend
        .append_batch(&[
            AppendEntry::new(
                build_entry(1, 10),
                vec![IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string())],
            ),
            AppendEntry::new(
                build_entry(2, 11),
                vec![IndexKey::new(IndexKind::ClientOrderId, "O-2".to_string())],
            ),
            append_with(3, 12, Vec::new()),
        ])
        .expect("append");
    backend.seal(RunStatus::Ended).expect("seal");
    // Backend drops here, releasing the redb file lock so the verifier can open it.
}

fn write_sealed_run_of(tmp: &TempDir, run_id: &str, count: u64) {
    let mut backend = RedbBackend::new(tmp.path());
    backend.open_run(manifest(run_id)).expect("open run");
    let batch: Vec<AppendEntry> = (1..=count)
        .map(|seq| append_with(seq, 10 + seq, Vec::new()))
        .collect();
    backend.append_batch(&batch).expect("append");
    backend.seal(RunStatus::Ended).expect("seal");
}

fn run_path(tmp: &TempDir, run_id: &str) -> std::path::PathBuf {
    tmp.path().join(INSTANCE_ID).join(format!("{run_id}.redb"))
}

fn marker_path(tmp: &TempDir, run_id: &str) -> std::path::PathBuf {
    tmp.path()
        .join(INSTANCE_ID)
        .join(format!("{run_id}.markers.redb"))
}

fn marker_manifest(run_id: &str) -> MarkerManifest {
    MarkerManifest {
        run_id: run_id.to_string(),
        enabled_classes: vec![DataClass::Quote],
        high_fidelity: false,
        snapshot_count: 0,
        hifi_count: 0,
        gap_count: 0,
        dict_count: 0,
        status: RunStatus::Running,
    }
}

fn marker_snapshot(marker_seq: u64, event_seq_before: u64) -> DataCursorSnapshot {
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

fn verify_bin(path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_verify"))
        .arg(path)
        .output()
        .expect("run verifier binary")
}

fn flip_stored_entry_payload_byte(path: &std::path::Path, seq: u64) {
    let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let db = redb::Database::create(path).expect("open redb");
    let txn = db.begin_write().expect("begin write");
    {
        let mut table = txn.open_table(entries).expect("open entries");
        let mut bytes = {
            let row = table.get(seq).expect("get entry").expect("entry present");
            row.value().to_vec()
        };
        // `build_entry` uses this fixed payload for every entry; flipping inside it
        // preserves the stored hash and forces the verifier's recompute check to fail.
        let payload_offset = bytes
            .windows(4)
            .position(|window| window == b"\x01\x02\x03\x04")
            .expect("payload bytes present");
        bytes[payload_offset + 2] ^= 0xFF;
        table
            .insert(seq, bytes.as_slice())
            .expect("overwrite entry");
    }
    txn.commit().expect("commit flip");
}

fn zero_tail_truncate(path: &std::path::Path) {
    let original_len = fs::metadata(path).expect("metadata").len();
    let retained_len = original_len / 2;
    let zeroed_len = usize::try_from(original_len - retained_len).expect("tail length fits");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("open for zero-tail truncate");
    file.set_len(retained_len).expect("truncate tail");
    file.seek(SeekFrom::Start(retained_len)).expect("seek tail");
    file.write_all(&vec![0_u8; zeroed_len])
        .expect("write zero tail");
    file.flush().expect("flush zero tail");
}

#[rstest]
fn clean_run_reports_no_findings() {
    let tmp = TempDir::new().expect("tempdir");
    write_sealed_run(&tmp, "1700000000-cafe0001");

    let verifier =
        Verifier::open_redb(tmp.path(), INSTANCE_ID, "1700000000-cafe0001").expect("open verifier");
    let report = verifier.verify().expect("verify");

    assert!(report.is_clean(), "findings was: {:?}", report.findings);
    assert_eq!(report.high_watermark, 3);
    assert_eq!(report.entries_scanned, 3);
    assert_eq!(report.status, RunStatus::Ended);
    assert_eq!(report.run_id, "1700000000-cafe0001");
}

#[rstest]
fn binary_clean_run_exits_zero() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0101";
    write_sealed_run(&tmp, run_id);

    let output = verify_bin(&run_path(&tmp, run_id));

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(output.status.success(), "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("clean"), "stdout was: {stdout}");
    assert!(stdout.contains("high_watermark=3"), "stdout was: {stdout}");
    assert!(stderr.is_empty(), "stderr was: {stderr}");
}

#[rstest]
fn binary_missing_marker_sidecar_reports_absent_without_error() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0111";
    write_sealed_run(&tmp, run_id);

    let output = verify_bin(&run_path(&tmp, run_id));

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(output.status.success(), "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("clean"), "stdout was: {stdout}");
    assert!(stdout.contains("markers=absent"), "stdout was: {stdout}");
    assert!(stderr.is_empty(), "stderr was: {stderr}");
}

#[rstest]
fn binary_clean_marker_sidecar_reports_clean_without_error() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0113";
    write_sealed_run(&tmp, run_id);

    {
        let mut marker_backend = RedbMarkerBackend::new(marker_path(&tmp, run_id));
        marker_backend
            .open_run(marker_manifest(run_id))
            .expect("open marker run");
        let snapshot = marker_snapshot(1, 1);
        marker_backend
            .append_snapshot(&snapshot, compute_marker_hash(&snapshot))
            .expect("append marker snapshot");
        marker_backend.seal(RunStatus::Ended).expect("seal markers");
    }

    let output = verify_bin(&run_path(&tmp, run_id));

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(output.status.success(), "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("clean"), "stdout was: {stdout}");
    assert!(stdout.contains("markers=clean"), "stdout was: {stdout}");
    assert!(
        stdout.contains("marker_snapshots_scanned=1"),
        "stdout was: {stdout}",
    );
    assert!(stderr.is_empty(), "stderr was: {stderr}");
}

#[rstest]
fn binary_marker_hash_mismatch_exits_corrupt_without_quarantine() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0112";
    write_sealed_run(&tmp, run_id);

    {
        let mut marker_backend = RedbMarkerBackend::new(marker_path(&tmp, run_id));
        marker_backend
            .open_run(marker_manifest(run_id))
            .expect("open marker run");
        let snapshot = marker_snapshot(1, 1);
        marker_backend
            .append_snapshot(&snapshot, [0xAA; 32])
            .expect("append marker snapshot");
        marker_backend.seal(RunStatus::Ended).expect("seal markers");
    }

    let output = verify_bin(&run_path(&tmp, run_id));

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={stdout} stderr={stderr}",
    );
    assert!(stdout.contains("corrupt"), "stdout was: {stdout}");
    assert!(
        stdout.contains("marker hash mismatch snapshot marker_seq=1"),
        "stdout was: {stdout}",
    );
    assert!(
        stdout.contains("quarantine=not-performed"),
        "stdout was: {stdout}",
    );
    assert!(stderr.is_empty(), "stderr was: {stderr}");
}

#[rstest]
fn binary_hash_mismatch_exits_corrupt_without_quarantine() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0102";
    write_sealed_run(&tmp, run_id);

    let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let path = run_path(&tmp, run_id);
    let mut tampered = build_entry(2, 11);
    tampered.payload = Bytes::from_static(b"\xFF");
    let bytes = codec::encode_to_vec(&tampered).expect("encode");
    {
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            table
                .insert(2_u64, bytes.as_slice())
                .expect("overwrite seq 2");
        }
        txn.commit().expect("commit overwrite");
    }

    let output = verify_bin(&path);

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.contains("corrupt"), "stdout was: {stdout}");
    assert!(
        stdout.contains("hash mismatch at seq 2"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("quarantine=not-performed"),
        "stdout was: {stdout}",
    );
    assert!(stderr.is_empty(), "stderr was: {stderr}");
}

#[rstest]
fn flipped_entry_byte_reports_hash_mismatch() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0109";
    write_sealed_run(&tmp, run_id);
    let path = run_path(&tmp, run_id);

    flip_stored_entry_payload_byte(&path, 2);

    let verifier = Verifier::open_redb(tmp.path(), INSTANCE_ID, run_id).expect("open verifier");
    let report = verifier.verify().expect("verify");

    assert!(
        report
            .findings
            .iter()
            .any(|finding| matches!(finding, VerifyFinding::HashMismatch { seq: 2 })),
        "findings was: {:?}",
        report.findings,
    );
}

#[cfg(debug_assertions)]
#[rstest]
fn binary_worker_abort_exits_corrupt_without_quarantine() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0105";
    write_sealed_run(&tmp, run_id);

    let output = Command::new(env!("CARGO_BIN_EXE_verify"))
        .env("NAUTILUS_EVENT_STORE_VERIFY_ABORT_WORKER", "1")
        .arg(run_path(&tmp, run_id))
        .output()
        .expect("run verifier binary");

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.contains("corrupt"), "stdout was: {stdout}");
    assert!(stdout.contains("worker_status="), "stdout was: {stdout}");
    assert!(
        stdout.contains("quarantine=not-performed"),
        "stdout was: {stdout}",
    );
    assert!(stderr.is_empty(), "stderr was: {stderr}");
}

#[cfg(debug_assertions)]
#[rstest]
fn binary_worker_timeout_exits_corrupt_without_quarantine() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0106";
    write_sealed_run(&tmp, run_id);

    let output = Command::new(env!("CARGO_BIN_EXE_verify"))
        .env("NAUTILUS_EVENT_STORE_VERIFY_SLEEP_WORKER_MS", "250")
        .env("NAUTILUS_EVENT_STORE_VERIFY_TIMEOUT_SECS", "0")
        .arg(run_path(&tmp, run_id))
        .output()
        .expect("run verifier binary");

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.contains("corrupt"), "stdout was: {stdout}");
    assert!(stdout.contains("timeout after 0s"), "stdout was: {stdout}");
    assert!(
        stdout.contains("quarantine=not-performed"),
        "stdout was: {stdout}",
    );
    assert!(stderr.is_empty(), "stderr was: {stderr}");
}

#[cfg(unix)]
#[rstest]
fn verifier_opens_read_only_run_file() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0103";
    write_sealed_run(&tmp, run_id);

    let path = run_path(&tmp, run_id);
    let original_len = fs::metadata(&path).expect("metadata").len();
    let mut permissions = fs::metadata(&path).expect("metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).expect("set read-only");

    let verifier = Verifier::open_redb_file(&path).expect("open read-only verifier");
    let report = verifier.verify().expect("verify");

    assert!(report.is_clean(), "findings was: {:?}", report.findings);
    assert_eq!(fs::metadata(&path).expect("metadata").len(), original_len);
}

#[rstest]
fn read_only_backend_rejects_open_run_reuse() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0107";
    write_sealed_run(&tmp, run_id);

    let path = run_path(&tmp, run_id);
    let mut backend = RedbBackend::open_sealed_file(path).expect("open sealed file");
    let err = backend
        .open_run(manifest("1700000000-cafe0108"))
        .expect_err("must reject writer reuse");

    assert!(matches!(err, nautilus_event_store::EventStoreError::Closed));
}

#[rstest]
fn truncated_run_file_reports_corrupt_open_error() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0104";
    write_sealed_run(&tmp, run_id);

    let path = run_path(&tmp, run_id);
    let original_len = fs::metadata(&path).expect("metadata").len();
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for truncate");
    file.set_len(original_len / 2).expect("truncate");

    let output = verify_bin(&path);

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.contains("corrupt"), "stdout was: {stdout}");
    assert!(
        stdout.contains("quarantine=not-performed"),
        "stdout was: {stdout}",
    );
}

#[rstest]
fn zero_tail_truncated_run_file_reports_corrupt() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0110";
    write_sealed_run_of(&tmp, run_id, 128);

    let path = run_path(&tmp, run_id);
    zero_tail_truncate(&path);

    let output = verify_bin(&path);

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout={stdout} stderr={stderr}",
    );
    assert!(stdout.contains("corrupt"), "stdout was: {stdout}");
    assert!(
        stdout.contains("quarantine=not-performed"),
        "stdout was: {stdout}",
    );
}

#[rstest]
fn binary_missing_run_file_exits_error() {
    let tmp = TempDir::new().expect("tempdir");
    let path = run_path(&tmp, "missing-run");

    let output = verify_bin(&path);

    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.is_empty(), "stdout was: {stdout}");
    assert!(stderr.contains("error path="), "stderr was: {stderr}");
    assert!(stderr.contains("no run file"), "stderr was: {stderr}");
}

#[rstest]
fn open_redb_rejects_missing_run() {
    let tmp = TempDir::new().expect("tempdir");

    let err = Verifier::open_redb(tmp.path(), INSTANCE_ID, "missing-run").expect_err("must fail");

    let msg = err.to_string();
    assert!(msg.contains("missing-run"), "msg was: {msg}");
}

#[rstest]
fn manufactured_seq_swap_surfaces_as_finding() {
    // Build a sealed run, then overwrite the bytes at table key=2 with the
    // codec-encoded entry whose embedded seq is 99. The hash recomputes
    // correctly because the hash hashes entry.seq=99, so scan_seq returns
    // Ok(Some(entry)) without raising HashMismatch. The verifier must catch
    // the key/embedded-seq divergence.
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0003";
    write_sealed_run(&tmp, run_id);

    let path = tmp.path().join(INSTANCE_ID).join(format!("{run_id}.redb"));
    let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let substitute = build_entry(99, 11);
    let bytes = codec::encode_to_vec(&substitute).expect("encode");
    {
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            table
                .insert(2_u64, bytes.as_slice())
                .expect("overwrite seq 2");
        }
        txn.commit().expect("commit overwrite");
    }

    let verifier = Verifier::open_redb(tmp.path(), INSTANCE_ID, run_id).expect("open verifier");
    let report = verifier.verify().expect("verify");

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
fn corrupt_snapshot_anchor_is_a_finding_not_clean() {
    // The restore path reads the anchor before tail replay; a run whose anchor fails
    // to decode must not verify clean and then fail at restore time.
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0110";
    write_sealed_run(&tmp, run_id);

    let path = run_path(&tmp, run_id);
    let snapshot_anchor: redb::TableDefinition<&str, &[u8]> =
        redb::TableDefinition::new("snapshot_anchor");
    {
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

    let verifier = Verifier::open_redb_file(&path).expect("open verifier");
    let report = verifier.verify().expect("verify");

    assert!(!report.is_clean(), "findings was: {:?}", report.findings);
    assert!(
        report.findings.iter().any(|f| matches!(
            f,
            VerifyFinding::SnapshotAnchorInvalid { reason } if reason.contains("unreadable")
        )),
        "findings was: {:?}",
        report.findings,
    );
}

#[rstest]
fn snapshot_anchor_past_durable_watermark_is_a_finding() {
    // A tail-trimmed run whose anchor points past the durable watermark cannot
    // restore (the reader rejects the plan); the verifier must surface it.
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0111";
    {
        let mut backend = RedbBackend::new(tmp.path());
        backend.open_run(manifest(run_id)).expect("open run");
        backend
            .append_batch(&[
                append_with(1, 10, Vec::new()),
                append_with(2, 11, Vec::new()),
            ])
            .expect("append");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(2, "cache://snapshots/2", "blake3:abc"))
            .expect("record anchor");
        backend.seal(RunStatus::Ended).expect("seal");
    }

    let path = run_path(&tmp, run_id);
    let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    {
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            table.remove(2_u64).expect("remove seq 2");
        }
        txn.commit().expect("commit removal");
    }

    let verifier = Verifier::open_redb_file(&path).expect("open verifier");
    let report = verifier.verify().expect("verify");

    assert!(
        report.findings.iter().any(|f| matches!(
            f,
            VerifyFinding::SnapshotAnchorInvalid { reason } if reason.contains("exceeds durable")
        )),
        "findings was: {:?}",
        report.findings,
    );
}

#[rstest]
fn manufactured_multi_gap_surfaces_two_findings() {
    // Build a 5-entry sealed run, then drop seqs 2 AND 4 from the entries table.
    // The verifier must walk to hwm=5 (last() still returns 5) and emit two
    // distinct GapRange findings, one per contiguous hole. Catches a regression
    // that lets a single gap_cursor span across the surviving seq=3.
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0004";
    write_sealed_run_of(&tmp, run_id, 5);

    let path = tmp.path().join(INSTANCE_ID).join(format!("{run_id}.redb"));
    let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    {
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            table.remove(2_u64).expect("remove seq 2");
            table.remove(4_u64).expect("remove seq 4");
        }
        txn.commit().expect("commit removal");
    }

    let verifier = Verifier::open_redb(tmp.path(), INSTANCE_ID, run_id).expect("open verifier");
    let report = verifier.verify().expect("verify");

    let gaps: Vec<GapRange> = report
        .findings
        .iter()
        .filter_map(|f| match f {
            VerifyFinding::Gap { range } => Some(*range),
            _ => None,
        })
        .collect();
    assert_eq!(
        gaps,
        vec![GapRange { from: 2, to: 2 }, GapRange { from: 4, to: 4 },],
    );
    assert_eq!(report.entries_scanned, 3);
    assert_eq!(report.high_watermark, 5);
}

#[rstest]
fn manufactured_gap_surfaces_as_finding() {
    // Build a sealed run, then drop seq=2 directly out of the entries table to
    // simulate the corruption class the SPEC describes: a run that opens cleanly
    // but is missing a row inside the high-watermark. The verifier must record a
    // Gap finding and continue past it instead of aborting on the first hit.
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0002";
    write_sealed_run(&tmp, run_id);

    let path = tmp.path().join(INSTANCE_ID).join(format!("{run_id}.redb"));
    let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    {
        let db = redb::Database::create(&path).expect("open redb");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(entries).expect("open table");
            table.remove(2_u64).expect("remove seq 2");
        }
        txn.commit().expect("commit removal");
    }

    let verifier = Verifier::open_redb(tmp.path(), INSTANCE_ID, run_id).expect("open verifier");
    let report = verifier.verify().expect("verify");

    let gaps: Vec<GapRange> = report
        .findings
        .iter()
        .filter_map(|f| match f {
            VerifyFinding::Gap { range } => Some(*range),
            _ => None,
        })
        .collect();
    assert_eq!(gaps, vec![GapRange { from: 2, to: 2 }]);
    // Verifier still scanned the surviving rows and surfaced the dangling index
    // pointing at the removed seq.
    assert_eq!(report.entries_scanned, 2);
    assert!(
        report.findings.iter().any(|f| matches!(
            f,
            VerifyFinding::IndexDrift {
                kind: IndexKind::ClientOrderId,
                drift: nautilus_event_store::IndexDrift::DanglingTarget { stored_seq: 2 },
                ..
            }
        )),
        "findings was: {:?}",
        report.findings,
    );
}
