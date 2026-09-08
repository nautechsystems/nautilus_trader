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

//! Closed-interval set operations used by the parquet catalog for coverage and gap analysis.

use nautilus_core::ClosedInterval;

use crate::common::coverage::missing_intervals;

/// Checks if a list of closed integer intervals are all mutually disjoint.
///
/// Returns `true` for empty lists or lists with a single interval.
#[must_use]
pub fn are_intervals_disjoint(intervals: &[(u64, u64)]) -> bool {
    let n = intervals.len();

    if n <= 1 {
        return true;
    }

    let mut sorted_intervals: Vec<(u64, u64)> = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);

    for i in 0..(n - 1) {
        let (_, end1) = sorted_intervals[i];
        let (start2, _) = sorted_intervals[i + 1];

        if end1 >= start2 {
            return false;
        }
    }

    true
}

/// Checks if intervals are contiguous (adjacent with no gaps).
///
/// Intervals are contiguous if, when sorted by start time, each interval's start
/// timestamp is exactly one more than the previous interval's end timestamp.
#[must_use]
pub fn are_intervals_contiguous(intervals: &[(u64, u64)]) -> bool {
    let n = intervals.len();
    if n <= 1 {
        return true;
    }

    let mut sorted_intervals: Vec<(u64, u64)> = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);

    for i in 0..(n - 1) {
        let (_, end1) = sorted_intervals[i];
        let (start2, _) = sorted_intervals[i + 1];

        if end1 + 1 != start2 {
            return false;
        }
    }

    true
}

/// Finds the parts of a query interval that are not covered by existing data intervals.
///
/// Returns a vector of (start, end) tuples representing the gaps in coverage.
pub(crate) fn query_interval_diff(
    start: u64,
    end: u64,
    closed_intervals: &[(u64, u64)],
) -> Vec<(u64, u64)> {
    if start > end {
        return Vec::new();
    }

    let intervals = closed_intervals
        .iter()
        .filter_map(|&(start, end)| ClosedInterval::new(start, end))
        .collect::<Vec<_>>();

    missing_intervals(start, end, &intervals)
        .into_iter()
        .map(Into::into)
        .collect()
}
