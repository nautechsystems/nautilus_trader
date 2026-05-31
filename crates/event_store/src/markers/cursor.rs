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

//! In-memory per-stream cursor state for the data marker sidecar.

use ahash::AHashMap;
use nautilus_core::UnixNanos;

use crate::{
    entry::Topic,
    markers::{DataClass, DataCursorSnapshot, StreamCursor, StreamDictEntry, StreamSlot},
};

/// In-memory per-stream cursor state for data marker capture.
///
/// Assigns a [`StreamSlot`] lazily on the first observation of a topic, advances each slot's
/// cursor (cumulative `count` and highest `ts_init`) on every message in O(1) with no I/O, and
/// builds a [`DataCursorSnapshot`] of the slots that changed since the previous snapshot.
#[derive(Debug, Default)]
pub struct CursorState {
    slots: AHashMap<Topic, StreamSlot>,
    dict: Vec<StreamDictEntry>,
    cursors: Vec<Cursor>,
    dirty: Vec<bool>,
}

#[derive(Debug, Default)]
struct Cursor {
    ts_init_hi: UnixNanos,
    count: u64,
    last_ts_init: UnixNanos,
    same_ts_count: u32,
}

impl CursorState {
    /// Creates a new empty [`CursorState`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Advances the cursor for `topic`, assigning a slot on its first observation.
    ///
    /// Returns the stream `slot` and the `same_ts_ordinal`: the running ordinal among records
    /// sharing the current `ts_init` within the slot, which resets to zero when `ts_init`
    /// advances (only the high-fidelity path reads it). Bumps the slot's cumulative `count`,
    /// raises its highest `ts_init`, and marks it dirty so the next
    /// [`build_snapshot`](Self::build_snapshot) includes it. A first observation also queues a
    /// [`StreamDictEntry`] for [`take_new_dict_entries`](Self::take_new_dict_entries).
    ///
    /// # Panics
    ///
    /// Panics if the number of distinct streams exceeds `u32::MAX`.
    pub fn advance(
        &mut self,
        topic: Topic,
        data_cls: DataClass,
        identifier: &str,
        ts_init: UnixNanos,
    ) -> (StreamSlot, u32) {
        let slot = if let Some(&slot) = self.slots.get(&topic) {
            slot
        } else {
            let slot = u32::try_from(self.cursors.len()).expect("stream slot count fits u32");
            self.cursors.push(Cursor::default());
            self.dirty.push(false);
            self.dict.push(StreamDictEntry {
                slot,
                data_cls,
                identifier: identifier.to_string(),
            });
            self.slots.insert(topic, slot);
            slot
        };

        let cursor = &mut self.cursors[slot as usize];

        let same_ts_ordinal = if cursor.count == 0 || ts_init != cursor.last_ts_init {
            cursor.last_ts_init = ts_init;
            cursor.same_ts_count = 0;
            0
        } else {
            cursor.same_ts_count += 1;
            cursor.same_ts_count
        };

        cursor.count += 1;
        if ts_init > cursor.ts_init_hi {
            cursor.ts_init_hi = ts_init;
        }
        self.dirty[slot as usize] = true;

        (slot, same_ts_ordinal)
    }

    /// Removes and returns the dict entries queued since the previous call.
    ///
    /// Each [`StreamDictEntry`] is produced once, when its slot is first assigned by
    /// [`advance`](Self::advance).
    pub fn take_new_dict_entries(&mut self) -> Vec<StreamDictEntry> {
        std::mem::take(&mut self.dict)
    }

    /// Builds a snapshot of every slot that advanced since the previous snapshot, clearing the
    /// dirty set.
    ///
    /// Returns `None` when no slot has advanced, so a quiet interval writes nothing.
    pub fn build_snapshot(
        &mut self,
        marker_seq: u64,
        event_seq_before: u64,
        ts_init: UnixNanos,
    ) -> Option<DataCursorSnapshot> {
        let mut advanced = Vec::new();

        for (slot, (cursor, dirty)) in (0_u32..).zip(self.cursors.iter().zip(self.dirty.iter_mut()))
        {
            if *dirty {
                *dirty = false;
                advanced.push(StreamCursor {
                    slot,
                    ts_init_hi: cursor.ts_init_hi,
                    count: cursor.count,
                });
            }
        }

        if advanced.is_empty() {
            None
        } else {
            Some(DataCursorSnapshot {
                marker_seq,
                event_seq_before,
                ts_init,
                advanced,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn advance_assigns_slot_lazily_and_writes_dict() {
        let mut state = CursorState::new();
        let quotes: Topic = "data.quotes.BINANCE.ETHUSDT".into();
        let trades: Topic = "data.trades.BINANCE.BTCUSDT".into();

        let (s0, _) = state.advance(
            quotes,
            DataClass::Quote,
            "ETHUSDT.BINANCE",
            UnixNanos::from(1),
        );
        let (s1, _) = state.advance(
            trades,
            DataClass::Trade,
            "BTCUSDT.BINANCE",
            UnixNanos::from(2),
        );
        // A repeat of the first topic reuses its slot rather than allocating a new one.
        let (s0_again, _) = state.advance(
            quotes,
            DataClass::Quote,
            "ETHUSDT.BINANCE",
            UnixNanos::from(3),
        );

        let dict = state.take_new_dict_entries();

        assert_eq!(s0, 0);
        assert_eq!(s1, 1);
        assert_eq!(s0_again, 0);
        assert_eq!(
            dict,
            vec![
                StreamDictEntry {
                    slot: 0,
                    data_cls: DataClass::Quote,
                    identifier: "ETHUSDT.BINANCE".to_string(),
                },
                StreamDictEntry {
                    slot: 1,
                    data_cls: DataClass::Trade,
                    identifier: "BTCUSDT.BINANCE".to_string(),
                },
            ]
        );
        // Dict entries are produced once; a second drain is empty.
        assert!(state.take_new_dict_entries().is_empty());
    }

    #[rstest]
    fn advance_updates_count_and_ts_hi() {
        let mut state = CursorState::new();
        let quotes: Topic = "data.quotes.BINANCE.ETHUSDT".into();

        let (slot, _) = state.advance(
            quotes,
            DataClass::Quote,
            "ETHUSDT.BINANCE",
            UnixNanos::from(100),
        );
        state.advance(
            quotes,
            DataClass::Quote,
            "ETHUSDT.BINANCE",
            UnixNanos::from(200),
        );
        // A lower ts_init must not lower the high-water mark.
        state.advance(
            quotes,
            DataClass::Quote,
            "ETHUSDT.BINANCE",
            UnixNanos::from(150),
        );

        let cursor = &state.cursors[slot as usize];

        assert_eq!(cursor.count, 3);
        assert_eq!(cursor.ts_init_hi, UnixNanos::from(200));
        assert!(state.dirty[slot as usize]);
    }

    #[rstest]
    fn build_snapshot_includes_only_dirty_and_clears() {
        let mut state = CursorState::new();
        let a: Topic = "data.quotes.BINANCE.A".into();
        let b: Topic = "data.quotes.BINANCE.B".into();
        let c: Topic = "data.quotes.BINANCE.C".into();

        let (sa, _) = state.advance(a, DataClass::Quote, "A.BINANCE", UnixNanos::from(10));
        state.advance(b, DataClass::Quote, "B.BINANCE", UnixNanos::from(20));
        let (sc, _) = state.advance(c, DataClass::Quote, "C.BINANCE", UnixNanos::from(30));

        // The first snapshot carries all three streams and clears the dirty set.
        let first = state
            .build_snapshot(1, 0, UnixNanos::from(30))
            .expect("snapshot");

        // Advance only two of the three streams.
        state.advance(a, DataClass::Quote, "A.BINANCE", UnixNanos::from(40));
        state.advance(c, DataClass::Quote, "C.BINANCE", UnixNanos::from(50));

        let second = state
            .build_snapshot(2, 7, UnixNanos::from(50))
            .expect("snapshot");

        // A follow-up with no new advances writes nothing.
        let third = state.build_snapshot(3, 7, UnixNanos::from(60));

        assert_eq!(first.advanced.len(), 3);
        assert_eq!(second.marker_seq, 2);
        assert_eq!(second.event_seq_before, 7);
        // Only the two advanced streams appear, carrying their cursor's ts_init_hi and count.
        assert_eq!(
            second.advanced,
            vec![
                StreamCursor {
                    slot: sa,
                    ts_init_hi: UnixNanos::from(40),
                    count: 2,
                },
                StreamCursor {
                    slot: sc,
                    ts_init_hi: UnixNanos::from(50),
                    count: 2,
                },
            ]
        );
        assert!(third.is_none());
    }

    // The ordinal counts records within a contiguous run of equal ts_init and resets on any
    // change. Per the design, the Phase 8 verifier owns detection of a decreasing per-slot
    // ts_init, so advance treats a non-monotonic decrease as just another run boundary.
    #[rstest]
    #[case::repeats_then_advance(vec![100, 100, 100, 200, 200], vec![0, 1, 2, 0, 1])]
    #[case::non_monotonic_resets_each_change(vec![100, 200, 100], vec![0, 0, 0])]
    fn same_ts_ordinal_tracks_contiguous_runs(
        #[case] timestamps: Vec<u64>,
        #[case] expected: Vec<u32>,
    ) {
        let mut state = CursorState::new();
        let trades: Topic = "data.trades.BINANCE.ETHUSDT".into();

        let ordinals: Vec<u32> = timestamps
            .into_iter()
            .map(|ts| {
                let (_, ordinal) = state.advance(
                    trades,
                    DataClass::Trade,
                    "ETHUSDT.BINANCE",
                    UnixNanos::from(ts),
                );
                ordinal
            })
            .collect();

        assert_eq!(ordinals, expected);
    }
}
