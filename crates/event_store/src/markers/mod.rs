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

//! Data marker sidecar capture, schema types, canonical content hashes, and writer lane.
//!
//! The sidecar records the observed order of streaming data at the message-bus boundary as
//! compact cursor snapshots, joinable back to catalog rows. No market-data payload is
//! persisted.

pub mod backend;
pub mod capture;
pub mod cursor;
pub mod extractor;
pub mod marker;
pub mod reader;
pub mod redb;
pub mod verifier;
pub mod writer;

pub use backend::{MarkerBackend, MarkerManifest, MemoryMarkerBackend, StoredMarkerRecord};
pub use capture::DataMarkerCapture;
pub use cursor::CursorState;
pub use extractor::{DataMarkerExtractor, DataMarkerExtractorRegistry};
pub use marker::{
    DataClass, DataCursorSnapshot, HiFiMarker, MarkerGap, MarkerGapReason, StreamCursor,
    StreamDictEntry, StreamSlot, compute_dict_hash, compute_gap_hash, compute_hifi_hash,
    compute_marker_hash,
};
pub use nautilus_system::event_store::DataMarkerConfig;
pub use reader::MarkerReader;
pub use redb::RedbMarkerBackend;
pub use verifier::{
    MarkerCountKind, MarkerFinding, MarkerRecordKind, MarkerVerifier, MarkerVerifyReport,
};
pub use writer::{
    DEFAULT_MARKER_CHANNEL_CAPACITY, DEFAULT_MARKER_MAX_BATCH, DEFAULT_MARKER_MAX_LATENCY,
    MarkerMsg, MarkerWriter, MarkerWriterConfig,
};
