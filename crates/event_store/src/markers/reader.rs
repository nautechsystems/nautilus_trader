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

//! Read-side cursor folding for data marker sidecars.

use std::fmt::Debug;

use ahash::AHashMap;

use crate::{
    error::EventStoreError,
    markers::{MarkerBackend, StreamCursor, StreamDictEntry, StreamSlot},
};

/// Read-side scanner for a single data marker sidecar.
///
/// The reader owns an already-open marker backend. It folds cursor snapshots up to a target
/// `event_seq_before` and resolves stream slots through the durable stream dictionary.
pub struct MarkerReader {
    backend: Box<dyn MarkerBackend>,
}

impl Debug for MarkerReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(MarkerReader))
            .finish_non_exhaustive()
    }
}

impl MarkerReader {
    /// Creates a reader over an already-open marker backend.
    #[must_use]
    pub fn new(backend: Box<dyn MarkerBackend>) -> Self {
        Self { backend }
    }

    /// Folds cursor snapshots whose `event_seq_before` is at or below `event_seq_before`.
    ///
    /// The returned map carries the latest known cursor per stream slot at the target event-store
    /// boundary. Slots that did not advance in later snapshots keep their previous cursor.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when the backend cannot scan cursor snapshots.
    pub fn fold_to(
        &self,
        event_seq_before: u64,
    ) -> Result<AHashMap<StreamSlot, StreamCursor>, EventStoreError> {
        let mut folded = AHashMap::new();

        for snapshot in self.backend.scan_snapshots()? {
            if snapshot.event_seq_before > event_seq_before {
                continue;
            }

            for cursor in snapshot.advanced {
                folded.insert(cursor.slot, cursor);
            }
        }

        Ok(folded)
    }

    /// Scans the durable stream dictionary into a map keyed by stream slot.
    ///
    /// # Errors
    ///
    /// Returns [`EventStoreError`] when the backend cannot scan the dictionary.
    pub fn stream_dictionary(
        &self,
    ) -> Result<AHashMap<StreamSlot, StreamDictEntry>, EventStoreError> {
        Ok(self
            .backend
            .scan_dict()?
            .into_iter()
            .map(|entry| (entry.slot, entry))
            .collect())
    }

    /// Resolves `slot` to its stream dictionary entry.
    ///
    /// Returns `None` when the slot is unknown or the backend cannot scan the dictionary.
    #[must_use]
    pub fn resolve_slot(&self, slot: StreamSlot) -> Option<StreamDictEntry> {
        self.stream_dictionary().ok()?.get(&slot).cloned()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::{
        manifest::RunStatus,
        markers::{
            DataClass, DataCursorSnapshot, MarkerBackend, MarkerManifest, MemoryMarkerBackend,
            StreamCursor, StreamDictEntry, compute_dict_hash, compute_marker_hash,
        },
    };

    fn manifest() -> MarkerManifest {
        MarkerManifest {
            run_id: "1700000000-reader".to_string(),
            enabled_classes: vec![DataClass::Quote, DataClass::Trade],
            high_fidelity: false,
            snapshot_count: 0,
            hifi_count: 0,
            gap_count: 0,
            dict_count: 0,
            status: RunStatus::Running,
        }
    }

    fn snapshot(
        marker_seq: u64,
        event_seq_before: u64,
        advanced: Vec<StreamCursor>,
    ) -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq,
            event_seq_before,
            ts_init: UnixNanos::from(1_700_000_000_000_000_000 + marker_seq),
            advanced,
        }
    }

    fn dict(slot: u32, data_cls: DataClass, identifier: &str) -> StreamDictEntry {
        StreamDictEntry {
            slot,
            data_cls,
            identifier: identifier.to_string(),
        }
    }

    #[rstest]
    fn fold_cursors_to_event_seq() {
        let mut backend = MemoryMarkerBackend::new();
        backend.open_run(manifest()).expect("open run");
        let quote_dict = dict(0, DataClass::Quote, "ETHUSDT.BINANCE");
        let trade_dict = dict(1, DataClass::Trade, "BTCUSDT.BINANCE");
        backend
            .put_dict(&quote_dict, compute_dict_hash(&quote_dict))
            .expect("put quote dict");
        backend
            .put_dict(&trade_dict, compute_dict_hash(&trade_dict))
            .expect("put trade dict");

        let s1 = snapshot(
            1,
            5,
            vec![StreamCursor {
                slot: 0,
                ts_init_hi: UnixNanos::from(100),
                count: 1,
            }],
        );
        let s2 = snapshot(
            2,
            10,
            vec![
                StreamCursor {
                    slot: 0,
                    ts_init_hi: UnixNanos::from(300),
                    count: 3,
                },
                StreamCursor {
                    slot: 1,
                    ts_init_hi: UnixNanos::from(1_000),
                    count: 1,
                },
            ],
        );
        let s3 = snapshot(
            3,
            15,
            vec![StreamCursor {
                slot: 1,
                ts_init_hi: UnixNanos::from(2_000),
                count: 2,
            }],
        );
        backend
            .append_snapshot(&s1, compute_marker_hash(&s1))
            .expect("append s1");
        backend
            .append_snapshot(&s2, compute_marker_hash(&s2))
            .expect("append s2");
        backend
            .append_snapshot(&s3, compute_marker_hash(&s3))
            .expect("append s3");

        let reader = MarkerReader::new(Box::new(backend));
        let folded = reader.fold_to(10).expect("fold");

        assert_eq!(
            folded.get(&0),
            Some(&StreamCursor {
                slot: 0,
                ts_init_hi: UnixNanos::from(300),
                count: 3,
            }),
        );
        assert_eq!(
            folded.get(&1),
            Some(&StreamCursor {
                slot: 1,
                ts_init_hi: UnixNanos::from(1_000),
                count: 1,
            }),
        );
        assert_eq!(reader.resolve_slot(1), Some(trade_dict));
    }
}
