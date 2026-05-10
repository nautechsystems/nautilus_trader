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

//! Integration tests for the bus capture adapter against a real `MessageBus` and the
//! production [`EventStoreWriter`] over a shared [`MemoryBackend`].
//!
//! Exercises the SPEC's "capture before fanout" contract end-to-end with the Phase 6
//! representative samples: a command (`SubmitOrder`), a generated event (`OrderFilled`),
//! and a raw venue report (`OrderStatusReport`).

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_common::{
    messages::execution::SubmitOrder,
    msgbus::{self, MessageBus, ShareableMessageHandler},
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_static};
use nautilus_event_store::{
    AppendEntry, BusCaptureAdapter, EncoderRegistry, EntryDraft, EventStore, EventStoreEntry,
    EventStoreError, EventStoreWriter, HaltCallback, Headers, IndexKind, MemoryBackend,
    RegisteredComponents, RunManifest, RunStatus, ScanDirection, Topic, WriterConfig,
    default_registry, noop_halt,
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderFilled, OrderInitialized},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    reports::OrderStatusReport,
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
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

/// Wraps `MemoryBackend` so the writer thread and the test thread can read the same
/// backend instance.
#[derive(Debug)]
struct SharedMemory(Arc<Mutex<MemoryBackend>>);

impl EventStore for SharedMemory {
    fn open_run(&mut self, _: RunManifest) -> Result<(), EventStoreError> {
        unreachable!("test wrapper does not forward open_run")
    }

    fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
        self.0.lock().expect("shared").append_batch(entries)
    }

    fn scan_range(
        &self,
        from: u64,
        to: u64,
        direction: ScanDirection,
    ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        self.0
            .lock()
            .expect("shared")
            .scan_range(from, to, direction)
    }

    fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
        self.0.lock().expect("shared").scan_seq(seq)
    }

    fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
        self.0.lock().expect("shared").lookup(kind, key)
    }

    fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
        self.0.lock().expect("shared").iter_index_keys(kind)
    }

    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
        self.0.lock().expect("shared").seal(status)
    }

    fn manifest(&self) -> Result<RunManifest, EventStoreError> {
        self.0.lock().expect("shared").manifest()
    }

    fn high_watermark(&self) -> Result<u64, EventStoreError> {
        self.0.lock().expect("shared").high_watermark()
    }
}

fn writer_with_open_run(
    run_id: &str,
    halt: HaltCallback,
) -> (Arc<EventStoreWriter>, Arc<Mutex<MemoryBackend>>) {
    let backend_arc: Arc<Mutex<MemoryBackend>> = Arc::new(Mutex::new(MemoryBackend::new()));
    backend_arc
        .lock()
        .expect("inner")
        .open_run(manifest(run_id))
        .expect("open run");

    let wrapper = SharedMemory(Arc::clone(&backend_arc));
    let writer = EventStoreWriter::spawn(
        Box::new(wrapper),
        get_atomic_clock_static(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");
    (Arc::new(writer), backend_arc)
}

fn drain(writer: &Arc<EventStoreWriter>, target_hwm: u64) {
    let mut waited = Duration::ZERO;
    let deadline = Duration::from_secs(2);
    while writer.high_watermark() < target_hwm && waited < deadline {
        std::thread::sleep(Duration::from_millis(5));
        waited += Duration::from_millis(5);
    }
    assert!(
        writer.high_watermark() >= target_hwm,
        "writer high_watermark {} did not reach {target_hwm} within {:?}",
        writer.high_watermark(),
        deadline,
    );
}

/// Makes a [`SubmitOrder`] command suitable for a representative-end-to-end capture.
fn make_submit_order(client_order_id: ClientOrderId) -> SubmitOrder {
    let order_init = OrderInitialized::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        client_order_id,
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from("1"),
        TimeInForce::Gtc,
        false,
        false,
        false,
        false,
        UUID4::new(),
        UnixNanos::from(1),
        UnixNanos::from(2),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    SubmitOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("BINANCE")),
        StrategyId::from("S-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        client_order_id,
        order_init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::from(3),
    )
}

fn make_order_filled(client_order_id: ClientOrderId, venue_order_id: VenueOrderId) -> OrderFilled {
    OrderFilled::new(
        TraderId::from("TRADER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        client_order_id,
        venue_order_id,
        AccountId::from("BINANCE-001"),
        TradeId::from("T-9999"),
        OrderSide::Buy,
        OrderType::Market,
        Quantity::from("1"),
        Price::from("100.00"),
        Currency::USDT(),
        LiquiditySide::Taker,
        UUID4::new(),
        UnixNanos::from(10),
        UnixNanos::from(11),
        false,
        None,
        Some(Money::new(0.10, Currency::USDT())),
    )
}

fn make_order_status_report(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
) -> OrderStatusReport {
    OrderStatusReport::new(
        AccountId::from("BINANCE-001"),
        InstrumentId::from("ETHUSDT-PERP.BINANCE"),
        Some(client_order_id),
        venue_order_id,
        OrderSide::Buy,
        OrderType::Market,
        TimeInForce::Gtc,
        OrderStatus::Filled,
        Quantity::from("1"),
        Quantity::from("1"),
        UnixNanos::from(20),
        UnixNanos::from(21),
        UnixNanos::from(22),
        Some(UUID4::new()),
    )
}

#[rstest]
fn end_to_end_capture_writes_command_event_and_report() {
    // Construct a real MessageBus on this thread so capture runs in the same
    // single-threaded environment the kernel uses; the writer still owns its dedicated
    // I/O thread.
    let bus = MessageBus::new(TraderId::from("TRADER-001"), UUID4::new(), None, None);
    let _bus_rc = bus.register_message_bus();

    let (writer, backend_arc) = writer_with_open_run("run-capture-e2e", noop_halt());
    let registry = Arc::new(default_registry());
    let adapter = BusCaptureAdapter::new(Arc::clone(&writer), registry, noop_halt());

    let client_order_id = ClientOrderId::from("O-20260510-000001");
    let venue_order_id = VenueOrderId::from("V-12345");

    // Capture the command first, emulating the kernel's send-side dispatch wrapper.
    let cmd = make_submit_order(client_order_id);
    let captured_cmd = adapter
        .capture::<SubmitOrder>(
            Topic::from("exec.command.SubmitOrder"),
            &cmd,
            Headers::empty(),
            cmd.ts_init,
        )
        .expect("capture cmd");
    assert!(captured_cmd, "command must be allow-listed");

    // Capture the generated event, emulating the publish-side wrapper.
    let event = make_order_filled(client_order_id, venue_order_id);
    let captured_event = adapter
        .capture::<OrderFilled>(
            Topic::from("events.order.OrderFilled.S-001"),
            &event,
            Headers::empty(),
            event.ts_init,
        )
        .expect("capture event");
    assert!(captured_event, "event must be allow-listed");

    // Capture the raw venue report, emulating the reconciliation publish boundary.
    let report = make_order_status_report(client_order_id, venue_order_id);
    let captured_report = adapter
        .capture::<OrderStatusReport>(
            Topic::from("reports.OrderStatusReport.BINANCE"),
            &report,
            Headers::empty(),
            report.ts_init,
        )
        .expect("capture report");
    assert!(captured_report, "report must be allow-listed");

    drain(&writer, 3);

    let backend = backend_arc.lock().expect("backend");

    let cmd_entry = backend.scan_seq(1).expect("scan").expect("present");
    assert_eq!(cmd_entry.payload_type.as_str(), "SubmitOrder");
    assert_eq!(cmd_entry.topic.as_ref(), "exec.command.SubmitOrder");
    assert_eq!(cmd_entry.ts_init, cmd.ts_init);

    let event_entry = backend.scan_seq(2).expect("scan").expect("present");
    assert_eq!(event_entry.payload_type.as_str(), "OrderFilled");
    assert_eq!(event_entry.topic.as_ref(), "events.order.OrderFilled.S-001");

    let report_entry = backend.scan_seq(3).expect("scan").expect("present");
    assert_eq!(report_entry.payload_type.as_str(), "OrderStatusReport");
    assert_eq!(
        report_entry.topic.as_ref(),
        "reports.OrderStatusReport.BINANCE"
    );

    // Sidecar indices must point at the entries that mentioned each id; the command's
    // entry is the earliest mention of the client order id, and the event's entry is
    // the earliest mention of the venue order id.
    let by_client = backend
        .lookup(IndexKind::ClientOrderId, client_order_id.as_str())
        .expect("lookup")
        .expect("indexed");
    assert_eq!(
        by_client, 1,
        "client_order_id index must point at the command"
    );
    let by_venue = backend
        .lookup(IndexKind::VenueOrderId, venue_order_id.as_str())
        .expect("lookup")
        .expect("indexed");
    assert_eq!(by_venue, 2, "venue_order_id index must point at the event");

    // Hashes survive a round trip through the backend.
    for entry in [&cmd_entry, &event_entry, &report_entry] {
        assert_eq!(entry.recompute_hash(), entry.entry_hash);
    }

    drop(backend);
    drop(adapter);

    // Recover the writer from the Arc so we can close it cleanly.
    let writer = Arc::try_unwrap(writer).expect("sole writer reference");
    let final_hwm = writer.close(run_ended_draft()).expect("close writer");
    assert_eq!(final_hwm, 4, "three captured entries plus RunEnded");

    let backend = backend_arc.lock().expect("backend");
    let manifest = backend.manifest().expect("manifest");
    assert_eq!(manifest.status, RunStatus::Ended);
    assert_eq!(manifest.high_watermark, 4);
}

#[rstest]
fn capture_skips_messages_outside_the_allow_list() {
    let bus = MessageBus::new(TraderId::from("TRADER-001"), UUID4::new(), None, None);
    let _bus_rc = bus.register_message_bus();

    let (writer, _backend) = writer_with_open_run("run-capture-deny", noop_halt());
    let registry = Arc::new(EncoderRegistry::new());
    let adapter = BusCaptureAdapter::new(Arc::clone(&writer), registry, noop_halt());

    // Empty registry: every type is out-of-list, so capture must report `false` rather
    // than surfacing an error. This is the contract that lets the adapter sit on dispatch
    // paths that mix state-affecting and non-state-affecting traffic.
    let cmd = make_submit_order(ClientOrderId::from("O-deny"));
    let captured = adapter
        .capture::<SubmitOrder>(
            Topic::from("exec.command.SubmitOrder"),
            &cmd,
            Headers::empty(),
            cmd.ts_init,
        )
        .expect("capture");
    assert!(!captured);
    assert_eq!(writer.high_watermark(), 0);
}

#[rstest]
fn captured_entry_observed_before_bus_subscriber_dispatch() {
    // Demonstrates the dispatch-boundary ordering contract: the writer high-watermark
    // for the captured entry advances before the bus dispatches to a downstream
    // subscriber. Modeled by capturing first, then publishing, with a subscriber that
    // records the writer's high-watermark at handler-invocation time.
    let bus = MessageBus::new(TraderId::from("TRADER-001"), UUID4::new(), None, None);
    let _bus_rc = bus.register_message_bus();

    let (writer, _backend) = writer_with_open_run("run-capture-order", noop_halt());
    let registry = Arc::new(default_registry());
    let adapter = BusCaptureAdapter::new(Arc::clone(&writer), registry, noop_halt());

    let observed_hwm: Arc<Mutex<Option<u64>>> = Arc::new(Mutex::new(None));
    let observed_for_handler = Arc::clone(&observed_hwm);
    let writer_for_handler = Arc::clone(&writer);
    let handler = ShareableMessageHandler::from_typed(move |_msg: &SubmitOrder| {
        *observed_for_handler.lock().expect("observed lock") =
            Some(writer_for_handler.high_watermark());
    });

    msgbus::subscribe_any("exec.command.*".into(), handler, Some(0));

    let cmd = make_submit_order(ClientOrderId::from("O-ordering"));
    adapter
        .capture::<SubmitOrder>(
            Topic::from("exec.command.SubmitOrder"),
            &cmd,
            Headers::empty(),
            cmd.ts_init,
        )
        .expect("capture");
    drain(&writer, 1);

    msgbus::publish_any("exec.command.SubmitOrder".into(), &cmd);

    let observed = observed_hwm
        .lock()
        .expect("observed lock")
        .expect("subscriber must have observed");
    assert!(
        observed >= 1,
        "downstream subscriber observed hwm {observed} but expected the captured entry's seq",
    );
}

fn run_ended_draft() -> EntryDraft {
    EntryDraft {
        headers: Headers::empty(),
        topic: Topic::from("run.lifecycle.RunEnded"),
        payload_type: Ustr::from("RunEnded"),
        payload: Bytes::new(),
        ts_init: UnixNanos::from(99_999),
        index_keys: Vec::new(),
    }
}
