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

//! HTTP retry timing for the Binance adapter.

use std::{collections::HashMap, time::Duration};

use jiff::{Timestamp, fmt::rfc2822::DateTimeParser};
use nautilus_network::retry::RetryConfig;

use crate::common::consts::BINANCE_RETRY_AFTER_HEADER;

pub(crate) fn retry_after(headers: &HashMap<String, String>, now: Timestamp) -> Option<Duration> {
    let value = headers.get(BINANCE_RETRY_AFTER_HEADER)?;
    let delay = parse_retry_after(value, now);
    if delay.is_none() {
        log::warn!("Invalid Binance response header {BINANCE_RETRY_AFTER_HEADER}={value:?}");
    }
    delay
}

pub(crate) fn retry_config() -> RetryConfig {
    RetryConfig {
        max_elapsed_ms: Some(180_000),
        ..Default::default()
    }
}

fn parse_retry_after(value: &str, now: Timestamp) -> Option<Duration> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }

    let retry_at = DateTimeParser::new().parse_timestamp(value).ok()?;
    let delay = retry_at.duration_since(now);
    if delay.is_negative() {
        Some(Duration::ZERO)
    } else {
        Some(delay.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("0", 0)]
    #[case("1", 1)]
    #[case("120", 120)]
    fn test_parse_retry_after_delay_seconds(#[case] value: &str, #[case] expected_secs: u64) {
        let now = Timestamp::now();
        assert_eq!(
            parse_retry_after(value, now),
            Some(Duration::from_secs(expected_secs))
        );
    }

    #[rstest]
    fn test_parse_retry_after_http_date() {
        let now: Timestamp = "1994-11-06T08:49:36Z".parse().unwrap();
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT", now),
            Some(Duration::from_secs(1))
        );
    }

    #[rstest]
    fn test_parse_retry_after_http_date_in_past_is_zero() {
        let now: Timestamp = "1994-11-06T08:49:36Z".parse().unwrap();
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:35 GMT", now),
            Some(Duration::ZERO)
        );
    }

    #[rstest]
    fn test_parse_retry_after_invalid() {
        let now = Timestamp::now();
        assert_eq!(parse_retry_after("not-a-delay", now), None);
    }

    #[rstest]
    fn test_retry_after_missing_header() {
        let headers = HashMap::new();
        assert_eq!(retry_after(&headers, Timestamp::now()), None);
    }
}
