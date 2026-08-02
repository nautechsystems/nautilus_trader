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

//! Provides utilities for determining Forex session times.
//! Includes functions to convert UTC times to session local times
//! and retrieve the next or previous session start/end.
//!
//! All FX sessions run Monday to Friday local time:
//!
//! - Sydney Session    0700-1600 (Australia / Sydney)
//! - Tokyo Session     0900-1800 (Asia / Tokyo)
//! - London Session    0800-1600 (Europe / London)
//! - New York Session  0800-1700 (America / New York)

use chrono::{DateTime, Datelike, Days, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::{America::New_York, Asia::Tokyo, Australia::Sydney, Europe::London, Tz};
use strum::{Display, EnumIter, EnumString, FromRepr};

/// Represents a major Forex market session based on trading hours.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, FromRepr, EnumIter, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        eq,
        eq_int,
        module = "nautilus_trader.core.nautilus_pyo3.common.enums",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.trading")
)]
pub enum ForexSession {
    Sydney,
    Tokyo,
    London,
    NewYork,
}

impl ForexSession {
    /// Returns the timezone associated with the session.
    const fn timezone(self) -> Tz {
        match self {
            Self::Sydney => Sydney,
            Self::Tokyo => Tokyo,
            Self::London => London,
            Self::NewYork => New_York,
        }
    }

    /// Returns the start and end times for the session in local time.
    const fn session_times(self) -> (NaiveTime, NaiveTime) {
        match self {
            Self::Sydney => (
                NaiveTime::from_hms_opt(7, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            ),
            Self::Tokyo => (
                NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            ),
            Self::London => (
                NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            ),
            Self::NewYork => (
                NaiveTime::from_hms_opt(8, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(17, 0, 0).unwrap(),
            ),
        }
    }
}

/// Converts a UTC timestamp to the local time for the given Forex session.
#[must_use]
pub fn fx_local_from_utc(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Tz> {
    session.timezone().from_utc_datetime(&time_now.naive_utc())
}

/// Returns the next session start time in UTC.
#[must_use]
pub fn fx_next_start(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Utc> {
    let local_now = fx_local_from_utc(session, time_now);
    let (start_time, _) = session.session_times();

    fx_next_boundary(local_now, start_time)
}

/// Returns the previous session start time in UTC.
#[must_use]
pub fn fx_prev_start(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Utc> {
    let local_now = fx_local_from_utc(session, time_now);
    let (start_time, _) = session.session_times();

    fx_prev_boundary(local_now, start_time)
}

/// Returns the next session end time in UTC.
#[must_use]
pub fn fx_next_end(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Utc> {
    let local_now = fx_local_from_utc(session, time_now);
    let (_, end_time) = session.session_times();

    fx_next_boundary(local_now, end_time)
}

/// Returns the previous session end time in UTC.
#[must_use]
pub fn fx_prev_end(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Utc> {
    let local_now = fx_local_from_utc(session, time_now);
    let (_, end_time) = session.session_times();

    fx_prev_boundary(local_now, end_time)
}

fn fx_next_boundary(local_now: DateTime<Tz>, session_time: NaiveTime) -> DateTime<Utc> {
    let timezone = local_now.timezone();
    let mut date = local_now.date_naive();

    if local_now.time() > session_time {
        date = date
            .checked_add_days(Days::new(1))
            .expect("FX session date must be representable");
    }

    let weekend_days = match date.weekday() {
        Weekday::Sat => 2,
        Weekday::Sun => 1,
        _ => 0,
    };
    date = date
        .checked_add_days(Days::new(weekend_days))
        .expect("FX session date must be representable");

    timezone
        .from_local_datetime(&date.and_time(session_time))
        .single()
        .expect("FX session boundary must be a unique local time")
        .with_timezone(&Utc)
}

fn fx_prev_boundary(local_now: DateTime<Tz>, session_time: NaiveTime) -> DateTime<Utc> {
    let timezone = local_now.timezone();
    let mut date = local_now.date_naive();

    if local_now.time() < session_time {
        date = date
            .checked_sub_days(Days::new(1))
            .expect("FX session date must be representable");
    }

    let weekend_days = match date.weekday() {
        Weekday::Sat => 1,
        Weekday::Sun => 2,
        _ => 0,
    };
    date = date
        .checked_sub_days(Days::new(weekend_days))
        .expect("FX session date must be representable");

    timezone
        .from_local_datetime(&date.and_time(session_time))
        .single()
        .expect("FX session boundary must be a unique local time")
        .with_timezone(&Utc)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn local_datetime(
        session: ForexSession,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
    ) -> DateTime<Tz> {
        session
            .timezone()
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .unwrap()
    }

    #[rstest]
    #[case(ForexSession::Sydney, "1970-01-01T10:00:00+10:00")]
    #[case(ForexSession::Tokyo, "1970-01-01T09:00:00+09:00")]
    #[case(ForexSession::London, "1970-01-01T01:00:00+01:00")]
    #[case(ForexSession::NewYork, "1969-12-31T19:00:00-05:00")]
    pub fn test_fx_local_from_utc(#[case] session: ForexSession, #[case] expected: &str) {
        let unix_epoch = Utc.timestamp_opt(0, 0).unwrap();
        let result = fx_local_from_utc(session, unix_epoch);
        assert_eq!(result.to_rfc3339(), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, "1970-01-01T21:00:00+00:00")]
    #[case(ForexSession::Tokyo, "1970-01-01T00:00:00+00:00")]
    #[case(ForexSession::London, "1970-01-01T07:00:00+00:00")]
    #[case(ForexSession::NewYork, "1970-01-01T13:00:00+00:00")]
    pub fn test_fx_next_start(#[case] session: ForexSession, #[case] expected: &str) {
        let unix_epoch = Utc.timestamp_opt(0, 0).unwrap();
        let result = fx_next_start(session, unix_epoch);
        assert_eq!(result.to_rfc3339(), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, "1969-12-31T21:00:00+00:00")]
    #[case(ForexSession::Tokyo, "1970-01-01T00:00:00+00:00")]
    #[case(ForexSession::London, "1969-12-31T07:00:00+00:00")]
    #[case(ForexSession::NewYork, "1969-12-31T13:00:00+00:00")]
    pub fn test_fx_prev_start(#[case] session: ForexSession, #[case] expected: &str) {
        let unix_epoch = Utc.timestamp_opt(0, 0).unwrap();
        let result = fx_prev_start(session, unix_epoch);
        assert_eq!(result.to_rfc3339(), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, "1970-01-01T06:00:00+00:00")]
    #[case(ForexSession::Tokyo, "1970-01-01T09:00:00+00:00")]
    #[case(ForexSession::London, "1970-01-01T15:00:00+00:00")]
    #[case(ForexSession::NewYork, "1970-01-01T22:00:00+00:00")]
    pub fn test_fx_next_end(#[case] session: ForexSession, #[case] expected: &str) {
        let unix_epoch = Utc.timestamp_opt(0, 0).unwrap();
        let result = fx_next_end(session, unix_epoch);
        assert_eq!(result.to_rfc3339(), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, "1969-12-31T06:00:00+00:00")]
    #[case(ForexSession::Tokyo, "1969-12-31T09:00:00+00:00")]
    #[case(ForexSession::London, "1969-12-31T15:00:00+00:00")]
    #[case(ForexSession::NewYork, "1969-12-31T22:00:00+00:00")]
    pub fn test_fx_prev_end(#[case] session: ForexSession, #[case] expected: &str) {
        let unix_epoch = Utc.timestamp_opt(0, 0).unwrap();
        let result = fx_prev_end(session, unix_epoch);
        assert_eq!(result.to_rfc3339(), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, (2024, 4, 5), (2024, 4, 8), 7)]
    #[case(ForexSession::Sydney, (2024, 10, 4), (2024, 10, 7), 7)]
    #[case(ForexSession::London, (2024, 3, 29), (2024, 4, 1), 8)]
    #[case(ForexSession::London, (2024, 10, 25), (2024, 10, 28), 8)]
    #[case(ForexSession::NewYork, (2024, 3, 8), (2024, 3, 11), 8)]
    #[case(ForexSession::NewYork, (2024, 11, 1), (2024, 11, 4), 8)]
    // Saturday input: advances to Sunday, then takes the Sunday weekend arm.
    #[case(ForexSession::London, (2024, 3, 30), (2024, 4, 1), 8)]
    fn test_fx_next_start_across_dst_weekend(
        #[case] session: ForexSession,
        #[case] input_date: (i32, u32, u32),
        #[case] expected_date: (i32, u32, u32),
        #[case] expected_hour: u32,
    ) {
        let (input_year, input_month, input_day) = input_date;
        let (expected_year, expected_month, expected_day) = expected_date;
        let time_now =
            local_datetime(session, input_year, input_month, input_day, 18).with_timezone(&Utc);
        let expected = local_datetime(
            session,
            expected_year,
            expected_month,
            expected_day,
            expected_hour,
        )
        .with_timezone(&Utc);

        assert_eq!(fx_next_start(session, time_now), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, (2024, 4, 8), (2024, 4, 5), 7)]
    #[case(ForexSession::Sydney, (2024, 10, 7), (2024, 10, 4), 7)]
    #[case(ForexSession::London, (2024, 4, 1), (2024, 3, 29), 8)]
    #[case(ForexSession::London, (2024, 10, 28), (2024, 10, 25), 8)]
    #[case(ForexSession::NewYork, (2024, 3, 11), (2024, 3, 8), 8)]
    #[case(ForexSession::NewYork, (2024, 11, 4), (2024, 11, 1), 8)]
    // Sunday input: retreats to Saturday, then takes the Saturday weekend arm.
    #[case(ForexSession::London, (2024, 3, 31), (2024, 3, 29), 8)]
    fn test_fx_prev_start_across_dst_weekend(
        #[case] session: ForexSession,
        #[case] input_date: (i32, u32, u32),
        #[case] expected_date: (i32, u32, u32),
        #[case] expected_hour: u32,
    ) {
        let (input_year, input_month, input_day) = input_date;
        let (expected_year, expected_month, expected_day) = expected_date;
        let time_now =
            local_datetime(session, input_year, input_month, input_day, 6).with_timezone(&Utc);
        let expected = local_datetime(
            session,
            expected_year,
            expected_month,
            expected_day,
            expected_hour,
        )
        .with_timezone(&Utc);

        assert_eq!(fx_prev_start(session, time_now), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, (2024, 4, 5), (2024, 4, 8), 16)]
    #[case(ForexSession::Sydney, (2024, 10, 4), (2024, 10, 7), 16)]
    #[case(ForexSession::London, (2024, 3, 29), (2024, 4, 1), 16)]
    #[case(ForexSession::London, (2024, 10, 25), (2024, 10, 28), 16)]
    #[case(ForexSession::NewYork, (2024, 3, 8), (2024, 3, 11), 17)]
    #[case(ForexSession::NewYork, (2024, 11, 1), (2024, 11, 4), 17)]
    fn test_fx_next_end_across_dst_weekend(
        #[case] session: ForexSession,
        #[case] input_date: (i32, u32, u32),
        #[case] expected_date: (i32, u32, u32),
        #[case] expected_hour: u32,
    ) {
        let (input_year, input_month, input_day) = input_date;
        let (expected_year, expected_month, expected_day) = expected_date;
        let time_now =
            local_datetime(session, input_year, input_month, input_day, 18).with_timezone(&Utc);
        let expected = local_datetime(
            session,
            expected_year,
            expected_month,
            expected_day,
            expected_hour,
        )
        .with_timezone(&Utc);

        assert_eq!(fx_next_end(session, time_now), expected);
    }

    #[rstest]
    #[case(ForexSession::Sydney, (2024, 4, 8), (2024, 4, 5), 16)]
    #[case(ForexSession::Sydney, (2024, 10, 7), (2024, 10, 4), 16)]
    #[case(ForexSession::London, (2024, 4, 1), (2024, 3, 29), 16)]
    #[case(ForexSession::London, (2024, 10, 28), (2024, 10, 25), 16)]
    #[case(ForexSession::NewYork, (2024, 3, 11), (2024, 3, 8), 17)]
    #[case(ForexSession::NewYork, (2024, 11, 4), (2024, 11, 1), 17)]
    fn test_fx_prev_end_across_dst_weekend(
        #[case] session: ForexSession,
        #[case] input_date: (i32, u32, u32),
        #[case] expected_date: (i32, u32, u32),
        #[case] expected_hour: u32,
    ) {
        let (input_year, input_month, input_day) = input_date;
        let (expected_year, expected_month, expected_day) = expected_date;
        let time_now =
            local_datetime(session, input_year, input_month, input_day, 6).with_timezone(&Utc);
        let expected = local_datetime(
            session,
            expected_year,
            expected_month,
            expected_day,
            expected_hour,
        )
        .with_timezone(&Utc);

        assert_eq!(fx_prev_end(session, time_now), expected);
    }

    #[rstest]
    pub fn test_fx_next_start_on_weekend() {
        let sunday_utc = Utc.with_ymd_and_hms(2020, 7, 12, 9, 0, 0).unwrap(); // Sunday
        let result = fx_next_start(ForexSession::Tokyo, sunday_utc);
        let expected = Utc.with_ymd_and_hms(2020, 7, 13, 0, 0, 0).unwrap(); // Monday

        assert_eq!(result, expected);
    }

    #[rstest]
    pub fn test_fx_next_start_during_active_session() {
        let during_session = Utc.with_ymd_and_hms(2020, 7, 13, 10, 0, 0).unwrap(); // Sydney session is active
        let result = fx_next_start(ForexSession::Sydney, during_session);
        let expected = Utc.with_ymd_and_hms(2020, 7, 13, 21, 0, 0).unwrap(); // Next Sydney session start

        assert_eq!(result, expected);
    }

    #[rstest]
    pub fn test_fx_prev_start_before_session() {
        let before_session = Utc.with_ymd_and_hms(2020, 7, 13, 6, 0, 0).unwrap(); // Before Tokyo session start
        let result = fx_prev_start(ForexSession::Tokyo, before_session);
        let expected = Utc.with_ymd_and_hms(2020, 7, 13, 0, 0, 0).unwrap(); // Current Tokyo session start

        assert_eq!(result, expected);
    }

    #[rstest]
    pub fn test_fx_next_end_crossing_midnight() {
        let late_night = Utc.with_ymd_and_hms(2020, 7, 13, 23, 0, 0).unwrap(); // After NY session ended
        let result = fx_next_end(ForexSession::NewYork, late_night);
        let expected = Utc.with_ymd_and_hms(2020, 7, 14, 21, 0, 0).unwrap(); // Next NY session end

        assert_eq!(result, expected);
    }

    #[rstest]
    pub fn test_fx_prev_end_after_session() {
        let after_session = Utc.with_ymd_and_hms(2020, 7, 13, 17, 30, 0).unwrap(); // Just after NY session ended
        let result = fx_prev_end(ForexSession::NewYork, after_session);
        let expected = Utc.with_ymd_and_hms(2020, 7, 10, 21, 0, 0).unwrap(); // Previous NY session end

        assert_eq!(result, expected);
    }
}
