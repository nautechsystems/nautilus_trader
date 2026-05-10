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

use bincode::config::standard;
use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use nautilus_event_store::{
    AppendEntry, EventStore, EventStoreEntry, GapRange, Headers, IndexKey, IndexKind, RedbBackend,
    RegisteredComponents, RunManifest, RunStatus, Topic, Verifier, VerifyFinding,
    compute_entry_hash,
};
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
fn open_redb_rejects_missing_run() {
    let tmp = TempDir::new().expect("tempdir");

    let err = Verifier::open_redb(tmp.path(), INSTANCE_ID, "missing-run").expect_err("must fail");

    let msg = err.to_string();
    assert!(msg.contains("missing-run"), "msg was: {msg}");
}

#[rstest]
fn manufactured_seq_swap_surfaces_as_finding() {
    // Build a sealed run, then overwrite the bytes at table key=2 with the
    // bincode-encoded entry whose embedded seq is 99. The hash recomputes
    // correctly because the hash hashes entry.seq=99, so scan_seq returns
    // Ok(Some(entry)) without raising HashMismatch. The verifier must catch
    // the key/embedded-seq divergence.
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "1700000000-cafe0003";
    write_sealed_run(&tmp, run_id);

    let path = tmp.path().join(INSTANCE_ID).join(format!("{run_id}.redb"));
    let entries: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let substitute = build_entry(99, 11);
    let bytes = bincode::serde::encode_to_vec(&substitute, standard()).expect("encode");
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
