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

//! Order book synchronization and recovery for AX Exchange.
//!
//! - [`sync`] owns snapshot tracking and recovery state transitions.
//! - [`recovery`] owns subscription and recovery tasks, using the shared retry runner.
//!
//! Every L2 and L3 frame carries a complete snapshot with no sequence number, and AX sends one
//! right after each subscribe acknowledgement, so the tracker accepts each frame as a snapshot and
//! validates no linkage. The data client routes book frames through the tracker and starts recovery
//! for missing initial or post-reconnect snapshots and invalid frames. Outcome types come from
//! [`nautilus_live::book`].

pub(crate) mod recovery;
pub(crate) mod sync;

pub(crate) use nautilus_live::book::{BookSequenceOutcome, BookSyncSignal, BookSyncSignalKind};
