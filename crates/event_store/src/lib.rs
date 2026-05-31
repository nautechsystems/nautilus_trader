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

//! Event store and authoritative log of state-affecting messages for [NautilusTrader](https://nautilustrader.io).
//!
//! The `nautilus-event-store` crate provides an embedded, append-only event store that captures
//! commands, events, venue reports, and correlations flowing across the message bus. Combined with
//! cache snapshots, it provides stable restarts via tail-replay, deterministic incident replay,
//! end-to-end audit of agent decisions, and counterfactual research.
//!
//! See `README.md` for the high-level specification.
//!
//! # NautilusTrader
//!
//! [NautilusTrader](https://nautilustrader.io) is an open-source, production-grade, Rust-native
//! engine for multi-asset, multi-venue trading systems.
//!
//! The system spans research, deterministic simulation, and live execution within a single
//! event-driven architecture, providing research-to-live semantic parity.

#![warn(rustc::all)]
#![warn(clippy::pedantic)]
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(nonstandard_style)]
#![deny(missing_debug_implementations)]
#![deny(clippy::missing_errors_doc)]
#![deny(clippy::missing_panics_doc)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod backend;
pub mod capture;
pub mod entry;
pub mod error;
pub mod hash;
pub mod headers;
pub mod kernel;
pub mod manifest;
pub mod markers;
pub mod reader;
pub mod replay;
pub mod retention;
pub mod snapshot;
pub mod verifier;
pub mod writer;

mod wire;

pub use backend::{
    AppendEntry, EventStore, IndexKey, IndexKind, MemoryBackend, RedbBackend, ScanDirection,
};
pub use capture::{
    BusCaptureAdapter, CaptureError, Encode, EncodeError, EncodedPayload, EncoderRegistry,
    PAYLOAD_TYPE_ACCOUNT_STATE, PAYLOAD_TYPE_FILL_REPORT, PAYLOAD_TYPE_ORDER_FILLED,
    PAYLOAD_TYPE_ORDER_STATUS_REPORT, PAYLOAD_TYPE_POSITION_STATUS_REPORT,
    PAYLOAD_TYPE_SUBMIT_ORDER, TypedEncoder, default_registry, encode_account_state,
    encode_fill_report, encode_order_filled, encode_order_status_report,
    encode_position_status_report, encode_submit_order, register_default,
};
pub use entry::{EventStoreEntry, PayloadType, Topic};
pub use error::EventStoreError;
pub use hash::{EntryHash, compute_entry_hash};
pub use headers::Headers;
pub use kernel::{
    BootError, EventStoreLifecycle, EventStoreLifecycleOptions, EventStoreSession, HaltSignal,
    KernelError, RecoveredRun, RecoveryOutcome, build_run_id, open_run, open_run_with_options,
    recover_predecessors,
};
pub use manifest::{RunId, RunManifest, RunStatus};
pub use markers::{
    CursorState, DEFAULT_MARKER_CHANNEL_CAPACITY, DEFAULT_MARKER_MAX_BATCH,
    DEFAULT_MARKER_MAX_LATENCY, DataClass, DataCursorSnapshot, DataMarkerCapture, DataMarkerConfig,
    DataMarkerExtractor, DataMarkerExtractorRegistry, HiFiMarker, MarkerBackend, MarkerGap,
    MarkerGapReason, MarkerManifest, MarkerMsg, MarkerWriter, MarkerWriterConfig,
    MemoryMarkerBackend, RedbMarkerBackend, StreamCursor, StreamDictEntry, StreamSlot,
    compute_dict_hash, compute_gap_hash, compute_hifi_hash, compute_marker_hash,
};
pub use nautilus_system::{
    RegisteredComponents,
    event_store::{EventStoreConfig, RetentionMode, RunIdentity},
};
pub use reader::{DEFAULT_SCAN_CHUNK_SIZE, EventStoreReader, RangeScan, SnapshotReplayPlan};
#[cfg(feature = "persistence")]
pub use replay::ParquetReplayCatalog;
pub use replay::{
    CacheReplayError, CacheReplayReport, CatalogReplayData, CatalogReplayRecord,
    CatalogReplaySlice, CatalogSliceCoverage, CatalogSlicePlan, CatalogSliceQuery,
    CatalogSliceSelector, EventStoreReplayReport, ReplayCatalog, ReplayInputError, ReplayInputPlan,
    ReplayInputs, ReplaySeqRange, ReplayTimeRange, apply_cache_replay_entry,
    load_catalog_replay_inputs, load_forensics_replay_inputs, open_event_store_replay_source,
    plan_catalog_replay_inputs, plan_forensics_replay_inputs, replay_cache_snapshot_tail,
    restore_cache_from_sealed_run, restore_cache_snapshot_and_replay_tail,
    restore_cache_snapshot_blob, validate_event_store_replay_source,
};
pub use retention::{
    RetentionPlan, RetentionRun, SnapshotAnchorStatus, list_redb_sealed_runs, plan_redb_retention,
    plan_retention,
};
pub use snapshot::{SnapshotAnchor, compute_snapshot_content_hash};
pub use verifier::{
    GapRange, IndexDrift, ManifestField, Verifier, VerifyError, VerifyFinding, VerifyReport,
};
pub use writer::{
    DEFAULT_CHANNEL_CAPACITY, DEFAULT_HALT_THRESHOLD, DEFAULT_MAX_BATCH_ENTRIES,
    DEFAULT_MAX_BATCH_LATENCY, EntryDraft, EventStoreWriter, HaltCallback, HaltReason, SubmitError,
    WriterConfig, noop_halt,
};
