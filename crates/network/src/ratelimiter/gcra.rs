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

use std::{cmp, fmt::Display, time::Duration};

use super::{StateStore, clock, nanos::Nanos, quota::Quota};

/// Rate‑limiting parameters captured for a rejected decision.
///
/// `t` is one cell's weight in time, `tau` is the burst capacity in time, and `tat` is the
/// theoretical arrival time used to calculate the next admissible request.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct StateSnapshot {
    t: Nanos,
    tau: Nanos,
    pub(crate) tat: Nanos,
}

impl StateSnapshot {
    /// Creates a new [`StateSnapshot`] instance.
    #[inline]
    pub(crate) const fn new(t: Nanos, tau: Nanos, tat: Nanos) -> Self {
        Self { t, tau, tat }
    }

    /// Returns the quota used to make the rate limiting decision.
    pub(crate) fn quota(&self) -> Quota {
        Quota::from_gcra_parameters(self.t, self.tau)
    }
}

/// A negative rate-limiting outcome.
///
/// `NotUntil`'s methods indicate when a caller can expect the next positive
/// rate-limiting result.
#[derive(Debug, PartialEq, Eq)]
pub struct NotUntil<P: clock::Reference> {
    state: StateSnapshot,
    start: P,
}

impl<P: clock::Reference> NotUntil<P> {
    /// Creates a `NotUntil` as a negative rate‑limiting result.
    #[inline]
    pub(crate) const fn new(state: StateSnapshot, start: P) -> Self {
        Self { state, start }
    }

    /// Returns the earliest time at which a decision could be
    /// conforming (excluding conforming decisions made by the Decider
    /// that are made in the meantime).
    #[inline]
    pub fn earliest_possible(&self) -> P {
        let tat: Nanos = self.state.tat;
        self.start + tat
    }

    /// Returns the minimum amount of time from the time that the
    /// decision was made that must pass before a
    /// decision can be conforming.
    ///
    /// If the time of the next expected positive result is in the past,
    /// `wait_time_from` returns a zero `Duration`.
    #[inline]
    pub fn wait_time_from(&self, from: P) -> Duration {
        let earliest = self.earliest_possible();
        earliest.duration_since(earliest.min(from)).into()
    }

    /// Returns the rate limiting [`Quota`] used to reach the decision.
    #[inline]
    pub fn quota(&self) -> Quota {
        self.state.quota()
    }
}

impl<P: clock::Reference> Display for NotUntil<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "rate-limited until {:?}", self.start + self.state.tat)
    }
}

// GCRA parameters: `t` is one cell's weight in time, and `tau` is burst capacity in time
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Gcra {
    t: Nanos,
    tau: Nanos,
}

impl Gcra {
    /// Creates a GCRA for `quota`.
    ///
    /// `t` and `tau` are clamped far below `u64::MAX` so TAT arithmetic
    /// cannot saturate into the always-admit regime (`tat - tau` collapsing
    /// to zero): a degenerate quota (period beyond ~146 years) admits its
    /// burst and then denies. Unclamped, such quotas panicked in the
    /// Duration multiplication.
    pub(crate) fn new(quota: Quota) -> Self {
        const MAX_QUOTA_NANOS: Nanos = Nanos::new(u64::MAX / 4);

        let t = Nanos::from_duration_saturating(quota.replenish_1_per).min(MAX_QUOTA_NANOS);
        let tau = t
            .saturating_mul(u64::from(quota.max_burst.get()))
            .min(MAX_QUOTA_NANOS);
        Self { t, tau }
    }

    /// Computes and returns a new ratelimiter state if none exists yet.
    fn starting_state(&self, t0: Nanos) -> Nanos {
        t0 + self.t
    }

    /// Tests a single cell against the rate limiter state and updates it at the given key.
    pub(crate) fn test_and_update<K, S: StateStore<Key = K>, P: clock::Reference>(
        &self,
        start: P,
        key: &K,
        state: &S,
        t0: P,
    ) -> Result<(), NotUntil<P>> {
        let t0 = t0.duration_since(start);
        let tau = self.tau;
        let t = self.t;
        state.measure_and_replace(key, |tat| {
            let tat = tat.unwrap_or_else(|| self.starting_state(t0));
            let earliest_time = tat.saturating_sub(tau);
            if t0 < earliest_time {
                Err(NotUntil::new(
                    StateSnapshot::new(self.t, self.tau, earliest_time),
                    start,
                ))
            } else {
                let next = cmp::max(tat, t0) + t;
                Ok(((), next))
            }
        })
    }
}
