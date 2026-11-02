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

//! FinFeed classifications and OHLCV aggregation labels.

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};
use ustr::Ustr;

/// Provider market mechanisms, retaining unfamiliar labels.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, AsRefStr, Display, EnumString,
)]
pub enum MarketMechanism {
    /// Constant product market maker.
    #[serde(rename = "CPMM")]
    #[strum(serialize = "CPMM")]
    Cpmm,
    /// Central limit order book.
    #[serde(rename = "CLOB")]
    #[strum(serialize = "CLOB")]
    Clob,
    /// Unrecognized provider label, retained verbatim.
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(Ustr),
}

/// Provider OHLCV aggregation intervals, retaining newly added periods.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, AsRefStr, Display, EnumString,
)]
pub enum OhlcvInterval {
    /// 1 second.
    #[serde(rename = "1SEC")]
    #[strum(serialize = "1SEC")]
    Second1,
    /// 2 seconds.
    #[serde(rename = "2SEC")]
    #[strum(serialize = "2SEC")]
    Second2,
    /// 3 seconds.
    #[serde(rename = "3SEC")]
    #[strum(serialize = "3SEC")]
    Second3,
    /// 4 seconds.
    #[serde(rename = "4SEC")]
    #[strum(serialize = "4SEC")]
    Second4,
    /// 5 seconds.
    #[serde(rename = "5SEC")]
    #[strum(serialize = "5SEC")]
    Second5,
    /// 6 seconds.
    #[serde(rename = "6SEC")]
    #[strum(serialize = "6SEC")]
    Second6,
    /// 10 seconds.
    #[serde(rename = "10SEC")]
    #[strum(serialize = "10SEC")]
    Second10,
    /// 15 seconds.
    #[serde(rename = "15SEC")]
    #[strum(serialize = "15SEC")]
    Second15,
    /// 20 seconds.
    #[serde(rename = "20SEC")]
    #[strum(serialize = "20SEC")]
    Second20,
    /// 30 seconds.
    #[serde(rename = "30SEC")]
    #[strum(serialize = "30SEC")]
    Second30,
    /// 1 minute.
    #[serde(rename = "1MIN")]
    #[strum(serialize = "1MIN")]
    Minute1,
    /// 2 minutes.
    #[serde(rename = "2MIN")]
    #[strum(serialize = "2MIN")]
    Minute2,
    /// 3 minutes.
    #[serde(rename = "3MIN")]
    #[strum(serialize = "3MIN")]
    Minute3,
    /// 4 minutes.
    #[serde(rename = "4MIN")]
    #[strum(serialize = "4MIN")]
    Minute4,
    /// 5 minutes.
    #[serde(rename = "5MIN")]
    #[strum(serialize = "5MIN")]
    Minute5,
    /// 6 minutes.
    #[serde(rename = "6MIN")]
    #[strum(serialize = "6MIN")]
    Minute6,
    /// 10 minutes.
    #[serde(rename = "10MIN")]
    #[strum(serialize = "10MIN")]
    Minute10,
    /// 15 minutes.
    #[serde(rename = "15MIN")]
    #[strum(serialize = "15MIN")]
    Minute15,
    /// 20 minutes.
    #[serde(rename = "20MIN")]
    #[strum(serialize = "20MIN")]
    Minute20,
    /// 30 minutes.
    #[serde(rename = "30MIN")]
    #[strum(serialize = "30MIN")]
    Minute30,
    /// 1 hour.
    #[serde(rename = "1HRS")]
    #[strum(serialize = "1HRS")]
    Hour1,
    /// 2 hours.
    #[serde(rename = "2HRS")]
    #[strum(serialize = "2HRS")]
    Hour2,
    /// 3 hours.
    #[serde(rename = "3HRS")]
    #[strum(serialize = "3HRS")]
    Hour3,
    /// 4 hours.
    #[serde(rename = "4HRS")]
    #[strum(serialize = "4HRS")]
    Hour4,
    /// 6 hours.
    #[serde(rename = "6HRS")]
    #[strum(serialize = "6HRS")]
    Hour6,
    /// 8 hours.
    #[serde(rename = "8HRS")]
    #[strum(serialize = "8HRS")]
    Hour8,
    /// 12 hours.
    #[serde(rename = "12HRS")]
    #[strum(serialize = "12HRS")]
    Hour12,
    /// 1 day.
    #[serde(rename = "1DAY")]
    #[strum(serialize = "1DAY")]
    Day1,
    /// 2 days.
    #[serde(rename = "2DAY")]
    #[strum(serialize = "2DAY")]
    Day2,
    /// 3 days.
    #[serde(rename = "3DAY")]
    #[strum(serialize = "3DAY")]
    Day3,
    /// 5 days.
    #[serde(rename = "5DAY")]
    #[strum(serialize = "5DAY")]
    Day5,
    /// 7 days.
    #[serde(rename = "7DAY")]
    #[strum(serialize = "7DAY")]
    Day7,
    /// 10 days.
    #[serde(rename = "10DAY")]
    #[strum(serialize = "10DAY")]
    Day10,
    /// 1 calendar month.
    #[serde(rename = "1MTH")]
    #[strum(serialize = "1MTH")]
    Month1,
    /// 2 calendar months.
    #[serde(rename = "2MTH")]
    #[strum(serialize = "2MTH")]
    Month2,
    /// 3 calendar months.
    #[serde(rename = "3MTH")]
    #[strum(serialize = "3MTH")]
    Month3,
    /// 4 calendar months.
    #[serde(rename = "4MTH")]
    #[strum(serialize = "4MTH")]
    Month4,
    /// 6 calendar months.
    #[serde(rename = "6MTH")]
    #[strum(serialize = "6MTH")]
    Month6,
    /// 1 calendar year.
    #[serde(rename = "1YRS")]
    #[strum(serialize = "1YRS")]
    Year1,
    /// 2 calendar years.
    #[serde(rename = "2YRS")]
    #[strum(serialize = "2YRS")]
    Year2,
    /// 3 calendar years.
    #[serde(rename = "3YRS")]
    #[strum(serialize = "3YRS")]
    Year3,
    /// 4 calendar years.
    #[serde(rename = "4YRS")]
    #[strum(serialize = "4YRS")]
    Year4,
    /// 5 calendar years.
    #[serde(rename = "5YRS")]
    #[strum(serialize = "5YRS")]
    Year5,
    /// Unrecognized provider label, retained verbatim.
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(Ustr),
}

/// Provider OHLCV time units, retaining unfamiliar labels.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, AsRefStr, Display, EnumString,
)]
pub enum OhlcvUnit {
    /// Second.
    #[serde(rename = "second")]
    #[strum(serialize = "second")]
    Second,
    /// Minute.
    #[serde(rename = "minute")]
    #[strum(serialize = "minute")]
    Minute,
    /// Hour.
    #[serde(rename = "hour")]
    #[strum(serialize = "hour")]
    Hour,
    /// Day.
    #[serde(rename = "day")]
    #[strum(serialize = "day")]
    Day,
    /// Month.
    #[serde(rename = "month")]
    #[strum(serialize = "month")]
    Month,
    /// Year.
    #[serde(rename = "year")]
    #[strum(serialize = "year")]
    Year,
    /// Unrecognized provider label, retained verbatim.
    #[serde(untagged)]
    #[strum(default, transparent)]
    Other(Ustr),
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::http::models::OhlcvPeriod;

    #[rstest]
    fn every_documented_interval_decodes_and_serializes_verbatim() {
        let periods: Vec<OhlcvPeriod> =
            serde_json::from_str(include_str!("../../test_data/periods.json")).unwrap();
        assert_eq!(periods.len(), 43);

        for period in periods {
            let interval = period.period_id.unwrap();
            assert!(!matches!(interval, OhlcvInterval::Other(_)));
            let label = interval.as_ref();
            assert_eq!(OhlcvInterval::from(label), interval);
            assert_eq!(interval.to_string(), label);
            let encoded = serde_json::to_string(&interval).unwrap();
            assert_eq!(serde_json::from_str::<String>(&encoded).unwrap(), label);
            assert_eq!(
                serde_json::from_str::<OhlcvInterval>(&encoded).unwrap(),
                interval
            );
        }
    }

    #[rstest]
    #[case("second", OhlcvUnit::Second)]
    #[case("minute", OhlcvUnit::Minute)]
    #[case("hour", OhlcvUnit::Hour)]
    #[case("day", OhlcvUnit::Day)]
    #[case("month", OhlcvUnit::Month)]
    #[case("year", OhlcvUnit::Year)]
    fn units_preserve_wire_labels(#[case] label: &str, #[case] expected: OhlcvUnit) {
        assert_eq!(OhlcvUnit::from(label), expected);
        let encoded = serde_json::to_string(label).unwrap();
        assert_eq!(
            serde_json::from_str::<OhlcvUnit>(&encoded).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_string(&expected).unwrap(), encoded);
        assert_eq!(expected.as_ref(), label);
        assert_eq!(expected.to_string(), label);
    }

    #[rstest]
    #[case("CPMM", MarketMechanism::Cpmm)]
    #[case("CLOB", MarketMechanism::Clob)]
    fn mechanisms_preserve_wire_labels(#[case] label: &str, #[case] expected: MarketMechanism) {
        assert_eq!(MarketMechanism::from(label), expected);
        let encoded = serde_json::to_string(label).unwrap();
        assert_eq!(
            serde_json::from_str::<MarketMechanism>(&encoded).unwrap(),
            expected
        );
        assert_eq!(serde_json::to_string(&expected).unwrap(), encoded);
        assert_eq!(expected.as_ref(), label);
        assert_eq!(expected.to_string(), label);
    }

    #[rstest]
    #[case("future-provider-label")]
    #[case("")]
    fn unfamiliar_labels_retain_exact_spelling(#[case] label: &str) {
        let encoded = serde_json::to_string(label).unwrap();
        let mechanism: MarketMechanism = serde_json::from_str(&encoded).unwrap();
        let interval: OhlcvInterval = serde_json::from_str(&encoded).unwrap();
        let unit: OhlcvUnit = serde_json::from_str(&encoded).unwrap();
        assert_eq!(mechanism, MarketMechanism::Other(label.into()));
        assert_eq!(interval, OhlcvInterval::Other(label.into()));
        assert_eq!(unit, OhlcvUnit::Other(label.into()));
        assert_eq!(mechanism.as_ref(), label);
        assert_eq!(interval.as_ref(), label);
        assert_eq!(unit.as_ref(), label);
        assert_eq!(mechanism.to_string(), label);
        assert_eq!(interval.to_string(), label);
        assert_eq!(unit.to_string(), label);
        assert_eq!(MarketMechanism::from(label), mechanism);
        assert_eq!(OhlcvInterval::from(label), interval);
        assert_eq!(OhlcvUnit::from(label), unit);
        assert_eq!(serde_json::to_string(&mechanism).unwrap(), encoded);
        assert_eq!(serde_json::to_string(&interval).unwrap(), encoded);
        assert_eq!(serde_json::to_string(&unit).unwrap(), encoded);
    }
}
