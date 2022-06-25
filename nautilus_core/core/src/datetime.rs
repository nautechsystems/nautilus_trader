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

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};

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
    let secs = (timestamp_ns / 1_000_000_000) as i64;
    let nanos = (timestamp_ns as i64 - (secs * 1_000_000_000)) as u32;
    let dt = NaiveDateTime::from_timestamp(secs, nanos);
    DateTime::<Utc>::from_utc(dt, Utc).to_rfc3339_opts(SecondsFormat::Nanos, true)
}
