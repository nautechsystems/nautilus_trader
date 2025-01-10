// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::{
    events::{PositionChanged, PositionClosed, PositionOpened},
    identifiers::{AccountId, InstrumentId},
};
pub mod changed;
pub mod closed;
pub mod opened;
pub mod snapshot;

pub enum PositionEvent {
    PositionOpened(PositionOpened),
    PositionChanged(PositionChanged),
    PositionClosed(PositionClosed),
}

impl PositionEvent {
    pub fn instrument_id(&self) -> InstrumentId {
        match self {
            PositionEvent::PositionOpened(position) => position.instrument_id,
            PositionEvent::PositionChanged(position) => position.instrument_id,
            PositionEvent::PositionClosed(position) => position.instrument_id,
        }
    }

    pub fn account_id(&self) -> AccountId {
        match self {
            PositionEvent::PositionOpened(position) => position.account_id,
            PositionEvent::PositionChanged(position) => position.account_id,
            PositionEvent::PositionClosed(position) => position.account_id,
        }
    }
}
