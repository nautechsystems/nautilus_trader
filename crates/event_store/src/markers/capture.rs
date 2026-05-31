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

//! Engine-thread data marker capture component.

use std::{
    any::Any,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashSet;
use nautilus_core::UnixNanos;

use crate::{
    Topic,
    markers::{
        CursorState, DataMarkerExtractorRegistry, HiFiMarker, MarkerMsg, MarkerWriter,
        StreamDictEntry,
    },
};

/// Default maximum interval between cursor snapshots when no entry boundary occurs.
pub const DEFAULT_DATA_MARKER_SAFETY_FLUSH_INTERVAL: Duration = Duration::from_secs(1);

/// Configuration for engine-thread data marker capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataMarkerConfig {
    /// Instrument identifiers that emit one high-fidelity marker per observed data message.
    pub high_fidelity: Vec<String>,
    /// Maximum interval between cursor snapshots when data advances without entry submissions.
    pub safety_flush_interval: Duration,
}

impl Default for DataMarkerConfig {
    fn default() -> Self {
        Self {
            high_fidelity: Vec::new(),
            safety_flush_interval: DEFAULT_DATA_MARKER_SAFETY_FLUSH_INTERVAL,
        }
    }
}

/// Engine-thread component that captures data marker cursors at the bus boundary.
///
/// `DataMarkerCapture` owns the in-memory cursor state and the run-local marker sequence. It reads
/// the shared entry submit counter with acquire ordering so marker records preserve engine-thread
/// causal order while the writer lane remains non-blocking.
#[derive(Debug)]
pub struct DataMarkerCapture {
    cursor: CursorState,
    registry: DataMarkerExtractorRegistry,
    writer: MarkerWriter,
    submit_counter: Arc<AtomicU64>,
    marker_seq: u64,
    hifi: AHashSet<String>,
    last_flush: UnixNanos,
    safety_flush_interval: Duration,
}

impl DataMarkerCapture {
    /// Creates a capture component over `registry`, `writer`, and the shared submit counter.
    #[must_use]
    pub fn new(
        registry: DataMarkerExtractorRegistry,
        writer: MarkerWriter,
        submit_counter: Arc<AtomicU64>,
        config: &DataMarkerConfig,
    ) -> Self {
        Self {
            cursor: CursorState::new(),
            registry,
            writer,
            submit_counter,
            marker_seq: 0,
            hifi: config.high_fidelity.iter().cloned().collect(),
            last_flush: UnixNanos::default(),
            safety_flush_interval: config.safety_flush_interval,
        }
    }

    /// Observes a bus publish and advances the data cursor when an extractor is registered.
    pub fn observe_publish(&mut self, topic: Topic, message: &dyn Any, _now: UnixNanos) {
        let Some(extractor) = self.registry.lookup(message) else {
            return;
        };
        let event_seq_before = self.submit_counter.load(Ordering::Acquire);
        let Some(identifier) = extractor.identifier(message) else {
            return;
        };
        let Some((ts_event, ts_init)) = extractor.timestamps(message) else {
            return;
        };
        let data_class = extractor.data_class();
        let record_fingerprint = if self.hifi.contains(identifier.as_str()) {
            let Some(record_fingerprint) = extractor.fingerprint(message) else {
                return;
            };
            Some(record_fingerprint)
        } else {
            None
        };

        let (slot, same_ts_ordinal) = self.cursor.advance(topic, data_class, &identifier, ts_init);
        self.drain_dict_entries();

        if let Some(record_fingerprint) = record_fingerprint {
            let marker_seq = self.marker_seq + 1;
            let marker = HiFiMarker {
                marker_seq,
                event_seq_before,
                slot,
                ts_event,
                ts_init,
                same_ts_ordinal,
                record_fingerprint,
            };
            self.submit_marker(MarkerMsg::HiFi(marker), marker_seq);
        }
    }

    /// Emits a cursor snapshot for an event-store entry boundary when data advanced.
    pub fn on_entry_submitted(&mut self, now: UnixNanos) {
        self.flush_snapshot(now);
    }

    /// Emits a cursor snapshot when the safety interval has elapsed and data advanced.
    pub fn maybe_safety_flush(&mut self, now: UnixNanos) {
        if now
            .duration_since(&self.last_flush)
            .is_some_and(|elapsed| elapsed >= duration_nanos_saturating(self.safety_flush_interval))
        {
            self.flush_snapshot(now);
        }
    }

    /// Closes the writer lane and seals the marker run.
    pub fn close(self) {
        self.writer.close();
    }

    fn drain_dict_entries(&mut self) {
        for entry in self.cursor.take_new_dict_entries() {
            self.submit_dict(entry);
        }
    }

    fn submit_dict(&self, entry: StreamDictEntry) {
        let _ = self.writer.put_dict(entry);
    }

    fn flush_snapshot(&mut self, now: UnixNanos) {
        let marker_seq = self.marker_seq + 1;
        let event_seq_before = self.submit_counter.load(Ordering::Acquire);

        if let Some(snapshot) = self
            .cursor
            .build_snapshot(marker_seq, event_seq_before, now)
        {
            self.submit_marker(MarkerMsg::Snapshot(snapshot), marker_seq);
            self.last_flush = now;
        }
    }

    fn submit_marker(&mut self, msg: MarkerMsg, marker_seq: u64) {
        let _ = self.writer.submit(msg, marker_seq);
        self.marker_seq = marker_seq;
    }
}

fn duration_nanos_saturating(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::{
        any::Any,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    use nautilus_core::{UnixNanos, time::get_atomic_clock_static};
    use rstest::rstest;

    use super::*;
    use crate::{
        Topic,
        error::EventStoreError,
        manifest::RunStatus,
        markers::{
            DataClass, DataCursorSnapshot, DataMarkerExtractor, DataMarkerExtractorRegistry,
            HiFiMarker, MarkerBackend, MarkerGap, MarkerManifest, MarkerWriter, MarkerWriterConfig,
            MemoryMarkerBackend, StreamDictEntry,
        },
    };

    #[derive(Debug)]
    struct TestQuote {
        identifier: &'static str,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        fingerprint: [u8; 32],
    }

    #[derive(Debug)]
    struct IgnoredMessage;

    #[derive(Debug)]
    struct TestQuoteExtractor;

    impl DataMarkerExtractor for TestQuoteExtractor {
        fn data_class(&self) -> DataClass {
            DataClass::Quote
        }

        fn identifier(&self, msg: &dyn Any) -> Option<String> {
            msg.downcast_ref::<TestQuote>()
                .map(|quote| quote.identifier.to_string())
        }

        fn timestamps(&self, msg: &dyn Any) -> Option<(UnixNanos, UnixNanos)> {
            msg.downcast_ref::<TestQuote>()
                .map(|quote| (quote.ts_event, quote.ts_init))
        }

        fn fingerprint(&self, msg: &dyn Any) -> Option<[u8; 32]> {
            msg.downcast_ref::<TestQuote>()
                .map(|quote| quote.fingerprint)
        }
    }

    #[derive(Debug)]
    struct SharedMemoryMarker(Arc<Mutex<MemoryMarkerBackend>>);

    impl SharedMemoryMarker {
        fn new() -> (Self, Arc<Mutex<MemoryMarkerBackend>>) {
            let shared = Arc::new(Mutex::new(MemoryMarkerBackend::new()));
            (Self(Arc::clone(&shared)), shared)
        }
    }

    impl MarkerBackend for SharedMemoryMarker {
        fn open_run(&mut self, _: MarkerManifest) -> Result<(), EventStoreError> {
            unreachable!("test wrapper does not forward open_run")
        }

        fn append_snapshot(
            &mut self,
            snapshot: &DataCursorSnapshot,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .append_snapshot(snapshot, hash)
        }

        fn append_hifi(
            &mut self,
            marker: &HiFiMarker,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .append_hifi(marker, hash)
        }

        fn append_gap(&mut self, gap: &MarkerGap, hash: [u8; 32]) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .append_gap(gap, hash)
        }

        fn put_dict(
            &mut self,
            entry: &StreamDictEntry,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .put_dict(entry, hash)
        }

        fn scan_snapshots(&self) -> Result<Vec<DataCursorSnapshot>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .scan_snapshots()
        }

        fn scan_hifi(&self) -> Result<Vec<HiFiMarker>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .scan_hifi()
        }

        fn scan_gaps(&self) -> Result<Vec<MarkerGap>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .scan_gaps()
        }

        fn scan_dict(&self) -> Result<Vec<StreamDictEntry>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .scan_dict()
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .seal(status)
        }

        fn manifest(&self) -> Result<MarkerManifest, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory marker poisoned")
                .manifest()
        }
    }

    fn manifest() -> MarkerManifest {
        MarkerManifest {
            run_id: "1700000000-phase7".to_string(),
            enabled_classes: vec![DataClass::Quote],
            high_fidelity: true,
            snapshot_count: 0,
            hifi_count: 0,
            gap_count: 0,
            dict_count: 0,
            status: RunStatus::Running,
        }
    }

    fn registry() -> DataMarkerExtractorRegistry {
        let mut registry = DataMarkerExtractorRegistry::new();
        registry.register::<TestQuote>(Box::new(TestQuoteExtractor));
        registry
    }

    fn config(high_fidelity: Vec<String>, safety_flush_interval: Duration) -> DataMarkerConfig {
        DataMarkerConfig {
            high_fidelity,
            safety_flush_interval,
        }
    }

    fn quote(identifier: &'static str, ts_event: u64, ts_init: u64) -> TestQuote {
        TestQuote {
            identifier,
            ts_event: UnixNanos::from(ts_event),
            ts_init: UnixNanos::from(ts_init),
            fingerprint: [7; 32],
        }
    }

    fn open_capture(
        config: &DataMarkerConfig,
        submit_counter: Arc<AtomicU64>,
    ) -> (DataMarkerCapture, Arc<Mutex<MemoryMarkerBackend>>) {
        let (wrapper, shared) = SharedMemoryMarker::new();
        shared
            .lock()
            .expect("shared marker")
            .open_run(manifest())
            .expect("open marker run");

        let writer = MarkerWriter::spawn(
            Box::new(wrapper),
            get_atomic_clock_static(),
            MarkerWriterConfig {
                channel_capacity: 100,
                max_batch: 1,
                max_latency: Duration::from_millis(1),
            },
        )
        .expect("spawn writer");

        (
            DataMarkerCapture::new(registry(), writer, submit_counter, config),
            shared,
        )
    }

    fn snapshots(shared: &Arc<Mutex<MemoryMarkerBackend>>) -> Vec<DataCursorSnapshot> {
        shared
            .lock()
            .expect("shared marker")
            .scan_snapshots()
            .expect("scan snapshots")
    }

    fn hifi(shared: &Arc<Mutex<MemoryMarkerBackend>>) -> Vec<HiFiMarker> {
        shared
            .lock()
            .expect("shared marker")
            .scan_hifi()
            .expect("scan hifi")
    }

    fn dict(shared: &Arc<Mutex<MemoryMarkerBackend>>) -> Vec<StreamDictEntry> {
        shared
            .lock()
            .expect("shared marker")
            .scan_dict()
            .expect("scan dict")
    }

    #[rstest]
    fn event_seq_before_tracks_submit_counter() {
        let submit_counter = Arc::new(AtomicU64::new(5));
        let cfg = config(vec!["ETHUSDT.BINANCE".to_string()], Duration::from_secs(1));
        let (mut capture, shared) = open_capture(&cfg, Arc::clone(&submit_counter));
        let topic: Topic = "data.quotes.BINANCE.ETHUSDT".into();

        capture.observe_publish(
            topic,
            &quote("ETHUSDT.BINANCE", 10, 20),
            UnixNanos::from(20),
        );
        submit_counter.store(6, Ordering::Release);
        capture.on_entry_submitted(UnixNanos::from(30));
        capture.close();

        let snapshots = snapshots(&shared);
        let hifi = hifi(&shared);

        assert_eq!(hifi.len(), 1);
        assert_eq!(hifi[0].event_seq_before, 5);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].event_seq_before, 6);
    }

    #[rstest]
    fn snapshot_written_on_entry_boundary() {
        let submit_counter = Arc::new(AtomicU64::new(1));
        let cfg = config(Vec::new(), Duration::from_secs(1));
        let (mut capture, shared) = open_capture(&cfg, submit_counter);
        let topic: Topic = "data.quotes.BINANCE.ETHUSDT".into();

        capture.observe_publish(
            topic,
            &quote("ETHUSDT.BINANCE", 10, 20),
            UnixNanos::from(20),
        );
        capture.on_entry_submitted(UnixNanos::from(30));
        capture.close();

        let snapshots = snapshots(&shared);
        let dict = dict(&shared);

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].marker_seq, 1);
        assert_eq!(snapshots[0].event_seq_before, 1);
        assert_eq!(snapshots[0].advanced[0].count, 1);
        assert_eq!(
            dict,
            vec![StreamDictEntry {
                slot: 0,
                data_cls: DataClass::Quote,
                identifier: "ETHUSDT.BINANCE".to_string(),
            }]
        );
    }

    #[rstest]
    fn safety_flush_emits_without_entry() {
        let submit_counter = Arc::new(AtomicU64::new(0));
        let cfg = config(Vec::new(), Duration::from_nanos(10));
        let (mut capture, shared) = open_capture(&cfg, submit_counter);
        let topic: Topic = "data.quotes.BINANCE.ETHUSDT".into();

        capture.observe_publish(
            topic,
            &quote("ETHUSDT.BINANCE", 10, 20),
            UnixNanos::from(20),
        );
        capture.maybe_safety_flush(UnixNanos::from(30));
        capture.close();

        let snapshots = snapshots(&shared);

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].event_seq_before, 0);
        assert_eq!(snapshots[0].ts_init, UnixNanos::from(30));
    }

    #[rstest]
    fn safety_flush_waits_for_interval_after_entry_boundary() {
        let submit_counter = Arc::new(AtomicU64::new(1));
        let cfg = config(Vec::new(), Duration::from_nanos(10));
        let (mut capture, shared) = open_capture(&cfg, submit_counter);
        let topic: Topic = "data.quotes.BINANCE.ETHUSDT".into();

        capture.observe_publish(
            topic,
            &quote("ETHUSDT.BINANCE", 10, 20),
            UnixNanos::from(20),
        );
        capture.on_entry_submitted(UnixNanos::from(100));
        capture.observe_publish(
            topic,
            &quote("ETHUSDT.BINANCE", 30, 40),
            UnixNanos::from(40),
        );
        capture.maybe_safety_flush(UnixNanos::from(109));
        capture.maybe_safety_flush(UnixNanos::from(110));
        capture.close();

        let snapshots = snapshots(&shared);

        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].ts_init, UnixNanos::from(100));
        assert_eq!(snapshots[0].advanced[0].count, 1);
        assert_eq!(snapshots[1].ts_init, UnixNanos::from(110));
        assert_eq!(snapshots[1].advanced[0].count, 2);
    }

    #[rstest]
    fn hifi_marker_emitted_for_configured_instrument() {
        let submit_counter = Arc::new(AtomicU64::new(2));
        let cfg = config(vec!["ETHUSDT.BINANCE".to_string()], Duration::from_secs(1));
        let (mut capture, shared) = open_capture(&cfg, submit_counter);
        let eth_topic: Topic = "data.quotes.BINANCE.ETHUSDT".into();
        let btc_topic: Topic = "data.quotes.BINANCE.BTCUSDT".into();

        capture.observe_publish(
            eth_topic,
            &quote("ETHUSDT.BINANCE", 10, 20),
            UnixNanos::from(20),
        );
        capture.observe_publish(
            btc_topic,
            &quote("BTCUSDT.BINANCE", 30, 40),
            UnixNanos::from(40),
        );
        capture.close();

        let hifi = hifi(&shared);

        assert_eq!(hifi.len(), 1);
        assert_eq!(hifi[0].marker_seq, 1);
        assert_eq!(hifi[0].event_seq_before, 2);
        assert_eq!(hifi[0].slot, 0);
        assert_eq!(hifi[0].ts_event, UnixNanos::from(10));
        assert_eq!(hifi[0].ts_init, UnixNanos::from(20));
        assert_eq!(hifi[0].same_ts_ordinal, 0);
        assert_eq!(hifi[0].record_fingerprint, [7; 32]);
    }

    #[rstest]
    fn hifi_same_ts_ordinal_increments() {
        let submit_counter = Arc::new(AtomicU64::new(0));
        let cfg = config(vec!["ETHUSDT.BINANCE".to_string()], Duration::from_secs(1));
        let (mut capture, shared) = open_capture(&cfg, submit_counter);
        let topic: Topic = "data.quotes.BINANCE.ETHUSDT".into();

        capture.observe_publish(
            topic,
            &quote("ETHUSDT.BINANCE", 10, 20),
            UnixNanos::from(20),
        );
        capture.observe_publish(
            topic,
            &quote("ETHUSDT.BINANCE", 11, 20),
            UnixNanos::from(20),
        );
        capture.close();

        let hifi = hifi(&shared);

        assert_eq!(hifi.len(), 2);
        assert_eq!(
            hifi.iter()
                .map(|marker| marker.same_ts_ordinal)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            hifi.iter()
                .map(|marker| marker.marker_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[rstest]
    fn unregistered_type_is_ignored() {
        let submit_counter = Arc::new(AtomicU64::new(0));
        let cfg = config(Vec::new(), Duration::from_nanos(1));
        let (mut capture, shared) = open_capture(&cfg, submit_counter);
        let topic: Topic = "data.quotes.BINANCE.ETHUSDT".into();

        capture.observe_publish(topic, &IgnoredMessage, UnixNanos::from(20));
        capture.maybe_safety_flush(UnixNanos::from(30));
        capture.close();

        assert!(snapshots(&shared).is_empty());
        assert!(hifi(&shared).is_empty());
        assert!(dict(&shared).is_empty());
    }
}
