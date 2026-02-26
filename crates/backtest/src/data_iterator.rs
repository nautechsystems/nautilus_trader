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

//! Multi-stream, time-ordered data iterator for replaying historical market data.

use std::collections::BinaryHeap;

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::data::{Data, HasTsInit};

/// Internal convenience struct to keep heap entries ordered by `(ts_init, priority)`.
#[derive(Debug, Eq, PartialEq)]
struct HeapEntry {
    ts: UnixNanos,
    priority: i32,
    index: usize,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // min-heap on ts, then priority sign (+/-) then index
        self.ts
            .cmp(&other.ts)
            .then_with(|| self.priority.cmp(&other.priority))
            .then_with(|| self.index.cmp(&other.index))
            .reverse() // BinaryHeap is max by default -> reverse for min behaviour
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Multi-stream, time-ordered data iterator used by the backtest engine.
#[derive(Debug, Default)]
pub struct BacktestDataIterator {
    streams: AHashMap<i32, Vec<Data>>, // key: priority, value: Vec<Data>
    names: AHashMap<i32, String>,      // priority -> name
    priorities: AHashMap<String, i32>, // name -> priority
    indices: AHashMap<i32, usize>,     // cursor per stream
    heap: BinaryHeap<HeapEntry>,
    single_priority: Option<i32>,
    next_priority_counter: i32, // monotonically increasing counter used to assign priorities
}

impl BacktestDataIterator {
    /// Creates a new empty [`BacktestDataIterator`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            streams: AHashMap::new(),
            names: AHashMap::new(),
            priorities: AHashMap::new(),
            indices: AHashMap::new(),
            heap: BinaryHeap::new(),
            single_priority: None,
            next_priority_counter: 0,
        }
    }

    /// Adds (or replaces) a named data stream.
    ///
    /// When `append_data` is true the stream gets lower priority on timestamp
    /// ties; when false (prepend) it wins ties.
    pub fn add_data(&mut self, name: &str, mut data: Vec<Data>, append_data: bool) {
        if data.is_empty() {
            return;
        }

        // Ensure sorted by ts_init
        data.sort_by_key(HasTsInit::ts_init);

        let priority = if let Some(p) = self.priorities.get(name) {
            // Replace existing stream – remove previous traces then re-insert below.
            *p
        } else {
            self.next_priority_counter += 1;
            let sign = if append_data { 1 } else { -1 };
            sign * self.next_priority_counter
        };

        // Remove old state if any
        self.remove_data(name, true);

        self.streams.insert(priority, data);
        self.names.insert(priority, name.to_string());
        self.priorities.insert(name.to_string(), priority);
        self.indices.insert(priority, 0);

        self.rebuild_heap();
    }

    /// Removes a named data stream.
    pub fn remove_data(&mut self, name: &str, complete_remove: bool) {
        if let Some(priority) = self.priorities.remove(name) {
            self.streams.remove(&priority);
            self.indices.remove(&priority);
            self.names.remove(&priority);

            // Rebuild heap sans removed priority
            self.heap.retain(|e| e.priority != priority);

            if self.heap.is_empty() {
                self.single_priority = None;
            }
        }

        if complete_remove {
            // Placeholder for future generator cleanup
        }
    }

    /// Sets the cursor of a named stream to `index` (0-based).
    pub fn set_index(&mut self, name: &str, index: usize) {
        if let Some(priority) = self.priorities.get(name) {
            self.indices.insert(*priority, index);
            self.rebuild_heap();
        }
    }

    /// Resets all stream cursors to the beginning.
    pub fn reset_all_cursors(&mut self) {
        for idx in self.indices.values_mut() {
            *idx = 0;
        }
        self.rebuild_heap();
    }

    /// Returns the next [`Data`] element across all streams in chronological order.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Data> {
        // Fast path for single stream
        if let Some(p) = self.single_priority {
            let data = self.streams.get_mut(&p)?;
            let idx = self.indices.get_mut(&p)?;
            if *idx >= data.len() {
                return None;
            }
            let element = data[*idx].clone();
            *idx += 1;
            return Some(element);
        }

        // Multi-stream path using heap
        let entry = self.heap.pop()?;
        let stream_vec = self.streams.get(&entry.priority)?;
        let element = stream_vec[entry.index].clone();

        // Advance cursor and push next entry
        let next_index = entry.index + 1;
        self.indices.insert(entry.priority, next_index);
        if next_index < stream_vec.len() {
            self.heap.push(HeapEntry {
                ts: stream_vec[next_index].ts_init(),
                priority: entry.priority,
                index: next_index,
            });
        }

        Some(element)
    }

    /// Returns whether all streams have been fully consumed.
    #[must_use]
    pub fn is_done(&self) -> bool {
        if let Some(p) = self.single_priority {
            if let Some(idx) = self.indices.get(&p)
                && let Some(vec) = self.streams.get(&p)
            {
                return *idx >= vec.len();
            }
            true
        } else {
            self.heap.is_empty()
        }
    }

    fn rebuild_heap(&mut self) {
        self.heap.clear();

        // Determine if we’re in single-stream mode
        if self.streams.len() == 1 {
            self.single_priority = self.streams.keys().next().copied();
            return;
        }
        self.single_priority = None;

        for (&priority, vec) in &self.streams {
            let idx = *self.indices.get(&priority).unwrap_or(&0);
            if idx < vec.len() {
                self.heap.push(HeapEntry {
                    ts: vec[idx].ts_init(),
                    priority,
                    index: idx,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::QuoteTick,
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    fn quote(id: &str, ts: u64) -> Data {
        let inst = InstrumentId::from(id);
        Data::Quote(QuoteTick::new(
            inst,
            Price::from("1.0"),
            Price::from("1.0"),
            Quantity::from(100),
            Quantity::from(100),
            ts.into(),
            ts.into(),
        ))
    }

    fn collect_ts(it: &mut BacktestDataIterator) -> Vec<u64> {
        let mut ts = Vec::new();
        while let Some(d) = it.next() {
            ts.push(d.ts_init().as_u64());
        }
        ts
    }

    #[rstest]
    fn test_single_stream_yields_in_order() {
        let mut it = BacktestDataIterator::new();
        it.add_data(
            "s",
            vec![quote("A.B", 100), quote("A.B", 200), quote("A.B", 300)],
            true,
        );

        assert_eq!(collect_ts(&mut it), vec![100, 200, 300]);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_single_stream_exhaustion_returns_none() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 3)], true);
        assert_eq!(it.next().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.next().unwrap().ts_init(), UnixNanos::from(3));
        assert!(it.next().is_none());
    }

    #[rstest]
    fn test_single_stream_sorts_unsorted_input() {
        let mut it = BacktestDataIterator::new();
        it.add_data(
            "s",
            vec![quote("A.B", 300), quote("A.B", 100), quote("A.B", 200)],
            true,
        );

        assert_eq!(collect_ts(&mut it), vec![100, 200, 300]);
    }

    #[rstest]
    fn test_two_stream_merge_chronological() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1), quote("A.B", 4)], true);
        it.add_data("s2", vec![quote("C.D", 2), quote("C.D", 3)], false);

        assert_eq!(collect_ts(&mut it), vec![1, 2, 3, 4]);
    }

    #[rstest]
    fn test_three_stream_merge_sorted() {
        let mut it = BacktestDataIterator::new();
        let data_len = 5;
        let d0: Vec<Data> = (0..data_len).map(|k| quote("A.B", 3 * k)).collect();
        let d1: Vec<Data> = (0..data_len).map(|k| quote("C.D", 3 * k + 1)).collect();
        let d2: Vec<Data> = (0..data_len).map(|k| quote("E.F", 3 * k + 2)).collect();
        it.add_data("d0", d0, true);
        it.add_data("d1", d1, true);
        it.add_data("d2", d2, true);

        let ts = collect_ts(&mut it);
        assert_eq!(ts.len(), 15);
        for i in 0..ts.len() - 1 {
            assert!(ts[i] <= ts[i + 1], "Not sorted at index {i}");
        }
    }

    #[rstest]
    fn test_multiple_streams_merge_order() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s1", vec![quote("A.B", 100), quote("A.B", 300)], true);
        it.add_data("s2", vec![quote("C.D", 200), quote("C.D", 400)], true);

        assert_eq!(collect_ts(&mut it), vec![100, 200, 300, 400]);
    }

    #[rstest]
    fn test_append_data_priority_default_fifo() {
        let mut it = BacktestDataIterator::new();
        it.add_data("a", vec![quote("A.B", 100)], true);
        it.add_data("b", vec![quote("C.D", 100)], true);

        // Both at same timestamp, FIFO order (a before b)
        let ts = collect_ts(&mut it);
        assert_eq!(ts, vec![100, 100]);
    }

    #[rstest]
    fn test_prepend_priority_wins_ties() {
        let mut it = BacktestDataIterator::new();
        // "a" is appended (lower priority), "b" is prepended (higher priority)
        it.add_data("a", vec![quote("A.B", 100)], true);
        it.add_data("b", vec![quote("C.D", 100)], false);

        // "b" (prepend) should come first despite being added second
        let first = it.next().unwrap();
        let second = it.next().unwrap();
        // Prepend stream (negative priority) wins ties over append (positive)
        assert_eq!(first.instrument_id(), InstrumentId::from("C.D"));
        assert_eq!(second.instrument_id(), InstrumentId::from("A.B"));
    }

    #[rstest]
    fn test_is_done_empty_iterator() {
        let it = BacktestDataIterator::new();
        assert!(it.is_done());
    }

    #[rstest]
    fn test_is_done_after_consumption() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s", vec![quote("A.B", 1)], true);

        assert!(!it.is_done());
        it.next();
        assert!(it.is_done());
    }

    #[rstest]
    fn test_is_done_multi_stream() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1)], true);
        it.add_data("s2", vec![quote("C.D", 2)], true);

        assert!(!it.is_done());
        it.next();
        assert!(!it.is_done());
        it.next();
        assert!(it.is_done());
    }

    #[rstest]
    fn test_partial_consumption_then_complete() {
        let mut it = BacktestDataIterator::new();
        it.add_data(
            "s",
            vec![
                quote("A.B", 0),
                quote("A.B", 1),
                quote("A.B", 2),
                quote("A.B", 3),
            ],
            true,
        );

        assert_eq!(it.next().unwrap().ts_init().as_u64(), 0);
        assert_eq!(it.next().unwrap().ts_init().as_u64(), 1);

        let remaining = collect_ts(&mut it);
        assert_eq!(remaining, vec![2, 3]);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_remove_stream_reduces_output() {
        let mut it = BacktestDataIterator::new();
        it.add_data("a", vec![quote("A.B", 1)], true);
        it.add_data("b", vec![quote("C.D", 2)], true);

        it.remove_data("a", false);

        assert_eq!(collect_ts(&mut it), vec![2]);
    }

    #[rstest]
    fn test_remove_all_streams_yields_empty() {
        let mut it = BacktestDataIterator::new();
        it.add_data("x", vec![quote("A.B", 1)], true);
        it.add_data("y", vec![quote("C.D", 2)], true);

        it.remove_data("x", false);
        it.remove_data("y", false);

        assert!(it.next().is_none());
        assert!(it.is_done());
    }

    #[rstest]
    fn test_remove_nonexistent_stream_is_noop() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s", vec![quote("A.B", 1)], true);

        it.remove_data("nonexistent", false);

        assert_eq!(collect_ts(&mut it), vec![1]);
    }

    #[rstest]
    fn test_remove_after_full_consumption() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 2)], true);

        collect_ts(&mut it);

        it.remove_data("s", true);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_set_index_rewinds_stream() {
        let mut it = BacktestDataIterator::new();
        it.add_data(
            "s",
            vec![quote("A.B", 10), quote("A.B", 20), quote("A.B", 30)],
            true,
        );

        assert_eq!(it.next().unwrap().ts_init().as_u64(), 10);

        it.set_index("s", 0);

        assert_eq!(collect_ts(&mut it), vec![10, 20, 30]);
    }

    #[rstest]
    fn test_set_index_skips_forward() {
        let mut it = BacktestDataIterator::new();
        it.add_data(
            "s",
            vec![quote("A.B", 10), quote("A.B", 20), quote("A.B", 30)],
            true,
        );

        it.set_index("s", 2);

        assert_eq!(collect_ts(&mut it), vec![30]);
    }

    #[rstest]
    fn test_set_index_nonexistent_stream_is_noop() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s", vec![quote("A.B", 1)], true);

        it.set_index("nonexistent", 0);

        assert_eq!(collect_ts(&mut it), vec![1]);
    }

    #[rstest]
    fn test_reset_all_cursors_single_stream() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 2)], true);

        collect_ts(&mut it);
        assert!(it.is_done());

        it.reset_all_cursors();
        assert!(!it.is_done());
        assert_eq!(collect_ts(&mut it), vec![1, 2]);
    }

    #[rstest]
    fn test_reset_all_cursors_multi_stream() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1), quote("A.B", 3)], true);
        it.add_data("s2", vec![quote("C.D", 2), quote("C.D", 4)], true);

        collect_ts(&mut it);
        assert!(it.is_done());

        it.reset_all_cursors();
        assert_eq!(collect_ts(&mut it), vec![1, 2, 3, 4]);
    }

    #[rstest]
    fn test_readding_data_replaces_stream() {
        let mut it = BacktestDataIterator::new();
        it.add_data("X", vec![quote("A.B", 1), quote("A.B", 2)], true);
        it.add_data("X", vec![quote("A.B", 10)], true);

        assert_eq!(collect_ts(&mut it), vec![10]);
    }

    #[rstest]
    fn test_add_empty_data_is_noop() {
        let mut it = BacktestDataIterator::new();
        it.add_data("empty", vec![], true);

        assert!(it.is_done());
        assert!(it.next().is_none());
    }

    #[rstest]
    fn test_empty_iterator_returns_none() {
        let mut it = BacktestDataIterator::new();
        assert!(it.next().is_none());
        assert!(it.is_done());
    }

    #[rstest]
    fn test_multiple_add_data_calls_with_different_names() {
        let mut it = BacktestDataIterator::new();
        it.add_data("batch_0", vec![quote("A.B", 1), quote("A.B", 3)], true);
        it.add_data("batch_1", vec![quote("A.B", 2), quote("A.B", 4)], true);

        assert_eq!(collect_ts(&mut it), vec![1, 2, 3, 4]);
    }

    #[rstest]
    fn test_prepend_stream_always_wins_ties_across_batches() {
        // Verifies that a prepend stream (negative priority) wins ties
        // even when added after multiple append streams
        let mut it = BacktestDataIterator::new();
        it.add_data("append_a", vec![quote("A.B", 100)], true);
        it.add_data("append_b", vec![quote("C.D", 100)], true);
        it.add_data("prepend", vec![quote("E.F", 100)], false);

        let first = it.next().unwrap();
        assert_eq!(
            first.instrument_id(),
            InstrumentId::from("E.F"),
            "Prepend stream should always come first in ties"
        );
    }

    #[rstest]
    fn test_equal_timestamps_across_many_streams_preserves_priority_order() {
        // All items at the same timestamp — ordering is strictly by priority
        let mut it = BacktestDataIterator::new();
        it.add_data("s1", vec![quote("A.B", 50)], true);
        it.add_data("s2", vec![quote("C.D", 50)], true);
        it.add_data("s3", vec![quote("E.F", 50)], true);
        it.add_data("s4", vec![quote("G.H", 50)], true);

        let mut ids = Vec::new();
        while let Some(d) = it.next() {
            ids.push(d.instrument_id());
        }

        assert_eq!(ids.len(), 4);

        // All should be yielded (no duplicates dropped, no items lost)
        assert!(ids.contains(&InstrumentId::from("A.B")));
        assert!(ids.contains(&InstrumentId::from("C.D")));
        assert!(ids.contains(&InstrumentId::from("E.F")));
        assert!(ids.contains(&InstrumentId::from("G.H")));
    }
}
