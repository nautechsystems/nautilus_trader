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

//! Data marker sidecar schema types and canonical content hashes.
//!
//! The sidecar records the observed order of streaming data at the message-bus boundary as
//! compact cursor snapshots, joinable back to catalog rows. No market-data payload is
//! persisted.

pub mod backend;
pub mod cursor;
pub mod marker;
pub mod redb;

pub use backend::{MarkerBackend, MarkerManifest, MemoryMarkerBackend};
pub use cursor::CursorState;
pub use marker::{
    DataClass, DataCursorSnapshot, HiFiMarker, MarkerGap, MarkerGapReason, StreamCursor,
    StreamDictEntry, StreamSlot, compute_dict_hash, compute_gap_hash, compute_hifi_hash,
    compute_marker_hash,
};
pub use redb::RedbMarkerBackend;
