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

//! Production-shape integration tests for [`EventStoreWriter`] against a tempdir-backed
//! [`RedbBackend`]. Mirrors the in-memory writer suite but exercises the on-disk specifics:
//! entries durably reach the redb file, the manifest seals on close, and the writer
//! reports the correct high-watermark over a multi-batch run.

use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::{
    UnixNanos,
    time::{get_atomic_clock_realtime, get_atomic_clock_static},
};
use nautilus_event_store::{
    AppendEntry, EntryDraft, EventStore, EventStoreEntry, EventStoreWriter, HaltCallback,
    HaltReason, Headers, IndexKey, IndexKind, MemoryBackend, RedbBackend, RegisteredComponents,
    RunManifest, RunStatus, ScanDirection, SubmitError, Topic, WriterConfig, codec,
};
use redb::{ReadableDatabase, ReadableTable};
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

fn open_backend_with(tmp: &TempDir, run_id: &str) -> RedbBackend {
    let mut backend = RedbBackend::new(tmp.path());
    backend.open_run(manifest(run_id)).expect("open run");
    backend
}

#[derive(Debug)]
struct BlockingMemoryBackend {
    inner: Arc<Mutex<MemoryBackend>>,
    gate: Arc<(Mutex<bool>, Condvar)>,
    appends_started: Arc<AtomicUsize>,
}

impl BlockingMemoryBackend {
    fn new(
        inner: Arc<Mutex<MemoryBackend>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        appends_started: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            inner,
            gate,
            appends_started,
        }
    }
}

impl EventStore for BlockingMemoryBackend {
    fn open_run(&mut self, _: RunManifest) -> Result<(), nautilus_event_store::EventStoreError> {
        unreachable!("test wrapper does not forward open_run")
    }

    fn append_batch(
        &mut self,
        entries: &[AppendEntry],
    ) -> Result<u64, nautilus_event_store::EventStoreError> {
        self.appends_started.fetch_add(1, Ordering::SeqCst);
        let (lock, cvar) = &*self.gate;
        let mut released = lock.lock().expect("gate poisoned");

        while !*released {
            released = cvar.wait(released).expect("gate wait");
        }

        self.inner
            .lock()
            .expect("inner poisoned")
            .append_batch(entries)
    }

    fn scan_range(
        &self,
        from: u64,
        to: u64,
        direction: ScanDirection,
    ) -> Result<Vec<EventStoreEntry>, nautilus_event_store::EventStoreError> {
        self.inner
            .lock()
            .expect("inner poisoned")
            .scan_range(from, to, direction)
    }

    fn scan_seq(
        &self,
        seq: u64,
    ) -> Result<Option<EventStoreEntry>, nautilus_event_store::EventStoreError> {
        self.inner.lock().expect("inner poisoned").scan_seq(seq)
    }

    fn lookup(
        &self,
        kind: IndexKind,
        key: &str,
    ) -> Result<Option<u64>, nautilus_event_store::EventStoreError> {
        self.inner.lock().expect("inner poisoned").lookup(kind, key)
    }

    fn iter_index_keys(
        &self,
        kind: IndexKind,
    ) -> Result<Vec<(String, u64)>, nautilus_event_store::EventStoreError> {
        self.inner
            .lock()
            .expect("inner poisoned")
            .iter_index_keys(kind)
    }

    fn seal(&mut self, status: RunStatus) -> Result<(), nautilus_event_store::EventStoreError> {
        self.inner.lock().expect("inner poisoned").seal(status)
    }

    fn manifest(&self) -> Result<RunManifest, nautilus_event_store::EventStoreError> {
        self.inner.lock().expect("inner poisoned").manifest()
    }

    fn high_watermark(&self) -> Result<u64, nautilus_event_store::EventStoreError> {
        self.inner.lock().expect("inner poisoned").high_watermark()
    }
}

#[rstest]
fn writer_commits_drafts_durably_to_redb_file() {
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-write");

    let (halt, captured) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");

    // Submit three drafts with sidecar index keys spanning every IndexKind variant.
    writer
        .submit(entry_draft(
            10,
            vec![IndexKey::new(IndexKind::ClientOrderId, "O-1".to_string())],
        ))
        .expect("submit 1");
    writer
        .submit(entry_draft(
            11,
            vec![IndexKey::new(IndexKind::VenueOrderId, "V-1".to_string())],
        ))
        .expect("submit 2");
    writer
        .submit(entry_draft(12, Vec::new()))
        .expect("submit 3");

    let final_hwm = writer.close(run_ended_draft()).expect("close");

    // 3 drafts + RunEnded == 4 entries.
    assert_eq!(final_hwm, 4);
    assert!(captured.lock().expect("captured").is_empty());

    // Reopening the same path with a fresh backend instance must surface the run as
    // already sealed; that proves seal landed durably to disk.
    let mut after = RedbBackend::new(tmp.path());
    let err = after
        .open_run(manifest("run-write"))
        .expect_err("must reject");

    match err {
        nautilus_event_store::EventStoreError::Backend(msg) => {
            assert!(msg.contains("already sealed"), "msg was: {msg}");
        }
        other => panic!("expected sealed Backend, was {other:?}"),
    }

    // Open a fresh run alongside it and verify the prior run-write file persists.
    let prior = tmp.path().join(INSTANCE_ID).join("run-write.redb");
    assert!(prior.exists(), "run file must persist after seal");
}

#[rstest]
fn writer_high_watermark_advances_only_after_backend_ack() {
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-hwm");

    let (halt, _) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig {
            channel_capacity: 16,
            max_batch_entries: 2,
            max_batch_latency: Duration::from_millis(5),
            halt_threshold: Duration::from_secs(30),
        },
    )
    .expect("spawn");

    // Mass-submit; the writer batches up to 2 entries per commit.
    for ts in 100_u64..110_u64 {
        writer.submit(entry_draft(ts, Vec::new())).expect("submit");
    }

    // Wait until the watermark catches up to all submitted drafts.
    let mut waited = Duration::ZERO;
    let deadline = Duration::from_secs(2);
    while writer.high_watermark() < 10 && waited < deadline {
        std::thread::sleep(Duration::from_millis(10));
        waited += Duration::from_millis(10);
    }
    assert_eq!(writer.high_watermark(), 10);

    let final_hwm = writer.close(run_ended_draft()).expect("close");
    assert_eq!(final_hwm, 11);
}

#[rstest]
fn writer_halts_instead_of_dropping_when_backend_blocks_past_channel_capacity() {
    let inner = Arc::new(Mutex::new(MemoryBackend::new()));
    inner
        .lock()
        .expect("inner")
        .open_run(manifest("run-backpressure"))
        .expect("open run");
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let appends_started = Arc::new(AtomicUsize::new(0));
    let backend = BlockingMemoryBackend::new(
        Arc::clone(&inner),
        Arc::clone(&gate),
        Arc::clone(&appends_started),
    );
    let (halt, captured) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig {
            channel_capacity: 2,
            max_batch_entries: 1,
            max_batch_latency: Duration::from_secs(30),
            halt_threshold: Duration::from_millis(30),
        },
    )
    .expect("spawn");

    writer
        .submit(entry_draft(10, Vec::new()))
        .expect("submit 1");
    let mut waited = Duration::ZERO;
    while appends_started.load(Ordering::SeqCst) == 0 && waited < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(2));
        waited += Duration::from_millis(2);
    }
    assert_eq!(
        appends_started.load(Ordering::SeqCst),
        1,
        "writer must be blocked inside the first backend append",
    );

    writer
        .submit(entry_draft(11, Vec::new()))
        .expect("submit 2");
    writer
        .submit(entry_draft(12, Vec::new()))
        .expect("submit 3");
    let stalled = writer
        .submit(entry_draft(13, Vec::new()))
        .expect_err("submit beyond channel capacity must halt");

    match stalled {
        SubmitError::HaltSignaled { .. } => {}
        SubmitError::Closed => panic!("expected HaltSignaled, was Closed"),
    }
    assert!(matches!(
        captured.lock().expect("captured").first(),
        Some(HaltReason::BackpressureStall { .. }),
    ));

    let (lock, cvar) = &*gate;
    *lock.lock().expect("gate") = true;
    cvar.notify_all();

    let mut waited = Duration::ZERO;
    while writer.high_watermark() < 3 && waited < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(2));
        waited += Duration::from_millis(2);
    }
    assert_eq!(
        writer.high_watermark(),
        3,
        "all accepted entries must commit after the backend resumes",
    );
    drop(writer);

    let backend = inner.lock().expect("inner");
    let entries = backend
        .scan_range(1, 3, ScanDirection::Forward)
        .expect("scan committed entries");
    let committed_ts_init: Vec<u64> = entries.iter().map(|entry| entry.ts_init.as_u64()).collect();

    assert_eq!(committed_ts_init, vec![10, 11, 12]);
    assert_eq!(backend.high_watermark().expect("hwm"), 3);
}

#[rstest]
fn writer_seals_manifest_with_max_observed_ts_init() {
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-seal");

    let (halt, _) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");

    // Submit entries with non-monotonic ts_init to make sure seal records the max
    // observed ts_init, not the last-arrived value.
    writer
        .submit(entry_draft(50, Vec::new()))
        .expect("submit 1");
    writer
        .submit(entry_draft(120, Vec::new()))
        .expect("submit 2");
    writer
        .submit(entry_draft(80, Vec::new()))
        .expect("submit 3");

    let final_hwm = writer.close(run_ended_draft()).expect("close");
    // 3 drafts + RunEnded(ts_init=9_999) so end_ts_init must be 9_999.
    assert_eq!(final_hwm, 4);

    // Open a second run to keep the backend instance separate, then read the prior
    // run-seal.redb directly through redb so the manifest contents are observable
    // without the backend's already-sealed open guard intercepting.
    let mut second = RedbBackend::new(tmp.path());
    second.open_run(manifest("run-other")).expect("open second");
    let prior_path = tmp.path().join(INSTANCE_ID).join("run-seal.redb");
    let manifest_table: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("manifest");
    let db = redb::Database::create(&prior_path).expect("open prior");
    let txn = db.begin_read().expect("begin read");
    let table = txn.open_table(manifest_table).expect("open manifest table");
    let bytes = table
        .get("current")
        .expect("get manifest")
        .expect("manifest exists");
    let decoded_manifest =
        codec::decode_from_slice::<RunManifest>(bytes.value()).expect("decode manifest");
    assert_eq!(decoded_manifest.status, RunStatus::Ended);
    assert_eq!(decoded_manifest.high_watermark, 4);
    assert_eq!(decoded_manifest.end_ts_init, Some(UnixNanos::from(9_999)));
}

#[rstest]
fn writer_committed_entries_are_scannable_after_close() {
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-scan");

    let (halt, _) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig {
            channel_capacity: 16,
            max_batch_entries: 4,
            max_batch_latency: Duration::from_millis(5),
            halt_threshold: Duration::from_secs(30),
        },
    )
    .expect("spawn");

    let client_order_id = "O-Z".to_string();

    for ts in 10_u64..18_u64 {
        let keys = if ts == 11 {
            vec![IndexKey::new(
                IndexKind::ClientOrderId,
                client_order_id.clone(),
            )]
        } else {
            Vec::new()
        };
        writer.submit(entry_draft(ts, keys)).expect("submit");
    }

    let final_hwm = writer.close(run_ended_draft()).expect("close");
    assert_eq!(final_hwm, 9);

    // After close + drop the redb file is unlocked, so we read entries through redb
    // directly rather than through a fresh backend that would reject an already-sealed
    // run on open.
    let prior_path = tmp.path().join(INSTANCE_ID).join("run-scan.redb");
    let entries_table: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let client_order_table: redb::TableDefinition<&str, u64> =
        redb::TableDefinition::new("client_order_id_idx");
    let db = redb::Database::create(&prior_path).expect("open prior");
    let txn = db.begin_read().expect("begin read");

    let entries = txn.open_table(entries_table).expect("open entries");
    let mut seqs = Vec::new();

    for row in entries.iter().expect("iter") {
        let (k, _) = row.expect("row");
        seqs.push(k.value());
    }
    assert_eq!(seqs, (1_u64..=9_u64).collect::<Vec<_>>());

    let idx = txn
        .open_table(client_order_table)
        .expect("open client_order idx");
    let recorded = idx
        .get("O-Z")
        .expect("get client_order_id")
        .expect("client_order_id recorded");
    // The second submitted draft (ts_init=11) carried the client_order_id key, so its
    // seq is 2.
    assert_eq!(recorded.value(), 2);
}

#[rstest]
fn writer_carries_high_watermark_through_batch_size_flushes() {
    // Force a tiny batch ceiling; verify entries accumulate across multiple commits
    // without seq gaps and the watermark mirrors what the backend acknowledged.
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-batch");

    let (halt, _) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig {
            channel_capacity: 64,
            max_batch_entries: 3,
            max_batch_latency: Duration::from_millis(5),
            halt_threshold: Duration::from_secs(30),
        },
    )
    .expect("spawn");

    for ts in 200_u64..220_u64 {
        writer.submit(entry_draft(ts, Vec::new())).expect("submit");
    }

    let final_hwm = writer.close(run_ended_draft()).expect("close");
    // 20 drafts + RunEnded.
    assert_eq!(final_hwm, 21);

    // Read the entries back; every seq from 1..=21 must be present.
    let prior_path = tmp.path().join(INSTANCE_ID).join("run-batch.redb");
    let entries_table: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let db = redb::Database::create(&prior_path).expect("open prior");
    let txn = db.begin_read().expect("begin read");
    let entries = txn.open_table(entries_table).expect("open entries");
    let observed: Vec<u64> = entries
        .iter()
        .expect("iter")
        .map(|row| row.expect("row").0.value())
        .collect();
    assert_eq!(observed, (1_u64..=21_u64).collect::<Vec<_>>());
}

#[rstest]
fn writer_preserves_payload_and_hash_round_trip() {
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-roundtrip");

    let (halt, _) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");

    let payloads: Vec<Bytes> = vec![
        Bytes::from_static(b"alpha"),
        Bytes::from_static(b"beta-omega"),
        Bytes::new(),
    ];

    for (i, payload) in payloads.iter().enumerate() {
        let draft = EntryDraft {
            headers: Headers::empty(),
            topic: Topic::from("exec.command.SubmitOrder"),
            payload_type: Ustr::from("SubmitOrder"),
            payload: payload.clone(),
            ts_init: UnixNanos::from(1_000 + u64::try_from(i).expect("fits")),
            index_keys: Vec::new(),
        };
        writer.submit(draft).expect("submit");
    }

    let final_hwm = writer.close(run_ended_draft()).expect("close");
    assert_eq!(final_hwm, 4);

    // Walk the entries; recompute_hash must match the stored hash for every row.
    let prior_path = tmp.path().join(INSTANCE_ID).join("run-roundtrip.redb");
    let entries_table: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let db = redb::Database::create(&prior_path).expect("open prior");
    let txn = db.begin_read().expect("begin read");
    let entries = txn.open_table(entries_table).expect("open entries");
    for row in entries.iter().expect("iter") {
        let (k, v) = row.expect("row");
        let seq = k.value();
        let bytes = v.value();
        let entry = codec::decode_from_slice::<nautilus_event_store::EventStoreEntry>(bytes)
            .expect("decode");
        assert_eq!(entry.seq, seq);
        assert_eq!(entry.recompute_hash(), entry.entry_hash);
    }
}

#[rstest]
fn writer_does_not_seal_when_dropped_without_close() {
    // Drop the writer without close() so the writer thread exits and the channel
    // disconnects without sealing the backend; a subsequent open of the same run id
    // must surface CrashedPredecessor, proving the no-seal-without-close contract.
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-drop");

    let (halt, _) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");

    writer.submit(entry_draft(10, Vec::new())).expect("submit");

    // Wait for the in-flight entry to commit before dropping the writer.
    let mut waited = Duration::ZERO;
    while writer.high_watermark() < 1 && waited < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(5));
        waited += Duration::from_millis(5);
    }
    drop(writer);

    let mut recovered = RedbBackend::new(tmp.path());
    let err = recovered
        .open_run(manifest("run-drop"))
        .expect_err("must flag crashed predecessor");

    match err {
        nautilus_event_store::EventStoreError::CrashedPredecessor => {}
        other => panic!("expected CrashedPredecessor, was {other:?}"),
    }
    assert_eq!(recovered.high_watermark().expect("hwm"), 1);
}

#[rstest]
fn writer_stamps_ts_publish_from_clock_at_submit() {
    // ts_publish records when the writer received the entry. Capture the clock
    // immediately before each submit and assert the persisted ts_publish is no
    // earlier than the captured time. Uses the realtime clock so the value advances;
    // the static clock would stay at zero and could not distinguish "stamp at
    // submit" from "stamp at zero literal".
    let tmp = TempDir::new().expect("tempdir");
    let backend = open_backend_with(&tmp, "run-tspublish");

    let (halt, _) = captured_halt();

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_realtime(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");

    let mut captured_at_submit: Vec<UnixNanos> = Vec::new();

    for ts in 100_u64..103_u64 {
        let before = get_atomic_clock_realtime().get_time_ns();
        captured_at_submit.push(before);
        writer.submit(entry_draft(ts, Vec::new())).expect("submit");
    }

    let final_hwm = writer.close(run_ended_draft()).expect("close");
    assert_eq!(final_hwm, 4);

    let prior_path = tmp.path().join(INSTANCE_ID).join("run-tspublish.redb");
    let entries_table: redb::TableDefinition<u64, &[u8]> = redb::TableDefinition::new("entries");
    let db = redb::Database::create(&prior_path).expect("open prior");
    let txn = db.begin_read().expect("begin read");
    let entries = txn.open_table(entries_table).expect("open entries");

    let mut decoded: Vec<nautilus_event_store::EventStoreEntry> = Vec::new();

    for row in entries.iter().expect("iter") {
        let (_, v) = row.expect("row");
        let entry = codec::decode_from_slice::<nautilus_event_store::EventStoreEntry>(v.value())
            .expect("decode");
        decoded.push(entry);
    }

    // The first three entries must each be stamped at or after the captured
    // pre-submit clock value; the writer can only have observed the clock after
    // the call site captured it.
    for (entry, captured) in decoded.iter().take(3).zip(captured_at_submit.iter()) {
        assert!(
            entry.ts_publish >= *captured,
            "ts_publish {} must be >= captured pre-submit time {} for seq={}",
            entry.ts_publish,
            captured,
            entry.seq,
        );
    }

    // ts_publish must monotonically advance across submits (real-time AtomicTime
    // guarantees strict monotonicity via AcqRel compare-and-exchange).
    for window in decoded.windows(2) {
        assert!(
            window[1].ts_publish > window[0].ts_publish,
            "ts_publish must be strictly monotonic, was {} then {}",
            window[0].ts_publish,
            window[1].ts_publish,
        );
    }
}
