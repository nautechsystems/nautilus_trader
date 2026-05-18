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

//! Dedicated writer for the event store.
//!
//! The writer owns a single backend instance and exposes a thread-safe `submit` entry
//! point. Captured entries enter via a bounded `std::sync::mpsc::sync_channel` and the
//! writer thread drains them into batched, atomic `append_batch` commits. The
//! high-watermark only advances on durable acknowledgement; a stalled submit or a
//! backend-side disk/corruption failure fires the registered halt callback.
//!
//! Under `cfg(madsim)` the writer drops the channel and the dedicated thread, mirroring
//! the logger's simulation pattern: submits commit synchronously on the calling thread so
//! tests assert against an authoritative in-process log without thread scheduling.

// `batcher` carries thread-loop helpers gated out under cfg(madsim) since the
// synchronous path bypasses the channel and the run loop, but `build_append_entry`
// is reused in both paths so the module stays compiled either way.
mod batcher;
pub mod halt;

use std::time::Duration;

use bytes::Bytes;
pub use halt::{HaltCallback, HaltReason, noop_halt};
use nautilus_core::UnixNanos;

use crate::{
    backend::IndexKey,
    entry::{PayloadType, Topic},
    headers::Headers,
    snapshot::SnapshotAnchor,
};

/// Default channel capacity for entries pending the writer thread.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 10_000;
/// Default maximum number of entries collected before forcing a commit.
pub const DEFAULT_MAX_BATCH_ENTRIES: usize = 100;
/// Default maximum time a batch may accumulate before forcing a commit.
pub const DEFAULT_MAX_BATCH_LATENCY: Duration = Duration::from_millis(5);
/// Default submit-side stall ceiling that fires the halt callback.
pub const DEFAULT_HALT_THRESHOLD: Duration = Duration::from_millis(250);

/// Configuration knobs for the writer.
#[derive(Clone, Debug)]
pub struct WriterConfig {
    /// Capacity of the bounded `sync_channel` between submit and the writer thread.
    pub channel_capacity: usize,
    /// Maximum entries collected before a commit is forced.
    pub max_batch_entries: usize,
    /// Maximum time a batch may accumulate before a commit is forced.
    pub max_batch_latency: Duration,
    /// Submit-side stall ceiling. A submit that blocks longer than this fires the halt
    /// callback once and returns [`SubmitError::HaltSignaled`].
    pub halt_threshold: Duration,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            max_batch_entries: DEFAULT_MAX_BATCH_ENTRIES,
            max_batch_latency: DEFAULT_MAX_BATCH_LATENCY,
            halt_threshold: DEFAULT_HALT_THRESHOLD,
        }
    }
}

/// An unsealed entry handed to [`EventStoreWriter::submit`].
///
/// `seq`, `ts_publish`, and `entry_hash` are stamped by the writer; everything else is the
/// captured message identity plus encoder output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryDraft {
    /// First-class correlation headers.
    pub headers: Headers,
    /// The bus topic the entry was captured on.
    pub topic: Topic,
    /// The canonical payload type tag.
    pub payload_type: PayloadType,
    /// The encoded payload bytes.
    pub payload: Bytes,
    /// The domain timestamp from `AtomicTime`.
    pub ts_init: UnixNanos,
    /// Sidecar index keys produced by the encoder.
    pub index_keys: Vec<IndexKey>,
}

impl EntryDraft {
    /// Creates a new [`EntryDraft`] with no sidecar index keys.
    #[must_use]
    pub const fn without_indices(
        headers: Headers,
        topic: Topic,
        payload_type: PayloadType,
        payload: Bytes,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            headers,
            topic,
            payload_type,
            payload,
            ts_init,
            index_keys: Vec::new(),
        }
    }
}

/// Errors returned by [`EventStoreWriter::submit`].
#[derive(Debug, thiserror::Error)]
pub enum SubmitError {
    /// The writer is shut down or the writer thread has exited.
    #[error("writer is closed")]
    Closed,
    /// The submit blocked longer than the configured halt threshold; the halt callback
    /// has been fired.
    #[error("submit stalled for {stalled_for:?}, halt threshold {threshold:?}")]
    HaltSignaled {
        /// How long the submit blocked before signaling halt.
        stalled_for: Duration,
        /// The configured threshold the stall exceeded.
        threshold: Duration,
    },
}

#[cfg(not(madsim))]
mod imp {
    use std::{
        fmt::Debug,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc::{self, RecvTimeoutError, SyncSender, TrySendError},
        },
        thread::{self, JoinHandle},
        time::{Duration, Instant},
    };

    use nautilus_core::time::AtomicTime;

    use super::{
        EntryDraft, SnapshotAnchor, SubmitError, WriterConfig,
        batcher::{self, WriterMessage},
        halt::{HaltCallback, HaltReason},
    };
    use crate::{backend::EventStore, error::EventStoreError};

    const WRITER_THREAD_NAME: &str = "event-store-writer";
    const SUBMIT_RETRY_INTERVAL: Duration = Duration::from_micros(100);

    /// The dedicated event store writer.
    pub struct EventStoreWriter {
        tx: Option<SyncSender<WriterMessage>>,
        handle: Option<JoinHandle<()>>,
        high_watermark: Arc<AtomicU64>,
        halt: HaltCallback,
        halt_threshold: Duration,
        // Set once when a backpressure stall fires the halt callback. Subsequent submits
        // observe this and return Closed instead of re-entering the retry loop, so the
        // run cannot keep accepting entries after a fail-stop signal.
        halted: AtomicBool,
        clock: &'static AtomicTime,
    }

    impl Debug for EventStoreWriter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct(stringify!(EventStoreWriter))
                .field(
                    "high_watermark",
                    &self.high_watermark.load(Ordering::Acquire),
                )
                .field("halt_threshold", &self.halt_threshold)
                .field("halted", &self.halted.load(Ordering::Acquire))
                .field("tx_attached", &self.tx.is_some())
                .finish_non_exhaustive()
        }
    }

    impl EventStoreWriter {
        /// Spawns the writer thread and takes ownership of `backend`.
        ///
        /// The backend must already have an open run; the writer reads its current
        /// high-watermark to seed the next assigned `seq`.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Backend`] when the backend has no open run or when
        /// the writer thread cannot be spawned.
        #[allow(clippy::needless_pass_by_value)] // taken by value so callers don't outlive the writer's copy
        pub fn spawn(
            backend: Box<dyn EventStore + Send>,
            clock: &'static AtomicTime,
            halt: HaltCallback,
            config: WriterConfig,
        ) -> Result<Self, EventStoreError> {
            let initial_hwm = backend.high_watermark()?;
            let high_watermark = Arc::new(AtomicU64::new(initial_hwm));
            let (tx, rx) = mpsc::sync_channel::<WriterMessage>(config.channel_capacity);

            let watermark_for_thread = Arc::clone(&high_watermark);
            let halt_for_thread = Arc::clone(&halt);
            let halt_threshold = config.halt_threshold;
            let config_for_thread = config;

            let handle = thread::Builder::new()
                .name(WRITER_THREAD_NAME.to_string())
                .spawn(move || {
                    batcher::run(
                        backend,
                        rx,
                        config_for_thread,
                        halt_for_thread,
                        watermark_for_thread,
                        clock,
                    );
                })
                .map_err(|e| EventStoreError::Backend(format!("spawn writer thread: {e}")))?;

            Ok(Self {
                tx: Some(tx),
                handle: Some(handle),
                high_watermark,
                halt,
                halt_threshold,
                halted: AtomicBool::new(false),
                clock,
            })
        }

        /// Submits a captured entry. Stamps `ts_publish` from the clock at receive time
        /// and hands the draft to the writer thread.
        ///
        /// Blocks (with retry) when the channel is full. If the cumulative wait exceeds
        /// the halt threshold, fires the halt callback once and returns
        /// [`SubmitError::HaltSignaled`]; subsequent submits return [`SubmitError::Closed`]
        /// without blocking.
        ///
        /// Under concurrent submitters, two threads stalled at the threshold can each
        /// reach the halt-fire path before either sets the halted flag, so the halt
        /// callback may run more than once and a submit already past the entry check
        /// may briefly race with another thread's halt-fire; the kernel's fail-stop
        /// callback must therefore be idempotent.
        ///
        /// # Errors
        ///
        /// Returns [`SubmitError::Closed`] when the writer is shut down, the writer
        /// thread has exited, or a prior submit already fired a fail-stop halt; returns
        /// [`SubmitError::HaltSignaled`] when this submit's stall first crosses the
        /// configured halt threshold.
        pub fn submit(&self, draft: EntryDraft) -> Result<(), SubmitError> {
            // Refuse further entries once a backpressure halt has been signaled, even if
            // the channel later drains. The kernel's halt callback is the fail-stop
            // signal, and the writer's local invariant is that halt is terminal for the
            // run.
            if self.halted.load(Ordering::Acquire) {
                return Err(SubmitError::Closed);
            }

            let tx = self.tx.as_ref().ok_or(SubmitError::Closed)?;
            let ts_publish = self.clock.get_time_ns();
            let mut pending = WriterMessage::Entry { draft, ts_publish };
            let start = Instant::now();

            // Check elapsed before each try_send (including after a sleep) so that a
            // stall which exceeds the threshold fires halt even when the next attempt
            // would have succeeded. The first iteration's elapsed is ~0, so it falls
            // through to try_send.
            loop {
                let elapsed = start.elapsed();

                if elapsed >= self.halt_threshold {
                    self.halted.store(true, Ordering::Release);
                    let reason = HaltReason::BackpressureStall {
                        stalled_for: elapsed,
                        threshold: self.halt_threshold,
                    };
                    (self.halt)(reason);
                    return Err(SubmitError::HaltSignaled {
                        stalled_for: elapsed,
                        threshold: self.halt_threshold,
                    });
                }

                match tx.try_send(pending) {
                    Ok(()) => return Ok(()),
                    Err(TrySendError::Full(returned)) => {
                        pending = returned;
                        thread::sleep(SUBMIT_RETRY_INTERVAL);
                    }
                    Err(TrySendError::Disconnected(_)) => return Err(SubmitError::Closed),
                }
            }
        }

        /// Returns the largest seq durably acknowledged by the backend.
        ///
        /// Updated only after a successful `append_batch` ack; reflects what is safe to
        /// anchor a snapshot against.
        #[must_use]
        pub fn high_watermark(&self) -> u64 {
            self.high_watermark.load(Ordering::Acquire)
        }

        /// Flushes pending entries and records a snapshot anchor at the durable
        /// high-watermark.
        ///
        /// The cache owns `blob_ref` and `content_hash`; the writer derives the
        /// high-watermark only after earlier submitted entries have committed, so the
        /// anchor never points past durable event-store state.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Closed`] when the writer is closed or halted, and
        /// forwards backend errors when flushing pending entries or recording the anchor
        /// fails.
        pub fn record_snapshot_anchor(
            &self,
            blob_ref: impl Into<String>,
            content_hash: impl Into<String>,
        ) -> Result<SnapshotAnchor, EventStoreError> {
            if self.halted.load(Ordering::Acquire) {
                return Err(EventStoreError::Closed);
            }

            let tx = self.tx.as_ref().ok_or(EventStoreError::Closed)?;
            let (ack_tx, ack_rx) = mpsc::sync_channel::<Result<SnapshotAnchor, EventStoreError>>(1);
            let mut pending = WriterMessage::RecordSnapshotAnchor {
                blob_ref: blob_ref.into(),
                content_hash: content_hash.into(),
                ack: ack_tx,
            };
            let start = Instant::now();

            loop {
                let elapsed = start.elapsed();

                if elapsed >= self.halt_threshold {
                    self.halted.store(true, Ordering::Release);
                    let reason = HaltReason::BackpressureStall {
                        stalled_for: elapsed,
                        threshold: self.halt_threshold,
                    };
                    (self.halt)(reason);
                    return Err(EventStoreError::Backend(format!(
                        "snapshot anchor submit stalled for {elapsed:?}, halt threshold {:?}",
                        self.halt_threshold,
                    )));
                }

                match tx.try_send(pending) {
                    Ok(()) => break,
                    Err(TrySendError::Full(returned)) => {
                        pending = returned;
                        thread::sleep(SUBMIT_RETRY_INTERVAL);
                    }
                    Err(TrySendError::Disconnected(_)) => return Err(EventStoreError::Closed),
                }
            }

            match ack_rx.recv_timeout(self.halt_threshold) {
                Ok(result) => result,
                Err(RecvTimeoutError::Timeout) => {
                    let elapsed = start.elapsed();
                    self.halted.store(true, Ordering::Release);
                    let reason = HaltReason::BackpressureStall {
                        stalled_for: elapsed,
                        threshold: self.halt_threshold,
                    };
                    (self.halt)(reason);
                    Err(EventStoreError::Backend(format!(
                        "snapshot anchor ack stalled for {elapsed:?}, halt threshold {:?}",
                        self.halt_threshold,
                    )))
                }
                Err(RecvTimeoutError::Disconnected) => Err(EventStoreError::Backend(
                    "snapshot anchor ack channel disconnected".to_string(),
                )),
            }
        }

        /// Drains the channel, commits `run_ended` as the final entry, and seals the
        /// manifest with [`crate::manifest::RunStatus::Ended`].
        ///
        /// Consumes the writer; further submits are unrepresentable.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError`] when the writer thread fails to commit the final
        /// batch, when seal fails, or when the writer thread panicked.
        pub fn close(mut self, run_ended: EntryDraft) -> Result<u64, EventStoreError> {
            let tx = self
                .tx
                .take()
                .ok_or_else(|| EventStoreError::Backend("writer already closed".to_string()))?;

            let (ack_tx, ack_rx) = mpsc::sync_channel::<Result<u64, EventStoreError>>(1);
            tx.send(WriterMessage::Close {
                run_ended,
                ack: ack_tx,
            })
            .map_err(|_| EventStoreError::Backend("writer thread disconnected".to_string()))?;
            // Drop the producer so the writer's recv loop will see Disconnected after it
            // finishes Close handling, even if the path through Close returns earlier.
            drop(tx);

            let result = ack_rx.recv().map_err(|_| {
                EventStoreError::Backend("writer ack channel disconnected".to_string())
            })?;

            if let Some(handle) = self.handle.take() {
                handle
                    .join()
                    .map_err(|_| EventStoreError::Backend("writer thread panicked".to_string()))?;
            }
            result
        }
    }

    impl Drop for EventStoreWriter {
        fn drop(&mut self) {
            // Implicit drop without close(): release the producer so the writer thread
            // exits cleanly, leaving the manifest unsealed so a later open of the same
            // run observes a CrashedPredecessor.
            self.tx.take();

            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(madsim)]
mod imp {
    use std::{
        fmt::Debug,
        sync::{
            Arc, Mutex,
            atomic::{AtomicU64, Ordering},
        },
    };

    use nautilus_core::time::AtomicTime;

    use super::{
        EntryDraft, SnapshotAnchor, SubmitError, WriterConfig, batcher,
        halt::{HaltCallback, HaltReason},
    };
    use crate::{backend::EventStore, error::EventStoreError, manifest::RunStatus};

    /// Synchronous, in-thread writer used under simulation.
    ///
    /// The dedicated thread and bounded channel are dropped: each `submit` commits a
    /// single-entry batch on the calling thread so tests can assert against the
    /// authoritative in-process log without thread scheduling.
    pub struct EventStoreWriter {
        inner: Mutex<Inner>,
        high_watermark: Arc<AtomicU64>,
        halt: HaltCallback,
        clock: &'static AtomicTime,
    }

    impl Debug for EventStoreWriter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct(stringify!(EventStoreWriter))
                .field(
                    "high_watermark",
                    &self.high_watermark.load(Ordering::Acquire),
                )
                .finish_non_exhaustive()
        }
    }

    struct Inner {
        backend: Box<dyn EventStore + Send>,
        next_seq: u64,
        closed: bool,
    }

    impl EventStoreWriter {
        /// Constructs a synchronous writer over `backend`.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Backend`] when the backend has no open run.
        #[allow(clippy::needless_pass_by_value)] // taken by value so callers don't outlive the writer's copy
        pub fn spawn(
            backend: Box<dyn EventStore + Send>,
            clock: &'static AtomicTime,
            halt: HaltCallback,
            _config: WriterConfig,
        ) -> Result<Self, EventStoreError> {
            let initial_hwm = backend.high_watermark()?;
            let high_watermark = Arc::new(AtomicU64::new(initial_hwm));
            let inner = Inner {
                backend,
                next_seq: initial_hwm + 1,
                closed: false,
            };

            Ok(Self {
                inner: Mutex::new(inner),
                high_watermark,
                halt,
                clock,
            })
        }

        /// Commits `draft` synchronously as a single-entry batch.
        ///
        /// # Errors
        ///
        /// Returns [`SubmitError::Closed`] if the writer has been closed or fail-stopped.
        ///
        /// # Panics
        ///
        /// Panics if the internal mutex is poisoned by a panic on a prior submit.
        pub fn submit(&self, draft: EntryDraft) -> Result<(), SubmitError> {
            let mut inner = self.inner.lock().expect("writer mutex poisoned");

            if inner.closed {
                return Err(SubmitError::Closed);
            }

            let ts_publish = self.clock.get_time_ns();
            let seq = inner.next_seq;
            let append = batcher::build_append_entry(draft, ts_publish, seq);

            match inner.backend.append_batch(std::slice::from_ref(&append)) {
                Ok(new_hwm) => {
                    inner.next_seq = seq + 1;
                    self.high_watermark.store(new_hwm, Ordering::Release);
                    Ok(())
                }
                Err(e) => {
                    (self.halt)(HaltReason::from_backend_error(&e));
                    inner.closed = true;
                    Err(SubmitError::Closed)
                }
            }
        }

        /// Returns the largest seq durably acknowledged by the backend.
        #[must_use]
        pub fn high_watermark(&self) -> u64 {
            self.high_watermark.load(Ordering::Acquire)
        }

        /// Records a snapshot anchor at the current durable high-watermark.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError::Closed`] when the writer has closed, and forwards
        /// backend errors when recording the anchor fails.
        ///
        /// # Panics
        ///
        /// Panics if the internal mutex is poisoned by a panic on a prior submit.
        pub fn record_snapshot_anchor(
            &self,
            blob_ref: impl Into<String>,
            content_hash: impl Into<String>,
        ) -> Result<SnapshotAnchor, EventStoreError> {
            let mut inner = self.inner.lock().expect("writer mutex poisoned");

            if inner.closed {
                return Err(EventStoreError::Closed);
            }

            let anchor = SnapshotAnchor::new(
                self.high_watermark.load(Ordering::Acquire),
                blob_ref,
                content_hash,
            );

            match inner.backend.record_snapshot_anchor(anchor.clone()) {
                Ok(()) => Ok(anchor),
                Err(e) => {
                    (self.halt)(HaltReason::from_backend_error(&e));
                    inner.closed = true;
                    Err(e)
                }
            }
        }

        /// Commits `run_ended` synchronously as the final entry and seals the manifest.
        ///
        /// # Errors
        ///
        /// Returns [`EventStoreError`] when the final commit or seal fails.
        ///
        /// # Panics
        ///
        /// Panics if the internal mutex is poisoned by a panic on a prior submit.
        pub fn close(self, run_ended: EntryDraft) -> Result<u64, EventStoreError> {
            let mut inner = self.inner.lock().expect("writer mutex poisoned");

            if inner.closed {
                return Err(EventStoreError::Backend(
                    "writer already closed".to_string(),
                ));
            }

            let ts_publish = self.clock.get_time_ns();
            let seq = inner.next_seq;
            let append = batcher::build_append_entry(run_ended, ts_publish, seq);

            match inner.backend.append_batch(std::slice::from_ref(&append)) {
                Ok(new_hwm) => {
                    inner.next_seq = seq + 1;
                    self.high_watermark.store(new_hwm, Ordering::Release);
                }
                Err(e) => {
                    (self.halt)(HaltReason::from_backend_error(&e));
                    inner.closed = true;
                    return Err(e);
                }
            }

            match inner.backend.seal(RunStatus::Ended) {
                Ok(()) => {
                    inner.closed = true;
                    Ok(self.high_watermark.load(Ordering::Acquire))
                }
                Err(e) => {
                    (self.halt)(HaltReason::from_backend_error(&e));
                    inner.closed = true;
                    Err(e)
                }
            }
        }
    }
}

pub use imp::EventStoreWriter;

#[cfg(test)]
#[cfg(not(madsim))]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_core::{UnixNanos, time::get_atomic_clock_static};
    use rstest::{fixture, rstest};
    use ustr::Ustr;

    use super::*;
    use crate::{
        backend::{AppendEntry, EventStore, IndexKind, MemoryBackend, ScanDirection},
        entry::EventStoreEntry,
        error::EventStoreError,
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

    fn entry_draft(ts_init: u64) -> EntryDraft {
        EntryDraft {
            headers: Headers::empty(),
            topic: "exec.command.SubmitOrder".into(),
            payload_type: Ustr::from("SubmitOrder"),
            payload: Bytes::from_static(b"\x01\x02\x03\x04"),
            ts_init: UnixNanos::from(ts_init),
            index_keys: Vec::new(),
        }
    }

    fn run_ended_draft() -> EntryDraft {
        EntryDraft {
            headers: Headers::empty(),
            topic: "run.lifecycle.RunEnded".into(),
            payload_type: Ustr::from("RunEnded"),
            payload: Bytes::new(),
            ts_init: UnixNanos::from(9_999),
            index_keys: Vec::new(),
        }
    }

    /// Wraps `MemoryBackend` so tests can read the same instance the writer thread
    /// commits into.
    #[derive(Debug)]
    struct SharedMemory(Arc<Mutex<MemoryBackend>>);

    impl SharedMemory {
        fn new() -> (Self, Arc<Mutex<MemoryBackend>>) {
            let arc = Arc::new(Mutex::new(MemoryBackend::new()));
            (Self(Arc::clone(&arc)), arc)
        }
    }

    impl EventStore for SharedMemory {
        fn open_run(&mut self, _: RunManifest) -> Result<(), EventStoreError> {
            // Tests open the underlying backend directly.
            unreachable!("test wrapper does not forward open_run")
        }

        fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .append_batch(entries)
        }

        fn scan_range(
            &self,
            from: u64,
            to: u64,
            direction: ScanDirection,
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .scan_range(from, to, direction)
        }

        fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            self.0.lock().expect("shared memory poisoned").scan_seq(seq)
        }

        fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .lookup(kind, key)
        }

        fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .iter_index_keys(kind)
        }

        fn record_snapshot_anchor(
            &mut self,
            anchor: SnapshotAnchor,
        ) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .record_snapshot_anchor(anchor)
        }

        fn latest_snapshot_anchor(&self) -> Result<Option<SnapshotAnchor>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .latest_snapshot_anchor()
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.0.lock().expect("shared memory poisoned").seal(status)
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            self.0.lock().expect("shared memory poisoned").manifest()
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .high_watermark()
        }
    }

    /// `EventStore` wrapper that blocks `append_batch` until a release flag flips.
    #[derive(Debug)]
    struct BlockingBackend {
        inner: Arc<Mutex<MemoryBackend>>,
        gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
        appends_seen: Arc<AtomicUsize>,
    }

    impl BlockingBackend {
        fn new(
            inner: Arc<Mutex<MemoryBackend>>,
            gate: Arc<(Mutex<bool>, std::sync::Condvar)>,
            appends_seen: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                inner,
                gate,
                appends_seen,
            }
        }
    }

    impl EventStore for BlockingBackend {
        fn open_run(&mut self, _: RunManifest) -> Result<(), EventStoreError> {
            unreachable!("test wrapper does not forward open_run")
        }

        fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
            let (lock, cvar) = &*self.gate;
            let mut released = lock.lock().expect("gate poisoned");

            while !*released {
                released = cvar.wait(released).expect("gate wait");
            }
            self.appends_seen.fetch_add(1, Ordering::SeqCst);
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
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            self.inner
                .lock()
                .expect("inner poisoned")
                .scan_range(from, to, direction)
        }

        fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            self.inner.lock().expect("inner poisoned").scan_seq(seq)
        }

        fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
            self.inner.lock().expect("inner poisoned").lookup(kind, key)
        }

        fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            self.inner
                .lock()
                .expect("inner poisoned")
                .iter_index_keys(kind)
        }

        fn record_snapshot_anchor(
            &mut self,
            anchor: SnapshotAnchor,
        ) -> Result<(), EventStoreError> {
            self.inner
                .lock()
                .expect("inner poisoned")
                .record_snapshot_anchor(anchor)
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.inner.lock().expect("inner poisoned").seal(status)
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            self.inner.lock().expect("inner poisoned").manifest()
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            self.inner.lock().expect("inner poisoned").high_watermark()
        }
    }

    /// `EventStore` wrapper that returns `EventStoreError::Disk` for every append.
    #[derive(Debug, Default)]
    struct DiskFailureBackend {
        appends_seen: Arc<AtomicUsize>,
    }

    impl EventStore for DiskFailureBackend {
        fn open_run(&mut self, _: RunManifest) -> Result<(), EventStoreError> {
            Ok(())
        }

        fn append_batch(&mut self, _: &[AppendEntry]) -> Result<u64, EventStoreError> {
            self.appends_seen.fetch_add(1, Ordering::SeqCst);
            Err(EventStoreError::Disk("ENOSPC".to_string()))
        }

        fn scan_range(
            &self,
            _: u64,
            _: u64,
            _: ScanDirection,
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            Ok(Vec::new())
        }

        fn scan_seq(&self, _: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            Ok(None)
        }

        fn lookup(&self, _: IndexKind, _: &str) -> Result<Option<u64>, EventStoreError> {
            Ok(None)
        }

        fn iter_index_keys(&self, _: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            Ok(Vec::new())
        }

        fn seal(&mut self, _: RunStatus) -> Result<(), EventStoreError> {
            Ok(())
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            Err(EventStoreError::Backend("disk failure".to_string()))
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            Ok(0)
        }
    }

    #[fixture]
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

    #[rstest]
    fn submit_then_close_records_entries_and_seals(
        captured_halt: (HaltCallback, Arc<Mutex<Vec<HaltReason>>>),
    ) {
        let (halt, captured) = captured_halt;
        let (wrapper, shared) = SharedMemory::new();
        shared
            .lock()
            .expect("shared")
            .open_run(manifest("run-1"))
            .expect("open");

        let writer = EventStoreWriter::spawn(
            Box::new(wrapper),
            get_atomic_clock_static(),
            halt,
            WriterConfig::default(),
        )
        .expect("spawn");

        for ts in 10_u64..15_u64 {
            writer.submit(entry_draft(ts)).expect("submit");
        }

        let final_hwm = writer.close(run_ended_draft()).expect("close");

        // Five drafts plus the RunEnded entry.
        assert_eq!(final_hwm, 6);
        let backend = shared.lock().expect("shared");
        let m = backend.manifest().expect("manifest");
        assert_eq!(m.status, RunStatus::Ended);
        assert_eq!(m.high_watermark, 6);

        let last = backend.scan_seq(6).expect("scan").expect("present");
        assert_eq!(last.payload_type.as_str(), "RunEnded");
        assert!(captured.lock().expect("captured").is_empty());
    }

    #[rstest]
    fn record_snapshot_anchor_flushes_pending_entries_and_replay_tail_starts_after_anchor(
        captured_halt: (HaltCallback, Arc<Mutex<Vec<HaltReason>>>),
    ) {
        let (halt, captured) = captured_halt;
        let (wrapper, shared) = SharedMemory::new();
        shared
            .lock()
            .expect("shared")
            .open_run(manifest("run-anchor"))
            .expect("open");

        let writer = EventStoreWriter::spawn(
            Box::new(wrapper),
            get_atomic_clock_static(),
            halt,
            WriterConfig::default(),
        )
        .expect("spawn");

        writer.submit(entry_draft(10)).expect("submit first");
        writer.submit(entry_draft(11)).expect("submit second");
        let anchor = writer
            .record_snapshot_anchor("cache://position-snapshots/P-1/0", "blake3:abc")
            .expect("record anchor");

        assert_eq!(anchor.high_watermark, 2);

        writer.submit(entry_draft(12)).expect("submit third");
        writer.submit(entry_draft(13)).expect("submit fourth");
        let final_hwm = writer.close(run_ended_draft()).expect("close");

        let backend = shared.lock().expect("shared");
        assert_eq!(
            backend.latest_snapshot_anchor().expect("latest anchor"),
            Some(anchor.clone()),
        );

        let tail_seqs: Vec<_> = backend
            .scan_range(anchor.high_watermark + 1, final_hwm, ScanDirection::Forward)
            .expect("scan tail")
            .into_iter()
            .map(|entry| entry.seq)
            .collect();

        assert_eq!(tail_seqs, vec![3, 4, 5]);
        assert!(captured.lock().expect("captured").is_empty());
    }

    #[rstest]
    fn batches_respect_max_entries_threshold(
        captured_halt: (HaltCallback, Arc<Mutex<Vec<HaltReason>>>),
    ) {
        // Tiny batch ceiling (2) plus a large latency window forces the size threshold
        // to drive every flush; six entries produce three size-driven commits and one
        // close commit.
        let (halt, _) = captured_halt;
        let inner = Arc::new(Mutex::new(MemoryBackend::new()));
        inner
            .lock()
            .expect("inner")
            .open_run(manifest("run-batch"))
            .expect("open");

        let appends_seen = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(true), std::sync::Condvar::new()));
        let backend = BlockingBackend::new(
            Arc::clone(&inner),
            Arc::clone(&gate),
            Arc::clone(&appends_seen),
        );

        let config = WriterConfig {
            channel_capacity: 16,
            max_batch_entries: 2,
            max_batch_latency: Duration::from_secs(30),
            halt_threshold: Duration::from_secs(30),
        };

        let clock = get_atomic_clock_static();
        let boxed = Box::new(backend);

        let writer = EventStoreWriter::spawn(boxed, clock, halt, config).expect("spawn");

        for ts in 10_u64..16_u64 {
            writer.submit(entry_draft(ts)).expect("submit");
        }

        let final_hwm = writer.close(run_ended_draft()).expect("close");

        // 6 submits + 1 RunEnded == 7 entries, batch=2 -> 4 commits (3 size-driven + 1 close).
        assert_eq!(final_hwm, 7);
        assert_eq!(appends_seen.load(Ordering::SeqCst), 4);
    }

    #[rstest]
    fn submit_signals_halt_when_stalled_past_threshold(
        captured_halt: (HaltCallback, Arc<Mutex<Vec<HaltReason>>>),
    ) {
        // Channel capacity 1 with a backend gate held closed forces a stall: the first
        // submit fills the buffer, the writer thread blocks inside append_batch, and a
        // subsequent submit can never enqueue before the halt threshold fires.
        let (halt, captured) = captured_halt;
        let inner = Arc::new(Mutex::new(MemoryBackend::new()));
        inner
            .lock()
            .expect("inner")
            .open_run(manifest("run-halt"))
            .expect("open");

        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let appends_seen = Arc::new(AtomicUsize::new(0));
        let backend = BlockingBackend::new(
            Arc::clone(&inner),
            Arc::clone(&gate),
            Arc::clone(&appends_seen),
        );

        let config = WriterConfig {
            channel_capacity: 1,
            max_batch_entries: 1,
            max_batch_latency: Duration::from_millis(1),
            halt_threshold: Duration::from_millis(50),
        };

        let clock = get_atomic_clock_static();
        let boxed = Box::new(backend);

        let writer = EventStoreWriter::spawn(boxed, clock, halt, config).expect("spawn");

        // First submit fits in the channel; the writer thread takes it and blocks.
        writer.submit(entry_draft(10)).expect("first submit fits");

        // Wait long enough for the writer to dequeue and become blocked at the gate.
        std::thread::sleep(Duration::from_millis(20));

        // Second and third submits saturate the slot; the channel buffer holds one,
        // the next one stalls past the halt threshold.
        let _ = writer.submit(entry_draft(11));
        let stalled = writer.submit(entry_draft(12)).expect_err("must stall");

        match stalled {
            SubmitError::HaltSignaled { .. } => {}
            SubmitError::Closed => panic!("expected HaltSignaled, was Closed"),
        }
        let captured_reasons = captured.lock().expect("captured");
        assert_eq!(
            captured_reasons.len(),
            1,
            "halt callback must fire exactly once",
        );
        assert!(matches!(
            captured_reasons.first(),
            Some(HaltReason::BackpressureStall { .. })
        ));
        drop(captured_reasons);

        // After a backpressure halt has fired, subsequent submits must reject without
        // re-entering the retry loop, even though the channel and writer thread are
        // still alive.
        let post_halt = writer
            .submit(entry_draft(13))
            .expect_err("post-halt submit");

        match post_halt {
            SubmitError::Closed => {}
            SubmitError::HaltSignaled { .. } => {
                panic!("expected Closed after halt, was HaltSignaled")
            }
        }
        // The halt callback must not refire on subsequent submits.
        assert_eq!(
            captured.lock().expect("captured").len(),
            1,
            "halt callback must not refire after the first stall",
        );

        // Release the gate so the writer thread can finish and the test can drop the
        // writer cleanly.
        let (lock, cvar) = &*gate;
        *lock.lock().expect("gate") = true;
        cvar.notify_all();
    }

    #[rstest]
    fn record_snapshot_anchor_signals_halt_when_ack_stalls(
        captured_halt: (HaltCallback, Arc<Mutex<Vec<HaltReason>>>),
    ) {
        let (halt, captured) = captured_halt;
        let inner = Arc::new(Mutex::new(MemoryBackend::new()));
        inner
            .lock()
            .expect("inner")
            .open_run(manifest("run-anchor-halt"))
            .expect("open");

        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let appends_seen = Arc::new(AtomicUsize::new(0));
        let backend = BlockingBackend::new(
            Arc::clone(&inner),
            Arc::clone(&gate),
            Arc::clone(&appends_seen),
        );

        let writer = EventStoreWriter::spawn(
            Box::new(backend),
            get_atomic_clock_static(),
            halt,
            WriterConfig {
                channel_capacity: 2,
                max_batch_entries: 1,
                max_batch_latency: Duration::from_millis(1),
                halt_threshold: Duration::from_millis(50),
            },
        )
        .expect("spawn");

        writer.submit(entry_draft(10)).expect("first submit fits");
        std::thread::sleep(Duration::from_millis(20));

        let err = writer
            .record_snapshot_anchor("cache://position-snapshots/P-1/0", "blake3:abc")
            .expect_err("snapshot anchor ack must time out");
        let post_halt = writer
            .submit(entry_draft(11))
            .expect_err("post-halt submit");

        let (lock, cvar) = &*gate;
        *lock.lock().expect("gate") = true;
        cvar.notify_all();

        match err {
            EventStoreError::Backend(msg) => {
                assert!(
                    msg.contains("snapshot anchor ack stalled"),
                    "msg was: {msg}"
                );
            }
            other => panic!("expected Backend, was {other:?}"),
        }

        match post_halt {
            SubmitError::Closed => {}
            SubmitError::HaltSignaled { .. } => {
                panic!("expected Closed after anchor halt, was HaltSignaled")
            }
        }

        assert!(matches!(
            captured.lock().expect("captured").first(),
            Some(HaltReason::BackpressureStall { .. })
        ));
    }

    #[rstest]
    fn backend_disk_error_fires_halt_and_closes_writer(
        captured_halt: (HaltCallback, Arc<Mutex<Vec<HaltReason>>>),
    ) {
        let (halt, captured) = captured_halt;
        let backend = DiskFailureBackend::default();

        let writer = EventStoreWriter::spawn(
            Box::new(backend),
            get_atomic_clock_static(),
            halt,
            WriterConfig {
                channel_capacity: 4,
                max_batch_entries: 1,
                max_batch_latency: Duration::from_millis(1),
                halt_threshold: Duration::from_millis(500),
            },
        )
        .expect("spawn");

        writer
            .submit(entry_draft(10))
            .expect("first submit fits in channel before writer fail-stops");

        // Wait until the writer fail-stops and the halt fires.
        let mut waited = Duration::ZERO;
        let deadline = Duration::from_millis(500);
        while captured.lock().expect("captured").is_empty() && waited < deadline {
            std::thread::sleep(Duration::from_millis(10));
            waited += Duration::from_millis(10);
        }

        let captured_reasons = captured.lock().expect("captured");
        assert!(matches!(
            captured_reasons.first(),
            Some(HaltReason::BackendDisk(_))
        ));
        drop(captured_reasons);

        // Subsequent submits return Closed once the writer thread has exited.
        let mut closed_seen = false;

        for _ in 0..50 {
            match writer.submit(entry_draft(11)) {
                Err(SubmitError::Closed) => {
                    closed_seen = true;
                    break;
                }
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        assert!(closed_seen, "submits must surface Closed after fail-stop");

        // Close after fail-stop returns an error rather than panicking.
        let close_result = writer.close(run_ended_draft());
        assert!(close_result.is_err());
    }

    #[rstest]
    fn time_driven_flush_advances_watermark_before_close(
        captured_halt: (HaltCallback, Arc<Mutex<Vec<HaltReason>>>),
    ) {
        // A single submit well below max_batch_entries must still commit on the
        // latency window. Without this, a broken recv_timeout deadline would only
        // surface at close drain, masking the steady-state batching contract.
        let (halt, _) = captured_halt;
        let (wrapper, shared) = SharedMemory::new();
        shared
            .lock()
            .expect("shared")
            .open_run(manifest("run-time"))
            .expect("open");

        let writer = EventStoreWriter::spawn(
            Box::new(wrapper),
            get_atomic_clock_static(),
            halt,
            WriterConfig {
                channel_capacity: 32,
                max_batch_entries: 100,
                max_batch_latency: Duration::from_millis(20),
                halt_threshold: Duration::from_secs(30),
            },
        )
        .expect("spawn");

        writer.submit(entry_draft(10)).expect("submit");

        // Wait long enough that the latency window has elapsed multiple times.
        let mut waited = Duration::ZERO;
        while writer.high_watermark() == 0 && waited < Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(5));
            waited += Duration::from_millis(5);
        }
        assert_eq!(
            writer.high_watermark(),
            1,
            "latency window must commit a sub-batch entry before close",
        );

        let final_hwm = writer.close(run_ended_draft()).expect("close");
        assert_eq!(final_hwm, 2);
    }

    #[rstest]
    fn entry_draft_without_indices_constructor() {
        let topic: crate::entry::Topic = "exec.command.SubmitOrder".into();
        let payload_type = Ustr::from("SubmitOrder");
        let payload = Bytes::from_static(b"\x01\x02");
        let ts_init = UnixNanos::from(42);
        let draft = EntryDraft::without_indices(
            Headers::empty(),
            topic,
            payload_type,
            payload.clone(),
            ts_init,
        );

        assert!(draft.headers.is_empty());
        assert_eq!(draft.topic.as_ref(), "exec.command.SubmitOrder");
        assert_eq!(draft.payload_type.as_str(), "SubmitOrder");
        assert_eq!(draft.payload, payload);
        assert_eq!(draft.ts_init, ts_init);
        assert!(draft.index_keys.is_empty());
    }
}

#[cfg(test)]
#[cfg(madsim)]
mod madsim_tests {
    use std::sync::{Arc, Mutex};

    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_core::{UnixNanos, time::get_atomic_clock_static};
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        backend::{AppendEntry, EventStore, IndexKind, MemoryBackend, ScanDirection},
        entry::EventStoreEntry,
        error::EventStoreError,
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

    fn entry_draft(ts_init: u64) -> EntryDraft {
        EntryDraft {
            headers: Headers::empty(),
            topic: "exec.command.SubmitOrder".into(),
            payload_type: Ustr::from("SubmitOrder"),
            payload: Bytes::from_static(b"\x01\x02\x03\x04"),
            ts_init: UnixNanos::from(ts_init),
            index_keys: Vec::new(),
        }
    }

    #[derive(Debug)]
    struct SharedMemory(Arc<Mutex<MemoryBackend>>);

    impl SharedMemory {
        fn new() -> (Self, Arc<Mutex<MemoryBackend>>) {
            let arc = Arc::new(Mutex::new(MemoryBackend::new()));
            (Self(Arc::clone(&arc)), arc)
        }
    }

    impl EventStore for SharedMemory {
        fn open_run(&mut self, _: RunManifest) -> Result<(), EventStoreError> {
            unreachable!("test wrapper does not forward open_run")
        }

        fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .append_batch(entries)
        }

        fn scan_range(
            &self,
            from: u64,
            to: u64,
            direction: ScanDirection,
        ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .scan_range(from, to, direction)
        }

        fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
            self.0.lock().expect("shared memory poisoned").scan_seq(seq)
        }

        fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .lookup(kind, key)
        }

        fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .iter_index_keys(kind)
        }

        fn record_snapshot_anchor(
            &mut self,
            anchor: SnapshotAnchor,
        ) -> Result<(), EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .record_snapshot_anchor(anchor)
        }

        fn latest_snapshot_anchor(&self) -> Result<Option<SnapshotAnchor>, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .latest_snapshot_anchor()
        }

        fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
            self.0.lock().expect("shared memory poisoned").seal(status)
        }

        fn manifest(&self) -> Result<RunManifest, EventStoreError> {
            self.0.lock().expect("shared memory poisoned").manifest()
        }

        fn high_watermark(&self) -> Result<u64, EventStoreError> {
            self.0
                .lock()
                .expect("shared memory poisoned")
                .high_watermark()
        }
    }

    #[rstest]
    fn record_snapshot_anchor_records_current_watermark_under_madsim() {
        let (wrapper, shared) = SharedMemory::new();
        shared
            .lock()
            .expect("shared")
            .open_run(manifest("run-anchor"))
            .expect("open");

        let writer = EventStoreWriter::spawn(
            Box::new(wrapper),
            get_atomic_clock_static(),
            noop_halt(),
            WriterConfig::default(),
        )
        .expect("spawn");

        writer.submit(entry_draft(10)).expect("submit first");
        writer.submit(entry_draft(11)).expect("submit second");
        let anchor = writer
            .record_snapshot_anchor("cache://position-snapshots/P-1/0", "blake3:abc")
            .expect("record anchor");

        let backend = shared.lock().expect("shared");
        assert_eq!(anchor.high_watermark, 2);
        assert_eq!(
            backend.latest_snapshot_anchor().expect("latest anchor"),
            Some(anchor),
        );
    }
}
