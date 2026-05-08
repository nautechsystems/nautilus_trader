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

//! Backend abstraction for the event store.
//!
//! The [`EventStore`] trait keeps backends interchangeable: in-memory under simulation,
//! `redb` in production, and any future swap (custom WAL, segmented log) without touching
//! consumers. Backend-specific types (`redb::Database`, `redb::Error`) never appear in this
//! trait surface.

use crate::{
    entry::EventStoreEntry,
    error::EventStoreError,
    manifest::{RunManifest, RunStatus},
};

/// The kind of secondary index the reader can look up.
///
/// Indices are rebuildable projections from the canonical `seq -> entry` table, not
/// authoritative storage. The verifier rebuilds them and cross-checks against the stored
/// rows.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IndexKind {
    /// `intent_id -> seq`. Looks up the first entry for an agent or strategy intent.
    IntentId,
    /// `client_order_id -> seq`. Looks up the first entry that mentions a client order id.
    ClientOrderId,
    /// `venue_order_id -> seq`. Looks up the first entry that mentions a venue order id.
    VenueOrderId,
}

/// Direction for a sequence-keyed scan.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScanDirection {
    /// Forward scan: ascending `seq`.
    Forward,
    /// Reverse scan: descending `seq`.
    Reverse,
}

/// The single-node embedded event store.
///
/// One backend instance owns one open run. Writes funnel through `append_batch`; reads are
/// `seq`-keyed scans or single-key lookups across the secondary indices.
///
/// Backends are responsible for:
///
/// - Per-run organization on disk (one redb file per run, one in-memory log per run).
/// - Durable commit semantics (`Durability::Immediate` for redb).
/// - High-watermark advance after commit acknowledgement.
/// - Mapping backend-specific errors onto [`EventStoreError`].
///
/// This trait does not own batching or thread-management policy: the writer
/// (`crates/event_store/src/writer/`) is the dedicated thread that batches entries and
/// invokes `append_batch` against the backend.
pub trait EventStore: Send {
    /// Opens an existing run or creates a new one with the supplied manifest.
    ///
    /// On reopening a run whose status is [`RunStatus::Running`] without a `RunEnded`
    /// entry, the backend returns [`EventStoreError::CrashedPredecessor`] so the kernel
    /// can seal it before opening a new run.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] for unclassified backend failures,
    /// [`EventStoreError::Corrupted`] for header-region damage, and
    /// [`EventStoreError::Disk`] for disk pressure during creation.
    fn open_run(&mut self, manifest: RunManifest) -> Result<(), EventStoreError>;

    /// Appends a batch of entries in a single backend transaction.
    ///
    /// The writer assigns `seq` and `ts_publish` before calling this method. The backend
    /// rejects batches whose first `seq` is not strictly above the current high-watermark.
    ///
    /// On successful commit, the backend advances its high-watermark and returns the new
    /// value.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Closed`] when the run is sealed,
    /// [`EventStoreError::OutOfOrder`] when the first seq is not above the high-watermark,
    /// [`EventStoreError::Disk`] when the backing storage refuses the write, and
    /// [`EventStoreError::Backend`] for unclassified backend failures.
    fn append_batch(&mut self, entries: &[EventStoreEntry]) -> Result<u64, EventStoreError>;

    /// Scans entries by `seq` over the inclusive range `[from, to]`.
    ///
    /// Backends may stream rows lazily; the trait's vector return is a simple
    /// implementation default for the in-memory backend and the verifier hot path. The
    /// reader (`crates/event_store/src/reader/`) wraps this with iterator semantics.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::HashMismatch`] when a row's recomputed hash diverges
    /// from the stored value, [`EventStoreError::Gap`] when the scan observes a missing
    /// `seq`, and [`EventStoreError::Backend`] for unclassified backend failures.
    fn scan_range(
        &self,
        from: u64,
        to: u64,
        direction: ScanDirection,
    ) -> Result<Vec<EventStoreEntry>, EventStoreError>;

    /// Reads a single entry by `seq`.
    ///
    /// # Errors
    ///
    /// See [`EventStore::scan_range`].
    fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError>;

    /// Looks up the first `seq` recorded under the given index key.
    ///
    /// Returns `None` when the key has not been observed.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] for unclassified backend failures.
    fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError>;

    /// Seals the open run with the given final status and persists the manifest update.
    ///
    /// Subsequent calls to [`EventStore::append_batch`] return [`EventStoreError::Closed`].
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] for unclassified backend failures and
    /// [`EventStoreError::Disk`] for disk pressure during the seal commit.
    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError>;

    /// Returns the current manifest snapshot.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open.
    fn manifest(&self) -> Result<RunManifest, EventStoreError>;

    /// Returns the largest `seq` durably acknowledged for the open run.
    ///
    /// Returns `0` when no entries have been committed yet (the writer assigns `seq`
    /// starting at `1`).
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open.
    fn high_watermark(&self) -> Result<u64, EventStoreError>;
}
