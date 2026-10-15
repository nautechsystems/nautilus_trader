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

//! What an order report read covered.

use ahash::AHashSet;
use nautilus_core::{DurationNanos, UnixNanos};
use nautilus_model::identifiers::VenueOrderId;

/// Allowance for skew between the node's clock, which stamps an order's creation, and the venue's,
/// which stamps how far back a read reached. Kept generous: too long defers an order the read did
/// cover, too short reports as absent an order it did not reach.
const COVERAGE_CLOCK_SKEW: DurationNanos = DurationNanos::from_secs(60);

/// What an order report read covered: the rows it skipped and how far back it reached.
///
/// A complete read rules out any order it did not return. An incomplete one rules out only an
/// order that none of its skipped rows can be and that the venue placed after the earliest time
/// from which the read is complete.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReportCoverage {
    skipped: AHashSet<VenueOrderId>,
    skipped_unidentified: bool,
    truncation: Option<Truncation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Truncation {
    /// The read holds every row from this time on.
    Since(UnixNanos),
    /// The read stopped at a point it cannot place in time.
    Unplaced,
}

impl ReportCoverage {
    /// Records a skipped row of the order `venue_order_id`; an ID that does not parse leaves the
    /// row unidentified.
    pub(crate) fn skip(&mut self, venue_order_id: &str) {
        match VenueOrderId::new_checked(venue_order_id) {
            Ok(id) => {
                self.skipped.insert(id);
            }
            Err(_) => self.skipped_unidentified = true,
        }
    }

    /// Records a skipped row that names no order.
    pub(crate) fn skip_unidentified(&mut self) {
        self.skipped_unidentified = true;
    }

    /// Records that the read stopped short, having read every row from `since` on; `None` when
    /// the point it stopped at cannot be placed in time.
    pub(crate) fn truncate(&mut self, since: Option<UnixNanos>) {
        self.truncation = match (self.truncation, since) {
            (Some(Truncation::Unplaced), _) | (_, None) => Some(Truncation::Unplaced),
            (Some(Truncation::Since(earlier)), Some(since)) => {
                Some(Truncation::Since(earlier.max(since)))
            }
            (None, Some(since)) => Some(Truncation::Since(since)),
        };
    }

    /// Whether the read skipped no row and reached its end.
    pub(crate) fn is_complete(&self) -> bool {
        self.skipped.is_empty() && !self.skipped_unidentified && self.truncation.is_none()
    }

    /// Whether the read rules out the order it did not return.
    ///
    /// No skipped row may be the order, so with any row skipped the order needs a venue order ID
    /// none of them carries. A cut-short read must have reached back past the order's placement,
    /// so `placed_from`, the earliest time the order can carry at the venue, must lie after the
    /// read's start by more than the clock skew allowance.
    pub(crate) fn rules_out(
        &self,
        venue_order_id: Option<VenueOrderId>,
        placed_from: Option<UnixNanos>,
    ) -> bool {
        if self.skipped_unidentified {
            return false;
        }

        if !self.skipped.is_empty() && venue_order_id.is_none_or(|id| self.skipped.contains(&id)) {
            return false;
        }

        match self.truncation {
            None => true,
            Some(Truncation::Unplaced) => false,
            Some(Truncation::Since(since)) => {
                placed_from.is_some_and(|placed| placed.saturating_sub(COVERAGE_CLOCK_SKEW) > since)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const SINCE_NS: u64 = 1_700_000_000_000_000_000;
    const SKEW_NS: u64 = 60_000_000_000;

    /// Where a test read stops.
    #[derive(Clone, Copy, Debug)]
    enum Cut {
        Whole,
        Unplaced,
        Since(u64),
    }

    fn coverage(skipped: &[&str], unidentified: bool, cut: Cut) -> ReportCoverage {
        let mut coverage = ReportCoverage::default();
        for id in skipped {
            coverage.skip(id);
        }

        if unidentified {
            coverage.skip_unidentified();
        }

        match cut {
            Cut::Whole => {}
            Cut::Unplaced => coverage.truncate(None),
            Cut::Since(since) => coverage.truncate(Some(UnixNanos::from(since))),
        }
        coverage
    }

    #[rstest]
    #[case::complete_read(&[], false, Cut::Whole, None, None, true)]
    #[case::complete_read_order_without_venue_id(&[], false, Cut::Whole, None, Some(SINCE_NS), true)]
    #[case::skipped_row_is_the_order(&["V-1"], false, Cut::Whole, Some("V-1"), None, false)]
    #[case::skipped_row_is_another_order(&["V-2"], false, Cut::Whole, Some("V-1"), None, true)]
    #[case::order_without_venue_id_beside_a_skipped_row(&["V-2"], false, Cut::Whole, None, None, false)]
    #[case::skipped_row_names_no_order(&[], true, Cut::Whole, Some("V-1"), None, false)]
    #[case::unparsable_skipped_id(&[""], false, Cut::Whole, Some("V-1"), None, false)]
    #[case::cut_short_at_an_unplaced_point(&[], false, Cut::Unplaced, Some("V-1"), Some(SINCE_NS * 2), false)]
    #[case::placed_after_the_start_beyond_the_skew(&[], false, Cut::Since(SINCE_NS), Some("V-1"), Some(SINCE_NS + SKEW_NS + 1), true)]
    #[case::placed_at_the_skew_bound(&[], false, Cut::Since(SINCE_NS), Some("V-1"), Some(SINCE_NS + SKEW_NS), false)]
    #[case::placed_before_the_start(&[], false, Cut::Since(SINCE_NS), Some("V-1"), Some(SINCE_NS - 1), false)]
    #[case::placement_unknown(&[], false, Cut::Since(SINCE_NS), Some("V-1"), None, false)]
    #[case::cut_short_and_another_order_skipped(&["V-2"], false, Cut::Since(SINCE_NS), Some("V-1"), Some(SINCE_NS + SKEW_NS + 1), true)]
    fn test_rules_out(
        #[case] skipped: &[&str],
        #[case] unidentified: bool,
        #[case] cut: Cut,
        #[case] venue_order_id: Option<&str>,
        #[case] placed_from: Option<u64>,
        #[case] expected: bool,
    ) {
        let coverage = coverage(skipped, unidentified, cut);

        let ruled_out = coverage.rules_out(
            venue_order_id.map(VenueOrderId::new),
            placed_from.map(UnixNanos::from),
        );

        assert_eq!(ruled_out, expected);
    }

    #[rstest]
    #[case::nothing_skipped_or_cut(&[], false, Cut::Whole, true)]
    #[case::row_skipped(&["V-1"], false, Cut::Whole, false)]
    #[case::unidentified_row_skipped(&[], true, Cut::Whole, false)]
    #[case::cut_short(&[], false, Cut::Since(SINCE_NS), false)]
    #[case::cut_short_unplaced(&[], false, Cut::Unplaced, false)]
    fn test_is_complete(
        #[case] skipped: &[&str],
        #[case] unidentified: bool,
        #[case] cut: Cut,
        #[case] expected: bool,
    ) {
        assert_eq!(coverage(skipped, unidentified, cut).is_complete(), expected);
    }

    #[rstest]
    #[case::later_start_wins(Some(SINCE_NS), Some(SINCE_NS + 5), Some(Truncation::Since(UnixNanos::from(SINCE_NS + 5))))]
    #[case::earlier_start_loses(Some(SINCE_NS + 5), Some(SINCE_NS), Some(Truncation::Since(UnixNanos::from(SINCE_NS + 5))))]
    #[case::unplaced_after_placed(Some(SINCE_NS), None, Some(Truncation::Unplaced))]
    #[case::placed_after_unplaced(None, Some(SINCE_NS), Some(Truncation::Unplaced))]
    fn test_truncate_keeps_the_narrower_coverage(
        #[case] first: Option<u64>,
        #[case] second: Option<u64>,
        #[case] expected: Option<Truncation>,
    ) {
        let mut coverage = ReportCoverage::default();
        coverage.truncate(first.map(UnixNanos::from));
        coverage.truncate(second.map(UnixNanos::from));

        assert_eq!(coverage.truncation, expected);
    }
}
