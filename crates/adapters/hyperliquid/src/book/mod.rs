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

//! Order book synchronization and recovery for Hyperliquid.
//!
//! - [`sync`] owns snapshot tracking and recovery state transitions.
//! - [`recovery`] owns subscription and recovery tasks, using the shared retry runner.
//!
//! Every `l2Book` frame carries a complete snapshot of the aggregated top levels with no sequence
//! number, so the tracker accepts each frame as a snapshot and validates no linkage. The data
//! client routes delta frames through the tracker and starts recovery for missing initial or
//! post-reconnect snapshots, invalid frames, and stale streams when the stream health monitor's
//! recovery is enabled. Outcome types come from [`nautilus_live::book`].

pub(crate) mod recovery;
pub(crate) mod sync;

pub(crate) use nautilus_live::book::{BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind};
