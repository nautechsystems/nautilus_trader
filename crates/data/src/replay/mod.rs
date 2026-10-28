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

//! Ordered batch consumption shared by historical replay callers.
//!
//! [`DataIterator`] merges static batches, caller-managed refills, and [`ReplaySource`] batches.
//! It retains one active batch per source. Source polling is synchronous and bounded per selection.
//! Callers arrange wakeups, cancellation of external work, timestamp-aligned chunks, and finalization.
//!
//! # Ordering and Pending Input
//!
//! Row selection returns `Result<ReplayPoll<_>>`. [`ReplayPoll::Pending`] is unfinished input, not
//! exhaustion. Unknown bounds block emission; known bounds permit only rows strictly below them.
//! Equal keys wait to preserve stream priority. Sources may omit a new bound without revoking an
//! earlier promise, but a known bound cannot move backward.
//!
//! A pending bound at timestamp `t` does not complete `t`: timers may advance strictly before it,
//! never at or beyond it.
//!
//! # Source Lifecycle and Errors
//!
//! Source registration polls once before insertion; a first-poll error leaves existing streams
//! unchanged. Later source errors fault the iterator until a successful reset; they never become
//! successful completion. Consumption returns its selected row before polling a replacement batch.
//! Removing a stream drops its source. Completed sources remain registered for reset.
//!
//! # Mutation and Refills
//!
//! Source registration requires a reset after consumption starts; removal remains permitted.
//! Static insertion, replacement, and cursor changes during a source-backed run fault the next
//! selection, including after the last source is removed. Source-owned cursors cannot be
//! changed directly. Static-only replay permits stream and cursor changes.
//!
//! Caller-managed refill is available before source-backed consumption starts, and throughout
//! static-only replay. Callers must refill an exhausted caller-managed batch before selecting
//! another row.
//!
//! # Reset
//!
//! Reset rewinds retained static batches and every registered source. A static book frame moved
//! by `next_stream_item` cannot be rewound and fails reset before changing state. A source
//! reset failure faults replay; sources must support another reset attempt. A successful reset
//! clears bounds and faults, and defers source loading until selection. External caller-managed
//! loaders retain reset ownership.

mod batch;
mod iterator;

pub use batch::ReplayBatch;
pub use iterator::{DataIterator, ReplayKey, ReplayPoll, ReplaySource};
