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

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use nautilus_common::messages::data::{SubscribeCommand, UnsubscribeCommand};
use nautilus_model::identifiers::OptionSeriesId;

/// Deferred subscribe/unsubscribe command.
///
/// Components that lack direct `DataClientAdapter` access (handlers, timers)
/// push commands here; the `DataEngine` drains on each data tick.
#[derive(Debug, Clone)]
pub(crate) enum DeferredCommand {
    Subscribe(SubscribeCommand),
    Unsubscribe(UnsubscribeCommand),
    ExpireSeries(OptionSeriesId),
}

/// Shared queue for deferred subscribe/unsubscribe commands.
pub(crate) type DeferredCommandQueue = Rc<RefCell<VecDeque<DeferredCommand>>>;
