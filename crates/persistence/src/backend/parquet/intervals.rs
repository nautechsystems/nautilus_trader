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

use crate::{common::coverage::missing_intervals, errors::CatalogOverlapError};

/// Checks if a list of closed integer intervals are all mutually disjoint.
///
/// Returns `true` for empty lists or lists with a single interval.
#[must_use]
pub fn are_intervals_disjoint(intervals: &[(u64, u64)]) -> bool {
    if intervals.len() <= 1 {
        return true;
    }

    let mut sorted_intervals = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);
    sorted_intervals
        .array_windows()
        .all(|&[(_, end), (start, _)]| end < start)
}

/// Checks that `new` intervals are disjoint from each other and from `existing` ones.
///
/// # Errors
///
/// Returns an error if any intervals overlap, with `message`, and as a [`CatalogOverlapError`]
/// if a `new` interval overlaps an `existing` one.
pub(crate) fn check_new_intervals_disjoint(
    existing: &[(u64, u64)],
    new: &[(u64, u64)],
    message: impl FnOnce() -> String,
) -> anyhow::Result<()> {
    let mut intervals = existing.to_vec();
    intervals.extend_from_slice(new);

    if are_intervals_disjoint(&intervals) {
        return Ok(());
    }

    let overlaps_existing = new.iter().any(|&(start, end)| {
        existing
            .iter()
            .any(|&(existing_start, existing_end)| start <= existing_end && existing_start <= end)
    });

    if overlaps_existing {
        anyhow::bail!(CatalogOverlapError(message()));
    }

    anyhow::bail!(message())
}

/// Checks if intervals are contiguous (adjacent with no gaps).
///
/// Intervals are contiguous if, when sorted by start time, each interval's start
/// timestamp is exactly one more than the previous interval's end timestamp.
#[must_use]
pub fn are_intervals_contiguous(intervals: &[(u64, u64)]) -> bool {
    if intervals.len() <= 1 {
        return true;
    }

    let mut sorted_intervals = intervals.to_vec();
    sorted_intervals.sort_by_key(|&(start, _)| start);
    sorted_intervals
        .array_windows()
        .all(|&[(_, end), (start, _)]| end + 1 == start)
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

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{are_intervals_contiguous, are_intervals_disjoint, check_new_intervals_disjoint};
    use crate::errors::CatalogOverlapError;

    #[rstest]
    #[case::empty(&[], true)]
    #[case::single(&[(1, 5)], true)]
    #[case::separated(&[(1, 2), (3, 4)], true)]
    #[case::shared_endpoint(&[(1, 5), (5, 6)], false)]
    #[case::nested(&[(1, 5), (3, 4)], false)]
    #[case::unsorted_separated(&[(5, 6), (1, 2)], true)]
    #[case::unsorted_overlapping(&[(4, 6), (1, 4)], false)]
    fn are_intervals_disjoint_cases(#[case] intervals: &[(u64, u64)], #[case] expected: bool) {
        assert_eq!(are_intervals_disjoint(intervals), expected);
    }

    #[rstest]
    #[case::disjoint(&[(1, 2)], &[(3, 4)], None)]
    #[case::new_overlaps_existing(&[(1, 5)], &[(5, 6)], Some(true))]
    #[case::existing_overlap_each_other(&[(1, 5), (4, 6)], &[(7, 8)], Some(false))]
    #[case::new_overlap_each_other(&[(1, 2)], &[(3, 5), (5, 6)], Some(false))]
    fn check_new_intervals_disjoint_cases(
        #[case] existing: &[(u64, u64)],
        #[case] new: &[(u64, u64)],
        #[case] expected_overlap: Option<bool>,
    ) {
        let overlap = check_new_intervals_disjoint(existing, new, || "overlap".to_string())
            .err()
            .map(|e| e.downcast_ref::<CatalogOverlapError>().is_some());

        assert_eq!(overlap, expected_overlap);
    }

    #[rstest]
    #[case::empty(&[], true)]
    #[case::single(&[(1, 1)], true)]
    #[case::adjacent(&[(1, 2), (3, 4)], true)]
    #[case::shared_endpoint(&[(1, 5), (5, 6)], false)]
    #[case::gap(&[(1, 2), (4, 5)], false)]
    #[case::unsorted_adjacent(&[(3, 4), (1, 2)], true)]
    #[case::unsorted_gap(&[(5, 6), (1, 2)], false)]
    fn are_intervals_contiguous_cases(#[case] intervals: &[(u64, u64)], #[case] expected: bool) {
        assert_eq!(are_intervals_contiguous(intervals), expected);
    }
}
