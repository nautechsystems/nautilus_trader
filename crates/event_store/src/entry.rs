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

//! The captured event store entry.

use bytes::Bytes;
use nautilus_common::msgbus::{self, MStr};
use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{hash::EntryHash, headers::Headers, wire};

/// A bus topic identifier captured for an event store entry.
///
/// Reuses `nautilus_common::msgbus::MStr<Topic>` so captured rows carry the same
/// phantom-typed handle the bus uses; the writer copies the handle rather than the
/// string and the type system rejects accidentally captured patterns or endpoints.
pub type Topic = MStr<msgbus::Topic>;

/// A canonical payload type identifier captured for an event store entry.
///
/// Identifies which encoder produced [`EventStoreEntry::payload`]. The bus capture adapter
/// allow-list maps Rust message types to these tags; the reader uses the tag to dispatch
/// the matching decoder. Unlike [`Topic`] this is not a bus type, so we keep the plain
/// `Ustr` and avoid taking on the bus's `MStr` discipline for a free-form encoder tag.
pub type PayloadType = Ustr;

/// One captured row in the event store: a state-affecting bus message plus metadata.
///
/// The fields cover the SPEC's logical entry contract:
///
/// - `seq` is the per-run monotonic sequence assigned by the writer at commit time. It is
///   the replay-order authority.
/// - `ts_init` is the domain timestamp; strictly monotonic and unique system-wide via the
///   shared `AtomicTime`.
/// - `ts_publish` records when the writer received the entry (and, when populated by the
///   bus, when the bus accepted it for fanout). Neither timestamp orders replay.
/// - `topic` and `payload_type` describe the logical message identity; the writer commits
///   them verbatim.
/// - `payload` carries the canonical bytes produced by the registered encoder.
/// - `headers` carry first-class correlation metadata.
/// - `entry_hash` is the canonical hash recomputed on every read; mismatch quarantines the
///   run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventStoreEntry {
    /// The canonical hash over every preceding field.
    pub entry_hash: EntryHash,
    /// The per-run monotonic sequence; replay-order authority.
    pub seq: u64,
    /// First-class correlation headers.
    pub headers: Headers,
    /// The logical bus topic this entry was captured on.
    pub topic: Topic,
    /// The canonical payload type tag chosen by the bus capture adapter.
    pub payload_type: PayloadType,
    /// The canonical encoded payload bytes.
    pub payload: Bytes,
    /// The domain timestamp from `AtomicTime`.
    #[serde(with = "wire::nanos_as_u64")]
    pub ts_init: UnixNanos,
    /// The bus-accepted or writer-receive timestamp.
    #[serde(with = "wire::nanos_as_u64")]
    pub ts_publish: UnixNanos,
}

impl EventStoreEntry {
    /// Creates a new [`EventStoreEntry`] with all fields supplied by the writer.
    #[must_use]
    #[allow(clippy::too_many_arguments)] // entry envelope fields per SPEC
    pub fn new(
        entry_hash: EntryHash,
        seq: u64,
        headers: Headers,
        topic: Topic,
        payload_type: PayloadType,
        payload: Bytes,
        ts_init: UnixNanos,
        ts_publish: UnixNanos,
    ) -> Self {
        Self {
            entry_hash,
            seq,
            headers,
            topic,
            payload_type,
            payload,
            ts_init,
            ts_publish,
        }
    }

    /// Recomputes the canonical hash for this entry from its current fields.
    ///
    /// Backends call this on every read to validate `entry_hash`; mismatch quarantines the
    /// run.
    #[must_use]
    pub fn recompute_hash(&self) -> EntryHash {
        crate::hash::compute_entry_hash(
            self.seq,
            self.ts_init,
            self.ts_publish,
            self.topic.as_ref(),
            self.payload_type.as_str(),
            &self.payload,
            &self.headers,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::hash::compute_entry_hash;

    fn entry() -> EventStoreEntry {
        let topic: Topic = "exec.command".into();
        let payload_type = Ustr::from("SubmitOrder");
        let payload = Bytes::from_static(b"\x01\x02\x03");
        let headers = Headers::empty();
        let hash = compute_entry_hash(
            1,
            UnixNanos::from(10),
            UnixNanos::from(11),
            topic.as_ref(),
            payload_type.as_str(),
            &payload,
            &headers,
        );
        EventStoreEntry::new(
            hash,
            1,
            headers,
            topic,
            payload_type,
            payload,
            UnixNanos::from(10),
            UnixNanos::from(11),
        )
    }

    #[rstest]
    fn recompute_matches_stored_hash() {
        let e = entry();

        assert_eq!(e.recompute_hash(), e.entry_hash);
    }

    #[rstest]
    fn tampered_payload_breaks_hash_check() {
        let mut e = entry();
        e.payload = Bytes::from_static(b"\x01\x02\x04");

        assert_ne!(e.recompute_hash(), e.entry_hash);
    }
}
