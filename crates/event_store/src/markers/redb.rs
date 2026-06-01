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

//! redb-backed marker backend for the data marker sidecar.

use std::{
    fmt::Debug,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use bincode::config::{Configuration, standard};
use redb::{
    CommitError, Database, DatabaseError, Durability, ReadOnlyDatabase, ReadTransaction,
    ReadableDatabase, ReadableTable, StorageError, TableDefinition, TableError, TransactionError,
    WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    error::EventStoreError,
    manifest::RunStatus,
    markers::{
        DataCursorSnapshot, HiFiMarker, MarkerBackend, MarkerGap, MarkerManifest,
        StoredMarkerRecord, StreamDictEntry,
    },
};

const CURSOR_SNAPSHOTS_TABLE: TableDefinition<u64, &[u8]> =
    TableDefinition::new("cursor_snapshots");
const HIFI_MARKERS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("hifi_markers");
const MARKER_GAPS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("marker_gaps");
const STREAM_DICT_TABLE: TableDefinition<u32, &[u8]> = TableDefinition::new("stream_dict");
const MARKER_MANIFEST_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("marker_manifest");

const MANIFEST_KEY: &str = "current";

const BINCODE_CONFIG: Configuration = standard();

/// On-disk [`MarkerBackend`] backed by a per-run [`redb`] marker sidecar file.
///
/// The caller supplies the full file path, normally
/// `<base>/<instance_id>/<run_id>.markers.redb`. The backend creates the parent directory on
/// first open, stores every durable record with its precomputed 32-byte integrity hash, and
/// commits each mutation with [`Durability::Immediate`]. Reopening an unsealed marker file seals
/// it as [`RunStatus::CrashedRecovered`] and returns `Ok(())`, because marker files never block
/// trader boot.
#[derive(Debug)]
pub struct RedbMarkerBackend {
    file_path: PathBuf,
    state: Option<RunState>,
}

#[derive(Debug)]
struct RunState {
    db: MarkerDatabase,
    manifest: MarkerManifest,
    file_path: PathBuf,
}

enum MarkerDatabase {
    ReadWrite(Database),
    ReadOnly(ReadOnlyDatabase),
}

impl Debug for MarkerDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadWrite(_) => f.write_str("MarkerDatabase::ReadWrite"),
            Self::ReadOnly(_) => f.write_str("MarkerDatabase::ReadOnly"),
        }
    }
}

impl MarkerDatabase {
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

impl RedbMarkerBackend {
    /// Creates a backend for the supplied marker sidecar file path.
    #[must_use]
    pub fn new(file_path: impl Into<PathBuf>) -> Self {
        Self {
            file_path: file_path.into(),
            state: None,
        }
    }

    /// Returns the path of the currently open marker file.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when no run is open.
    pub fn current_path(&self) -> Result<&Path, EventStoreError> {
        Ok(self.state()?.file_path.as_path())
    }

    /// Opens an existing marker sidecar file read-only for verification.
    ///
    /// Marker verification must not seal, recover, or otherwise mutate the sidecar it scans. This
    /// constructor accepts any manifest status and leaves lifecycle policy to the caller's report.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError::Backend`] when the marker file is missing, and
    /// [`EventStoreError::Corrupted`] when the file lacks a marker manifest or fails to decode.
    pub fn open_read_only_file(path: impl Into<PathBuf>) -> Result<Self, EventStoreError> {
        let path = path.into();
        if !path.exists() {
            return Err(EventStoreError::Backend(format!(
                "no marker run file at {}",
                path.display()
            )));
        }

        let db = ReadOnlyDatabase::open(&path).map_err(map_database_err)?;
        let manifest = Self::read_manifest(&db)?.ok_or_else(|| {
            EventStoreError::Corrupted(format!(
                "missing marker manifest in run file at {}",
                path.display()
            ))
        })?;

        Ok(Self {
            file_path: path.clone(),
            state: Some(RunState {
                db: MarkerDatabase::ReadOnly(db),
                manifest,
                file_path: path,
            }),
        })
    }

    fn state(&self) -> Result<&RunState, EventStoreError> {
        self.state
            .as_ref()
            .ok_or_else(|| EventStoreError::Backend("no marker run open".to_string()))
    }

    fn state_mut(&mut self) -> Result<&mut RunState, EventStoreError> {
        self.state
            .as_mut()
            .ok_or_else(|| EventStoreError::Backend("no marker run open".to_string()))
    }

    fn writable_state(&mut self) -> Result<&mut RunState, EventStoreError> {
        let state = self.state_mut()?;
        if state.manifest.is_sealed() {
            return Err(EventStoreError::Closed);
        }
        state.db.read_write()?;
        Ok(state)
    }

    fn initialize_fresh(db: &Database, manifest: &MarkerManifest) -> Result<(), EventStoreError> {
        let txn = begin_immediate_write(db)?;
        {
            txn.open_table(CURSOR_SNAPSHOTS_TABLE)
                .map_err(map_table_err)?;
            txn.open_table(HIFI_MARKERS_TABLE).map_err(map_table_err)?;
            txn.open_table(MARKER_GAPS_TABLE).map_err(map_table_err)?;
            txn.open_table(STREAM_DICT_TABLE).map_err(map_table_err)?;
        }
        insert_marker_manifest(&txn, manifest)?;
        txn.commit().map_err(map_commit_err)?;
        Ok(())
    }

    fn write_manifest(db: &Database, manifest: &MarkerManifest) -> Result<(), EventStoreError> {
        let txn = begin_immediate_write(db)?;
        insert_marker_manifest(&txn, manifest)?;
        txn.commit().map_err(map_commit_err)?;
        Ok(())
    }

    fn read_manifest<D: ReadableDatabase + ?Sized>(
        db: &D,
    ) -> Result<Option<MarkerManifest>, EventStoreError> {
        let txn = db.begin_read().map_err(map_transaction_err)?;
        let table = txn
            .open_table(MARKER_MANIFEST_TABLE)
            .map_err(map_table_err)?;
        let Some(value) = table.get(MANIFEST_KEY).map_err(map_storage_err)? else {
            return Ok(None);
        };
        let bytes = value.value();
        Ok(Some(decode_value("marker manifest", bytes)?))
    }
}

impl MarkerBackend for RedbMarkerBackend {
    fn open_run(&mut self, mut manifest: MarkerManifest) -> Result<(), EventStoreError> {
        self.state = None;

        if let Some(parent) = self
            .file_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|e| {
                let msg = format!("create dir {}: {e}", parent.display());

                if is_disk_pressure(e.kind()) {
                    EventStoreError::Disk(msg)
                } else {
                    EventStoreError::Backend(msg)
                }
            })?;
        }

        let path = self.file_path.clone();
        let path_existed = path.exists();
        let db = Database::create(&path).map_err(map_database_err)?;

        if path_existed {
            let on_disk = Self::read_manifest(&db)?.ok_or_else(|| {
                EventStoreError::Corrupted(format!(
                    "missing marker manifest in existing run file at {}",
                    path.display()
                ))
            })?;

            if on_disk.run_id != manifest.run_id {
                return Err(EventStoreError::Backend(format!(
                    "marker run file at {} belongs to run_id {}, not {}",
                    path.display(),
                    on_disk.run_id,
                    manifest.run_id
                )));
            }

            let mut opened = on_disk;
            if matches!(opened.status, RunStatus::Running) {
                opened.status = RunStatus::CrashedRecovered;
                Self::write_manifest(&db, &opened)?;
            }

            self.state = Some(RunState {
                db: MarkerDatabase::ReadWrite(db),
                manifest: opened,
                file_path: path,
            });
            return Ok(());
        }

        manifest.status = RunStatus::Running;
        manifest.snapshot_count = 0;
        manifest.hifi_count = 0;
        manifest.gap_count = 0;
        manifest.dict_count = 0;
        Self::initialize_fresh(&db, &manifest)?;

        self.state = Some(RunState {
            db: MarkerDatabase::ReadWrite(db),
            manifest,
            file_path: path,
        });
        Ok(())
    }

    fn append_snapshot(
        &mut self,
        snapshot: &DataCursorSnapshot,
        hash: [u8; 32],
    ) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        let stored = StoredMarkerRecord {
            record: snapshot.clone(),
            hash,
        };
        let record_bytes = encode_value("cursor snapshot", &stored)?;
        let mut updated = state.manifest.clone();
        updated.snapshot_count += 1;
        let db = state.db.read_write()?;
        append_u64_marker_record(
            db,
            CURSOR_SNAPSHOTS_TABLE,
            snapshot.marker_seq,
            record_bytes.as_slice(),
            &updated,
        )?;

        state.manifest = updated;
        Ok(())
    }

    fn append_hifi(&mut self, marker: &HiFiMarker, hash: [u8; 32]) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        let stored = StoredMarkerRecord {
            record: marker.clone(),
            hash,
        };
        let record_bytes = encode_value("hifi marker", &stored)?;
        let mut updated = state.manifest.clone();
        updated.hifi_count += 1;
        let db = state.db.read_write()?;
        append_u64_marker_record(
            db,
            HIFI_MARKERS_TABLE,
            marker.marker_seq,
            record_bytes.as_slice(),
            &updated,
        )?;

        state.manifest = updated;
        Ok(())
    }

    fn append_gap(&mut self, gap: &MarkerGap, hash: [u8; 32]) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        let stored = StoredMarkerRecord {
            record: gap.clone(),
            hash,
        };
        let record_bytes = encode_value("marker gap", &stored)?;
        let mut updated = state.manifest.clone();
        updated.gap_count += 1;
        let db = state.db.read_write()?;
        append_u64_marker_record(
            db,
            MARKER_GAPS_TABLE,
            gap.from_marker_seq,
            record_bytes.as_slice(),
            &updated,
        )?;

        state.manifest = updated;
        Ok(())
    }

    fn put_dict(&mut self, entry: &StreamDictEntry, hash: [u8; 32]) -> Result<(), EventStoreError> {
        let state = self.writable_state()?;
        let stored = StoredMarkerRecord {
            record: entry.clone(),
            hash,
        };
        let record_bytes = encode_value("stream dict entry", &stored)?;
        let mut updated = state.manifest.clone();
        updated.dict_count += 1;
        let mut inserted = false;

        let db = state.db.read_write()?;
        let txn = begin_immediate_write(db)?;
        {
            let mut table = txn.open_table(STREAM_DICT_TABLE).map_err(map_table_err)?;
            let already = table.get(entry.slot).map_err(map_storage_err)?.is_some();

            if !already {
                table
                    .insert(entry.slot, record_bytes.as_slice())
                    .map_err(map_storage_err)?;
                inserted = true;
            }
        }

        if inserted {
            insert_marker_manifest(&txn, &updated)?;
        }

        txn.commit().map_err(map_commit_err)?;

        if inserted {
            state.manifest = updated;
        }
        Ok(())
    }

    fn scan_snapshots(&self) -> Result<Vec<DataCursorSnapshot>, EventStoreError> {
        let out = self
            .scan_snapshot_records()?
            .unwrap_or_default()
            .into_iter()
            .map(|stored| stored.record)
            .collect();
        Ok(out)
    }

    fn scan_snapshot_records(
        &self,
    ) -> Result<Option<Vec<StoredMarkerRecord<DataCursorSnapshot>>>, EventStoreError> {
        let state = self.state()?;
        let txn = state.db.begin_read()?;
        let table = txn
            .open_table(CURSOR_SNAPSHOTS_TABLE)
            .map_err(map_table_err)?;
        let iter = table.iter().map_err(map_storage_err)?;
        let mut out = Vec::new();

        for row in iter {
            let (_, value) = row.map_err(map_storage_err)?;
            let stored: StoredMarkerRecord<DataCursorSnapshot> =
                decode_value("cursor snapshot", value.value())?;
            out.push(stored);
        }
        Ok(Some(out))
    }

    fn scan_hifi(&self) -> Result<Vec<HiFiMarker>, EventStoreError> {
        let out = self
            .scan_hifi_records()?
            .unwrap_or_default()
            .into_iter()
            .map(|stored| stored.record)
            .collect();
        Ok(out)
    }

    fn scan_hifi_records(
        &self,
    ) -> Result<Option<Vec<StoredMarkerRecord<HiFiMarker>>>, EventStoreError> {
        let state = self.state()?;
        let txn = state.db.begin_read()?;
        let table = txn.open_table(HIFI_MARKERS_TABLE).map_err(map_table_err)?;
        let iter = table.iter().map_err(map_storage_err)?;
        let mut out = Vec::new();

        for row in iter {
            let (_, value) = row.map_err(map_storage_err)?;
            let stored: StoredMarkerRecord<HiFiMarker> =
                decode_value("hifi marker", value.value())?;
            out.push(stored);
        }
        Ok(Some(out))
    }

    fn scan_gaps(&self) -> Result<Vec<MarkerGap>, EventStoreError> {
        let out = self
            .scan_gap_records()?
            .unwrap_or_default()
            .into_iter()
            .map(|stored| stored.record)
            .collect();
        Ok(out)
    }

    fn scan_gap_records(
        &self,
    ) -> Result<Option<Vec<StoredMarkerRecord<MarkerGap>>>, EventStoreError> {
        let state = self.state()?;
        let txn = state.db.begin_read()?;
        let table = txn.open_table(MARKER_GAPS_TABLE).map_err(map_table_err)?;
        let iter = table.iter().map_err(map_storage_err)?;
        let mut out = Vec::new();

        for row in iter {
            let (_, value) = row.map_err(map_storage_err)?;
            let stored: StoredMarkerRecord<MarkerGap> = decode_value("marker gap", value.value())?;
            out.push(stored);
        }
        Ok(Some(out))
    }

    fn scan_dict(&self) -> Result<Vec<StreamDictEntry>, EventStoreError> {
        let out = self
            .scan_dict_records()?
            .unwrap_or_default()
            .into_iter()
            .map(|stored| stored.record)
            .collect();
        Ok(out)
    }

    fn scan_dict_records(
        &self,
    ) -> Result<Option<Vec<StoredMarkerRecord<StreamDictEntry>>>, EventStoreError> {
        let state = self.state()?;
        let txn = state.db.begin_read()?;
        let table = txn.open_table(STREAM_DICT_TABLE).map_err(map_table_err)?;
        let iter = table.iter().map_err(map_storage_err)?;
        let mut out = Vec::new();

        for row in iter {
            let (_, value) = row.map_err(map_storage_err)?;
            let stored: StoredMarkerRecord<StreamDictEntry> =
                decode_value("stream dict entry", value.value())?;
            out.push(stored);
        }
        Ok(Some(out))
    }

    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
        let state = self.state_mut()?;

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
        let db = state.db.read_write()?;
        Self::write_manifest(db, &updated)?;
        state.manifest = updated;
        Ok(())
    }

    fn manifest(&self) -> Result<MarkerManifest, EventStoreError> {
        Ok(self.state()?.manifest.clone())
    }
}

fn append_u64_marker_record(
    db: &Database,
    table_def: TableDefinition<u64, &[u8]>,
    key: u64,
    record_bytes: &[u8],
    manifest: &MarkerManifest,
) -> Result<(), EventStoreError> {
    let txn = begin_immediate_write(db)?;
    {
        let mut table = txn.open_table(table_def).map_err(map_table_err)?;
        table.insert(key, record_bytes).map_err(map_storage_err)?;
    }
    insert_marker_manifest(&txn, manifest)?;
    txn.commit().map_err(map_commit_err)?;
    Ok(())
}

fn begin_immediate_write(db: &Database) -> Result<WriteTransaction, EventStoreError> {
    let mut txn = db.begin_write().map_err(map_transaction_err)?;
    txn.set_durability(Durability::Immediate)
        .map_err(|e| EventStoreError::Backend(format!("set durability: {e}")))?;
    Ok(txn)
}

fn insert_marker_manifest(
    txn: &WriteTransaction,
    manifest: &MarkerManifest,
) -> Result<(), EventStoreError> {
    let manifest_bytes = encode_value("marker manifest", manifest)?;
    let mut table = txn
        .open_table(MARKER_MANIFEST_TABLE)
        .map_err(map_table_err)?;
    table
        .insert(MANIFEST_KEY, manifest_bytes.as_slice())
        .map_err(map_storage_err)?;
    Ok(())
}

fn encode_value<T: Serialize>(label: &str, value: &T) -> Result<Vec<u8>, EventStoreError> {
    bincode::serde::encode_to_vec(value, BINCODE_CONFIG)
        .map_err(|e| EventStoreError::Backend(format!("encode {label}: {e}")))
}

fn decode_value<T: DeserializeOwned>(label: &str, bytes: &[u8]) -> Result<T, EventStoreError> {
    let (value, _) = bincode::serde::decode_from_slice::<T, _>(bytes, BINCODE_CONFIG)
        .map_err(|e| EventStoreError::Corrupted(format!("decode {label}: {e}")))?;
    Ok(value)
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

fn map_table_err(err: TableError) -> EventStoreError {
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

fn map_transaction_err(err: TransactionError) -> EventStoreError {
    match err {
        TransactionError::Storage(storage) => map_storage_err(storage),
        other => EventStoreError::Backend(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fmt::Debug,
        path::{Path, PathBuf},
    };

    use nautilus_core::UnixNanos;
    use rstest::{fixture, rstest};
    use tempfile::TempDir;

    use super::RedbMarkerBackend;
    use crate::{
        error::EventStoreError,
        manifest::RunStatus,
        markers::{
            DataClass, DataCursorSnapshot, HiFiMarker, MarkerBackend, MarkerGap, MarkerGapReason,
            MarkerManifest, MarkerVerifier, StreamCursor, StreamDictEntry, compute_dict_hash,
            compute_gap_hash, compute_hifi_hash, compute_marker_hash,
        },
    };

    fn marker_path(base: &Path, instance_id: &str, run_id: &str) -> PathBuf {
        base.join(instance_id)
            .join(format!("{run_id}.markers.redb"))
    }

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

    fn snapshot(marker_seq: u64, event_seq_before: u64) -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq,
            event_seq_before,
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
            event_seq_before: 42,
            slot: 0,
            ts_event: UnixNanos::from(1_700_000_000_000_000_100 + marker_seq),
            ts_init: UnixNanos::from(1_700_000_000_000_000_200 + marker_seq),
            same_ts_ordinal: 0,
            record_fingerprint: [7u8; 32],
        }
    }

    fn dict(slot: u32, data_cls: DataClass, identifier: &str) -> StreamDictEntry {
        StreamDictEntry {
            slot,
            data_cls,
            identifier: identifier.to_string(),
        }
    }

    fn gap(from_marker_seq: u64, to_marker_seq: u64) -> MarkerGap {
        MarkerGap {
            from_marker_seq,
            to_marker_seq,
            reason: MarkerGapReason::Overflow,
        }
    }

    fn assert_closed<T: Debug>(result: Result<T, EventStoreError>) {
        match result {
            Err(EventStoreError::Closed) => {}
            other => panic!("expected Closed error, was {other:?}"),
        }
    }

    fn assert_backend_message<T: Debug>(result: Result<T, EventStoreError>, expected: &str) {
        match result {
            Err(EventStoreError::Backend(message)) => assert_eq!(message, expected),
            other => panic!("expected Backend error, was {other:?}"),
        }
    }

    #[fixture]
    fn temp_dir() -> TempDir {
        TempDir::new().expect("create temp dir")
    }

    #[rstest]
    fn roundtrip_snapshots_dict_gaps_through_redb(temp_dir: TempDir) {
        let run_id = "1700000000-redb-roundtrip";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let s2 = snapshot(2, 20);
        let s1 = snapshot(1, 10);
        let h2 = hifi(4);
        let h1 = hifi(3);
        let g2 = gap(20, 24);
        let g1 = gap(5, 8);
        let first_dict = dict(0, DataClass::Quote, "ETHUSDT.BINANCE");
        let remap_dict = dict(0, DataClass::Trade, "BTCUSDT.BINANCE");
        let second_dict = dict(1, DataClass::Trade, "BTCUSDT.BINANCE");

        {
            let mut backend = RedbMarkerBackend::new(&path);
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_snapshot(&s2, compute_marker_hash(&s2))
                .expect("append snapshot 2");
            backend
                .append_snapshot(&s1, compute_marker_hash(&s1))
                .expect("append snapshot 1");
            backend
                .append_hifi(&h2, compute_hifi_hash(&h2))
                .expect("append hifi 4");
            backend
                .append_hifi(&h1, compute_hifi_hash(&h1))
                .expect("append hifi 3");
            backend
                .append_gap(&g2, compute_gap_hash(&g2))
                .expect("append gap 2");
            backend
                .append_gap(&g1, compute_gap_hash(&g1))
                .expect("append gap 1");
            backend
                .put_dict(&first_dict, compute_dict_hash(&first_dict))
                .expect("put dict 0");
            backend
                .put_dict(&remap_dict, compute_dict_hash(&remap_dict))
                .expect("re-put dict 0");
            backend
                .put_dict(&second_dict, compute_dict_hash(&second_dict))
                .expect("put dict 1");
        }

        let mut reopened = RedbMarkerBackend::new(&path);
        reopened.open_run(manifest(run_id)).expect("reopen run");

        assert_eq!(
            reopened.scan_snapshots().expect("scan snapshots"),
            vec![s1, s2]
        );
        assert_eq!(reopened.scan_hifi().expect("scan hifi"), vec![h1, h2]);
        assert_eq!(reopened.scan_gaps().expect("scan gaps"), vec![g1, g2]);
        assert_eq!(
            reopened.scan_dict().expect("scan dict"),
            vec![first_dict, second_dict]
        );
    }

    #[rstest]
    fn open_running_then_reopen_reports_crashed(temp_dir: TempDir) {
        let run_id = "1700000000-redb-crash";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let s1 = snapshot(1, 10);

        {
            let mut backend = RedbMarkerBackend::new(&path);
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_snapshot(&s1, compute_marker_hash(&s1))
                .expect("append snapshot");
        }

        let mut reopened = RedbMarkerBackend::new(&path);
        reopened.open_run(manifest(run_id)).expect("reopen run");

        assert_eq!(
            reopened.manifest().expect("manifest").status,
            RunStatus::CrashedRecovered
        );
        assert_eq!(reopened.scan_snapshots().expect("scan snapshots"), vec![s1]);
    }

    #[rstest]
    fn read_only_marker_backend_rejects_writes_without_recovery_mutation(temp_dir: TempDir) {
        let run_id = "1700000000-redb-read-only-running";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let s1 = snapshot(1, 10);

        {
            let mut backend = RedbMarkerBackend::new(&path);
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_snapshot(&s1, compute_marker_hash(&s1))
                .expect("append snapshot");
        }

        let mut read_only =
            RedbMarkerBackend::open_read_only_file(&path).expect("open read-only marker file");
        let s2 = snapshot(2, 11);

        assert_eq!(
            read_only.manifest().expect("manifest").status,
            RunStatus::Running,
        );
        assert_closed(read_only.append_snapshot(&s2, compute_marker_hash(&s2)));
        assert_closed(read_only.seal(RunStatus::Ended));
        assert_eq!(
            read_only.scan_snapshots().expect("scan snapshots"),
            vec![s1]
        );
    }

    #[rstest]
    fn seal_persists_manifest_counts(temp_dir: TempDir) {
        let run_id = "1700000000-redb-seal";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let s1 = snapshot(1, 10);
        let h1 = hifi(2);
        let g1 = gap(3, 5);
        let d1 = dict(0, DataClass::Quote, "ETHUSDT.BINANCE");

        {
            let mut backend = RedbMarkerBackend::new(&path);
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_snapshot(&s1, compute_marker_hash(&s1))
                .expect("append snapshot");
            backend
                .append_hifi(&h1, compute_hifi_hash(&h1))
                .expect("append hifi");
            backend
                .append_gap(&g1, compute_gap_hash(&g1))
                .expect("append gap");
            backend
                .put_dict(&d1, compute_dict_hash(&d1))
                .expect("put dict");
            backend.seal(RunStatus::Ended).expect("seal");
        }

        let mut reopened = RedbMarkerBackend::new(&path);
        reopened
            .open_run(manifest(run_id))
            .expect("reopen sealed run");
        let persisted = reopened.manifest().expect("manifest");

        assert_eq!(persisted.status, RunStatus::Ended);
        assert_eq!(persisted.snapshot_count, 1);
        assert_eq!(persisted.hifi_count, 1);
        assert_eq!(persisted.gap_count, 1);
        assert_eq!(persisted.dict_count, 1);
    }

    #[rstest]
    fn unsealed_appends_persist_manifest_counts_for_verifier(temp_dir: TempDir) {
        let run_id = "1700000000-redb-unsealed-counts";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let s1 = snapshot(1, 1);
        let h1 = hifi(2);
        let g1 = gap(3, 3);
        let first_dict = dict(0, DataClass::Quote, "ETHUSDT.BINANCE");
        let remap_dict = dict(0, DataClass::Trade, "BTCUSDT.BINANCE");
        let second_dict = dict(1, DataClass::Trade, "BTCUSDT.BINANCE");

        {
            let mut backend = RedbMarkerBackend::new(&path);
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_snapshot(&s1, compute_marker_hash(&s1))
                .expect("append snapshot");
            backend
                .append_hifi(&h1, compute_hifi_hash(&h1))
                .expect("append hifi");
            backend
                .append_gap(&g1, compute_gap_hash(&g1))
                .expect("append gap");
            backend
                .put_dict(&first_dict, compute_dict_hash(&first_dict))
                .expect("put dict 0");
            backend
                .put_dict(&remap_dict, compute_dict_hash(&remap_dict))
                .expect("re-put dict 0");
            backend
                .put_dict(&second_dict, compute_dict_hash(&second_dict))
                .expect("put dict 1");
        }

        let read_only =
            RedbMarkerBackend::open_read_only_file(&path).expect("open read-only marker file");
        let persisted = read_only.manifest().expect("manifest");
        let report = MarkerVerifier::scan(&read_only, 42).expect("scan");

        assert_eq!(persisted.status, RunStatus::Running);
        assert_eq!(persisted.snapshot_count, 1);
        assert_eq!(persisted.hifi_count, 1);
        assert_eq!(persisted.gap_count, 1);
        assert_eq!(persisted.dict_count, 2);
        assert_eq!(report.snapshots_scanned, 1);
        assert_eq!(report.hifi_scanned, 1);
        assert_eq!(report.gaps_scanned, 1);
        assert_eq!(report.dict_entries_scanned, 2);
        assert!(report.is_clean(), "findings was: {:?}", report.findings);
    }

    #[rstest]
    fn sealed_backend_rejects_writes(temp_dir: TempDir) {
        let run_id = "1700000000-redb-sealed-writes";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let mut backend = RedbMarkerBackend::new(&path);
        backend.open_run(manifest(run_id)).expect("open run");
        backend.seal(RunStatus::Ended).expect("seal");

        let s1 = snapshot(1, 10);
        let h1 = hifi(2);
        let g1 = gap(3, 5);
        let d1 = dict(0, DataClass::Quote, "ETHUSDT.BINANCE");

        assert_closed(backend.append_snapshot(&s1, compute_marker_hash(&s1)));
        assert_closed(backend.append_hifi(&h1, compute_hifi_hash(&h1)));
        assert_closed(backend.append_gap(&g1, compute_gap_hash(&g1)));
        assert_closed(backend.put_dict(&d1, compute_dict_hash(&d1)));

        let persisted = backend.manifest().expect("manifest");
        assert_eq!(persisted.status, RunStatus::Ended);
        assert_eq!(persisted.snapshot_count, 0);
        assert_eq!(persisted.hifi_count, 0);
        assert_eq!(persisted.gap_count, 0);
        assert_eq!(persisted.dict_count, 0);
    }

    #[rstest]
    fn crash_recovered_backend_rejects_writes(temp_dir: TempDir) {
        let run_id = "1700000000-redb-crash-closed";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let s1 = snapshot(1, 10);
        let s2 = snapshot(2, 20);

        {
            let mut backend = RedbMarkerBackend::new(&path);
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_snapshot(&s1, compute_marker_hash(&s1))
                .expect("append snapshot");
        }

        let mut reopened = RedbMarkerBackend::new(&path);
        reopened.open_run(manifest(run_id)).expect("reopen run");

        assert_eq!(
            reopened.manifest().expect("manifest").status,
            RunStatus::CrashedRecovered
        );
        assert_closed(reopened.append_snapshot(&s2, compute_marker_hash(&s2)));
        assert_eq!(reopened.scan_snapshots().expect("scan snapshots"), vec![s1]);
    }

    #[rstest]
    fn seal_running_is_rejected_and_does_not_close(temp_dir: TempDir) {
        let run_id = "1700000000-redb-running-seal";
        let path = marker_path(temp_dir.path(), "trader-001", run_id);
        let s1 = snapshot(1, 10);
        let mut backend = RedbMarkerBackend::new(&path);
        backend.open_run(manifest(run_id)).expect("open run");

        assert_backend_message(
            backend.seal(RunStatus::Running),
            "seal status must be a terminal state, was Running",
        );
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append after rejected seal");
        backend.seal(RunStatus::Ended).expect("seal ended");

        let persisted = backend.manifest().expect("manifest");
        assert_eq!(persisted.status, RunStatus::Ended);
        assert_eq!(persisted.snapshot_count, 1);
    }

    #[rstest]
    fn unopened_backend_reports_backend_error(temp_dir: TempDir) {
        let path = marker_path(temp_dir.path(), "trader-001", "1700000000-redb-unopened");
        let mut backend = RedbMarkerBackend::new(&path);
        let s1 = snapshot(1, 10);
        let h1 = hifi(2);
        let g1 = gap(3, 5);
        let d1 = dict(0, DataClass::Quote, "ETHUSDT.BINANCE");

        assert_backend_message(backend.current_path(), "no marker run open");
        assert_backend_message(backend.manifest(), "no marker run open");
        assert_backend_message(backend.scan_snapshots(), "no marker run open");
        assert_backend_message(backend.scan_hifi(), "no marker run open");
        assert_backend_message(backend.scan_gaps(), "no marker run open");
        assert_backend_message(backend.scan_dict(), "no marker run open");
        assert_backend_message(backend.seal(RunStatus::Ended), "no marker run open");
        assert_backend_message(
            backend.append_snapshot(&s1, compute_marker_hash(&s1)),
            "no marker run open",
        );
        assert_backend_message(
            backend.append_hifi(&h1, compute_hifi_hash(&h1)),
            "no marker run open",
        );
        assert_backend_message(
            backend.append_gap(&g1, compute_gap_hash(&g1)),
            "no marker run open",
        );
        assert_backend_message(
            backend.put_dict(&d1, compute_dict_hash(&d1)),
            "no marker run open",
        );
    }
}
