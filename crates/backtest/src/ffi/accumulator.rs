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

use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
};

use nautilus_common::ffi::{clock::TestClock_API, timer::TimeEventHandler_API};
use nautilus_core::{UnixNanos, ffi::parsing::u8_as_bool};

use crate::accumulator::TimeEventAccumulator;

/// FFI wrapper for [`TimeEventAccumulator`].
#[repr(C)]
#[allow(non_camel_case_types)]
pub struct TimeEventAccumulator_API(Box<TimeEventAccumulator>);

impl Deref for TimeEventAccumulator_API {
    type Target = TimeEventAccumulator;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Debug for TimeEventAccumulator_API {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TimeEventAccumulator_API({:p})", &*self.0)
    }
}

impl DerefMut for TimeEventAccumulator_API {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Creates a new [`TimeEventAccumulator_API`] instance.
#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_new() -> TimeEventAccumulator_API {
    TimeEventAccumulator_API(Box::default())
}

/// Drops a [`TimeEventAccumulator_API`] instance.
#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_drop(accumulator: TimeEventAccumulator_API) {
    drop(accumulator);
}

/// Advance the clock and push events to the heap.
#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_advance_clock(
    accumulator: &mut TimeEventAccumulator_API,
    clock: &mut TestClock_API,
    to_time_ns: UnixNanos,
    set_time: u8,
) {
    accumulator.advance_clock(clock, to_time_ns, u8_as_bool(set_time));
}

/// Peek at the next event timestamp.
///
/// Returns `u64::MAX` if the heap is empty.
#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_peek_next_time(
    accumulator: &TimeEventAccumulator_API,
) -> u64 {
    accumulator
        .peek_next_time()
        .map_or(u64::MAX, |t| t.as_u64())
}

/// Pop the next event if its timestamp is at or before `ts`.
///
/// Returns a handler with `callback_ptr = NULL` if no event is available.
#[unsafe(no_mangle)]
pub extern "C" fn time_event_accumulator_pop_next_at_or_before(
    accumulator: &mut TimeEventAccumulator_API,
    ts: UnixNanos,
) -> TimeEventHandler_API {
    accumulator
        .pop_next_at_or_before(ts)
        .map_or_else(TimeEventHandler_API::null, Into::into)
}
