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

//! Account-owned token admission using the shared monotonic clock boundary.
//!
//! Reservations cover every affected bucket atomically. Changing limits clamps retained balances;
//! discovery never creates a fresh burst. Costs and limits expire after 60 seconds or grant expiry.

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    sync::Mutex,
    time::Duration,
};

use nautilus_network::ratelimiter::clock::{Clock, MonotonicClock, Reference};

use super::{
    error::KalshiHttpError,
    models::{AccountLimits, BucketLimit, EndpointCosts, Field},
};
use crate::common::{
    consts::{BASIC_READ_TOKENS_PER_SEC, BASIC_WRITE_TOKENS_PER_SEC},
    enums::ApiUsageTier,
};

const TOKEN_SCALE: u128 = 1_000_000_000;
const DISCOVERY_TTL: Duration = Duration::from_secs(60);

/// Shared token owner for one Kalshi account and environment.
///
/// Pass the same owner to data and execution transports, including transports using different
/// API keys for that account. Public-only transports can share it with authenticated transports.
/// Writes remain disabled until both limit and cost discovery succeed.
#[derive(Debug)]
pub struct KalshiQuota {
    pub(super) refresh: tokio::sync::Mutex<()>,
    meter: Mutex<Meter<MonotonicClock>>,
}

impl Default for KalshiQuota {
    fn default() -> Self {
        Self {
            refresh: tokio::sync::Mutex::new(()),
            meter: Mutex::new(Meter::new(MonotonicClock)),
        }
    }
}

impl KalshiQuota {
    /// Creates an account owner with conservative bootstrap reads and disabled writes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn needs_refresh(&self) -> bool {
        let meter = self.meter.lock().expect("quota lock is not poisoned");
        meter.needs_refresh() || (meter.limits_loaded && meter.costs.is_none())
    }

    pub(super) fn set_limits(
        &self,
        limits: &AccountLimits,
        unix_ms: i64,
    ) -> Result<(), KalshiHttpError> {
        self.meter
            .lock()
            .expect("quota lock is not poisoned")
            .set_limits(limits, unix_ms)
    }

    pub(super) fn set_costs(&self, costs: EndpointCosts) -> Result<(), KalshiHttpError> {
        self.meter
            .lock()
            .expect("quota lock is not poisoned")
            .set_costs(costs)
    }

    pub(super) fn discovery_failed(&self) {
        self.meter
            .lock()
            .expect("quota lock is not poisoned")
            .discovery_failed();
    }

    pub(super) async fn acquire(&self, charge: &Charge) -> Result<(), KalshiHttpError> {
        loop {
            let wait = self
                .meter
                .lock()
                .expect("quota lock is not poisoned")
                .reserve(charge)?;

            match wait {
                None => return Ok(()),
                Some(wait) => nautilus_network::dst::time::sleep(wait).await,
            }
        }
    }

    pub(super) fn batch_size(&self, charge: &Charge) -> Result<usize, KalshiHttpError> {
        let meter = self.meter.lock().expect("quota lock is not poisoned");
        if !meter.limits_loaded
            || meter.needs_refresh()
            || meter.costs_expired()
            || meter.costs.is_none()
        {
            return Err(KalshiHttpError::Quota(
                "write quota discovery is unavailable or expired",
            ));
        }

        let size = meter.write.bucket_capacity as u128 / meter.cost(charge);
        usize::try_from(size)
            .ok()
            .filter(|size| *size > 0)
            .ok_or(KalshiHttpError::Quota(
                "one batch item exceeds write capacity",
            ))
    }
}

impl Display for KalshiQuota {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let meter = self.meter.lock().expect("quota lock is not poisoned");
        write!(
            f,
            "Read {}/s, capacity {}; write {}/s, capacity {}; limits loaded {}, costs loaded {}",
            meter.read.refill_rate,
            meter.read.bucket_capacity,
            meter.write.refill_rate,
            meter.write.bucket_capacity,
            meter.limits_loaded,
            meter.costs.is_some()
        )
    }
}

#[derive(Debug)]
pub(super) struct Charge {
    pub method: String,
    pub path: String,
    pub bucket: Bucket,
    pub items: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Bucket {
    Read,
    Balance,
    Write(u32),
    Auto,
}

#[derive(Debug)]
struct Meter<C: Clock> {
    clock: C,
    read: BucketLimit,
    write: BucketLimit,
    balance: BucketLimit,
    buckets: HashMap<Bucket, Balance<C::Instant>>,
    costs: Option<EndpointCosts>,
    costs_refreshed: Option<C::Instant>,
    limits_loaded: bool,
    refreshed: Option<C::Instant>,
    ttl: Duration,
}

impl<C: Clock> Meter<C> {
    fn new(clock: C) -> Self {
        Self {
            clock,
            read: BucketLimit {
                refill_rate: BASIC_READ_TOKENS_PER_SEC.into(),
                bucket_capacity: BASIC_READ_TOKENS_PER_SEC.into(),
            },
            write: BucketLimit {
                refill_rate: BASIC_WRITE_TOKENS_PER_SEC.into(),
                bucket_capacity: BASIC_WRITE_TOKENS_PER_SEC.into(),
            },
            balance: BucketLimit {
                refill_rate: 1,
                bucket_capacity: 1,
            },
            buckets: HashMap::new(),
            costs: None,
            costs_refreshed: None,
            limits_loaded: false,
            refreshed: None,
            ttl: DISCOVERY_TTL,
        }
    }

    fn needs_refresh(&self) -> bool {
        self.refreshed.is_none_or(|last| {
            let elapsed: u64 = self.clock.now().duration_since(last).into();
            Duration::from_nanos(elapsed) >= self.ttl
        }) || self.costs_expired()
    }

    fn set_limits(&mut self, limits: &AccountLimits, unix_ms: i64) -> Result<(), KalshiHttpError> {
        for limit in [&limits.read, &limits.write] {
            if limit.refill_rate <= 0 || limit.bucket_capacity <= 0 {
                return Err(KalshiHttpError::Quota("nonpositive venue token limit"));
            }
        }

        let ttl = limits
            .grants
            .iter()
            .filter_map(|grant| match grant.expires_ts {
                Field::Value(expiry) => Some(Duration::from_millis(
                    expiry.saturating_mul(1000).saturating_sub(unix_ms).max(0) as u64,
                )),
                _ => None,
            })
            .min()
            .unwrap_or(DISCOVERY_TTL)
            .min(DISCOVERY_TTL);

        if ttl.is_zero() {
            return Err(KalshiHttpError::Quota("expired venue token grant"));
        }

        self.update_balances();
        self.read = limits.read.clone();
        self.write = limits.write.clone();

        let rate = if matches!(
            limits.usage_tier,
            ApiUsageTier::Expert
                | ApiUsageTier::Premier
                | ApiUsageTier::Paragon
                | ApiUsageTier::Prime
                | ApiUsageTier::Prestige
        ) {
            5
        } else {
            1
        };

        self.balance = BucketLimit {
            refill_rate: rate,
            bucket_capacity: rate,
        };

        self.clamp_balances();
        self.limits_loaded = true;
        self.refreshed = Some(self.clock.now());
        self.ttl = ttl;

        Ok(())
    }

    fn set_costs(&mut self, costs: EndpointCosts) -> Result<(), KalshiHttpError> {
        if costs.default_cost <= 0
            || costs
                .endpoint_costs
                .iter()
                .any(|cost| cost.cost <= 0 || !cost.path.starts_with('/') || cost.method.is_empty())
        {
            return Err(KalshiHttpError::Quota("invalid venue endpoint costs"));
        }

        self.costs = Some(costs);
        self.costs_refreshed = Some(self.clock.now());
        Ok(())
    }

    fn discovery_failed(&mut self) {
        self.update_balances();

        self.read = BucketLimit {
            refill_rate: BASIC_READ_TOKENS_PER_SEC.into(),
            bucket_capacity: BASIC_READ_TOKENS_PER_SEC.into(),
        };

        self.write = BucketLimit {
            refill_rate: BASIC_WRITE_TOKENS_PER_SEC.into(),
            bucket_capacity: BASIC_WRITE_TOKENS_PER_SEC.into(),
        };

        self.balance = BucketLimit {
            refill_rate: 1,
            bucket_capacity: 1,
        };

        self.clamp_balances();
        self.limits_loaded = false;
        self.costs = None;
        self.costs_refreshed = None;
        self.refreshed = Some(self.clock.now());
        self.ttl = Duration::from_secs(5);
    }

    fn reserve(&mut self, charge: &Charge) -> Result<Option<Duration>, KalshiHttpError> {
        if self.costs_expired() || (self.limits_loaded && self.needs_refresh()) {
            self.discovery_failed();
            self.refreshed = None;
        }

        let write = matches!(charge.bucket, Bucket::Write(_) | Bucket::Auto);
        if write && (!self.limits_loaded || self.costs.is_none() || self.needs_refresh()) {
            return Err(KalshiHttpError::Quota(
                "write quota discovery is unavailable or expired",
            ));
        }

        let cost = if charge.bucket == Bucket::Balance {
            1
        } else {
            self.cost(charge)
        };

        let cost = cost
            .checked_mul(u128::from(charge.items))
            .ok_or(KalshiHttpError::Quota("batch token cost overflow"))?;

        let keys = match charge.bucket {
            // Current Predictions shards: https://docs.kalshi.com/getting_started/exchange_sharding
            Bucket::Auto => vec![
                Bucket::Write(0),
                Bucket::Write(1),
                Bucket::Write(2),
                Bucket::Write(3),
            ],
            key => vec![key],
        };

        let now = self.clock.now();
        let mut wait = Duration::ZERO;

        for key in &keys {
            let limit = self.limit(*key).clone();
            if cost > limit.bucket_capacity as u128 {
                return Err(KalshiHttpError::Quota(
                    "request token cost exceeds bucket capacity",
                ));
            }

            let balance = self
                .buckets
                .entry(*key)
                .or_insert_with(|| Balance::new(&limit, now));
            balance.refill(&limit, now);
            let missing = (cost * TOKEN_SCALE).saturating_sub(balance.tokens);
            let nanos = missing.div_ceil(limit.refill_rate as u128);
            let nanos = u64::try_from(nanos)
                .map_err(|_| KalshiHttpError::Quota("quota wait exceeds clock range"))?;
            wait = wait.max(Duration::from_nanos(nanos));
        }

        if !wait.is_zero() {
            return Ok(Some(wait));
        }

        for key in keys {
            self.buckets
                .get_mut(&key)
                .expect("admitted bucket exists")
                .tokens -= cost * TOKEN_SCALE;
        }

        Ok(None)
    }

    fn costs_expired(&self) -> bool {
        self.costs_refreshed.is_some_and(|last| {
            let elapsed: u64 = self.clock.now().duration_since(last).into();
            Duration::from_nanos(elapsed) >= DISCOVERY_TTL
        })
    }

    fn cost(&self, charge: &Charge) -> u128 {
        let Some(costs) = &self.costs else { return 50 };
        costs
            .endpoint_costs
            .iter()
            .filter(|cost| {
                cost.method.eq_ignore_ascii_case(&charge.method)
                    && path_matches(&cost.path, &charge.path)
            })
            .max_by_key(|cost| {
                cost.path
                    .split('/')
                    .filter(|part| !part.starts_with(':') && !part.starts_with('*'))
                    .count()
            })
            .map_or(costs.default_cost, |cost| cost.cost) as u128
    }

    fn limit(&self, key: Bucket) -> &BucketLimit {
        match key {
            Bucket::Read => &self.read,
            Bucket::Balance => &self.balance,
            _ => &self.write,
        }
    }

    fn update_balances(&mut self) {
        let now = self.clock.now();

        for (key, balance) in &mut self.buckets {
            let limit = match key {
                Bucket::Read => &self.read,
                Bucket::Balance => &self.balance,
                _ => &self.write,
            };

            balance.refill(limit, now);
        }
    }

    fn clamp_balances(&mut self) {
        for (key, balance) in &mut self.buckets {
            let limit = match key {
                Bucket::Read => &self.read,
                Bucket::Balance => &self.balance,
                _ => &self.write,
            };

            balance.tokens = balance
                .tokens
                .min(limit.bucket_capacity as u128 * TOKEN_SCALE);
        }
    }
}

#[derive(Debug)]
struct Balance<I> {
    tokens: u128,
    updated: I,
}

impl<I: Reference> Balance<I> {
    fn new(limit: &BucketLimit, now: I) -> Self {
        Self {
            tokens: limit.bucket_capacity as u128 * TOKEN_SCALE,
            updated: now,
        }
    }

    fn refill(&mut self, limit: &BucketLimit, now: I) {
        let elapsed: u64 = now.duration_since(self.updated).into();
        self.tokens = self
            .tokens
            .saturating_add(u128::from(elapsed) * limit.refill_rate as u128)
            .min(limit.bucket_capacity as u128 * TOKEN_SCALE);
        self.updated = now;
    }
}

fn path_matches(pattern: &str, path: &str) -> bool {
    let mut parts = path.trim_matches('/').split('/');

    for expected in pattern.trim_matches('/').split('/') {
        if expected.starts_with('*') {
            return parts.next().is_some();
        }

        let Some(actual) = parts.next() else {
            return false;
        };

        if !expected.starts_with(':') && expected != actual {
            return false;
        }
    }

    parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use nautilus_network::ratelimiter::clock::FakeRelativeClock;
    use rstest::rstest;

    use super::{super::models::EndpointCost, *};

    fn meter() -> Meter<FakeRelativeClock> {
        let mut meter = Meter::new(FakeRelativeClock::default());
        meter
            .set_limits(
                &AccountLimits {
                    usage_tier: "basic".into(),
                    read: BucketLimit {
                        refill_rate: 20,
                        bucket_capacity: 60,
                    },
                    write: BucketLimit {
                        refill_rate: 10,
                        bucket_capacity: 30,
                    },
                    grants: vec![],
                },
                0,
            )
            .unwrap();

        meter
            .set_costs(EndpointCosts {
                default_cost: 10,
                endpoint_costs: vec![EndpointCost {
                    method: "DELETE".into(),
                    path: "/trade-api/v2/portfolio/events/orders/:order_id".into(),
                    cost: 2,
                }],
            })
            .unwrap();

        meter
    }

    fn charge(bucket: Bucket, items: u32) -> Charge {
        Charge {
            method: "POST".into(),
            path: "/trade-api/v2/portfolio/events/orders".into(),
            bucket,
            items,
        }
    }

    #[rstest]
    fn weighted_refill_capacity_and_full_batch_admission() {
        let mut meter = meter();
        let batch = charge(Bucket::Write(0), 3);
        assert_eq!(meter.reserve(&batch).unwrap(), None);
        assert_eq!(meter.reserve(&batch).unwrap(), Some(Duration::from_secs(3)));
        meter.clock.advance(Duration::from_millis(2_999));
        assert_eq!(
            meter.reserve(&batch).unwrap(),
            Some(Duration::from_millis(1))
        );
        meter.clock.advance(Duration::from_millis(1));
        assert_eq!(meter.reserve(&batch).unwrap(), None);
        meter.clock.advance(Duration::from_secs(9));
        assert_eq!(meter.reserve(&batch).unwrap(), None);
        assert_eq!(
            meter
                .reserve(&charge(Bucket::Write(0), 4))
                .unwrap_err()
                .to_string(),
            "quota admission failed: request token cost exceeds bucket capacity"
        );
        assert_eq!(meter.reserve(&charge(Bucket::Read, 6)).unwrap(), None);
        assert_eq!(
            meter.reserve(&charge(Bucket::Read, 1)).unwrap(),
            Some(Duration::from_millis(500))
        );
    }

    #[rstest]
    fn shard_independence_and_atomic_auto_route() {
        let mut meter = meter();
        assert_eq!(meter.reserve(&charge(Bucket::Write(2), 3)).unwrap(), None);
        assert_eq!(
            meter.reserve(&charge(Bucket::Auto, 1)).unwrap(),
            Some(Duration::from_secs(1))
        );

        for shard in [0, 1, 3] {
            assert_eq!(
                meter.reserve(&charge(Bucket::Write(shard), 3)).unwrap(),
                None
            );
        }

        meter.clock.advance(Duration::from_secs(1));
        assert_eq!(meter.reserve(&charge(Bucket::Auto, 1)).unwrap(), None);

        for shard in [0, 1, 2, 3] {
            assert_eq!(
                meter.reserve(&charge(Bucket::Write(shard), 1)).unwrap(),
                Some(Duration::from_secs(1))
            );
        }
    }

    #[rstest]
    fn endpoint_weights_and_changed_costs_apply_to_every_item() {
        let mut meter = meter();
        let mut cancel = charge(Bucket::Write(1), 15);
        cancel.method = "DELETE".into();
        cancel.path.push_str("/order-id");
        assert_eq!(meter.reserve(&cancel).unwrap(), None);
        assert_eq!(
            meter.reserve(&cancel).unwrap(),
            Some(Duration::from_secs(3))
        );
        meter
            .set_costs(EndpointCosts {
                default_cost: 5,
                endpoint_costs: vec![],
            })
            .unwrap();

        meter.clock.advance(Duration::from_millis(500));
        assert_eq!(meter.reserve(&charge(Bucket::Write(1), 1)).unwrap(), None);
    }

    #[rstest]
    fn changed_limits_clamp_without_refilling_or_erasing_depletion() {
        let mut meter = meter();
        assert_eq!(meter.reserve(&charge(Bucket::Write(0), 2)).unwrap(), None);
        meter
            .set_limits(
                &AccountLimits {
                    usage_tier: "advanced".into(),
                    read: BucketLimit {
                        refill_rate: 30,
                        bucket_capacity: 90,
                    },
                    write: BucketLimit {
                        refill_rate: 5,
                        bucket_capacity: 15,
                    },
                    grants: vec![],
                },
                0,
            )
            .unwrap();

        assert_eq!(meter.reserve(&charge(Bucket::Write(0), 1)).unwrap(), None);
        assert_eq!(
            meter.reserve(&charge(Bucket::Write(0), 1)).unwrap(),
            Some(Duration::from_secs(2))
        );
    }

    #[rstest]
    fn missing_failed_and_expired_discovery_disable_writes() {
        let mut missing = Meter::new(FakeRelativeClock::default());
        assert!(
            missing
                .reserve(&charge(Bucket::Write(0), 1))
                .unwrap_err()
                .is_definitely_not_sent()
        );
        let mut meter = meter();
        meter.clock.advance(DISCOVERY_TTL);
        assert!(
            meter
                .reserve(&charge(Bucket::Write(0), 1))
                .unwrap_err()
                .is_definitely_not_sent()
        );
        assert_eq!(
            meter.read,
            BucketLimit {
                refill_rate: 200,
                bucket_capacity: 200
            }
        );
        assert!(meter.costs.is_none());
        assert_eq!(meter.reserve(&charge(Bucket::Read, 4)).unwrap(), None);
        assert_eq!(
            meter.reserve(&charge(Bucket::Read, 1)).unwrap(),
            Some(Duration::from_millis(250))
        );
    }

    #[rstest]
    fn balance_is_separate_and_has_no_burst() {
        let mut meter = meter();
        assert_eq!(meter.reserve(&charge(Bucket::Balance, 1)).unwrap(), None);
        assert_eq!(
            meter.reserve(&charge(Bucket::Balance, 1)).unwrap(),
            Some(Duration::from_secs(1))
        );
        assert_eq!(meter.reserve(&charge(Bucket::Read, 6)).unwrap(), None);
    }

    #[rstest]
    #[case("/trade-api/v2/a/:id", "/trade-api/v2/a/id", true)]
    #[case("/trade-api/v2/a/:id", "/trade-api/v2/a/id/more", false)]
    #[case("/trade-api/v2/a/*rest", "/trade-api/v2/a/id/more", true)]
    #[case("/trade-api/v2/a/:id", "/trade-api/v2/b/id", false)]
    fn endpoint_templates(#[case] pattern: &str, #[case] path: &str, #[case] expected: bool) {
        assert_eq!(path_matches(pattern, path), expected);
    }

    #[rstest]
    fn public_costs_expire_without_loaded_limits() {
        let mut meter = Meter::new(FakeRelativeClock::default());
        meter
            .set_costs(EndpointCosts {
                default_cost: 10,
                endpoint_costs: vec![],
            })
            .unwrap();

        assert_eq!(meter.reserve(&charge(Bucket::Read, 20)).unwrap(), None);
        assert!(meter.costs.is_some());
        meter.clock.advance(DISCOVERY_TTL);
        assert_eq!(meter.reserve(&charge(Bucket::Read, 4)).unwrap(), None);
        assert!(meter.costs.is_none());
        assert_eq!(
            meter.reserve(&charge(Bucket::Read, 1)).unwrap(),
            Some(Duration::from_millis(250))
        );
        assert!(meter.needs_refresh());
    }

    #[rstest]
    fn expired_reservation_keeps_discovery_due() {
        let mut meter = meter();
        meter.clock.advance(DISCOVERY_TTL);
        assert!(meter.reserve(&charge(Bucket::Write(0), 1)).is_err());
        assert!(meter.needs_refresh());
    }

    #[rstest]
    fn limit_only_refresh_does_not_extend_cost_freshness() {
        let mut meter = meter();
        meter.clock.advance(Duration::from_secs(30));

        let limits = AccountLimits {
            usage_tier: "basic".into(),
            read: meter.read.clone(),
            write: meter.write.clone(),
            grants: vec![],
        };

        meter.set_limits(&limits, 30_000).unwrap();
        meter.clock.advance(Duration::from_secs(29));
        assert!(!meter.needs_refresh());
        meter.clock.advance(Duration::from_secs(1));
        assert!(meter.needs_refresh());
    }

    #[rstest]
    #[case(1_000)]
    #[case(1_001)]
    fn received_expired_grant_is_immediately_stale(#[case] unix_ms: i64) {
        let mut meter = meter();

        let limits = AccountLimits {
            usage_tier: "expert".into(),
            read: meter.read.clone(),
            write: meter.write.clone(),
            grants: vec![super::super::models::UsageGrant {
                exchange_instance: "event_contract".into(),
                level: "expert".into(),
                source: "synthetic".into(),
                expires_ts: Field::Value(1),
            }],
        };

        let error = meter.set_limits(&limits, unix_ms).unwrap_err();
        assert!(matches!(
            error,
            KalshiHttpError::Quota("expired venue token grant")
        ));
    }

    #[rstest]
    fn temporary_grant_expires_at_exact_millisecond_boundary() {
        let mut meter = meter();

        let limits = AccountLimits {
            usage_tier: "expert".into(),
            read: meter.read.clone(),
            write: meter.write.clone(),
            grants: vec![super::super::models::UsageGrant {
                exchange_instance: "event_contract".into(),
                level: "expert".into(),
                source: "synthetic".into(),
                expires_ts: Field::Value(1),
            }],
        };

        meter.set_limits(&limits, 750).unwrap();
        meter.clock.advance(Duration::from_millis(249));
        assert!(!meter.needs_refresh());
        meter.clock.advance(Duration::from_millis(1));
        assert!(meter.needs_refresh());
        assert!(
            meter
                .reserve(&charge(Bucket::Write(0), 1))
                .unwrap_err()
                .is_definitely_not_sent()
        );
    }
}
