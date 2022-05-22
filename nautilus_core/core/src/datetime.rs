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

const NANOSECONDS_IN_SECOND: i64 = 1_000_000_000;
const NANOSECONDS_IN_MILLISECOND: i64 = 1_000_000;
const NANOSECONDS_IN_MICROSECOND: i64 = 1_000;

#[inline]
pub fn nanos_to_secs(nanos: f64) -> f64 {
    nanos / NANOSECONDS_IN_SECOND as f64
}

#[inline]
pub fn nanos_to_millis(nanos: i64) -> i64 {
    nanos / NANOSECONDS_IN_MILLISECOND
}

#[inline]
pub fn nanos_to_micros(nanos: i64) -> i64 {
    nanos / NANOSECONDS_IN_MICROSECOND
}
