// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

#![allow(dead_code)]
#![allow(clippy::module_name_repetitions)]

use std::collections::{BinaryHeap, HashMap};

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
    streams: HashMap<i32, Vec<Data>>, // key: priority, value: Vec<Data>
    names: HashMap<i32, String>,      // priority -> name
    priorities: HashMap<String, i32>, // name -> priority
    indices: HashMap<i32, usize>,     // cursor per stream
    heap: BinaryHeap<HeapEntry>,
    single_priority: Option<i32>,
    next_priority_counter: i32, // monotonically increasing counter used to assign priorities
}

impl BacktestDataIterator {
    /// Create an empty [`BacktestDataIterator`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            streams: HashMap::new(),
            names: HashMap::new(),
            priorities: HashMap::new(),
            indices: HashMap::new(),
            heap: BinaryHeap::new(),
            single_priority: None,
            next_priority_counter: 0,
        }
    }

    /// Add (or replace) a named data stream.  `append_data=true` gives the stream
    /// lower priority when timestamps tie, mirroring the original behaviour.
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

    /// Remove a stream.  `complete_remove` also discards placeholder generator
    /// (not implemented yet).
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

    /// Move cursor of stream to `index` (0-based).
    pub fn set_index(&mut self, name: &str, index: usize) {
        if let Some(priority) = self.priorities.get(name) {
            self.indices.insert(*priority, index);
            self.rebuild_heap();
        }
    }

    /// Return next Data element across all streams in chronological order.
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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

    #[rstest]
    fn test_single_stream() {
        let mut it = BacktestDataIterator::new();
        let stream = vec![quote("BTC-PERP.BINANCE", 1), quote("BTC-PERP.BINANCE", 3)];
        it.add_data("main", stream, true);
        assert_eq!(it.next().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.next().unwrap().ts_init(), UnixNanos::from(3));
        assert!(it.next().is_none());
    }

    #[rstest]
    fn test_two_stream_merge() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1), quote("A.B", 4)], true);
        it.add_data("s2", vec![quote("C.D", 2), quote("C.D", 3)], false);

        let mut ts = Vec::new();
        while let Some(d) = it.next() {
            ts.push(d.ts_init());
        }
        assert_eq!(
            ts,
            vec![
                UnixNanos::from(1),
                UnixNanos::from(2),
                UnixNanos::from(3),
                UnixNanos::from(4)
            ]
        );
    }
}
