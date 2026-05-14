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

//! The dedicated writer-thread loop that drains the bounded channel into atomic
//! `append_batch` commits.
//!
//! The loop owns the backend instance for the lifetime of the run: it stamps `seq`,
//! computes `entry_hash`, and groups incoming entries into bounded-size or bounded-latency
//! batches. The high-watermark mirror only advances after the backend acknowledges the
//! commit. Disk-pressure or corruption errors fire the halt callback and end the loop;
//! a graceful Close drains the channel, writes the supplied `RunEnded` entry as the final
//! row, and seals the manifest.

#[cfg(not(madsim))]
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender},
    },
    time::Instant,
};

use nautilus_core::UnixNanos;
#[cfg(not(madsim))]
use nautilus_core::time::AtomicTime;

use crate::{
    backend::AppendEntry, entry::EventStoreEntry, hash::compute_entry_hash, writer::EntryDraft,
};
#[cfg(not(madsim))]
use crate::{
    backend::EventStore,
    error::EventStoreError,
    snapshot::SnapshotAnchor,
    writer::{
        WriterConfig,
        halt::{HaltCallback, HaltReason},
    },
};

/// Internal message exchanged between submit and the writer thread.
#[cfg(not(madsim))]
pub(super) enum WriterMessage {
    /// A single captured entry plus its writer-receive timestamp.
    Entry {
        /// The unsealed entry handed in by the bus capture adapter.
        draft: EntryDraft,
        /// The writer-receive timestamp stamped at submit time.
        ts_publish: UnixNanos,
    },
    /// Graceful shutdown signal. The writer thread drains pending entries, appends the
    /// `RunEnded` entry, seals the manifest, and reports the final high-watermark via
    /// `ack`.
    Close {
        /// The terminal `RunEnded` entry committed before seal.
        run_ended: EntryDraft,
        /// One-shot reply channel for the close result.
        ack: SyncSender<Result<u64, EventStoreError>>,
    },
    /// Records a cache snapshot anchor after all pending entries have been flushed.
    RecordSnapshotAnchor {
        /// Cache-owned reference to the snapshot blob.
        blob_ref: String,
        /// Cache-owned content hash for the snapshot blob.
        content_hash: String,
        /// One-shot reply channel for the recorded anchor.
        ack: SyncSender<Result<SnapshotAnchor, EventStoreError>>,
    },
}

/// Runs the writer loop until the channel disconnects, a halt-worthy backend error fires,
/// or a Close message is received.
#[cfg(not(madsim))]
#[allow(clippy::needless_pass_by_value)] // arguments are owned by the writer thread
pub(super) fn run(
    mut backend: Box<dyn EventStore + Send>,
    rx: Receiver<WriterMessage>,
    config: WriterConfig,
    halt: HaltCallback,
    high_watermark: Arc<AtomicU64>,
    clock: &'static AtomicTime,
) {
    // Recover the next seq from the backend so a writer attached to an existing run
    // continues from the durable high-watermark rather than colliding with already-
    // committed entries.
    let mut next_seq = match backend.high_watermark() {
        Ok(hwm) => hwm + 1,
        Err(e) => {
            halt(HaltReason::from_backend_error(&e));
            return;
        }
    };

    let mut batch: Vec<AppendEntry> = Vec::with_capacity(config.max_batch_entries);
    let mut batch_deadline: Option<Instant> = None;

    loop {
        let recv_result = match batch_deadline {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(deadline) => {
                let now = Instant::now();
                let remaining = deadline.saturating_duration_since(now);
                rx.recv_timeout(remaining)
            }
        };

        match recv_result {
            Ok(WriterMessage::Entry { draft, ts_publish }) => {
                let append = build_append_entry(draft, ts_publish, next_seq);
                next_seq += 1;
                batch.push(append);

                if batch_deadline.is_none() {
                    batch_deadline = Some(Instant::now() + config.max_batch_latency);
                }

                if batch.len() >= config.max_batch_entries
                    && !flush(backend.as_mut(), &mut batch, &halt, high_watermark.as_ref())
                {
                    return;
                }

                if batch.is_empty() {
                    batch_deadline = None;
                }
            }
            Ok(WriterMessage::Close { run_ended, ack }) => {
                drain_pending(&rx, &mut batch, &mut next_seq);

                let now = clock.get_time_ns();
                let run_ended_append = build_append_entry(run_ended, now, next_seq);
                batch.push(run_ended_append);

                let flush_ok = flush(backend.as_mut(), &mut batch, &halt, high_watermark.as_ref());
                if !flush_ok {
                    let _ = ack.send(Err(EventStoreError::Backend(
                        "writer fail-stopped before seal".to_string(),
                    )));
                    return;
                }

                let seal_result = backend.seal(crate::manifest::RunStatus::Ended);
                let final_result = match seal_result {
                    Ok(()) => Ok(high_watermark.load(Ordering::Acquire)),
                    Err(e) => {
                        halt(HaltReason::from_backend_error(&e));
                        Err(e)
                    }
                };
                let _ = ack.send(final_result);
                return;
            }
            Ok(WriterMessage::RecordSnapshotAnchor {
                blob_ref,
                content_hash,
                ack,
            }) => {
                if !record_snapshot_anchor(
                    backend.as_mut(),
                    &mut batch,
                    &halt,
                    high_watermark.as_ref(),
                    blob_ref,
                    content_hash,
                    &ack,
                ) {
                    return;
                }

                batch_deadline = None;
            }
            Err(RecvTimeoutError::Timeout) => {
                if !batch.is_empty()
                    && !flush(backend.as_mut(), &mut batch, &halt, high_watermark.as_ref())
                {
                    return;
                }
                batch_deadline = None;
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !batch.is_empty() {
                    let _ = flush(backend.as_mut(), &mut batch, &halt, high_watermark.as_ref());
                }
                return;
            }
        }
    }
}

fn record_snapshot_anchor(
    backend: &mut dyn EventStore,
    batch: &mut Vec<AppendEntry>,
    halt: &HaltCallback,
    high_watermark: &AtomicU64,
    blob_ref: String,
    content_hash: String,
    ack: &SyncSender<Result<SnapshotAnchor, EventStoreError>>,
) -> bool {
    if !batch.is_empty() && !flush(backend, batch, halt, high_watermark) {
        let _ = ack.send(Err(EventStoreError::Backend(
            "writer fail-stopped before snapshot anchor".to_string(),
        )));

        return false;
    }

    let hwm = high_watermark.load(Ordering::Acquire);
    let anchor = SnapshotAnchor::new(hwm, blob_ref, content_hash);
    let result = match backend.record_snapshot_anchor(anchor.clone()) {
        Ok(()) => Ok(anchor),
        Err(e) => {
            halt(HaltReason::from_backend_error(&e));
            Err(e)
        }
    };

    let keep_running = result.is_ok();
    let _ = ack.send(result);

    keep_running
}

#[cfg(not(madsim))]
fn drain_pending(rx: &Receiver<WriterMessage>, batch: &mut Vec<AppendEntry>, next_seq: &mut u64) {
    while let Ok(msg) = rx.try_recv() {
        match msg {
            WriterMessage::Entry { draft, ts_publish } => {
                let append = build_append_entry(draft, ts_publish, *next_seq);
                *next_seq += 1;
                batch.push(append);
            }
            // A second Close while draining: there is no producer that should send one
            // (close consumes the writer), but if a future API permits it, fall back
            // to the run_ended entry the outer loop already queued and ignore this one.
            WriterMessage::Close { ack, .. } => {
                let _ = ack.send(Err(EventStoreError::Backend(
                    "writer is already closing".to_string(),
                )));
            }
            WriterMessage::RecordSnapshotAnchor { ack, .. } => {
                let _ = ack.send(Err(EventStoreError::Backend(
                    "writer is closing before snapshot anchor".to_string(),
                )));
            }
        }
    }
}

/// Commits the current batch, advances the high-watermark mirror on success, or fires the
/// halt callback on backend failure. Returns `true` if the writer loop should continue.
#[cfg(not(madsim))]
fn flush(
    backend: &mut dyn EventStore,
    batch: &mut Vec<AppendEntry>,
    halt: &HaltCallback,
    high_watermark: &AtomicU64,
) -> bool {
    if batch.is_empty() {
        return true;
    }

    match backend.append_batch(batch) {
        Ok(new_hwm) => {
            high_watermark.store(new_hwm, Ordering::Release);
            batch.clear();
            true
        }
        Err(e) => {
            halt(HaltReason::from_backend_error(&e));
            batch.clear();
            false
        }
    }
}

/// Builds a fully-formed [`AppendEntry`] from a draft, the writer-receive timestamp, and
/// the assigned sequence.
pub(super) fn build_append_entry(
    draft: EntryDraft,
    ts_publish: UnixNanos,
    seq: u64,
) -> AppendEntry {
    let EntryDraft {
        headers,
        topic,
        payload_type,
        payload,
        ts_init,
        index_keys,
    } = draft;

    let entry_hash = compute_entry_hash(
        seq,
        ts_init,
        ts_publish,
        topic.as_ref(),
        payload_type.as_str(),
        &payload,
        &headers,
    );

    let entry = EventStoreEntry::new(
        entry_hash,
        seq,
        headers,
        topic,
        payload_type,
        payload,
        ts_init,
        ts_publish,
    );

    AppendEntry::new(entry, index_keys)
}
