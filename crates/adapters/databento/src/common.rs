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

//! Common functions to support Databento adapter operations.

use databento::historical::DateTimeRange;
use nautilus_core::UnixNanos;
use time::OffsetDateTime;

pub const DATABENTO: &str = "DATABENTO";
pub const ALL_SYMBOLS: &str = "ALL_SYMBOLS";

pub fn get_date_time_range(start: UnixNanos, end: UnixNanos) -> anyhow::Result<DateTimeRange> {
    Ok(DateTimeRange::from((
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(start.as_u64()))?,
        OffsetDateTime::from_unix_timestamp_nanos(i128::from(end.as_u64()))?,
    )))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    #[case(
        UnixNanos::default(),
        UnixNanos::default(),
        "DateTimeRange { start: 1970-01-01 0:00:00.0 +00:00:00, end: 1970-01-01 0:00:00.0 +00:00:00 }"
    )]
    #[case(UnixNanos::default(), 1_000_000_000.into(), "DateTimeRange { start: 1970-01-01 0:00:00.0 +00:00:00, end: 1970-01-01 0:00:01.0 +00:00:00 }")]
    fn test_get_date_time_range(
        #[case] start: UnixNanos,
        #[case] end: UnixNanos,
        #[case] range_str: &str,
    ) {
        let range = get_date_time_range(start, end).unwrap();
        assert_eq!(format!("{range:?}"), range_str);
    }
}
