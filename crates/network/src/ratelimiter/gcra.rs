// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

/// Information about the rate-limiting state used to reach a decision.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StateSnapshot {
    /// The "weight" of a single packet in units of time.
    t: Nanos,
    /// The "burst capacity" of the bucket.
    tau: Nanos,
    /// The time at which the measurement was taken.
    pub(crate) time_of_measurement: Nanos,
    /// The next time a cell is expected to arrive
    pub(crate) tat: Nanos,
}

impl StateSnapshot {
    /// Creates a new [`StateSnapshot`] instance.
    #[inline]
    pub(crate) const fn new(t: Nanos, tau: Nanos, time_of_measurement: Nanos, tat: Nanos) -> Self {
        Self {
            t,
            tau,
            time_of_measurement,
            tat,
        }
    }

    /// Returns the quota used to make the rate limiting decision.
    pub fn quota(&self) -> Quota {
        Quota::from_gcra_parameters(self.t, self.tau)
    }

    /// Returns the number of cells that can be let through in
    /// addition to a (possible) positive outcome.
    ///
    /// If this state snapshot is based on a negative rate limiting
    /// outcome, this method returns 0.
    #[allow(dead_code, reason = "Under development")]
    pub fn remaining_burst_capacity(&self) -> u32 {
        let t0 = self.time_of_measurement + self.t;
        (cmp::min(
            (t0 + self.tau).saturating_sub(self.tat).as_u64(),
            self.tau.as_u64(),
        ) / self.t.as_u64()) as u32
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
    /// Create a `NotUntil` as a negative rate-limiting result.
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

#[derive(Debug, PartialEq, Eq)]
pub struct Gcra {
    /// The "weight" of a single packet in units of time.
    t: Nanos,

    /// The "burst capacity" of the bucket.
    tau: Nanos,
}

impl Gcra {
    pub(crate) fn new(quota: Quota) -> Self {
        let tau: Nanos = (quota.replenish_1_per * quota.max_burst.get()).into();
        let t: Nanos = quota.replenish_1_per.into();
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
                    StateSnapshot::new(self.t, self.tau, earliest_time, earliest_time),
                    start,
                ))
            } else {
                let next = cmp::max(tat, t0) + t;
                Ok(((), next))
            }
        })
    }
}
