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

//! Order emulation components for simulating order execution behavior.

use nautilus_common::messages::execution::TradingCommand;
use nautilus_model::events::OrderEventAny;

pub mod adapter;
pub mod config;
pub mod emulator;
pub mod handlers;

/// A message deferred while the emulator was already handling another call,
/// drained once the active call completes (msgbus dispatches synchronously,
/// so events the emulator publishes during handling would otherwise be dropped
/// or panic on the reentrant borrow).
#[derive(Debug)]
pub(crate) enum PendingMessage {
    Command(Box<TradingCommand>),
    Event(Box<OrderEventAny>),
}
