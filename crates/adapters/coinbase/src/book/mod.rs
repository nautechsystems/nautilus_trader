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

//! Order book synchronization and recovery for Coinbase.
//!
//! - [`sync`] owns snapshot tracking, gap marking, and recovery state transitions.
//! - [`recovery`] owns recovery tasks and replacement subscriptions, using the shared retry runner.
//!
//! Coinbase numbers envelopes with one `sequence_num` per connection, across every channel and
//! product, and its `level2` events carry no per-book sequence. The feed handler reports a skipped
//! envelope as a connection gap, which marks every subscribed book, since the lost envelope could
//! have belonged to any of them. Each book then recovers on its own: an unsubscribe and subscribe
//! pair brings a fresh snapshot, and only that book's snapshot resumes its output.
//!
//! The data client routes book events through the tracker and starts recovery when needed.
//! Outcome types come from [`nautilus_live::book`].

pub(crate) mod recovery;
pub(crate) mod sync;

pub(crate) use nautilus_live::book::{BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind};
