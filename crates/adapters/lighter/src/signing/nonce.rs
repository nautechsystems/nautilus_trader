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

//! Per-`(account_index, api_key_index)` sequential nonce manager.
//!
//! Mirrors the optimistic strategy `lighter-python`'s `OptimisticNonceManager`
//! uses: each call to [`NonceManager::next_nonce`] hands out the next monotonic
//! integer for the requested key without waiting for the venue to confirm the
//! prior one. The caller bounds the number of unconfirmed allocations through
//! the [`NonceManager::skip_window`] argument; once the window is exhausted,
//! [`NonceManager::next_nonce`] errors so the caller can drain or refresh
//! before issuing further transactions.
//!
//! The window is the L2's tolerance for out-of-order submission: the sequencer
//! accepts a tx whose nonce is up to `skip_window` ahead of the last applied
//! nonce, which is what makes the optimistic allocation correct in the first
//! place. Lower the window if the venue rejects rates of out-of-order accepts;
//! raise it when burst-trading multiple keys.
//!
//! The module is lock-free per key: a [`DashMap`] keys an [`Arc`] holding two
//! [`AtomicI64`]s (`last_issued` and `baseline`). [`NonceManager::next_nonce`]
//! is a CAS loop bounded by `skip_window`; [`NonceManager::ack_success`]
//! monotonically advances the baseline when the venue confirms a tx;
//! [`NonceManager::ack_failure_if_latest`] rolls back the most recent
//! allocation when the failed nonce is still the latest issuance.

use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

use dashmap::DashMap;
use thiserror::Error;

/// Default skip-window used when the caller does not specify one.
///
/// The venue accepts up to 16 out-of-order nonces per `(account, api_key)`;
/// matching that bound on the client keeps optimistic submissions inside the
/// sequencer's tolerance.
pub const DEFAULT_SKIP_WINDOW: u32 = 16;

/// Errors raised by [`NonceManager`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum NonceError {
    /// `next_nonce` was called before [`NonceManager::refresh`] seeded the
    /// `(account_index, api_key_index)` pair.
    #[error("nonce manager not initialized for account={account_index}, api_key={api_key_index}")]
    NotInitialized {
        /// Lighter L2 account index.
        account_index: i64,
        /// Per-account API key slot.
        api_key_index: u8,
    },
    /// More than `skip_window` nonces are outstanding for this key.
    #[error(
        "skip-window exhausted for account={account_index}, api_key={api_key_index}: outstanding={outstanding}, window={skip_window}"
    )]
    SkipWindowExhausted {
        /// Lighter L2 account index.
        account_index: i64,
        /// Per-account API key slot.
        api_key_index: u8,
        /// Number of nonces already issued past the last `refresh` baseline.
        outstanding: u32,
        /// Configured tolerance.
        skip_window: u32,
    },
    /// `ack_failure` was called while no nonce had been issued past the
    /// baseline; nothing to roll back.
    #[error(
        "no outstanding nonce to roll back for account={account_index}, api_key={api_key_index}"
    )]
    NothingToRollBack {
        /// Lighter L2 account index.
        account_index: i64,
        /// Per-account API key slot.
        api_key_index: u8,
    },
}

/// Thread-safe sequential nonce allocator keyed by `(account_index, api_key_index)`.
///
/// Construct with [`NonceManager::new`] (or [`NonceManager::default`] for
/// [`DEFAULT_SKIP_WINDOW`]) and seed each key via [`NonceManager::refresh`]
/// from the `nextNonce` REST endpoint before calling [`NonceManager::next_nonce`].
#[derive(Debug)]
pub struct NonceManager {
    skip_window: u32,
    states: DashMap<(i64, u8), Arc<AccountNonce>>,
}

impl NonceManager {
    /// Construct a manager with an explicit `skip_window` tolerance.
    #[must_use]
    pub fn new(skip_window: u32) -> Self {
        Self {
            skip_window,
            states: DashMap::new(),
        }
    }

    /// Configured outstanding-allocation bound.
    #[must_use]
    pub fn skip_window(&self) -> u32 {
        self.skip_window
    }

    /// Seed (or hard-reset) the nonce baseline for a key.
    ///
    /// `venue_next_nonce` is what the venue's `nextNonce` endpoint reports as
    /// the next expected nonce for `(account_index, api_key_index)`. After
    /// this call, the very next [`NonceManager::next_nonce`] for the same key
    /// returns `venue_next_nonce`.
    ///
    /// Call this once at startup, and again after a venue rejection signals
    /// the local view is stale (mirrors `hard_refresh_nonce` in the Python
    /// reference).
    ///
    /// Caller contract: `refresh` is a control-plane operation and is not
    /// mutually exclusive with concurrent [`NonceManager::next_nonce`] calls
    /// on the same key. A `next_nonce` that started before `refresh` may
    /// still complete with a pre-refresh value. The Python reference makes
    /// the same trade-off; callers that need exclusive semantics must
    /// serialize `refresh` against in-flight allocations themselves
    /// (typically by quiescing submission before issuing a hard refresh).
    pub fn refresh(&self, account_index: i64, api_key_index: u8, venue_next_nonce: i64) {
        let entry = self
            .states
            .entry((account_index, api_key_index))
            .or_insert_with(|| Arc::new(AccountNonce::new(venue_next_nonce - 1)));

        // Per the caller contract above, `refresh` is not mutually exclusive
        // with concurrent `next_nonce` on the same key: a racing allocator
        // may load a mixed pre/post-refresh pair. The CAS in `next_nonce`
        // detects only the `last_issued` mutation, so the documented
        // contract is what makes refresh-vs-allocate safe — not these
        // stores. Release ordering carries `baseline` to subsequent Acquire
        // loads after the caller serializes refresh quiescently.
        entry
            .baseline
            .store(venue_next_nonce - 1, Ordering::Release);
        entry
            .last_issued
            .store(venue_next_nonce - 1, Ordering::Release);
    }

    /// Monotonically advance the baseline toward the venue's reported
    /// `nextNonce` without ever moving state backwards.
    ///
    /// Unlike [`NonceManager::refresh`], which hard-resets both `baseline`
    /// and `last_issued` and can therefore reissue nonces already signed
    /// into in-flight transactions, this method only lifts values: it is
    /// safe to call while submissions are in flight. Use it to recover from
    /// [`NonceError::SkipWindowExhausted`] when the venue may have applied
    /// transactions whose acks never reached the manager (for example HTTP
    /// `sendTxBatch` submissions).
    ///
    /// `last_issued` is lifted before `baseline` so a concurrent
    /// [`NonceManager::next_nonce`] never observes a baseline ahead of
    /// `last_issued`, which would make it hand out nonces the venue has
    /// already consumed.
    ///
    /// # Errors
    ///
    /// Returns [`NonceError::NotInitialized`] if [`NonceManager::refresh`]
    /// has not run for this key.
    pub fn sync_from_venue(
        &self,
        account_index: i64,
        api_key_index: u8,
        venue_next_nonce: i64,
    ) -> Result<(), NonceError> {
        let state = self.state_for(account_index, api_key_index)?;
        let applied = venue_next_nonce - 1;
        state.last_issued.fetch_max(applied, Ordering::AcqRel);
        state.baseline.fetch_max(applied, Ordering::AcqRel);
        Ok(())
    }

    /// Allocate the next nonce for `(account_index, api_key_index)`.
    ///
    /// Returns the issued integer on success. Errors with
    /// [`NonceError::NotInitialized`] if [`NonceManager::refresh`] has not run
    /// for this key, and with [`NonceError::SkipWindowExhausted`] if the
    /// number of nonces already issued past the last baseline exceeds
    /// [`NonceManager::skip_window`]. The CAS loop guarantees monotonic,
    /// gap-free issuance under contention; concurrent callers serialize
    /// through the atomic compare-exchange.
    pub fn next_nonce(&self, account_index: i64, api_key_index: u8) -> Result<i64, NonceError> {
        let state = self.state_for(account_index, api_key_index)?;

        loop {
            let last = state.last_issued.load(Ordering::Acquire);
            let baseline = state.baseline.load(Ordering::Acquire);
            let next = last.wrapping_add(1);
            let outstanding = next.saturating_sub(baseline);

            if outstanding > i64::from(self.skip_window) {
                return Err(NonceError::SkipWindowExhausted {
                    account_index,
                    api_key_index,
                    outstanding: u32::try_from(outstanding).unwrap_or(u32::MAX),
                    skip_window: self.skip_window,
                });
            }

            if state
                .last_issued
                .compare_exchange_weak(last, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(next);
            }
        }
    }

    /// Record a venue success ack for `nonce`, monotonically advancing the
    /// baseline to `max(baseline, nonce)`.
    ///
    /// The venue has applied the acked transaction, so every nonce up to and
    /// including `nonce` no longer counts against the skip window. The
    /// advance is a monotonic max: a misattributed ack (one that pops the
    /// wrong pending entry) can only open the window early, never shrink it
    /// or cause a nonce to be reissued, because acked nonces are always ones
    /// this manager issued.
    ///
    /// # Errors
    ///
    /// Returns [`NonceError::NotInitialized`] if [`NonceManager::refresh`]
    /// has not run for this key.
    pub fn ack_success(
        &self,
        account_index: i64,
        api_key_index: u8,
        nonce: i64,
    ) -> Result<(), NonceError> {
        let state = self.state_for(account_index, api_key_index)?;
        state.baseline.fetch_max(nonce, Ordering::AcqRel);
        Ok(())
    }

    /// Roll back the most recently issued nonce for the given key.
    ///
    /// Mirrors `acknowledge_failure` in the Python reference: when the venue
    /// rejects a tx outside the "stale nonce" path, the manager decrements
    /// `last_issued` so the next [`NonceManager::next_nonce`] reuses the freed
    /// integer. Errors with [`NonceError::NothingToRollBack`] when called
    /// while `last_issued == baseline`.
    ///
    /// Caller contract: only the most recent issuance may be rolled back, and
    /// only before any newer nonce reaches the wire. Callers that cannot
    /// guarantee this (any path with multiple in-flight txs) must use
    /// [`NonceManager::ack_failure_if_latest`] instead.
    pub fn ack_failure(&self, account_index: i64, api_key_index: u8) -> Result<i64, NonceError> {
        let state = self.state_for(account_index, api_key_index)?;

        loop {
            let last = state.last_issued.load(Ordering::Acquire);
            let baseline = state.baseline.load(Ordering::Acquire);

            if last == baseline {
                return Err(NonceError::NothingToRollBack {
                    account_index,
                    api_key_index,
                });
            }

            let prev = last - 1;

            if state
                .last_issued
                .compare_exchange_weak(last, prev, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(last);
            }
        }
    }

    /// Roll back `nonce` only when it is still the most recent issuance.
    ///
    /// Returns `Ok(true)` when `last_issued` was decremented from `nonce` to
    /// `nonce - 1`, and `Ok(false)` when the rollback was skipped: either a
    /// newer nonce has been issued (so decrementing would free an integer
    /// already signed into an in-flight tx, and the next allocation would
    /// duplicate it on the wire), or the baseline has already advanced to
    /// `nonce` (the venue applied it, so the failure signal is stale). A
    /// skipped rollback leaves a gap that heals through
    /// [`NonceManager::ack_success`] or [`NonceManager::sync_from_venue`].
    ///
    /// # Errors
    ///
    /// Returns [`NonceError::NotInitialized`] if [`NonceManager::refresh`]
    /// has not run for this key.
    pub fn ack_failure_if_latest(
        &self,
        account_index: i64,
        api_key_index: u8,
        nonce: i64,
    ) -> Result<bool, NonceError> {
        let state = self.state_for(account_index, api_key_index)?;

        loop {
            let last = state.last_issued.load(Ordering::Acquire);
            let baseline = state.baseline.load(Ordering::Acquire);

            if last != nonce || last <= baseline {
                return Ok(false);
            }

            if state
                .last_issued
                .compare_exchange_weak(last, last - 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(true);
            }
        }
    }

    /// Snapshot the last issued nonce for diagnostic and test purposes.
    #[must_use]
    pub fn last_issued(&self, account_index: i64, api_key_index: u8) -> Option<i64> {
        self.states
            .get(&(account_index, api_key_index))
            .map(|s| s.last_issued.load(Ordering::Acquire))
    }

    /// Snapshot the configured baseline for diagnostic and test purposes.
    #[must_use]
    pub fn baseline(&self, account_index: i64, api_key_index: u8) -> Option<i64> {
        self.states
            .get(&(account_index, api_key_index))
            .map(|s| s.baseline.load(Ordering::Acquire))
    }

    /// Look up the per-key state, dropping the [`DashMap`] guard before
    /// returning so callers can spin in CAS loops without holding a shard
    /// lock.
    fn state_for(
        &self,
        account_index: i64,
        api_key_index: u8,
    ) -> Result<Arc<AccountNonce>, NonceError> {
        let entry =
            self.states
                .get(&(account_index, api_key_index))
                .ok_or(NonceError::NotInitialized {
                    account_index,
                    api_key_index,
                })?;
        let state = entry.value().clone();
        drop(entry);
        Ok(state)
    }
}

impl Default for NonceManager {
    fn default() -> Self {
        Self::new(DEFAULT_SKIP_WINDOW)
    }
}

#[derive(Debug)]
struct AccountNonce {
    last_issued: AtomicI64,
    baseline: AtomicI64,
}

impl AccountNonce {
    fn new(initial: i64) -> Self {
        Self {
            last_issued: AtomicI64::new(initial),
            baseline: AtomicI64::new(initial),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc as StdArc, thread};

    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    const ACCOUNT: i64 = 12345;
    const API_KEY: u8 = 5;

    #[rstest]
    fn next_nonce_uninitialized_errors() {
        let mgr = NonceManager::new(8);
        let err = mgr.next_nonce(ACCOUNT, API_KEY).expect_err("must error");
        assert_eq!(
            err,
            NonceError::NotInitialized {
                account_index: ACCOUNT,
                api_key_index: API_KEY
            },
        );
    }

    #[rstest]
    fn ack_failure_uninitialized_errors() {
        let mgr = NonceManager::new(8);
        let err = mgr.ack_failure(ACCOUNT, API_KEY).expect_err("must error");
        assert_eq!(
            err,
            NonceError::NotInitialized {
                account_index: ACCOUNT,
                api_key_index: API_KEY
            },
        );
    }

    #[rstest]
    fn default_uses_default_skip_window() {
        let mgr = NonceManager::default();
        assert_eq!(
            mgr.skip_window(),
            DEFAULT_SKIP_WINDOW,
            "Default impl must use DEFAULT_SKIP_WINDOW, was {}",
            mgr.skip_window(),
        );
    }

    #[rstest]
    fn last_issued_and_baseline_return_none_for_absent_key() {
        let mgr = NonceManager::new(8);
        assert_eq!(
            mgr.last_issued(ACCOUNT, API_KEY),
            None,
            "absent key must report no last_issued",
        );
        assert_eq!(
            mgr.baseline(ACCOUNT, API_KEY),
            None,
            "absent key must report no baseline",
        );
    }

    #[rstest]
    fn baseline_pins_to_refresh_value_through_allocations() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 42);
        assert_eq!(
            mgr.baseline(ACCOUNT, API_KEY),
            Some(41),
            "baseline must equal venue_next_nonce - 1 after refresh",
        );

        for _ in 0..3 {
            mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        }
        assert_eq!(
            mgr.baseline(ACCOUNT, API_KEY),
            Some(41),
            "baseline must not move when next_nonce advances last_issued",
        );

        mgr.refresh(ACCOUNT, API_KEY, 100);
        assert_eq!(
            mgr.baseline(ACCOUNT, API_KEY),
            Some(99),
            "subsequent refresh must reset baseline to new venue value - 1",
        );
    }

    #[rstest]
    fn refresh_then_next_nonce_starts_at_venue_value() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 42);
        let n = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(n, 42, "first nonce must equal venue baseline, was {n}");
    }

    #[rstest]
    fn next_nonce_is_monotonic_and_gap_free() {
        let mgr = NonceManager::new(64);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        let issued: Vec<i64> = (0..32)
            .map(|_| mgr.next_nonce(ACCOUNT, API_KEY).unwrap())
            .collect();
        let expected: Vec<i64> = (0..32).collect();
        assert_eq!(
            issued, expected,
            "nonces must be monotonic and gap-free, was {issued:?}",
        );
    }

    #[rstest]
    fn skip_window_caps_outstanding_allocations() {
        let mgr = NonceManager::new(4);
        mgr.refresh(ACCOUNT, API_KEY, 100);
        for _ in 0..4 {
            mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        }
        let err = mgr.next_nonce(ACCOUNT, API_KEY).expect_err("must error");
        match err {
            NonceError::SkipWindowExhausted {
                outstanding,
                skip_window,
                ..
            } => {
                assert_eq!(skip_window, 4, "skip_window mismatch, was {skip_window}");
                assert_eq!(outstanding, 5, "outstanding mismatch, was {outstanding}");
            }
            other => panic!("expected SkipWindowExhausted, was {other:?}"),
        }
    }

    #[rstest]
    fn ack_failure_rolls_back_most_recent_issuance() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        let issued = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        let rolled = mgr.ack_failure(ACCOUNT, API_KEY).unwrap();
        assert_eq!(
            rolled, issued,
            "ack_failure must report rolled-back nonce, was {rolled}",
        );
        let reused = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(
            reused, issued,
            "rolled-back nonce must be reissued, was {reused}"
        );
    }

    #[rstest]
    fn ack_failure_at_baseline_errors() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 7);
        let err = mgr.ack_failure(ACCOUNT, API_KEY).expect_err("must error");
        assert_eq!(
            err,
            NonceError::NothingToRollBack {
                account_index: ACCOUNT,
                api_key_index: API_KEY
            },
        );
    }

    #[rstest]
    fn ack_success_uninitialized_errors() {
        let mgr = NonceManager::new(8);
        let err = mgr
            .ack_success(ACCOUNT, API_KEY, 5)
            .expect_err("must error");
        assert_eq!(
            err,
            NonceError::NotInitialized {
                account_index: ACCOUNT,
                api_key_index: API_KEY
            },
        );
    }

    #[rstest]
    fn ack_success_advances_baseline_monotonically() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        for _ in 0..5 {
            mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        }

        mgr.ack_success(ACCOUNT, API_KEY, 2).unwrap();
        assert_eq!(
            mgr.baseline(ACCOUNT, API_KEY),
            Some(2),
            "ack must advance baseline to the acked nonce",
        );

        mgr.ack_success(ACCOUNT, API_KEY, 0).unwrap();
        assert_eq!(
            mgr.baseline(ACCOUNT, API_KEY),
            Some(2),
            "lower ack must not retreat the baseline",
        );

        mgr.ack_success(ACCOUNT, API_KEY, 4).unwrap();
        assert_eq!(mgr.baseline(ACCOUNT, API_KEY), Some(4));
        assert_eq!(
            mgr.last_issued(ACCOUNT, API_KEY),
            Some(4),
            "ack must not touch last_issued",
        );
    }

    #[rstest]
    fn ack_success_recovers_window_across_more_than_window_txs() {
        let window = 16_u32;
        let total = 40_i64;
        let mgr = NonceManager::new(window);
        mgr.refresh(ACCOUNT, API_KEY, 0);

        let mut issued = Vec::with_capacity(total as usize);
        for i in 0..total {
            if i >= i64::from(window) {
                // Ack the oldest outstanding tx; pre-fix the 17th allocation failed
                mgr.ack_success(ACCOUNT, API_KEY, i - i64::from(window))
                    .unwrap();
            }
            issued.push(mgr.next_nonce(ACCOUNT, API_KEY).unwrap());
        }

        let expected: Vec<i64> = (0..total).collect();
        assert_eq!(
            issued, expected,
            "interleaved acks must keep issuance contiguous past the window",
        );
    }

    #[rstest]
    fn sync_from_venue_uninitialized_errors() {
        let mgr = NonceManager::new(8);
        let err = mgr
            .sync_from_venue(ACCOUNT, API_KEY, 5)
            .expect_err("must error");
        assert_eq!(
            err,
            NonceError::NotInitialized {
                account_index: ACCOUNT,
                api_key_index: API_KEY
            },
        );
    }

    #[rstest]
    fn sync_from_venue_lifts_baseline_and_last_issued() {
        let mgr = NonceManager::new(2);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        for _ in 0..2 {
            mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        }
        assert!(
            mgr.next_nonce(ACCOUNT, API_KEY).is_err(),
            "window must trip"
        );

        // Venue applied both txs (next expected nonce is 2)
        mgr.sync_from_venue(ACCOUNT, API_KEY, 2).unwrap();
        assert_eq!(mgr.baseline(ACCOUNT, API_KEY), Some(1));
        assert_eq!(
            mgr.last_issued(ACCOUNT, API_KEY),
            Some(1),
            "venue sync must not retreat last_issued below issued nonces",
        );
        let n = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(n, 2, "venue sync must re-arm allocation, was {n}");

        // Venue jumped ahead; both values lift so allocation resumes there
        mgr.sync_from_venue(ACCOUNT, API_KEY, 10).unwrap();
        assert_eq!(mgr.baseline(ACCOUNT, API_KEY), Some(9));
        assert_eq!(mgr.last_issued(ACCOUNT, API_KEY), Some(9));
        let n = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(n, 10, "allocation must resume at venue nonce, was {n}");
    }

    #[rstest]
    fn sync_from_venue_never_moves_backwards() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 100);
        for _ in 0..2 {
            mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        }

        // A stale venue read must not free nonces signed into in-flight txs
        mgr.sync_from_venue(ACCOUNT, API_KEY, 50).unwrap();
        assert_eq!(mgr.baseline(ACCOUNT, API_KEY), Some(99));
        assert_eq!(mgr.last_issued(ACCOUNT, API_KEY), Some(101));
        let n = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(n, 102, "stale venue read must not cause reissue, was {n}");
    }

    #[rstest]
    fn ack_failure_if_latest_uninitialized_errors() {
        let mgr = NonceManager::new(8);
        let err = mgr
            .ack_failure_if_latest(ACCOUNT, API_KEY, 5)
            .expect_err("must error");
        assert_eq!(
            err,
            NonceError::NotInitialized {
                account_index: ACCOUNT,
                api_key_index: API_KEY
            },
        );
    }

    #[rstest]
    fn ack_failure_if_latest_rolls_back_latest_issuance() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        let latest = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();

        let rolled = mgr.ack_failure_if_latest(ACCOUNT, API_KEY, latest).unwrap();
        assert!(rolled, "latest issuance must roll back");
        let reused = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(
            reused, latest,
            "rolled-back nonce must be reissued, was {reused}",
        );
    }

    #[rstest]
    fn ack_failure_if_latest_skips_with_newer_issuance() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        let older = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        let newer = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();

        let rolled = mgr.ack_failure_if_latest(ACCOUNT, API_KEY, older).unwrap();
        assert!(!rolled, "non-latest nonce must not roll back");
        assert_eq!(
            mgr.last_issued(ACCOUNT, API_KEY),
            Some(newer),
            "skipped rollback must leave last_issued alone",
        );
        let next = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(
            next,
            newer + 1,
            "no nonce signed into an in-flight tx may be reissued, was {next}",
        );
    }

    #[rstest]
    fn ack_failure_if_latest_skips_when_baseline_caught_up() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        let nonce = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        mgr.ack_success(ACCOUNT, API_KEY, nonce).unwrap();

        let rolled = mgr.ack_failure_if_latest(ACCOUNT, API_KEY, nonce).unwrap();
        assert!(
            !rolled,
            "a nonce the venue already applied must not roll back",
        );
        assert_eq!(mgr.last_issued(ACCOUNT, API_KEY), Some(nonce));
    }

    #[rstest]
    fn refresh_resets_after_skip_window_exhausted() {
        let mgr = NonceManager::new(2);
        mgr.refresh(ACCOUNT, API_KEY, 0);
        for _ in 0..2 {
            mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        }
        assert!(
            mgr.next_nonce(ACCOUNT, API_KEY).is_err(),
            "window must trip"
        );
        // Venue confirms our view caught up; refresh re-anchors the baseline.
        mgr.refresh(ACCOUNT, API_KEY, 5);
        let n = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
        assert_eq!(n, 5, "refresh must re-arm allocation, was {n}");
    }

    #[rstest]
    fn distinct_keys_track_independent_state() {
        let mgr = NonceManager::new(8);
        mgr.refresh(ACCOUNT, 0, 0);
        mgr.refresh(ACCOUNT, 1, 100);
        let a = mgr.next_nonce(ACCOUNT, 0).unwrap();
        let b = mgr.next_nonce(ACCOUNT, 1).unwrap();
        assert_eq!(a, 0, "key 0 must start at 0, was {a}");
        assert_eq!(b, 100, "key 1 must start at 100, was {b}");
    }

    #[rstest]
    fn concurrent_callers_see_no_duplicate_or_gap() {
        let mgr = StdArc::new(NonceManager::new(10_000));
        mgr.refresh(ACCOUNT, API_KEY, 0);
        let threads = 8;
        let per_thread = 250;
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let mgr = StdArc::clone(&mgr);

                thread::spawn(move || -> Vec<i64> {
                    (0..per_thread)
                        .map(|_| mgr.next_nonce(ACCOUNT, API_KEY).unwrap())
                        .collect()
                })
            })
            .collect();
        let mut all = Vec::with_capacity(threads * per_thread);
        for h in handles {
            all.extend(h.join().unwrap());
        }
        all.sort_unstable();
        let expected: Vec<i64> = (0..(threads as i64) * (per_thread as i64)).collect();
        assert_eq!(
            all, expected,
            "concurrent issuance must cover [0, N) without gaps or duplicates",
        );
    }

    #[rstest]
    fn concurrent_allocation_with_interleaved_acks_is_gap_free() {
        let threads = 4;
        let per_thread = 200;
        // Window must absorb at most `threads` unacked allocations at a time
        let mgr = StdArc::new(NonceManager::new(64));
        mgr.refresh(ACCOUNT, API_KEY, 0);
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let mgr = StdArc::clone(&mgr);

                thread::spawn(move || -> Vec<i64> {
                    (0..per_thread)
                        .map(|_| {
                            let nonce = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
                            mgr.ack_success(ACCOUNT, API_KEY, nonce).unwrap();
                            nonce
                        })
                        .collect()
                })
            })
            .collect();
        let mut all = Vec::with_capacity(threads * per_thread);
        for h in handles {
            all.extend(h.join().unwrap());
        }
        all.sort_unstable();
        let expected: Vec<i64> = (0..(threads as i64) * (per_thread as i64)).collect();
        assert_eq!(
            all, expected,
            "concurrent issuance with acks must cover [0, N) without gaps or duplicates",
        );
    }

    proptest! {
        /// Sequential `next_nonce` calls produce a strictly monotonic, contiguous
        /// run starting at the refreshed baseline.
        #[rstest]
        fn prop_sequential_issuance_is_contiguous(
            baseline in 0i64..1_000_000,
            count in 1usize..256,
        ) {
            let mgr = NonceManager::new(u32::MAX);
            mgr.refresh(ACCOUNT, API_KEY, baseline);
            let issued: Vec<i64> = (0..count)
                .map(|_| mgr.next_nonce(ACCOUNT, API_KEY).unwrap())
                .collect();

            for (i, &n) in issued.iter().enumerate() {
                prop_assert_eq!(n, baseline + i as i64);
            }
            prop_assert_eq!(
                mgr.last_issued(ACCOUNT, API_KEY),
                Some(baseline + count as i64 - 1),
            );
        }

        /// Issue then roll back, and the next allocation reuses the rolled-back
        /// nonce: the round-trip is identity-preserving on `last_issued`.
        #[rstest]
        fn prop_ack_failure_is_idempotent_round_trip(
            baseline in 0i64..1_000_000,
            advance in 1usize..32,
        ) {
            let mgr = NonceManager::new(u32::MAX);
            mgr.refresh(ACCOUNT, API_KEY, baseline);
            for _ in 0..advance - 1 {
                mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
            }
            let issued = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
            let rolled = mgr.ack_failure(ACCOUNT, API_KEY).unwrap();
            prop_assert_eq!(rolled, issued);
            let reused = mgr.next_nonce(ACCOUNT, API_KEY).unwrap();
            prop_assert_eq!(reused, issued);
        }
    }
}
