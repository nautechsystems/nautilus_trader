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

//! Timestamp-scale checks for live testers.

use nautilus_common::log_warn;
use nautilus_core::UnixNanos;

// `10^16` nanoseconds is about 116 days after 1970-01-01. Venue timestamps left in
// seconds, milliseconds, or microseconds fall below this bound.
const MIN_PLAUSIBLE_UNIX_NANOS: u64 = 10_000_000_000_000_000;

const fn unix_nanos_scale_is_plausible(timestamp: UnixNanos) -> bool {
    timestamp.as_u64() >= MIN_PLAUSIBLE_UNIX_NANOS
}

/// Logs a warning when `ts_event` or `ts_init` is not a plausible Unix-nanosecond value.
pub(super) fn warn_if_implausible_unix_nanos(kind: &str, ts_event: UnixNanos, ts_init: UnixNanos) {
    warn_if_implausible_named(kind, "ts_event", ts_event);
    warn_if_implausible_named(kind, "ts_init", ts_init);
}

/// Logs a warning when an optional timestamp is present and not nanosecond-scale.
pub(super) fn warn_if_implausible_optional(kind: &str, field: &str, timestamp: Option<UnixNanos>) {
    if let Some(timestamp) = timestamp {
        warn_if_implausible_named(kind, field, timestamp);
    }
}

fn warn_if_implausible_named(kind: &str, field: &str, timestamp: UnixNanos) {
    if !unix_nanos_scale_is_plausible(timestamp) {
        log_warn!(
            "Implausible Unix-nanosecond scale for {kind} {field}={timestamp}; value looks like leftover seconds, milliseconds, or microseconds"
        );
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(0, false)]
    #[case(1_770_000_000, false)]
    #[case(1_770_000_000_000, false)]
    #[case(1_770_000_000_000_000, false)]
    #[case(MIN_PLAUSIBLE_UNIX_NANOS - 1, false)]
    #[case(MIN_PLAUSIBLE_UNIX_NANOS, true)]
    #[case(1_770_000_000_000_000_000, true)]
    fn test_unix_nanos_scale_is_plausible(#[case] raw: u64, #[case] expected: bool) {
        assert_eq!(unix_nanos_scale_is_plausible(UnixNanos::new(raw)), expected);
    }
}
