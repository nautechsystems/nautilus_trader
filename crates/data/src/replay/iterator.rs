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

use std::{collections::BinaryHeap, fmt::Debug, ops::ControlFlow};

use ahash::AHashMap;
use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use nautilus_model::data::{BatchView, Data, DataBatch, DataRef, HasTsInit};
#[cfg(feature = "defi")]
use nautilus_model::defi::DefiData;

use super::ReplayBatch;

/// A ready value, an unfinished source, or successful exhaustion.
#[derive(Debug, PartialEq, Eq)]
pub enum ReplayPoll<T> {
    /// A value is available.
    Ready(T),
    /// The input is unfinished and requires progress before selection can continue.
    Pending {
        /// Source polls omit a new bound with `None`; row polls use `None` for an unknown bound.
        frontier: Option<ReplayKey>,
    },
    /// All input has completed and all buffered rows have drained.
    Complete,
}

/// A source supplying batches to ordered replay.
///
/// Ready batches must be nonempty. Omitted frontiers retain earlier promises; known frontiers
/// and batches cannot move backward. Polling must return promptly, without waiting for input.
/// Removal or iterator drop releases the source. Callers arrange wakeups and external cancellation.
pub trait ReplaySource {
    /// Returns a batch, a pending lower bound, or successful exhaustion.
    ///
    /// # Errors
    ///
    /// Returns an error if the source cannot load its next batch.
    fn poll_batch(&mut self) -> anyhow::Result<ReplayPoll<ReplayBatch>>;

    /// Rewinds the source, including after completion or an earlier reset failure.
    ///
    /// # Errors
    ///
    /// Returns an error if reset is unsupported or fails.
    fn reset(&mut self) -> anyhow::Result<()> {
        anyhow::bail!("Replay source reset is unsupported")
    }
}

/// Multi-stream, time-ordered data iterator for static and synchronously refilled replay.
///
/// Refills and polled sources retain stream priority and validate ordering across batches.
/// See the [replay module](super) for source mutation, failure, and reset contracts.
#[derive(Debug, Default)]
pub struct DataIterator {
    streams: AHashMap<i32, ReplayCursor>,
    names: AHashMap<i32, String>,
    priorities: AHashMap<String, i32>,
    heap: BinaryHeap<HeapEntry>,
    single_priority: Option<i32>,
    next_priority_counter: i32, // monotonically increasing counter used to assign priorities
    sources: IndexMap<i32, SourceCursor>,
    source_mode: bool,
    started: bool,
    refresh: bool,
    fault: Option<String>,
}

impl DataIterator {
    /// Creates a new empty [`DataIterator`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            streams: AHashMap::new(),
            names: AHashMap::new(),
            priorities: AHashMap::new(),
            heap: BinaryHeap::new(),
            single_priority: None,
            next_priority_counter: 0,
            sources: IndexMap::new(),
            source_mode: false,
            started: false,
            refresh: false,
            fault: None,
        }
    }

    /// Adds (or replaces) a named data stream.
    ///
    /// When `append_data` is true the stream gets lower priority on equal replay keys;
    /// when false (prepend) it wins ties. Named replacements retain their original priority.
    pub fn add_data(&mut self, name: &str, mut data: Vec<Data>, append_data: bool) {
        if data.is_empty() {
            return;
        }

        data.sort_by_key(|item| replay_key(DataRef::from(item)));

        self.add_stream(name, Some(ReplayBatch::from_data(data)), append_data);
    }

    /// Adds (or replaces) a named typed data stream.
    ///
    /// Items are ordered by replay key before insertion. A batch that arrives out of order while
    /// sharing its backing allocation with another view is copied once, so the shared allocation
    /// is never reordered.
    pub fn add_data_batch(&mut self, name: &str, data: DataBatch, append_data: bool) {
        self.add_batch(name, ReplayBatch::Typed(data), append_data);
    }

    /// Adds or replaces a logical stream, stably sorting its batch by replay key.
    /// Empty input leaves any existing stream unchanged.
    pub fn add_batch(&mut self, name: &str, mut data: ReplayBatch, append_data: bool) {
        if data.len() == 0 {
            return;
        }

        sort_batch(&mut data);
        self.add_stream(name, Some(data), append_data);
    }

    /// Registers a source and polls its first batch before returning.
    ///
    /// Named replacements retain priority. Registration requires a successful reset after
    /// consumption starts; removal remains permitted. First-poll errors leave existing streams
    /// intact.
    ///
    /// # Errors
    ///
    /// Returns an error if replay has started, is faulted, or the first poll fails or supplies
    /// invalid input.
    pub fn add_source(
        &mut self,
        name: &str,
        source: Box<dyn ReplaySource>,
        append_data: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.started, "Replay source admission requires a reset");
        self.check_fault()?;

        let mut cursor = SourceCursor {
            source,
            bound: None,
            state: SourceState::Exhausted,
        };

        let data = cursor.poll(name)?;
        self.add_stream(name, data, append_data);
        let priority = self.priorities[name];
        self.sources.insert(priority, cursor);
        self.source_mode = true;
        self.refresh = true;
        Ok(())
    }

    /// Refills an exhausted stream without changing its tie priority.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The stream is unknown, source-owned, or replay is faulted.
    /// - Source-backed replay has already consumed a row.
    /// - The current batch has unconsumed rows.
    /// - The new batch is empty.
    /// - The new batch's first replay key after stable sorting precedes the previous batch's
    ///   last key.
    pub fn refill_batch(&mut self, name: &str, mut data: ReplayBatch) -> anyhow::Result<()> {
        let priority = self
            .priorities
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown replay stream '{name}'"))?;
        self.check_fault()?;
        anyhow::ensure!(
            !self.sources.contains_key(priority),
            "Replay stream '{name}' is source-owned"
        );
        anyhow::ensure!(
            !(self.source_mode && self.started),
            "Caller-managed refill during source replay requires a reset"
        );
        let previous = &self.streams[priority];
        anyhow::ensure!(
            previous.index >= previous.data.len(),
            "Replay stream '{name}' still has unconsumed data"
        );
        anyhow::ensure!(
            data.len() > 0,
            "Replay stream '{name}' supplied an empty batch"
        );

        sort_batch(&mut data);

        let last = previous.data.ts_consumed().map_or_else(
            || {
                let data = previous
                    .data
                    .get(previous.data.len() - 1)
                    .expect("registered batches are nonempty");
                replay_key(data)
            },
            |ts| ReplayKey {
                ts,
                ..ReplayKey::default()
            },
        );

        let first = data.get(0).expect("refill batch is nonempty");
        anyhow::ensure!(
            replay_key(first) >= last,
            "Replay stream '{name}' moved backward across batches: {:?} after {:?}",
            replay_key(first),
            last
        );

        self.streams
            .insert(*priority, ReplayCursor { data, index: 0 });
        self.rebuild_heap();
        Ok(())
    }

    fn add_stream(&mut self, name: &str, data: Option<ReplayBatch>, append_data: bool) {
        self.record_mutation();

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

        if let Some(data) = data {
            self.streams
                .insert(priority, ReplayCursor { data, index: 0 });
        }

        self.names.insert(priority, name.to_string());
        self.priorities.insert(name.to_string(), priority);

        self.rebuild_heap();
    }

    /// Removes a named data stream.
    pub fn remove_data(&mut self, name: &str, complete_remove: bool) {
        if let Some(priority) = self.priorities.remove(name) {
            self.streams.remove(&priority);
            self.sources.shift_remove(&priority);
            self.names.remove(&priority);
            self.refresh = true;

            // Rebuild heap sans removed priority
            self.heap.retain(|e| e.priority != priority);

            if self.single_priority == Some(priority) {
                self.single_priority = None;
            }
        }

        if complete_remove {
            // Placeholder for future generator cleanup
        }
    }

    /// Sets a caller-managed stream cursor to `index` (0-based).
    /// Source-owned cursor changes fault the next selection.
    pub fn set_index(&mut self, name: &str, index: usize) {
        let Some(priority) = self.priorities.get(name).copied() else {
            return;
        };

        if self.sources.contains_key(&priority) {
            self.fault
                .get_or_insert_with(|| "Source-owned cursor changes require a reset".into());
            return;
        }

        self.record_mutation();

        if let Some(cursor) = self.streams.get_mut(&priority) {
            cursor.index = index;
        }

        self.rebuild_heap();
    }

    /// Rewinds static batches and every source, preserving stream priorities.
    ///
    /// Source reset failures latch a replay fault; another successful reset is required before
    /// selection resumes. Source batches are loaded on the next selection.
    ///
    /// # Errors
    ///
    /// Returns an error if a static book frame has moved out, or a source reset fails.
    /// A moved-frame error leaves the iterator unchanged.
    pub fn reset_all_cursors(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.streams.iter().any(|(priority, cursor)| {
                !self.sources.contains_key(priority) && cursor.data.ts_consumed().is_some()
            }),
            "Cannot reset a consumed static book frame"
        );

        for source in self.sources.values_mut() {
            if let Err(e) = source.source.reset() {
                self.fault = Some(format!("{e:#}"));
                return Err(e);
            }
        }

        self.streams
            .retain(|priority, _| !self.sources.contains_key(priority));

        for source in self.sources.values_mut() {
            source.bound = None;
            source.state = SourceState::Exhausted;
        }

        for cursor in self.streams.values_mut() {
            cursor.index = 0;
        }

        self.started = false;
        self.source_mode = !self.sources.is_empty();
        self.refresh = !self.sources.is_empty();
        self.fault = None;
        self.rebuild_heap();
        Ok(())
    }

    /// Prepares the next borrowed row without consuming it.
    ///
    /// Pending never means exhaustion. Its frontier bounds every remaining row, and equality
    /// waits to preserve stream priority. A bound at timestamp `t` does not prove `t` complete;
    /// callers must not finalize that timestamp or fire timers at or beyond a pending bound.
    ///
    /// # Errors
    ///
    /// Returns an error if a source fails, supplies an invalid batch or frontier, or replay is
    /// faulted.
    #[inline]
    pub fn peek(&mut self) -> anyhow::Result<ReplayPoll<DataRef<'_>>> {
        self.prepare()?;
        Ok(match self.pending() {
            ControlFlow::Break(frontier) => ReplayPoll::Pending { frontier },
            ControlFlow::Continue(()) => match self.peek_ready() {
                Some(data) => ReplayPoll::Ready(data),
                None => ReplayPoll::Complete,
            },
        })
    }

    /// Consumes an eligible buffered row without polling a source.
    /// Does nothing when blocked, faulted, or exhausted.
    pub fn advance(&mut self) {
        if self.fault.is_none() && self.pending().is_continue() {
            let _ = self.advance_cursor();
        }
    }

    /// Consumes an owned row and reports a caller-managed batch's exhaustion.
    ///
    /// Book frames move without cloning and cannot be rewound unless supplied by a resettable
    /// source. The next selection polls replacement batches, so a refill failure cannot swallow
    /// the selected row. In static-only replay, callers refill an exhausted caller-managed stream
    /// before selecting another row. Refills are rejected during source-backed consumption.
    ///
    /// # Errors
    ///
    /// Returns an error if a source fails, supplies an invalid batch or frontier, or replay is
    /// faulted.
    pub fn next_stream_item(&mut self) -> anyhow::Result<ReplayPoll<(Data, Option<&str>)>> {
        self.prepare()?;

        if let ControlFlow::Break(frontier) = self.pending() {
            return Ok(ReplayPoll::Pending { frontier });
        }

        let Some((batch, index, name)) = self.advance_cursor() else {
            return Ok(ReplayPoll::Complete);
        };

        let data = batch.take_owned(index).expect("selected row exists");
        Ok(ReplayPoll::Ready((data, name)))
    }

    /// Returns the next owned row by cloning the selected row from retained storage.
    ///
    /// # Errors
    ///
    /// Returns an error if a source fails, supplies an invalid batch or frontier, or replay is
    /// faulted.
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> anyhow::Result<ReplayPoll<Data>> {
        let data = match self.peek()? {
            ReplayPoll::Ready(data) => data.to_owned_data(),
            ReplayPoll::Pending { frontier } => return Ok(ReplayPoll::Pending { frontier }),
            ReplayPoll::Complete => return Ok(ReplayPoll::Complete),
        };

        self.advance();
        Ok(ReplayPoll::Ready(data))
    }

    /// Returns whether all sources completed and all retained batches drained.
    /// Pending and faulted replay never report successful completion.
    #[must_use]
    pub fn is_done(&self) -> bool {
        if self.fault.is_some()
            || self
                .sources
                .values()
                .any(|s| s.state != SourceState::Complete)
        {
            return false;
        }

        if let Some(priority) = self.single_priority {
            self.streams
                .get(&priority)
                .is_none_or(|cursor| cursor.index >= cursor.data.len())
        } else {
            self.heap.is_empty()
        }
    }

    #[inline]
    fn peek_ready(&self) -> Option<DataRef<'_>> {
        if let Some(p) = self.single_priority {
            let cursor = self.streams.get(&p)?;
            return cursor.data.get(cursor.index);
        }

        let entry = self.heap.peek()?;
        self.streams.get(&entry.priority)?.data.get(entry.index)
    }

    #[inline]
    fn prepare(&mut self) -> anyhow::Result<()> {
        self.check_fault()?;

        if self.refresh
            && let Err(e) = self.refresh_sources()
        {
            self.fault = Some(format!("{e:#}"));
            return Err(e);
        }

        Ok(())
    }

    fn check_fault(&self) -> anyhow::Result<()> {
        if let Some(e) = &self.fault {
            anyhow::bail!("Replay requires a successful reset after failure: {e}");
        }

        Ok(())
    }

    fn record_mutation(&mut self) {
        if self.source_mode && self.started && self.fault.is_none() {
            self.fault = Some("Replay mutation after source consumption requires a reset".into());
        }
    }

    fn refresh_sources(&mut self) -> anyhow::Result<()> {
        for index in 0..self.sources.len() {
            let (priority, source) = self.sources.get_index(index).expect("source index exists");

            let needs_poll = match source.state {
                SourceState::Exhausted => true,
                SourceState::Pending => self
                    .peek_ready()
                    .is_none_or(|data| source.bound.is_none_or(|bound| replay_key(data) >= bound)),
                SourceState::Buffered | SourceState::Complete => false,
            };

            if needs_poll {
                let priority = *priority;
                let name = &self.names[&priority];
                let source = self
                    .sources
                    .get_index_mut(index)
                    .expect("source index exists")
                    .1;
                let batch = source.poll(name)?;
                self.streams.remove(&priority);

                if let Some(data) = batch {
                    self.streams
                        .insert(priority, ReplayCursor { data, index: 0 });
                }

                self.rebuild_heap();
            }
        }

        self.refresh = self
            .sources
            .values()
            .any(|source| matches!(source.state, SourceState::Pending | SourceState::Exhausted));
        Ok(())
    }

    #[inline]
    fn pending(&self) -> ControlFlow<Option<ReplayKey>> {
        if !self.refresh {
            return ControlFlow::Continue(());
        }

        let head = self.peek_ready().map(replay_key);
        let mut frontier = None;

        for source in self.sources.values() {
            if !matches!(source.state, SourceState::Pending | SourceState::Exhausted) {
                continue;
            }

            let Some(bound) = source.bound else {
                return ControlFlow::Break(None);
            };

            frontier = Some(frontier.map_or(bound, |previous: ReplayKey| previous.min(bound)));
        }

        match frontier.filter(|bound| head.is_none_or(|head| head >= *bound)) {
            Some(bound) => ControlFlow::Break(Some(bound)),
            None => ControlFlow::Continue(()),
        }
    }

    // Lets borrowed advancement omit row metadata and owned consumption reuse the cursor lookup
    #[inline(always)]
    fn advance_cursor(&mut self) -> Option<(&mut ReplayBatch, usize, Option<&str>)> {
        let (priority, index, exhausted, cursor) = if let Some(priority) = self.single_priority {
            let cursor = self.streams.get_mut(&priority)?;
            let index = cursor.index;
            let len = cursor.data.len();
            if index >= len {
                return None;
            }

            cursor.index += 1;
            (priority, index, cursor.index == len, cursor)
        } else {
            let entry = self.heap.pop()?;
            let cursor = self.streams.get_mut(&entry.priority)?;
            let next_index = entry.index + 1;
            cursor.index = next_index;
            let exhausted = next_index >= cursor.data.len();
            if !exhausted {
                self.heap.push(HeapEntry {
                    key: replay_key(
                        cursor
                            .data
                            .get(next_index)
                            .expect("next index is within the data batch"),
                    ),
                    priority: entry.priority,
                    index: next_index,
                });
            }

            (entry.priority, entry.index, exhausted, cursor)
        };

        self.started = true;

        let source = if exhausted {
            self.sources.get_mut(&priority)
        } else {
            None
        };

        let source_owned = source.is_some();
        if exhausted && let Some(source) = source {
            let key = replay_key(cursor.data.get(index).expect("selected row exists"));
            source.bound = Some(source.bound.map_or(key, |bound| bound.max(key)));
            source.state = SourceState::Exhausted;
            self.refresh = true;
        }

        let name = if exhausted && !source_owned {
            self.names.get(&priority).map(String::as_str)
        } else {
            None
        };

        Some((&mut cursor.data, index, name))
    }

    fn rebuild_heap(&mut self) {
        self.heap.clear();

        // Determine if we're in single-stream mode
        if self.streams.len() == 1 {
            self.single_priority = self.streams.keys().next().copied();
            return;
        }

        self.single_priority = None;

        for (&priority, cursor) in &self.streams {
            if cursor.index < cursor.data.len() {
                self.heap.push(HeapEntry {
                    key: replay_key(
                        cursor
                            .data
                            .get(cursor.index)
                            .expect("index is within the data batch"),
                    ),
                    priority,
                    index: cursor.index,
                });
            }
        }
    }
}

struct SourceCursor {
    source: Box<dyn ReplaySource>,
    bound: Option<ReplayKey>,
    state: SourceState,
}

impl Debug for SourceCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SourceCursor))
            .field("bound", &self.bound)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl SourceCursor {
    fn poll(&mut self, name: &str) -> anyhow::Result<Option<ReplayBatch>> {
        match self.source.poll_batch()? {
            ReplayPoll::Ready(mut data) => {
                anyhow::ensure!(
                    data.len() > 0,
                    "Replay stream '{name}' supplied an empty batch"
                );
                sort_batch(&mut data);
                let first = replay_key(data.get(0).expect("batch is nonempty"));
                anyhow::ensure!(
                    self.bound.is_none_or(|bound| first >= bound),
                    "Replay stream '{name}' moved backward across batches: {first:?} below its frontier {:?}",
                    self.bound
                );
                self.state = SourceState::Buffered;
                Ok(Some(data))
            }
            ReplayPoll::Pending { frontier } => {
                if let Some(frontier) = frontier {
                    anyhow::ensure!(
                        self.bound.is_none_or(|bound| frontier >= bound),
                        "Replay stream '{name}' moved its frontier backward"
                    );
                    self.bound = Some(frontier);
                }

                self.state = SourceState::Pending;
                Ok(None)
            }
            ReplayPoll::Complete => {
                self.state = SourceState::Complete;
                Ok(None)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceState {
    Buffered,
    Exhausted,
    Pending,
    Complete,
}

#[derive(Debug)]
struct ReplayCursor {
    data: ReplayBatch,
    index: usize,
}

/// An opaque, full-width replay ordering key, excluding stream priority.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReplayKey {
    ts: UnixNanos,
    block_number: u64,
    transaction_index: u32,
    log_index: u32,
    phase: u8,
}

impl ReplayKey {
    /// Returns the lowest key at the given timestamp.
    #[must_use]
    pub fn lowest(ts: UnixNanos) -> Self {
        Self {
            ts,
            ..Self::default()
        }
    }

    /// Returns the ordering key of a replay row.
    #[must_use]
    pub fn of(data: DataRef<'_>) -> Self {
        replay_key(data)
    }

    /// Returns the key's replay timestamp.
    #[must_use]
    pub const fn timestamp(self) -> UnixNanos {
        self.ts
    }
}

fn replay_key(data: DataRef<'_>) -> ReplayKey {
    let ts = data.ts_init();

    let (block_number, transaction_index, log_index, phase) = match data {
        DataRef::Instrument(_)
        | DataRef::BookDelta(_)
        | DataRef::BookDeltas(_)
        | DataRef::BookDepth(_)
        | DataRef::Quote(_)
        | DataRef::Trade(_)
        | DataRef::Bar(_)
        | DataRef::MarkPrice(_)
        | DataRef::IndexPrice(_)
        | DataRef::FundingRate(_)
        | DataRef::OptionGreeks(_)
        | DataRef::InstrumentStatus(_)
        | DataRef::InstrumentClose(_)
        | DataRef::Custom(_) => (0, 0, 0, 0),
        #[cfg(feature = "defi")]
        DataRef::Defi(defi) => replay_position(defi),
        #[cfg(not(feature = "defi"))]
        #[allow(
            unreachable_patterns,
            reason = "DeFi variants can exist without this crate's defi feature"
        )]
        _ => (0, 0, 0, 0),
    };

    ReplayKey {
        ts,
        block_number,
        transaction_index,
        log_index,
        phase,
    }
}

fn sort_by_replay_key(batch: &mut DataBatch) {
    match batch {
        DataBatch::Instrument(data) => {
            sort_view_by_replay_key(data, |item| DataRef::Instrument(item));
        }
        DataBatch::Custom(data) => sort_view_by_replay_key(data, |item| DataRef::Custom(item)),
        DataBatch::BookDelta(data) => {
            sort_view_by_replay_key(data, |item| DataRef::BookDelta(item));
        }
        DataBatch::BookDeltas(data) => {
            sort_view_by_replay_key(data, |item| DataRef::BookDeltas(item));
        }
        DataBatch::BookDepth(data) => {
            sort_view_by_replay_key(data, |item| DataRef::BookDepth(item));
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
        #[cfg(not(feature = "defi"))]
        #[allow(
            unreachable_patterns,
            reason = "DeFi variants can exist without this crate's defi feature"
        )]
        _ => {}
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
            .reverse() // BinaryHeap is max by default -> reverse for min behavior
    }
}

impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn sort_batch(batch: &mut ReplayBatch) {
    match batch {
        ReplayBatch::Typed(data) => sort_by_replay_key(data),
        ReplayBatch::Compatibility(data) => {
            sort_view_by_replay_key(data, |item| DataRef::from(item));
        }
        ReplayBatch::BookDeltas { .. } => {}
    }
}

#[cfg(feature = "defi")]
fn replay_position(data: &DefiData) -> (u64, u32, u32, u8) {
    let (block_number, transaction_index, log_index) = data.block_position();
    (
        block_number,
        transaction_index,
        log_index,
        replay_phase(data),
    )
}

#[cfg(feature = "defi")]
const fn replay_phase(data: &DefiData) -> u8 {
    match data {
        DefiData::Block(_) => 0,
        DefiData::Pool(_) => 1,
        DefiData::PoolSnapshot(_) => 2,
        DefiData::PoolSwap(_)
        | DefiData::PoolLiquidityUpdate(_)
        | DefiData::PoolFeeCollect(_)
        | DefiData::PoolFeeProtocolUpdate(_)
        | DefiData::PoolFeeProtocolCollect(_)
        | DefiData::PoolFlash(_) => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc, sync::Arc};

    use nautilus_model::{
        data::{
            Bar, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
            MarkPriceUpdate, OptionGreeks, OrderBookDelta, OrderBookDeltas, OrderBookDepth,
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

    fn ready<T>(poll: anyhow::Result<ReplayPoll<T>>) -> T {
        match poll.unwrap() {
            ReplayPoll::Ready(data) => data,
            _ => panic!("expected ready replay data"),
        }
    }

    #[derive(Clone)]
    enum Step {
        Batch(Vec<u64>),
        Pending(Option<u64>),
        Complete,
        Fail,
        #[cfg(feature = "defi")]
        Frontier(ReplayKey),
    }

    struct ScriptSource {
        steps: std::collections::VecDeque<Step>,
        original: Vec<Step>,
        polls: Rc<Cell<usize>>,
        reset_fails: Rc<Cell<bool>>,
    }

    impl ScriptSource {
        fn new(steps: Vec<Step>) -> Self {
            Self {
                steps: steps.clone().into(),
                original: steps,
                polls: Rc::new(Cell::new(0)),
                reset_fails: Rc::new(Cell::new(false)),
            }
        }
    }

    impl ReplaySource for ScriptSource {
        fn poll_batch(&mut self) -> anyhow::Result<ReplayPoll<ReplayBatch>> {
            self.polls.set(self.polls.get() + 1);

            match self.steps.pop_front().expect("unexpected source poll") {
                Step::Batch(times) => Ok(ReplayPoll::Ready(ReplayBatch::from_data(
                    times
                        .into_iter()
                        .map(|ts| quote("SOURCE.SIM", ts))
                        .collect(),
                ))),
                Step::Pending(ts) => Ok(ReplayPoll::Pending {
                    frontier: ts.map(|ts| ReplayKey::lowest(ts.into())),
                }),
                #[cfg(feature = "defi")]
                Step::Frontier(frontier) => Ok(ReplayPoll::Pending {
                    frontier: Some(frontier),
                }),
                Step::Complete => Ok(ReplayPoll::Complete),
                Step::Fail => anyhow::bail!("scripted source failure"),
            }
        }

        fn reset(&mut self) -> anyhow::Result<()> {
            anyhow::ensure!(!self.reset_fails.get(), "scripted reset failure");
            self.steps = self.original.clone().into();
            Ok(())
        }
    }

    #[rstest]
    #[case(None, false)]
    #[case(Some(10), false)]
    #[case(Some(11), true)]
    fn test_source_pending_gates_ready_rows(#[case] bound: Option<u64>, #[case] emits: bool) {
        let source = ScriptSource::new(vec![Step::Pending(bound), Step::Pending(bound)]);
        let polls = Rc::clone(&source.polls);
        let mut it = DataIterator::new();
        it.add_data("static", vec![quote("STATIC.SIM", 10)], true);
        it.add_source("source", Box::new(source), false).unwrap();
        let result = it.next().unwrap();

        if emits {
            assert_eq!(result, ReplayPoll::Ready(quote("STATIC.SIM", 10)));
            assert_eq!(polls.get(), 1);
        } else {
            assert_eq!(
                result,
                ReplayPoll::Pending {
                    frontier: bound.map(|ts| ReplayKey::lowest(ts.into())),
                }
            );
            assert_eq!(polls.get(), 2);
        }

        assert!(!it.is_done());
    }

    #[rstest]
    fn test_repeated_ready_peek_and_advance_do_not_poll_pending_source() {
        let source = ScriptSource::new(vec![Step::Pending(Some(11))]);
        let polls = Rc::clone(&source.polls);
        let mut it = DataIterator::new();
        it.add_data("static", vec![quote("STATIC.SIM", 10)], true);
        it.add_source("source", Box::new(source), true).unwrap();
        let first = ready(it.peek()).to_owned_data();
        let second = ready(it.peek()).to_owned_data();
        it.advance();

        assert_eq!(first, quote("STATIC.SIM", 10));
        assert_eq!(second, quote("STATIC.SIM", 10));
        assert_eq!(polls.get(), 1);
        assert!(!it.is_done());
    }

    #[rstest]
    #[case(Some(11), Some(13), Some(11))]
    #[case(Some(13), Some(11), Some(11))]
    #[case(Some(11), None, None)]
    #[case(None, Some(11), None)]
    fn test_sources_combine_pending_frontiers(
        #[case] first: Option<u64>,
        #[case] second: Option<u64>,
        #[case] frontier: Option<u64>,
    ) {
        let mut it = DataIterator::new();
        it.add_data("static", vec![quote("STATIC.SIM", 12)], true);
        for (name, bound) in [("first", first), ("second", second)] {
            it.add_source(
                name,
                Box::new(ScriptSource::new(vec![
                    Step::Pending(bound),
                    Step::Pending(bound),
                ])),
                true,
            )
            .unwrap();
        }

        let result = it.next().unwrap();

        assert_eq!(
            result,
            ReplayPoll::Pending {
                frontier: frontier.map(|ts| ReplayKey::lowest(ts.into())),
            }
        );
        assert!(!it.is_done());
    }

    #[rstest]
    #[case(10, false)]
    #[case(12, true)]
    fn test_source_retains_frontier_through_unknown(#[case] resumed: u64, #[case] valid: bool) {
        let mut it = DataIterator::new();
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![
                Step::Pending(Some(11)),
                Step::Pending(None),
                Step::Batch(vec![resumed]),
            ])),
            true,
        )
        .unwrap();
        let waiting = it.next().unwrap();
        let result = it.next();

        assert_eq!(
            waiting,
            ReplayPoll::Pending {
                frontier: Some(ReplayKey::lowest(11.into()))
            }
        );

        if valid {
            assert_eq!(result.unwrap(), ReplayPoll::Ready(quote("SOURCE.SIM", 12)));
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("below its frontier")
            );
        }
    }

    #[rstest]
    #[case(Step::Fail, "scripted source failure")]
    #[case(Step::Batch(vec![]), "empty batch")]
    #[case(Step::Batch(vec![9]), "moved backward")]
    #[case(Step::Pending(Some(9)), "frontier backward")]
    fn test_source_failures_latch_after_delivering_selected_row(
        #[case] step: Step,
        #[case] message: &str,
    ) {
        let source = ScriptSource::new(vec![Step::Batch(vec![10]), step]);
        let polls = Rc::clone(&source.polls);
        let mut it = DataIterator::new();
        it.add_source("source", Box::new(source), true).unwrap();
        let first = ready(it.next());
        let before_refill = polls.get();
        let first_failure = it.next().unwrap_err();
        let repeated_failure = it.next().unwrap_err();
        it.advance();

        assert_eq!(first, quote("SOURCE.SIM", 10));
        assert_eq!(before_refill, 1);
        assert!(first_failure.to_string().contains(message));
        assert!(repeated_failure.to_string().contains(message));
        assert_eq!(polls.get(), 2);
        assert!(!it.is_done());
    }

    #[rstest]
    fn test_source_initial_failure_preserves_existing_stream() {
        let mut it = DataIterator::new();
        it.add_data("source", vec![quote("STATIC.SIM", 7)], false);
        let failure = it
            .add_source(
                "source",
                Box::new(ScriptSource::new(vec![Step::Fail])),
                true,
            )
            .unwrap_err();
        let retained = ready(it.next());

        assert_eq!(failure.to_string(), "scripted source failure");
        assert_eq!(retained, quote("STATIC.SIM", 7));
        assert!(it.is_done());
    }

    #[rstest]
    fn test_source_completion_and_reset() {
        let mut it = DataIterator::new();
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![
                Step::Batch(vec![3, 5]),
                Step::Complete,
            ])),
            true,
        )
        .unwrap();
        let first = collect_ts(&mut it);
        assert!(it.is_done());
        it.reset_all_cursors().unwrap();
        assert!(!it.is_done());
        let replayed = collect_ts(&mut it);

        assert_eq!(first, vec![3, 5]);
        assert_eq!(replayed, first);
        assert!(it.is_done());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_empty_replay_is_complete(#[case] register_source: bool) {
        let mut it = DataIterator::new();
        if register_source {
            it.add_source(
                "empty",
                Box::new(ScriptSource::new(vec![Step::Complete])),
                true,
            )
            .unwrap();
        }

        let result = it.next().unwrap();

        assert_eq!(result, ReplayPoll::Complete);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_source_mutation_guard_survives_cancellation() {
        let mut it = DataIterator::new();
        it.add_data("static", vec![quote("STATIC.SIM", 20)], true);
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![Step::Batch(vec![10])])),
            true,
        )
        .unwrap();
        let first = ready(it.next());
        it.remove_data("source", true);
        it.add_data("late", vec![quote("LATE.SIM", 1)], true);
        let failure = it.next().unwrap_err();
        assert!(!it.is_done());
        it.reset_all_cursors().unwrap();
        let reset = collect_ts(&mut it);

        assert_eq!(first, quote("SOURCE.SIM", 10));
        assert!(failure.to_string().contains("mutation"));
        assert_eq!(reset, vec![1, 20]);
    }

    #[rstest]
    fn test_source_admission_and_refill_ownership() {
        let mut it = DataIterator::new();
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![
                Step::Batch(vec![10]),
                Step::Complete,
            ])),
            true,
        )
        .unwrap();
        let refill = it
            .refill_batch(
                "source",
                ReplayBatch::from_data(vec![quote("LATE.SIM", 12)]),
            )
            .unwrap_err();
        let first = ready(it.next());
        let admission = it
            .add_source(
                "late",
                Box::new(ScriptSource::new(vec![Step::Complete])),
                true,
            )
            .unwrap_err();
        let result = it.next().unwrap();

        assert!(refill.to_string().contains("source-owned"));
        assert_eq!(first, quote("SOURCE.SIM", 10));
        assert!(admission.to_string().contains("admission"));
        assert_eq!(result, ReplayPoll::Complete);
    }

    #[rstest]
    fn test_caller_managed_refill_rejected_during_source_replay() {
        let mut it = DataIterator::new();
        it.add_data("static", vec![quote("STATIC.SIM", 1)], true);
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![
                Step::Batch(vec![2]),
                Step::Complete,
            ])),
            true,
        )
        .unwrap();
        let first = ready(it.next());
        let refill = it
            .refill_batch(
                "static",
                ReplayBatch::from_data(vec![quote("STATIC.SIM", 3)]),
            )
            .unwrap_err();
        let second = ready(it.next());
        let result = it.next().unwrap();

        assert_eq!(first, quote("STATIC.SIM", 1));
        assert_eq!(
            refill.to_string(),
            "Caller-managed refill during source replay requires a reset"
        );
        assert_eq!(second, quote("SOURCE.SIM", 2));
        assert_eq!(result, ReplayPoll::Complete);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_source_reset_failure_blocks_consumption() {
        let source = ScriptSource::new(vec![Step::Batch(vec![10]), Step::Complete]);
        source.reset_fails.set(true);
        let polls = Rc::clone(&source.polls);
        let mut it = DataIterator::new();
        it.add_source("source", Box::new(source), true).unwrap();
        let failure = it.reset_all_cursors().unwrap_err();
        let selection = it.next().unwrap_err();

        assert_eq!(failure.to_string(), "scripted reset failure");
        assert!(selection.to_string().contains("scripted reset failure"));
        assert_eq!(polls.get(), 1);
        assert!(!it.is_done());
    }

    #[rstest]
    fn test_source_reset_retry_recovers_after_partial_reset() {
        let first = ScriptSource::new(vec![Step::Batch(vec![1]), Step::Complete]);
        let second = ScriptSource::new(vec![Step::Batch(vec![2]), Step::Complete]);
        let first_polls = Rc::clone(&first.polls);
        let second_polls = Rc::clone(&second.polls);
        let reset_fails = Rc::clone(&second.reset_fails);
        let mut it = DataIterator::new();
        it.add_source("first", Box::new(first), true).unwrap();
        it.add_source("second", Box::new(second), true).unwrap();
        let initial = collect_ts(&mut it);
        reset_fails.set(true);
        let failure = it.reset_all_cursors().unwrap_err();
        let selection = it.next().unwrap_err();

        assert_eq!(initial, vec![1, 2]);
        assert_eq!(failure.to_string(), "scripted reset failure");
        assert_eq!(
            selection.to_string(),
            "Replay requires a successful reset after failure: scripted reset failure"
        );
        assert_eq!(first_polls.get(), 2);
        assert_eq!(second_polls.get(), 2);
        assert!(!it.is_done());

        reset_fails.set(false);
        it.reset_all_cursors().unwrap();
        let replayed = collect_ts(&mut it);

        assert_eq!(replayed, vec![1, 2]);
        assert_eq!(first_polls.get(), 4);
        assert_eq!(second_polls.get(), 4);
        assert!(it.is_done());
    }

    #[rstest]
    #[case(Step::Pending(None), false)]
    #[case(Step::Complete, false)]
    #[case(Step::Pending(None), true)]
    #[case(Step::Complete, true)]
    fn test_removing_unbuffered_source_preserves_single_stream(
        #[case] removed: Step,
        #[case] retained_source: bool,
    ) {
        let mut it = DataIterator::new();
        let expected = if retained_source {
            it.add_source(
                "retained",
                Box::new(ScriptSource::new(vec![
                    Step::Batch(vec![10]),
                    Step::Complete,
                ])),
                true,
            )
            .unwrap();
            quote("SOURCE.SIM", 10)
        } else {
            let expected = quote("STATIC.SIM", 10);
            it.add_data("retained", vec![expected.clone()], true);
            expected
        };

        it.add_source("removed", Box::new(ScriptSource::new(vec![removed])), false)
            .unwrap();
        it.remove_data("removed", true);
        let done_before_selection = it.is_done();
        let first = ready(it.next());
        let complete = it.next().unwrap();

        assert!(!done_before_selection);
        assert_eq!(first, expected);
        assert_eq!(complete, ReplayPoll::Complete);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_removing_unbuffered_source_after_completion_preserves_single_stream() {
        let mut it = DataIterator::new();
        it.add_data("retained", vec![quote("STATIC.SIM", 10)], true);
        it.add_source(
            "removed",
            Box::new(ScriptSource::new(vec![
                Step::Batch(vec![1]),
                Step::Complete,
            ])),
            false,
        )
        .unwrap();
        let consumed = ready(it.next());
        let prepared = ready(it.peek()).to_owned_data();
        it.remove_data("removed", true);
        let retained = ready(it.next());

        assert_eq!(consumed, quote("SOURCE.SIM", 1));
        assert_eq!(prepared, quote("STATIC.SIM", 10));
        assert_eq!(retained, prepared);
        assert!(it.is_done());
    }

    struct EventSource(Option<ReplayBatch>);

    impl ReplaySource for EventSource {
        fn poll_batch(&mut self) -> anyhow::Result<ReplayPoll<ReplayBatch>> {
            Ok(self
                .0
                .take()
                .map_or(ReplayPoll::Complete, ReplayPoll::Ready))
        }
    }

    #[rstest]
    fn test_source_moves_book_frame_and_reports_unsupported_reset() {
        let frame = Box::new(stub_deltas());
        let expected = frame.clone();
        let frame_ptr = std::ptr::from_ref(frame.as_ref());
        let rows_ptr = frame.deltas.as_ptr();
        let mut it = DataIterator::new();
        it.add_source(
            "book",
            Box::new(EventSource(Some(ReplayBatch::from_book_deltas(frame)))),
            true,
        )
        .unwrap();
        let (data, refill) = ready(it.next_stream_item());
        let refill = refill.map(str::to_owned);

        let ReplayPoll::Complete = it.next().unwrap() else {
            panic!("expected source completion");
        };

        let failure = it.reset_all_cursors().unwrap_err();
        let repeated = it.next().unwrap_err();

        let Data::BookDeltas(frame) = data else {
            panic!("expected owned book frame");
        };

        assert_eq!(frame, expected);
        assert_eq!(std::ptr::from_ref(frame.as_ref()), frame_ptr);
        assert_eq!(frame.deltas.as_ptr(), rows_ptr);
        assert_eq!(refill, None);
        assert_eq!(failure.to_string(), "Replay source reset is unsupported");
        assert!(
            repeated
                .to_string()
                .contains("Replay source reset is unsupported")
        );
        assert!(!it.is_done());
    }

    #[rstest]
    fn test_static_moved_frame_reset_error_preserves_sources() {
        let source = ScriptSource::new(vec![Step::Batch(vec![10]), Step::Complete]);
        source.reset_fails.set(true);
        let mut it = DataIterator::new();
        let mut frame = Box::new(stub_deltas());
        frame.ts_init = 1.into();
        it.add_batch("book", ReplayBatch::from_book_deltas(frame), true);
        it.add_source("source", Box::new(source), true).unwrap();
        let _ = ready(it.next_stream_item());
        let failure = it.reset_all_cursors().unwrap_err();
        let retained = ready(it.next());

        assert_eq!(
            failure.to_string(),
            "Cannot reset a consumed static book frame"
        );
        assert_eq!(retained, quote("SOURCE.SIM", 10));
    }

    #[rstest]
    fn test_successful_reset_recovers_latched_source_failure() {
        let mut it = DataIterator::new();
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![Step::Batch(vec![10]), Step::Fail])),
            true,
        )
        .unwrap();
        let first = ready(it.next());
        let failure = it.next().unwrap_err();
        it.reset_all_cursors().unwrap();
        let replayed = ready(it.next());

        assert_eq!(first, quote("SOURCE.SIM", 10));
        assert_eq!(failure.to_string(), "scripted source failure");
        assert_eq!(replayed, first);
    }

    #[rstest]
    fn test_source_cursor_changes_cannot_report_empty_success() {
        let mut it = DataIterator::new();
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![
                Step::Batch(vec![10]),
                Step::Complete,
            ])),
            true,
        )
        .unwrap();
        it.set_index("source", 1);
        let failure = it.next().unwrap_err();
        it.reset_all_cursors().unwrap();
        let replayed = ready(it.next());

        assert_eq!(
            failure.to_string(),
            "Replay requires a successful reset after failure: Source-owned cursor changes require a reset"
        );
        assert_eq!(replayed, quote("SOURCE.SIM", 10));
    }

    #[rstest]
    fn test_source_completion_releases_pending_barrier() {
        let mut it = DataIterator::new();
        it.add_data("static", vec![quote("STATIC.SIM", 10)], true);
        it.add_source(
            "source",
            Box::new(ScriptSource::new(vec![
                Step::Pending(None),
                Step::Pending(None),
                Step::Complete,
            ])),
            false,
        )
        .unwrap();
        let pending = it.next().unwrap();
        it.advance();
        let resumed = ready(it.next());

        assert_eq!(pending, ReplayPoll::Pending { frontier: None });
        assert_eq!(resumed, quote("STATIC.SIM", 10));
        assert!(it.is_done());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_source_frontier_orders_full_width_defi_keys_at_same_timestamp() {
        let before = defi_snapshot(10, u64::MAX - 1, u32::MAX, u32::MAX);
        let equal = defi_snapshot(10, u64::MAX, u32::MAX, u32::MAX);
        let bound = ReplayKey::of(DataRef::from(&equal));
        let source = ScriptSource::new(vec![Step::Frontier(bound), Step::Frontier(bound)]);
        let polls = Rc::clone(&source.polls);
        let mut it = DataIterator::new();
        it.add_data("static", vec![before.clone(), equal], true);
        it.add_source("source", Box::new(source), false).unwrap();
        let first = ready(it.next());
        let pending = it.next().unwrap();

        assert_eq!(first, before);
        assert_eq!(
            pending,
            ReplayPoll::Pending {
                frontier: Some(bound)
            }
        );
        assert_eq!(bound.timestamp(), UnixNanos::from(10));
        assert_eq!(polls.get(), 2);
    }

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

    fn collect_sequence(it: &mut DataIterator) -> Vec<(InstrumentId, UnixNanos)> {
        let mut sequence = Vec::new();
        while let ReplayPoll::Ready(data) = it.peek().unwrap() {
            sequence.push((data.instrument_id(), data.ts_init()));
            it.advance();
        }

        sequence
    }

    fn collect_ts(it: &mut DataIterator) -> Vec<u64> {
        let mut ts = Vec::new();
        while let ReplayPoll::Ready(d) = it.next().unwrap() {
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
    #[case(true)]
    #[case(false)]
    fn test_refill_preserves_equal_key_stream_priority(#[case] append_data: bool) {
        let mut it = DataIterator::new();
        it.add_data("first", vec![quote("A.B", 10)], append_data);
        it.add_data("second", vec![quote("C.D", 10)], true);
        let (first, exhausted) = ready(it.next_stream_item());
        let exhausted = exhausted.map(str::to_owned);
        it.refill_batch(
            "first",
            ReplayBatch::Typed(DataBatch::Quote(vec![quote_tick("E.F", 10)].into())),
        )
        .unwrap();
        let remaining = collect_sequence(&mut it);

        assert_eq!(first, quote("A.B", 10));
        assert_eq!(exhausted.as_deref(), Some("first"));
        assert_eq!(
            remaining,
            vec![
                (InstrumentId::from("E.F"), UnixNanos::from(10)),
                (InstrumentId::from("C.D"), UnixNanos::from(10)),
            ]
        );
    }

    #[rstest]
    fn test_owned_book_event_moves_backing_and_keeps_refill_ordering() {
        let mut deltas = Box::new(stub_deltas());
        deltas.ts_init = UnixNanos::from(10);
        let expected = deltas.clone();
        let event_ptr = std::ptr::from_ref(deltas.as_ref());
        let rows_ptr = deltas.deltas.as_ptr();
        let mut it = DataIterator::new();
        it.add_batch("source", ReplayBatch::from_book_deltas(deltas), true);
        let (first, exhausted) = ready(it.next_stream_item());
        let exhausted = exhausted.map(str::to_owned);

        let Data::BookDeltas(first) = first else {
            panic!("expected a completed book event");
        };

        let mut backward = expected.clone();
        backward.ts_init = UnixNanos::from(9);
        let failure = it
            .refill_batch("source", ReplayBatch::from_book_deltas(backward))
            .unwrap_err();
        it.refill_batch("source", ReplayBatch::from_book_deltas(expected.clone()))
            .unwrap();
        let (second, second_exhausted) = ready(it.next_stream_item());
        let second_exhausted = second_exhausted.map(str::to_owned);

        assert_eq!(std::ptr::from_ref(first.as_ref()), event_ptr);
        assert_eq!(first.deltas.as_ptr(), rows_ptr);
        assert_eq!(first.instrument_id, expected.instrument_id);
        assert_eq!(first.deltas, expected.deltas);
        assert_eq!(first.flags, expected.flags);
        assert_eq!(first.sequence, expected.sequence);
        assert_eq!(first.ts_event, expected.ts_event);
        assert_eq!(first.ts_init, expected.ts_init);
        assert_eq!(exhausted.as_deref(), Some("source"));
        assert!(
            failure
                .to_string()
                .contains("moved backward across batches")
        );
        assert_eq!(second, Data::BookDeltas(expected));
        assert_eq!(second_exhausted.as_deref(), Some("source"));
        assert!(it.is_done());
        let failure = it.reset_all_cursors().unwrap_err();
        assert_eq!(
            failure.to_string(),
            "Cannot reset a consumed static book frame"
        );
        assert!(matches!(it.peek(), Ok(ReplayPoll::Complete)));
        assert!(it.is_done());
    }

    #[rstest]
    #[case("missing", false, false, "Unknown replay stream 'missing'")]
    #[case(
        "source",
        false,
        false,
        "Replay stream 'source' still has unconsumed data"
    )]
    #[case("source", true, true, "Replay stream 'source' supplied an empty batch")]
    fn test_refill_rejects_invalid_state_without_changing_retained_rows(
        #[case] name: &str,
        #[case] consume: bool,
        #[case] empty: bool,
        #[case] expected_error: &str,
    ) {
        let mut it = DataIterator::new();
        it.add_data("source", vec![quote("A.B", 10)], true);
        if consume {
            it.advance();
        }

        let rows = if empty {
            Vec::new()
        } else {
            vec![quote_tick("C.D", 11)]
        };

        let failure = it
            .refill_batch(name, ReplayBatch::Typed(DataBatch::Quote(rows.into())))
            .unwrap_err();
        let remaining = collect_sequence(&mut it);
        it.reset_all_cursors().unwrap();
        let reset = collect_sequence(&mut it);
        let original = vec![(InstrumentId::from("A.B"), UnixNanos::from(10))];

        assert_eq!(failure.to_string(), expected_error);
        assert_eq!(
            remaining.as_slice(),
            if consume { &[] } else { original.as_slice() }
        );
        assert_eq!(reset, original);
    }

    #[rstest]
    fn test_refill_rejects_backward_batch_without_replacing_consumed_batch() {
        let mut it = DataIterator::new();
        it.add_data("source", vec![quote("A.B", 10)], true);
        it.advance();
        let failure = it
            .refill_batch(
                "source",
                ReplayBatch::Typed(DataBatch::Quote(
                    vec![quote_tick("A.B", 12), quote_tick("A.B", 9)].into(),
                )),
            )
            .unwrap_err();
        it.reset_all_cursors().unwrap();

        assert!(
            failure
                .to_string()
                .contains("moved backward across batches")
        );
        assert_eq!(collect_ts(&mut it), vec![10]);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_refill_rejects_backward_defi_position_at_equal_timestamp() {
        let mut it = DataIterator::new();
        it.add_data("source", vec![defi_snapshot(10, u64::MAX, 3, 5)], true);
        it.advance();
        let failure = it
            .refill_batch(
                "source",
                ReplayBatch::Typed(DataBatch::Defi(
                    vec![defi_pool_snapshot(10, u64::MAX, 3, 4)].into(),
                )),
            )
            .unwrap_err();

        assert!(
            failure
                .to_string()
                .contains("moved backward across batches")
        );
        assert!(it.is_done());
    }

    #[rstest]
    fn test_refill_sorts_batch_stably_without_mutating_shared_backing() {
        let mut it = DataIterator::new();
        it.add_data("source", vec![quote("A.B", 1)], true);
        it.advance();
        let backing = vec![
            quote_tick("C.D", 3),
            quote_tick("E.F", 2),
            quote_tick("G.H", 2),
        ];
        let backing = Arc::new(backing);
        let batch = DataBatch::Quote(BatchView::from(Arc::clone(&backing)));
        it.refill_batch("source", ReplayBatch::Typed(batch))
            .unwrap();

        assert_eq!(
            collect_sequence(&mut it),
            vec![
                (InstrumentId::from("E.F"), UnixNanos::from(2)),
                (InstrumentId::from("G.H"), UnixNanos::from(2)),
                (InstrumentId::from("C.D"), UnixNanos::from(3)),
            ]
        );
        assert_eq!(
            backing.as_ref(),
            &[
                quote_tick("C.D", 3),
                quote_tick("E.F", 2),
                quote_tick("G.H", 2)
            ]
        );
    }

    #[rstest]
    fn test_single_stream_yields_in_order() {
        let mut it = DataIterator::new();
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
        let mut it = DataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 3)], true);
        assert_eq!(ready(it.next()).ts_init(), UnixNanos::from(1));
        assert_eq!(ready(it.next()).ts_init(), UnixNanos::from(3));
        assert!(matches!(it.next(), Ok(ReplayPoll::Complete)));
    }

    #[rstest]
    fn test_peek_does_not_consume_single_stream_item() {
        let mut it = DataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 2)], true);

        assert_eq!(ready(it.peek()).ts_init(), UnixNanos::from(1));
        assert_eq!(ready(it.peek()).ts_init(), UnixNanos::from(1));
        assert_eq!(ready(it.next()).ts_init(), UnixNanos::from(1));
        assert_eq!(ready(it.peek()).ts_init(), UnixNanos::from(2));
    }

    #[rstest]
    fn test_single_stream_sorts_unsorted_input() {
        let mut it = DataIterator::new();
        it.add_data(
            "s",
            vec![quote("A.B", 300), quote("A.B", 100), quote("A.B", 200)],
            true,
        );

        assert_eq!(collect_ts(&mut it), vec![100, 200, 300]);
    }

    #[rstest]
    fn test_two_stream_merge_chronological() {
        let mut it = DataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1), quote("A.B", 4)], true);
        it.add_data("s2", vec![quote("C.D", 2), quote("C.D", 3)], false);

        assert_eq!(collect_ts(&mut it), vec![1, 2, 3, 4]);
    }

    #[rstest]
    fn test_peek_does_not_consume_multi_stream_heap_item() {
        let mut it = DataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1), quote("A.B", 4)], true);
        it.add_data("s2", vec![quote("C.D", 2), quote("C.D", 3)], true);

        assert_eq!(ready(it.peek()).ts_init(), UnixNanos::from(1));
        assert_eq!(ready(it.peek()).ts_init(), UnixNanos::from(1));
        assert_eq!(ready(it.next()).ts_init(), UnixNanos::from(1));
        assert_eq!(ready(it.peek()).ts_init(), UnixNanos::from(2));
        assert_eq!(collect_ts(&mut it), vec![2, 3, 4]);
    }

    #[rstest]
    fn test_three_stream_merge_sorted() {
        let mut it = DataIterator::new();
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
        let mut it = DataIterator::new();
        it.add_data("s1", vec![quote("A.B", 100), quote("A.B", 300)], true);
        it.add_data("s2", vec![quote("C.D", 200), quote("C.D", 400)], true);

        assert_eq!(collect_ts(&mut it), vec![100, 200, 300, 400]);
    }

    #[rstest]
    fn test_append_data_priority_default_fifo() {
        let mut it = DataIterator::new();
        it.add_data("a", vec![quote("A.B", 100)], true);
        it.add_data("b", vec![quote("C.D", 100)], true);

        // Both at same timestamp, FIFO order (a before b)
        let ts = collect_ts(&mut it);
        assert_eq!(ts, vec![100, 100]);
    }

    #[rstest]
    fn test_prepend_priority_wins_ties() {
        let mut it = DataIterator::new();
        // "a" is appended (lower priority), "b" is prepended (higher priority)
        it.add_data("a", vec![quote("A.B", 100)], true);
        it.add_data("b", vec![quote("C.D", 100)], false);

        // "b" (prepend) should come first despite being added second
        let first = ready(it.next());
        let second = ready(it.next());
        // Prepend stream (negative priority) wins ties over append (positive)
        assert_eq!(first.instrument_id(), InstrumentId::from("C.D"));
        assert_eq!(second.instrument_id(), InstrumentId::from("A.B"));
    }

    #[rstest]
    fn test_is_done_empty_iterator() {
        let it = DataIterator::new();
        assert!(it.is_done());
    }

    #[rstest]
    fn test_is_done_after_consumption() {
        let mut it = DataIterator::new();
        it.add_data("s", vec![quote("A.B", 1)], true);

        assert!(!it.is_done());
        it.next().unwrap();
        assert!(it.is_done());
    }

    #[rstest]
    fn test_is_done_multi_stream() {
        let mut it = DataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1)], true);
        it.add_data("s2", vec![quote("C.D", 2)], true);

        assert!(!it.is_done());
        it.next().unwrap();
        assert!(!it.is_done());
        it.next().unwrap();
        assert!(it.is_done());
    }

    #[rstest]
    fn test_partial_consumption_then_complete() {
        let mut it = DataIterator::new();
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

        assert_eq!(ready(it.next()).ts_init().as_u64(), 0);
        assert_eq!(ready(it.next()).ts_init().as_u64(), 1);

        let remaining = collect_ts(&mut it);
        assert_eq!(remaining, vec![2, 3]);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_remove_stream_reduces_output() {
        let mut it = DataIterator::new();
        it.add_data("a", vec![quote("A.B", 1)], true);
        it.add_data("b", vec![quote("C.D", 2)], true);

        it.remove_data("a", false);

        assert_eq!(collect_ts(&mut it), vec![2]);
    }

    #[rstest]
    fn test_remove_all_streams_yields_empty() {
        let mut it = DataIterator::new();
        it.add_data("x", vec![quote("A.B", 1)], true);
        it.add_data("y", vec![quote("C.D", 2)], true);

        it.remove_data("x", false);
        it.remove_data("y", false);

        assert!(matches!(it.next(), Ok(ReplayPoll::Complete)));
        assert!(it.is_done());
    }

    #[rstest]
    fn test_remove_nonexistent_stream_is_noop() {
        let mut it = DataIterator::new();
        it.add_data("s", vec![quote("A.B", 1)], true);

        it.remove_data("nonexistent", false);

        assert_eq!(collect_ts(&mut it), vec![1]);
    }

    #[rstest]
    fn test_remove_after_full_consumption() {
        let mut it = DataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 2)], true);

        collect_ts(&mut it);

        it.remove_data("s", true);
        assert!(it.is_done());
    }

    #[rstest]
    fn test_set_index_rewinds_stream() {
        let mut it = DataIterator::new();
        it.add_data(
            "s",
            vec![quote("A.B", 10), quote("A.B", 20), quote("A.B", 30)],
            true,
        );

        assert_eq!(ready(it.next()).ts_init().as_u64(), 10);

        it.set_index("s", 0);

        assert_eq!(collect_ts(&mut it), vec![10, 20, 30]);
    }

    #[rstest]
    fn test_set_index_skips_forward() {
        let mut it = DataIterator::new();
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
        let mut it = DataIterator::new();
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
        let mut it = DataIterator::new();
        it.add_data("s", vec![quote("A.B", 1)], true);

        it.set_index("nonexistent", 0);

        assert_eq!(collect_ts(&mut it), vec![1]);
    }

    #[rstest]
    fn test_reset_all_cursors_single_stream() {
        let mut it = DataIterator::new();
        it.add_data("s", vec![quote("A.B", 1), quote("A.B", 2)], true);

        collect_ts(&mut it);
        assert!(it.is_done());

        it.reset_all_cursors().unwrap();
        assert!(!it.is_done());
        assert_eq!(collect_ts(&mut it), vec![1, 2]);
    }

    #[rstest]
    fn test_reset_all_cursors_multi_stream() {
        let mut it = DataIterator::new();
        it.add_data("s1", vec![quote("A.B", 1), quote("A.B", 3)], true);
        it.add_data("s2", vec![quote("C.D", 2), quote("C.D", 4)], true);

        collect_ts(&mut it);
        assert!(it.is_done());

        it.reset_all_cursors().unwrap();
        assert_eq!(collect_ts(&mut it), vec![1, 2, 3, 4]);
    }

    #[rstest]
    fn test_readding_data_replaces_stream() {
        let mut it = DataIterator::new();
        it.add_data("X", vec![quote("A.B", 1), quote("A.B", 2)], true);
        it.add_data("X", vec![quote("A.B", 10)], true);

        assert_eq!(collect_ts(&mut it), vec![10]);
    }

    #[rstest]
    fn test_readding_data_reuses_equal_key_stream_priority() {
        let mut it = DataIterator::new();
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
        let mut it = DataIterator::new();
        it.add_data("empty", vec![], true);

        assert!(it.is_done());
        assert!(matches!(it.next(), Ok(ReplayPoll::Complete)));
    }

    #[rstest]
    fn test_empty_iterator_returns_none() {
        let mut it = DataIterator::new();
        assert!(matches!(it.next(), Ok(ReplayPoll::Complete)));
        assert!(it.is_done());
    }

    #[rstest]
    fn test_multiple_add_data_calls_with_different_names() {
        let mut it = DataIterator::new();
        it.add_data("batch_0", vec![quote("A.B", 1), quote("A.B", 3)], true);
        it.add_data("batch_1", vec![quote("A.B", 2), quote("A.B", 4)], true);

        assert_eq!(collect_ts(&mut it), vec![1, 2, 3, 4]);
    }

    #[rstest]
    fn test_typed_and_compatibility_streams_preserve_equal_key_order() {
        let mut single_batch = DataIterator::new();
        single_batch.add_data(
            "single",
            vec![quote("A.B", 10), trade("C.D", 10), quote("E.F", 20)],
            true,
        );

        let mut split_batches = DataIterator::new();
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
        let mut it = DataIterator::new();
        it.add_data("append_a", vec![quote("A.B", 100)], true);
        it.add_data("append_b", vec![quote("C.D", 100)], true);
        it.add_data("prepend", vec![quote("E.F", 100)], false);

        let first = ready(it.next());
        assert_eq!(
            first.instrument_id(),
            InstrumentId::from("E.F"),
            "Prepend stream should always come first in ties"
        );
    }

    #[rstest]
    fn test_equal_timestamps_across_many_streams_preserves_priority_order() {
        // All items at the same timestamp - ordering is strictly by priority
        let mut it = DataIterator::new();
        it.add_data("s1", vec![quote("A.B", 50)], true);
        it.add_data("s2", vec![quote("C.D", 50)], true);
        it.add_data("s3", vec![quote("E.F", 50)], true);
        it.add_data("s4", vec![quote("G.H", 50)], true);

        let mut ids = Vec::new();
        while let ReplayPoll::Ready(d) = it.next().unwrap() {
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
                OrderBookDepth {
                    ts_init: late,
                    ..stub_depth10()
                },
                OrderBookDepth {
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
            let mut it = DataIterator::new();
            it.add_data_batch("typed", batch, true);

            assert_eq!(collect_ts(&mut it), vec![100, 200]);
        }
    }

    #[rstest]
    fn test_add_data_batch_shares_sorted_backing_allocation() {
        let data = Arc::new(vec![quote_tick("A.B", 100), quote_tick("A.B", 200)]);
        let mut it = DataIterator::new();

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
        let data = Arc::new(vec![
            quote_tick("A.B", 50),
            quote_tick("A.B", 200),
            quote_tick("A.B", 100),
            quote_tick("A.B", 250),
        ]);
        let mut it = DataIterator::new();

        it.add_data_batch(
            "typed",
            DataBatch::Quote(BatchView::new(Arc::clone(&data), 1..3)),
            true,
        );

        assert_eq!(Arc::strong_count(&data), 1);
        assert_eq!(
            data.iter()
                .map(|row| row.ts_init.as_u64())
                .collect::<Vec<_>>(),
            vec![50, 200, 100, 250]
        );
        assert_eq!(collect_ts(&mut it), vec![100, 200]);
    }

    #[rstest]
    fn test_add_empty_data_batch_is_noop() {
        let mut it = DataIterator::new();
        it.add_data_batch("typed", DataBatch::from(Vec::<QuoteTick>::new()), true);

        assert!(it.is_done());
        assert!(matches!(it.peek(), Ok(ReplayPoll::Complete)));
    }

    #[rstest]
    fn test_typed_batch_and_legacy_streams_preserve_equal_key_order() {
        let mut single_batch = DataIterator::new();
        single_batch.add_data(
            "single",
            vec![quote("A.B", 10), trade("C.D", 10), quote("E.F", 20)],
            true,
        );

        let mut split_streams = DataIterator::new();
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
        let mut it = DataIterator::new();
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
        while let ReplayPoll::Ready(Data::Defi(data)) = it.next().unwrap() {
            positions.push(data.block_position());
        }

        assert_eq!(positions, vec![(11, 9, 9), (12, 2, 7), (12, 4, 1)]);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_defi_data_orders_equal_timestamps_by_block_position() {
        let mut it = DataIterator::new();
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
        while let ReplayPoll::Ready(Data::Defi(data)) = it.next().unwrap() {
            positions.push(data.block_position());
        }

        assert_eq!(positions, vec![(11, 9, 9), (12, 2, 7), (12, 4, 1)]);
    }
}
