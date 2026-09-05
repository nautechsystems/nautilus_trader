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

//! Multi-stream, time-ordered data iterator for replaying historical data.

use std::collections::BinaryHeap;

use ahash::AHashMap;
use nautilus_core::UnixNanos;
use nautilus_model::data::{BatchView, Data, DataBatch, DataRef, HasTsInit};

use crate::data_batch::ReplayBatch;
#[cfg(feature = "defi")]
use crate::defi::replay::replay_position;

// TODO: block_number/transaction_index/log_index/phase are DeFi-only (zero for all other data,
// even in non-DeFi builds); they exist to order same-block DeFi events in canonical chain order.
// This leaks DeFi-specific shape into a general key, so it could be cfg-gated or moved behind an
// opaque secondary key later (non-breaking, no correctness or perf cost).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct ReplayKey {
    ts: UnixNanos,
    block_number: u64,
    transaction_index: u32,
    log_index: u32,
    phase: u8,
}

fn replay_key(data: DataRef<'_>) -> ReplayKey {
    let ts = data.ts_init();
    match data {
        DataRef::BookDelta(_)
        | DataRef::BookDeltas(_)
        | DataRef::BookDepth10(_)
        | DataRef::Quote(_)
        | DataRef::Trade(_)
        | DataRef::Bar(_)
        | DataRef::MarkPrice(_)
        | DataRef::IndexPrice(_)
        | DataRef::FundingRate(_)
        | DataRef::OptionGreeks(_)
        | DataRef::InstrumentStatus(_)
        | DataRef::InstrumentClose(_)
        | DataRef::Custom(_) => ReplayKey {
            ts,
            block_number: 0,
            transaction_index: 0,
            log_index: 0,
            phase: 0,
        },
        #[cfg(feature = "defi")]
        DataRef::Defi(defi) => {
            let (block_number, transaction_index, log_index, phase) = replay_position(defi);
            ReplayKey {
                ts,
                block_number,
                transaction_index,
                log_index,
                phase,
            }
        }
    }
}

fn sort_by_replay_key(batch: &mut DataBatch) {
    match batch {
        DataBatch::BookDelta(data) => {
            sort_view_by_replay_key(data, |item| DataRef::BookDelta(item));
        }
        DataBatch::BookDeltas(data) => {
            sort_view_by_replay_key(data, |item| DataRef::BookDeltas(item));
        }
        DataBatch::BookDepth10(data) => {
            sort_view_by_replay_key(data, |item| DataRef::BookDepth10(item));
        }
        DataBatch::Quote(data) => sort_view_by_replay_key(data, |item| DataRef::Quote(item)),
        DataBatch::Trade(data) => sort_view_by_replay_key(data, |item| DataRef::Trade(item)),
        DataBatch::Bar(data) => sort_view_by_replay_key(data, |item| DataRef::Bar(item)),
        DataBatch::MarkPrice(data) => {
            sort_view_by_replay_key(data, |item| DataRef::MarkPrice(item));
        }
        DataBatch::IndexPrice(data) => {
            sort_view_by_replay_key(data, |item| DataRef::IndexPrice(item));
        }
        DataBatch::FundingRate(data) => {
            sort_view_by_replay_key(data, |item| DataRef::FundingRate(item));
        }
        DataBatch::OptionGreeks(data) => {
            sort_view_by_replay_key(data, |item| DataRef::OptionGreeks(item));
        }
        DataBatch::InstrumentStatus(data) => {
            sort_view_by_replay_key(data, |item| DataRef::InstrumentStatus(item));
        }
        DataBatch::InstrumentClose(data) => {
            sort_view_by_replay_key(data, |item| DataRef::InstrumentClose(item));
        }
        #[cfg(feature = "defi")]
        DataBatch::Defi(data) => sort_view_by_replay_key(data, |item| DataRef::Defi(item)),
    }
}

// `as_ref` takes a closure because `DataRef`'s lifetime is an enum parameter, so its constructors
// cannot coerce to this higher-ranked fn pointer.
fn sort_view_by_replay_key<T: Clone>(view: &mut BatchView<T>, as_ref: fn(&T) -> DataRef<'_>) {
    if !view.is_sorted_by_key(|item| replay_key(as_ref(item))) {
        view.make_mut().sort_by_key(|item| replay_key(as_ref(item)));
    }
}

/// Internal convenience struct to keep heap entries ordered by replay key and priority.
#[derive(Debug, Eq, PartialEq)]
struct HeapEntry {
    key: ReplayKey,
    priority: i32,
    index: usize,
}

impl Ord for HeapEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // min-heap on replay key, then priority sign (+/-) then index
        self.key
            .cmp(&other.key)
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
    streams: AHashMap<i32, ReplayBatch>,
    names: AHashMap<i32, String>,
    priorities: AHashMap<String, i32>,
    indices: AHashMap<i32, usize>,
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

        data.sort_by_key(|item| replay_key(DataRef::from(item)));

        self.add_stream(name, ReplayBatch::from_data(data), append_data);
    }

    /// Adds (or replaces) a named typed data stream.
    ///
    /// Items are ordered by replay key before insertion. A batch that arrives out of order while
    /// sharing its backing allocation with another view is copied once, so the shared allocation
    /// is never reordered.
    pub fn add_data_batch(&mut self, name: &str, mut data: DataBatch, append_data: bool) {
        if data.is_empty() {
            return;
        }

        sort_by_replay_key(&mut data);

        self.add_stream(name, ReplayBatch::Typed(data), append_data);
    }

    fn add_stream(&mut self, name: &str, data: ReplayBatch, append_data: bool) {
        let priority = if let Some(p) = self.priorities.get(name) {
            // Replace existing stream - remove previous traces then re-insert below.
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

    /// Returns the next backtest data element without advancing the stream cursor.
    pub(crate) fn peek(&self) -> Option<DataRef<'_>> {
        if let Some(p) = self.single_priority {
            let data = self.streams.get(&p)?;
            let idx = *self.indices.get(&p)?;
            return data.get(idx);
        }

        let entry = self.heap.peek()?;
        self.streams.get(&entry.priority)?.get(entry.index)
    }

    /// Advances past the current backtest data element, or does nothing if the iterator is
    /// exhausted.
    pub(crate) fn advance(&mut self) {
        if let Some(p) = self.single_priority {
            let Some(data) = self.streams.get(&p) else {
                return;
            };

            let Some(idx) = self.indices.get_mut(&p) else {
                return;
            };

            if *idx < data.len() {
                *idx += 1;
            }

            return;
        }

        // Multi-stream path using heap
        let Some(entry) = self.heap.pop() else {
            return;
        };

        let Some(stream) = self.streams.get(&entry.priority) else {
            return;
        };

        // Advance cursor and push next entry
        let next_index = entry.index + 1;
        self.indices.insert(entry.priority, next_index);

        if next_index < stream.len() {
            self.heap.push(HeapEntry {
                key: replay_key(
                    stream
                        .get(next_index)
                        .expect("next index is within the data batch"),
                ),
                priority: entry.priority,
                index: next_index,
            });
        }
    }

    /// Returns the next backtest data element across all streams in replay order.
    pub(crate) fn next_item(&mut self) -> Option<Data> {
        let element = if let Some(p) = self.single_priority {
            let data = self.streams.get(&p)?;
            let idx = *self.indices.get(&p)?;
            data.get_owned(idx)?
        } else {
            let entry = self.heap.peek()?;
            self.streams.get(&entry.priority)?.get_owned(entry.index)?
        };

        self.advance();

        Some(element)
    }

    /// Returns the next market [`Data`] element across all streams in chronological order.
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Data> {
        self.next_item()
    }

    /// Returns whether all streams have been fully consumed.
    #[must_use]
    pub fn is_done(&self) -> bool {
        if let Some(p) = self.single_priority {
            if let Some(idx) = self.indices.get(&p)
                && let Some(data) = self.streams.get(&p)
            {
                return *idx >= data.len();
            }
            true
        } else {
            self.heap.is_empty()
        }
    }

    fn rebuild_heap(&mut self) {
        self.heap.clear();

        // Determine if we're in single-stream mode
        if self.streams.len() == 1 {
            self.single_priority = self.streams.keys().next().copied();
            return;
        }
        self.single_priority = None;

        for (&priority, data) in &self.streams {
            let idx = *self.indices.get(&priority).unwrap_or(&0);
            if idx < data.len() {
                self.heap.push(HeapEntry {
                    key: replay_key(data.get(idx).expect("index is within the data batch")),
                    priority,
                    index: idx,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::{
        data::{
            Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
            MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDeltas, OrderBookDepth10,
            QuoteTick, TradeTick,
            stubs::{
                stub_bar, stub_delta, stub_deltas, stub_depth10, stub_instrument_close,
                stub_instrument_status, stub_trade_ethusdt_buy,
            },
        },
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    #[cfg(feature = "defi")]
    use nautilus_model::{
        defi::{
            DefiData,
            data::block::BlockPosition,
            pool_analysis::snapshot::{PoolAnalytics, PoolSnapshot, PoolState},
        },
        identifiers::{Symbol, Venue},
    };
    use rstest::rstest;

    use super::*;

    fn quote_tick(id: &str, ts: u64) -> QuoteTick {
        QuoteTick::new(
            InstrumentId::from(id),
            Price::from("1.0"),
            Price::from("1.0"),
            Quantity::from(100),
            Quantity::from(100),
            ts.into(),
            ts.into(),
        )
    }

    fn quote(id: &str, ts: u64) -> Data {
        Data::Quote(quote_tick(id, ts))
    }

    fn trade(id: &str, ts: u64) -> Data {
        let mut trade = stub_trade_ethusdt_buy();
        trade.instrument_id = InstrumentId::from(id);
        trade.ts_event = UnixNanos::from(ts);
        trade.ts_init = UnixNanos::from(ts);
        Data::Trade(trade)
    }

    fn collect_sequence(it: &mut BacktestDataIterator) -> Vec<(InstrumentId, UnixNanos)> {
        let mut sequence = Vec::new();
        while let Some(data) = it.peek() {
            sequence.push((data.instrument_id(), data.ts_init()));
            it.advance();
        }
        sequence
    }

    fn collect_ts(it: &mut BacktestDataIterator) -> Vec<u64> {
        let mut ts = Vec::new();
        while let Some(d) = it.next() {
            ts.push(d.ts_init().as_u64());
        }
        ts
    }

    #[cfg(feature = "defi")]
    fn defi_pool_snapshot(ts: u64, block: u64, transaction_index: u32, log_index: u32) -> DefiData {
        let instrument_id = InstrumentId::new(Symbol::from("ETH/USDC"), Venue::from("UNISWAPV3"));
        let snapshot = PoolSnapshot::new(
            instrument_id,
            PoolState::default(),
            Vec::new(),
            Vec::new(),
            PoolAnalytics::default(),
            BlockPosition::new(block, format!("0x{block:x}"), transaction_index, log_index),
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        );

        DefiData::PoolSnapshot(snapshot)
    }

    #[cfg(feature = "defi")]
    fn defi_snapshot(ts: u64, block: u64, transaction_index: u32, log_index: u32) -> Data {
        Data::Defi(Box::new(defi_pool_snapshot(
            ts,
            block,
            transaction_index,
            log_index,
        )))
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
    fn test_peek_does_not_consume_single_stream_item() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 2)], true);

        assert_eq!(it.peek().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.peek().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.next().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.peek().unwrap().ts_init(), UnixNanos::from(2));
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
    fn test_peek_does_not_consume_multi_stream_heap_item() {
        let mut it = BacktestDataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1), quote("A.B", 4)], true);
        it.add_data("s2", vec![quote("C.D", 2), quote("C.D", 3)], true);

        assert_eq!(it.peek().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.peek().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.next().unwrap().ts_init(), UnixNanos::from(1));
        assert_eq!(it.peek().unwrap().ts_init(), UnixNanos::from(2));
        assert_eq!(collect_ts(&mut it), vec![2, 3, 4]);
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
    fn test_set_index_uses_logical_mixed_stream_offset() {
        let mut it = BacktestDataIterator::new();
        it.add_data(
            "mixed",
            vec![quote("A.B", 10), trade("C.D", 10), quote("E.F", 20)],
            true,
        );

        it.set_index("mixed", 1);

        assert_eq!(
            collect_sequence(&mut it),
            vec![
                (InstrumentId::from("C.D"), UnixNanos::from(10)),
                (InstrumentId::from("E.F"), UnixNanos::from(20)),
            ]
        );
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
    fn test_readding_data_reuses_equal_key_stream_priority() {
        let mut it = BacktestDataIterator::new();
        it.add_data("first", vec![quote("A.B", 10)], true);
        it.add_data("second", vec![quote("C.D", 10)], true);
        it.add_data("first", vec![quote("E.F", 10)], true);

        assert_eq!(
            collect_sequence(&mut it),
            vec![
                (InstrumentId::from("E.F"), UnixNanos::from(10)),
                (InstrumentId::from("C.D"), UnixNanos::from(10)),
            ]
        );
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
    fn test_typed_and_compatibility_streams_preserve_equal_key_order() {
        let mut single_batch = BacktestDataIterator::new();
        single_batch.add_data(
            "single",
            vec![quote("A.B", 10), trade("C.D", 10), quote("E.F", 20)],
            true,
        );

        let mut split_batches = BacktestDataIterator::new();
        split_batches.add_data("first", vec![quote("A.B", 10)], true);
        split_batches.add_data("second", vec![trade("C.D", 10), quote("E.F", 20)], true);

        assert_eq!(
            collect_sequence(&mut split_batches),
            collect_sequence(&mut single_batch)
        );
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
        // All items at the same timestamp - ordering is strictly by priority
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

    #[rstest]
    fn test_add_data_batch_sorts_every_typed_family_by_replay_key() {
        let instrument_id = InstrumentId::from("A.B");
        let late = UnixNanos::from(200);
        let early = UnixNanos::from(100);
        let mark = |ts| MarkPriceUpdate::new(instrument_id, Price::from("1.0"), ts, ts);
        let index = |ts| IndexPriceUpdate::new(instrument_id, Price::from("1.0"), ts, ts);
        let funding = |ts| {
            FundingRateUpdate::new(instrument_id, "0.0001".parse().unwrap(), None, None, ts, ts)
        };
        let greeks = |ts| OptionGreeks {
            instrument_id,
            ts_init: ts,
            ..OptionGreeks::default()
        };
        let batches = vec![
            DataBatch::from(vec![
                OrderBookDelta {
                    ts_init: late,
                    ..stub_delta()
                },
                OrderBookDelta {
                    ts_init: early,
                    ..stub_delta()
                },
            ]),
            DataBatch::from(vec![
                OrderBookDeltas {
                    ts_init: late,
                    ..stub_deltas()
                },
                OrderBookDeltas {
                    ts_init: early,
                    ..stub_deltas()
                },
            ]),
            DataBatch::from(vec![
                OrderBookDepth10 {
                    ts_init: late,
                    ..stub_depth10()
                },
                OrderBookDepth10 {
                    ts_init: early,
                    ..stub_depth10()
                },
            ]),
            DataBatch::from(vec![quote_tick("A.B", 200), quote_tick("A.B", 100)]),
            DataBatch::from(vec![
                TradeTick {
                    ts_init: late,
                    ..stub_trade_ethusdt_buy()
                },
                TradeTick {
                    ts_init: early,
                    ..stub_trade_ethusdt_buy()
                },
            ]),
            DataBatch::from(vec![
                Bar {
                    ts_init: late,
                    ..stub_bar()
                },
                Bar {
                    ts_init: early,
                    ..stub_bar()
                },
            ]),
            DataBatch::from(vec![mark(late), mark(early)]),
            DataBatch::from(vec![index(late), index(early)]),
            DataBatch::from(vec![funding(late), funding(early)]),
            DataBatch::from(vec![greeks(late), greeks(early)]),
            DataBatch::from(vec![
                InstrumentStatus {
                    ts_init: late,
                    ..stub_instrument_status()
                },
                InstrumentStatus {
                    ts_init: early,
                    ..stub_instrument_status()
                },
            ]),
            DataBatch::from(vec![
                InstrumentClose {
                    ts_init: late,
                    ..stub_instrument_close()
                },
                InstrumentClose {
                    ts_init: early,
                    ..stub_instrument_close()
                },
            ]),
        ];
        assert_eq!(
            batches.len(),
            12,
            "every static DataBatch variant needs a case"
        );

        for batch in batches {
            let mut it = BacktestDataIterator::new();
            it.add_data_batch("typed", batch, true);

            assert_eq!(collect_ts(&mut it), vec![100, 200]);
        }
    }

    #[rstest]
    fn test_add_data_batch_shares_sorted_backing_allocation() {
        let data = Arc::new(vec![quote_tick("A.B", 100), quote_tick("A.B", 200)]);
        let mut it = BacktestDataIterator::new();

        it.add_data_batch(
            "typed",
            DataBatch::Quote(BatchView::from(Arc::clone(&data))),
            true,
        );

        assert_eq!(Arc::strong_count(&data), 2);
        assert_eq!(collect_ts(&mut it), vec![100, 200]);
    }

    #[rstest]
    fn test_add_data_batch_copies_shared_unsorted_backing_allocation() {
        let data = Arc::new(vec![quote_tick("A.B", 200), quote_tick("A.B", 100)]);
        let mut it = BacktestDataIterator::new();

        it.add_data_batch(
            "typed",
            DataBatch::Quote(BatchView::from(Arc::clone(&data))),
            true,
        );

        assert_eq!(Arc::strong_count(&data), 1);
        assert_eq!(data[0].ts_init, UnixNanos::from(200));
        assert_eq!(collect_ts(&mut it), vec![100, 200]);
    }

    #[rstest]
    fn test_add_empty_data_batch_is_noop() {
        let mut it = BacktestDataIterator::new();
        it.add_data_batch("typed", DataBatch::from(Vec::<QuoteTick>::new()), true);

        assert!(it.is_done());
        assert!(it.peek().is_none());
    }

    #[rstest]
    fn test_typed_batch_and_legacy_streams_preserve_equal_key_order() {
        let mut single_batch = BacktestDataIterator::new();
        single_batch.add_data(
            "single",
            vec![quote("A.B", 10), trade("C.D", 10), quote("E.F", 20)],
            true,
        );

        let mut split_streams = BacktestDataIterator::new();
        split_streams.add_data_batch(
            "typed",
            DataBatch::from(vec![quote_tick("A.B", 10), quote_tick("E.F", 20)]),
            true,
        );
        split_streams.add_data("legacy", vec![trade("C.D", 10)], true);

        assert_eq!(
            collect_sequence(&mut split_streams),
            collect_sequence(&mut single_batch)
        );
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_add_data_batch_orders_defi_by_block_position() {
        let mut it = BacktestDataIterator::new();
        it.add_data_batch(
            "defi",
            DataBatch::from(vec![
                defi_pool_snapshot(100, 12, 4, 1),
                defi_pool_snapshot(100, 11, 9, 9),
                defi_pool_snapshot(100, 12, 2, 7),
            ]),
            true,
        );

        let mut positions = Vec::new();
        while let Some(Data::Defi(data)) = it.next_item() {
            positions.push(data.block_position());
        }

        assert_eq!(positions, vec![(11, 9, 9), (12, 2, 7), (12, 4, 1)]);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_defi_data_orders_equal_timestamps_by_block_position() {
        let mut it = BacktestDataIterator::new();
        it.add_data(
            "defi",
            vec![
                defi_snapshot(100, 12, 4, 1),
                defi_snapshot(100, 11, 9, 9),
                defi_snapshot(100, 12, 2, 7),
            ],
            true,
        );

        let mut positions = Vec::new();
        while let Some(Data::Defi(data)) = it.next_item() {
            positions.push(data.block_position());
        }

        assert_eq!(positions, vec![(11, 9, 9), (12, 2, 7), (12, 4, 1)]);
    }
}
