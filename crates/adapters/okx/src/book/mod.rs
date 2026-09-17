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

//! Order book synchronization and recovery for OKX.
//!
//! - [`sync`] owns sequence validation, snapshot tracking, and recovery state transitions.
//! - [`recovery`] owns recovery tasks and replacement subscriptions, using the shared retry runner.
//!
//! The data client routes book events through the tracker and starts recovery when needed.
//! Recovery tasks use the tracker to claim ownership and report failure; incoming snapshots
//! complete recovery through the same tracker. Outcome types come from [`nautilus_live::book`];
//! [`BookChannelScope`] keeps public and business socket routing local to OKX.

pub(crate) mod recovery;
pub(crate) mod sync;

use nautilus_live::book::recovery::BookRecoveryOutcome as RecoveryOutcome;
pub(crate) use nautilus_live::book::{BookSequenceOutcome, BookSyncSignalKind};

use crate::websocket::error::OKXWsError;

pub(crate) type BookRecoveryOutcome = RecoveryOutcome<OKXWsError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BookChannelScope {
    Public,
    Business,
}
