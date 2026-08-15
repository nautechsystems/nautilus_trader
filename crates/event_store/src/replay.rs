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

use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    messages::{
        data::{
            BarsResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
            QuotesResponse, TradesResponse,
        },
        execution::SubmitOrderList,
    },
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    data::{Bar, QuoteTick, TradeTick},
    enums::{OmsType, OrderSide, PositionSide},
    events::{
        AccountState, OrderEventAny, OrderFillVoided, OrderFilled, OrderInitialized,
        PositionAdjusted, PositionChanged, PositionClosed, PositionOpened,
    },
    identifiers::PositionId,
    orders::{Order, OrderAny},
    position::{Position, PositionReplayEvent},
    types::{Money, Quantity},
};
use serde::de::DeserializeOwned;

#[cfg(test)]
use crate::capture::builtins::{
    PAYLOAD_TYPE_BATCH_CANCEL_ORDERS, PAYLOAD_TYPE_BATCH_MODIFY_ORDERS,
    PAYLOAD_TYPE_BOOK_DELTAS_RESPONSE, PAYLOAD_TYPE_BOOK_DEPTH_RESPONSE,
    PAYLOAD_TYPE_BOOK_RESPONSE, PAYLOAD_TYPE_CANCEL_ALL_ORDERS, PAYLOAD_TYPE_CANCEL_ORDER,
    PAYLOAD_TYPE_CUSTOM_DATA_RESPONSE, PAYLOAD_TYPE_EXECUTION_MASS_STATUS,
    PAYLOAD_TYPE_FILL_REPORT, PAYLOAD_TYPE_FORWARD_PRICES_RESPONSE, PAYLOAD_TYPE_MODIFY_ORDER,
    PAYLOAD_TYPE_ORDER_STATUS_REPORT, PAYLOAD_TYPE_ORDER_WITH_FILLS,
    PAYLOAD_TYPE_POSITION_STATUS_REPORT, PAYLOAD_TYPE_QUERY_ACCOUNT, PAYLOAD_TYPE_QUERY_ORDER,
    PAYLOAD_TYPE_REQUEST_COMMAND, PAYLOAD_TYPE_SUBMIT_ORDER, PAYLOAD_TYPE_SUBSCRIBE_COMMAND,
    PAYLOAD_TYPE_TIME_EVENT, PAYLOAD_TYPE_UNSUBSCRIBE_COMMAND,
};
#[cfg(all(test, feature = "defi"))]
use crate::capture::builtins::{
    PAYLOAD_TYPE_DEFI_REQUEST_COMMAND, PAYLOAD_TYPE_DEFI_SUBSCRIBE_COMMAND,
    PAYLOAD_TYPE_DEFI_UNSUBSCRIBE_COMMAND,
};
use crate::{
    RedbBackend,
    backend::{EventStore, ScanDirection},
    capture::builtins::{
        PAYLOAD_TYPE_ACCOUNT_STATE, PAYLOAD_TYPE_BARS_RESPONSE,
        PAYLOAD_TYPE_FUNDING_RATES_RESPONSE, PAYLOAD_TYPE_INSTRUMENT_RESPONSE,
        PAYLOAD_TYPE_INSTRUMENTS_RESPONSE, PAYLOAD_TYPE_ORDER_ACCEPTED,
        PAYLOAD_TYPE_ORDER_CANCEL_REJECTED, PAYLOAD_TYPE_ORDER_CANCELED, PAYLOAD_TYPE_ORDER_DENIED,
        PAYLOAD_TYPE_ORDER_EMULATED, PAYLOAD_TYPE_ORDER_EXPIRED, PAYLOAD_TYPE_ORDER_FILL_VOIDED,
        PAYLOAD_TYPE_ORDER_FILLED, PAYLOAD_TYPE_ORDER_INITIALIZED,
        PAYLOAD_TYPE_ORDER_MODIFY_REJECTED, PAYLOAD_TYPE_ORDER_PENDING_CANCEL,
        PAYLOAD_TYPE_ORDER_PENDING_UPDATE, PAYLOAD_TYPE_ORDER_REJECTED,
        PAYLOAD_TYPE_ORDER_RELEASED, PAYLOAD_TYPE_ORDER_SUBMITTED, PAYLOAD_TYPE_ORDER_TRIGGERED,
        PAYLOAD_TYPE_ORDER_UPDATED, PAYLOAD_TYPE_POSITION_ADJUSTED, PAYLOAD_TYPE_POSITION_CHANGED,
        PAYLOAD_TYPE_POSITION_CLOSED, PAYLOAD_TYPE_POSITION_OPENED, PAYLOAD_TYPE_QUOTES_RESPONSE,
        PAYLOAD_TYPE_SUBMIT_ORDER_LIST, PAYLOAD_TYPE_TRADES_RESPONSE,
    },
    entry::EventStoreEntry,
    error::EventStoreError,
    manifest::{RunManifest, RunStatus},
    reader::{EventStoreReader, SnapshotReplayPlan},
    snapshot::{SnapshotAnchor, compute_snapshot_content_hash},
};

#[cfg(feature = "persistence")]
mod catalog;

#[cfg(feature = "persistence")]
pub use catalog::ParquetReplayCatalog;

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

#[cfg(test)]
pub(crate) const CACHE_REPLAY_CAPTURE_PAYLOAD_TYPES: &[&str] = &[
    PAYLOAD_TYPE_SUBMIT_ORDER_LIST,
    PAYLOAD_TYPE_ACCOUNT_STATE,
    PAYLOAD_TYPE_INSTRUMENT_RESPONSE,
    PAYLOAD_TYPE_INSTRUMENTS_RESPONSE,
    PAYLOAD_TYPE_QUOTES_RESPONSE,
    PAYLOAD_TYPE_TRADES_RESPONSE,
    PAYLOAD_TYPE_FUNDING_RATES_RESPONSE,
    PAYLOAD_TYPE_BARS_RESPONSE,
    PAYLOAD_TYPE_ORDER_INITIALIZED,
    PAYLOAD_TYPE_ORDER_DENIED,
    PAYLOAD_TYPE_ORDER_EMULATED,
    PAYLOAD_TYPE_ORDER_RELEASED,
    PAYLOAD_TYPE_ORDER_SUBMITTED,
    PAYLOAD_TYPE_ORDER_ACCEPTED,
    PAYLOAD_TYPE_ORDER_REJECTED,
    PAYLOAD_TYPE_ORDER_CANCELED,
    PAYLOAD_TYPE_ORDER_EXPIRED,
    PAYLOAD_TYPE_ORDER_TRIGGERED,
    PAYLOAD_TYPE_ORDER_PENDING_UPDATE,
    PAYLOAD_TYPE_ORDER_PENDING_CANCEL,
    PAYLOAD_TYPE_ORDER_MODIFY_REJECTED,
    PAYLOAD_TYPE_ORDER_CANCEL_REJECTED,
    PAYLOAD_TYPE_ORDER_UPDATED,
    PAYLOAD_TYPE_ORDER_FILLED,
    PAYLOAD_TYPE_ORDER_FILL_VOIDED,
    PAYLOAD_TYPE_POSITION_OPENED,
    PAYLOAD_TYPE_POSITION_CHANGED,
    PAYLOAD_TYPE_POSITION_CLOSED,
    PAYLOAD_TYPE_POSITION_ADJUSTED,
];

#[cfg(test)]
pub(crate) const FORENSIC_ONLY_CAPTURE_PAYLOAD_TYPES: &[&str] = &[
    PAYLOAD_TYPE_SUBMIT_ORDER,
    PAYLOAD_TYPE_MODIFY_ORDER,
    PAYLOAD_TYPE_BATCH_MODIFY_ORDERS,
    PAYLOAD_TYPE_CANCEL_ORDER,
    PAYLOAD_TYPE_CANCEL_ALL_ORDERS,
    PAYLOAD_TYPE_BATCH_CANCEL_ORDERS,
    PAYLOAD_TYPE_QUERY_ORDER,
    PAYLOAD_TYPE_QUERY_ACCOUNT,
    PAYLOAD_TYPE_ORDER_STATUS_REPORT,
    PAYLOAD_TYPE_FILL_REPORT,
    PAYLOAD_TYPE_ORDER_WITH_FILLS,
    PAYLOAD_TYPE_POSITION_STATUS_REPORT,
    PAYLOAD_TYPE_EXECUTION_MASS_STATUS,
    PAYLOAD_TYPE_TIME_EVENT,
    PAYLOAD_TYPE_REQUEST_COMMAND,
    PAYLOAD_TYPE_SUBSCRIBE_COMMAND,
    PAYLOAD_TYPE_UNSUBSCRIBE_COMMAND,
    #[cfg(feature = "defi")]
    PAYLOAD_TYPE_DEFI_REQUEST_COMMAND,
    #[cfg(feature = "defi")]
    PAYLOAD_TYPE_DEFI_SUBSCRIBE_COMMAND,
    #[cfg(feature = "defi")]
    PAYLOAD_TYPE_DEFI_UNSUBSCRIBE_COMMAND,
    PAYLOAD_TYPE_CUSTOM_DATA_RESPONSE,
    PAYLOAD_TYPE_BOOK_RESPONSE,
    PAYLOAD_TYPE_BOOK_DELTAS_RESPONSE,
    PAYLOAD_TYPE_BOOK_DEPTH_RESPONSE,
    PAYLOAD_TYPE_FORWARD_PRICES_RESPONSE,
];

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

/// Planned catalog slice joined to a replay input scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSlicePlan {
    /// Resolved catalog query.
    pub query: CatalogSliceQuery,
    /// Catalog coverage reported during planning.
    pub coverage: CatalogSliceCoverage,
}

impl CatalogSlicePlan {
    /// Returns whether the catalog reported no files for this slice.
    #[must_use]
    pub fn is_missing(&self) -> bool {
        self.coverage.is_missing()
    }
}

/// Typed catalog data loaded for replay context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogReplayData {
    /// Quote tick loaded from the `quotes` catalog.
    Quote(QuoteTick),
    /// Trade tick loaded from the `trades` catalog.
    Trade(TradeTick),
    /// Bar loaded from the `bars` catalog.
    Bar(Bar),
}

impl CatalogReplayData {
    /// Returns the catalog data class for this record.
    #[must_use]
    pub const fn data_cls(&self) -> &'static str {
        match self {
            Self::Quote(_) => "quotes",
            Self::Trade(_) => "trades",
            Self::Bar(_) => "bars",
        }
    }

    /// Returns the catalog identifier for this record.
    #[must_use]
    pub fn identifier(&self) -> String {
        match self {
            Self::Quote(quote) => quote.instrument_id.to_string(),
            Self::Trade(trade) => trade.instrument_id.to_string(),
            Self::Bar(bar) => bar.bar_type.to_string(),
        }
    }

    /// Returns the initialization timestamp for this record.
    #[must_use]
    pub const fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Quote(quote) => quote.ts_init,
            Self::Trade(trade) => trade.ts_init,
            Self::Bar(bar) => bar.ts_init,
        }
    }
}

impl From<QuoteTick> for CatalogReplayData {
    fn from(value: QuoteTick) -> Self {
        Self::Quote(value)
    }
}

impl From<TradeTick> for CatalogReplayData {
    fn from(value: TradeTick) -> Self {
        Self::Trade(value)
    }
}

impl From<Bar> for CatalogReplayData {
    fn from(value: Bar) -> Self {
        Self::Bar(value)
    }
}

/// Catalog record loaded for replay context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogReplayRecord {
    /// Catalog data class or directory name for the record.
    pub data_cls: String,
    /// Optional catalog identifier for the record.
    pub identifier: Option<String>,
    /// Record timestamp used for contextual joins.
    pub ts_init: UnixNanos,
    /// Typed catalog data loaded for contextual analysis.
    pub data: CatalogReplayData,
}

impl CatalogReplayRecord {
    /// Builds a typed catalog replay record.
    #[must_use]
    pub fn from_data(data: CatalogReplayData) -> Self {
        Self {
            data_cls: data.data_cls().to_string(),
            identifier: Some(data.identifier()),
            ts_init: data.ts_init(),
            data,
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

/// Planned replay inputs for an event-store scan with optional catalog context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayInputPlan {
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

/// Loaded replay inputs with event-store entries and optional catalog context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayInputs {
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
    /// Implementations return records in catalog order so marker cursor joins can take a
    /// deterministic prefix from a cumulative stream slice.
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
    /// A catalog-joined replay plan had no selected catalog slices.
    #[error("catalog replay requires at least one selected catalog slice")]
    EmptyCatalogSelection,
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

#[derive(Default)]
struct CacheReplayContext {
    allow_deferred_orderless_flips: bool,
    pending_orderless_flips: Vec<PendingOrderlessFlip>,
}

struct PendingOrderlessFlip {
    source_seq: u64,
    source_position_id: PositionId,
    oms_type: OmsType,
    opening_fill: OrderFilled,
}

impl CacheReplayContext {
    fn for_snapshot_tail() -> Self {
        Self {
            allow_deferred_orderless_flips: true,
            pending_orderless_flips: Vec::new(),
        }
    }

    fn contains_flip_source(&self, fill: &OrderFilled) -> bool {
        self.pending_orderless_flips.iter().any(|pending| {
            pending.opening_fill.client_order_id == fill.client_order_id
                && pending.opening_fill.trade_id == fill.trade_id
                && pending.opening_fill.causation_id == Some(fill.event_id)
        })
    }

    fn push_orderless_flip(
        &mut self,
        source_seq: u64,
        source_position_id: PositionId,
        oms_type: OmsType,
        opening_fill: OrderFilled,
    ) {
        self.pending_orderless_flips.push(PendingOrderlessFlip {
            source_seq,
            source_position_id,
            oms_type,
            opening_fill,
        });
    }

    fn take_opening_fill(
        &mut self,
        entry: &EventStoreEntry,
        opened: &PositionOpened,
    ) -> Result<Option<(OrderFilled, OmsType)>, CacheReplayError> {
        let matching: Vec<usize> = self
            .pending_orderless_flips
            .iter()
            .enumerate()
            .filter_map(|(index, pending)| {
                let fill = &pending.opening_fill;
                (fill.trader_id == opened.trader_id
                    && fill.strategy_id == opened.strategy_id
                    && fill.instrument_id == opened.instrument_id
                    && fill.account_id == opened.account_id
                    && fill.client_order_id == opened.opening_order_id
                    && fill.order_side == opened.entry
                    && fill.last_qty == opened.last_qty
                    && fill.last_px == opened.last_px
                    && fill.currency == opened.currency)
                    .then_some(index)
            })
            .collect();

        if matching.len() > 1 {
            return Err(apply_error(
                entry,
                format!(
                    "ambiguous orderless flip recovery for position {}: {} pending fills match",
                    opened.position_id,
                    matching.len(),
                ),
            ));
        }

        let Some(index) = matching.first().copied() else {
            return Ok(None);
        };
        let pending = self.pending_orderless_flips.remove(index);
        let mut fill = pending.opening_fill;
        let oms_type = if pending.source_position_id == opened.position_id {
            pending.oms_type
        } else {
            // A virtual HEDGING flip is the only live orderless path which opens a
            // replacement ID. This recovers the OMS metadata even during a full
            // event-store replay where no cache snapshot supplied it.
            OmsType::Hedging
        };
        fill.position_id = Some(opened.position_id);
        // The live opening fragment is not stored separately. The PositionOpened event ID is
        // stable and identifies the recovered opening transition, while causation still points
        // to the original unsplit venue fill.
        fill.event_id = opened.event_id;
        Ok(Some((fill, oms_type)))
    }

    fn ensure_complete(&self) -> Result<(), CacheReplayError> {
        let Some(pending) = self.pending_orderless_flips.first() else {
            return Ok(());
        };

        Err(CacheReplayError::Apply {
            seq: pending.source_seq,
            payload_type: PAYLOAD_TYPE_ORDER_FILLED.to_string(),
            message: format!(
                "orderless flip fill {} has no matching PositionOpened event",
                pending.opening_fill.trade_id,
            ),
        })
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
    let mut context = CacheReplayContext::for_snapshot_tail();

    for entry in scan {
        let entry = entry?;

        if entry.seq < plan.from_seq {
            return Err(CacheReplayError::UnexpectedSeq {
                seq: entry.seq,
                from_seq: plan.from_seq,
            });
        }

        if apply_cache_replay_entry_with_context(cache, &entry, &mut context)? {
            applied_entries += 1;
        } else {
            ignored_entries += 1;
        }
    }

    context.ensure_complete()?;

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
/// Returns [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn load_forensics_replay_inputs<B>(
    reader: &EventStoreReader<B>,
    plan: &ReplayInputPlan,
) -> Result<ReplayInputs, ReplayInputError>
where
    B: EventStore,
{
    let entries = load_replay_entries(reader, plan.requested_range)?;
    Ok(ReplayInputs {
        entries,
        catalog_slices: Vec::new(),
    })
}

/// Plans replay inputs by joining event-store entries with selected catalog slices.
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
pub fn plan_catalog_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    range: ReplaySeqRange,
    catalog_slices: &[CatalogSliceSelector],
) -> Result<ReplayInputPlan, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    plan_catalog_joined_replay_inputs(reader, catalog, range, catalog_slices)
}

/// Loads catalog replay inputs from an existing plan.
///
/// Event-store entries are returned in durable `seq` order. Catalog records are loaded through
/// the caller-provided catalog source only; this function does not query live venues or run engine
/// logic.
///
/// # Errors
///
/// Returns [`ReplayInputError::MissingCatalogSlice`] when a required slice is missing,
/// [`ReplayInputError::Catalog`] when catalog loading fails, and
/// [`ReplayInputError::EventStore`] when the reader scan fails.
pub fn load_catalog_replay_inputs<B, C>(
    reader: &EventStoreReader<B>,
    catalog: &mut C,
    plan: &ReplayInputPlan,
) -> Result<ReplayInputs, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    load_catalog_joined_replay_inputs(reader, catalog, plan)
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
    range: ReplaySeqRange,
    catalog_slices: &[CatalogSliceSelector],
) -> Result<ReplayInputPlan, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    if catalog_slices.is_empty() {
        return Err(ReplayInputError::EmptyCatalogSelection);
    }

    let span = collect_replay_entry_span(reader, range)?;
    let catalog_slices = plan_catalog_slices(catalog, catalog_slices, span.time_range)?;

    Ok(ReplayInputPlan {
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
) -> Result<ReplayInputs, ReplayInputError>
where
    B: EventStore,
    C: ReplayCatalog,
{
    let entries = load_replay_entries(reader, plan.requested_range)?;
    let catalog_slices = load_catalog_slices(catalog, &plan.catalog_slices)?;

    Ok(ReplayInputs {
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
        plans.push(CatalogSlicePlan { query, coverage });
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
/// payload is outside the current cache bootstrap replay surface, or when the entry's
/// position target cannot be established (a position event's target is absent, or a
/// fill's instrument is missing so the position cannot open). The latter paths log a
/// warning so the report's ignored count surfaces the divergence instead of claiming
/// a full apply.
///
/// # Errors
///
/// Returns [`CacheReplayError::Decode`] when a supported payload cannot be decoded and
/// [`CacheReplayError::Apply`] when the decoded payload cannot be applied to the cache.
pub fn apply_cache_replay_entry(
    cache: &mut Cache,
    entry: &EventStoreEntry,
) -> Result<bool, CacheReplayError> {
    let mut context = CacheReplayContext::default();
    let applied = apply_cache_replay_entry_with_context(cache, entry, &mut context)?;
    context.ensure_complete()?;
    Ok(applied)
}

fn apply_cache_replay_entry_with_context(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    context: &mut CacheReplayContext,
) -> Result<bool, CacheReplayError> {
    if apply_complete_cache_payload_entry(cache, entry)? {
        return Ok(true);
    }

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
        PAYLOAD_TYPE_ORDER_FILLED => return apply_order_filled(cache, entry, context),
        PAYLOAD_TYPE_ORDER_FILL_VOIDED => {
            let fill_voided = decode_payload::<OrderFillVoided>(entry)?;
            apply_fill_void_to_order_and_positions(cache, entry, &fill_voided)?;
        }
        PAYLOAD_TYPE_POSITION_OPENED => {
            let opened = decode_payload::<PositionOpened>(entry)?;
            if let Some(applied) =
                apply_pending_orderless_flip_opened(cache, entry, &opened, context)?
            {
                return Ok(applied);
            }
            return apply_position_opened(cache, entry, &opened);
        }
        PAYLOAD_TYPE_POSITION_CHANGED => {
            let changed = decode_payload::<PositionChanged>(entry)?;
            return apply_position_changed(cache, entry, &changed);
        }
        PAYLOAD_TYPE_POSITION_CLOSED => {
            let closed = decode_payload::<PositionClosed>(entry)?;
            return apply_position_closed(cache, entry, &closed);
        }
        PAYLOAD_TYPE_POSITION_ADJUSTED => {
            let adjustment = decode_payload::<PositionAdjusted>(entry)?;
            return apply_position_adjustment(cache, entry, adjustment);
        }
        _ => return Ok(false),
    }

    Ok(true)
}

fn apply_complete_cache_payload_entry(
    cache: &mut Cache,
    entry: &EventStoreEntry,
) -> Result<bool, CacheReplayError> {
    match entry.payload_type.as_str() {
        PAYLOAD_TYPE_SUBMIT_ORDER_LIST => {
            let command = decode_payload::<SubmitOrderList>(entry)?;
            apply_result(entry, cache.add_order_list(command.order_list))?;
        }
        PAYLOAD_TYPE_INSTRUMENT_RESPONSE => {
            let response = decode_payload::<InstrumentResponse>(entry)?;
            apply_result(entry, cache.add_instrument(response.data))?;
        }
        PAYLOAD_TYPE_INSTRUMENTS_RESPONSE => {
            let response = decode_payload::<InstrumentsResponse>(entry)?;
            for instrument in response.data {
                apply_result(entry, cache.add_instrument(instrument))?;
            }
        }
        PAYLOAD_TYPE_QUOTES_RESPONSE => {
            let response = decode_payload::<QuotesResponse>(entry)?;
            if !response.data.is_empty() {
                apply_result(entry, cache.add_quotes(&response.data))?;
            }
        }
        PAYLOAD_TYPE_TRADES_RESPONSE => {
            let response = decode_payload::<TradesResponse>(entry)?;
            if !response.data.is_empty() {
                apply_result(entry, cache.add_trades(&response.data))?;
            }
        }
        PAYLOAD_TYPE_FUNDING_RATES_RESPONSE => {
            let response = decode_payload::<FundingRatesResponse>(entry)?;
            if !response.data.is_empty() {
                apply_result(entry, cache.add_funding_rates(&response.data))?;
            }
        }
        PAYLOAD_TYPE_BARS_RESPONSE => {
            let response = decode_payload::<BarsResponse>(entry)?;
            if !response.data.is_empty() {
                apply_result(entry, cache.add_bars(&response.data))?;
            }
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

fn apply_order_filled(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    context: &mut CacheReplayContext,
) -> Result<bool, CacheReplayError> {
    let fill = decode_payload::<OrderFilled>(entry)?;
    // The fill side panics deep inside Position/Order application; the hash
    // proves the bytes match what was written, not that the producer wrote a
    // semantically valid fill, so guard before the model invariants fire.
    if matches!(fill.order_side, OrderSide::NoOrderSide) {
        return Err(apply_error(
            entry,
            "OrderFilled.order_side must be Buy or Sell, was NoOrderSide",
        ));
    }
    let event = OrderEventAny::Filled(fill.clone());
    let orderless_leg_fill = is_orderless_leg_fill(cache, &fill);
    if !orderless_leg_fill {
        apply_result(entry, cache.update_order(&event))?;
    }

    let flip_applied =
        orderless_leg_fill && apply_orderless_flip_fill(cache, entry, &fill, context)?;

    if flip_applied {
        return Ok(true);
    }

    apply_fill_to_position(cache, entry, &fill, orderless_leg_fill)
}

fn apply_orderless_flip_fill(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    fill: &OrderFilled,
    context: &mut CacheReplayContext,
) -> Result<bool, CacheReplayError> {
    if context.contains_flip_source(fill) {
        return Ok(true);
    }

    let Some(position_id) = fill.position_id else {
        return Ok(false);
    };
    let Some(mut position) = cache.position_owned(&position_id) else {
        return Ok(false);
    };

    if position.is_closed()
        || !position.is_opposite_side(fill.order_side)
        || fill.last_qty.raw <= position.quantity.raw
    {
        return Ok(false);
    }

    if position.side != PositionSide::Flat && position.trade_ids().contains(&fill.trade_id) {
        return Ok(true);
    }

    if !context.allow_deferred_orderless_flips {
        return Err(apply_error(
            entry,
            "orderless position flip requires snapshot-tail replay context to match the following PositionOpened event",
        ));
    }

    let oms_type = cache.oms_type(&position_id).unwrap_or(OmsType::Unspecified);
    let (closing_fill, opening_fill) = fill
        .split_for_position_flip(position.quantity, None, fill.event_id)
        .map_err(|e| apply_error(entry, e))?;
    position.apply(&closing_fill);
    apply_result(entry, cache.update_position(&position))?;
    context.push_orderless_flip(entry.seq, position_id, oms_type, opening_fill);
    Ok(true)
}

fn apply_pending_orderless_flip_opened(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    opened: &PositionOpened,
    context: &mut CacheReplayContext,
) -> Result<Option<bool>, CacheReplayError> {
    let Some((opening_fill, oms_type)) = context.take_opening_fill(entry, opened)? else {
        return Ok(None);
    };
    let instrument = cache
        .instrument(&opening_fill.instrument_id)
        .cloned()
        .ok_or_else(|| {
            apply_error(
                entry,
                format!(
                    "instrument {} not found for orderless flip position {}",
                    opening_fill.instrument_id, opened.position_id,
                ),
            )
        })?;
    let prior = cache.position_owned(&opened.position_id);
    let mut position = Position::new(&instrument, opening_fill);
    if let Some(prior) = prior {
        let current_replay = position.replay_events.clone();
        position.replay_events = prior.replay_events;
        position.replay_events.extend(current_replay);
        position.fill_voids = prior.fill_voids;
    }
    apply_result(entry, cache.add_position_without_order(&position, oms_type))?;

    apply_position_opened(cache, entry, opened).map(Some)
}

// Returns `Ok(true)` when the position side applied (no position association, or an
// idempotent replay no-op) and `Ok(false)` when the instrument needed to open the
// position is missing, so the report's ignored count surfaces the divergence.
fn apply_fill_to_position(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    fill: &OrderFilled,
    orderless_leg_fill: bool,
) -> Result<bool, CacheReplayError> {
    let Some(position_id) = fill.position_id else {
        return Ok(true);
    };

    if let Some(mut position) = cache.position_owned(&position_id) {
        // Mirror live `Position::apply_fill`: a duplicate inside an open episode is
        // the idempotent replay no-op; historical duplicates on a Flat position are
        // ignored inside `apply` itself from the carried replay history.
        if position.side != PositionSide::Flat && position.trade_ids().contains(&fill.trade_id) {
            return Ok(true);
        }

        position.apply(fill);
        apply_result(entry, cache.update_position(&position))?;
        return Ok(true);
    }

    let Some(instrument) = cache.instrument(&fill.instrument_id).cloned() else {
        log::warn!(
            "Replay seq {} skipped opening position {position_id}: instrument {} not in cache",
            entry.seq,
            fill.instrument_id,
        );
        return Ok(false);
    };

    let position = Position::new(&instrument, fill.clone());

    if orderless_leg_fill {
        apply_result(
            entry,
            cache.add_position_without_order(&position, OmsType::Unspecified),
        )?;
    } else {
        apply_result(entry, cache.add_position(&position, OmsType::Unspecified))?;
    }
    Ok(true)
}

fn is_orderless_leg_fill(cache: &Cache, fill: &OrderFilled) -> bool {
    if !fill.client_order_id.as_str().contains("-LEG-")
        && !fill.venue_order_id.as_str().contains("-LEG-")
    {
        return false;
    }

    let is_non_spread_instrument = cache
        .instrument(&fill.instrument_id)
        .is_none_or(|instrument| !instrument.is_spread());

    is_non_spread_instrument
        && !cache.order_exists(&fill.client_order_id)
        && cache.client_order_id(&fill.venue_order_id).is_none()
}

fn apply_fill_void_to_order_and_positions(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    fill_voided: &OrderFillVoided,
) -> Result<(), CacheReplayError> {
    let event = OrderEventAny::FillVoided(fill_voided.clone());
    let order = cache
        .order_owned(&fill_voided.client_order_id)
        .ok_or_else(|| {
            apply_error(
                entry,
                format!("order {} not found", fill_voided.client_order_id),
            )
        })?;
    let original_fill = order
        .events()
        .into_iter()
        .find_map(|candidate| match candidate {
            OrderEventAny::Filled(fill) if fill.trade_id == fill_voided.trade_id => Some(fill),
            _ => None,
        });

    let mut validated_order = order.clone();
    apply_result(entry, validated_order.apply(event.clone()))?;

    let corrected_positions =
        if let Some(original_fill) = original_fill.filter(|fill| fill.position_id.is_some()) {
            prepare_fill_void_positions(cache, entry, fill_voided, original_fill.event_id)?
        } else {
            Vec::new()
        };

    apply_result(entry, cache.update_order(&event))?;
    for position in corrected_positions {
        apply_result(entry, cache.update_position(&position))?;
    }
    Ok(())
}

fn prepare_fill_void_positions(
    cache: &Cache,
    entry: &EventStoreEntry,
    fill_voided: &OrderFillVoided,
    source_event_id: UUID4,
) -> Result<Vec<Position>, CacheReplayError> {
    let fragments = collect_fill_void_fragments(cache, entry, fill_voided, source_event_id)?;
    let allocations = allocate_fill_void_fragments(entry, fill_voided, &fragments)?;
    let mut corrected_positions = Vec::new();

    for (position_id, (voided_qty, commission_voided)) in allocations {
        if voided_qty.is_zero() {
            return Err(apply_error(
                entry,
                format!(
                    "commission-only position correction requires authoritative reconciliation for fill {}",
                    fill_voided.trade_id
                ),
            ));
        }
        let mut position = cache
            .position_owned(&position_id)
            .ok_or_else(|| apply_error(entry, format!("position {position_id} not found")))?;
        let previous = position
            .fill_voids
            .iter()
            .rev()
            .find(|record| {
                record.event.client_order_id == fill_voided.client_order_id
                    && record.event.trade_id == fill_voided.trade_id
            })
            .map(|record| (record.voided_qty, record.commission_voided));
        if previous == Some((voided_qty, commission_voided)) {
            continue;
        }
        apply_result(
            entry,
            position.apply_fill_void(fill_voided.clone(), voided_qty, commission_voided),
        )?;
        corrected_positions.push(position);
    }
    Ok(corrected_positions)
}

#[derive(Clone, Copy, Debug)]
struct FillVoidFragment {
    position_id: PositionId,
    split_rank: u8,
    quantity: Quantity,
    commission: Option<Money>,
}

fn collect_fill_void_fragments(
    cache: &Cache,
    entry: &EventStoreEntry,
    fill_voided: &OrderFillVoided,
    source_event_id: UUID4,
) -> Result<Vec<FillVoidFragment>, CacheReplayError> {
    let positions: Vec<Position> = cache
        .positions(
            None,
            Some(&fill_voided.instrument_id),
            Some(&fill_voided.strategy_id),
            Some(&fill_voided.account_id),
            None,
        )
        .into_iter()
        .map(|position| position.cloned())
        .collect();
    let mut fragments = Vec::new();

    for position in &positions {
        for replay_event in &position.replay_events {
            let PositionReplayEvent::Filled(fill) = replay_event else {
                continue;
            };

            if fill.client_order_id != fill_voided.client_order_id
                || fill.trade_id != fill_voided.trade_id
            {
                continue;
            }
            let split_rank = if fill.event_id == source_event_id {
                0
            } else if fill.causation_id == Some(source_event_id) {
                1
            } else {
                continue;
            };
            fragments.push(FillVoidFragment {
                position_id: position.id,
                split_rank,
                quantity: fill.last_qty,
                commission: fill.commission,
            });
        }
    }

    if fragments.is_empty() {
        return Err(apply_error(
            entry,
            format!(
                "no position fragments found for fill {}",
                fill_voided.trade_id
            ),
        ));
    }
    fragments.sort_by_key(|fragment| fragment.split_rank);
    Ok(fragments)
}

fn allocate_fill_void_fragments(
    entry: &EventStoreEntry,
    fill_voided: &OrderFillVoided,
    fragments: &[FillVoidFragment],
) -> Result<IndexMap<PositionId, (Quantity, Option<Money>)>, CacheReplayError> {
    let mut allocations = IndexMap::<PositionId, (Quantity, Option<Money>)>::new();
    let mut remaining_qty = fill_voided.voided_qty;
    for fragment in fragments.iter().rev() {
        if remaining_qty.is_zero() {
            break;
        }
        let removed = remaining_qty.min(fragment.quantity);
        allocations
            .entry(fragment.position_id)
            .and_modify(|allocation| allocation.0 = allocation.0 + removed)
            .or_insert((removed, None));
        remaining_qty = remaining_qty - removed;
    }

    if !remaining_qty.is_zero() {
        return Err(apply_error(
            entry,
            format!(
                "position fragments do not cover voided quantity for fill {}",
                fill_voided.trade_id
            ),
        ));
    }

    if let Some(mut remaining_commission) = fill_voided.commission_voided {
        for fragment in fragments.iter().rev() {
            if remaining_commission.is_zero() {
                break;
            }
            let Some(commission) = fragment.commission else {
                continue;
            };

            if commission.currency != remaining_commission.currency {
                return Err(apply_error(
                    entry,
                    format!(
                        "position commission currency differs for fill {}",
                        fill_voided.trade_id
                    ),
                ));
            }
            let removed_raw = remaining_commission.raw.abs().min(commission.raw.abs());
            let removed = Money::from_raw(
                removed_raw * remaining_commission.raw.signum(),
                remaining_commission.currency,
            );
            allocations
                .entry(fragment.position_id)
                .and_modify(|allocation| {
                    allocation.1 = Some(
                        allocation
                            .1
                            .map_or(removed, |commission| commission + removed),
                    );
                })
                .or_insert((
                    Quantity::zero(fill_voided.voided_qty.precision),
                    Some(removed),
                ));
            remaining_commission = remaining_commission - removed;
        }

        if !remaining_commission.is_zero() {
            return Err(apply_error(
                entry,
                format!(
                    "position fragments do not cover voided commission for fill {}",
                    fill_voided.trade_id
                ),
            ));
        }
    }
    Ok(allocations)
}

fn apply_position_opened(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    opened: &PositionOpened,
) -> Result<bool, CacheReplayError> {
    let Some(mut position) = cache.position_owned(&opened.position_id) else {
        warn_position_skip(entry, opened.position_id);
        return Ok(false);
    };

    position.trader_id = opened.trader_id;
    position.strategy_id = opened.strategy_id;
    position.instrument_id = opened.instrument_id;
    position.id = opened.position_id;
    position.account_id = opened.account_id;
    position.opening_order_id = opened.opening_order_id;
    position.closing_order_id = None;
    position.entry = opened.entry;
    position.side = opened.side;
    position.signed_qty = opened.signed_qty;
    position.quantity = opened.quantity;
    position.peak_qty = opened.quantity;
    position.quote_currency = opened.currency;
    position.ts_opened = opened.ts_event;
    position.ts_last = opened.ts_event;
    position.ts_closed = None;
    position.duration_ns = 0;
    position.avg_px_open = opened.avg_px_open;
    position.avg_px_close = None;
    position.realized_return = 0.0;
    position.realized_pnl = opened.realized_pnl;

    apply_result(entry, cache.update_position(&position))?;
    Ok(true)
}

fn apply_position_changed(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    changed: &PositionChanged,
) -> Result<bool, CacheReplayError> {
    let Some(mut position) = cache.position_owned(&changed.position_id) else {
        warn_position_skip(entry, changed.position_id);
        return Ok(false);
    };

    position.trader_id = changed.trader_id;
    position.strategy_id = changed.strategy_id;
    position.instrument_id = changed.instrument_id;
    position.id = changed.position_id;
    position.account_id = changed.account_id;
    position.opening_order_id = changed.opening_order_id;
    position.entry = changed.entry;
    position.side = changed.side;
    position.signed_qty = changed.signed_qty;
    position.quantity = changed.quantity;
    position.peak_qty = changed.peak_quantity;
    position.quote_currency = changed.currency;
    position.ts_opened = changed.ts_opened;
    position.ts_last = changed.ts_event;
    position.ts_closed = None;
    position.avg_px_open = changed.avg_px_open;
    position.avg_px_close = changed.avg_px_close;
    position.realized_return = changed.realized_return;
    position.realized_pnl = changed.realized_pnl;

    apply_result(entry, cache.update_position(&position))?;
    Ok(true)
}

fn apply_position_closed(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    closed: &PositionClosed,
) -> Result<bool, CacheReplayError> {
    let Some(mut position) = cache.position_owned(&closed.position_id) else {
        warn_position_skip(entry, closed.position_id);
        return Ok(false);
    };

    position.trader_id = closed.trader_id;
    position.strategy_id = closed.strategy_id;
    position.instrument_id = closed.instrument_id;
    position.id = closed.position_id;
    position.account_id = closed.account_id;
    position.opening_order_id = closed.opening_order_id;
    position.closing_order_id = closed.closing_order_id;
    position.entry = closed.entry;
    position.side = closed.side;
    position.signed_qty = closed.signed_qty;
    position.quantity = closed.quantity;
    position.peak_qty = closed.peak_quantity;
    position.quote_currency = closed.currency;
    position.ts_opened = closed.ts_opened;
    position.ts_last = closed.ts_event;
    position.ts_closed = closed.ts_closed;
    position.duration_ns = closed.duration;
    position.avg_px_open = closed.avg_px_open;
    position.avg_px_close = closed.avg_px_close;
    position.realized_return = closed.realized_return;
    position.realized_pnl = closed.realized_pnl;

    apply_result(entry, cache.update_position(&position))?;
    Ok(true)
}

fn apply_position_adjustment(
    cache: &mut Cache,
    entry: &EventStoreEntry,
    adjustment: PositionAdjusted,
) -> Result<bool, CacheReplayError> {
    let Some(mut position) = cache.position_owned(&adjustment.position_id) else {
        warn_position_skip(entry, adjustment.position_id);
        return Ok(false);
    };

    position.apply_adjustment(adjustment);
    apply_result(entry, cache.update_position(&position))?;
    Ok(true)
}

// A position event whose position is absent cannot apply; counting it as applied would
// let a restore report full success while an open position is missing from the cache.
fn warn_position_skip(entry: &EventStoreEntry, position_id: PositionId) {
    log::warn!(
        "Replay seq {} skipped {}: position {position_id} not in cache",
        entry.seq,
        entry.payload_type,
    );
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

    use ahash::AHashSet;
    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_common::msgbus::{self, BusTap, Endpoint, MStr, Topic as BusTopic};
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        accounts::AccountAny,
        data::{Bar, BarSpecification, BarType, FundingRateUpdate, QuoteTick, TradeTick},
        enums::{
            AggregationSource, AggressorSide, BarAggregation, OrderSide, OrderStatus,
            PositionAdjustmentType, PriceType,
        },
        events::{
            PositionEvent,
            account::stubs::{cash_account_state, cash_account_state_million_usd},
            order::spec::{
                OrderAcceptedSpec, OrderFillVoidedSpec, OrderFilledSpec, OrderInitializedSpec,
                OrderSubmittedSpec,
            },
        },
        identifiers::{
            AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, PositionId, TradeId,
            VenueOrderId,
        },
        instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
        orders::{Order, OrderList},
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;
    use serde::Serialize;
    use tempfile::TempDir;
    use ustr::Ustr;

    use super::*;
    use crate::{
        backend::{AppendEntry, MemoryBackend, RedbBackend},
        capture::{
            builtins::{
                DEFAULT_CAPTURE_PAYLOAD_TYPES, encode_order_event_any, encode_position_event,
            },
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

    fn append_serde_payload<T: Serialize>(seq: u64, payload_type: &str, value: &T) -> AppendEntry {
        let payload = rmp_serde::to_vec_named(value).expect("encode replay payload");
        append_payload(seq, payload_type, Bytes::from(payload))
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

    fn catalog_quote_record(ts_init: u64) -> CatalogReplayRecord {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        CatalogReplayRecord::from_data(CatalogReplayData::Quote(QuoteTick::new(
            instrument_id,
            Price::from("1.0001"),
            Price::from("1.0002"),
            Quantity::from("100"),
            Quantity::from("100"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )))
    }

    fn catalog_trade_record(ts_init: u64) -> CatalogReplayRecord {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        CatalogReplayRecord::from_data(CatalogReplayData::Trade(TradeTick::new(
            instrument_id,
            Price::from("1.0001"),
            Quantity::from("100"),
            AggressorSide::Buy,
            TradeId::from("T-1"),
            UnixNanos::from(ts_init),
            UnixNanos::from(ts_init),
        )))
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

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum CacheMutationRecoveryClass {
        SnapshotOwned,
        EventStoreCapturedAndReplayed,
        ForensicOnly,
        MissingLiveRecovery,
    }

    #[derive(Clone, Copy, Debug)]
    struct CacheMutationCoverage {
        method: &'static str,
        class: CacheMutationRecoveryClass,
        payload_types: &'static [&'static str],
    }

    const CACHE_MUTATION_COVERAGE: &[CacheMutationCoverage] = &[
        cache_mutation(
            "set_database",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "cache_general",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation("cache_all", CacheMutationRecoveryClass::SnapshotOwned, &[]),
        cache_mutation(
            "cache_currencies",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "cache_instruments",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "cache_synthetics",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "cache_accounts",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "cache_orders",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "cache_positions",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "build_index",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "purge_closed_orders",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "purge_closed_positions",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "purge_order",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "purge_position",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "settle_position_snapshots",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "purge_instrument",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "purge_instrument_skip_order_guard",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "purge_account_events",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "clear_index",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation("reset", CacheMutationRecoveryClass::SnapshotOwned, &[]),
        cache_mutation("dispose", CacheMutationRecoveryClass::SnapshotOwned, &[]),
        cache_mutation("flush_db", CacheMutationRecoveryClass::SnapshotOwned, &[]),
        cache_mutation("add", CacheMutationRecoveryClass::SnapshotOwned, &[]),
        cache_mutation(
            "add_order_book",
            CacheMutationRecoveryClass::ForensicOnly,
            &[PAYLOAD_TYPE_BOOK_RESPONSE],
        ),
        cache_mutation(
            "add_own_order_book",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "add_mark_price",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "add_index_price",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "add_funding_rate",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_FUNDING_RATES_RESPONSE],
        ),
        cache_mutation(
            "add_funding_rates",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_FUNDING_RATES_RESPONSE],
        ),
        cache_mutation(
            "add_instrument_status",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "add_quote",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_QUOTES_RESPONSE],
        ),
        cache_mutation(
            "add_quotes",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_QUOTES_RESPONSE],
        ),
        cache_mutation(
            "add_trade",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_TRADES_RESPONSE],
        ),
        cache_mutation(
            "add_trades",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_TRADES_RESPONSE],
        ),
        cache_mutation(
            "add_bar",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_BARS_RESPONSE],
        ),
        cache_mutation(
            "add_bars",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_BARS_RESPONSE],
        ),
        cache_mutation(
            "add_greeks",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "add_option_greeks",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "add_yield_curve",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "add_currency",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "add_instrument",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[
                PAYLOAD_TYPE_INSTRUMENT_RESPONSE,
                PAYLOAD_TYPE_INSTRUMENTS_RESPONSE,
            ],
        ),
        cache_mutation(
            "add_synthetic",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "add_account",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ACCOUNT_STATE],
        ),
        cache_mutation(
            "add_venue_order_id",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ORDER_ACCEPTED, PAYLOAD_TYPE_ORDER_UPDATED],
        ),
        cache_mutation(
            "add_order",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ORDER_INITIALIZED],
        ),
        cache_mutation(
            "add_order_list",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_SUBMIT_ORDER_LIST],
        ),
        cache_mutation(
            "add_position_id",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[
                PAYLOAD_TYPE_ORDER_FILLED,
                PAYLOAD_TYPE_POSITION_OPENED,
                PAYLOAD_TYPE_POSITION_CHANGED,
                PAYLOAD_TYPE_POSITION_CLOSED,
            ],
        ),
        cache_mutation(
            "add_position",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ORDER_FILLED],
        ),
        cache_mutation(
            "add_position_without_order",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ORDER_FILLED],
        ),
        cache_mutation(
            "update_account",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ACCOUNT_STATE],
        ),
        cache_mutation(
            "take_account",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ACCOUNT_STATE],
        ),
        cache_mutation(
            "cache_account_owned",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ACCOUNT_STATE],
        ),
        cache_mutation(
            "update_account_owned",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ACCOUNT_STATE],
        ),
        cache_mutation(
            "update_account_state",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[PAYLOAD_TYPE_ACCOUNT_STATE],
        ),
        cache_mutation(
            "replace_order",
            CacheMutationRecoveryClass::ForensicOnly,
            &[
                PAYLOAD_TYPE_ORDER_STATUS_REPORT,
                PAYLOAD_TYPE_ORDER_WITH_FILLS,
                PAYLOAD_TYPE_EXECUTION_MASS_STATUS,
            ],
        ),
        cache_mutation(
            "update_order",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[
                PAYLOAD_TYPE_ORDER_DENIED,
                PAYLOAD_TYPE_ORDER_EMULATED,
                PAYLOAD_TYPE_ORDER_RELEASED,
                PAYLOAD_TYPE_ORDER_SUBMITTED,
                PAYLOAD_TYPE_ORDER_ACCEPTED,
                PAYLOAD_TYPE_ORDER_REJECTED,
                PAYLOAD_TYPE_ORDER_CANCELED,
                PAYLOAD_TYPE_ORDER_EXPIRED,
                PAYLOAD_TYPE_ORDER_TRIGGERED,
                PAYLOAD_TYPE_ORDER_PENDING_UPDATE,
                PAYLOAD_TYPE_ORDER_PENDING_CANCEL,
                PAYLOAD_TYPE_ORDER_MODIFY_REJECTED,
                PAYLOAD_TYPE_ORDER_CANCEL_REJECTED,
                PAYLOAD_TYPE_ORDER_UPDATED,
                PAYLOAD_TYPE_ORDER_FILLED,
                PAYLOAD_TYPE_ORDER_FILL_VOIDED,
            ],
        ),
        cache_mutation(
            "update_order_pending_cancel_local",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "update_position",
            CacheMutationRecoveryClass::EventStoreCapturedAndReplayed,
            &[
                PAYLOAD_TYPE_ORDER_FILLED,
                PAYLOAD_TYPE_ORDER_FILL_VOIDED,
                PAYLOAD_TYPE_POSITION_OPENED,
                PAYLOAD_TYPE_POSITION_CHANGED,
                PAYLOAD_TYPE_POSITION_CLOSED,
                PAYLOAD_TYPE_POSITION_ADJUSTED,
            ],
        ),
        cache_mutation(
            "snapshot_position",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "snapshot_position_encoded",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "snapshot_position_state",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "load_snapshot_blob",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "restore_snapshot_blob",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "order_mut",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "position_mut",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "order_book_mut",
            CacheMutationRecoveryClass::ForensicOnly,
            &[
                PAYLOAD_TYPE_BOOK_DELTAS_RESPONSE,
                PAYLOAD_TYPE_BOOK_DEPTH_RESPONSE,
            ],
        ),
        cache_mutation(
            "own_order_book_mut",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "set_mark_xrate",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "clear_mark_xrate",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "clear_mark_xrates",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "account_mut",
            CacheMutationRecoveryClass::MissingLiveRecovery,
            &[],
        ),
        cache_mutation(
            "update_own_order_book",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "force_remove_from_own_order_book",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
        cache_mutation(
            "audit_own_order_books",
            CacheMutationRecoveryClass::SnapshotOwned,
            &[],
        ),
    ];

    const CACHE_MUTATION_EXCLUSIONS: &[&str] = &["check_integrity"];

    const fn cache_mutation(
        method: &'static str,
        class: CacheMutationRecoveryClass,
        payload_types: &'static [&'static str],
    ) -> CacheMutationCoverage {
        CacheMutationCoverage {
            method,
            class,
            payload_types,
        }
    }

    fn cache_public_methods() -> AHashSet<&'static str> {
        collect_cache_public_methods(false)
    }

    fn cache_public_mutable_methods() -> AHashSet<&'static str> {
        collect_cache_public_methods(true)
    }

    /// Every file carrying an `impl Cache` block, since `include_str!` cannot glob a directory.
    /// Add a file here when the cache module is split further, or its methods drop out of this
    /// classification guard.
    const CACHE_IMPL_SOURCES: &[&str] = &[
        include_str!("../../common/src/cache/mod.rs"),
        include_str!("../../common/src/cache/position.rs"),
    ];

    fn collect_cache_public_methods(require_mut_self: bool) -> AHashSet<&'static str> {
        let mut methods = AHashSet::new();
        let mut pending_name: Option<&'static str> = None;
        let mut pending_signature = String::new();

        for line in CACHE_IMPL_SOURCES.iter().flat_map(|source| source.lines()) {
            let trimmed = line.trim_start();

            if pending_name.is_none() {
                let Some(rest) = trimmed
                    .strip_prefix("pub fn ")
                    .or_else(|| trimmed.strip_prefix("pub async fn "))
                else {
                    continue;
                };
                pending_name = rest.split('(').next();
                pending_signature.clear();
                pending_signature.push_str(trimmed);
            } else {
                pending_signature.push(' ');
                pending_signature.push_str(trimmed);
            }

            if trimmed.contains('{') {
                if let Some(name) = pending_name.take()
                    && (!require_mut_self || pending_signature.contains("&mut self"))
                {
                    methods.insert(name);
                }
                pending_signature.clear();
            }
        }

        methods
    }

    fn sorted_missing_methods<'a>(
        actual: &'a AHashSet<&'static str>,
        classified: &'a AHashSet<&'static str>,
    ) -> Vec<&'static str> {
        let mut missing: Vec<_> = actual
            .iter()
            .copied()
            .filter(|method| !classified.contains(method))
            .collect();
        missing.sort_unstable();
        missing
    }

    fn sorted_stale_methods<'a>(
        classified: &'a AHashSet<&'static str>,
        actual: &'a AHashSet<&'static str>,
    ) -> Vec<&'static str> {
        let mut stale: Vec<_> = classified
            .iter()
            .copied()
            .filter(|method| !actual.contains(method))
            .collect();
        stale.sort_unstable();
        stale
    }

    #[rstest]
    fn catalog_replay_inputs_join_event_entries_with_selected_catalog_slice() {
        let reader = reader_with_entries(
            "run-catalog",
            &[
                append_payload_with_ts(1, 120, "RunStarted", Bytes::from_static(b"started")),
                append_payload_with_ts(2, 100, "SubmitOrder", Bytes::from_static(b"submit")),
            ],
        );
        let record = catalog_quote_record(110);
        let mut catalog = FakeReplayCatalog::new(
            CatalogSliceCoverage::from_files(vec!["quotes/AUDUSD.SIM/100_120.parquet".into()]),
            vec![record.clone()],
        );

        let plan = plan_catalog_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 2),
            &[CatalogSliceSelector::new("quotes").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan catalog replay");

        assert_eq!(plan.event_range, Some(ReplaySeqRange::new(1, 2)));
        assert_eq!(plan.event_count, 2);
        assert_eq!(
            plan.event_time_range,
            Some(ReplayTimeRange::new(
                UnixNanos::from(100),
                UnixNanos::from(120),
            )),
        );
        assert!(!plan.catalog_slices[0].is_missing());
        assert_eq!(catalog.plan_queries.len(), 1);
        assert_eq!(catalog.plan_queries[0].data_cls, "quotes");
        assert_eq!(
            catalog.plan_queries[0].identifiers,
            vec!["AUD/USD.SIM".to_string()],
        );
        assert_eq!(catalog.plan_queries[0].start, UnixNanos::from(100));
        assert_eq!(catalog.plan_queries[0].end, UnixNanos::from(120));

        let loaded =
            load_catalog_replay_inputs(&reader, &mut catalog, &plan).expect("load catalog");
        let seqs: Vec<_> = loaded.entries.iter().map(|entry| entry.seq).collect();

        assert_eq!(seqs, vec![1, 2]);
        assert_eq!(loaded.catalog_slices.len(), 1);
        assert_eq!(loaded.catalog_slices[0].records, vec![record]);
        assert_eq!(catalog.load_plans.len(), 1);
    }

    #[rstest]
    fn catalog_plan_marks_missing_catalog_slice() {
        let reader = reader_with_entries(
            "run-missing-catalog",
            &[append_payload_with_ts(
                1,
                1_000,
                "RunStarted",
                Bytes::from_static(b"started"),
            )],
        );
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());

        let plan = plan_catalog_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("trades").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan catalog replay");
        let missing = plan.missing_catalog_slices();

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
        let plan = plan_catalog_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("quotes")
                .with_identifier("AUD/USD.SIM")
                .require_coverage()],
        )
        .expect("plan missing slice");

        let err = load_catalog_replay_inputs(&reader, &mut catalog, &plan)
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
        let plan = plan_catalog_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 1),
            &[CatalogSliceSelector::new("quotes").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan optional missing slice");

        let loaded =
            load_catalog_replay_inputs(&reader, &mut catalog, &plan).expect("load optional");

        assert_eq!(loaded.catalog_slices.len(), 1);
        assert!(loaded.catalog_slices[0].plan.is_missing());
        assert!(loaded.catalog_slices[0].records.is_empty());
        assert!(catalog.load_plans.is_empty());
    }

    #[rstest]
    fn catalog_joined_planner_rejects_empty_catalog_selection() {
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

        let err = plan_catalog_replay_inputs(&reader, &mut catalog, ReplaySeqRange::new(1, 1), &[])
            .expect_err("empty catalog selection must fail");

        match err {
            ReplayInputError::EmptyCatalogSelection => {}
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

        let plan = plan_catalog_replay_inputs(
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
    fn catalog_replay_inputs_load_catalog_records() {
        let reader = reader_with_entries(
            "run-catalog-load",
            &[
                append_payload_with_ts(1, 100, "RunStarted", Bytes::from_static(b"started")),
                append_payload_with_ts(2, 110, "OrderFilled", Bytes::from_static(b"filled")),
            ],
        );
        let record = catalog_trade_record(105);
        let mut catalog = FakeReplayCatalog::new(
            CatalogSliceCoverage::from_files(vec!["trades/AUDUSD.SIM/100_110.parquet".into()]),
            vec![record.clone()],
        );
        let plan = plan_catalog_replay_inputs(
            &reader,
            &mut catalog,
            ReplaySeqRange::new(1, 2),
            &[CatalogSliceSelector::new("trades").with_identifier("AUD/USD.SIM")],
        )
        .expect("plan catalog replay");

        assert_eq!(
            plan.catalog_slices[0].query.identifiers_option(),
            Some(vec!["AUD/USD.SIM".to_string()]),
        );

        let loaded =
            load_catalog_replay_inputs(&reader, &mut catalog, &plan).expect("load catalog");
        let seqs: Vec<_> = loaded.entries.iter().map(|entry| entry.seq).collect();

        assert_eq!(seqs, vec![1, 2]);
        assert_eq!(loaded.catalog_slices[0].records, vec![record]);
        assert_eq!(catalog.load_plans.len(), 1);
    }

    #[rstest]
    fn unbounded_catalog_selector_rejects_empty_event_scan() {
        let reader = reader_with_entries("run-empty", &[]);
        let mut catalog = FakeReplayCatalog::new(CatalogSliceCoverage::default(), Vec::new());

        let err = plan_catalog_replay_inputs(
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

        let err = plan_catalog_replay_inputs(
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

        assert!(plan.catalog_slices.is_empty());
        assert_eq!(loaded.entries.len(), 1);
        assert!(loaded.catalog_slices.is_empty());
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
    fn default_capture_payload_types_are_classified_for_cache_replay() {
        let mut classified = AHashSet::new();
        let mut overlap = Vec::new();

        for payload_type in CACHE_REPLAY_CAPTURE_PAYLOAD_TYPES {
            classified.insert(*payload_type);
        }

        for payload_type in FORENSIC_ONLY_CAPTURE_PAYLOAD_TYPES {
            if !classified.insert(*payload_type) {
                overlap.push(*payload_type);
            }
        }

        let mut seen_defaults = AHashSet::new();
        let duplicate_defaults: Vec<_> = DEFAULT_CAPTURE_PAYLOAD_TYPES
            .iter()
            .copied()
            .filter(|payload_type| !seen_defaults.insert(*payload_type))
            .collect();
        let unclassified: Vec<_> = DEFAULT_CAPTURE_PAYLOAD_TYPES
            .iter()
            .copied()
            .filter(|payload_type| !classified.contains(payload_type))
            .collect();
        let extra: Vec<_> = classified
            .iter()
            .copied()
            .filter(|payload_type| !seen_defaults.contains(payload_type))
            .collect();

        assert!(
            duplicate_defaults.is_empty(),
            "default capture payload types must be unique: {duplicate_defaults:?}",
        );
        assert!(
            overlap.is_empty(),
            "cache replay and forensic-only classes must not overlap: {overlap:?}",
        );
        assert!(
            unclassified.is_empty(),
            "default capture payload types must be cache replayed or forensic-only: {unclassified:?}",
        );
        assert!(
            extra.is_empty(),
            "cache replay classification must not list uncaptured payload types: {extra:?}",
        );
    }

    #[rstest]
    fn cache_replay_capture_payload_types_have_replay_rules() {
        for payload_type in CACHE_REPLAY_CAPTURE_PAYLOAD_TYPES {
            let entry = append_payload(1, payload_type, Bytes::from_static(&[0xc1])).entry;
            let mut cache = Cache::default();

            let err = apply_cache_replay_entry(&mut cache, &entry)
                .expect_err("cache replay payload type must have a decode rule");

            match err {
                CacheReplayError::Decode {
                    payload_type: actual,
                    ..
                } => {
                    assert_eq!(actual, *payload_type);
                }
                other => panic!("expected Decode for {payload_type}, was {other:?}"),
            }
        }
    }

    #[rstest]
    fn forensic_only_capture_payload_types_are_not_cache_replayed() {
        for payload_type in FORENSIC_ONLY_CAPTURE_PAYLOAD_TYPES {
            let entry = append_payload(1, payload_type, Bytes::from_static(&[0xc1])).entry;
            let mut cache = Cache::default();

            let applied = apply_cache_replay_entry(&mut cache, &entry)
                .expect("forensic-only payload type must not be decoded by cache replay");

            assert!(
                !applied,
                "forensic-only payload type must be ignored by cache replay: {payload_type}",
            );
        }
    }

    #[rstest]
    fn cache_public_mutators_have_recovery_classification() {
        let mut classified = AHashSet::new();
        let mut duplicates = Vec::new();

        for row in CACHE_MUTATION_COVERAGE {
            if !classified.insert(row.method) {
                duplicates.push(row.method);
            }
        }

        for method in CACHE_MUTATION_EXCLUSIONS {
            if !classified.insert(*method) {
                duplicates.push(*method);
            }
        }

        let public_methods = cache_public_methods();
        let mutable_methods = cache_public_mutable_methods();
        let missing = sorted_missing_methods(&mutable_methods, &classified);
        let stale = sorted_stale_methods(&classified, &public_methods);

        assert!(
            duplicates.is_empty(),
            "cache mutation recovery classifications must be unique: {duplicates:?}",
        );
        assert!(
            missing.is_empty(),
            "public Cache mutators must be classified for recovery: {missing:?}",
        );
        assert!(
            stale.is_empty(),
            "cache mutation recovery classifications reference missing methods: {stale:?}",
        );
    }

    #[rstest]
    fn cache_mutation_replay_classification_matches_payload_buckets() {
        for row in CACHE_MUTATION_COVERAGE {
            match row.class {
                CacheMutationRecoveryClass::EventStoreCapturedAndReplayed => {
                    assert!(
                        !row.payload_types.is_empty(),
                        "cache-replayed mutation must cite captured payloads: {}",
                        row.method,
                    );

                    for payload_type in row.payload_types {
                        assert!(
                            CACHE_REPLAY_CAPTURE_PAYLOAD_TYPES.contains(payload_type),
                            "cache mutation {} cites non-replayed payload {payload_type}",
                            row.method,
                        );
                    }
                }
                CacheMutationRecoveryClass::ForensicOnly => {
                    assert!(
                        !row.payload_types.is_empty(),
                        "forensic-only mutation must cite forensic payloads: {}",
                        row.method,
                    );

                    for payload_type in row.payload_types {
                        assert!(
                            FORENSIC_ONLY_CAPTURE_PAYLOAD_TYPES.contains(payload_type),
                            "cache mutation {} cites non-forensic payload {payload_type}",
                            row.method,
                        );
                    }
                }
                CacheMutationRecoveryClass::SnapshotOwned
                | CacheMutationRecoveryClass::MissingLiveRecovery => {
                    assert!(
                        row.payload_types.is_empty(),
                        "non-event-store cache mutation {} should not cite payloads",
                        row.method,
                    );
                }
            }
        }
    }

    #[rstest]
    fn submit_order_list_replay_restores_order_list() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let instrument_id = instrument.id();
        let first_init = OrderInitializedSpec::builder()
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-LIST-001"))
            .build();
        let second_init = OrderInitializedSpec::builder()
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-LIST-002"))
            .build();
        let order_list = OrderList::new(
            OrderListId::from("OL-001"),
            instrument_id,
            first_init.strategy_id,
            vec![first_init.client_order_id, second_init.client_order_id],
            UnixNanos::from(1),
        );
        let command = SubmitOrderList::new(
            first_init.trader_id,
            Some(ClientId::from("SIM")),
            first_init.strategy_id,
            order_list.clone(),
            vec![first_init, second_init],
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(2),
            None,
        );
        let entry = append_serde_payload(1, PAYLOAD_TYPE_SUBMIT_ORDER_LIST, &command).entry;
        let mut cache = Cache::default();

        let applied = apply_cache_replay_entry(&mut cache, &entry).expect("apply order list");
        let replayed = cache
            .order_list(&order_list.id)
            .expect("order list replayed");

        assert!(applied);
        assert_eq!(replayed, &order_list);
    }

    #[rstest]
    fn data_response_replay_restores_instruments_and_market_data() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let instrument_id = instrument.id();
        let client_id = ClientId::from("DATA");
        let quote = QuoteTick::new(
            instrument_id,
            Price::from("1.00000"),
            Price::from("1.00010"),
            Quantity::from("100000"),
            Quantity::from("100000"),
            UnixNanos::from(10),
            UnixNanos::from(11),
        );
        let trade = TradeTick::new(
            instrument_id,
            Price::from("1.00005"),
            Quantity::from("50000"),
            AggressorSide::Buy,
            TradeId::from("T-DATA-001"),
            UnixNanos::from(12),
            UnixNanos::from(13),
        );
        let funding_rate = FundingRateUpdate::new(
            instrument_id,
            "0.0001".parse().expect("funding rate"),
            Some(480),
            Some(UnixNanos::from(60)),
            UnixNanos::from(14),
            UnixNanos::from(15),
        );
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let bar = Bar::new(
            bar_type,
            Price::from("1.00000"),
            Price::from("1.00020"),
            Price::from("0.99990"),
            Price::from("1.00010"),
            Quantity::from("150000"),
            UnixNanos::from(16),
            UnixNanos::from(17),
        );
        let reader = reader_with_entries(
            "run-data-response-replay",
            &[
                append_serde_payload(
                    1,
                    PAYLOAD_TYPE_INSTRUMENT_RESPONSE,
                    &InstrumentResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id,
                        instrument.clone(),
                        None,
                        None,
                        UnixNanos::from(1),
                        None,
                    ),
                ),
                append_serde_payload(
                    2,
                    PAYLOAD_TYPE_INSTRUMENTS_RESPONSE,
                    &InstrumentsResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id.venue,
                        vec![instrument],
                        None,
                        None,
                        UnixNanos::from(2),
                        None,
                    ),
                ),
                append_serde_payload(
                    3,
                    PAYLOAD_TYPE_QUOTES_RESPONSE,
                    &QuotesResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id,
                        vec![quote],
                        None,
                        None,
                        UnixNanos::from(3),
                        None,
                    ),
                ),
                append_serde_payload(
                    4,
                    PAYLOAD_TYPE_TRADES_RESPONSE,
                    &TradesResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id,
                        vec![trade],
                        None,
                        None,
                        UnixNanos::from(4),
                        None,
                    ),
                ),
                append_serde_payload(
                    5,
                    PAYLOAD_TYPE_FUNDING_RATES_RESPONSE,
                    &FundingRatesResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id,
                        vec![funding_rate],
                        None,
                        None,
                        UnixNanos::from(5),
                        None,
                    ),
                ),
                append_serde_payload(
                    6,
                    PAYLOAD_TYPE_BARS_RESPONSE,
                    &BarsResponse::new(
                        UUID4::new(),
                        client_id,
                        bar_type,
                        vec![bar],
                        None,
                        None,
                        UnixNanos::from(6),
                        None,
                    ),
                ),
            ],
        );
        let mut cache = Cache::default();

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");

        assert_eq!(report.applied_entries, 6);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(
            cache.instrument(&instrument_id).map(Instrument::id),
            Some(instrument_id)
        );
        assert_eq!(cache.quotes(&instrument_id), Some(vec![quote]));
        assert_eq!(cache.trades(&instrument_id), Some(vec![trade]));
        assert_eq!(
            cache.funding_rates(&instrument_id),
            Some(vec![funding_rate])
        );
        assert_eq!(cache.bars(&bar_type), Some(vec![bar]));
    }

    #[rstest]
    fn empty_data_response_replay_is_noop() {
        let instrument_id = InstrumentAny::CurrencyPair(audusd_sim()).id();
        let client_id = ClientId::from("DATA");
        let bar_type = BarType::new(
            instrument_id,
            BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
            AggregationSource::External,
        );
        let reader = reader_with_entries(
            "run-empty-data-response-replay",
            &[
                append_serde_payload(
                    1,
                    PAYLOAD_TYPE_INSTRUMENTS_RESPONSE,
                    &InstrumentsResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id.venue,
                        Vec::new(),
                        None,
                        None,
                        UnixNanos::from(1),
                        None,
                    ),
                ),
                append_serde_payload(
                    2,
                    PAYLOAD_TYPE_QUOTES_RESPONSE,
                    &QuotesResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id,
                        Vec::new(),
                        None,
                        None,
                        UnixNanos::from(2),
                        None,
                    ),
                ),
                append_serde_payload(
                    3,
                    PAYLOAD_TYPE_TRADES_RESPONSE,
                    &TradesResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id,
                        Vec::new(),
                        None,
                        None,
                        UnixNanos::from(3),
                        None,
                    ),
                ),
                append_serde_payload(
                    4,
                    PAYLOAD_TYPE_FUNDING_RATES_RESPONSE,
                    &FundingRatesResponse::new(
                        UUID4::new(),
                        client_id,
                        instrument_id,
                        Vec::new(),
                        None,
                        None,
                        UnixNanos::from(4),
                        None,
                    ),
                ),
                append_serde_payload(
                    5,
                    PAYLOAD_TYPE_BARS_RESPONSE,
                    &BarsResponse::new(
                        UUID4::new(),
                        client_id,
                        bar_type,
                        Vec::new(),
                        None,
                        None,
                        UnixNanos::from(5),
                        None,
                    ),
                ),
            ],
        );
        let mut cache = Cache::default();

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");

        assert_eq!(report.applied_entries, 5);
        assert_eq!(report.ignored_entries, 0);
        assert!(cache.instrument(&instrument_id).is_none());
        assert_eq!(cache.quotes(&instrument_id), None);
        assert_eq!(cache.trades(&instrument_id), None);
        assert_eq!(cache.funding_rates(&instrument_id), None);
        assert_eq!(cache.bars(&bar_type), None);
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
        let filled_event = OrderEventAny::Filled(filled.clone());
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
        assert_eq!(position.last_event(), Some(filled.clone()));
        assert_eq!(position.trade_ids(), vec![filled.trade_id]);
        assert_eq!(position.commissions(), vec![Money::from("1 USD")]);
    }

    #[rstest]
    fn orderless_leg_fill_replay_creates_position_without_order_mapping() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let client_order_id = ClientOrderId::from("SPREAD-LEG-AUDUSD");
        let position_id = PositionId::from("P-ORDERLESS-LEG");
        let filled = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-AUDUSD"))
            .position_id(position_id)
            .commission(Money::from("1 USD"))
            .build();
        let reader = reader_with_entries(
            "run-orderless-leg-fill-replay",
            &[append_order_event(
                1,
                &OrderEventAny::Filled(filled.clone()),
            )],
        );
        let mut cache = Cache::default();
        cache.add_instrument(instrument).expect("add instrument");

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let position = cache
            .position_owned(&position_id)
            .expect("orderless leg position replayed");

        assert_eq!(report.applied_entries, 1);
        assert_eq!(report.ignored_entries, 0);
        assert!(cache.order_owned(&client_order_id).is_none());
        assert_eq!(cache.position_id(&client_order_id), None);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.last_event(), Some(filled.clone()));
        assert_eq!(position.trade_ids(), vec![filled.trade_id]);
        assert_eq!(position.commissions(), vec![Money::from("1 USD")]);
        assert!(cache.check_integrity());
    }

    #[rstest]
    fn orderless_netting_reopen_replay_does_not_treat_closed_position_as_flip() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let client_order_id = ClientOrderId::from("SPREAD-LEG-AUDUSD");
        let position_id = PositionId::from("P-ORDERLESS-NETTING");
        let opening_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-1"))
            .trade_id(TradeId::from("T-SPREAD-LEG-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .position_id(position_id)
            .build();
        let closing_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-2"))
            .trade_id(TradeId::from("T-SPREAD-LEG-2"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(1))
            .position_id(position_id)
            .build();
        let mut closed_position = Position::new(&instrument, opening_fill);
        closed_position.apply(&closing_fill);
        assert!(closed_position.is_closed());

        let reopening_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-3"))
            .trade_id(TradeId::from("T-SPREAD-LEG-3"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(1))
            .position_id(position_id)
            .build();
        let mut reopened_position = closed_position.clone();
        reopened_position.apply(&reopening_fill);
        let reopened = PositionOpened::create(
            &reopened_position,
            &reopening_fill,
            UUID4::new(),
            reopening_fill.ts_init,
        );
        let reader = reader_with_entries(
            "run-orderless-netting-reopen-replay",
            &[
                append_order_event(1, &OrderEventAny::Filled(reopening_fill.clone())),
                append_position_event(2, &PositionEvent::PositionOpened(reopened)),
            ],
        );
        let mut cache = Cache::default();
        cache.add_instrument(instrument).expect("add instrument");
        cache
            .add_position_without_order(&closed_position, OmsType::Netting)
            .expect("seed closed orderless position");

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let position = cache
            .position_owned(&position_id)
            .expect("netting position reopened");

        assert_eq!(report.applied_entries, 2);
        assert_eq!(report.ignored_entries, 0);
        assert!(position.is_open());
        assert_eq!(position.side, PositionSide::Short);
        assert_eq!(position.entry, OrderSide::Sell);
        assert_eq!(position.quantity, Quantity::from(1));
        assert_eq!(position.opening_order_id, client_order_id);
        assert_eq!(position.closing_order_id, None);
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.trade_ids(), vec![reopening_fill.trade_id]);
        assert_eq!(position.last_event(), Some(reopening_fill));
        assert_eq!(cache.oms_type(&position_id), Some(OmsType::Netting));
        assert!(cache.orders_for_position(&position_id).is_empty());
        assert_eq!(cache.position_id(&client_order_id), None);
        assert!(cache.check_integrity());
    }

    #[rstest]
    fn orderless_hedging_flip_replay_recreates_replacement_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let client_order_id = ClientOrderId::from("SPREAD-LEG-AUDUSD");
        let first_position_id = PositionId::from("P-ORDERLESS-LEG-1");
        let replacement_position_id = PositionId::from("P-ORDERLESS-LEG-2");
        let opening_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-1"))
            .trade_id(TradeId::from("T-SPREAD-LEG-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("1.00000"))
            .position_id(first_position_id)
            .commission(Money::from("1 USD"))
            .build();
        let first_position = Position::new(&instrument, opening_fill.clone());
        let first_opened = PositionOpened::create(
            &first_position,
            &opening_fill,
            UUID4::new(),
            opening_fill.ts_init,
        );

        let flip_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-2"))
            .trade_id(TradeId::from("T-SPREAD-LEG-2"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(2))
            .last_px(Price::from("1.10000"))
            .position_id(first_position_id)
            .commission(Money::from("2 USD"))
            .build();
        let mut closing_fragment = flip_fill.clone();
        closing_fragment.last_qty = Quantity::from(1);
        closing_fragment.commission = Some(Money::from("1 USD"));
        let mut closed_position = first_position;
        closed_position.apply(&closing_fragment);
        let first_closed = PositionClosed::create(
            &closed_position,
            &closing_fragment,
            UUID4::new(),
            flip_fill.ts_init,
        );

        let mut opening_fragment = flip_fill.clone();
        opening_fragment.last_qty = Quantity::from(1);
        opening_fragment.position_id = Some(replacement_position_id);
        opening_fragment.commission = Some(Money::from("1 USD"));
        opening_fragment.event_id = UUID4::new();
        opening_fragment.causation_id = Some(flip_fill.event_id);
        let mut replacement_position = Position::new(&instrument, opening_fragment.clone());
        let replacement_opened = PositionOpened::create(
            &replacement_position,
            &opening_fragment,
            UUID4::new(),
            opening_fragment.ts_init,
        );

        let subsequent_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-3"))
            .trade_id(TradeId::from("T-SPREAD-LEG-3"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(1))
            .last_px(Price::from("1.20000"))
            .position_id(replacement_position_id)
            .commission(Money::from("1 USD"))
            .build();
        replacement_position.apply(&subsequent_fill);
        let replacement_changed = PositionChanged::create(
            &replacement_position,
            &subsequent_fill,
            UUID4::new(),
            subsequent_fill.ts_init,
        );
        let reader = reader_with_entries(
            "run-orderless-hedging-flip-replay",
            &[
                append_order_event(1, &OrderEventAny::Filled(opening_fill)),
                append_position_event(2, &PositionEvent::PositionOpened(first_opened)),
                append_order_event(3, &OrderEventAny::Filled(flip_fill.clone())),
                append_position_event(4, &PositionEvent::PositionClosed(first_closed)),
                append_position_event(5, &PositionEvent::PositionOpened(replacement_opened)),
                append_order_event(6, &OrderEventAny::Filled(subsequent_fill.clone())),
                append_position_event(7, &PositionEvent::PositionChanged(replacement_changed)),
            ],
        );
        let mut cache = Cache::default();
        cache.add_instrument(instrument).expect("add instrument");

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");

        assert_eq!(report.applied_entries, 7);
        assert_eq!(report.ignored_entries, 0);
        let closed = cache
            .position_owned(&first_position_id)
            .expect("closed predecessor replayed");
        assert!(closed.is_closed());
        assert_eq!(closed.event_count(), 2);
        let closing_fragments = closed.fill_fragments(client_order_id, flip_fill.trade_id);
        assert_eq!(closing_fragments.len(), 1);
        assert_eq!(closing_fragments[0].last_qty, Quantity::from(1));
        assert_eq!(closing_fragments[0].commission, Some(Money::from("1 USD")));
        assert_eq!(closing_fragments[0].event_id, flip_fill.event_id);

        let replacement = cache
            .position_owned(&replacement_position_id)
            .expect("open replacement replayed");
        assert!(replacement.is_open());
        assert_eq!(replacement.side, PositionSide::Short);
        assert_eq!(replacement.quantity, Quantity::from(2));
        assert_eq!(replacement.event_count(), 2);
        assert_eq!(
            cache.oms_type(&replacement_position_id),
            Some(OmsType::Hedging)
        );
        assert!(replacement.trade_ids().contains(&flip_fill.trade_id));
        assert!(replacement.trade_ids().contains(&subsequent_fill.trade_id));
        let opening_fragments = replacement.fill_fragments(client_order_id, flip_fill.trade_id);
        assert_eq!(opening_fragments.len(), 1);
        assert_eq!(opening_fragments[0].last_qty, Quantity::from(1));
        assert_eq!(opening_fragments[0].commission, Some(Money::from("1 USD")));
        assert_eq!(opening_fragments[0].causation_id, Some(flip_fill.event_id));

        assert!(cache.orders_for_position(&first_position_id).is_empty());
        assert!(
            cache
                .orders_for_position(&replacement_position_id)
                .is_empty()
        );
        assert_eq!(cache.position_id(&client_order_id), None);
        assert!(cache.check_integrity());
    }

    #[rstest]
    fn single_entry_orderless_flip_is_rejected_before_mutating_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-ORDERLESS-SINGLE-ENTRY");
        let opening_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("SPREAD-LEG-SINGLE"))
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-SINGLE-1"))
            .trade_id(TradeId::from("T-SPREAD-LEG-SINGLE-1"))
            .order_side(OrderSide::Buy)
            .last_qty(Quantity::from(1))
            .position_id(position_id)
            .build();
        let original = Position::new(&instrument, opening_fill.clone());
        let flip_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(opening_fill.client_order_id)
            .venue_order_id(VenueOrderId::from("V-SPREAD-LEG-SINGLE-2"))
            .trade_id(TradeId::from("T-SPREAD-LEG-SINGLE-2"))
            .order_side(OrderSide::Sell)
            .last_qty(Quantity::from(2))
            .position_id(position_id)
            .build();
        let entry = append_order_event(1, &OrderEventAny::Filled(flip_fill)).entry;
        let mut cache = Cache::default();
        cache
            .add_instrument(instrument)
            .expect("add replay instrument");
        cache
            .add_position_without_order(&original, OmsType::Hedging)
            .expect("seed orderless position");

        let error = apply_cache_replay_entry(&mut cache, &entry)
            .expect_err("single-entry API cannot defer the opening fragment");
        let after = cache
            .position_owned(&position_id)
            .expect("position retained");

        assert!(error.to_string().contains("snapshot-tail replay context"));
        assert_eq!(after.side, original.side);
        assert_eq!(after.quantity, original.quantity);
        assert_eq!(after.event_count(), original.event_count());
        assert_eq!(after.trade_ids(), original.trade_ids());
    }

    #[rstest]
    fn order_fill_replay_without_instrument_counts_fill_as_ignored() {
        // The position side cannot open without the instrument; the fill must count
        // as ignored rather than claim a full apply.
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-NO-INSTR");
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
            .build();
        let reader = reader_with_entries(
            "run-fill-no-instrument",
            &[
                append_order_event(1, &OrderEventAny::Initialized(initialized)),
                append_order_event(2, &OrderEventAny::Submitted(submitted)),
                append_order_event(3, &OrderEventAny::Accepted(accepted)),
                append_order_event(4, &OrderEventAny::Filled(filled)),
            ],
        );
        let mut cache = Cache::default();

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");

        assert_eq!(report.applied_entries, 3);
        assert_eq!(report.ignored_entries, 1);
        assert!(cache.position_owned(&position_id).is_none());
    }

    #[rstest]
    fn order_fill_void_replay_updates_order_and_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-VOID-001");
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
        let fill_voided = OrderFillVoidedSpec::builder()
            .trader_id(filled.trader_id)
            .strategy_id(filled.strategy_id)
            .instrument_id(filled.instrument_id)
            .client_order_id(filled.client_order_id)
            .venue_order_id(filled.venue_order_id)
            .account_id(filled.account_id)
            .trade_id(filled.trade_id)
            .voided_qty(Quantity::from(50_000))
            .commission_voided(Money::from("0.40 USD"))
            .order_side(filled.order_side)
            .order_type(filled.order_type)
            .last_px(filled.last_px)
            .currency(filled.currency)
            .liquidity_side(filled.liquidity_side)
            .position_id(position_id)
            .is_reopened(true)
            .build();
        let reader = reader_with_entries(
            "run-fill-void-replay",
            &[
                append_order_event(1, &OrderEventAny::Initialized(initialized)),
                append_order_event(2, &OrderEventAny::Submitted(submitted)),
                append_order_event(3, &OrderEventAny::Accepted(accepted)),
                append_order_event(4, &OrderEventAny::Filled(filled)),
                append_order_event(5, &OrderEventAny::FillVoided(fill_voided.clone())),
            ],
        );
        let mut cache = Cache::default();
        cache.add_instrument(instrument).expect("add instrument");

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let order = cache.order_owned(&client_order_id).expect("order replayed");
        let position = cache
            .position_owned(&position_id)
            .expect("position replayed");

        assert_eq!(report.applied_entries, 5);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_qty(), Quantity::from(50_000));
        assert_eq!(order.voided_qty(), Quantity::from(50_000));
        assert_eq!(position.quantity, Quantity::from(50_000));
        assert_eq!(position.commissions(), vec![Money::from("0.60 USD")]);
        assert_eq!(position.fill_voids.len(), 1);
        assert_eq!(position.fill_voids[0].event, fill_voided);
    }

    #[rstest]
    fn order_fill_void_replay_updates_order_without_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
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
            .build();
        let fill_voided = OrderFillVoidedSpec::builder()
            .trader_id(filled.trader_id)
            .strategy_id(filled.strategy_id)
            .instrument_id(filled.instrument_id)
            .client_order_id(filled.client_order_id)
            .venue_order_id(filled.venue_order_id)
            .account_id(filled.account_id)
            .trade_id(filled.trade_id)
            .voided_qty(Quantity::from(50_000))
            .order_side(filled.order_side)
            .order_type(filled.order_type)
            .last_px(filled.last_px)
            .currency(filled.currency)
            .liquidity_side(filled.liquidity_side)
            .is_reopened(true)
            .build();
        let reader = reader_with_entries(
            "run-order-only-fill-void-replay",
            &[
                append_order_event(1, &OrderEventAny::Initialized(initialized)),
                append_order_event(2, &OrderEventAny::Submitted(submitted)),
                append_order_event(3, &OrderEventAny::Accepted(accepted)),
                append_order_event(4, &OrderEventAny::Filled(filled)),
                append_order_event(5, &OrderEventAny::FillVoided(fill_voided)),
            ],
        );
        let mut cache = Cache::default();
        cache.add_instrument(instrument).expect("add instrument");

        let report = replay_cache_snapshot_tail(&mut cache, &reader).expect("replay");
        let order = cache.order_owned(&client_order_id).expect("order replayed");

        assert_eq!(report.applied_entries, 5);
        assert_eq!(report.ignored_entries, 0);
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_qty(), Quantity::from(50_000));
        assert_eq!(order.voided_qty(), Quantity::from(50_000));
        assert_eq!(cache.positions_total_count(None, None, None, None, None), 0);
    }

    #[rstest]
    fn position_lifecycle_replay_updates_existing_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let opened_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-OPEN"))
            .venue_order_id(VenueOrderId::from("V-OPEN"))
            .trade_id(TradeId::from("T-OPEN"))
            .position_id(position_id)
            .last_qty(Quantity::from("1"))
            .last_px(Price::from("1.00000"))
            .build();
        let mut live_position = Position::new(&instrument, opened_fill.clone());
        let opened = PositionOpened::create(
            &live_position,
            &opened_fill,
            UUID4::new(),
            UnixNanos::from(10),
        );

        let changed_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CHANGE"))
            .venue_order_id(VenueOrderId::from("V-CHANGE"))
            .trade_id(TradeId::from("T-CHANGE"))
            .position_id(position_id)
            .last_qty(Quantity::from("2"))
            .last_px(Price::from("1.10000"))
            .build();
        live_position.apply(&changed_fill);
        let changed = PositionChanged::create(
            &live_position,
            &changed_fill,
            UUID4::new(),
            UnixNanos::from(20),
        );

        let closed_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .client_order_id(ClientOrderId::from("O-CLOSE"))
            .venue_order_id(VenueOrderId::from("V-CLOSE"))
            .trade_id(TradeId::from("T-CLOSE"))
            .order_side(OrderSide::Sell)
            .position_id(position_id)
            .last_qty(Quantity::from("3"))
            .last_px(Price::from("1.20000"))
            .build();
        live_position.apply(&closed_fill);
        let closed = PositionClosed::create(
            &live_position,
            &closed_fill,
            UUID4::new(),
            UnixNanos::from(30),
        );

        let mut stale_position = Position::new(&instrument, opened_fill);
        stale_position.signed_qty = 9.0;
        stale_position.quantity = Quantity::from("9");
        let mut cache = Cache::default();
        cache
            .add_position(&stale_position, OmsType::Unspecified)
            .expect("seed stale position");

        let opened_entry =
            append_position_event(1, &PositionEvent::PositionOpened(opened.clone())).entry;
        let changed_entry =
            append_position_event(2, &PositionEvent::PositionChanged(changed.clone())).entry;
        let closed_entry =
            append_position_event(3, &PositionEvent::PositionClosed(closed.clone())).entry;

        assert!(apply_cache_replay_entry(&mut cache, &opened_entry).expect("apply opened"));
        let replayed = cache
            .position_owned(&position_id)
            .expect("position after opened");
        assert_eq!(replayed.signed_qty.to_bits(), opened.signed_qty.to_bits());
        assert_eq!(replayed.quantity, opened.quantity);
        assert_eq!(replayed.ts_last, opened.ts_event);

        assert!(apply_cache_replay_entry(&mut cache, &changed_entry).expect("apply changed"));
        let replayed = cache
            .position_owned(&position_id)
            .expect("position after changed");
        assert_eq!(replayed.signed_qty.to_bits(), changed.signed_qty.to_bits());
        assert_eq!(replayed.quantity, changed.quantity);
        assert_eq!(replayed.peak_qty, changed.peak_quantity);
        assert_eq!(
            replayed.avg_px_open.to_bits(),
            changed.avg_px_open.to_bits()
        );
        assert!(replayed.is_open());

        assert!(apply_cache_replay_entry(&mut cache, &closed_entry).expect("apply closed"));
        let replayed = cache
            .position_owned(&position_id)
            .expect("position after closed");
        assert_eq!(replayed.signed_qty.to_bits(), closed.signed_qty.to_bits());
        assert_eq!(replayed.quantity, closed.quantity);
        assert_eq!(replayed.closing_order_id, closed.closing_order_id);
        assert_eq!(replayed.duration_ns, closed.duration);
        assert!(replayed.is_closed());
        assert!(cache.is_position_closed(&position_id));
    }

    #[rstest]
    fn position_opened_replay_replaces_realized_pnl() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .commission(Money::from("1 USD"))
            .build();
        let position = Position::new(&instrument, fill.clone());
        let mut opened =
            PositionOpened::create(&position, &fill, UUID4::new(), UnixNanos::from(10));
        assert_eq!(opened.realized_pnl, Some(Money::from("-1 USD")));

        let mut stale_position = position;
        stale_position.realized_pnl = Some(Money::from("9 USD"));
        let mut cache = Cache::default();
        cache
            .add_position(&stale_position, OmsType::Unspecified)
            .expect("seed stale position");
        let entry = append_position_event(1, &PositionEvent::PositionOpened(opened.clone())).entry;

        assert!(apply_cache_replay_entry(&mut cache, &entry).expect("apply opened"));
        assert_eq!(
            cache
                .position_owned(&position_id)
                .expect("position after opened")
                .realized_pnl,
            Some(Money::from("-1 USD")),
        );

        opened.realized_pnl = None;
        let entry = append_position_event(2, &PositionEvent::PositionOpened(opened)).entry;

        assert!(apply_cache_replay_entry(&mut cache, &entry).expect("apply opened without PnL"));
        assert_eq!(
            cache
                .position_owned(&position_id)
                .expect("position after opened without PnL")
                .realized_pnl,
            None,
        );
    }

    #[rstest]
    fn position_adjustment_replay_updates_existing_position() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-001");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .build();
        let position = Position::new(&instrument, fill.clone());
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
    fn position_event_for_unknown_position_is_counted_as_ignored() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-MISSING");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .build();
        let position = Position::new(&instrument, fill.clone());
        let opened = PositionOpened::create(&position, &fill, UUID4::new(), UnixNanos::from(10));
        let entry = append_position_event(1, &PositionEvent::PositionOpened(opened)).entry;
        let mut cache = Cache::default();

        let applied = apply_cache_replay_entry(&mut cache, &entry).expect("apply");

        assert!(
            !applied,
            "missing position must count as ignored, not applied"
        );
    }

    #[rstest]
    fn order_filled_with_no_order_side_is_an_apply_error_not_a_panic() {
        // The entry hash proves the stored bytes match what was written, not that the
        // producer wrote a valid fill; OrderSide::NoOrderSide deserializes cleanly and
        // without the guard panics deep inside Position/Order application.
        let mut fill = OrderFilledSpec::builder()
            .position_id(PositionId::from("P-001"))
            .build();
        fill.order_side = OrderSide::NoOrderSide;
        let entry = append_serde_payload(1, PAYLOAD_TYPE_ORDER_FILLED, &fill).entry;
        let mut cache = Cache::default();

        let err = apply_cache_replay_entry(&mut cache, &entry).expect_err("must reject");

        match err {
            CacheReplayError::Apply { seq, message, .. } => {
                assert_eq!(seq, 1);
                assert!(message.contains("NoOrderSide"), "message was: {message}");
            }
            other => panic!("expected Apply, was {other:?}"),
        }
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
        let position = Position::new(&instrument, fill.clone());
        let entry = append_order_event(1, &OrderEventAny::Filled(fill.clone())).entry;
        let mut cache = Cache::default();
        cache
            .add_position(&position, OmsType::Unspecified)
            .expect("seed position");

        let applied = apply_fill_to_position(&mut cache, &entry, &fill, false).expect("apply fill");
        let position = cache
            .position_owned(&position_id)
            .expect("position updated");

        assert!(
            applied,
            "duplicate trade within an open episode is the idempotent no-op and counts as applied"
        );
        assert_eq!(position.event_count(), 1);
        assert_eq!(position.trade_ids(), vec![fill.trade_id]);
        assert_eq!(position.commissions(), vec![Money::from("1 USD")]);
    }

    #[rstest]
    fn flat_position_with_reused_trade_id_is_ignored_like_live() {
        // Live `Position::apply_fill` ignores a fill whose trade id already sits in
        // the position's carried replay history, so replay must not skip it early or
        // reopen the position either: `apply` ignores it and state matches live.
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-REOPEN");
        let open_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .order_side(OrderSide::Buy)
            .trade_id(TradeId::from("T-1"))
            .commission(Money::from("2 USD"))
            .build();
        let close_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .order_side(OrderSide::Sell)
            .trade_id(TradeId::from("T-2"))
            .build();
        let mut position = Position::new(&instrument, open_fill);
        position.apply(&close_fill);
        assert_eq!(position.side, PositionSide::Flat);

        let dup_fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .order_side(OrderSide::Buy)
            .trade_id(TradeId::from("T-1"))
            .commission(Money::from("1 USD"))
            .build();
        let entry = append_order_event(3, &OrderEventAny::Filled(dup_fill.clone())).entry;
        let mut cache = Cache::default();
        cache
            .add_position(&position, OmsType::Unspecified)
            .expect("seed position");

        let applied = apply_fill_to_position(&mut cache, &entry, &dup_fill, false).expect("apply");
        let position = cache
            .position_owned(&position_id)
            .expect("position updated");

        assert!(
            applied,
            "a historical duplicate is the idempotent no-op and counts as applied"
        );
        assert_eq!(position.side, PositionSide::Flat);
        assert_eq!(position.event_count(), 2);
        assert_eq!(position.trade_ids().len(), 2);
        assert_eq!(position.commissions(), vec![Money::from("2 USD")]);
    }

    #[rstest]
    fn fill_for_missing_instrument_is_counted_as_ignored() {
        let instrument = InstrumentAny::CurrencyPair(audusd_sim());
        let position_id = PositionId::from("P-NO-INSTR");
        let fill = OrderFilledSpec::builder()
            .instrument_id(instrument.id())
            .position_id(position_id)
            .build();
        let entry = append_order_event(1, &OrderEventAny::Filled(fill.clone())).entry;
        let mut cache = Cache::default();

        let applied = apply_fill_to_position(&mut cache, &entry, &fill, false).expect("apply");

        assert!(
            !applied,
            "a position that cannot open must count as ignored, was claimed applied"
        );
        assert!(cache.position_owned(&position_id).is_none());
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
            .snapshot_position_encoded(&position)
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
            .snapshot_position_encoded(&position)
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
