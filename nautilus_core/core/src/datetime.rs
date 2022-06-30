// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use chrono::prelude::{DateTime, Utc};
use chrono::{Datelike, Timelike};
use std::time::{UNIX_EPOCH, Duration};

const NANOSECONDS_IN_SECOND: u64 = 1_000_000_000;
const NANOSECONDS_IN_MILLISECOND: u64 = 1_000_000;
const NANOSECONDS_IN_MICROSECOND: u64 = 1_000;

#[inline]
pub fn nanos_to_secs(nanos: f64) -> f64 {
    nanos / NANOSECONDS_IN_SECOND as f64
}

#[inline]
pub fn nanos_to_millis(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MILLISECOND
}

#[inline]
pub fn nanos_to_micros(nanos: u64) -> u64 {
    nanos / NANOSECONDS_IN_MICROSECOND
}

#[inline]
pub fn unix_nanos_to_iso8601(timestamp_ns: u64) -> String {
    let dt = DateTime::<Utc>::from(UNIX_EPOCH + Duration::from_nanos(timestamp_ns));
    let date = dt.date();
    let time = dt.time();
    format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z", 
        date.year(),
        date.month(),
        date.day(),
        time.hour(),
        time.minute(),
        time.second(),
        time.nanosecond()
    )
}
