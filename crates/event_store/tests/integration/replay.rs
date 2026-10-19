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

//! Canonical replay capability tests over a tempdir-backed redb run.

#[cfg(feature = "persistence")]
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, time::get_atomic_clock_static};
#[cfg(feature = "persistence")]
use nautilus_event_store::{
    CatalogReplayData, CatalogReplayRecord, CatalogSliceCoverage, CatalogSlicePlan,
    CatalogSliceQuery, DataClass, DataMarkerCapture, DataMarkerClass, DataMarkerConfig,
    DataMarkerExtractorRegistry, MarkerBackend, MarkerManifest, MarkerReader, MarkerWriter,
    MarkerWriterConfig, RedbMarkerBackend, ReplayCatalog, join_at_entry,
};
use nautilus_event_store::{
    EntryDraft, EventStore, EventStoreWriter, Headers, RedbBackend, RegisteredComponents,
    ReplaySeqRange, ReplayTimeRange, RunManifest, RunStatus, Topic, WriterConfig,
    load_forensics_replay_inputs, noop_halt, open_event_store_replay_source,
    plan_forensics_replay_inputs,
};
#[cfg(feature = "persistence")]
use nautilus_model::{data::QuoteTick, data::stubs::quote_ethusdt_binance};
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

fn entry_draft(
    topic: &'static str,
    payload_type: &'static str,
    payload: &'static [u8],
    ts_init: u64,
) -> EntryDraft {
    EntryDraft {
        headers: Headers::empty(),
        topic: Topic::from(topic),
        payload_type: Ustr::from(payload_type),
        payload: Bytes::from_static(payload),
        ts_init: UnixNanos::from(ts_init),
        index_keys: Vec::new(),
    }
}

fn run_ended_draft() -> EntryDraft {
    EntryDraft {
        headers: Headers::empty(),
        topic: Topic::from("run.lifecycle.RunEnded"),
        payload_type: Ustr::from("RunEnded"),
        payload: Bytes::new(),
        ts_init: UnixNanos::from(130),
        index_keys: Vec::new(),
    }
}

fn seed_sealed_run(tmp: &TempDir, run_id: &str) -> u64 {
    let mut backend = RedbBackend::new(tmp.path());
    backend.open_run(manifest(run_id)).expect("open run");

    let writer = EventStoreWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        noop_halt(),
        WriterConfig::default(),
    )
    .expect("spawn");

    writer
        .submit(entry_draft(
            "run.lifecycle.RunStarted",
            "RunStarted",
            b"started",
            100,
        ))
        .expect("submit started");
    writer
        .submit(entry_draft(
            "exec.command.SubmitOrder",
            "SubmitOrder",
            b"submit",
            80,
        ))
        .expect("submit command");
    writer
        .submit(entry_draft(
            "events.order.OrderFilled.S-001",
            "OrderFilled",
            b"filled",
            120,
        ))
        .expect("submit event");

    writer.close(run_ended_draft()).expect("close")
}

#[cfg(feature = "persistence")]
struct StubReplayCatalog {
    records: Vec<CatalogReplayRecord>,
    plan_queries: Vec<CatalogSliceQuery>,
    load_plans: Vec<CatalogSlicePlan>,
}

#[cfg(feature = "persistence")]
impl StubReplayCatalog {
    fn new(records: Vec<CatalogReplayRecord>) -> Self {
        Self {
            records,
            plan_queries: Vec::new(),
            load_plans: Vec::new(),
        }
    }
}

#[cfg(feature = "persistence")]
impl ReplayCatalog for StubReplayCatalog {
    type Error = String;

    fn plan_slice(
        &mut self,
        query: &CatalogSliceQuery,
    ) -> Result<CatalogSliceCoverage, Self::Error> {
        self.plan_queries.push(query.clone());
        Ok(CatalogSliceCoverage::from_files(vec![format!(
            "{}/{}",
            query.data_cls,
            query.identifiers.join(",")
        )]))
    }

    fn load_slice(
        &mut self,
        plan: &CatalogSlicePlan,
    ) -> Result<Vec<CatalogReplayRecord>, Self::Error> {
        self.load_plans.push(plan.clone());
        Ok(self
            .records
            .iter()
            .filter(|record| catalog_record_matches_query(record, &plan.query))
            .cloned()
            .collect())
    }
}

#[cfg(feature = "persistence")]
fn catalog_record_matches_query(record: &CatalogReplayRecord, query: &CatalogSliceQuery) -> bool {
    record.data_cls == query.data_cls
        && record
            .identifier
            .as_ref()
            .is_some_and(|identifier| query.identifiers.contains(identifier))
        && record.ts_init >= query.start
        && record.ts_init <= query.end
}

#[cfg(feature = "persistence")]
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

#[cfg(feature = "persistence")]
fn marker_config() -> DataMarkerConfig {
    DataMarkerConfig {
        classes: vec![DataMarkerClass::Quote],
        high_fidelity: Vec::new(),
        safety_flush_interval: Duration::from_secs(1),
        channel_capacity: 64,
    }
}

#[cfg(feature = "persistence")]
fn quote_tick(ts_init: u64) -> QuoteTick {
    let mut quote = quote_ethusdt_binance();
    quote.ts_event = UnixNanos::from(ts_init);
    quote.ts_init = UnixNanos::from(ts_init);
    quote
}

#[cfg(feature = "persistence")]
fn catalog_quote_record(ts_init: u64) -> CatalogReplayRecord {
    CatalogReplayRecord::from_data(CatalogReplayData::Quote(quote_tick(ts_init)))
}

#[rstest]
fn forensics_replay_loads_entries_from_sealed_redb_run() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "run-forensics-replay";
    let final_hwm = seed_sealed_run(&tmp, run_id);

    let (manifest, reader) =
        open_event_store_replay_source(tmp.path().to_path_buf(), INSTANCE_ID, run_id)
            .expect("open replay source");
    let replay_range = ReplaySeqRange::new(1, final_hwm);
    let plan = plan_forensics_replay_inputs(&reader, replay_range).expect("plan replay");
    let loaded = load_forensics_replay_inputs(&reader, &plan).expect("load replay");

    assert_eq!(final_hwm, 4);
    assert_eq!(manifest.status, RunStatus::Ended);
    assert_eq!(manifest.high_watermark, final_hwm);
    assert_eq!(plan.requested_range, replay_range);
    assert_eq!(plan.event_range, Some(replay_range));
    assert_eq!(plan.event_count, 4);
    assert_eq!(
        plan.event_time_range,
        Some(ReplayTimeRange::new(
            UnixNanos::from(80),
            UnixNanos::from(130),
        )),
    );
    assert!(plan.catalog_slices.is_empty());

    let seqs: Vec<u64> = loaded.entries.iter().map(|entry| entry.seq).collect();
    let payload_types: Vec<&str> = loaded
        .entries
        .iter()
        .map(|entry| entry.payload_type.as_str())
        .collect();

    assert_eq!(seqs, vec![1, 2, 3, 4]);
    assert_eq!(
        payload_types,
        vec!["RunStarted", "SubmitOrder", "OrderFilled", "RunEnded"],
    );
    assert!(
        loaded
            .entries
            .iter()
            .all(|entry| entry.recompute_hash() == entry.entry_hash),
        "loaded entries must preserve their canonical hashes",
    );
    assert!(loaded.catalog_slices.is_empty());
}

#[cfg(feature = "persistence")]
#[rstest]
fn marker_cursor_join_loads_catalog_records_from_durable_sidecar() {
    let tmp = TempDir::new().expect("tempdir");
    let run_id = "run-marker-catalog-join";
    let marker_path = tmp
        .path()
        .join(INSTANCE_ID)
        .join(format!("{run_id}.markers.redb"));
    let mut backend = RedbMarkerBackend::new(&marker_path);
    backend
        .open_run(marker_manifest(run_id))
        .expect("open marker run");

    let writer = MarkerWriter::spawn(
        Box::new(backend),
        get_atomic_clock_static(),
        MarkerWriterConfig {
            channel_capacity: 64,
            max_batch: 1,
            max_latency: Duration::from_millis(1),
        },
    )
    .expect("spawn marker writer");
    let submit_counter = Arc::new(AtomicU64::new(0));
    let mut capture = DataMarkerCapture::new(
        DataMarkerExtractorRegistry::default_registry(&[DataClass::Quote]),
        writer,
        Arc::clone(&submit_counter),
        &marker_config(),
    );
    let topic = Topic::from("data.quotes.BINANCE.ETHUSDT-PERP");

    capture.observe_publish(topic, &quote_tick(10), UnixNanos::from(10));
    capture.observe_publish(topic, &quote_tick(20), UnixNanos::from(20));
    submit_counter.store(7, Ordering::Release);
    capture.on_entry_submitted(UnixNanos::from(25));
    capture.close();

    let marker = RedbMarkerBackend::open_read_only_file(&marker_path).expect("open marker sidecar");
    assert_eq!(
        marker.manifest().expect("marker manifest").status,
        RunStatus::Ended,
    );
    let reader = MarkerReader::new(Box::new(marker));
    let mut catalog = StubReplayCatalog::new(vec![
        catalog_quote_record(10),
        catalog_quote_record(20),
        catalog_quote_record(30),
    ]);

    let joined = join_at_entry(&reader, &mut catalog, 7).expect("join marker cursor");

    assert_eq!(joined.len(), 1);
    assert_eq!(joined[0].entry.data_cls, DataClass::Quote);
    assert_eq!(joined[0].entry.identifier, "ETHUSDT-PERP.BINANCE");
    assert_eq!(joined[0].cursor.count, 2);
    assert_eq!(joined[0].cursor.ts_init_hi, UnixNanos::from(20));
    assert!(!joined[0].candidate);
    assert_eq!(
        joined[0]
            .records
            .iter()
            .map(|record| record.ts_init)
            .collect::<Vec<_>>(),
        vec![UnixNanos::from(10), UnixNanos::from(20)],
    );
    assert_eq!(catalog.plan_queries.len(), 1);
    assert_eq!(catalog.plan_queries[0].data_cls, "quotes");
    assert_eq!(
        catalog.plan_queries[0].identifiers,
        vec!["ETHUSDT-PERP.BINANCE".to_string()],
    );
    assert_eq!(catalog.plan_queries[0].end, UnixNanos::from(20));
    assert_eq!(catalog.load_plans.len(), 1);
}
