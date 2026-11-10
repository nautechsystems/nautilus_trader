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

//! Book synchronization and recovery for Deribit order books.
//!
//! Raw delta channels sequence by `change_id`/`prev_change_id` per instrument, while grouped
//! channels carry snapshot-only books keyed by exact channel. The WebSocket feed handler owns
//! the [`sync::BookSyncTracker`] and polls recovery work alongside commands and frames.

pub(crate) mod recovery;
pub(crate) mod sync;
