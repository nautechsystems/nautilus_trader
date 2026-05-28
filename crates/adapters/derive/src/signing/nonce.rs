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

//! Per-`(wallet, subaccount)` nonce manager for Derive self-custodial requests.
//!
//! Nonce format (matching `derive_action_signing/utils.py::get_action_nonce`):
//! `int(str(utc_ms) || str(suffix))`. The suffix is a non-negative integer
//! whose decimal representation is concatenated to the millisecond timestamp.
//!
//! Manager guarantees:
//!
//! 1. Monotonically increasing per `(wallet, subaccount)`. The venue rejects
//!    a nonce equal to or below the last accepted nonce for the signer, so
//!    the manager always issues `max(now_ms_with_suffix0, last_issued + 1)`.
//! 2. Lock-free per key under contention. A `DashMap` shards the per-key
//!    state and a `compare_exchange` loop on a single `AtomicU64` serialises
//!    concurrent allocators.
//!
//! Cross-instance ordering (multiple processes signing for the same key) is
//! the caller's responsibility; the venue's `nextNonce`-style endpoint should
//! seed the manager via [`NonceManager::refresh`] at startup.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use dashmap::DashMap;
use thiserror::Error;

use crate::signing::encoding::utc_now_ms;

/// Errors raised by [`NonceManager`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NonceError {
    /// The system clock is before the UNIX epoch.
    #[error("system clock is before UNIX epoch")]
    ClockBeforeEpoch,
}

/// Thread-safe sequential nonce allocator keyed by `(wallet, subaccount_id)`.
#[derive(Debug, Default)]
pub struct NonceManager {
    states: DashMap<(String, u64), Arc<AtomicU64>>,
}

impl NonceManager {
    /// Constructs an empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates the next nonce for `(wallet, subaccount_id)` using the
    /// system clock as the millisecond reference.
    ///
    /// # Errors
    ///
    /// Returns [`NonceError::ClockBeforeEpoch`] when the system clock is
    /// invalid.
    pub fn next_nonce(&self, wallet: &str, subaccount_id: u64) -> Result<u64, NonceError> {
        let now_ms = utc_now_ms().map_err(|_| NonceError::ClockBeforeEpoch)?;
        Ok(self.next_nonce_at(wallet, subaccount_id, now_ms))
    }

    /// Allocates the next nonce for `(wallet, subaccount_id)` with an
    /// injected `now_ms`, suitable for deterministic testing.
    pub fn next_nonce_at(&self, wallet: &str, subaccount_id: u64, now_ms: u64) -> u64 {
        let state = self.state_for(wallet, subaccount_id);
        // Suffix "0" -> multiply ms by 10 and append 0 (which is just *10).
        let initial = now_ms.saturating_mul(10);

        loop {
            let last = state.load(Ordering::Acquire);
            let candidate = if initial > last { initial } else { last + 1 };
            if state
                .compare_exchange_weak(last, candidate, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return candidate;
            }
        }
    }

    /// Aligns the local last-issued state with a venue-reported `nextNonce`,
    /// monotonically. The stored value advances to `last_seen_nonce` only
    /// when it is strictly greater than the current state, otherwise the
    /// call is a no-op.
    ///
    /// Monotonicity matters because rewinding would re-issue nonces already
    /// dispatched on the wire, which the venue rejects as replays. A stale
    /// or out-of-date venue snapshot must never clobber more recent local
    /// allocations.
    pub fn refresh(&self, wallet: &str, subaccount_id: u64, last_seen_nonce: u64) {
        let state = self.state_for(wallet, subaccount_id);
        loop {
            let current = state.load(Ordering::Acquire);
            if last_seen_nonce <= current {
                return;
            }

            if state
                .compare_exchange_weak(
                    current,
                    last_seen_nonce,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return;
            }
        }
    }

    /// Returns the most recently issued nonce for a key, if any.
    #[must_use]
    pub fn last_issued(&self, wallet: &str, subaccount_id: u64) -> Option<u64> {
        self.states
            .get(&Self::normalize_key(wallet, subaccount_id))
            .map(|s| s.load(Ordering::Acquire))
            .filter(|n| *n != 0)
    }

    fn state_for(&self, wallet: &str, subaccount_id: u64) -> Arc<AtomicU64> {
        let entry = self
            .states
            .entry(Self::normalize_key(wallet, subaccount_id))
            .or_insert_with(|| Arc::new(AtomicU64::new(0)));
        entry.value().clone()
    }

    // Lowercase the wallet hex so checksum and lowercase forms of the same
    // EVM address share a single nonce stream; mixing them would otherwise
    // issue duplicate nonces for the same on-chain account. All read and
    // write paths must route through here to stay symmetrical.
    fn normalize_key(wallet: &str, subaccount_id: u64) -> (String, u64) {
        (wallet.to_ascii_lowercase(), subaccount_id)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc as StdArc, thread};

    use rstest::rstest;

    use super::*;

    const WALLET_A: &str = "0x000000000000000000000000000000000000aaaa";
    const WALLET_B: &str = "0x000000000000000000000000000000000000bbbb";

    #[rstest]
    fn test_next_nonce_at_first_call_concatenates_zero_suffix() {
        let mgr = NonceManager::new();
        let nonce = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        // ms appended with "0" -> ms * 10
        assert_eq!(nonce, 17_000_000_000_000);
    }

    #[rstest]
    fn test_sequential_calls_within_same_ms_are_monotonic() {
        let mgr = NonceManager::new();
        let n1 = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        let n2 = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        let n3 = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        assert!(
            n1 < n2 && n2 < n3,
            "nonces must be monotonic, was {n1}, {n2}, {n3}"
        );
        assert_eq!(n2 - n1, 1);
        assert_eq!(n3 - n2, 1);
    }

    #[rstest]
    fn test_advancing_clock_jumps_to_new_prefix() {
        let mgr = NonceManager::new();
        let n1 = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        let n2 = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_001);
        // n1 = ms * 10 = 17000000000000
        // n2 should restart at ms2 * 10 = 17000000000010, larger by 10.
        assert_eq!(n1, 17_000_000_000_000);
        assert_eq!(n2, 17_000_000_000_010);
        assert!(n2 > n1);
    }

    #[rstest]
    fn test_distinct_keys_track_independent_state() {
        let mgr = NonceManager::new();
        let a = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        let b = mgr.next_nonce_at(WALLET_B, 1, 1_700_000_000_000);
        // Same ms reference, but each wallet starts from scratch
        assert_eq!(a, b);
        // And subsequent calls advance independently
        let a2 = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        assert_eq!(a2, a + 1);
        assert_eq!(mgr.last_issued(WALLET_B, 1), Some(b));
    }

    #[rstest]
    fn test_distinct_subaccounts_track_independent_state() {
        let mgr = NonceManager::new();
        let a1 = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        let a2 = mgr.next_nonce_at(WALLET_A, 2, 1_700_000_000_000);
        assert_eq!(a1, a2);
    }

    #[rstest]
    fn test_refresh_advances_last_issued() {
        let mgr = NonceManager::new();
        mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        mgr.refresh(WALLET_A, 1, 99_999_999_999_999);
        let n = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        assert_eq!(n, 99_999_999_999_999 + 1);
    }

    #[rstest]
    fn test_refresh_never_rewinds_below_local_state() {
        let mgr = NonceManager::new();
        // Issue a high local nonce, then accept a stale (lower) venue
        // snapshot. The stored state must not rewind, otherwise the next
        // allocation would re-issue an already-dispatched nonce.
        let high = mgr.next_nonce_at(WALLET_A, 1, 9_000_000_000_000);
        mgr.refresh(WALLET_A, 1, 1_000);
        let next = mgr.next_nonce_at(WALLET_A, 1, 1_000);
        assert!(
            next > high,
            "refresh below local state must not rewind, last={high}, next={next}",
        );
    }

    #[rstest]
    fn test_checksum_and_lowercase_wallet_share_state() {
        let mgr = NonceManager::new();
        let lowercase = "0x000000000000000000000000000000000000abcd";
        let checksum = "0x000000000000000000000000000000000000ABCD";
        let n1 = mgr.next_nonce_at(lowercase, 1, 1_700_000_000_000);
        let n2 = mgr.next_nonce_at(checksum, 1, 1_700_000_000_000);
        assert_eq!(
            n2,
            n1 + 1,
            "checksum and lowercase forms of the same address must share one nonce stream",
        );
    }

    #[rstest]
    fn test_last_issued_finds_state_regardless_of_address_case() {
        let mgr = NonceManager::new();
        let lowercase = "0x000000000000000000000000000000000000abcd";
        let checksum = "0x000000000000000000000000000000000000ABCD";
        let issued = mgr.next_nonce_at(lowercase, 1, 1_700_000_000_000);
        assert_eq!(mgr.last_issued(lowercase, 1), Some(issued));
        assert_eq!(
            mgr.last_issued(checksum, 1),
            Some(issued),
            "lookup must normalize the same way as next_nonce/refresh",
        );
    }

    #[rstest]
    fn test_last_issued_reports_latest_value() {
        let mgr = NonceManager::new();
        assert_eq!(mgr.last_issued(WALLET_A, 1), None);
        let n = mgr.next_nonce_at(WALLET_A, 1, 1_700_000_000_000);
        assert_eq!(mgr.last_issued(WALLET_A, 1), Some(n));
    }

    #[rstest]
    fn test_concurrent_callers_see_no_duplicates_or_gaps() {
        let mgr = StdArc::new(NonceManager::new());
        let threads = 8;
        let per_thread = 250;
        let now_ms = 1_700_000_000_000;
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let mgr = StdArc::clone(&mgr);

                thread::spawn(move || -> Vec<u64> {
                    (0..per_thread)
                        .map(|_| mgr.next_nonce_at(WALLET_A, 1, now_ms))
                        .collect()
                })
            })
            .collect();
        let mut all = Vec::with_capacity(threads * per_thread);
        for h in handles {
            all.extend(h.join().unwrap());
        }
        all.sort_unstable();
        let total = (threads * per_thread) as u64;
        let expected: Vec<u64> = (0..total).map(|i| now_ms * 10 + i).collect();
        assert_eq!(
            all, expected,
            "concurrent issuance must be contiguous from ms*10",
        );
    }

    #[rstest]
    fn test_next_nonce_uses_system_clock_when_called_without_injection() {
        let mgr = NonceManager::new();
        let n = mgr.next_nonce(WALLET_A, 1).unwrap();
        // System clock must be past Jan 2026 (1.7e12 ms), and the nonce is
        // ms * 10 so it must exceed 1.7e13.
        assert!(n > 17_000_000_000_000, "nonce too small: {n}");
    }
}
