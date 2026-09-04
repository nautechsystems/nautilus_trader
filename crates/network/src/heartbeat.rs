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

//! Shared heartbeat timeout derivation for socket and WebSocket transports.

/// Heartbeat intervals tolerated before an unset `heartbeat_timeout_secs` tears the connection down.
///
/// Sending a heartbeat implies the peer answers it, so a configured interval is enough to derive a
/// liveness window without each adapter restating one. Three cycles tolerate two lost replies.
pub(crate) const DEFAULT_HEARTBEAT_TIMEOUT_INTERVALS: u64 = 3;

/// Resolves the liveness window, defaulting to a multiple of the heartbeat interval.
///
/// An explicit `heartbeat_timeout_secs` always wins. A transport with no heartbeat gets no default:
/// nothing would guarantee the inbound traffic needed to keep the window open.
pub(crate) fn resolve_heartbeat_timeout(
    heartbeat_timeout_secs: Option<u64>,
    heartbeat_interval_secs: Option<u64>,
) -> Option<u64> {
    heartbeat_timeout_secs.or(heartbeat_interval_secs
        .map(|secs| secs.saturating_mul(DEFAULT_HEARTBEAT_TIMEOUT_INTERVALS)))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::resolve_heartbeat_timeout;

    #[rstest]
    #[case::none_without_heartbeat(None, None, None)]
    #[case::derived_from_interval(None, Some(30), Some(90))]
    #[case::derived_from_short_interval(None, Some(5), Some(15))]
    #[case::explicit_wins(Some(45), Some(30), Some(45))]
    #[case::explicit_without_heartbeat(Some(60), None, Some(60))]
    fn test_resolve_heartbeat_timeout(
        #[case] timeout_secs: Option<u64>,
        #[case] interval_secs: Option<u64>,
        #[case] expected: Option<u64>,
    ) {
        assert_eq!(
            resolve_heartbeat_timeout(timeout_secs, interval_secs),
            expected
        );
    }
}
