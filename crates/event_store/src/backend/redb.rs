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

//! redb-backed [`EventStore`] implementation.
//!
//! One redb file per run at `<base>/<instance_id>/<run_id>.redb`. Every commit uses
//! [`Durability::Immediate`] so a crashed writer never leaves the in-flight tail visible
//! after reopen, and the high-watermark only advances after a durable acknowledgement.

use std::{
    fmt::Debug,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use nautilus_core::UnixNanos;
use redb::{
    CommitError, Database, DatabaseError, Durability, ReadOnlyDatabase, ReadTransaction,
    ReadableDatabase, ReadableTable, StorageError, TableDefinition, TableError, TransactionError,
    WriteTransaction,
};

use crate::{
    backend::{AppendEntry, EventStore, IndexKey, IndexKind, ScanDirection},
    codec,
    entry::EventStoreEntry,
    error::EventStoreError,
    format,
    manifest::{RunManifest, RunStatus},
    snapshot::{SnapshotAnchor, validate_new_anchor},
};

const ENTRIES_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("entries");
const MANIFEST_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("manifest");
const CLIENT_ORDER_INDEX: TableDefinition<&str, u64> = TableDefinition::new("client_order_id_idx");
const VENUE_ORDER_INDEX: TableDefinition<&str, u64> = TableDefinition::new("venue_order_id_idx");
const SNAPSHOT_ANCHOR_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("snapshot_anchor");

const MANIFEST_KEY: &str = "current";
const SNAPSHOT_ANCHOR_KEY: &str = "latest";

/// On-disk [`EventStore`] backed by a per-run [`redb`] file.
///
/// One backend instance owns at most one open run at a time. Opening a fresh run creates
/// `<base>/<instance_id>/<run_id>.redb` and writes the manifest with status
/// [`RunStatus::Running`] before returning. Reopening a path whose manifest is still
/// [`RunStatus::Running`] returns [`EventStoreError::CrashedPredecessor`]; the caller seals
/// it as [`RunStatus::CrashedRecovered`] (or [`RunStatus::Quarantined`]) and then opens a new
/// run, mirroring the in-memory backend's contract.
#[derive(Debug)]
pub struct RedbBackend {
    base_dir: PathBuf,
    state: Option<RunState>,
}

#[derive(Debug)]
struct RunState {
    db: RunDatabase,
    manifest: RunManifest,
    high_watermark: u64,
    max_ts_init: UnixNanos,
    file_path: PathBuf,
}

enum RunDatabase {
    ReadWrite(Database),
    ReadOnly(ReadOnlyDatabase),
}

impl Debug for RunDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadWrite(_) => f.write_str("RunDatabase::ReadWrite"),
            Self::ReadOnly(_) => f.write_str("RunDatabase::ReadOnly"),
        }
    }
}

impl RunDatabase {
    fn readable(&self) -> &dyn ReadableDatabase {
        match self {
            Self::ReadWrite(db) => db,
            Self::ReadOnly(db) => db,
        }
    }

    fn read_write(&self) -> Result<&Database, EventStoreError> {
        match self {
            Self::ReadWrite(db) => Ok(db),
            Self::ReadOnly(_) => Err(EventStoreError::Closed),
        }
    }

    fn begin_read(&self) -> Result<ReadTransaction, EventStoreError> {
        self.readable().begin_read().map_err(map_transaction_err)
    }
}

impl RedbBackend {
    /// Creates a new [`RedbBackend`] rooted at `base_dir`.
    ///
    /// The backend creates `<base_dir>/<instance_id>/` lazily on the first
    /// [`EventStore::open_run`] call.
    #[must_use]
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            state: None,
        }
    }

    /// Returns the directory the backend writes run files to for `instance_id`.
    #[must_use]
    pub fn run_dir(&self, instance_id: &str) -> PathBuf {
        self.base_dir.join(instance_id)
    }

    /// Returns the on-disk path the backend uses for `(instance_id, run_id)`.
    #[must_use]
    pub fn run_path(&self, instance_id: &str, run_id: &str) -> PathBuf {
        self.run_dir(instance_id).join(format!("{run_id}.redb"))
    }

    /// Returns the path of the currently open run file.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open.
    pub fn current_path(&self) -> Result<&Path, EventStoreError> {
        Ok(self.state()?.file_path.as_path())
    }

    /// Opens the sealed run file at `<base>/<instance_id>/<run_id>.redb` for read-only replay.
    ///
    /// # Design
    ///
    /// The standard [`EventStore::open_run`] path rejects sealed files: that is the
    /// crash-recovery guard, a successor must not silently reopen a predecessor's log
    /// without going through seal. Event-store replay is the legitimate case for touching
    /// a sealed file, so the reader uses this constructor instead.
    ///
    /// The shared [`EventStore`] trait is held intentionally narrow and is locked by
    /// design; adding a sealed-open method to it would force the in-memory backend
    /// (whose sealed runs stay readable in place without a reopen step) to carry a
    /// useless second entry point, and would conflate the writer's open-or-recover
    /// lifecycle with the reader's pure read-only path. The sealed-open path therefore
    /// lives as a backend-specific constructor: each backend adds the entry points it
    /// actually needs. The resulting [`RedbBackend`] still implements [`EventStore`],
    /// so the reader composes over the locked trait without pulling in writer-only
    /// methods. [`crate::backend::MemoryBackend`] has no equivalent constructor: a
    /// sealed in-memory run keeps its state accessible to any reader holding the
    /// backend instance, and the reader receives that instance directly.
    ///
    /// The returned backend holds a read-only database handle, rejects
    /// [`EventStore::append_batch`] with [`EventStoreError::Closed`] (the manifest is
    /// already sealed), and exposes every read path: [`EventStore::scan_range`],
    /// [`EventStore::scan_seq`], [`EventStore::lookup`], and [`EventStore::manifest`].
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when the run file does not exist or its
    /// status is not a sealed terminal state (use [`EventStore::open_run`] for that
    /// path); [`EventStoreError::Corrupted`] when the run file lacks a manifest or
    /// fails to decode.
    pub fn open_sealed(
        base_dir: impl Into<PathBuf>,
        instance_id: &str,
        run_id: &str,
    ) -> Result<Self, EventStoreError> {
        let base = base_dir.into();
        let path = base.join(instance_id).join(format!("{run_id}.redb"));
        Self::open_sealed_path(base, path)
    }

    /// Opens a sealed redb run file directly by path for read-only replay or verification.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when the run file does not exist or its
    /// status is not a sealed terminal state (use [`EventStore::open_run`] for that
    /// path); [`EventStoreError::Corrupted`] when the run file lacks a manifest or
    /// fails to decode.
    pub fn open_sealed_file(path: impl Into<PathBuf>) -> Result<Self, EventStoreError> {
        let path = path.into();
        let base = path
            .parent()
            .and_then(Path::parent)
            .map_or_else(PathBuf::new, Path::to_path_buf);
        Self::open_sealed_path(base, path)
    }

    fn open_sealed_path(base: PathBuf, path: PathBuf) -> Result<Self, EventStoreError> {
        if !path.exists() {
            return Err(EventStoreError::Backend(format!(
                "no run file at {}",
                path.display()
            )));
        }

        let db = ReadOnlyDatabase::open(&path).map_err(map_read_only_database_err)?;
        format::verify_store_format(&db)?;
        let manifest = Self::read_manifest(&db)?.ok_or_else(|| {
            EventStoreError::Corrupted(format!(
                "missing manifest in run file at {}",
                path.display()
            ))
        })?;

        if !manifest.is_sealed() {
            return Err(EventStoreError::Backend(format!(
                "run file at {} is not sealed, status was {:?}",
                path.display(),
                manifest.status,
            )));
        }
        let (high_watermark, max_ts_init) = Self::compute_progress(&db)?;

        Ok(Self {
            base_dir: base,
            state: Some(RunState {
                db: RunDatabase::ReadOnly(db),
                manifest,
                high_watermark,
                max_ts_init,
                file_path: path,
            }),
        })
    }

    /// Lists the manifests of every run file under `<base_dir>/<instance_id>/*.redb`.
    ///
    /// Used by the reader for forensics navigation across runs without requiring an
    /// active backend instance per run. The result is sorted by `start_ts_init` so
    /// chronologically-newer runs appear last.
    ///
    /// Opens each run file with a read-only database handle. A run file whose process
    /// died hard (kill, OOM, power loss) lacks redb's allocator-state table and refuses
    /// the read-only open; the listing falls back to a writable open, which performs
    /// redb's repair pass and leaves the file readable again. Files that still cannot
    /// be opened or that lack a manifest are skipped with a logged error so one damaged
    /// file cannot block recovery or retention over the healthy runs; such files never
    /// become recovery parents or reclaim candidates and are left in place for manual
    /// inspection.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when the directory iterator fails.
    pub fn list_runs(
        base_dir: &Path,
        instance_id: &str,
    ) -> Result<Vec<RunManifest>, EventStoreError> {
        let dir = base_dir.join(instance_id);
        let entries = match fs::read_dir(&dir) {
            Ok(it) => it,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(EventStoreError::Backend(format!(
                    "read_dir {}: {e}",
                    dir.display()
                )));
            }
        };

        let mut manifests = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|e| {
                EventStoreError::Backend(format!("read_dir entry in {}: {e}", dir.display()))
            })?;
            let path = entry.path();

            if !is_run_file(&path) {
                continue;
            }

            match Self::read_run_manifest(&path) {
                Ok(manifest) => manifests.push(manifest),
                Err(e) => {
                    log::error!("Skipping unreadable run file {}: {e}", path.display());
                }
            }
        }
        manifests.sort_by_key(|m| m.start_ts_init);
        Ok(manifests)
    }

    fn read_run_manifest(path: &Path) -> Result<RunManifest, EventStoreError> {
        let manifest = match ReadOnlyDatabase::open(path) {
            Ok(db) => {
                format::verify_store_format(&db)?;
                Self::read_manifest(&db)?
            }
            // Each durable commit deletes redb's allocator-state table and only a clean
            // `Database::drop` rewrites it, so a hard-killed process leaves a file the
            // read-only open refuses. A writable open repairs it for future opens.
            Err(DatabaseError::RepairAborted) => {
                log::warn!(
                    "Run file {} was not shut down cleanly, repairing",
                    path.display()
                );
                let db = Database::open(path).map_err(map_database_err)?;
                format::verify_store_format(&db)?;
                Self::read_manifest(&db)?
            }
            Err(e) => return Err(map_read_only_database_err(e)),
        };
        manifest.ok_or_else(|| {
            EventStoreError::Corrupted(format!(
                "missing manifest in run file at {}",
                path.display()
            ))
        })
    }

    fn state(&self) -> Result<&RunState, EventStoreError> {
        self.state
            .as_ref()
            .ok_or_else(|| EventStoreError::Backend("no run open".to_string()))
    }

    fn state_mut(&mut self) -> Result<&mut RunState, EventStoreError> {
        self.state
            .as_mut()
            .ok_or_else(|| EventStoreError::Backend("no run open".to_string()))
    }

    fn initialize_fresh(db: &Database, manifest: &RunManifest) -> Result<(), EventStoreError> {
        let txn = begin_immediate_write(db)?;
        {
            txn.open_table(ENTRIES_TABLE).map_err(map_table_err)?;
            txn.open_table(CLIENT_ORDER_INDEX).map_err(map_table_err)?;
            txn.open_table(VENUE_ORDER_INDEX).map_err(map_table_err)?;
            txn.open_table(SNAPSHOT_ANCHOR_TABLE)
                .map_err(map_table_err)?;
        }
        format::write_store_format(&txn)?;
        insert_run_manifest(&txn, manifest)?;
        txn.commit().map_err(map_commit_err)?;
        Ok(())
    }

    fn write_manifest(db: &Database, manifest: &RunManifest) -> Result<(), EventStoreError> {
        let txn = begin_immediate_write(db)?;
        insert_run_manifest(&txn, manifest)?;
        txn.commit().map_err(map_commit_err)?;
        Ok(())
    }

    fn read_manifest<D: ReadableDatabase + ?Sized>(
        db: &D,
    ) -> Result<Option<RunManifest>, EventStoreError> {
        let txn = db.begin_read().map_err(map_transaction_err)?;
        let table = txn.open_table(MANIFEST_TABLE).map_err(map_table_err)?;
        let Some(value) = table.get(MANIFEST_KEY).map_err(map_storage_err)? else {
            return Ok(None);
        };
        let bytes = value.value();
        let manifest = codec::decode_from_slice::<RunManifest>(bytes)
            .map_err(|e| EventStoreError::Corrupted(format!("decode manifest: {e}")))?;
        Ok(Some(manifest))
    }

    fn read_snapshot_anchor<D: ReadableDatabase + ?Sized>(
        db: &D,
    ) -> Result<Option<SnapshotAnchor>, EventStoreError> {
        let txn = db.begin_read().map_err(map_transaction_err)?;
        let table = match txn.open_table(SNAPSHOT_ANCHOR_TABLE) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(map_table_err(e)),
        };
        let Some(value) = table.get(SNAPSHOT_ANCHOR_KEY).map_err(map_storage_err)? else {
            return Ok(None);
        };
        let bytes = value.value();
        let anchor = codec::decode_from_slice::<SnapshotAnchor>(bytes)
            .map_err(|e| EventStoreError::Corrupted(format!("decode snapshot anchor: {e}")))?;
        Ok(Some(anchor))
    }

    fn compute_progress<D: ReadableDatabase + ?Sized>(
        db: &D,
    ) -> Result<(u64, UnixNanos), EventStoreError> {
        let txn = db.begin_read().map_err(map_transaction_err)?;
        let table = txn.open_table(ENTRIES_TABLE).map_err(map_table_err)?;

        let Some((last_key, _)) = table.last().map_err(map_storage_err)? else {
            return Ok((0, UnixNanos::default()));
        };
        let high_watermark = last_key.value();

        // Walk the entry table once to recover the maximum `ts_init`. Memory.rs tracks this
        // across appends; on crash recovery we have nothing to fall back on, so we recompute
        // it from the durable rows. An undecodable row must not make the run unopenable:
        // max ts_init is best-effort, and the corruption itself surfaces on the scan paths,
        // where the recovery sweep quarantines the run.
        let mut max_ts = UnixNanos::default();
        let iter = table.iter().map_err(map_storage_err)?;

        for row in iter {
            let (key, value) = row.map_err(map_storage_err)?;
            let bytes = value.value();

            match codec::decode_from_slice::<EventStoreEntry>(bytes) {
                Ok(entry) => {
                    if entry.ts_init > max_ts {
                        max_ts = entry.ts_init;
                    }
                }
                Err(e) => {
                    log::error!("Undecodable entry at seq {} on load: {e}", key.value());
                }
            }
        }

        Ok((high_watermark, max_ts))
    }
}

impl EventStore for RedbBackend {
    fn open_run(&mut self, mut manifest: RunManifest) -> Result<(), EventStoreError> {
        if let Some(state) = &self.state {
            if matches!(state.db, RunDatabase::ReadOnly(_)) {
                return Err(EventStoreError::Closed);
            }

            if !state.manifest.is_sealed() {
                return Err(EventStoreError::CrashedPredecessor);
            }
        }

        let dir = self.run_dir(&manifest.instance_id);
        fs::create_dir_all(&dir).map_err(|e| {
            let msg = format!("create dir {}: {e}", dir.display());

            if is_disk_pressure(e.kind()) {
                EventStoreError::Disk(msg)
            } else {
                EventStoreError::Backend(msg)
            }
        })?;
        let path = self.run_path(&manifest.instance_id, &manifest.run_id);
        let path_existed = path.exists();

        let db = Database::create(&path).map_err(map_database_err)?;

        if path_existed {
            format::verify_store_format(&db)?;
            let on_disk = Self::read_manifest(&db)?.ok_or_else(|| {
                EventStoreError::Corrupted(format!(
                    "missing manifest in existing run file at {}",
                    path.display()
                ))
            })?;

            if !matches!(on_disk.status, RunStatus::Running) {
                return Err(EventStoreError::Backend(format!(
                    "run file at {} already sealed, status was {:?}",
                    path.display(),
                    on_disk.status
                )));
            }

            let (high_watermark, max_ts_init) = Self::compute_progress(&db)?;
            let mut recovered = on_disk;
            recovered.high_watermark = high_watermark;
            self.state = Some(RunState {
                db: RunDatabase::ReadWrite(db),
                manifest: recovered,
                high_watermark,
                max_ts_init,
                file_path: path,
            });
            return Err(EventStoreError::CrashedPredecessor);
        }

        manifest.status = RunStatus::Running;
        manifest.end_ts_init = None;
        manifest.high_watermark = 0;
        Self::initialize_fresh(&db, &manifest)?;

        self.state = Some(RunState {
            db: RunDatabase::ReadWrite(db),
            manifest,
            high_watermark: 0,
            max_ts_init: UnixNanos::default(),
            file_path: path,
        });
        Ok(())
    }

    fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
        let state = self.state_mut()?;

        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }

        if entries.is_empty() {
            return Ok(state.high_watermark);
        }

        for (expected, append) in (state.high_watermark + 1..).zip(entries.iter()) {
            if append.entry.seq != expected {
                // Atomically rejected: surface the durable high-watermark, not the within-batch
                // validation cursor, so callers that resync from this value never skip entries
                // that were never committed.
                return Err(EventStoreError::OutOfOrder {
                    high_watermark: state.high_watermark,
                    seq: append.entry.seq,
                });
            }
        }

        let encoded: Vec<Vec<u8>> = entries
            .iter()
            .map(|append| {
                codec::encode_to_vec(&append.entry).map_err(|e| {
                    EventStoreError::Backend(format!("encode entry seq={}: {e}", append.entry.seq))
                })
            })
            .collect::<Result<_, _>>()?;

        let db = state.db.read_write()?;
        let txn = begin_immediate_write(db)?;
        {
            let mut entries_table = txn.open_table(ENTRIES_TABLE).map_err(map_table_err)?;
            let mut client_table = txn.open_table(CLIENT_ORDER_INDEX).map_err(map_table_err)?;
            let mut venue_table = txn.open_table(VENUE_ORDER_INDEX).map_err(map_table_err)?;

            for (append, bytes) in entries.iter().zip(encoded.iter()) {
                entries_table
                    .insert(append.entry.seq, bytes.as_slice())
                    .map_err(map_storage_err)?;

                for IndexKey { kind, key } in &append.index_keys {
                    let table = match kind {
                        IndexKind::ClientOrderId => &mut client_table,
                        IndexKind::VenueOrderId => &mut venue_table,
                    };
                    let already = table.get(key.as_str()).map_err(map_storage_err)?.is_some();

                    if !already {
                        table
                            .insert(key.as_str(), append.entry.seq)
                            .map_err(map_storage_err)?;
                    }
                }
            }
        }
        txn.commit().map_err(map_commit_err)?;

        let mut max_ts = state.max_ts_init;
        let mut new_hwm = state.high_watermark;

        for append in entries {
            if append.entry.ts_init > max_ts {
                max_ts = append.entry.ts_init;
            }
            new_hwm = append.entry.seq;
        }
        state.high_watermark = new_hwm;
        state.max_ts_init = max_ts;
        state.manifest.high_watermark = new_hwm;

        Ok(new_hwm)
    }

    fn scan_range(
        &self,
        from: u64,
        to: u64,
        direction: ScanDirection,
    ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        let state = self.state()?;

        if from > to || from == 0 || state.high_watermark == 0 {
            return Ok(Vec::new());
        }

        let lo = from;
        let hi = to.min(state.high_watermark);

        if lo > hi {
            return Ok(Vec::new());
        }

        let txn = state.db.begin_read()?;
        let table = txn.open_table(ENTRIES_TABLE).map_err(map_table_err)?;

        // hi is capped to high_watermark above, so every seq in [lo, hi] is supposed to be
        // present. redb iterates only existing keys, so a missing row inside this range
        // means a committed sequence has been lost (corruption, external tampering); we
        // surface Gap rather than silently shortening the result.
        let mut out = Vec::new();
        let mut expected = lo;
        let iter = table.range(lo..=hi).map_err(map_storage_err)?;

        for row in iter {
            let (k, v) = row.map_err(map_storage_err)?;
            let seq = k.value();

            if seq != expected {
                return Err(EventStoreError::Gap {
                    prev: expected.saturating_sub(1),
                    next: seq,
                    missing: expected,
                });
            }
            let bytes = v.value();
            let entry = codec::decode_from_slice::<EventStoreEntry>(bytes)
                .map_err(|e| EventStoreError::Corrupted(format!("decode entry seq={seq}: {e}")))?;

            if entry.recompute_hash() != entry.entry_hash {
                return Err(EventStoreError::HashMismatch { seq });
            }
            out.push(entry);
            expected = seq + 1;
        }

        if expected <= hi {
            return Err(EventStoreError::Gap {
                prev: expected.saturating_sub(1),
                next: hi + 1,
                missing: expected,
            });
        }

        if matches!(direction, ScanDirection::Reverse) {
            out.reverse();
        }
        Ok(out)
    }

    fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
        let state = self.state()?;

        if seq == 0 || seq > state.high_watermark {
            return Ok(None);
        }

        let txn = state.db.begin_read()?;
        let table = txn.open_table(ENTRIES_TABLE).map_err(map_table_err)?;
        let Some(value) = table.get(seq).map_err(map_storage_err)? else {
            // seq is inside the watermark per the guard above, so the row must exist;
            // its absence is a committed-but-missing entry.
            return Err(EventStoreError::Gap {
                prev: seq.saturating_sub(1),
                next: seq + 1,
                missing: seq,
            });
        };

        let bytes = value.value();
        let entry = codec::decode_from_slice::<EventStoreEntry>(bytes)
            .map_err(|e| EventStoreError::Corrupted(format!("decode entry seq={seq}: {e}")))?;

        if entry.recompute_hash() != entry.entry_hash {
            return Err(EventStoreError::HashMismatch { seq });
        }
        Ok(Some(entry))
    }

    fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
        let state = self.state()?;
        let txn = state.db.begin_read()?;
        let definition = match kind {
            IndexKind::ClientOrderId => CLIENT_ORDER_INDEX,
            IndexKind::VenueOrderId => VENUE_ORDER_INDEX,
        };
        let table = txn.open_table(definition).map_err(map_table_err)?;
        let value = table.get(key).map_err(map_storage_err)?;
        Ok(value.map(|v| v.value()))
    }

    fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
        let state = self.state()?;
        let txn = state.db.begin_read()?;
        let definition = match kind {
            IndexKind::ClientOrderId => CLIENT_ORDER_INDEX,
            IndexKind::VenueOrderId => VENUE_ORDER_INDEX,
        };
        let table = txn.open_table(definition).map_err(map_table_err)?;
        let iter = table.iter().map_err(map_storage_err)?;
        let mut out = Vec::new();

        for row in iter {
            let (k, v) = row.map_err(map_storage_err)?;
            out.push((k.value().to_string(), v.value()));
        }
        Ok(out)
    }

    fn record_snapshot_anchor(&mut self, anchor: SnapshotAnchor) -> Result<(), EventStoreError> {
        let state = self.state_mut()?;

        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }

        let latest = Self::read_snapshot_anchor(state.db.readable())?;
        validate_new_anchor(&anchor, state.high_watermark, latest.as_ref())?;

        let bytes = codec::encode_to_vec(&anchor)
            .map_err(|e| EventStoreError::Backend(format!("encode snapshot anchor: {e}")))?;
        let db = state.db.read_write()?;
        let txn = begin_immediate_write(db)?;
        {
            let mut table = txn
                .open_table(SNAPSHOT_ANCHOR_TABLE)
                .map_err(map_table_err)?;
            table
                .insert(SNAPSHOT_ANCHOR_KEY, bytes.as_slice())
                .map_err(map_storage_err)?;
        }
        txn.commit().map_err(map_commit_err)?;
        Ok(())
    }

    fn latest_snapshot_anchor(&self) -> Result<Option<SnapshotAnchor>, EventStoreError> {
        Self::read_snapshot_anchor(self.state()?.db.readable())
    }

    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
        let state = self.state_mut()?;

        // Running is not a terminal state; accepting it would leave `is_sealed()` returning
        // false while the seal call returned Ok, so subsequent appends would not see Closed.
        if matches!(status, RunStatus::Running) {
            return Err(EventStoreError::Backend(
                "seal status must be a terminal state, was Running".to_string(),
            ));
        }

        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }

        let mut updated = state.manifest.clone();
        updated.status = status;
        updated.high_watermark = state.high_watermark;

        if state.high_watermark > 0 {
            updated.end_ts_init = Some(state.max_ts_init);
        }

        Self::write_manifest(state.db.read_write()?, &updated)?;
        state.manifest = updated;
        Ok(())
    }

    fn manifest(&self) -> Result<RunManifest, EventStoreError> {
        Ok(self.state()?.manifest.clone())
    }

    fn high_watermark(&self) -> Result<u64, EventStoreError> {
        Ok(self.state()?.high_watermark)
    }
}

fn begin_immediate_write(db: &Database) -> Result<WriteTransaction, EventStoreError> {
    let mut txn = db.begin_write().map_err(map_transaction_err)?;
    txn.set_durability(Durability::Immediate)
        .map_err(|e| EventStoreError::Backend(format!("set durability: {e}")))?;
    Ok(txn)
}

fn insert_run_manifest(
    txn: &WriteTransaction,
    manifest: &RunManifest,
) -> Result<(), EventStoreError> {
    let bytes = codec::encode_to_vec(manifest)
        .map_err(|e| EventStoreError::Backend(format!("encode manifest: {e}")))?;
    let mut table = txn.open_table(MANIFEST_TABLE).map_err(map_table_err)?;
    table
        .insert(MANIFEST_KEY, bytes.as_slice())
        .map_err(map_storage_err)?;
    Ok(())
}

fn map_storage_err(err: StorageError) -> EventStoreError {
    match err {
        StorageError::Io(io_err) if is_disk_pressure(io_err.kind()) => {
            EventStoreError::Disk(io_err.to_string())
        }
        StorageError::Corrupted(msg) => EventStoreError::Corrupted(msg),
        other => EventStoreError::Backend(other.to_string()),
    }
}

// `EventStoreError::Disk` documents ENOSPC and `RLIMIT_FSIZE` as its targets. On the
// stable toolchain `ENOSPC` surfaces as `StorageFull`, `RLIMIT_FSIZE`/`EFBIG` as
// `FileTooLarge`, and `EDQUOT` as `QuotaExceeded`; the kernel halt path keys off
// `Disk`, so all three must classify the same way.
fn is_disk_pressure(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::FileTooLarge | ErrorKind::StorageFull | ErrorKind::QuotaExceeded
    )
}

fn map_database_err(err: DatabaseError) -> EventStoreError {
    match err {
        DatabaseError::RepairAborted => EventStoreError::Corrupted(
            "database requires repair and cannot be verified read-only".to_string(),
        ),
        DatabaseError::UpgradeRequired(version) => EventStoreError::Corrupted(format!(
            "database file format version {version} requires manual upgrade",
        )),
        DatabaseError::Storage(storage) => map_storage_err(storage),
        other => EventStoreError::Backend(other.to_string()),
    }
}

fn map_read_only_database_err(err: DatabaseError) -> EventStoreError {
    match err {
        DatabaseError::Storage(StorageError::Io(io_err)) if is_corrupt_read(io_err.kind()) => {
            EventStoreError::Corrupted(format!("read-only open failed: {io_err}"))
        }
        other => map_database_err(other),
    }
}

fn is_corrupt_read(kind: ErrorKind) -> bool {
    matches!(kind, ErrorKind::UnexpectedEof | ErrorKind::InvalidData)
}

fn map_table_err(err: TableError) -> EventStoreError {
    // Mirror redb's own classification: schema-shape failures (missing table, type
    // mismatch, definition drift) are structural corruption, not generic backend
    // errors. Programmer-error variants (`TableAlreadyOpen`, `TableExists`) stay
    // Backend so they surface as bugs rather than quarantine triggers.
    match err {
        TableError::Storage(storage) => map_storage_err(storage),
        TableError::TableDoesNotExist(_)
        | TableError::TableTypeMismatch { .. }
        | TableError::TableIsMultimap(_)
        | TableError::TableIsNotMultimap(_)
        | TableError::TypeDefinitionChanged { .. } => EventStoreError::Corrupted(err.to_string()),
        other => EventStoreError::Backend(other.to_string()),
    }
}

fn map_commit_err(err: CommitError) -> EventStoreError {
    match err {
        CommitError::Storage(storage) => map_storage_err(storage),
        other => EventStoreError::Backend(other.to_string()),
    }
}

fn is_run_file(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("redb")
        && path
            .file_name()
            .and_then(|s| s.to_str())
            .is_none_or(|name| !name.ends_with(".markers.redb"))
}

fn map_transaction_err(err: TransactionError) -> EventStoreError {
    match err {
        TransactionError::Storage(storage) => map_storage_err(storage),
        other => EventStoreError::Backend(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    fn raw_run_path(base: &Path, run_id: &str) -> PathBuf {
        let dir = base.join("trader-001");
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir.join(format!("{run_id}.redb"))
    }

    fn create_pre_codec_run_file(path: &Path) {
        let entries: TableDefinition<u64, &[u8]> = TableDefinition::new("entries");
        let manifest: TableDefinition<&str, &[u8]> = TableDefinition::new("manifest");
        let db = Database::create(path).expect("create redb");
        let txn = db.begin_write().expect("begin write");
        {
            txn.open_table(entries).expect("open entries");
            let mut table = txn.open_table(manifest).expect("open manifest");
            table
                .insert("current", b"old-format".as_slice())
                .expect("insert");
        }
        txn.commit().expect("commit");
    }

    #[rstest]
    fn read_run_manifest_rejects_store_without_format_marker() {
        let tmp = TempDir::new().expect("tempdir");
        let path = raw_run_path(tmp.path(), "run-old-format");
        create_pre_codec_run_file(&path);

        let err = RedbBackend::read_run_manifest(&path).expect_err("must reject old format");

        match err {
            EventStoreError::Corrupted(msg) => {
                assert!(msg.contains("regenerated"), "msg was: {msg}");
            }
            other => panic!("expected Corrupted, was {other:?}"),
        }
    }

    #[rstest]
    #[case::file_too_large(ErrorKind::FileTooLarge, true)]
    #[case::storage_full(ErrorKind::StorageFull, true)]
    #[case::quota_exceeded(ErrorKind::QuotaExceeded, true)]
    #[case::other(ErrorKind::Other, false)]
    #[case::not_found(ErrorKind::NotFound, false)]
    #[case::permission_denied(ErrorKind::PermissionDenied, false)]
    #[case::interrupted(ErrorKind::Interrupted, false)]
    fn is_disk_pressure_matches_documented_kinds(#[case] kind: ErrorKind, #[case] expected: bool) {
        assert_eq!(is_disk_pressure(kind), expected);
    }

    #[rstest]
    fn map_storage_err_classifies_disk_pressure_as_disk() {
        let io_err = std::io::Error::from(ErrorKind::StorageFull);
        let mapped = map_storage_err(StorageError::Io(io_err));

        match mapped {
            EventStoreError::Disk(_) => {}
            other => panic!("expected Disk, was {other:?}"),
        }
    }

    #[rstest]
    fn map_storage_err_classifies_corrupted_as_corrupted() {
        let mapped = map_storage_err(StorageError::Corrupted("boom".to_string()));

        match mapped {
            EventStoreError::Corrupted(msg) => assert!(msg.contains("boom")),
            other => panic!("expected Corrupted, was {other:?}"),
        }
    }

    #[rstest]
    fn map_storage_err_falls_back_to_backend_for_unrelated_io() {
        let io_err = std::io::Error::from(ErrorKind::PermissionDenied);
        let mapped = map_storage_err(StorageError::Io(io_err));

        match mapped {
            EventStoreError::Backend(_) => {}
            other => panic!("expected Backend, was {other:?}"),
        }
    }
}
