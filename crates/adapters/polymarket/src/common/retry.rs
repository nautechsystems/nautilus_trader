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

//! Retry timing utilities for the Polymarket adapter.

use std::time::Duration;

use rand::RngExt;

/// Computes the auto-load retry sleep duration for `attempt`.
///
/// Matches the Python adapter's `auto_load_retry_delay` formula: takes
/// `base_secs * 2^attempt`, caps at `max_secs`, then adds positive jitter of up
/// to 25% of the capped value. Jitter decorrelates concurrent retries after a
/// venue lifecycle race so they do not all rehit Gamma at the same instant.
#[must_use]
pub fn auto_load_retry_delay(attempt: u32, base_secs: f64, max_secs: f64) -> Duration {
    auto_load_retry_delay_with_jitter(
        attempt,
        base_secs,
        max_secs,
        rand::rng().random_range(0.0..1.0),
    )
}

#[must_use]
pub(crate) fn auto_load_retry_delay_with_jitter(
    attempt: u32,
    base_secs: f64,
    max_secs: f64,
    jitter_unit: f64,
) -> Duration {
    let base = base_secs.max(0.0);
    let max = max_secs.max(base);
    let raw = base * 2f64.powi(attempt.min(30) as i32);
    let capped = raw.min(max);
    let total = capped + capped * 0.25 * jitter_unit.clamp(0.0, 1.0);
    Duration::from_secs_f64(total.max(0.0))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_attempt_zero_no_jitter_returns_base() {
        let d = auto_load_retry_delay_with_jitter(0, 5.0, 15.0, 0.0);
        assert_eq!(d, Duration::from_secs_f64(5.0));
    }

    #[rstest]
    fn test_attempt_zero_full_jitter_returns_base_plus_25_percent() {
        let d = auto_load_retry_delay_with_jitter(0, 5.0, 15.0, 1.0);
        assert!((d.as_secs_f64() - 6.25).abs() < 1e-9);
    }

    #[rstest]
    fn test_attempt_exponentiates_until_max_cap() {
        let d = auto_load_retry_delay_with_jitter(2, 5.0, 15.0, 0.0);
        assert_eq!(d, Duration::from_secs_f64(15.0));
    }

    #[rstest]
    fn test_capped_delay_still_takes_jitter() {
        let d = auto_load_retry_delay_with_jitter(2, 5.0, 15.0, 1.0);
        assert!((d.as_secs_f64() - 18.75).abs() < 1e-9);
    }

    #[rstest]
    fn test_high_attempt_count_does_not_overflow() {
        let d = auto_load_retry_delay_with_jitter(50, 5.0, 15.0, 0.0);
        assert_eq!(d, Duration::from_secs_f64(15.0));
    }

    #[rstest]
    fn test_zero_base_returns_zero() {
        let d = auto_load_retry_delay_with_jitter(3, 0.0, 15.0, 0.5);
        assert_eq!(d, Duration::ZERO);
    }
}
