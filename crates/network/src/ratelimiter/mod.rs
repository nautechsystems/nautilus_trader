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

//! A rate limiter implementation heavily inspired by [governor](https://github.com/antifuchs/governor).
//!
//! The governor does not support different quota for different key. It is an open [issue](https://github.com/antifuchs/governor/issues/193).
pub mod clock;
pub mod quota;

mod gcra;
mod nanos;

use std::{
    collections::HashMap,
    fmt::Debug,
    hash::Hash,
    num::NonZeroU64,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use dashmap::DashMap;

use self::{
    clock::{Clock, FakeRelativeClock, MonotonicClock},
    gcra::{Gcra, NotUntil},
    nanos::Nanos,
    quota::Quota,
};

/// An in-memory representation of a GCRA's rate-limiting state.
///
/// Implemented using [`AtomicU64`] operations, this state representation can be used to
/// construct rate limiting states for other in-memory states: e.g., this crate uses
/// `InMemoryState` as the states it tracks in the keyed rate limiters it implements.
///
/// Internally, the number tracked here is the theoretical arrival time (a GCRA term) in number of
/// nanoseconds since the rate limiter was created.
#[derive(Debug, Default)]
pub struct InMemoryState(AtomicU64);

impl InMemoryState {
    fn load(&self) -> Option<Nanos> {
        NonZeroU64::new(self.0.load(Ordering::Acquire)).map(|n| n.get().into())
    }

    fn store(&self, value: Nanos) {
        self.0.store(value.into(), Ordering::Release);
    }

    /// Measures and updates the GCRA's state atomically, retrying on concurrent modifications.
    ///
    /// # Errors
    ///
    /// Returns an error if the provided closure returns an error.
    pub(crate) fn measure_and_replace_one<T, F, E>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut(Option<Nanos>) -> Result<(T, Nanos), E>,
    {
        let mut prev = self.0.load(Ordering::Acquire);
        loop {
            let (result, new_data) = f(NonZeroU64::new(prev).map(|n| n.get().into()))?;

            // Lock-free CAS loop: retry with current value if another thread modified it,
            // uses weak variant (faster) since spurious failures are fine in a retry loop.
            match self.0.compare_exchange_weak(
                prev,
                new_data.into(),
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(result),
                Err(e) => prev = e, // Retry with value written by another thread
            }
        }
    }
}

/// A concurrent, thread-safe and fairly performant hashmap based on [`DashMap`].
pub type DashMapStateStore<K> = DashMap<K, InMemoryState>;

/// A way for rate limiters to keep state.
///
/// There are two important kinds of state stores: Direct and keyed. The direct kind have only
/// one state, and are useful for "global" rate limit enforcement (e.g. a process should never
/// do more than N tasks a day). The keyed kind allows one rate limit per key (e.g. an API
/// call budget per client API key).
///
/// A direct state store is expressed as [`StateStore::Key`] = `NotKeyed`.
/// Keyed state stores have a
/// type parameter for the key and set their key to that.
pub trait StateStore {
    /// The type of key that the state store can represent.
    type Key;

    /// Updates a state store's rate limiting state for a given key, using the given closure.
    ///
    /// The closure parameter takes the old value (`None` if this is the first measurement) of the
    /// state store at the key's location, checks if the request an be accommodated and:
    ///
    /// - If the request is rate-limited, returns `Err(E)`.
    /// - If the request can make it through, returns `Ok(T)` (an arbitrary positive return
    ///   value) and the updated state.
    ///
    /// It is `measure_and_replace`'s job then to safely replace the value at the key - it must
    /// only update the value if the value hasn't changed. The implementations in this
    /// crate use `AtomicU64` operations for this.
    ///
    /// # Errors
    ///
    /// Returns `Err(E)` if the closure returns an error or the request is rate-limited.
    fn measure_and_replace<T, F, E>(&self, key: &Self::Key, f: F) -> Result<T, E>
    where
        F: Fn(Option<Nanos>) -> Result<(T, Nanos), E>;
}

impl<K: Hash + Eq + Clone> StateStore for DashMapStateStore<K> {
    type Key = K;

    fn measure_and_replace<T, F, E>(&self, key: &Self::Key, f: F) -> Result<T, E>
    where
        F: Fn(Option<Nanos>) -> Result<(T, Nanos), E>,
    {
        if let Some(v) = self.get(key) {
            // fast path: measure existing entry
            return v.measure_and_replace_one(f);
        }
        // make an entry and measure that:
        let entry = self.entry(key.clone()).or_default();
        (*entry).measure_and_replace_one(f)
    }
}

/// A rate limiter that enforces different quotas per key using the GCRA algorithm.
///
/// This implementation allows setting different rate limits for different keys,
/// with an optional default quota for keys that don't have specific quotas.
pub struct RateLimiter<K, C>
where
    C: Clock,
{
    default_gcra: Option<Gcra>,
    state: DashMapStateStore<K>,
    gcra: DashMap<K, Gcra>,
    clock: C,
    start: C::Instant,
    decision_lock: Mutex<()>,
}

impl<K, C> Debug for RateLimiter<K, C>
where
    K: Debug,
    C: Clock,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(RateLimiter)).finish()
    }
}

impl<K> RateLimiter<K, MonotonicClock>
where
    K: Eq + Hash,
{
    /// Creates a new rate limiter with a base quota and keyed quotas.
    ///
    /// The base quota applies to all keys that don't have specific quotas.
    /// Keyed quotas override the base quota for specific keys.
    #[must_use]
    pub fn new_with_quota(base_quota: Option<Quota>, keyed_quotas: Vec<(K, Quota)>) -> Self {
        let clock = MonotonicClock {};
        Self::new_with_clock(base_quota, keyed_quotas, clock)
    }
}

impl<K, C> RateLimiter<K, C>
where
    K: Eq + Hash,
    C: Clock,
{
    /// Creates a new rate limiter with an explicit clock.
    ///
    /// The base quota applies to all keys that do not have specific quotas.
    /// Keyed quotas override the base quota for specific keys.
    #[must_use]
    pub fn new_with_clock(
        base_quota: Option<Quota>,
        keyed_quotas: Vec<(K, Quota)>,
        clock: C,
    ) -> Self {
        let start = clock.now();
        let gcra: DashMap<_, _> = keyed_quotas
            .into_iter()
            .map(|(k, q)| (k, Gcra::new(q)))
            .collect();
        Self {
            default_gcra: base_quota.map(Gcra::new),
            state: DashMapStateStore::new(),
            gcra,
            clock,
            start,
            decision_lock: Mutex::new(()),
        }
    }
}

impl<K> RateLimiter<K, FakeRelativeClock>
where
    K: Hash + Eq + Clone,
{
    /// Advances the fake clock by the specified duration.
    ///
    /// This is only available for testing with `FakeRelativeClock`.
    pub fn advance_clock(&self, by: Duration) {
        self.clock.advance(by);
    }
}

impl<K, C> RateLimiter<K, C>
where
    K: Hash + Eq + Clone,
    C: Clock,
{
    /// Adds or updates a quota for a specific key.
    ///
    /// # Panics
    ///
    /// Panics if the rate limiter decision mutex is poisoned.
    pub fn add_quota_for_key(&self, key: K, value: Quota) {
        let _guard = self
            .decision_lock
            .lock()
            .expect("rate limiter decision lock poisoned");
        self.gcra.insert(key, Gcra::new(value));
    }

    /// Checks if the given key is allowed under the rate limit.
    ///
    /// # Errors
    ///
    /// Returns `Err(NotUntil)` if the key is rate-limited, indicating when it will be allowed.
    ///
    /// # Panics
    ///
    /// Panics if the rate limiter decision mutex is poisoned.
    pub fn check_key(&self, key: &K) -> Result<(), NotUntil<C::Instant>> {
        let _guard = self
            .decision_lock
            .lock()
            .expect("rate limiter decision lock poisoned");

        match self.gcra.get(key) {
            Some(quota) => quota.test_and_update(self.start, key, &self.state, self.clock.now()),
            None => self.default_gcra.as_ref().map_or(Ok(()), |gcra| {
                gcra.test_and_update(self.start, key, &self.state, self.clock.now())
            }),
        }
    }

    /// Waits until the specified key is ready (not rate-limited).
    ///
    /// # Panics
    ///
    /// Panics if the rate limiter decision mutex is poisoned.
    pub async fn until_key_ready(&self, key: &K) {
        loop {
            match self.check_key(key) {
                Ok(()) => {
                    break;
                }
                Err(e) => {
                    self.clock.sleep(e.wait_time_from(self.clock.now())).await;
                }
            }
        }
    }

    /// Waits until all specified keys are ready (not rate-limited).
    ///
    /// If no keys are provided, this function returns immediately.
    ///
    /// # Panics
    ///
    /// Panics if the rate limiter decision mutex is poisoned.
    pub async fn await_keys_ready(&self, keys: Option<&[K]>) {
        let Some(keys) = keys else {
            return;
        };

        loop {
            let wait = {
                let _guard = self
                    .decision_lock
                    .lock()
                    .expect("rate limiter decision lock poisoned");

                match self.plan_keys(keys, self.clock.now()) {
                    Ok(planned) => {
                        self.commit_keys(planned);
                        None
                    }
                    Err(wait) => Some(wait),
                }
            };

            match wait {
                Some(wait) => self.clock.sleep(wait).await,
                None => return,
            }
        }
    }

    fn plan_keys<'a>(
        &self,
        keys: &'a [K],
        now: C::Instant,
    ) -> Result<HashMap<&'a K, Nanos>, Duration> {
        let mut planned = HashMap::with_capacity(keys.len());
        let mut wait: Option<Duration> = None;

        for key in keys {
            let tat = planned
                .get(key)
                .copied()
                .or_else(|| self.state.get(key).and_then(|state| state.load()));
            let decision = match self.gcra.get(key) {
                Some(quota) => Some(quota.test(self.start, tat, now)),
                None => self
                    .default_gcra
                    .as_ref()
                    .map(|gcra| gcra.test(self.start, tat, now)),
            };

            match decision {
                Some(Ok(next)) => {
                    planned.insert(key, next);
                }
                Some(Err(denied)) => {
                    let duration = denied.wait_time_from(now);
                    wait = Some(wait.map_or(duration, |current| current.max(duration)));
                }
                None => {}
            }
        }

        match wait {
            Some(wait) => Err(wait),
            None => Ok(planned),
        }
    }

    fn commit_keys(&self, planned: HashMap<&K, Nanos>) {
        for (key, tat) in planned {
            self.state.entry(key.clone()).or_default().store(tat);
        }
    }
}

impl<K> RateLimiter<K, MonotonicClock>
where
    K: Hash + Eq + Clone,
{
    pub(crate) async fn await_limiters_ready(rate_limiters: &[Arc<Self>], keys: Option<&[K]>) {
        let Some(keys) = keys else {
            return;
        };

        if rate_limiters.is_empty() || keys.is_empty() {
            return;
        }

        let mut ordered = rate_limiters.iter().map(Arc::as_ref).collect::<Vec<_>>();
        ordered.sort_unstable_by_key(|limiter| std::ptr::from_ref(*limiter) as usize);
        ordered.dedup_by(|a, b| std::ptr::eq(*a, *b));

        loop {
            let wait = {
                let _guards = ordered
                    .iter()
                    .map(|limiter| {
                        limiter
                            .decision_lock
                            .lock()
                            .expect("rate limiter decision lock poisoned")
                    })
                    .collect::<Vec<_>>();
                let mut plans = Vec::with_capacity(ordered.len());
                let mut wait: Option<Duration> = None;

                for limiter in &ordered {
                    match limiter.plan_keys(keys, limiter.clock.now()) {
                        Ok(planned) => plans.push((*limiter, planned)),
                        Err(duration) => {
                            wait = Some(wait.map_or(duration, |current| current.max(duration)));
                        }
                    }
                }

                if wait.is_none() {
                    for (limiter, planned) in plans {
                        limiter.commit_keys(planned);
                    }
                }
                wait
            };

            match wait {
                Some(wait) => ordered[0].clock.sleep(wait).await,
                None => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        },
        time::Duration,
    };

    use dashmap::DashMap;
    #[cfg(all(feature = "simulation", madsim))]
    use madsim::task as test_task;
    use rstest::rstest;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use tokio::task as test_task;

    use super::{
        DashMapStateStore, RateLimiter,
        clock::{Clock, FakeRelativeClock},
        gcra::Gcra,
        nanos::Nanos,
        quota::Quota,
    };

    fn initialize_mock_rate_limiter() -> RateLimiter<String, FakeRelativeClock> {
        let clock = FakeRelativeClock::default();
        let start = clock.now();
        let gcra = DashMap::new();
        let base_quota = Quota::per_second(NonZeroU32::new(2).unwrap()).unwrap();
        RateLimiter {
            default_gcra: Some(Gcra::new(base_quota)),
            state: DashMapStateStore::new(),
            gcra,
            clock,
            start,
            decision_lock: std::sync::Mutex::new(()),
        }
    }

    #[rstest]
    fn test_enormous_quota_denies_after_burst() {
        // Regression: a period beyond ~584 years panicked in Gcra::new; with
        // clamping it must admit the burst and then deny, not admit everything
        let quota = Quota::with_period(Duration::MAX)
            .unwrap()
            .allow_burst(NonZeroU32::new(u32::MAX).unwrap());
        let clock = FakeRelativeClock::default();
        let limiter: RateLimiter<String, FakeRelativeClock> =
            RateLimiter::new_with_clock(Some(quota), vec![], clock);

        let key = "key".to_string();
        assert!(limiter.check_key(&key).is_ok());
        assert!(limiter.check_key(&key).is_err());
    }

    #[rstest]
    fn test_default_quota() {
        let mock_limiter = initialize_mock_rate_limiter();

        // Check base quota is not exceeded
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());

        // Check base quota is exceeded
        assert!(mock_limiter.check_key(&"default".to_string()).is_err());

        // Increment clock and check base quota is reset
        mock_limiter.advance_clock(Duration::from_secs(1));
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());
    }

    #[rstest]
    fn test_custom_key_quota() {
        let mock_limiter = initialize_mock_rate_limiter();

        // Add new key quota pair
        mock_limiter.add_quota_for_key(
            "custom".to_string(),
            Quota::per_second(NonZeroU32::new(1).unwrap()).unwrap(),
        );

        // Check custom quota
        assert!(mock_limiter.check_key(&"custom".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"custom".to_string()).is_err());

        // Check that default quota still applies to other keys
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"default".to_string()).is_err());
    }

    #[rstest]
    fn test_multiple_keys() {
        let mock_limiter = initialize_mock_rate_limiter();

        mock_limiter.add_quota_for_key(
            "key1".to_string(),
            Quota::per_second(NonZeroU32::new(1).unwrap()).unwrap(),
        );
        mock_limiter.add_quota_for_key(
            "key2".to_string(),
            Quota::per_second(NonZeroU32::new(3).unwrap()).unwrap(),
        );

        // Test key1
        assert!(mock_limiter.check_key(&"key1".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"key1".to_string()).is_err());

        // Test key2
        assert!(mock_limiter.check_key(&"key2".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"key2".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"key2".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"key2".to_string()).is_err());
    }

    #[rstest]
    fn test_quota_reset() {
        let mock_limiter = initialize_mock_rate_limiter();

        // Exhaust quota
        assert!(mock_limiter.check_key(&"reset".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"reset".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"reset".to_string()).is_err());

        // Advance clock by less than a second
        mock_limiter.advance_clock(Duration::from_millis(499));
        assert!(mock_limiter.check_key(&"reset".to_string()).is_err());

        // Advance clock to reset
        mock_limiter.advance_clock(Duration::from_millis(501));
        assert!(mock_limiter.check_key(&"reset".to_string()).is_ok());
    }

    #[rstest]
    fn test_different_quotas() {
        let mock_limiter = initialize_mock_rate_limiter();

        mock_limiter.add_quota_for_key(
            "per_second".to_string(),
            Quota::per_second(NonZeroU32::new(2).unwrap()).unwrap(),
        );
        mock_limiter.add_quota_for_key(
            "per_minute".to_string(),
            Quota::per_minute(NonZeroU32::new(3).unwrap()),
        );

        // Test per_second quota
        assert!(mock_limiter.check_key(&"per_second".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"per_second".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"per_second".to_string()).is_err());

        // Test per_minute quota
        assert!(mock_limiter.check_key(&"per_minute".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"per_minute".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"per_minute".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"per_minute".to_string()).is_err());

        // Advance clock and check reset
        mock_limiter.advance_clock(Duration::from_secs(1));
        assert!(mock_limiter.check_key(&"per_second".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"per_minute".to_string()).is_err());
    }

    #[tokio::test]
    async fn test_await_keys_ready() {
        let mock_limiter = initialize_mock_rate_limiter();

        // Check base quota is not exceeded
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());

        // Check base quota is exceeded
        assert!(mock_limiter.check_key(&"default".to_string()).is_err());

        // Wait keys to be ready and check base quota is reset
        mock_limiter.advance_clock(Duration::from_secs(1));
        let keys = ["default".to_string()];
        mock_limiter.await_keys_ready(Some(keys.as_slice())).await;
        assert!(mock_limiter.check_key(&"default".to_string()).is_ok());
    }

    #[cfg_attr(
        not(all(feature = "simulation", madsim)),
        tokio::test(start_paused = true)
    )]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn test_await_keys_ready_reserves_keys_together() {
        let fast = "fast".to_string();
        let slow = "slow".to_string();
        let limiter = Arc::new(RateLimiter::new_with_quota(
            None,
            vec![
                (
                    fast.clone(),
                    Quota::with_period(Duration::from_secs(1)).unwrap(),
                ),
                (
                    slow.clone(),
                    Quota::with_period(Duration::from_secs(10)).unwrap(),
                ),
            ],
        ));
        limiter.check_key(&slow).unwrap();

        let waiting_limiter = Arc::clone(&limiter);
        let waiting_fast = fast.clone();
        let waiting_slow = slow.clone();

        let request = test_task::spawn(async move {
            waiting_limiter
                .await_keys_ready(Some(&[waiting_fast, waiting_slow]))
                .await;
        });
        test_task::yield_now().await;

        limiter.check_key(&fast).unwrap();
        assert!(!request.is_finished());

        advance_test_clock(Duration::from_millis(9_999)).await;
        limiter.until_key_ready(&fast).await;
        limiter.until_key_ready(&fast).await;
        advance_test_clock(Duration::from_millis(1)).await;
        test_task::yield_now().await;
        assert!(!request.is_finished());

        advance_test_clock(Duration::from_millis(998)).await;
        test_task::yield_now().await;
        assert!(!request.is_finished());

        advance_test_clock(Duration::from_millis(1)).await;
        request.await.unwrap();

        assert!(limiter.check_key(&fast).is_err());
        assert!(limiter.check_key(&slow).is_err());
    }

    #[cfg(all(feature = "simulation", madsim))]
    async fn advance_test_clock(duration: Duration) {
        madsim::time::advance(duration);
        test_task::yield_now().await;
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    async fn advance_test_clock(duration: Duration) {
        tokio::time::advance(duration).await;
    }

    #[rstest]
    fn test_per_second_returns_none_on_zero_replenish_interval() {
        assert!(Quota::per_second(NonZeroU32::new(u32::MAX).unwrap()).is_none());
    }

    #[rstest]
    fn test_per_minute_accepts_max_burst() {
        let quota = Quota::per_minute(NonZeroU32::new(u32::MAX).unwrap());
        assert!(quota.replenish_interval().as_nanos() > 0);
    }

    #[rstest]
    fn test_per_hour_accepts_max_burst() {
        let quota = Quota::per_hour(NonZeroU32::new(u32::MAX).unwrap());
        assert!(quota.replenish_interval().as_nanos() > 0);
    }

    mod property_tests {
        use proptest::prelude::*;
        use rstest::rstest;

        use crate::ratelimiter::nanos::Nanos;

        proptest! {
            #![proptest_config(ProptestConfig {
                failure_persistence: Some(Box::new(
                    proptest::test_runner::FileFailurePersistence::WithSource("ratelimiter")
                )),
                ..ProptestConfig::default()
            })]

            // Operators must saturate across the full u64 domain (a wrapped TAT admits everything)
            #[rstest]
            fn nanos_operators_never_panic(a in proptest::num::u64::ANY, b in proptest::num::u64::ANY) {
                let na = Nanos::from(a);
                let nb = Nanos::from(b);

                prop_assert_eq!((na + nb).as_u64(), a.saturating_add(b));
                prop_assert_eq!((na * b).as_u64(), a.saturating_mul(b));
                prop_assert_eq!(na.saturating_sub(nb).as_u64(), a.saturating_sub(b));
            }
        }
    }

    #[rstest]
    fn test_gcra_boundary_exact_replenishment() {
        // Test GCRA boundary condition where t0 equals earliest_time exactly.
        // This exercises the saturating_sub edge case deterministically without sleeps.
        let mock_limiter = initialize_mock_rate_limiter();
        let key = "boundary_test".to_string();

        assert!(mock_limiter.check_key(&key).is_ok());
        assert!(mock_limiter.check_key(&key).is_ok());
        assert!(mock_limiter.check_key(&key).is_err());

        // Advance clock by exactly one replenish interval (500ms for 2 req/sec)
        let quota = Quota::per_second(NonZeroU32::new(2).unwrap()).unwrap();
        let replenish_interval = quota.replenish_interval();
        mock_limiter.advance_clock(replenish_interval);

        assert!(
            mock_limiter.check_key(&key).is_ok(),
            "Request at exact replenish boundary should be allowed"
        );
        assert!(
            mock_limiter.check_key(&key).is_err(),
            "Immediate follow-up should be rate-limited"
        );
    }

    #[rstest]
    fn test_per_second_boundary_exact_limit() {
        // 1_000_000_000ns / 1_000_000_000 = 1ns per replenish, the exact boundary
        let quota = Quota::per_second(NonZeroU32::new(1_000_000_000).unwrap()).unwrap();
        assert_eq!(quota.replenish_interval().as_nanos(), 1);
    }

    #[rstest]
    fn test_per_second_returns_none_above_one_billion() {
        // 1_000_000_000ns / 1_000_000_001 rounds to 0ns
        assert!(Quota::per_second(NonZeroU32::new(1_000_000_001).unwrap()).is_none());
    }

    #[rstest]
    #[case::large(Duration::from_secs(100), u32::MAX, Duration::from_mins(7_158_278_825))]
    #[case::saturated(Duration::MAX, 2, Duration::MAX)]
    fn test_burst_size_replenished_in(
        #[case] replenish_interval: Duration,
        #[case] burst_size: u32,
        #[case] expected: Duration,
    ) {
        let quota = Quota::with_period(replenish_interval)
            .unwrap()
            .allow_burst(NonZeroU32::new(burst_size).unwrap());

        assert_eq!(quota.burst_size_replenished_in(), expected);
    }

    #[rstest]
    #[should_panic(expected = "t cannot be zero")]
    fn test_from_gcra_parameters_panics_on_zero_t() {
        let _ = Quota::from_gcra_parameters(Nanos::from(0u64), Nanos::from(100u64));
    }

    #[rstest]
    #[should_panic(expected = "tau/t results in zero burst capacity")]
    fn test_from_gcra_parameters_panics_on_zero_division() {
        // tau=1, t=2 → integer division yields 0
        let _ = Quota::from_gcra_parameters(Nanos::from(2u64), Nanos::from(1u64));
    }

    #[rstest]
    #[should_panic(expected = "tau/t exceeds u32::MAX")]
    fn test_from_gcra_parameters_panics_on_overflow() {
        let _ = Quota::from_gcra_parameters(Nanos::from(1u64), Nanos::from(u64::MAX));
    }

    #[rstest]
    fn test_concurrent_check_key_respects_burst() {
        let rate = 10u32;
        let clock = FakeRelativeClock::default();
        let start = clock.now();
        let limiter = RateLimiter {
            default_gcra: Some(Gcra::new(
                Quota::per_second(NonZeroU32::new(rate).unwrap()).unwrap(),
            )),
            state: DashMapStateStore::new(),
            gcra: DashMap::new(),
            clock,
            start,
            decision_lock: std::sync::Mutex::new(()),
        };

        let accepted = AtomicU32::new(0);
        let num_threads = 50;

        // Clock is frozen: no replenishment occurs
        std::thread::scope(|s| {
            for _ in 0..num_threads {
                s.spawn(|| {
                    if limiter.check_key(&"hot_key".to_string()).is_ok() {
                        accepted.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });

        let total = accepted.load(Ordering::Relaxed);
        assert!(total >= 1, "At least one request should be accepted");
        assert!(
            total <= rate,
            "Accepted {total} but burst capacity is {rate}"
        );
    }
}
