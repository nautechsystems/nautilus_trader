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

//! Closed interval types shared across request planning and persistence coverage.

/// A closed nanosecond interval, inclusive at both ends.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClosedInterval {
    /// Inclusive start in nanoseconds since the Unix epoch.
    pub start: u64,
    /// Inclusive end in nanoseconds since the Unix epoch.
    pub end: u64,
}

impl ClosedInterval {
    /// Creates a closed interval if `start <= end`.
    #[must_use]
    pub const fn new(start: u64, end: u64) -> Option<Self> {
        if start <= end {
            Some(Self { start, end })
        } else {
            None
        }
    }
}

impl From<ClosedInterval> for (u64, u64) {
    fn from(interval: ClosedInterval) -> Self {
        (interval.start, interval.end)
    }
}
