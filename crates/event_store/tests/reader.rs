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

//! Production-shape integration tests for [`EventStoreReader`] over a tempdir-backed
//! [`RedbBackend`]. Mirrors `tests/writer.rs` in shape: each test seeds a sealed run via
//! the writer's close path, then opens it through [`RedbBackend::open_sealed`] and
//! exercises the reader API against the on-disk file.

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, time::get_atomic_clock_static};
use nautilus_event_store::{
    AppendEntry, EntryDraft, EventStore, EventStoreEntry, EventStoreError, EventStoreReader,
    EventStoreWriter, HaltCallback, HaltReason, Headers, IndexKey, IndexKind, RedbBackend,
    RegisteredComponents, RunManifest, RunStatus, ScanDirection, Topic, WriterConfig,
    compute_entry_hash,
};
use rstest::rstest;
use tempfile::TempDir;
use ustr::Ustr;

const INSTANCE_ID: &str = "trader-001";

fn manifest_with(run_id: &str, start_ts_init: u64) -> RunManifest {
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
        start_ts_init: UnixNanos::from(start_ts_init),
        end_ts_init: None,
        high_watermark: 0,
        status: RunStatus::Running,
    }
}

fn entry_draft(ts_init: u64, index_keys: Vec<IndexKey>) -> EntryDraft {
    EntryDraft {
        headers: Headers::empty(),
        topic: Topic::from("exec.command.SubmitOrder"),
        payload_type: Ustr::from("SubmitOrder"),
        payload: Bytes::from_static(b"\x01\x02\x03\x04"),
        ts_init: UnixNanos::from(ts_init),
        index_keys,
    }
}

fn run_ended_draft() -> EntryDraft {
    EntryDraft {
        headers: Headers::empty(),
        topic: Topic::from("run.lifecycle.RunEnded"),
        payload_type: Ustr::from("RunEnded"),
        payload: Bytes::new(),
        ts_init: UnixNanos::from(9_999),
        index_keys: Vec::new(),
    }
}

fn captured_halt() -> (HaltCallback, Arc<Mutex<Vec<HaltReason>>>) {
    let captured: Arc<Mutex<Vec<HaltReason>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_for_cb = Arc::clone(&captured);
    let halt: HaltCallback = Arc::new(move |reason| {
        captured_for_cb
            .lock()
            .expect("captured halt poisoned")
            .push(reason);
    });
    (halt, captured)
}

/// Writes `count` drafts plus a `RunEnded` entry through the writer, then drops it
/// so the redb file is unlocked for the reader. Returns the durable high-watermark.
fn seed_sealed_run(
    tmp: &TempDir,
    run_id: &str,
    start_ts_init: u64,
    drafts: Vec<EntryDraft>,
) -> u64 {
    let mut backend = RedbBackend::new(tmp.path());
    backend
        .open_run(manifest_with(run_id, start_ts_init))
        .expect("open run");

    let (halt, captured) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");

    for draft in drafts {
        writer.submit(draft).expect("submit");
    }

    let final_hwm = writer.close(run_ended_draft()).expect("close");
    assert!(captured.lock().expect("captured").is_empty());
    final_hwm
}

#[rstest]
fn reader_reads_sealed_run_through_redb_backend() {
    let tmp = TempDir::new().expect("tempdir");
    let drafts: Vec<EntryDraft> = (10_u64..15_u64)
        .map(|ts| entry_draft(ts, Vec::new()))
        .collect();
    let final_hwm = seed_sealed_run(&tmp, "run-read", 100, drafts);
    // 5 drafts + RunEnded.
    assert_eq!(final_hwm, 6);

    let backend = RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "run-read").expect("open");
    let reader = EventStoreReader::new(backend);

    assert_eq!(reader.high_watermark().expect("hwm"), 6);
    let m = reader.manifest().expect("manifest");
    assert_eq!(m.run_id, "run-read");
    assert_eq!(m.status, RunStatus::Ended);
    assert_eq!(m.high_watermark, 6);

    let scanned: Vec<u64> = reader
        .scan_range(1, final_hwm, ScanDirection::Forward)
        .map(|r| r.expect("entry").seq)
        .collect();
    assert_eq!(scanned, (1_u64..=6).collect::<Vec<_>>());

    let last = reader.scan_seq(6).expect("scan").expect("present");
    assert_eq!(last.payload_type.as_str(), "RunEnded");
}

#[rstest]
fn reader_chunked_iteration_yields_every_seq_over_disk_file() {
    // Force the backend to handle multiple round-trips; the chunked iterator must
    // stitch them into a contiguous sequence with no missing seqs at the boundary.
    let tmp = TempDir::new().expect("tempdir");
    let drafts: Vec<EntryDraft> = (200_u64..212_u64)
        .map(|ts| entry_draft(ts, Vec::new()))
        .collect();
    let final_hwm = seed_sealed_run(&tmp, "run-chunked", 200, drafts);
    // 12 drafts + RunEnded.
    assert_eq!(final_hwm, 13);

    let backend = RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "run-chunked").expect("open");
    let reader = EventStoreReader::new(backend);

    let forward: Vec<u64> = reader
        .scan_range_chunked(1, final_hwm, ScanDirection::Forward, 4)
        .map(|r| r.expect("entry").seq)
        .collect();
    assert_eq!(forward, (1_u64..=13).collect::<Vec<_>>());

    let reverse: Vec<u64> = reader
        .scan_range_chunked(1, final_hwm, ScanDirection::Reverse, 5)
        .map(|r| r.expect("entry").seq)
        .collect();
    assert_eq!(reverse, (1_u64..=13).rev().collect::<Vec<_>>());
}

#[rstest]
fn reader_lookup_returns_seq_recorded_through_writer() {
    let tmp = TempDir::new().expect("tempdir");
    let intent = "intent-Z".to_string();
    let cl_ord = "O-7".to_string();
    let venue = "V-7".to_string();

    let drafts = vec![
        entry_draft(10, Vec::new()),
        entry_draft(11, vec![IndexKey::new(IndexKind::IntentId, intent.clone())]),
        entry_draft(
            12,
            vec![IndexKey::new(IndexKind::ClientOrderId, cl_ord.clone())],
        ),
        entry_draft(
            13,
            vec![IndexKey::new(IndexKind::VenueOrderId, venue.clone())],
        ),
    ];
    let final_hwm = seed_sealed_run(&tmp, "run-lookup", 300, drafts);
    // 4 drafts + RunEnded.
    assert_eq!(final_hwm, 5);

    let backend = RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "run-lookup").expect("open");
    let reader = EventStoreReader::new(backend);

    assert_eq!(
        reader.lookup(IndexKind::IntentId, &intent).expect("lookup"),
        Some(2),
    );
    assert_eq!(
        reader
            .lookup(IndexKind::ClientOrderId, &cl_ord)
            .expect("lookup"),
        Some(3),
    );
    assert_eq!(
        reader
            .lookup(IndexKind::VenueOrderId, &venue)
            .expect("lookup"),
        Some(4),
    );
    assert!(
        reader
            .lookup(IndexKind::IntentId, "missing")
            .expect("lookup")
            .is_none(),
    );
}

#[rstest]
fn open_sealed_rejects_running_file() {
    // A run file whose manifest is still `Running` is the crash-recovery domain;
    // open_sealed must refuse it so the reader path does not silently bypass the
    // crashed-predecessor seal.
    let tmp = TempDir::new().expect("tempdir");
    let mut backend = RedbBackend::new(tmp.path());
    backend
        .open_run(manifest_with("run-running", 400))
        .expect("open run");
    drop(backend);

    let err = RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "run-running")
        .expect_err("must reject running");

    match err {
        EventStoreError::Backend(msg) => {
            assert!(msg.contains("not sealed"), "msg was: {msg}");
        }
        other => panic!("expected Backend, was {other:?}"),
    }
}

#[rstest]
fn open_sealed_rejects_missing_file() {
    let tmp = TempDir::new().expect("tempdir");
    let err =
        RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "nope").expect_err("must reject missing");

    match err {
        EventStoreError::Backend(msg) => {
            assert!(msg.contains("no run file"), "msg was: {msg}");
        }
        other => panic!("expected Backend, was {other:?}"),
    }
}

#[rstest]
fn open_sealed_appends_return_closed() {
    let tmp = TempDir::new().expect("tempdir");
    let drafts = vec![entry_draft(10, Vec::new())];
    let _ = seed_sealed_run(&tmp, "run-seal-closed", 500, drafts);

    let mut backend =
        RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "run-seal-closed").expect("open sealed");

    // Construct a synthetic AppendEntry just to exercise the trait surface; the seal
    // guard must intercept before any seq validation runs.
    let topic: Topic = "exec.command.SubmitOrder".into();
    let payload_type = Ustr::from("SubmitOrder");
    let payload = Bytes::from_static(b"\x01");
    let headers = Headers::empty();
    let ts_init = UnixNanos::from(1_000);
    let ts_publish = UnixNanos::from(1_001);
    let hash = compute_entry_hash(
        99,
        ts_init,
        ts_publish,
        topic.as_ref(),
        payload_type.as_str(),
        &payload,
        &headers,
    );
    let entry = EventStoreEntry::new(
        hash,
        99,
        headers,
        topic,
        payload_type,
        payload,
        ts_init,
        ts_publish,
    );
    let err = backend
        .append_batch(&[AppendEntry::without_indices(entry)])
        .expect_err("must reject closed");

    assert!(matches!(err, EventStoreError::Closed));
}

#[rstest]
fn list_runs_returns_manifests_sorted_by_start_ts() {
    // Seed three sealed runs with non-monotonic start_ts_init; list_runs must walk the
    // directory, decode every manifest, and sort by start_ts_init so chronologically
    // ordered consumers (forensics navigation, retention sweeps) see a stable order.
    let tmp = TempDir::new().expect("tempdir");
    let _ = seed_sealed_run(&tmp, "run-third", 300, vec![entry_draft(310, Vec::new())]);
    let _ = seed_sealed_run(&tmp, "run-first", 100, vec![entry_draft(110, Vec::new())]);
    let _ = seed_sealed_run(&tmp, "run-second", 200, vec![entry_draft(210, Vec::new())]);

    let manifests = RedbBackend::list_runs(tmp.path(), INSTANCE_ID).expect("list runs");
    let ids: Vec<&str> = manifests.iter().map(|m| m.run_id.as_str()).collect();

    assert_eq!(ids, vec!["run-first", "run-second", "run-third"]);
    assert!(
        manifests
            .iter()
            .all(|m| matches!(m.status, RunStatus::Ended)),
        "every listed run must be sealed",
    );
}

#[rstest]
fn list_runs_returns_empty_when_directory_missing() {
    let tmp = TempDir::new().expect("tempdir");
    let manifests = RedbBackend::list_runs(tmp.path(), "no-such-instance").expect("list runs");

    assert!(manifests.is_empty(), "manifests was: {manifests:?}");
}

#[rstest]
fn reader_round_trip_preserves_payload_and_hash() {
    // Walk every entry through the reader and re-validate: the writer-stamped
    // entry_hash must still match a fresh recompute after the round-trip through
    // bincode + redb + reader iteration.
    let tmp = TempDir::new().expect("tempdir");
    let drafts: Vec<EntryDraft> = (10_u64..14_u64)
        .map(|ts| EntryDraft {
            headers: Headers::empty(),
            topic: Topic::from("exec.command.SubmitOrder"),
            payload_type: Ustr::from("SubmitOrder"),
            payload: Bytes::from(format!("payload-{ts}")),
            ts_init: UnixNanos::from(ts),
            index_keys: Vec::new(),
        })
        .collect();
    let final_hwm = seed_sealed_run(&tmp, "run-roundtrip", 600, drafts);
    assert_eq!(final_hwm, 5);

    let backend =
        RedbBackend::open_sealed(tmp.path(), INSTANCE_ID, "run-roundtrip").expect("open sealed");
    let reader = EventStoreReader::new(backend);

    for item in reader.scan_range(1, final_hwm, ScanDirection::Forward) {
        let entry = item.expect("entry");
        assert_eq!(entry.recompute_hash(), entry.entry_hash);
    }
}
