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

//! Bootstrap replay for restoring cache state after a cache-owned snapshot.
//!
//! This module is deliberately state-only: it consumes event-store entries, decodes the
//! cache-affecting payloads, and mutates [`nautilus_common::cache::Cache`] directly. It
//! does not publish to the live message bus, send commands, invoke adapters, or submit
//! entries back into the event store.

use std::{fmt::Display, path::PathBuf};

use bytes::Bytes;
use nautilus_common::cache::Cache;
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::OmsType,
    events::{AccountState, OrderEventAny, OrderFilled, OrderInitialized, PositionAdjusted},
    orders::OrderAny,
    position::Position,
};
use serde::de::DeserializeOwned;

use crate::{
    RedbBackend,
    backend::{EventStore, ScanDirection},
    capture::builtins::{
        PAYLOAD_TYPE_ACCOUNT_STATE, PAYLOAD_TYPE_ORDER_ACCEPTED,
        PAYLOAD_TYPE_ORDER_CANCEL_REJECTED, PAYLOAD_TYPE_ORDER_CANCELED, PAYLOAD_TYPE_ORDER_DENIED,
        PAYLOAD_TYPE_ORDER_EMULATED, PAYLOAD_TYPE_ORDER_EXPIRED, PAYLOAD_TYPE_ORDER_FILLED,
        PAYLOAD_TYPE_ORDER_INITIALIZED, PAYLOAD_TYPE_ORDER_MODIFY_REJECTED,
        PAYLOAD_TYPE_ORDER_PENDING_CANCEL, PAYLOAD_TYPE_ORDER_PENDING_UPDATE,
        PAYLOAD_TYPE_ORDER_REJECTED, PAYLOAD_TYPE_ORDER_RELEASED, PAYLOAD_TYPE_ORDER_SUBMITTED,
        PAYLOAD_TYPE_ORDER_TRIGGERED, PAYLOAD_TYPE_ORDER_UPDATED, PAYLOAD_TYPE_POSITION_ADJUSTED,
    },
    entry::EventStoreEntry,
    error::EventStoreError,
    manifest::{RunManifest, RunStatus},
    reader::{EventStoreReader, SnapshotReplayPlan},
    snapshot::{SnapshotAnchor, compute_snapshot_content_hash},
};

/// Summary of a cache snapshot-tail replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CacheReplayReport {
    /// Replay bounds derived from the latest snapshot anchor.
    pub plan: SnapshotReplayPlan,
    /// Number of entries applied to cache state.
    pub applied_entries: usize,
    /// Number of event-store entries that do not have a cache replay rule yet.
    pub ignored_entries: usize,
}

/// Summary of an event-store replay source and cache restore.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventStoreReplayReport {
    /// Manifest of the sealed replay source.
    pub manifest: RunManifest,
    /// Cache snapshot-tail replay result.
    pub cache: CacheReplayReport,
}

/// Replay input scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplayScope {
    /// Event-store entries only.
    Forensics,
    /// Event-store entries plus selected data catalog slices for decision analysis.
    Decision,
    /// Event-store entries plus all selected catalog slices for an incident window.
    FullIncident,
}

impl Display for ReplayScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forensics => f.write_str("forensics"),
            Self::Decision => f.write_str("decision"),
            Self::FullIncident => f.write_str("full_incident"),
        }
    }
}

/// Inclusive event-store `seq` bounds for replay input scans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaySeqRange {
    /// First event-store `seq` to scan.
    pub from_seq: u64,
    /// Last event-store `seq` to scan.
    pub to_seq: u64,
}

impl ReplaySeqRange {
    /// Builds inclusive event-store `seq` bounds.
    #[must_use]
    pub const fn new(from_seq: u64, to_seq: u64) -> Self {
        Self { from_seq, to_seq }
    }
}

/// Inclusive nanosecond time bounds for catalog slice selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplayTimeRange {
    /// First catalog timestamp to include.
    pub start: UnixNanos,
    /// Last catalog timestamp to include.
    pub end: UnixNanos,
}

impl ReplayTimeRange {
    /// Builds inclusive nanosecond time bounds.
    #[must_use]
    pub const fn new(start: UnixNanos, end: UnixNanos) -> Self {
        Self { start, end }
    }

    fn from_entry(entry: &EventStoreEntry) -> Self {
        Self {
            start: entry.ts_init,
            end: entry.ts_init,
        }
    }

    fn include_entry(&mut self, entry: &EventStoreEntry) {
        self.start = self.start.min(entry.ts_init);
        self.end = self.end.max(entry.ts_init);
    }
}

/// Caller-selected data catalog slice before replay window defaults are applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSliceSelector {
    /// Catalog data class or directory name, such as `quotes`, `trades`, or `bars`.
    pub data_cls: String,
    /// Optional catalog identifiers, such as instrument IDs or bar type strings.
    pub identifiers: Vec<String>,
    /// Optional lower timestamp bound. When absent, the event-store scan lower bound applies.
    pub start: Option<UnixNanos>,
    /// Optional upper timestamp bound. When absent, the event-store scan upper bound applies.
    pub end: Option<UnixNanos>,
    /// Whether loading should fail when the catalog reports no files for this slice.
    pub required: bool,
}

impl CatalogSliceSelector {
    /// Builds a selector for `data_cls` with no identifiers or explicit time bounds.
    pub fn new(data_cls: impl Into<String>) -> Self {
        Self {
            data_cls: data_cls.into(),
            identifiers: Vec::new(),
            start: None,
            end: None,
            required: false,
        }
    }

    /// Adds one catalog identifier to the selector.
    #[must_use]
    pub fn with_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.identifiers.push(identifier.into());
        self
    }

    /// Sets explicit inclusive catalog time bounds.
    #[must_use]
    pub const fn with_time_bounds(mut self, start: UnixNanos, end: UnixNanos) -> Self {
        self.start = Some(start);
        self.end = Some(end);
        self
    }

    /// Marks the selector as required.
    #[must_use]
    pub const fn require_coverage(mut self) -> Self {
        self.required = true;
        self
    }
}

/// Resolved catalog query after replay time bounds have been applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSliceQuery {
    /// Catalog data class or directory name, such as `quotes`, `trades`, or `bars`.
    pub data_cls: String,
    /// Catalog identifiers, such as instrument IDs or bar type strings.
    pub identifiers: Vec<String>,
    /// Inclusive lower timestamp bound.
    pub start: UnixNanos,
    /// Inclusive upper timestamp bound.
    pub end: UnixNanos,
    /// Whether loading should fail when the catalog reports no files for this slice.
    pub required: bool,
}

impl CatalogSliceQuery {
    /// Returns identifiers in the shape expected by catalog APIs.
    #[must_use]
    pub fn identifiers_option(&self) -> Option<Vec<String>> {
        if self.identifiers.is_empty() {
            None
        } else {
            Some(self.identifiers.clone())
        }
    }
}

/// Catalog file and interval coverage for a planned slice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogSliceCoverage {
    /// Catalog files selected for the slice.
    pub files: Vec<String>,
    /// Covered timestamp intervals reported by the catalog.
    pub intervals: Vec<ReplayTimeRange>,
}

impl CatalogSliceCoverage {
    /// Builds coverage from selected catalog files.
    #[must_use]
    pub fn from_files(files: Vec<String>) -> Self {
        Self {
            files,
            intervals: Vec::new(),
        }
    }

    /// Returns whether the catalog found no files for the slice.
    #[must_use]
    pub fn is_missing(&self) -> bool {
        self.files.is_empty()
    }
}

/// Planned catalog slice availability.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogSliceStatus {
    /// The catalog reported files for this slice.
    Available,
    /// The catalog reported no files for this slice.
    Missing,
}

/// Planned catalog slice joined to a replay input scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSlicePlan {
    /// Resolved catalog query.
    pub query: CatalogSliceQuery,
    /// Catalog coverage reported during planning.
    pub coverage: CatalogSliceCoverage,
    /// Slice availability status.
    pub status: CatalogSliceStatus,
}

impl CatalogSlicePlan {
    /// Returns whether the catalog reported no files for this slice.
    #[must_use]
    pub const fn is_missing(&self) -> bool {
        matches!(self.status, CatalogSliceStatus::Missing)
    }
}

/// Opaque catalog record loaded for replay context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogReplayRecord {
    /// Catalog data class or directory name for the record.
    pub data_cls: String,
    /// Optional catalog identifier for the record.
    pub identifier: Option<String>,
    /// Record timestamp used for contextual joins.
    pub ts_init: UnixNanos,
    /// Opaque caller-supplied payload bytes.
    pub payload: Bytes,
}

impl CatalogReplayRecord {
    /// Builds an opaque catalog replay record.
    pub fn new(
        data_cls: impl Into<String>,
        identifier: Option<String>,
        ts_init: UnixNanos,
        payload: Bytes,
    ) -> Self {
        Self {
            data_cls: data_cls.into(),
            identifier,
            ts_init,
            payload,
        }
    }
}

/// Loaded catalog records for one planned slice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogReplaySlice {
    /// Planned catalog slice metadata.
    pub plan: CatalogSlicePlan,
    /// Loaded catalog records.
    pub records: Vec<CatalogReplayRecord>,
}

/// Planned replay inputs for a forensics, decision, or full incident replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayInputPlan {
    /// Explicit replay scope.
    pub scope: ReplayScope,
    /// Requested event-store `seq` bounds.
    pub requested_range: ReplaySeqRange,
    /// Actual event-store range found inside the requested bounds.
    pub event_range: Option<ReplaySeqRange>,
    /// Number of event-store entries found inside the requested bounds.
    pub event_count: usize,
    /// Minimum and maximum event-store `ts_init` values inside the requested bounds.
    pub event_time_range: Option<ReplayTimeRange>,
    /// Catalog slices joined to the event-store scan.
    pub catalog_slices: Vec<CatalogSlicePlan>,
}

impl ReplayInputPlan {
    /// Returns all catalog slices that had no selected files.
    #[must_use]
    pub fn missing_catalog_slices(&self) -> Vec<&CatalogSlicePlan> {
        self.catalog_slices
            .iter()
            .filter(|slice| slice.is_missing())
            .collect()
    }
}

/// Loaded replay inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayInputs {
    /// Explicit replay scope.
    pub scope: ReplayScope,
    /// Event-store entries in durable `seq` order.
    pub entries: Vec<EventStoreEntry>,
    /// Catalog slices loaded as contextual input.
    pub catalog_slices: Vec<CatalogReplaySlice>,
}

/// Read-only catalog source used by catalog-joined replay input loaders.
pub trait ReplayCatalog {
    /// Catalog-specific error type.
    type Error: Display;

    /// Plans one catalog slice without mutating catalog state.
    ///
    /// # Errors
    ///
    /// Returns the catalog implementation's error when slice planning fails.
    fn plan_slice(
        &mut self,
        query: &CatalogSliceQuery,
    ) -> Result<CatalogSliceCoverage, Self::Error>;

    /// Loads records for one planned catalog slice without live venue access.
    ///
    /// # Errors
    ///
    /// Returns the catalog implementation's error when slice loading fails.
    fn load_slice(
        &mut self,
        plan: &CatalogSlicePlan,
    ) -> Result<Vec<CatalogReplayRecord>, Self::Error>;
}

/// Errors surfaced while planning or loading replay inputs.
#[derive(Debug, thiserror::Error)]
pub enum ReplayInputError {
    /// The event-store reader failed.
    #[error(transparent)]
    EventStore(#[from] EventStoreError),
    /// The requested event-store `seq` range is invalid.
    #[error("invalid replay seq range {from_seq}..={to_seq}: {message}")]
    InvalidSeqRange {
        /// Requested lower `seq`.
        from_seq: u64,
        /// Requested upper `seq`.
        to_seq: u64,
        /// Validation failure.
        message: String,
    },
    /// A catalog-joined replay scope had no selected catalog slices.
    #[error("{scope} replay requires at least one selected catalog slice")]
    EmptyCatalogSelection {
        /// Replay scope.
        scope: ReplayScope,
    },
    /// A replay input plan was loaded through the wrong scope-specific API.
    #[error("replay input plan scope {actual} does not match expected scope {expected}")]
    ScopeMismatch {
        /// Expected replay scope.
        expected: ReplayScope,
        /// Actual replay scope.
        actual: ReplayScope,
    },
    /// A catalog slice needs time bounds, but neither selector nor event-store scan supplied them.
    #[error(
        "catalog slice {data_cls} requires explicit time bounds because the replay scan is empty"
    )]
    MissingCatalogTimeBounds {
        /// Catalog data class or directory name.
        data_cls: String,
    },
    /// A catalog slice has an invalid timestamp range.
    #[error("invalid catalog time range for {data_cls}: {start}..={end}")]
    InvalidCatalogTimeRange {
        /// Catalog data class or directory name.
        data_cls: String,
        /// Lower timestamp bound.
        start: u64,
        /// Upper timestamp bound.
        end: u64,
    },
    /// A required catalog slice had no files.
    #[error("required catalog slice {data_cls} is missing for identifiers {identifiers:?}")]
    MissingCatalogSlice {
        /// Catalog data class or directory name.
        data_cls: String,
        /// Catalog identifiers.
        identifiers: Vec<String>,
    },
    /// The catalog source failed.
    #[error("catalog slice {data_cls}: {message}")]
    Catalog {
        /// Catalog data class or directory name.
        data_cls: String,
        /// Catalog error message.
        message: String,
    },
}

/// Errors surfaced while restoring a cache snapshot tail.
#[derive(Debug, thiserror::Error)]
pub enum CacheReplayError {
    /// The event-store reader failed.
    #[error(transparent)]
    EventStore(#[from] EventStoreError),
    /// The caller-provided snapshot restore hook failed.
    #[error("restore cache snapshot {blob_ref}: {message}")]
    SnapshotRestore {
        /// Cache-owned snapshot blob reference.
        blob_ref: String,
        /// Error message returned by the restore hook.
        message: String,
    },
    /// The replay scan yielded an entry outside the derived restore bounds.
    #[error("entry seq {seq} is before replay start seq {from_seq}")]
    UnexpectedSeq {
        /// Entry sequence yielded by the scan.
        seq: u64,
        /// First sequence this replay is allowed to apply.
        from_seq: u64,
    },
    /// A captured payload failed to decode.
    #[error("decode seq {seq} payload_type {payload_type}: {message}")]
    Decode {
        /// Event-store sequence number.
        seq: u64,
        /// Captured payload type tag.
        payload_type: String,
        /// Decode error message.
        message: String,
    },
    /// Applying a decoded payload to the cache failed.
    #[error("apply seq {seq} payload_type {payload_type}: {message}")]
    Apply {
        /// Event-store sequence number.
        seq: u64,
        /// Captured payload type tag.
        payload_type: String,
        /// Apply error message.
        message: String,
    },
}

impl CacheReplayError {
    /// Builds a snapshot-restore error for `anchor`.
    #[must_use]
    pub fn snapshot_restore(anchor: &SnapshotAnchor, error: impl Display) -> Self {
        Self::SnapshotRestore {
            blob_ref: anchor.blob_ref.clone(),
            message: error.to_string(),
        }
    }
}

/// Replays the cache snapshot tail after the caller restores the cache-owned snapshot blob.
///
/// The restore hook runs before the tail iterator is consumed. When `anchor` is `Some`,
/// the hook should fetch and apply the cache-owned blob identified by
/// [`SnapshotAnchor::blob_ref`] and validate it against
/// [`SnapshotAnchor::content_hash`]. When `anchor` is `None`, restore starts from
/// event-store seq `1` and the hook may be a no-op.
///
/// This is a bootstrap path: it mutates cache state directly and never publishes replay
/// entries to the live message bus.
///
/// # Errors
///
/// Returns [`CacheReplayError::EventStore`] when the reader fails, `restore_snapshot`'s
/// error when the cache snapshot restore hook fails, [`CacheReplayError::Decode`] when
/// a supported payload cannot be decoded, and [`CacheReplayError::Apply`] when the
/// decoded payload cannot be applied to the cache.
pub fn restore_cache_snapshot_and_replay_tail<B, F>(
    cache: &mut Cache,
    reader: &EventStoreReader<B>,
    restore_snapshot: F,
) -> Result<CacheReplayReport, CacheReplayError>
where
    B: EventStore,
    F: FnOnce(&mut Cache, Option<&SnapshotAnchor>) -> Result<(), CacheReplayError>,
{
    let (plan, scan) = reader.scan_snapshot_replay_tail()?;
    restore_snapshot(cache, plan.anchor.as_ref())?;

    let mut applied_entries = 0;
    let mut ignored_entries = 0;

    for entry in scan {
        let entry = entry?;

        if entry.seq < plan.from_seq {
            return Err(CacheReplayError::UnexpectedSeq {
                seq: entry.seq,
                from_seq: plan.from_seq,
            });
        }

        if apply_cache_replay_entry(cache, &entry)? {
            applied_entries += 1;
        } else {
            ignored_entries += 1;
        }
    }

    Ok(CacheReplayReport {
        plan,
        applied_entries,
        ignored_entries,
    })
}

/// Replays the cache snapshot tail when the cache snapshot has already been restored.
///
/// This is a convenience wrapper for callers that load the cache-owned snapshot blob
/// before entering the event-store replay path.
///
/// # Errors
///
/// See [`restore_cache_snapshot_and_replay_tail`].
pub fn replay_cache_snapshot_tail<B>(
    cache: &mut Cache,
    reader: &EventStoreReader<B>,
) -> Result<CacheReplayReport, CacheReplayError>
where
    B: EventStore,
{
    restore_cache_snapshot_and_replay_tail(cache, reader, |_, _| Ok(()))
}

/// Plans event-store-only forensics replay inputs.
///
/// The plan scans the requested range in durable `seq` order and records only summary
/// metadata. Use [`load_forensics_replay_inputs`] to materialize the entries.
///
/// # Errors
///
/// Returns [`ReplayInputError::InvalidSeqRange`] when `range` is invalid and
/// [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn plan_forensics_replay_inputs<B>(
    reader: &EventStoreReader<B>,
    range: ReplaySeqRange,
) -> Result<ReplayInputPlan, ReplayInputError>
where
    B: EventStore,
{
    let span = collect_replay_entry_span(reader, range)?;
    Ok(ReplayInputPlan {
        scope: ReplayScope::Forensics,
        requested_range: range,
        event_range: span.event_range,
        event_count: span.event_count,
        event_time_range: span.time_range,
        catalog_slices: Vec::new(),
    })
}

/// Loads event-store-only forensics replay inputs.
///
/// Entries are returned in durable `seq` order. This function does not touch the data catalog,
/// live venues, strategy code, reconciliation, or clocks.
///
/// # Errors
///
/// Returns [`ReplayInputError::ScopeMismatch`] when `plan` is not a forensics plan and
/// [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn load_forensics_replay_inputs<B>(
    reader: &EventStoreReader<B>,
    plan: &ReplayInputPlan,
) -> Result<ReplayInputs, ReplayInputError>
where
    B: EventStore,
{
    ensure_plan_scope(plan, ReplayScope::Forensics)?;
    let entries = load_replay_entries(reader, plan.requested_range)?;
    Ok(ReplayInputs {
        scope: ReplayScope::Forensics,
        entries,
        catalog_slices: Vec::new(),
    })
}

/// Plans decision replay inputs by joining event-store entries with selected catalog slices.
///
/// The event-store range supplies durable replay order. Catalog slices are contextual input
/// selected by the caller; their timestamps bound data lookup but never replace `seq` ordering.
///
/// # Errors
///
/// Returns [`ReplayInputError::EmptyCatalogSelection`] when no catalog slices are selected,
/// [`ReplayInputError::InvalidSeqRange`] when `range` is invalid,
/// [`ReplayInputError::MissingCatalogTimeBounds`] when an unbounded selector cannot inherit
/// bounds from an empty event-store scan, [`ReplayInputError::InvalidCatalogTimeRange`] when a
/// resolved slice has `start > end`, [`ReplayInputError::Catalog`] when catalog planning fails,
/// and [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn plan_decision_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    range: ReplaySeqRange,
    catalog_slices: &[CatalogSliceSelector],
) -> Result<ReplayInputPlan, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    plan_catalog_joined_replay_inputs(
        reader,
        catalog,
        ReplayScope::Decision,
        range,
        catalog_slices,
    )
}

/// Loads decision replay inputs from an existing plan.
///
/// Event-store entries are returned in durable `seq` order. Catalog records are loaded through
/// the caller-provided catalog source only; this function does not query live venues or run engine
/// logic.
///
/// # Errors
///
/// Returns [`ReplayInputError::ScopeMismatch`] when `plan` is not a decision plan,
/// [`ReplayInputError::MissingCatalogSlice`] when a required slice is missing,
/// [`ReplayInputError::Catalog`] when catalog loading fails, and
/// [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn load_decision_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    plan: &ReplayInputPlan,
) -> Result<ReplayInputs, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    load_catalog_joined_replay_inputs(reader, catalog, plan, ReplayScope::Decision)
}

/// Plans full incident replay inputs by joining event-store entries with selected catalog slices.
///
/// Callers should select every catalog slice relevant to the incident window. The event-store
/// range remains the only ordering authority; catalog slices provide read-only context.
///
/// # Errors
///
/// Returns [`ReplayInputError::EmptyCatalogSelection`] when no catalog slices are selected,
/// [`ReplayInputError::InvalidSeqRange`] when `range` is invalid,
/// [`ReplayInputError::MissingCatalogTimeBounds`] when an unbounded selector cannot inherit
/// bounds from an empty event-store scan, [`ReplayInputError::InvalidCatalogTimeRange`] when a
/// resolved slice has `start > end`, [`ReplayInputError::Catalog`] when catalog planning fails,
/// and [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn plan_full_incident_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    range: ReplaySeqRange,
    catalog_slices: &[CatalogSliceSelector],
) -> Result<ReplayInputPlan, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    plan_catalog_joined_replay_inputs(
        reader,
        catalog,
        ReplayScope::FullIncident,
        range,
        catalog_slices,
    )
}

/// Loads full incident replay inputs from an existing plan.
///
/// Event-store entries are returned in durable `seq` order. Catalog records are loaded through
/// the caller-provided catalog source only; this function does not query live venues or run engine
/// logic.
///
/// # Errors
///
/// Returns [`ReplayInputError::ScopeMismatch`] when `plan` is not a full incident plan,
/// [`ReplayInputError::MissingCatalogSlice`] when a required slice is missing,
/// [`ReplayInputError::Catalog`] when catalog loading fails, and
/// [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn load_full_incident_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    plan: &ReplayInputPlan,
) -> Result<ReplayInputs, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    load_catalog_joined_replay_inputs(reader, catalog, plan, ReplayScope::FullIncident)
}

/// Restores cache state from a sealed run without publishing to the bus or touching live venues.
///
/// The loader opens `<base_dir>/<instance_id>/<run_id>.redb` through the sealed-run reader path,
/// rejects quarantined sources, restores the cache-owned snapshot blob when an anchor exists, and
/// applies the event-store tail in `seq` order. It does not open adapters, reconcile against a
/// venue, submit new entries, or query the data catalog.
///
/// # Errors
///
/// Returns [`CacheReplayError::EventStore`] when the run is missing, not sealed, quarantined, or
/// unreadable; see [`restore_cache_snapshot_and_replay_tail`] for snapshot, decode, and apply
/// failures.
pub fn restore_cache_from_sealed_run(
    cache: &mut Cache,
    base_dir: impl Into<PathBuf>,
    instance_id: &str,
    run_id: &str,
) -> Result<EventStoreReplayReport, CacheReplayError> {
    let (manifest, reader) = open_event_store_replay_source(base_dir, instance_id, run_id)?;
    let cache_report =
        restore_cache_snapshot_and_replay_tail(cache, &reader, restore_cache_snapshot_blob)?;

    Ok(EventStoreReplayReport {
        manifest,
        cache: cache_report,
    })
}

/// Opens a sealed run for replay without touching live venues.
///
/// # Errors
///
/// Returns [`CacheReplayError::EventStore`] when the run is missing, not sealed, quarantined, or
/// unreadable.
pub fn open_event_store_replay_source(
    base_dir: impl Into<PathBuf>,
    instance_id: &str,
    run_id: &str,
) -> Result<(RunManifest, EventStoreReader<RedbBackend>), CacheReplayError> {
    let backend = RedbBackend::open_sealed(base_dir, instance_id, run_id)?;
    let manifest = backend.manifest()?;
    reject_quarantined_replay_source(run_id, manifest.status)?;
    Ok((manifest, EventStoreReader::new(backend)))
}

/// Validates that a configured replay source exists, is sealed, and is not quarantined.
///
/// # Errors
///
/// Returns [`CacheReplayError::EventStore`] when the run is missing, not sealed, quarantined, or
/// unreadable.
pub fn validate_event_store_replay_source(
    base_dir: impl Into<PathBuf>,
    instance_id: &str,
    run_id: &str,
) -> Result<RunManifest, CacheReplayError> {
    let backend = RedbBackend::open_sealed(base_dir, instance_id, run_id)?;
    let manifest = backend.manifest()?;
    reject_quarantined_replay_source(run_id, manifest.status)?;
    Ok(manifest)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReplayEntrySpan {
    event_range: Option<ReplaySeqRange>,
    event_count: usize,
    time_range: Option<ReplayTimeRange>,
}

fn plan_catalog_joined_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    scope: ReplayScope,
    range: ReplaySeqRange,
    catalog_slices: &[CatalogSliceSelector],
) -> Result<ReplayInputPlan, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    if catalog_slices.is_empty() {
        return Err(ReplayInputError::EmptyCatalogSelection { scope });
    }

    let span = collect_replay_entry_span(reader, range)?;
    let catalog_slices = plan_catalog_slices(catalog, catalog_slices, span.time_range)?;

    Ok(ReplayInputPlan {
        scope,
        requested_range: range,
        event_range: span.event_range,
        event_count: span.event_count,
        event_time_range: span.time_range,
        catalog_slices,
    })
}

fn load_catalog_joined_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    plan: &ReplayInputPlan,
    expected_scope: ReplayScope,
) -> Result<ReplayInputs, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    ensure_plan_scope(plan, expected_scope)?;
    let entries = load_replay_entries(reader, plan.requested_range)?;
    let catalog_slices = load_catalog_slices(catalog, &plan.catalog_slices)?;

    Ok(ReplayInputs {
        scope: expected_scope,
        entries,
        catalog_slices,
    })
}

fn collect_replay_entry_span<B>(
    reader: &EventStoreReader<B>,
    range: ReplaySeqRange,
) -> Result<ReplayEntrySpan, ReplayInputError>
where
    B: EventStore,
{
    validate_seq_range(range)?;

    let mut first_seq = None;
    let mut last_seq = None;
    let mut event_count = 0;
    let mut time_range: Option<ReplayTimeRange> = None;

    for entry in reader.scan_range(range.from_seq, range.to_seq, ScanDirection::Forward) {
        let entry = entry?;
        first_seq.get_or_insert(entry.seq);
        last_seq = Some(entry.seq);
        event_count += 1;

        match time_range.as_mut() {
            Some(bounds) => bounds.include_entry(&entry),
            None => time_range = Some(ReplayTimeRange::from_entry(&entry)),
        }
    }

    let event_range = match (first_seq, last_seq) {
        (Some(from_seq), Some(to_seq)) => Some(ReplaySeqRange::new(from_seq, to_seq)),
        _ => None,
    };

    Ok(ReplayEntrySpan {
        event_range,
        event_count,
        time_range,
    })
}

fn load_replay_entries<B>(
    reader: &EventStoreReader<B>,
    range: ReplaySeqRange,
) -> Result<Vec<EventStoreEntry>, ReplayInputError>
where
    B: EventStore,
{
    validate_seq_range(range)?;

    reader
        .scan_range(range.from_seq, range.to_seq, ScanDirection::Forward)
        .collect::<Result<Vec<_>, _>>()
        .map_err(ReplayInputError::from)
}

fn plan_catalog_slices<C>(
    catalog: &mut C,
    selectors: &[CatalogSliceSelector],
    event_time_range: Option<ReplayTimeRange>,
) -> Result<Vec<CatalogSlicePlan>, ReplayInputError>
where
    C: ReplayCatalog,
{
    let mut plans = Vec::with_capacity(selectors.len());

    for selector in selectors {
        let query = resolve_catalog_slice_query(selector, event_time_range)?;
        let coverage = catalog
            .plan_slice(&query)
            .map_err(|e| ReplayInputError::Catalog {
                data_cls: query.data_cls.clone(),
                message: e.to_string(),
            })?;
        let status = if coverage.is_missing() {
            CatalogSliceStatus::Missing
        } else {
            CatalogSliceStatus::Available
        };
        plans.push(CatalogSlicePlan {
            query,
            coverage,
            status,
        });
    }

    Ok(plans)
}

fn load_catalog_slices<C>(
    catalog: &mut C,
    plans: &[CatalogSlicePlan],
) -> Result<Vec<CatalogReplaySlice>, ReplayInputError>
where
    C: ReplayCatalog,
{
    let mut slices = Vec::with_capacity(plans.len());

    for plan in plans {
        if plan.is_missing() {
            if plan.query.required {
                return Err(ReplayInputError::MissingCatalogSlice {
                    data_cls: plan.query.data_cls.clone(),
                    identifiers: plan.query.identifiers.clone(),
                });
            }

            slices.push(CatalogReplaySlice {
                plan: plan.clone(),
                records: Vec::new(),
            });
            continue;
        }

        let records = catalog
            .load_slice(plan)
            .map_err(|e| ReplayInputError::Catalog {
                data_cls: plan.query.data_cls.clone(),
                message: e.to_string(),
            })?;
        slices.push(CatalogReplaySlice {
            plan: plan.clone(),
            records,
        });
    }

    Ok(slices)
}

fn resolve_catalog_slice_query(
    selector: &CatalogSliceSelector,
    event_time_range: Option<ReplayTimeRange>,
) -> Result<CatalogSliceQuery, ReplayInputError> {
    let Some(start) = selector
        .start
        .or(event_time_range.map(|bounds| bounds.start))
    else {
        return Err(ReplayInputError::MissingCatalogTimeBounds {
            data_cls: selector.data_cls.clone(),
        });
    };
    let Some(end) = selector.end.or(event_time_range.map(|bounds| bounds.end)) else {
        return Err(ReplayInputError::MissingCatalogTimeBounds {
            data_cls: selector.data_cls.clone(),
        });
    };

    if start > end {
        return Err(ReplayInputError::InvalidCatalogTimeRange {
            data_cls: selector.data_cls.clone(),
            start: start.as_u64(),
            end: end.as_u64(),
        });
    }

    Ok(CatalogSliceQuery {
        data_cls: selector.data_cls.clone(),
        identifiers: selector.identifiers.clone(),
        start,
        end,
        required: selector.required,
    })
}

fn ensure_plan_scope(
    plan: &ReplayInputPlan,
    expected: ReplayScope,
) -> Result<(), ReplayInputError> {
    if plan.scope != expected {
        return Err(ReplayInputError::ScopeMismatch {
            expected,
            actual: plan.scope,
        });
    }

    Ok(())
}

fn validate_seq_range(range: ReplaySeqRange) -> Result<(), ReplayInputError> {
    if range.from_seq == 0 {
        return Err(ReplayInputError::InvalidSeqRange {
            from_seq: range.from_seq,
            to_seq: range.to_seq,
            message: "seq is 1-based".to_string(),
        });
    }

    if range.from_seq > range.to_seq {
        return Err(ReplayInputError::InvalidSeqRange {
            from_seq: range.from_seq,
            to_seq: range.to_seq,
            message: "from_seq exceeds to_seq".to_string(),
        });
    }

    Ok(())
}

/// Restores the cache-owned snapshot blob identified by `anchor`.
///
/// # Errors
///
/// Returns [`CacheReplayError::SnapshotRestore`] when the blob is missing, fails to load, fails its
/// content hash check, or fails to restore into the cache.
pub fn restore_cache_snapshot_blob(
    cache: &mut Cache,
    anchor: Option<&SnapshotAnchor>,
) -> Result<(), CacheReplayError> {
    let Some(anchor) = anchor else {
        return Ok(());
    };

    let blob = cache
        .load_snapshot_blob(&anchor.blob_ref)
        .map_err(|e| CacheReplayError::snapshot_restore(anchor, e))?
        .ok_or_else(|| CacheReplayError::snapshot_restore(anchor, "snapshot blob not found"))?;
    let actual_hash = compute_snapshot_content_hash(blob.as_ref());

    if actual_hash != anchor.content_hash {
        return Err(CacheReplayError::snapshot_restore(
            anchor,
            format!(
                "content_hash mismatch: expected {}, actual {actual_hash}",
                anchor.content_hash
            ),
        ));
    }

    cache
        .restore_snapshot_blob(&anchor.blob_ref, blob)
        .map_err(|e| CacheReplayError::snapshot_restore(anchor, e))
}

/// Applies one event-store entry to cache state when a replay rule exists.
///
/// Returns `Ok(true)` when the entry changed cache state and `Ok(false)` when the
/// payload is outside the current cache bootstrap replay surface.
///
/// # Errors
///
/// Returns [`CacheReplayError::Decode`] when a supported payload cannot be decoded and
/// [`CacheReplayError::Apply`] when the decoded payload cannot be applied to the cache.
pub fn apply_cache_replay_entry(
    cache: &mut Cache,
    entry: &EventStoreEntry,
) -> Result<bool, CacheReplayError> {
    match entry.payload_type.as_str() {
        PAYLOAD_TYPE_ACCOUNT_STATE => {
            let state = decode_payload::<AccountState>(entry)?;
            apply_result(entry, cache.update_account_state(&state))?;
        }
        PAYLOAD_TYPE_ORDER_INITIALIZED => {
            let event = decode_order_event::<OrderInitialized>(entry, OrderEventAny::Initialized)?;
            let order = OrderAny::from_events(vec![event]).map_err(|e| apply_error(entry, e))?;
            apply_result(entry, cache.add_order(order, None, None, false))?;
        }
        PAYLOAD_TYPE_ORDER_DENIED => {
            apply_order_event(cache, entry, OrderEventAny::Denied)?;
        }
        PAYLOAD_TYPE_ORDER_EMULATED => {
            apply_order_event(cache, entry, OrderEventAny::Emulated)?;
        }
        PAYLOAD_TYPE_ORDER_RELEASED => {
            apply_order_event(cache, entry, OrderEventAny::Released)?;
        }
        PAYLOAD_TYPE_ORDER_SUBMITTED => {
            apply_order_event(cache, entry, OrderEventAny::Submitted)?;
        }
        PAYLOAD_TYPE_ORDER_ACCEPTED => {
            apply_order_event(cache, entry, OrderEventAny::Accepted)?;
        }
        PAYLOAD_TYPE_ORDER_REJECTED => {
            apply_order_event(cache, entry, OrderEventAny::Rejected)?;
        }
        PAYLOAD_TYPE_ORDER_CANCELED => {
            apply_order_event(cache, entry, OrderEventAny::Canceled)?;
        }
        PAYLOAD_TYPE_ORDER_EXPIRED => {
            apply_order_event(cache, entry, OrderEventAny::Expired)?;
        }
        PAYLOAD_TYPE_ORDER_TRIGGERED => {
            apply_order_event(cache, entry, OrderEventAny::Triggered)?;
        }
        PAYLOAD_TYPE_ORDER_PENDING_UPDATE => {
            apply_order_event(cache, entry, OrderEventAny::PendingUpdate)?;
        }
        PAYLOAD_TYPE_ORDER_PENDING_CANCEL => {
            apply_order_event(cache, entry, OrderEventAny::PendingCancel)?;
        }
        PAYLOAD_TYPE_ORDER_MODIFY_REJECTED => {
            apply_order_event(cache, entry, OrderEventAny::ModifyRejected)?;
        }
        PAYLOAD_TYPE_ORDER_CANCEL_REJECTED => {
            apply_order_event(cache, entry, OrderEventAny::CancelRejected)?;
        }
        PAYLOAD_TYPE_ORDER_UPDATED => {
            apply_order_event(cache, entry, OrderEventAny::Updated)?;
        }
        PAYLOAD_TYPE_ORDER_FILLED => {
            let fill = decode_payload::<OrderFilled>(entry)?;
            let event = OrderEventAny::Filled(fill);
            apply_result(entry, cache.update_order(&event))?;
            apply_fill_to_position(cache, entry, &fill)?;
        }
        PAYLOAD_TYPE_POSITION_ADJUSTED => {
            let adjustment = decode_payload::<PositionAdjusted>(entry)?;
            apply_position_adjustment(cache, entry, adjustment)?;
        }
        _ => return Ok(false),
    }

    Ok(true)
}

fn apply_order_event<T>(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    wrap: impl FnOnce(T) -> OrderEventAny,
) -> Result<(), CacheReplayError>
where
    T: DeserializeOwned,
{
    let event = decode_order_event(entry, wrap)?;
    apply_result(entry, cache.update_order(&event))?;
    Ok(())
}

fn decode_order_event<T>(
    entry: &EventStoreEntry,
    wrap: impl FnOnce(T) -> OrderEventAny,
) -> Result<OrderEventAny, CacheReplayError>
where
    T: DeserializeOwned,
{
    Ok(wrap(decode_payload(entry)?))
}

fn apply_fill_to_position(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    fill: &OrderFilled,
) -> Result<(), CacheReplayError> {
    let Some(position_id) = fill.position_id else {
        return Ok(());
    };

    if let Some(mut position) = cache.position_owned(&position_id) {
        if position.trade_ids().contains(&fill.trade_id) {
            return Ok(());
        }

        position.apply(fill);
        apply_result(entry, cache.update_position(&position))?;
        return Ok(());
    }

    let Some(instrument) = cache.instrument(&fill.instrument_id).cloned() else {
        return Ok(());
    };

    let position = Position::new(&instrument, *fill);
    apply_result(entry, cache.add_position(&position, OmsType::Unspecified))?;
    Ok(())
}

fn apply_position_adjustment(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    adjustment: PositionAdjusted,
) -> Result<(), CacheReplayError> {
    let Some(mut position) = cache.position_owned(&adjustment.position_id) else {
        return Ok(());
    };

    position.apply_adjustment(adjustment);
    apply_result(entry, cache.update_position(&position))?;
    Ok(())
}

fn decode_payload<T>(entry: &EventStoreEntry) -> Result<T, CacheReplayError>
where
    T: DeserializeOwned,
{
    rmp_serde::from_slice(&entry.payload).map_err(|e| CacheReplayError::Decode {
        seq: entry.seq,
        payload_type: entry.payload_type.to_string(),
        message: e.to_string(),
    })
}

fn apply_result<T, E>(entry: &EventStoreEntry, result: Result<T, E>) -> Result<T, CacheReplayError>
where
    E: Display,
{
    result.map_err(|e| apply_error(entry, e))
}

fn apply_error(entry: &EventStoreEntry, error: impl Display) -> CacheReplayError {
    CacheReplayError::Apply {
        seq: entry.seq,
        payload_type: entry.payload_type.to_string(),
        message: error.to_string(),
    }
}

fn reject_quarantined_replay_source(
    run_id: &str,
    status: RunStatus,
) -> Result<(), CacheReplayError> {
    if matches!(status, RunStatus::Quarantined) {
        let error = EventStoreError::Backend(format!("replay source {run_id} is quarantined"));
        return Err(CacheReplayError::from(error));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{any::Any, cell::Cell, rc::Rc};

    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_common::msgbus::{self, BusTap, Endpoint, MStr, Topic as BusTopic};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        accounts::AccountAny,
        enums::{OrderStatus, PositionAdjustmentType},
        events::{
            PositionEvent,
            account::stubs::{cash_account_state, cash_account_state_million_usd},
            order::spec::{
                OrderAcceptedSpec, OrderFilledSpec, OrderInitializedSpec, OrderSubmittedSpec,
            },
        },
        identifiers::{AccountId, PositionId},
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::Order,
        types::{Currency, Money},
    };
    use rstest::rstest;
    use tempfile::TempDir;
    use ustr::Ustr;

    use super::*;
    use crate::{
        backend::{AppendEntry, MemoryBackend, RedbBackend},
        capture::{
            builtins::{encode_order_event_any, encode_position_event},
            encode_account_state,
        },
        entry::Topic as EntryTopic,
        hash::compute_entry_hash,
        headers::Headers,
        manifest::{RegisteredComponents, RunManifest, RunStatus},
        snapshot::SnapshotAnchor,
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

    fn append_payload(seq: u64, payload_type: &str, payload: Bytes) -> AppendEntry {
        append_payload_with_ts(seq, seq, payload_type, payload)
    }

    fn append_payload_with_ts(
        seq: u64,
        ts_init: u64,
        payload_type: &str,
        payload: Bytes,
    ) -> AppendEntry {
        let topic = EntryTopic::from("events.account.SIM");
        let ts = UnixNanos::from(ts_init);
        let headers = Headers::empty();
        let hash = compute_entry_hash(
            seq,
            ts,
            ts,
            topic.as_ref(),
            payload_type,
            &payload,
            &headers,
        );
        let entry = EventStoreEntry::new(
            hash,
            seq,
            headers,
            topic,
            Ustr::from(payload_type),
            payload,
            ts,
            ts,
        );
        AppendEntry::without_indices(entry)
    }

    fn append_account_state(seq: u64, state: &AccountState) -> AppendEntry {
        let encoded = encode_account_state(state).expect("encode account state");
        append_payload(seq, PAYLOAD_TYPE_ACCOUNT_STATE, encoded.payload)
    }

    fn append_order_event(seq: u64, event: &OrderEventAny) -> AppendEntry {
        let encoded = encode_order_event_any(event).expect("encode order event");
        let payload_type = encoded.payload_type.expect("order payload type");
        append_payload(seq, payload_type.as_str(), encoded.payload)
    }

    fn append_position_event(seq: u64, event: &PositionEvent) -> AppendEntry {
        let encoded = encode_position_event(event).expect("encode position event");
        let payload_type = encoded.payload_type.expect("position payload type");
        append_payload(seq, payload_type.as_str(), encoded.payload)
    }

    fn reader_with_entries(
        run_id: &str,
        entries: &[AppendEntry],
    ) -> EventStoreReader<MemoryBackend> {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest(run_id)).expect("open");
        backend.append_batch(entries).expect("append");
        EventStoreReader::new(backend)
    }

    fn reader_with_anchor(anchor_seq: u64) -> (EventStoreReader<MemoryBackend>, AccountState) {
        let anchored = cash_account_state();
        let replayed = cash_account_state_million_usd("200 USD", "0 USD", "200 USD");
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-replay")).expect("open");
        backend
            .append_batch(&[
                append_account_state(1, &anchored),
                append_account_state(2, &replayed),
            ])
            .expect("append");
        backend
            .record_snapshot_anchor(SnapshotAnchor::new(anchor_seq, "cache://account", "hash"))
            .expect("record anchor");
        (EventStoreReader::new(backend), replayed)
    }

    #[derive(Debug)]
    struct CountingTap {
        calls: Rc<Cell<usize>>,
    }

    impl CountingTap {
        fn new(calls: Rc<Cell<usize>>) -> Self {
            Self { calls }
        }

        fn increment(&self) {
            self.calls.set(self.calls.get() + 1);
        }
    }

    impl BusTap for CountingTap {
        fn on_publish(&self, _topic: MStr<BusTopic>, _message: &dyn Any) {
            self.increment();
        }

        fn on_send(&self, _endpoint: MStr<Endpoint>, _message: &dyn Any) {
            self.increment();
        }
    }

    #[derive(Debug)]
    struct FakeReplayCatalog {
        coverage: CatalogSliceCoverage,
        records: Vec<CatalogReplayRecord>,
        plan_queries: Vec<CatalogSliceQuery>,
        load_plans: Vec<CatalogSlicePlan>,
    }

    impl FakeReplayCatalog {
        fn new(coverage: CatalogSliceCoverage, records: Vec<CatalogReplayRecord>) -> Self {
            Self {
                coverage,
                records,
                plan_queries: Vec::new(),
                load_plans: Vec::new(),
            }
        }
    }

    impl ReplayCatalog for FakeReplayCatalog {
        type Error = String;

        fn plan_slice(
            &mut self,
            query: &CatalogSliceQuery,
        ) -> Result<CatalogSliceCoverage, Self::Error> {
            self.plan_queries.push(query.clone());
            Ok(self.coverage.clone())
        }

        fn load_slice(
            &mut self,
            plan: &CatalogSlicePlan,
        ) -> Result<Vec<CatalogReplayRecord>, Self::Error> {
            self.load_plans.push(plan.clone());
            Ok(self.records.clone())
        }
    }

    struct BusTapGuard;

    impl Drop for BusTapGuard {
        fn drop(&mut self) {
            msgbus::clear_bus_tap();
        }
    }

    #[rstest]
    fn decision_replay_inputs_join_event_entries_with_selected_catalog_slice() {
        let reader = reader_with_entries(
            "run-decision",
            &[
                append_payload_with_ts(1, 120, "RunStarted", Bytes::from_static(b"started")),
                append_payload_with_ts(2, 100, "SubmitOrder", Bytes::from_static(b"submit")),
            ],
        );
        let record = CatalogReplayRecord::new(
            "quotes",
            Some("AUD/USD.SIM".to_string()),
            UnixNanos::from(110),
            Bytes::from_static(b"quote"),
        );
        let mut catalog = FakeReplayCatalog::new(
            CatalogSliceCoverage::from_files(vec!["quotes/AUDUSD.SIM/100_120.parquet".into()]),
            vec![record.clone()],
        );

        let plan = plan_decision_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 2),
            &[CatalogSliceSelector::new("quotes").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan decision replay");

        assert_eq!(plan.scope, ReplayScope::Decision);
        assert_eq!(plan.event_range, Some(ReplaySeqRange::new(1, 2)));
        assert_eq!(plan.event_count, 2);
        assert_eq!(
            plan.event_time_range,
            Some(ReplayTimeRange::new(
                UnixNanos::from(100),
                UnixNanos::from(120),
            )),
        );
        assert_eq!(plan.catalog_slices[0].status, CatalogSliceStatus::Available);
        assert_eq!(catalog.plan_queries.len(), 1);
        assert_eq!(catalog.plan_queries[0].data_cls, "quotes");
        assert_eq!(
            catalog.plan_queries[0].identifiers,
            vec!["AUD/USD.SIM".to_string()],
        );
        assert_eq!(catalog.plan_queries[0].start, UnixNanos::from(100));
        assert_eq!(catalog.plan_queries[0].end, UnixNanos::from(120));

        let loaded =
            load_decision_replay_inputs(&reader, &mut catalog, &plan).expect("load decision");
        let seqs: Vec<_> = loaded.entries.iter().map(|entry| entry.seq).collect();

        assert_eq!(loaded.scope, ReplayScope::Decision);
        assert_eq!(seqs, vec![1, 2]);
        assert_eq!(loaded.catalog_slices.len(), 1);
        assert_eq!(loaded.catalog_slices[0].records, vec![record]);
        assert_eq!(catalog.load_plans.len(), 1);
    }

    #[rstest]
    fn full_incident_plan_marks_missing_catalog_slice() {
        let reader = reader_with_entries(
            "run-incident",
            &[append_payload_with_ts(
                1,
                1_000,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());

        let plan = plan_full_incident_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("trades").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan full incident");
        let missing = plan.missing_catalog_slices();

        assert_eq!(plan.scope, ReplayScope::FullIncident);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].query.data_cls, "trades");
        assert_eq!(
            missing[0].query.identifiers,
            vec!["AUD/USD.SIM".to_string()],
        );
        assert_eq!(missing[0].query.start, UnixNanos::from(1_000));
        assert_eq!(missing[0].query.end, UnixNanos::from(1_000));
    }

    #[rstest]
    fn required_missing_catalog_slice_rejects_load() {
        let reader = reader_with_entries(
            "run-required-missing",
            &[append_payload_with_ts(
                1,
                1_000,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());
        let plan = plan_decision_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("quotes")
                .with_identifier("AUD/USD.SIM")
                .require_coverage()],
        )
        .expect("plan missing slice");

        let err = load_decision_replay_inputs(&reader, &mut catalog, &plan)
            .expect_err("required missing slice must fail");

        match err {
            ReplayInputError::MissingCatalogSlice {
                data_cls,
                identifiers,
            } => {
                assert_eq!(data_cls, "quotes");
                assert_eq!(identifiers, vec!["AUD/USD.SIM".to_string()]);
            }
            other => panic!("expected MissingCatalogSlice, was {other:?}"),
        }
    }

    #[rstest]
    fn optional_missing_catalog_slice_loads_as_empty_without_catalog_load() {
        let reader = reader_with_entries(
            "run-optional-missing",
            &[append_payload_with_ts(
                1,
                1_000,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());
        let plan = plan_decision_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("quotes").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan optional missing slice");

        let loaded =
            load_decision_replay_inputs(&reader, &mut catalog, &plan).expect("load optional");

        assert_eq!(loaded.catalog_slices.len(), 1);
        assert!(loaded.catalog_slices[0].plan.is_missing());
        assert!(loaded.catalog_slices[0].records.is_empty());
        assert!(catalog.load_plans.is_empty());
    }

    #[rstest]
    #[case::decision(ReplayScope::Decision)]
    #[case::full_incident(ReplayScope::FullIncident)]
    fn catalog_joined_planners_reject_empty_catalog_selection(#[case] scope: ReplayScope) {
        let reader = reader_with_entries(
            "run-empty-selection",
            &[append_payload_with_ts(
                1,
                1_000,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());

        let err = match scope {
            ReplayScope::Decision => {
                plan_decision_replay_inputs(&reader, &mut catalog, ReplaySeqRange::new(1, 1), &[])
            }
            ReplayScope::FullIncident => plan_full_incident_replay_inputs(
                &reader,
                &mut catalog,
                ReplaySeqRange::new(1, 1),
                &[],
            ),
            ReplayScope::Forensics => unreachable!("forensics has no catalog selection"),
        }
        .expect_err("empty catalog selection must fail");

        match err {
            ReplayInputError::EmptyCatalogSelection { scope: actual } => {
                assert_eq!(actual, scope);
            }
            other => panic!("expected EmptyCatalogSelection, was {other:?}"),
        }
        assert!(catalog.plan_queries.is_empty());
    }

    #[rstest]
    fn catalog_selector_explicit_time_bounds_override_event_span() {
        let reader = reader_with_entries(
            "run-explicit-bounds",
            &[append_payload_with_ts(
                1,
                1_000,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let mut catalog = FakeReplayCatalog::new(
            CatalogSliceCoverage::from_files(vec!["bars/AUDUSD.SIM/900_950.parquet".into()]),
            Vec::new(),
        );

        let plan = plan_decision_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("bars")
                .with_identifier("AUD/USD.SIM-1-MINUTE-BID-EXTERNAL")
                .with_time_bounds(UnixNanos::from(900), UnixNanos::from(950))],
        )
        .expect("plan explicit bounds");

        assert_eq!(plan.catalog_slices[0].query.start, UnixNanos::from(900));
        assert_eq!(plan.catalog_slices[0].query.end, UnixNanos::from(950));
        assert_eq!(catalog.plan_queries[0].start, UnixNanos::from(900));
        assert_eq!(catalog.plan_queries[0].end, UnixNanos::from(950));
    }

    #[rstest]
    fn full_incident_replay_inputs_load_catalog_records() {
        let reader = reader_with_entries(
            "run-full-load",
            &[
                append_payload_with_ts(1, 100, "RunStarted", Bytes::from_static(b"started")),
                append_payload_with_ts(2, 110, "OrderFilled", Bytes::from_static(b"filled")),
            ],
        );
        let record = CatalogReplayRecord::new(
            "trades",
            Some("AUD/USD.SIM".to_string()),
            UnixNanos::from(105),
            Bytes::from_static(b"trade"),
        );
        let mut catalog = FakeReplayCatalog::new(
            CatalogSliceCoverage::from_files(vec!["trades/AUDUSD.SIM/100_110.parquet".into()]),
            vec![record.clone()],
        );
        let plan = plan_full_incident_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 2),
            &[CatalogSliceSelector::new("trades").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan full incident");

        assert_eq!(
            plan.catalog_slices[0].query.identifiers_option(),
            Some(vec!["AUD/USD.SIM".to_string()]),
        );

        let loaded =
            load_full_incident_replay_inputs(&reader, &mut catalog, &plan).expect("load full");
        let seqs: Vec<_> = loaded.entries.iter().map(|entry| entry.seq).collect();

        assert_eq!(loaded.scope, ReplayScope::FullIncident);
        assert_eq!(seqs, vec![1, 2]);
        assert_eq!(loaded.catalog_slices[0].records, vec![record]);
        assert_eq!(catalog.load_plans.len(), 1);
    }

    #[rstest]
    fn unbounded_catalog_selector_rejects_empty_event_scan() {
        let reader = reader_with_entries("run-empty", &[]);
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());

        let err = plan_decision_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 10),
            &[CatalogSliceSelector::new("quotes")],
        )
        .expect_err("empty replay scan must need explicit bounds");

        match err {
            ReplayInputError::MissingCatalogTimeBounds { data_cls } => {
                assert_eq!(data_cls, "quotes");
            }
            other => panic!("expected MissingCatalogTimeBounds, was {other:?}"),
        }
    }

    #[rstest]
    fn invalid_catalog_time_bounds_are_rejected_before_catalog_access() {
        let reader = reader_with_entries(
            "run-invalid-bounds",
            &[append_payload_with_ts(
                1,
                1_000,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());

        let err = plan_decision_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("quotes")
                .with_time_bounds(UnixNanos::from(200), UnixNanos::from(100))],
        )
        .expect_err("invalid catalog bounds must fail");

        match err {
            ReplayInputError::InvalidCatalogTimeRange {
                data_cls,
                start,
                end,
            } => {
                assert_eq!(data_cls, "quotes");
                assert_eq!(start, 200);
                assert_eq!(end, 100);
            }
            other => panic!("expected InvalidCatalogTimeRange, was {other:?}"),
        }
        assert!(catalog.plan_queries.is_empty());
    }

    #[rstest]
    fn forensics_replay_inputs_do_not_require_catalog_source() {
        let reader = reader_with_entries(
            "run-forensics",
            &[append_payload_with_ts(
                1,
                500,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );

        let plan = plan_forensics_replay_inputs(&reader, ReplaySeqRange::new(1, 1))
            .expect("plan forensics");
        let loaded = load_forensics_replay_inputs(&reader, &plan).expect("load forensics");

        assert_eq!(plan.scope, ReplayScope::Forensics);
        assert!(plan.catalog_slices.is_empty());
        assert_eq!(loaded.entries.len(), 1);
        assert!(loaded.catalog_slices.is_empty());
    }

    #[rstest]
    fn scope_specific_loader_rejects_mismatched_plan() {
        let reader = reader_with_entries(
            "run-scope-mismatch",
            &[append_payload_with_ts(
                1,
                500,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let plan = plan_forensics_replay_inputs(&reader, ReplaySeqRange::new(1, 1))
            .expect("plan forensics");
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());

        let err = load_decision_replay_inputs(&reader, &mut catalog, &plan)
            .expect_err("decision loader must reject forensics plan");

        match err {
            ReplayInputError::ScopeMismatch { expected, actual } => {
                assert_eq!(expected, ReplayScope::Decision);
                assert_eq!(actual, ReplayScope::Forensics);
            }
            other => panic!("expected ScopeMismatch, was {other:?}"),
        }
        assert!(catalog.load_plans.is_empty());
    }

    #[rstest]
    #[case::zero_start(ReplaySeqRange::new(0, 1), "seq is 1-based")]
    #[case::from_after_to(ReplaySeqRange::new(2, 1), "from_seq exceeds to_seq")]
    fn invalid_replay_seq_range_rejected(
        #[case] range: ReplaySeqRange,
        #[case] expected_message: &str,
    ) {
        let reader = reader_with_entries("run-invalid-seq", &[]);

        let err =
            plan_forensics_replay_inputs(&reader, range).expect_err("invalid seq range must fail");

        match err {
            ReplayInputError::InvalidSeqRange {
                from_seq,
                to_seq,
                message,
            } => {
                assert_eq!(from_seq, range.from_seq);
                assert_eq!(to_seq, range.to_seq);
                assert_eq!(message, expected_message);
            }
            other => panic!("expected InvalidSeqRange, was {other:?}"),
        }
    }

    #[rstest]
    fn replay_restores_snapshot_before_applying_tail() {
        let (reader, replayed) = reader_with_anchor(1);
        let mut cache = Cache::default();
        let restored = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
        let restored_id = restored.account_id;

        let report =
            restore_cache_snapshot_and_replay_tail(&mut cache, &reader, |cache, anchor| {
                assert_eq!(anchor.expect("anchor").high_watermark, 1);
                let account = AccountAny::from_events(std::slice::from_ref(&restored))
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))?;
                cache
                    .add_account(account)
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))
            })
            .expect("replay");

        let account = cache.account_owned(&restored_id).expect("account restored");
        let events = account.events();

        assert_eq!(report.plan.from_seq, 2);
        assert_eq!(report.applied_entries, 1);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(events, vec![restored, replayed]);
    }

    #[rstest]
    fn replay_does_not_apply_entries_at_or_below_anchor_watermark() {
        let (reader, _) = reader_with_anchor(2);
        let mut cache = Cache::default();
        let restored = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
        let restored_id = restored.account_id;

        let report =
            restore_cache_snapshot_and_replay_tail(&mut cache, &reader, |cache, anchor| {
                assert_eq!(anchor.expect("anchor").high_watermark, 2);
                let account = AccountAny::from_events(std::slice::from_ref(&restored))
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))?;
                cache
                    .add_account(account)
                    .map_err(|e| CacheReplayError::snapshot_restore(anchor.unwrap(), e))
            })
            .expect("replay");

        let account = cache.account_owned(&restored_id).expect("account restored");

        assert!(report.plan.is_empty());
        assert_eq!(report.applied_entries, 0);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(account.events(), vec![restored]);
    }

    #[rstest]
    fn replay_from_start_applies_account_state_without_bus_publish() {
        let state = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
        let account_id = AccountId::from("SIM-001");
        let bus_calls = Rc::new(Cell::new(0));
        msgbus::set_bus_tap(Rc::new(CountingTap::new(Rc::clone(&bus_calls))));
        let _guard = BusTapGuard;
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-replay")).expect("open");
        backend
            .append_batch(&[append_account_state(1, &state)])
            .expect("append");
        let reader = EventStoreReader::new(backend);
        let mut cache = Cache::default();

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let account = cache.account_owned(&account_id).expect("account replayed");

        assert_eq!(report.plan.anchor, None);
        assert_eq!(report.plan.from_seq, 1);
        assert_eq!(report.applied_entries, 1);
        assert_eq!(bus_calls.get(), 0);
        assert_eq!(account.last_event(), Some(state));
        assert_eq!(account.base_currency(), Some(Currency::USD()));
    }

    #[rstest]
    fn unsupported_payload_is_ignored() {
        let mut backend = MemoryBackend::new();
        backend.open_run(manifest("run-replay")).expect("open");
        backend
            .append_batch(&[append_payload(
                1,
                "RunStarted",
                Bytes::copy_from_slice(UUID4::new().to_string().as_bytes()),
            )])
            .expect("append");
        let reader = EventStoreReader::new(backend);
        let mut cache = Cache::default();

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");

        assert_eq!(report.applied_entries, 0);
        assert_eq!(report.ignored_entries, 1);
    }

    #[rstest]
    fn order_fill_replay_updates_order_and_creates_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let initialized = OrderInitializedSpec::builder()
            .instrument_id(instrument.id())
            .build();
        let client_order_id = initialized.client_order_id;
        let submitted = OrderSubmittedSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .build();
        let accepted = OrderAcceptedSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .account_id(submitted.account_id)
            .build();
        let filled = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(accepted.venue_order_id)
            .account_id(submitted.account_id)
            .position_id(position_id)
            .commission(Money::from("1 USD"))
            .build();
        let filled_event = OrderEventAny::Filled(filled);
        let reader = reader_with_entries(
            "run-order-replay",
            &[
                append_order_event(1, &OrderEventAny::Initialized(initialized)),
                append_order_event(2, &OrderEventAny::Submitted(submitted)),
                append_order_event(3, &OrderEventAny::Accepted(accepted)),
                append_order_event(4, &filled_event),
            ],
        );
        let mut cache = Cache::default();
        cache.add_instrument(instrument).expect("add instrument");

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let order = cache.order_owned(&client_order_id).expect("order replayed");
        let position = cache
            .position_owned(&position_id)
            .expect("position replayed");

        assert_eq!(report.applied_entries, 4);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.event_count(), 4);
        assert_eq!(order.last_event(), &filled_event);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.last_event(), Some(filled));
        assert_eq!(position.trade_ids(), vec![filled.trade_id]);
        assert_eq!(position.commissions(), vec![Money::from("1 USD")]);
    }

    #[rstest]
    fn position_adjustment_replay_updates_existing_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .build();
        let position = Position::new(&instrument, fill);
        let adjustment = PositionAdjusted::new(
            fill.trader_id,
            fill.strategy_id,
            fill.instrument_id,
            position_id,
            fill.account_id,
            PositionAdjustmentType::Funding,
            None,
            Some(Money::from("2 USD")),
            Some(Ustr::from("funding")),
            UUID4::new(),
            UnixNanos::from(10),
            UnixNanos::from(11),
        );
        let entry = append_position_event(1, &PositionEvent::PositionAdjusted(adjustment)).entry;
        let mut cache = Cache::default();
        cache
            .add_position(&position, OmsType::Unspecified)
            .expect("seed position");

        let applied = apply_cache_replay_entry(&mut cache, &entry).expect("apply");
        let position = cache
            .position_owned(&position_id)
            .expect("position updated");

        assert!(applied);
        assert_eq!(position.adjustments, vec![adjustment]);
        assert_eq!(position.realized_pnl, Some(Money::from("2 USD")));
        assert_eq!(position.ts_last, adjustment.ts_event);
    }

    #[rstest]
    fn duplicate_position_fill_is_not_applied_twice() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .commission(Money::from("1 USD"))
            .build();
        let position = Position::new(&instrument, fill);
        let entry = append_order_event(1, &OrderEventAny::Filled(fill)).entry;
        let mut cache = Cache::default();
        cache
            .add_position(&position, OmsType::Unspecified)
            .expect("seed position");

        apply_fill_to_position(&mut cache, &entry, &fill).expect("apply fill");
        let position = cache
            .position_owned(&position_id)
            .expect("position updated");

        assert_eq!(position.event_count(), 1);
        assert_eq!(position.trade_ids(), vec![fill.trade_id]);
        assert_eq!(position.commissions(), vec![Money::from("1 USD")]);
    }

    #[rstest]
    fn corrupt_supported_payload_returns_decode_error() {
        let reader = reader_with_entries(
            "run-decode-error",
            &[append_payload(
                1,
                PAYLOAD_TYPE_ACCOUNT_STATE,
                Bytes::copy_from_slice(&[0xc1]),
            )],
        );
        let mut cache = Cache::default();

        let err = replay_cache_snapshot_tail(&mut cache, &reader).expect_err("decode error");

        match err {
            CacheReplayError::Decode {
                seq, payload_type, ..
            } => {
                assert_eq!(seq, 1);
                assert_eq!(payload_type, PAYLOAD_TYPE_ACCOUNT_STATE);
            }
            other => panic!("expected Decode, was {other:?}"),
        }
    }

    #[rstest]
    fn missing_order_event_returns_apply_error() {
        let submitted = OrderSubmittedSpec::builder().build();
        let reader = reader_with_entries(
            "run-apply-error",
            &[append_order_event(1, &OrderEventAny::Submitted(submitted))],
        );
        let mut cache = Cache::default();

        let err = replay_cache_snapshot_tail(&mut cache, &reader).expect_err("apply error");

        match err {
            CacheReplayError::Apply {
                seq,
                payload_type,
                message,
            } => {
                assert_eq!(seq, 1);
                assert_eq!(payload_type, PAYLOAD_TYPE_ORDER_SUBMITTED);
                assert!(
                    message.contains("not found"),
                    "message should include cache apply failure: {message}",
                );
            }
            other => panic!("expected Apply, was {other:?}"),
        }
    }

    #[rstest]
    fn restore_cache_from_sealed_run_restores_snapshot_and_tail() {
        let tmp = TempDir::new().expect("tempdir");
        let run_id = "sealed-replay";
        let instance_id = "trader-001";
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-SEALED-REPLAY-1"))
            .build();
        let position = Position::new(&instrument, fill);
        let mut snapshot_cache = Cache::default();
        let snapshot_ref = snapshot_cache
            .snapshot_position(&position)
            .expect("snapshot position");
        let anchored_state = cash_account_state_million_usd("100 USD", "0 USD", "100 USD");
        let replayed_state = cash_account_state_million_usd("200 USD", "0 USD", "200 USD");

        {
            let mut backend = RedbBackend::new(tmp.path().to_path_buf());
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_batch(&[append_account_state(1, &anchored_state)])
                .expect("append anchored state");
            backend
                .record_snapshot_anchor(SnapshotAnchor::new(
                    1,
                    snapshot_ref.blob_ref.clone(),
                    compute_snapshot_content_hash(snapshot_ref.blob.as_ref()),
                ))
                .expect("record snapshot anchor");
            backend
                .append_batch(&[append_account_state(2, &replayed_state)])
                .expect("append replay tail");
            backend.seal(RunStatus::Ended).expect("seal run");
        }

        let mut cache = Cache::default();
        cache
            .add(&snapshot_ref.blob_ref, snapshot_ref.blob.clone())
            .expect("seed snapshot blob");

        let report = restore_cache_from_sealed_run(
            &mut cache,
            tmp.path().to_path_buf(),
            instance_id,
            run_id,
        )
        .expect("restore sealed run");

        let frames = cache
            .position_snapshot_bytes(&position.id)
            .expect("restored position snapshot");
        let account = cache
            .account_owned(&replayed_state.account_id)
            .expect("replayed account");

        assert_eq!(report.manifest.run_id, run_id);
        assert_eq!(report.manifest.status, RunStatus::Ended);
        assert_eq!(report.cache.plan.from_seq, 2);
        assert_eq!(report.cache.applied_entries, 1);
        assert_eq!(report.cache.ignored_entries, 0);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].as_slice(), snapshot_ref.blob.as_ref());
        assert_eq!(account.events(), vec![replayed_state]);
    }

    #[rstest]
    fn restore_cache_from_sealed_run_rejects_snapshot_hash_mismatch() {
        let tmp = TempDir::new().expect("tempdir");
        let run_id = "sealed-replay-bad-snapshot";
        let instance_id = "trader-001";
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(PositionId::from("P-SEALED-REPLAY-BAD-SNAPSHOT-1"))
            .build();
        let position = Position::new(&instrument, fill);
        let mut snapshot_cache = Cache::default();
        let snapshot_ref = snapshot_cache
            .snapshot_position(&position)
            .expect("snapshot position");

        {
            let mut backend = RedbBackend::new(tmp.path().to_path_buf());
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .record_snapshot_anchor(SnapshotAnchor::new(
                    0,
                    snapshot_ref.blob_ref.clone(),
                    compute_snapshot_content_hash(snapshot_ref.blob.as_ref()),
                ))
                .expect("record snapshot anchor");
            backend.seal(RunStatus::Ended).expect("seal run");
        }

        let mut cache = Cache::default();
        cache
            .add(
                &snapshot_ref.blob_ref,
                Bytes::from_static(b"tampered snapshot"),
            )
            .expect("seed tampered snapshot blob");

        let err = restore_cache_from_sealed_run(
            &mut cache,
            tmp.path().to_path_buf(),
            instance_id,
            run_id,
        )
        .expect_err("hash mismatch");

        match err {
            CacheReplayError::SnapshotRestore { blob_ref, message } => {
                assert_eq!(blob_ref, snapshot_ref.blob_ref);
                assert!(
                    message.contains("content_hash mismatch"),
                    "message should explain hash mismatch: {message}",
                );
            }
            other => panic!("expected SnapshotRestore, was {other:?}"),
        }
    }

    #[rstest]
    fn open_event_store_replay_source_rejects_running_run() {
        let tmp = TempDir::new().expect("tempdir");
        let run_id = "running-replay";
        {
            let mut backend = RedbBackend::new(tmp.path().to_path_buf());
            backend.open_run(manifest(run_id)).expect("open run");
        }

        let err = open_event_store_replay_source(tmp.path().to_path_buf(), "trader-001", run_id)
            .expect_err("running source must fail");

        assert!(
            err.to_string().contains("not sealed"),
            "error should name sealed-run requirement: {err}",
        );
    }

    #[rstest]
    fn validate_event_store_replay_source_rejects_quarantined_run() {
        let tmp = TempDir::new().expect("tempdir");
        let run_id = "quarantined-replay";
        {
            let mut backend = RedbBackend::new(tmp.path().to_path_buf());
            backend.open_run(manifest(run_id)).expect("open run");
            backend
                .append_batch(&[append_payload(1, "RunStarted", Bytes::new())])
                .expect("append");
            backend.seal(RunStatus::Quarantined).expect("seal run");
        }

        let err =
            validate_event_store_replay_source(tmp.path().to_path_buf(), "trader-001", run_id)
                .expect_err("quarantined source must fail");

        assert!(
            err.to_string().contains("quarantined"),
            "error should reject quarantined replay sources: {err}",
        );
    }
}
