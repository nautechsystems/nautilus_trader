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

//! Shared parsing utilities for the Kalshi adapter.

use chrono::DateTime;
use nautilus_core::UnixNanos;

/// Parse an ISO 8601 / RFC 3339 datetime string to [`UnixNanos`].
///
/// # Errors
///
/// Returns an error if the string is not a valid RFC 3339 datetime.
pub fn parse_datetime_to_nanos(s: &str) -> anyhow::Result<UnixNanos> {
    let dt = DateTime::parse_from_rfc3339(s)
        .map_err(|e| anyhow::anyhow!("Invalid datetime '{s}': {e}"))?;
    let nanos = dt
        .timestamp_nanos_opt()
        .ok_or_else(|| anyhow::anyhow!("Datetime out of nanosecond range: {s}"))?;
    let nanos_u64 = u64::try_from(nanos)
        .map_err(|_| anyhow::anyhow!("Datetime predates Unix epoch: {s}"))?;
    Ok(UnixNanos::from(nanos_u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rfc3339_datetime() {
        let nanos = parse_datetime_to_nanos("2025-03-15T23:59:59Z").unwrap();
        assert!(u64::from(nanos) > 0);
    }

    #[test]
    fn test_parse_invalid_datetime_fails() {
        let result = parse_datetime_to_nanos("not-a-date");
        assert!(result.is_err());
    }
}
