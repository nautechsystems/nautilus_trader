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

//! Process-wide `(wallet, subaccount)` nonce manager for Derive self-custodial requests.
//!
//! Nonces are UNIX nanoseconds, allocated monotonically per `(wallet, subaccount)`
//! across client instances in the same process. Clock rollback advances the last
//! issued value, provided it remains inside the inclusive order window
//! [now minus 90 days, now plus 1 hour].
//! Local time approximates server time; a clock offset beyond the window still
//! causes venue rejection. Login timestamps remain milliseconds.
//!
//! A process-wide `DashMap` shares state and a compare-exchange loop serializes
//! allocations under contention. Wallet hex is normalized to lowercase.

use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::UNIX_EPOCH,
};

use dashmap::DashMap;
use serde::{Deserialize, Deserializer, Serializer, de::Error};
use thiserror::Error;

const NONCE_PAST_NS: u64 = 90 * 24 * 60 * 60 * 1_000_000_000;
pub(crate) const NONCE_FUTURE_NS: u64 = 60 * 60 * 1_000_000_000;
const NONCE_UNINITIALIZED: u64 = u64::MAX;

/// Errors raised by [`NonceManager`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NonceError {
    /// The system clock is before the UNIX epoch.
    #[error("system clock is before UNIX epoch")]
    ClockBeforeEpoch,
    /// The clock's nanosecond timestamp exceeds the unsigned 64-bit range.
    #[error("nanosecond timestamp overflows u64")]
    TimestampOverflow,
    /// The candidate is outside the inclusive order nonce window.
    #[error("nonce {nonce} is outside the order window relative to time {now_ns}")]
    OutsideWindow {
        /// Candidate nonce.
        nonce: u64,
        /// Reference UNIX time in nanoseconds.
        now_ns: u64,
    },
    /// The next nonce cannot fit in an unsigned 64-bit integer.
    #[error("next nonce exceeds u64::MAX")]
    NonceOverflow,
}

/// Thread-safe process-wide nonce allocator keyed by `(wallet, subaccount_id)`.
#[derive(Debug, Default)]
pub struct NonceManager;

impl NonceManager {
    /// Constructs a manager backed by the process-wide nonce registry.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Allocates the next nonce for `(wallet, subaccount_id)` using the
    /// system clock as the nanosecond reference.
    ///
    /// # Errors
    ///
    /// Returns an error when the system clock is invalid, the timestamp
    /// cannot be encoded, or rollback places the nonce outside the order window.
    pub fn next_nonce(&self, wallet: &str, subaccount_id: u64) -> Result<u64, NonceError> {
        let now_ns = nautilus_core::time::wall_clock_now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| NonceError::ClockBeforeEpoch)?
            .as_nanos()
            .try_into()
            .map_err(|_| NonceError::TimestampOverflow)?;
        self.next_nonce_at(wallet, subaccount_id, now_ns)
    }

    /// Allocates the next nonce for `(wallet, subaccount_id)` with
    /// caller-supplied UNIX time in nanoseconds.
    ///
    /// # Errors
    ///
    /// Returns an error on overflow or when rollback places the next nonce
    /// outside the inclusive order window.
    pub fn next_nonce_at(
        &self,
        wallet: &str,
        subaccount_id: u64,
        now_ns: u64,
    ) -> Result<u64, NonceError> {
        if now_ns == NONCE_UNINITIALIZED {
            return Err(NonceError::NonceOverflow);
        }

        let state = self.state_for(wallet, subaccount_id);
        loop {
            let last = state.load(Ordering::Acquire);

            let candidate = if last == NONCE_UNINITIALIZED || now_ns > last {
                now_ns
            } else {
                let next = last.checked_add(1).ok_or(NonceError::NonceOverflow)?;
                if next == NONCE_UNINITIALIZED {
                    return Err(NonceError::NonceOverflow);
                }

                next
            };

            validate_nonce_at(candidate, now_ns)?;
            if state
                .compare_exchange_weak(last, candidate, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(candidate);
            }
        }
    }

    /// Returns the most recently issued nonce for a key, if any.
    #[must_use]
    pub fn last_issued(&self, wallet: &str, subaccount_id: u64) -> Option<u64> {
        Self::states()
            .get(&Self::normalize_key(wallet, subaccount_id))
            .map(|s| s.load(Ordering::Acquire))
            .filter(|n| *n != NONCE_UNINITIALIZED)
    }

    fn state_for(&self, wallet: &str, subaccount_id: u64) -> Arc<AtomicU64> {
        let entry = Self::states()
            .entry(Self::normalize_key(wallet, subaccount_id))
            .or_insert_with(|| Arc::new(AtomicU64::new(NONCE_UNINITIALIZED)));
        entry.value().clone()
    }

    // Lowercase the wallet hex so checksum and lowercase forms of the same
    // EVM address share a single nonce stream; mixing them would otherwise
    // issue duplicate nonces for the same on-chain account. All read and
    // write paths must route through here to stay symmetrical.
    fn normalize_key(wallet: &str, subaccount_id: u64) -> (String, u64) {
        (wallet.to_ascii_lowercase(), subaccount_id)
    }

    fn states() -> &'static DashMap<(String, u64), Arc<AtomicU64>> {
        static STATES: OnceLock<DashMap<(String, u64), Arc<AtomicU64>>> = OnceLock::new();
        STATES.get_or_init(DashMap::new)
    }
}

pub(crate) fn serialize_nonce<S>(nonce: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_str(nonce)
}

pub(crate) fn deserialize_nonce<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    value.parse::<u64>().map_err(D::Error::custom)
}

fn validate_nonce_at(nonce: u64, now_ns: u64) -> Result<(), NonceError> {
    if nonce < now_ns.saturating_sub(NONCE_PAST_NS)
        || nonce > now_ns.saturating_add(NONCE_FUTURE_NS)
    {
        return Err(NonceError::OutsideWindow { nonce, now_ns });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc as StdArc, Barrier},
        thread,
    };

    use rstest::rstest;

    use super::*;

    const NOW_NS: u64 = 1_700_000_000_000_000_000;
    const NONCE_START: u64 = NOW_NS;
    const WALLET_A: &str = "0x000000000000000000000000000000000000aaaa";
    const WALLET_B: &str = "0x000000000000000000000000000000000000bbbb";

    #[rstest]
    fn test_next_nonce_at_first_call_uses_nanoseconds() {
        let mgr = NonceManager::new();
        let nonce = mgr.next_nonce_at(WALLET_A, 1, NOW_NS).unwrap();

        assert_eq!(nonce, 1_700_000_000_000_000_000);
    }

    #[rstest]
    fn test_sequential_calls_within_same_ns_are_monotonic() {
        let mgr = NonceManager::new();
        let nonces = [
            mgr.next_nonce_at(WALLET_A, 2, NOW_NS).unwrap(),
            mgr.next_nonce_at(WALLET_A, 2, NOW_NS).unwrap(),
            mgr.next_nonce_at(WALLET_A, 2, NOW_NS).unwrap(),
        ];

        assert_eq!(nonces, [NONCE_START, NONCE_START + 1, NONCE_START + 2]);
    }

    #[rstest]
    fn test_separate_managers_share_state() {
        let first = NonceManager::new()
            .next_nonce_at(WALLET_A, 3, NOW_NS)
            .unwrap();
        let second = NonceManager::new()
            .next_nonce_at(WALLET_A, 3, NOW_NS)
            .unwrap();

        assert_eq!(first, NONCE_START);
        assert_eq!(second, NONCE_START + 1);
    }

    #[rstest]
    #[expect(
        clippy::needless_collect,
        reason = "all threads must start before any can pass the barrier"
    )]
    fn test_simultaneous_managers_allocate_unique_ordered_range() {
        const THREADS: u64 = 8;
        const ALLOCATIONS: u64 = 128;

        let barrier = StdArc::new(Barrier::new(THREADS as usize));
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let barrier = StdArc::clone(&barrier);

                thread::spawn(move || {
                    let mgr = NonceManager::new();
                    barrier.wait();
                    (0..ALLOCATIONS)
                        .map(|_| mgr.next_nonce_at(WALLET_A, 4, NOW_NS).unwrap())
                        .collect::<Vec<_>>()
                })
            })
            .collect();

        let mut nonces: Vec<_> = handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect();
        nonces.sort_unstable();

        let expected: Vec<_> = (0..THREADS * ALLOCATIONS)
            .map(|offset| NONCE_START + offset)
            .collect();
        assert_eq!(nonces, expected);
    }

    #[rstest]
    fn test_advancing_clock_uses_nanosecond_timestamp() {
        let mgr = NonceManager::new();
        let first = mgr.next_nonce_at(WALLET_A, 5, NOW_NS).unwrap();
        let second = mgr.next_nonce_at(WALLET_A, 5, NOW_NS + 1).unwrap();

        assert_eq!(first, NONCE_START);
        assert_eq!(second, NONCE_START + 1);
    }

    #[rstest]
    fn test_clock_rollback_advances_last_nanosecond() {
        let first = NonceManager::new()
            .next_nonce_at(WALLET_A, 6, NOW_NS + 10)
            .unwrap();
        let second = NonceManager::new()
            .next_nonce_at(WALLET_A, 6, NOW_NS)
            .unwrap();

        assert_eq!(first, NOW_NS + 10);
        assert_eq!(second, first + 1);
    }

    #[rstest]
    fn test_distinct_wallets_track_independent_state() {
        let mgr = NonceManager::new();
        let first_a = mgr.next_nonce_at(WALLET_A, 7, NOW_NS).unwrap();
        let first_b = mgr.next_nonce_at(WALLET_B, 7, NOW_NS).unwrap();
        let second_a = mgr.next_nonce_at(WALLET_A, 7, NOW_NS).unwrap();

        assert_eq!(first_a, NONCE_START);
        assert_eq!(first_b, NONCE_START);
        assert_eq!(second_a, NONCE_START + 1);
        assert_eq!(mgr.last_issued(WALLET_A, 7), Some(second_a));
        assert_eq!(mgr.last_issued(WALLET_B, 7), Some(first_b));
    }

    #[rstest]
    fn test_distinct_subaccounts_track_independent_state() {
        let mgr = NonceManager::new();
        let first = mgr.next_nonce_at(WALLET_A, 8, NOW_NS).unwrap();
        let second = mgr.next_nonce_at(WALLET_A, 9, NOW_NS).unwrap();

        assert_eq!(first, NONCE_START);
        assert_eq!(second, NONCE_START);
    }

    #[rstest]
    fn test_checksum_and_lowercase_wallet_share_state() {
        let lowercase = "0x000000000000000000000000000000000000abcd";
        let checksum = "0x000000000000000000000000000000000000ABCD";
        let first = NonceManager::new()
            .next_nonce_at(lowercase, 10, NOW_NS)
            .unwrap();
        let second = NonceManager::new()
            .next_nonce_at(checksum, 10, NOW_NS)
            .unwrap();

        assert_eq!(first, NONCE_START);
        assert_eq!(second, NONCE_START + 1);
        assert_eq!(NonceManager::new().last_issued(lowercase, 10), Some(second),);
        assert_eq!(NonceManager::new().last_issued(checksum, 10), Some(second),);
    }

    #[rstest]
    fn test_last_issued_reports_latest_value() {
        let mgr = NonceManager::new();
        assert_eq!(mgr.last_issued(WALLET_A, 11), None);

        let nonce = mgr.next_nonce_at(WALLET_A, 11, NOW_NS).unwrap();

        assert_eq!(nonce, NONCE_START);
        assert_eq!(mgr.last_issued(WALLET_A, 11), Some(NONCE_START));
    }

    #[rstest]
    fn test_contention_has_no_millisecond_suffix_limit() {
        let mgr = NonceManager::new();

        for offset in 0..2_000 {
            assert_eq!(
                mgr.next_nonce_at(WALLET_A, 12, NOW_NS),
                Ok(NONCE_START + offset)
            );
        }

        assert_eq!(mgr.last_issued(WALLET_A, 12), Some(NONCE_START + 1_999));
    }

    #[rstest]
    #[case(NOW_NS - NONCE_PAST_NS, true)]
    #[case(NOW_NS - NONCE_PAST_NS - 1, false)]
    #[case(NOW_NS + NONCE_FUTURE_NS, true)]
    #[case(NOW_NS + NONCE_FUTURE_NS + 1, false)]
    fn test_nonce_window_is_inclusive(#[case] nonce: u64, #[case] accepted: bool) {
        let expected = if accepted {
            Ok(())
        } else {
            Err(NonceError::OutsideWindow {
                nonce,
                now_ns: NOW_NS,
            })
        };

        assert_eq!(validate_nonce_at(nonce, NOW_NS), expected);
    }

    #[rstest]
    fn test_rollback_past_future_window_preserves_state() {
        let mgr = NonceManager::new();
        assert_eq!(mgr.next_nonce_at(WALLET_A, 13, NOW_NS), Ok(NOW_NS));
        let now_ns = NOW_NS - NONCE_FUTURE_NS;
        assert_eq!(
            mgr.next_nonce_at(WALLET_A, 13, now_ns),
            Err(NonceError::OutsideWindow {
                nonce: NOW_NS + 1,
                now_ns
            })
        );
        assert_eq!(mgr.last_issued(WALLET_A, 13), Some(NOW_NS));
        assert_eq!(mgr.next_nonce_at(WALLET_A, 13, now_ns + 1), Ok(NOW_NS + 1));
    }

    #[rstest]
    fn test_window_bounds_do_not_overflow() {
        assert_eq!(validate_nonce_at(0, 0), Ok(()));
        assert_eq!(validate_nonce_at(u64::MAX - 1, u64::MAX), Ok(()));
    }

    #[rstest]
    fn test_epoch_first_call_uses_zero_nonce() {
        let mgr = NonceManager::new();
        let first = mgr.next_nonce_at(WALLET_A, 17, 0).unwrap();
        let second = mgr.next_nonce_at(WALLET_A, 17, 0).unwrap();

        assert_eq!(first, 0);
        assert_eq!(second, 1);
        assert_eq!(mgr.last_issued(WALLET_A, 17), Some(1));
    }

    #[rstest]
    fn test_nonce_overflow_does_not_emit_uninitialized_sentinel() {
        let mgr = NonceManager::new();
        assert_eq!(
            mgr.next_nonce_at(WALLET_A, 15, u64::MAX - 1),
            Ok(u64::MAX - 1)
        );
        assert_eq!(
            mgr.next_nonce_at(WALLET_A, 15, u64::MAX - 1),
            Err(NonceError::NonceOverflow)
        );
        assert_eq!(mgr.last_issued(WALLET_A, 15), Some(u64::MAX - 1));
        assert_eq!(
            mgr.next_nonce_at(WALLET_A, 14, u64::MAX),
            Err(NonceError::NonceOverflow)
        );
        assert_eq!(mgr.last_issued(WALLET_A, 14), None);
    }

    #[rstest]
    fn test_next_nonce_uses_system_clock_when_called_without_injection() {
        let mgr = NonceManager::new();
        let nonce = mgr.next_nonce(WALLET_A, 16).unwrap();

        assert!(nonce > NONCE_START);
        assert_eq!(mgr.last_issued(WALLET_A, 16), Some(nonce));
    }
}
