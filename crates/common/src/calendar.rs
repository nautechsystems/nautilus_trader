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

//! Timezone-aware recurring schedules expressed as UTC timer deadlines.

use std::num::NonZeroU64;

use jiff::{SignedDuration, Span, Zoned};
use nautilus_core::{UnixNanos, datetime::try_datetime_to_unix_nanos};

use crate::timer::{TimerInterval, TimerSchedule};

/// A recurring calendar schedule anchored to its original local date and time.
///
/// Days, weeks, months, and years use the origin's timezone. Time-only spans use elapsed time.
/// Each occurrence is calculated from the original origin, so DST adjustments and month-end
/// clamping do not shift later occurrences. The schedule supplies UTC deadlines and owns no clock,
/// callbacks, or timer lifecycle state.
#[derive(Debug, Clone)]
pub struct CalendarSchedule {
    origin: Zoned,
    span: Span,
    start_time_ns: UnixNanos,
}

impl CalendarSchedule {
    /// Creates a schedule with a positive span and a timezone-aware origin.
    ///
    /// The origin retains its civil date, wall-clock time, timezone, and selected offset in a DST
    /// fold. Jiff's compatible disambiguation resolves later DST gaps and folds.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The span is not positive.
    /// - The origin is outside the [`UnixNanos`] range.
    pub fn new(origin: &Zoned, span: Span) -> anyhow::Result<Self> {
        anyhow::ensure!(
            span.is_positive(),
            "Calendar interval {span} must be positive"
        );

        let start_time_ns = try_datetime_to_unix_nanos(origin.timestamp())?;

        Ok(Self {
            origin: origin.clone(),
            span,
            start_time_ns,
        })
    }

    fn scheduled_time(&self, index: u64) -> Option<UnixNanos> {
        let offset = self.span.checked_mul(i64::try_from(index).ok()?).ok()?;
        let scheduled = self.origin.checked_add(offset).ok()?;
        try_datetime_to_unix_nanos(scheduled.timestamp()).ok()
    }
}

impl TimerSchedule for CalendarSchedule {
    fn start_time_ns(&self) -> UnixNanos {
        self.start_time_ns
    }

    fn next_after(&self, time_ns: UnixNanos, index: &mut u64) -> Option<UnixNanos> {
        if let Ok(duration) = SignedDuration::try_from(self.span) {
            let interval_ns = NonZeroU64::new(u64::try_from(duration.as_nanos()).ok()?)?;
            return TimerInterval::Fixed(interval_ns).next_time_after(
                self.start_time_ns,
                time_ns,
                index,
            );
        }

        loop {
            *index = index.checked_add(1)?;
            let scheduled_time_ns = self.scheduled_time(*index)?;
            if scheduled_time_ns > time_ns {
                return Some(scheduled_time_ns);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use jiff::{Span, Timestamp, Zoned};
    use nautilus_core::{UnixNanos, datetime::get_timezone};
    use rstest::rstest;

    use super::CalendarSchedule;
    use crate::timer::TimerSchedule;

    fn utc_nanos(time: &str) -> UnixNanos {
        UnixNanos::from(time.parse::<Timestamp>().unwrap())
    }

    #[rstest]
    fn test_calendar_schedule_next_time_after_skips_to_local_grid() {
        let interval = CalendarSchedule::new(
            &"2026-10-30T09:30:00[America/New_York]"
                .parse::<Zoned>()
                .unwrap(),
            Span::new().days(1),
        )
        .unwrap();

        let mut index = 0;

        let next_time_ns = interval.next_after(utc_nanos("2026-11-01T20:00:00Z"), &mut index);

        assert_eq!(next_time_ns, Some(utc_nanos("2026-11-02T14:30:00Z")));
        assert_eq!(index, 3);
    }

    #[rstest]
    #[case::nanosecond(Span::new().nanoseconds(1), 10_000_000_000, 10_000_000_001, 10_000_000_001)]
    #[case::second(Span::new().seconds(1), 10_000_000_000, 11_000_000_000, 11)]
    fn test_calendar_schedule_time_only_skips_to_grid(
        #[case] span: Span,
        #[case] time_ns: u64,
        #[case] expected: u64,
        #[case] expected_index: u64,
    ) {
        let interval = CalendarSchedule::new(
            &Timestamp::UNIX_EPOCH.to_zoned(get_timezone("America/New_York").unwrap()),
            span,
        )
        .unwrap();

        let mut index = 0;

        let next_time_ns = interval.next_after(UnixNanos::from(time_ns), &mut index);

        assert_eq!(next_time_ns, Some(UnixNanos::from(expected)));
        assert_eq!(index, expected_index);
    }

    #[rstest]
    #[case::fall_back_keeps_local_time(
        "2026-10-30T09:30:00[America/New_York]",
        "1d",
        ["2026-10-30T13:30:00Z", "2026-10-31T13:30:00Z", "2026-11-01T14:30:00Z", "2026-11-02T14:30:00Z"],
    )]
    #[case::spring_forward_keeps_local_time(
        "2026-03-06T09:30:00[America/New_York]",
        "1d",
        ["2026-03-06T14:30:00Z", "2026-03-07T14:30:00Z", "2026-03-08T13:30:00Z", "2026-03-09T13:30:00Z"],
    )]
    #[case::spring_forward_gap_does_not_shift_later_days(
        "2026-03-07T02:30:00[America/New_York]",
        "1d",
        ["2026-03-07T07:30:00Z", "2026-03-08T07:30:00Z", "2026-03-09T06:30:00Z", "2026-03-10T06:30:00Z"],
    )]
    #[case::month_end_clamp_does_not_shift_later_months(
        "2026-01-31T00:00:00[UTC]",
        "1mo",
        ["2026-01-31T00:00:00Z", "2026-02-28T00:00:00Z", "2026-03-31T00:00:00Z", "2026-04-30T00:00:00Z"],
    )]
    #[case::hours_stay_exact_across_fall_back(
        "2026-11-01T00:00:00[America/New_York]",
        "12h",
        ["2026-11-01T04:00:00Z", "2026-11-01T16:00:00Z", "2026-11-02T04:00:00Z", "2026-11-02T16:00:00Z"],
    )]
    fn test_calendar_schedule_nominal_origin(
        #[case] start: &str,
        #[case] span: &str,
        #[case] expected: [&str; 4],
    ) {
        let start = start.parse::<Zoned>().unwrap();
        let schedule = CalendarSchedule::new(&start, span.parse::<Span>().unwrap()).unwrap();
        let mut index = 0;
        let mut actual = vec![schedule.start_time_ns()];
        for _ in 0..3 {
            actual.push(
                schedule
                    .next_after(*actual.last().unwrap(), &mut index)
                    .unwrap(),
            );
        }

        assert_eq!(
            actual,
            expected.into_iter().map(utc_nanos).collect::<Vec<_>>()
        );
        assert_eq!(index, 3);
    }

    #[rstest]
    #[case::zero(Span::new(), "Calendar interval PT0S must be positive")]
    #[case::negative(Span::new().days(-1), "Calendar interval -P1D must be positive")]
    fn test_calendar_schedule_rejects_non_positive_span(
        #[case] span: Span,
        #[case] expected: &str,
    ) {
        let error = CalendarSchedule::new(
            &Timestamp::UNIX_EPOCH.to_zoned(jiff::tz::TimeZone::UTC),
            span,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn test_calendar_schedule_exhausts_unrepresentable_deadline() {
        let origin = UnixNanos::from(u64::MAX)
            .to_datetime_utc()
            .to_zoned(jiff::tz::TimeZone::UTC);
        let schedule = CalendarSchedule::new(&origin, Span::new().days(1)).unwrap();
        let mut index = 0;
        assert_eq!(
            schedule.next_after(UnixNanos::from(u64::MAX), &mut index),
            None
        );
    }
}
