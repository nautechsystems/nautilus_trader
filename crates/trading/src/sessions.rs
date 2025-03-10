// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Timelike, Utc};
use chrono_tz::{America::New_York, Asia::Tokyo, Australia::Sydney, Europe::London, Tz};
use strum::{Display, EnumIter, EnumString, FromRepr};

/// Represents a major Forex market session based on trading hours.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, FromRepr, EnumIter, EnumString, Display)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(eq, eq_int, module = "nautilus_trader.core.nautilus_pyo3.common.enums")
)]
pub enum ForexSession {
    Sydney,
    Tokyo,
    London,
    NewYork,
}

impl ForexSession {
    /// Returns the timezone associated with the session.
    const fn timezone(&self) -> Tz {
        match self {
            Self::Sydney => Sydney,
            Self::Tokyo => Tokyo,
            Self::London => London,
            Self::NewYork => New_York,
        }
    }

    /// Returns the start and end times for the session in local time.
    const fn session_times(&self) -> (NaiveTime, NaiveTime) {
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
    let timezone = session.timezone();
    let local_now = fx_local_from_utc(session, time_now);
    let (start_time, _) = session.session_times();

    let mut next_start = timezone
        .with_ymd_and_hms(
            local_now.year(),
            local_now.month(),
            local_now.day(),
            start_time.hour(),
            start_time.minute(),
            0,
        )
        .unwrap();

    if local_now > next_start {
        next_start += Duration::days(1);
    }

    if next_start.weekday().number_from_monday() > 5 {
        next_start += Duration::days(8 - i64::from(next_start.weekday().number_from_monday()));
    }

    next_start.with_timezone(&Utc)
}

/// Returns the previous session start time in UTC.
#[must_use]
pub fn fx_prev_start(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Utc> {
    let timezone = session.timezone();
    let local_now = fx_local_from_utc(session, time_now);
    let (start_time, _) = session.session_times();

    let mut prev_start = timezone
        .with_ymd_and_hms(
            local_now.year(),
            local_now.month(),
            local_now.day(),
            start_time.hour(),
            start_time.minute(),
            0,
        )
        .unwrap();

    if local_now < prev_start {
        prev_start -= Duration::days(1);
    }

    if prev_start.weekday().number_from_monday() > 5 {
        prev_start -= Duration::days(i64::from(prev_start.weekday().number_from_monday()) - 5);
    }

    prev_start.with_timezone(&Utc)
}

/// Returns the next session end time in UTC.
#[must_use]
pub fn fx_next_end(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Utc> {
    let timezone = session.timezone();
    let local_now = fx_local_from_utc(session, time_now);
    let (_, end_time) = session.session_times();

    let mut next_end = timezone
        .with_ymd_and_hms(
            local_now.year(),
            local_now.month(),
            local_now.day(),
            end_time.hour(),
            end_time.minute(),
            0,
        )
        .unwrap();

    if local_now > next_end {
        next_end += Duration::days(1);
    }

    if next_end.weekday().number_from_monday() > 5 {
        next_end += Duration::days(8 - i64::from(next_end.weekday().number_from_monday()));
    }

    next_end.with_timezone(&Utc)
}

/// Returns the previous session end time in UTC.
#[must_use]
pub fn fx_prev_end(session: ForexSession, time_now: DateTime<Utc>) -> DateTime<Utc> {
    let timezone = session.timezone();
    let local_now = fx_local_from_utc(session, time_now);
    let (_, end_time) = session.session_times();

    let mut prev_end = timezone
        .with_ymd_and_hms(
            local_now.year(),
            local_now.month(),
            local_now.day(),
            end_time.hour(),
            end_time.minute(),
            0,
        )
        .unwrap();

    if local_now < prev_end {
        prev_end -= Duration::days(1);
    }

    if prev_end.weekday().number_from_monday() > 5 {
        prev_end -= Duration::days(i64::from(prev_end.weekday().number_from_monday()) - 5);
    }

    prev_end.with_timezone(&Utc)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
