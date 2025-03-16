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

use std::ops::{Deref, DerefMut};

use nautilus_common::ffi::{clock::TestClock_API, timer::TimeEventHandler};
use nautilus_core::{
    UnixNanos,
    ffi::{cvec::CVec, parsing::u8_as_bool},
};

use crate::accumulator::TimeEventAccumulator;

#[repr(C)]
pub struct TimeEventAccumulatorAPI(Box<TimeEventAccumulator>);

impl Deref for TimeEventAccumulatorAPI {
    type Target = TimeEventAccumulator;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TimeEventAccumulatorAPI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_new() -> TimeEventAccumulatorAPI {
    TimeEventAccumulatorAPI(Box::default())
}

#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_drop(accumulator: TimeEventAccumulatorAPI) {
    drop(accumulator); // Memory freed here
}

#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_advance_clock(
    accumulator: &mut TimeEventAccumulatorAPI,
    clock: &mut TestClock_API,
    to_time_ns: UnixNanos,
    set_time: u8,
) {
    accumulator.advance_clock(clock, to_time_ns, u8_as_bool(set_time));
}

#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_drain(accumulator: &mut TimeEventAccumulatorAPI) -> CVec {
    let handlers: Vec<TimeEventHandler> = accumulator.drain().into_iter().map(Into::into).collect();
    handlers.into()
}
