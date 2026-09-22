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

//! Resolves the live quarterly futures contract the testers trade, so they keep working after
//! a contract expires. Mirrors `active_quarterly_contract` in
//! `examples/live/interactive_brokers/_common.py`.

use jiff::{
    Timestamp, ToSpan,
    civil::{Date, Weekday},
    tz::TimeZone,
};
use nautilus_model::identifiers::{InstrumentId, format_futures_symbol, futures_month_code};

const QUARTERLY_CONTRACT_MONTHS: [i8; 4] = [3, 6, 9, 12];
// Roll to the next contract this many days before expiry, so a run never lands in expiry week.
const MIN_DAYS_TO_EXPIRY: i64 = 45;

pub(crate) fn es_future_instrument_id() -> InstrumentId {
    let today = Timestamp::now().to_zoned(TimeZone::UTC).date();
    active_quarterly_contract("ES", "XCME", today)
}

/// Returns the first quarterly contract whose third-Friday expiry is at least
/// `MIN_DAYS_TO_EXPIRY` days after `today`.
pub(crate) fn active_quarterly_contract(root: &str, venue: &str, today: Date) -> InstrumentId {
    let target_expiry = today
        .checked_add(MIN_DAYS_TO_EXPIRY.days())
        .expect("target expiry within the supported date range");
    let mut year = today.year();

    loop {
        for month in QUARTERLY_CONTRACT_MONTHS {
            if third_friday(year, month) < target_expiry {
                continue;
            }

            let month_code = futures_month_code(month as u8).expect("quarterly month code");
            let symbol = format_futures_symbol(root, month_code, i32::from(year), 1)
                .expect("single-digit futures symbol");
            return InstrumentId::from(format!("{symbol}.{venue}").as_str());
        }
        year += 1;
    }
}

fn third_friday(year: i16, month: i8) -> Date {
    Date::new(year, month, 1)
        .expect("first day of a quarterly month")
        .nth_weekday_of_month(3, Weekday::Friday)
        .expect("every month has a third Friday")
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(2026, 3, date(2026, 3, 20))]
    #[case(2026, 12, date(2026, 12, 18))]
    #[case(2027, 3, date(2027, 3, 19))]
    fn test_third_friday(#[case] year: i16, #[case] month: i8, #[case] expected: Date) {
        assert_eq!(third_friday(year, month), expected);
    }

    #[rstest]
    #[case(date(2026, 9, 21), "ESZ6.XCME")]
    #[case(date(2026, 11, 3), "ESZ6.XCME")]
    #[case(date(2026, 11, 4), "ESH7.XCME")]
    #[case(date(2026, 12, 31), "ESH7.XCME")]
    fn test_active_quarterly_contract_rolls_before_expiry(
        #[case] today: Date,
        #[case] expected: &str,
    ) {
        assert_eq!(
            active_quarterly_contract("ES", "XCME", today),
            InstrumentId::from(expected)
        );
    }

    #[rstest]
    fn test_es_future_instrument_id_uses_xcme() {
        assert_eq!(es_future_instrument_id().venue.as_str(), "XCME");
    }
}
