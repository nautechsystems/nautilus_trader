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

//! Non-blocking writer lane for the data marker sidecar.

use std::time::Duration;

use crate::{
    error::EventStoreError,
    markers::{DataCursorSnapshot, HiFiMarker, MarkerBackend, MarkerGap, StreamDictEntry},
};

/// Default channel capacity for markers pending the writer thread.
pub const DEFAULT_MARKER_CHANNEL_CAPACITY: usize = 10_000;
/// Default maximum number of markers collected before forcing a flush.
pub const DEFAULT_MARKER_MAX_BATCH: usize = 100;
/// Default maximum time a marker batch may accumulate before forcing a flush.
pub const DEFAULT_MARKER_MAX_LATENCY: Duration = Duration::from_millis(5);

/// Configuration knobs for the data marker writer.
#[derive(Clone, Debug)]
pub struct MarkerWriterConfig {
    /// Capacity of the bounded `sync_channel` between submit and the writer thread.
    pub channel_capacity: usize,
    /// Maximum marker messages collected before a flush is forced.
    pub max_batch: usize,
    /// Maximum time a marker batch may accumulate before a flush is forced.
    pub max_latency: Duration,
}

impl Default for MarkerWriterConfig {
    fn default() -> Self {
        Self {
            channel_capacity: DEFAULT_MARKER_CHANNEL_CAPACITY,
            max_batch: DEFAULT_MARKER_MAX_BATCH,
            max_latency: DEFAULT_MARKER_MAX_LATENCY,
        }
    }
}

/// A message sent to the data marker writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerMsg {
    /// Cursor snapshot marker.
    Snapshot(DataCursorSnapshot),
    /// High-fidelity per-record marker.
    HiFi(HiFiMarker),
    /// Stream dictionary entry.
    Dict(StreamDictEntry),
    /// Closes and seals the marker run after draining older messages.
    Close,
    #[doc(hidden)]
    GapThen { gap: MarkerGap, msg: Box<Self> },
}

#[cfg(not(madsim))]
mod imp {
    use std::{
        fmt::Debug,
        sync::{
            Mutex,
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
        },
        thread::{self, JoinHandle},
        time::Instant,
    };

    use nautilus_core::time::AtomicTime;

    use super::{
        EventStoreError, MarkerBackend, MarkerGap, MarkerMsg, MarkerWriterConfig, StreamDictEntry,
    };
    use crate::{
        manifest::RunStatus,
        markers::{
            MarkerGapReason, compute_dict_hash, compute_gap_hash, compute_hifi_hash,
            compute_marker_hash,
        },
    };

    const MARKER_WRITER_THREAD_NAME: &str = "event-store-marker-writer";

    /// Dedicated marker writer thread.
    pub struct MarkerWriter {
        tx: Option<SyncSender<MarkerMsg>>,
        handle: Option<JoinHandle<()>>,
        last_submitted_seq: AtomicU64,
        dropped: Mutex<Option<DroppedRange>>,
        closed: AtomicBool,
    }

    #[derive(Debug, Clone, Copy)]
    struct DroppedRange {
        from_marker_seq: u64,
        to_marker_seq: u64,
    }

    impl DroppedRange {
        const fn new(marker_seq: u64) -> Self {
            Self {
                from_marker_seq: marker_seq,
                to_marker_seq: marker_seq,
            }
        }

        fn extend(&mut self, marker_seq: u64) {
            if marker_seq < self.from_marker_seq {
                self.from_marker_seq = marker_seq;
            }

            if marker_seq > self.to_marker_seq {
                self.to_marker_seq = marker_seq;
            }
        }

        const fn gap(self, reason: MarkerGapReason) -> MarkerGap {
            MarkerGap {
                from_marker_seq: self.from_marker_seq,
                to_marker_seq: self.to_marker_seq,
                reason,
            }
        }
    }

    impl Debug for MarkerWriter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct(stringify!(MarkerWriter))
                .field(
                    "last_submitted_seq",
                    &self.last_submitted_seq.load(Ordering::Acquire),
                )
                .field("closed", &self.closed.load(Ordering::Acquire))
                .field("tx_attached", &self.tx.is_some())
                .finish_non_exhaustive()
        }
    }

    impl MarkerWriter {
        /// Spawns the marker writer thread and takes ownership of `backend`.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Backend`] when the backend has no open run or when the
        /// writer thread cannot be spawned.
        #[allow(clippy::needless_pass_by_value)]
        pub fn spawn(
            backend: Box<dyn MarkerBackend + Send>,
            _clock: &'static AtomicTime,
            config: MarkerWriterConfig,
        ) -> Result<Self, EventStoreError> {
            backend.manifest()?;
            let (tx, rx) = mpsc::sync_channel::<MarkerMsg>(config.channel_capacity);
            let config_for_thread = config;

            let handle = thread::Builder::new()
                .name(MARKER_WRITER_THREAD_NAME.to_string())
                .spawn(move || run(backend, &rx, &config_for_thread))
                .map_err(|e| {
                    EventStoreError::Backend(format!("spawn marker writer thread: {e}"))
                })?;

            Ok(Self {
                tx: Some(tx),
                handle: Some(handle),
                last_submitted_seq: AtomicU64::new(0),
                dropped: Mutex::new(None),
                closed: AtomicBool::new(false),
            })
        }

        /// Submits a marker without blocking the caller.
        ///
        /// Returns `Ok(false)` when the bounded channel is full; the dropped marker
        /// sequence is recorded into the next overflow gap.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Closed`] when the writer is closed, and
        /// [`EventStoreError::Backend`] when `msg` is not a `Snapshot` or `HiFi` marker.
        ///
        /// # Panics
        ///
        /// Panics if the pending overflow range mutex is poisoned.
        pub fn submit(&self, msg: MarkerMsg, marker_seq: u64) -> Result<bool, EventStoreError> {
            if self.closed.load(Ordering::Acquire) {
                return Err(EventStoreError::Closed);
            }

            if matches!(
                msg,
                MarkerMsg::Dict(_) | MarkerMsg::Close | MarkerMsg::GapThen { .. }
            ) {
                return Err(EventStoreError::Backend(
                    "submit accepts Snapshot or HiFi marker messages".to_string(),
                ));
            }

            let tx = self.tx.as_ref().ok_or(EventStoreError::Closed)?;
            let mut dropped = self
                .dropped
                .lock()
                .expect("marker dropped range mutex poisoned");
            let outbound = if let Some(range) = *dropped {
                MarkerMsg::GapThen {
                    gap: range.gap(MarkerGapReason::Overflow),
                    msg: Box::new(msg),
                }
            } else {
                msg
            };

            match tx.try_send(outbound) {
                Ok(()) => {
                    *dropped = None;
                    self.last_submitted_seq.store(marker_seq, Ordering::Release);
                    Ok(true)
                }
                Err(TrySendError::Full(_)) => {
                    extend_dropped_range(&mut dropped, marker_seq);
                    Ok(false)
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.closed.store(true, Ordering::Release);
                    Err(EventStoreError::Closed)
                }
            }
        }

        /// Submits a stream dictionary entry to the writer.
        ///
        /// Dictionary entries are one-time stream metadata, so this path waits for channel
        /// capacity instead of dropping and gap-accounting them.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Closed`] when the writer is closed.
        pub fn put_dict(&self, entry: StreamDictEntry) -> Result<bool, EventStoreError> {
            if self.closed.load(Ordering::Acquire) {
                return Err(EventStoreError::Closed);
            }

            let tx = self.tx.as_ref().ok_or(EventStoreError::Closed)?;
            if tx.send(MarkerMsg::Dict(entry)).is_ok() {
                Ok(true)
            } else {
                self.closed.store(true, Ordering::Release);
                Err(EventStoreError::Closed)
            }
        }

        /// Drains pending markers and seals the marker run.
        ///
        /// # Panics
        ///
        /// Panics if the pending overflow range mutex is poisoned.
        pub fn close(mut self) {
            self.closed.store(true, Ordering::Release);

            if let Some(tx) = self.tx.take() {
                let close_msg = {
                    let mut dropped = self
                        .dropped
                        .lock()
                        .expect("marker dropped range mutex poisoned");
                    if let Some(range) = dropped.take() {
                        MarkerMsg::GapThen {
                            gap: range.gap(MarkerGapReason::WriterClosed),
                            msg: Box::new(MarkerMsg::Close),
                        }
                    } else {
                        MarkerMsg::Close
                    }
                };
                let _ = tx.send(close_msg);
                drop(tx);
            }

            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    impl Drop for MarkerWriter {
        fn drop(&mut self) {
            self.closed.store(true, Ordering::Release);
            self.tx.take();

            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn extend_dropped_range(dropped: &mut Option<DroppedRange>, marker_seq: u64) {
        if let Some(range) = dropped {
            range.extend(marker_seq);
        } else {
            *dropped = Some(DroppedRange::new(marker_seq));
        }
    }

    fn run(
        mut backend: Box<dyn MarkerBackend + Send>,
        rx: &mpsc::Receiver<MarkerMsg>,
        config: &MarkerWriterConfig,
    ) {
        let max_batch = config.max_batch.max(1);
        let mut batch = Vec::with_capacity(max_batch);

        while let Ok(first) = rx.recv() {
            let mut should_close = push_batch(&mut batch, first);
            let mut disconnected = false;
            let started = Instant::now();

            while !should_close && batch.len() < max_batch {
                let Some(remaining) = config.max_latency.checked_sub(started.elapsed()) else {
                    break;
                };

                if remaining.is_zero() {
                    break;
                }

                match rx.recv_timeout(remaining) {
                    Ok(msg) => should_close = push_batch(&mut batch, msg),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }

            if write_batch(backend.as_mut(), batch.drain(..)).is_err() {
                return;
            }

            if should_close {
                let _ = backend.seal(RunStatus::Ended);
                return;
            }

            if disconnected {
                return;
            }
        }
    }

    fn push_batch(batch: &mut Vec<MarkerMsg>, msg: MarkerMsg) -> bool {
        match msg {
            MarkerMsg::Close => true,
            other => {
                let should_close = closes_after_msg(&other);
                batch.push(other);
                should_close
            }
        }
    }

    fn closes_after_msg(msg: &MarkerMsg) -> bool {
        match msg {
            MarkerMsg::Close => true,
            MarkerMsg::GapThen { msg, .. } => closes_after_msg(msg),
            MarkerMsg::Snapshot(_) | MarkerMsg::HiFi(_) | MarkerMsg::Dict(_) => false,
        }
    }

    fn write_batch(
        backend: &mut dyn MarkerBackend,
        batch: impl IntoIterator<Item = MarkerMsg>,
    ) -> Result<(), EventStoreError> {
        for msg in batch {
            write_msg(backend, msg)?;
        }
        Ok(())
    }

    fn write_msg(backend: &mut dyn MarkerBackend, msg: MarkerMsg) -> Result<(), EventStoreError> {
        match msg {
            MarkerMsg::Snapshot(snapshot) => {
                backend.append_snapshot(&snapshot, compute_marker_hash(&snapshot))
            }
            MarkerMsg::HiFi(marker) => backend.append_hifi(&marker, compute_hifi_hash(&marker)),
            MarkerMsg::Dict(entry) => backend.put_dict(&entry, compute_dict_hash(&entry)),
            MarkerMsg::GapThen { gap, msg } => {
                backend.append_gap(&gap, compute_gap_hash(&gap))?;
                write_msg(backend, *msg)
            }
            MarkerMsg::Close => Ok(()),
        }
    }
}

#[cfg(madsim)]
mod imp {
    use std::{
        fmt::Debug,
        sync::{
            Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };

    use nautilus_core::time::AtomicTime;

    use super::{EventStoreError, MarkerBackend, MarkerMsg, MarkerWriterConfig, StreamDictEntry};
    use crate::{
        manifest::RunStatus,
        markers::{compute_dict_hash, compute_hifi_hash, compute_marker_hash},
    };

    /// Synchronous marker writer used under simulation.
    pub struct MarkerWriter {
        inner: Mutex<Inner>,
        last_submitted_seq: AtomicU64,
    }

    struct Inner {
        backend: Box<dyn MarkerBackend + Send>,
        closed: bool,
    }

    impl Debug for MarkerWriter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct(stringify!(MarkerWriter))
                .field(
                    "last_submitted_seq",
                    &self.last_submitted_seq.load(Ordering::Acquire),
                )
                .finish_non_exhaustive()
        }
    }

    impl MarkerWriter {
        /// Constructs a synchronous marker writer over `backend`.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Backend`] when the backend has no open run.
        #[allow(clippy::needless_pass_by_value)]
        pub fn spawn(
            backend: Box<dyn MarkerBackend + Send>,
            _clock: &'static AtomicTime,
            _config: MarkerWriterConfig,
        ) -> Result<Self, EventStoreError> {
            backend.manifest()?;
            Ok(Self {
                inner: Mutex::new(Inner {
                    backend,
                    closed: false,
                }),
                last_submitted_seq: AtomicU64::new(0),
            })
        }

        /// Commits the marker synchronously.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Closed`] when the writer is closed, and
        /// [`EventStoreError::Backend`] when `msg` is not a `Snapshot` or `HiFi` marker.
        ///
        /// # Panics
        ///
        /// Panics if the writer mutex is poisoned.
        pub fn submit(&self, msg: MarkerMsg, marker_seq: u64) -> Result<bool, EventStoreError> {
            let mut inner = self.inner.lock().expect("marker writer mutex poisoned");

            if inner.closed {
                return Err(EventStoreError::Closed);
            }

            match msg {
                MarkerMsg::Snapshot(snapshot) => inner
                    .backend
                    .append_snapshot(&snapshot, compute_marker_hash(&snapshot))?,
                MarkerMsg::HiFi(marker) => inner
                    .backend
                    .append_hifi(&marker, compute_hifi_hash(&marker))?,
                MarkerMsg::Dict(_) | MarkerMsg::Close | MarkerMsg::GapThen { .. } => {
                    return Err(EventStoreError::Backend(
                        "submit accepts Snapshot or HiFi marker messages".to_string(),
                    ));
                }
            }

            self.last_submitted_seq.store(marker_seq, Ordering::Release);
            Ok(true)
        }

        /// Commits the stream dictionary entry synchronously.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Closed`] when the writer is closed.
        ///
        /// # Panics
        ///
        /// Panics if the writer mutex is poisoned.
        pub fn put_dict(&self, entry: StreamDictEntry) -> Result<bool, EventStoreError> {
            let mut inner = self.inner.lock().expect("marker writer mutex poisoned");

            if inner.closed {
                return Err(EventStoreError::Closed);
            }

            inner.backend.put_dict(&entry, compute_dict_hash(&entry))?;
            Ok(true)
        }

        /// Seals the marker run.
        ///
        /// # Panics
        ///
        /// Panics if the writer mutex is poisoned.
        pub fn close(self) {
            let mut inner = self.inner.lock().expect("marker writer mutex poisoned");

            if !inner.closed {
                let _ = inner.backend.seal(RunStatus::Ended);
                inner.closed = true;
            }
        }
    }
}

pub use imp::MarkerWriter;

#[cfg(test)]
#[cfg(not(madsim))]
mod tests {
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use nautilus_core::{UnixNanos, time::get_atomic_clock_static};
    use rstest::rstest;

    use super::*;
    use crate::{
        error::EventStoreError,
        manifest::RunStatus,
        markers::{
            DataClass, MarkerGap, MarkerGapReason, MarkerManifest, MemoryMarkerBackend,
            StreamCursor, StreamDictEntry,
        },
    };

    fn manifest(run_id: &str) -> MarkerManifest {
        MarkerManifest {
            run_id: run_id.to_string(),
            enabled_classes: vec![DataClass::Quote, DataClass::Trade],
            high_fidelity: true,
            snapshot_count: 0,
            hifi_count: 0,
            gap_count: 0,
            dict_count: 0,
            status: RunStatus::Running,
        }
    }

    fn snapshot(marker_seq: u64) -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq,
            event_seq_before: marker_seq.saturating_sub(1),
            ts_init: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
            advanced: vec![StreamCursor {
                slot: 0,
                ts_init_hi: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
                count: marker_seq,
            }],
        }
    }

    fn hifi(marker_seq: u64) -> HiFiMarker {
        HiFiMarker {
            marker_seq,
            event_seq_before: marker_seq.saturating_sub(1),
            slot: 0,
            ts_event: UnixNanos::from(1_700_000_000_000_000_100 + marker_seq),
            ts_init: UnixNanos::from(1_700_000_000_000_000_200 + marker_seq),
            same_ts_ordinal: 0,
            record_fingerprint: [7u8; 32],
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

    #[derive(Debug)]
    struct BlockingMarkerBackend {
        inner: Arc<Mutex<MemoryMarkerBackend>>,
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        appends_seen: Arc<AtomicUsize>,
    }

    impl BlockingMarkerBackend {
        fn new(
            inner: Arc<Mutex<MemoryMarkerBackend>>,
            gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
            appends_seen: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                inner,
                gate,
                appends_seen,
            }
        }

        fn wait_for_release(&self) {
            let (lock, cvar) = &*self.gate;
            let mut released = lock.lock().expect("gate poisoned");

            while !*released {
                released = cvar.wait(released).expect("gate wait");
            }
        }
    }

    impl MarkerBackend for BlockingMarkerBackend {
        fn open_run(&mut self, _: MarkerManifest) -> Result<(), EventStoreError> {
            unreachable!("test wrapper does not forward open_run")
        }

        fn append_snapshot(
            &mut self,
            snapshot: &DataCursorSnapshot,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.appends_seen.fetch_add(1, Ordering::SeqCst);
            self.wait_for_release();
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .append_snapshot(snapshot, hash)
        }

        fn append_hifi(
            &mut self,
            marker: &HiFiMarker,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.appends_seen.fetch_add(1, Ordering::SeqCst);
            self.wait_for_release();
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .append_hifi(marker, hash)
        }

        fn append_gap(&mut self, gap: &MarkerGap, hash: [u8; 32]) -> Result<(), EventStoreError> {
            self.appends_seen.fetch_add(1, Ordering::SeqCst);
            self.wait_for_release();
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .append_gap(gap, hash)
        }

        fn put_dict(
            &mut self,
            entry: &StreamDictEntry,
            hash: [u8; 32],
        ) -> Result<(), EventStoreError> {
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .put_dict(entry, hash)
        }

        fn scan_snapshots(&self) -> Result<Vec<DataCursorSnapshot>, EventStoreError> {
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .scan_snapshots()
        }

        fn scan_hifi(&self) -> Result<Vec<HiFiMarker>, EventStoreError> {
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .scan_hifi()
        }

        fn scan_gaps(&self) -> Result<Vec<MarkerGap>, EventStoreError> {
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .scan_gaps()
        }

        fn scan_dict(&self) -> Result<Vec<StreamDictEntry>, EventStoreError> {
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .scan_dict()
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.inner
                .lock()
                .expect("inner marker poisoned")
                .seal(status)
        }

        fn manifest(&self) -> Result<MarkerManifest, EventStoreError> {
            self.inner.lock().expect("inner marker poisoned").manifest()
        }
    }

    fn wait_until(mut predicate: impl FnMut() -> bool, label: &str) {
        let start = Instant::now();

        while !predicate() {
            assert!(
                start.elapsed() < Duration::from_millis(500),
                "timed out waiting for {label}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[rstest]
    fn submitted_snapshots_reach_backend() {
        let (wrapper, shared) = SharedMemoryMarker::new();
        shared
            .lock()
            .expect("shared marker")
            .open_run(manifest("run-snapshots"))
            .expect("open marker run");
        let config = MarkerWriterConfig {
            channel_capacity: 16,
            max_batch: 2,
            max_latency: Duration::from_secs(30),
        };

        let writer = MarkerWriter::spawn(Box::new(wrapper), get_atomic_clock_static(), config)
            .expect("spawn marker writer");
        let s1 = snapshot(1);
        let s2 = snapshot(2);

        assert!(
            writer
                .submit(MarkerMsg::Snapshot(s1.clone()), s1.marker_seq)
                .expect("submit first")
        );
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(s2.clone()), s2.marker_seq)
                .expect("submit second")
        );
        writer.close();

        let backend = shared.lock().expect("shared marker");
        assert_eq!(
            backend.scan_snapshots().expect("scan snapshots"),
            vec![s1, s2]
        );
    }

    #[rstest]
    fn put_dict_waits_for_capacity_and_persists_metadata() {
        let inner = Arc::new(Mutex::new(MemoryMarkerBackend::new()));
        inner
            .lock()
            .expect("inner marker")
            .open_run(manifest("run-dict-capacity"))
            .expect("open marker run");
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let appends_seen = Arc::new(AtomicUsize::new(0));
        let backend = BlockingMarkerBackend::new(
            Arc::clone(&inner),
            Arc::clone(&gate),
            Arc::clone(&appends_seen),
        );

        let writer = MarkerWriter::spawn(
            Box::new(backend),
            get_atomic_clock_static(),
            MarkerWriterConfig {
                channel_capacity: 1,
                max_batch: 1,
                max_latency: Duration::from_secs(30),
            },
        )
        .expect("spawn marker writer");

        let first = snapshot(1);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(first.clone()), first.marker_seq)
                .expect("submit first")
        );
        wait_until(
            || appends_seen.load(Ordering::SeqCst) == 1,
            "writer to block in backend append",
        );

        let second = snapshot(2);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(second.clone()), second.marker_seq)
                .expect("submit second")
        );

        let entry = StreamDictEntry {
            slot: 1,
            data_cls: DataClass::Trade,
            identifier: "BTCUSDT.BINANCE".to_string(),
        };
        let gate_for_release = Arc::clone(&gate);

        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            let (lock, cvar) = &*gate_for_release;
            *lock.lock().expect("gate") = true;
            cvar.notify_all();
        });

        assert!(writer.put_dict(entry.clone()).expect("put dict"));
        release.join().expect("release gate");
        writer.close();

        let backend = inner.lock().expect("inner marker");
        assert_eq!(backend.scan_dict().expect("scan dict"), vec![entry]);
        assert_eq!(
            backend
                .scan_snapshots()
                .expect("scan snapshots")
                .into_iter()
                .map(|snapshot| snapshot.marker_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[rstest]
    fn spawn_requires_open_marker_run() {
        let backend = MemoryMarkerBackend::new();

        let err = MarkerWriter::spawn(
            Box::new(backend),
            get_atomic_clock_static(),
            MarkerWriterConfig::default(),
        )
        .expect_err("spawn must reject a backend with no open run");

        match err {
            EventStoreError::Backend(msg) => {
                assert!(msg.contains("no run open"), "msg was: {msg}");
            }
            other => panic!("expected Backend, was {other:?}"),
        }
    }

    #[rstest]
    fn latency_window_flushes_marker_before_close() {
        let (wrapper, shared) = SharedMemoryMarker::new();
        shared
            .lock()
            .expect("shared marker")
            .open_run(manifest("run-latency"))
            .expect("open marker run");

        let writer = MarkerWriter::spawn(
            Box::new(wrapper),
            get_atomic_clock_static(),
            MarkerWriterConfig {
                channel_capacity: 16,
                max_batch: 100,
                max_latency: Duration::from_millis(20),
            },
        )
        .expect("spawn marker writer");
        let snap = snapshot(1);

        assert!(
            writer
                .submit(MarkerMsg::Snapshot(snap.clone()), snap.marker_seq)
                .expect("submit")
        );
        wait_until(
            || {
                shared
                    .lock()
                    .expect("shared marker")
                    .scan_snapshots()
                    .expect("scan")
                    .len()
                    == 1
            },
            "latency flush",
        );
        writer.close();

        assert_eq!(
            shared
                .lock()
                .expect("shared marker")
                .scan_snapshots()
                .expect("scan snapshots"),
            vec![snap]
        );
    }

    #[rstest]
    fn overflow_drops_and_records_single_gap() {
        let inner = Arc::new(Mutex::new(MemoryMarkerBackend::new()));
        inner
            .lock()
            .expect("inner marker")
            .open_run(manifest("run-overflow"))
            .expect("open marker run");
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let appends_seen = Arc::new(AtomicUsize::new(0));
        let backend = BlockingMarkerBackend::new(
            Arc::clone(&inner),
            Arc::clone(&gate),
            Arc::clone(&appends_seen),
        );

        let writer = MarkerWriter::spawn(
            Box::new(backend),
            get_atomic_clock_static(),
            MarkerWriterConfig {
                channel_capacity: 1,
                max_batch: 1,
                max_latency: Duration::from_secs(30),
            },
        )
        .expect("spawn marker writer");

        let first = snapshot(1);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(first.clone()), first.marker_seq)
                .expect("submit first")
        );
        wait_until(
            || appends_seen.load(Ordering::SeqCst) == 1,
            "writer to block in backend append",
        );

        let second = snapshot(2);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(second.clone()), second.marker_seq)
                .expect("submit second")
        );

        let start = Instant::now();

        for marker_seq in 3..=5 {
            let dropped = snapshot(marker_seq);
            assert!(
                !writer
                    .submit(MarkerMsg::Snapshot(dropped), marker_seq)
                    .expect("submit drop")
            );
        }
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "overflow submits must not block"
        );

        let (lock, cvar) = &*gate;
        *lock.lock().expect("gate") = true;
        cvar.notify_all();

        wait_until(
            || {
                inner
                    .lock()
                    .expect("inner marker")
                    .scan_snapshots()
                    .expect("scan")
                    .len()
                    == 2
            },
            "first two snapshots to drain",
        );

        let sixth = snapshot(6);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(sixth.clone()), sixth.marker_seq)
                .expect("submit after overflow")
        );
        writer.close();

        let backend = inner.lock().expect("inner marker");
        let gaps = backend.scan_gaps().expect("scan gaps");
        assert_eq!(
            gaps,
            vec![MarkerGap {
                from_marker_seq: 3,
                to_marker_seq: 5,
                reason: MarkerGapReason::Overflow,
            }]
        );
        assert_eq!(
            backend
                .scan_snapshots()
                .expect("scan snapshots")
                .into_iter()
                .map(|snapshot| snapshot.marker_seq)
                .collect::<Vec<_>>(),
            vec![1, 2, 6]
        );
    }

    #[rstest]
    fn overflow_gap_precedes_next_hifi_marker() {
        let inner = Arc::new(Mutex::new(MemoryMarkerBackend::new()));
        inner
            .lock()
            .expect("inner marker")
            .open_run(manifest("run-overflow-hifi"))
            .expect("open marker run");
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let appends_seen = Arc::new(AtomicUsize::new(0));
        let backend = BlockingMarkerBackend::new(
            Arc::clone(&inner),
            Arc::clone(&gate),
            Arc::clone(&appends_seen),
        );

        let writer = MarkerWriter::spawn(
            Box::new(backend),
            get_atomic_clock_static(),
            MarkerWriterConfig {
                channel_capacity: 1,
                max_batch: 1,
                max_latency: Duration::from_secs(30),
            },
        )
        .expect("spawn marker writer");

        let first = snapshot(1);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(first.clone()), first.marker_seq)
                .expect("submit first")
        );
        wait_until(
            || appends_seen.load(Ordering::SeqCst) == 1,
            "writer to block in backend append",
        );

        let second = snapshot(2);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(second.clone()), second.marker_seq)
                .expect("submit second")
        );

        for marker_seq in 3..=5 {
            let dropped = snapshot(marker_seq);
            assert!(
                !writer
                    .submit(MarkerMsg::Snapshot(dropped), marker_seq)
                    .expect("submit drop")
            );
        }

        let (lock, cvar) = &*gate;
        *lock.lock().expect("gate") = true;
        cvar.notify_all();

        wait_until(
            || {
                inner
                    .lock()
                    .expect("inner marker")
                    .scan_snapshots()
                    .expect("scan")
                    .len()
                    == 2
            },
            "first two snapshots to drain",
        );

        let marker = hifi(6);
        assert!(
            writer
                .submit(MarkerMsg::HiFi(marker.clone()), marker.marker_seq)
                .expect("submit hifi after overflow")
        );
        writer.close();

        let backend = inner.lock().expect("inner marker");
        assert_eq!(
            backend.scan_gaps().expect("scan gaps"),
            vec![MarkerGap {
                from_marker_seq: 3,
                to_marker_seq: 5,
                reason: MarkerGapReason::Overflow,
            }]
        );
        assert_eq!(backend.scan_hifi().expect("scan hifi"), vec![marker]);
    }

    #[rstest]
    fn close_records_pending_drop_as_writer_closed_gap() {
        let inner = Arc::new(Mutex::new(MemoryMarkerBackend::new()));
        inner
            .lock()
            .expect("inner marker")
            .open_run(manifest("run-close-gap"))
            .expect("open marker run");
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let appends_seen = Arc::new(AtomicUsize::new(0));
        let backend = BlockingMarkerBackend::new(
            Arc::clone(&inner),
            Arc::clone(&gate),
            Arc::clone(&appends_seen),
        );

        let writer = MarkerWriter::spawn(
            Box::new(backend),
            get_atomic_clock_static(),
            MarkerWriterConfig {
                channel_capacity: 1,
                max_batch: 1,
                max_latency: Duration::from_secs(30),
            },
        )
        .expect("spawn marker writer");

        let first = snapshot(1);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(first.clone()), first.marker_seq)
                .expect("submit first")
        );
        wait_until(
            || appends_seen.load(Ordering::SeqCst) == 1,
            "writer to block in backend append",
        );

        let second = snapshot(2);
        assert!(
            writer
                .submit(MarkerMsg::Snapshot(second.clone()), second.marker_seq)
                .expect("submit second")
        );

        for marker_seq in 3..=4 {
            let dropped = snapshot(marker_seq);
            assert!(
                !writer
                    .submit(MarkerMsg::Snapshot(dropped), marker_seq)
                    .expect("submit drop")
            );
        }

        let close_thread = std::thread::spawn(move || writer.close());
        std::thread::sleep(Duration::from_millis(20));
        let (lock, cvar) = &*gate;
        *lock.lock().expect("gate") = true;
        cvar.notify_all();
        close_thread.join().expect("close thread");

        let backend = inner.lock().expect("inner marker");
        assert_eq!(
            backend.scan_gaps().expect("scan gaps"),
            vec![MarkerGap {
                from_marker_seq: 3,
                to_marker_seq: 4,
                reason: MarkerGapReason::WriterClosed,
            }]
        );
        assert_eq!(
            backend
                .scan_snapshots()
                .expect("scan snapshots")
                .into_iter()
                .map(|snapshot| snapshot.marker_seq)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            backend.manifest().expect("manifest").status,
            RunStatus::Ended
        );
    }

    #[rstest]
    fn close_drains_and_seals() {
        let (wrapper, shared) = SharedMemoryMarker::new();
        shared
            .lock()
            .expect("shared marker")
            .open_run(manifest("run-close"))
            .expect("open marker run");
        let config = MarkerWriterConfig {
            channel_capacity: 16,
            max_batch: 10,
            max_latency: Duration::from_secs(30),
        };

        let writer = MarkerWriter::spawn(Box::new(wrapper), get_atomic_clock_static(), config)
            .expect("spawn marker writer");
        let marker = hifi(1);

        assert!(
            writer
                .submit(MarkerMsg::HiFi(marker.clone()), marker.marker_seq)
                .expect("submit hifi")
        );
        writer.close();

        let backend = shared.lock().expect("shared marker");
        assert_eq!(backend.scan_hifi().expect("scan hifi"), vec![marker]);
        assert_eq!(
            backend.manifest().expect("manifest").status,
            RunStatus::Ended
        );
    }
}

#[cfg(test)]
#[cfg(madsim)]
mod madsim_tests {
    use std::sync::{Arc, Mutex};

    use nautilus_core::{UnixNanos, time::get_atomic_clock_static};
    use rstest::rstest;

    use super::*;
    use crate::{
        error::EventStoreError,
        manifest::RunStatus,
        markers::{
            DataClass, MarkerGap, MarkerManifest, MemoryMarkerBackend, StreamCursor,
            StreamDictEntry,
        },
    };

    fn manifest(run_id: &str) -> MarkerManifest {
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

    fn snapshot(marker_seq: u64) -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq,
            event_seq_before: marker_seq.saturating_sub(1),
            ts_init: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
            advanced: vec![StreamCursor {
                slot: 0,
                ts_init_hi: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
                count: marker_seq,
            }],
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

    #[rstest]
    fn submit_persists_synchronously_and_close_seals() {
        let (wrapper, shared) = SharedMemoryMarker::new();
        shared
            .lock()
            .expect("shared marker")
            .open_run(manifest("run-madsim"))
            .expect("open marker run");

        let writer = MarkerWriter::spawn(
            Box::new(wrapper),
            get_atomic_clock_static(),
            MarkerWriterConfig::default(),
        )
        .expect("spawn marker writer");
        let snap = snapshot(1);

        assert!(
            writer
                .submit(MarkerMsg::Snapshot(snap.clone()), snap.marker_seq)
                .expect("submit")
        );
        assert_eq!(
            shared
                .lock()
                .expect("shared marker")
                .scan_snapshots()
                .expect("scan snapshots"),
            vec![snap]
        );

        writer.close();

        assert_eq!(
            shared
                .lock()
                .expect("shared marker")
                .manifest()
                .expect("manifest")
                .status,
            RunStatus::Ended
        );
    }
}
