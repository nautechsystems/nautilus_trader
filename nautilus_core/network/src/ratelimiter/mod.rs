//! A rate limiter implementation heavily inspired by [governor](https://github.com/antifuchs/governor)
//!
//! The governor does not support different quota for different key. It is an open [issue](https://github.com/antifuchs/governor/issues/193)
pub mod clock;
mod gcra;
mod nanos;
pub mod quota;

use std::{
    hash::Hash,
    num::NonZeroU64,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use dashmap::DashMap;
use tokio::time::sleep;

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
#[derive(Default)]
pub struct InMemoryState(AtomicU64);

impl InMemoryState {
    pub(crate) fn measure_and_replace_one<T, F, E>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut(Option<Nanos>) -> Result<(T, Nanos), E>,
    {
        let mut prev = self.0.load(Ordering::Acquire);
        let mut decision = f(NonZeroU64::new(prev).map(|n| n.get().into()));
        while let Ok((result, new_data)) = decision {
            match self.0.compare_exchange_weak(
                prev,
                new_data.into(),
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(result),
                Err(next_prev) => prev = next_prev,
            }
            decision = f(NonZeroU64::new(prev).map(|n| n.get().into()));
        }
        // This map shouldn't be needed, as we only get here in the error case, but the compiler
        // can't see it.
        decision.map(|(result, _)| result)
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
/// A direct state store is expressed as [`StateStore::Key`] = [`NotKeyed`][direct::NotKeyed].
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
    /// * If the request is rate-limited, returns `Err(E)`.
    /// * If the request can make it through, returns `Ok(T)` (an arbitrary positive return
    ///   value) and the updated state.
    ///
    /// It is `measure_and_replace`'s job then to safely replace the value at the key - it must
    /// only update the value if the value hasn't changed. The implementations in this
    /// crate use `AtomicU64` operations for this.
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

pub struct RateLimiter<K, C>
where
    C: Clock,
{
    default_gcra: Option<Gcra>,
    state: DashMapStateStore<K>,
    gcra: DashMap<K, Gcra>,
    clock: C,
    start: C::Instant,
}

impl<K> RateLimiter<K, MonotonicClock>
where
    K: Eq + Hash,
{
    pub fn new_with_quota(base_quota: Option<Quota>, keyed_quotas: Vec<(K, Quota)>) -> Self {
        let clock = MonotonicClock {};
        let start = MonotonicClock::now(&clock);
        let gcra = DashMap::from_iter(keyed_quotas.into_iter().map(|(k, q)| (k, Gcra::new(q))));
        Self {
            default_gcra: base_quota.map(Gcra::new),
            state: DashMapStateStore::new(),
            gcra,
            clock,
            start,
        }
    }
}

impl<K> RateLimiter<K, FakeRelativeClock>
where
    K: Hash + Eq + Clone,
{
    pub fn advance_clock(&self, by: Duration) {
        self.clock.advance(by);
    }
}

impl<K, C> RateLimiter<K, C>
where
    K: Hash + Eq + Clone,
    C: Clock,
{
    pub fn add_quota_for_key(&self, key: K, value: Quota) {
        self.gcra.insert(key, Gcra::new(value));
    }

    pub fn check_key(&self, key: &K) -> Result<(), NotUntil<C::Instant>> {
        match self.gcra.get(key) {
            Some(quota) => quota.test_and_update(self.start, key, &self.state, self.clock.now()),
            None => self.default_gcra.as_ref().map_or(Ok(()), |gcra| {
                gcra.test_and_update(self.start, key, &self.state, self.clock.now())
            }),
        }
    }

    pub async fn until_key_ready(&self, key: &K) {
        loop {
            match self.check_key(key) {
                Ok(x) => x,
                Err(neg) => {
                    sleep(neg.wait_time_from(self.clock.now())).await;
                }
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, time::Duration};

    use dashmap::DashMap;

    use super::{
        clock::{Clock, FakeRelativeClock},
        gcra::Gcra,
        quota::Quota,
        DashMapStateStore, RateLimiter,
    };

    fn initialize_mock_rate_limiter() -> RateLimiter<String, FakeRelativeClock> {
        let clock = FakeRelativeClock::default();
        let start = clock.now();
        let gcra = DashMap::new();
        let base_quota = Quota::per_second(NonZeroU32::new(2).unwrap());
        RateLimiter {
            default_gcra: Some(Gcra::new(base_quota)),
            state: DashMapStateStore::new(),
            gcra,
            clock,
            start,
        }
    }

    #[test]
    fn test_default_quota() {
        let mock_limiter = initialize_mock_rate_limiter();
        // check base quota is exceeded
        assert!(mock_limiter.check_key(&"lmao".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"lmao".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"lmao".to_string()).is_err());

        // increment clock and check base quota is passed
        mock_limiter.advance_clock(Duration::from_secs(1));
        assert!(mock_limiter.check_key(&"lmao".to_string()).is_ok());

        // add new key quota pair
        mock_limiter.add_quota_for_key(
            "yeet".to_string(),
            Quota::per_second(NonZeroU32::new(1).unwrap()),
        );

        // check base quota and key quota to exceed
        assert!(mock_limiter.check_key(&"lmao".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"lmao".to_string()).is_err());
        assert!(mock_limiter.check_key(&"yeet".to_string()).is_ok());
        assert!(mock_limiter.check_key(&"yeet".to_string()).is_err());
    }
}
