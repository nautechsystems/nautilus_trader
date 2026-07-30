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
//! Derive's [venue schema] defines a unique nonce as UTC milliseconds followed
//! by a suffix of up to three digits and illustrates the suffix as `001`. The
//! [reference SDK] accepts caller-selected suffixes in `0..=999`. The allocator
//! uses a fixed three-digit suffix field: `utc_ms * 1000 + suffix`.
//!
//! Manager guarantees:
//!
//! 1. Unique and monotonically increasing per `(wallet, subaccount)` across
//!    every manager in the process, including during clock rollback.
//! 2. Suffixes stay within the venue's documented `0..=999` range.
//! 3. Atomic allocation per key under contention. A process-wide `DashMap`
//!    shards the state and a `compare_exchange` loop serialises allocators.
//!
//! [venue schema]: https://docs.derive.xyz/reference/private-replace
//! [reference SDK]: https://github.com/derivexyz/v2-action-signing-python/blob/d1914d61985e33559244da242892c7255b6fd0ca/derive_action_signing/utils.py#L19-L29

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use dashmap::DashMap;
use thiserror::Error;

use crate::signing::encoding::utc_now_ms;

const NONCE_SUFFIX_BASE: u64 = 1_000;
const NONCE_SUFFIX_MAX: u64 = NONCE_SUFFIX_BASE - 1;
const NONCE_UNINITIALIZED: u64 = u64::MAX;

/// Errors raised by [`NonceManager`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NonceError {
    /// The system clock is before the UNIX epoch.
    #[error("system clock is before UNIX epoch")]
    ClockBeforeEpoch,
    /// All suffixes for the current logical millisecond have been allocated.
    #[error("nonce suffix range exhausted for millisecond {millisecond}")]
    SuffixExhausted { millisecond: u64 },
    /// The millisecond timestamp cannot fit in the nonce's fixed-width prefix.
    #[error("millisecond timestamp {milliseconds} overflows the nonce format")]
    TimestampOverflow { milliseconds: u64 },
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
    /// system clock as the millisecond reference.
    ///
    /// # Errors
    ///
    /// Returns an error when the system clock is invalid, the timestamp
    /// cannot be encoded, or the suffix range is exhausted.
    pub fn next_nonce(&self, wallet: &str, subaccount_id: u64) -> Result<u64, NonceError> {
        let now_ms = utc_now_ms().map_err(|_| NonceError::ClockBeforeEpoch)?;
        self.next_nonce_at(wallet, subaccount_id, now_ms)
    }

    /// Allocates the next nonce for `(wallet, subaccount_id)` with an
    /// injected `now_ms`, suitable for deterministic testing.
    ///
    /// # Errors
    ///
    /// Returns an error when the timestamp cannot be encoded or all suffixes
    /// for the current logical millisecond have been allocated.
    pub fn next_nonce_at(
        &self,
        wallet: &str,
        subaccount_id: u64,
        now_ms: u64,
    ) -> Result<u64, NonceError> {
        let initial =
            now_ms
                .checked_mul(NONCE_SUFFIX_BASE)
                .ok_or(NonceError::TimestampOverflow {
                    milliseconds: now_ms,
                })?;
        let state = self.state_for(wallet, subaccount_id);

        loop {
            let last = state.load(Ordering::Acquire);
            let candidate = if last == NONCE_UNINITIALIZED || initial > last {
                initial
            } else {
                if last % NONCE_SUFFIX_BASE == NONCE_SUFFIX_MAX {
                    return Err(NonceError::SuffixExhausted {
                        millisecond: last / NONCE_SUFFIX_BASE,
                    });
                }
                let next = last.checked_add(1).ok_or(NonceError::NonceOverflow)?;
                if next == NONCE_UNINITIALIZED {
                    return Err(NonceError::NonceOverflow);
                }
                next
            };

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

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc as StdArc, Barrier},
        thread,
    };

    use rstest::rstest;

    use super::*;

    const NOW_MS: u64 = 1_700_000_000_000;
    const NONCE_START: u64 = NOW_MS * NONCE_SUFFIX_BASE;
    const WALLET_A: &str = "0x000000000000000000000000000000000000aaaa";
    const WALLET_B: &str = "0x000000000000000000000000000000000000bbbb";

    #[rstest]
    fn test_next_nonce_at_first_call_uses_zero_suffix() {
        let mgr = NonceManager::new();
        let nonce = mgr.next_nonce_at(WALLET_A, 1, NOW_MS).unwrap();

        assert_eq!(nonce, 1_700_000_000_000_000);
    }

    #[rstest]
    fn test_sequential_calls_within_same_ms_are_monotonic() {
        let mgr = NonceManager::new();
        let nonces = [
            mgr.next_nonce_at(WALLET_A, 2, NOW_MS).unwrap(),
            mgr.next_nonce_at(WALLET_A, 2, NOW_MS).unwrap(),
            mgr.next_nonce_at(WALLET_A, 2, NOW_MS).unwrap(),
        ];

        assert_eq!(nonces, [NONCE_START, NONCE_START + 1, NONCE_START + 2]);
    }

    #[rstest]
    fn test_separate_managers_share_state() {
        let first = NonceManager::new()
            .next_nonce_at(WALLET_A, 3, NOW_MS)
            .unwrap();
        let second = NonceManager::new()
            .next_nonce_at(WALLET_A, 3, NOW_MS)
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

        let barrier = StdArc::new(Barrier::new(THREADS as usize));
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let barrier = StdArc::clone(&barrier);

                thread::spawn(move || {
                    let mgr = NonceManager::new();
                    barrier.wait();
                    mgr.next_nonce_at(WALLET_A, 4, NOW_MS).unwrap()
                })
            })
            .collect();
        let mut nonces: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        nonces.sort_unstable();

        let expected: Vec<_> = (0..THREADS).map(|suffix| NONCE_START + suffix).collect();
        assert_eq!(nonces, expected);
    }

    #[rstest]
    fn test_advancing_clock_starts_new_suffix_range() {
        let mgr = NonceManager::new();
        let first = mgr.next_nonce_at(WALLET_A, 5, NOW_MS).unwrap();
        let second = mgr.next_nonce_at(WALLET_A, 5, NOW_MS + 1).unwrap();

        assert_eq!(first, NONCE_START);
        assert_eq!(second, NONCE_START + NONCE_SUFFIX_BASE);
    }

    #[rstest]
    fn test_clock_rollback_advances_last_logical_millisecond() {
        let first = NonceManager::new()
            .next_nonce_at(WALLET_A, 6, NOW_MS + 10)
            .unwrap();
        let second = NonceManager::new()
            .next_nonce_at(WALLET_A, 6, NOW_MS)
            .unwrap();

        assert_eq!(first, (NOW_MS + 10) * NONCE_SUFFIX_BASE);
        assert_eq!(second, first + 1);
    }

    #[rstest]
    fn test_distinct_wallets_track_independent_state() {
        let mgr = NonceManager::new();
        let first_a = mgr.next_nonce_at(WALLET_A, 7, NOW_MS).unwrap();
        let first_b = mgr.next_nonce_at(WALLET_B, 7, NOW_MS).unwrap();
        let second_a = mgr.next_nonce_at(WALLET_A, 7, NOW_MS).unwrap();

        assert_eq!(first_a, NONCE_START);
        assert_eq!(first_b, NONCE_START);
        assert_eq!(second_a, NONCE_START + 1);
        assert_eq!(mgr.last_issued(WALLET_A, 7), Some(second_a));
        assert_eq!(mgr.last_issued(WALLET_B, 7), Some(first_b));
    }

    #[rstest]
    fn test_distinct_subaccounts_track_independent_state() {
        let mgr = NonceManager::new();
        let first = mgr.next_nonce_at(WALLET_A, 8, NOW_MS).unwrap();
        let second = mgr.next_nonce_at(WALLET_A, 9, NOW_MS).unwrap();

        assert_eq!(first, NONCE_START);
        assert_eq!(second, NONCE_START);
    }

    #[rstest]
    fn test_checksum_and_lowercase_wallet_share_state() {
        let lowercase = "0x000000000000000000000000000000000000abcd";
        let checksum = "0x000000000000000000000000000000000000ABCD";
        let first = NonceManager::new()
            .next_nonce_at(lowercase, 10, NOW_MS)
            .unwrap();
        let second = NonceManager::new()
            .next_nonce_at(checksum, 10, NOW_MS)
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

        let nonce = mgr.next_nonce_at(WALLET_A, 11, NOW_MS).unwrap();

        assert_eq!(nonce, NONCE_START);
        assert_eq!(mgr.last_issued(WALLET_A, 11), Some(NONCE_START));
    }

    #[rstest]
    fn test_suffix_exhaustion_stops_after_suffix_999() {
        let mgr = NonceManager::new();
        for suffix in 0..=NONCE_SUFFIX_MAX {
            let nonce = mgr.next_nonce_at(WALLET_A, 12, NOW_MS).unwrap();
            assert_eq!(nonce, NONCE_START + suffix);
        }

        assert_eq!(
            mgr.next_nonce_at(WALLET_A, 12, NOW_MS),
            Err(NonceError::SuffixExhausted {
                millisecond: NOW_MS,
            }),
        );
        assert_eq!(
            mgr.last_issued(WALLET_A, 12),
            Some(NONCE_START + NONCE_SUFFIX_MAX),
        );
    }

    #[rstest]
    fn test_suffix_exhaustion_during_clock_rollback_reports_logical_millisecond() {
        let mgr = NonceManager::new();
        for suffix in 0..=NONCE_SUFFIX_MAX {
            let nonce = mgr.next_nonce_at(WALLET_A, 13, NOW_MS).unwrap();
            assert_eq!(nonce, NONCE_START + suffix);
        }

        assert_eq!(
            NonceManager::new().next_nonce_at(WALLET_A, 13, NOW_MS - 1),
            Err(NonceError::SuffixExhausted {
                millisecond: NOW_MS,
            }),
        );
    }

    #[rstest]
    fn test_timestamp_overflow_does_not_create_stream_state() {
        let now_ms = (u64::MAX / NONCE_SUFFIX_BASE) + 1;
        let mgr = NonceManager::new();

        assert_eq!(
            mgr.next_nonce_at(WALLET_A, 14, now_ms),
            Err(NonceError::TimestampOverflow {
                milliseconds: now_ms,
            }),
        );
        assert_eq!(mgr.last_issued(WALLET_A, 14), None);
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
        let now_ms = u64::MAX / NONCE_SUFFIX_BASE;
        let start = now_ms * NONCE_SUFFIX_BASE;
        let mgr = NonceManager::new();
        for suffix in 0..(u64::MAX - start) {
            let nonce = mgr.next_nonce_at(WALLET_A, 15, now_ms).unwrap();
            assert_eq!(nonce, start + suffix);
        }

        assert_eq!(
            mgr.next_nonce_at(WALLET_A, 15, now_ms),
            Err(NonceError::NonceOverflow),
        );
        assert_eq!(mgr.last_issued(WALLET_A, 15), Some(u64::MAX - 1));
    }

    #[rstest]
    fn test_next_nonce_uses_system_clock_when_called_without_injection() {
        let mgr = NonceManager::new();
        let nonce = mgr.next_nonce(WALLET_A, 16).unwrap();

        assert!(nonce > NONCE_START);
        assert_eq!(nonce % NONCE_SUFFIX_BASE, 0);
    }
}
