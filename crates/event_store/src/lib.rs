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
pub mod entry;
pub mod error;
pub mod hash;
pub mod headers;
pub mod manifest;
mod wire;
pub mod writer;

pub use backend::{
    AppendEntry, EventStore, IndexKey, IndexKind, MemoryBackend, RedbBackend, ScanDirection,
};
pub use entry::{EventStoreEntry, PayloadType, Topic};
pub use error::EventStoreError;
pub use hash::{EntryHash, compute_entry_hash};
pub use headers::Headers;
pub use manifest::{RegisteredComponents, RunId, RunManifest, RunStatus};
pub use writer::{
    DEFAULT_CHANNEL_CAPACITY, DEFAULT_HALT_THRESHOLD, DEFAULT_MAX_BATCH_ENTRIES,
    DEFAULT_MAX_BATCH_LATENCY, EntryDraft, EventStoreWriter, HaltCallback, HaltReason, SubmitError,
    WriterConfig, noop_halt,
};
