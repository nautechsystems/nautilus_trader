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

//! Trading rate limits for the Polymarket CLOB API.

use std::{
    collections::HashMap,
    fmt::Display,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex as StdMutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nautilus_network::ratelimiter::quota::Quota;
use tokio::{
    sync::Mutex,
    time::{Instant, sleep},
};

use super::error::{Error, Result};
use crate::common::consts::HTTP_RATE_LIMIT;

pub(crate) const HEADER_RATE_LIMIT_REMAINING: &str = "Poly-RateLimit-Remaining";
pub(crate) const HEADER_RATE_LIMIT_RESET: &str = "Poly-RateLimit-Reset";
pub(crate) const HEADER_RATE_LIMIT_TIER: &str = "Poly-RateLimit-Tier";
pub(crate) const HEADER_RATE_LIMIT_WARNING: &str = "Poly-RateLimit-Warning";
pub(crate) const HEADER_RETRY_AFTER: &str = "Retry-After";

const RATE_LIMIT_HEADERS: [&str; 5] = [
    HEADER_RATE_LIMIT_REMAINING,
    HEADER_RATE_LIMIT_RESET,
    HEADER_RATE_LIMIT_TIER,
    HEADER_RATE_LIMIT_WARNING,
    HEADER_RETRY_AFTER,
];

static SIGNER_LIMITERS: LazyLock<StdMutex<HashMap<String, Arc<PolymarketRateLimiter>>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

/// Global REST quota for Polymarket Gamma API requests.
pub static POLYMARKET_GAMMA_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_minute(NonZeroU32::new(HTTP_RATE_LIMIT).unwrap()));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TradingBucket {
    Order,
    Cancel,
}

impl Display for TradingBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Order => f.write_str("order"),
            Self::Cancel => f.write_str("cancel"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RateLimitTier {
    #[default]
    Standard,
    Copper,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
    Elite,
}

impl RateLimitTier {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "standard" => Some(Self::Standard),
            "copper" => Some(Self::Copper),
            "bronze" => Some(Self::Bronze),
            "silver" => Some(Self::Silver),
            "gold" => Some(Self::Gold),
            "platinum" => Some(Self::Platinum),
            "diamond" => Some(Self::Diamond),
            "elite" => Some(Self::Elite),
            _ => None,
        }
    }

    const fn limits(self) -> TierLimits {
        match self {
            Self::Standard => TierLimits::new(40, 60, 80, 120, true),
            Self::Copper => TierLimits::new(60, 90, 120, 180, true),
            Self::Bronze => TierLimits::new(80, 120, 160, 240, true),
            Self::Silver => TierLimits::new(200, 300, 400, 600, true),
            Self::Gold => TierLimits::new(400, 600, 800, 1_200, true),
            Self::Platinum => TierLimits::new(450, 675, 900, 1_350, false),
            Self::Diamond => TierLimits::new(525, 787, 1_050, 1_575, false),
            Self::Elite => TierLimits::new(600, 900, 1_200, 1_800, false),
        }
    }
}

impl Display for RateLimitTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Standard => f.write_str("Standard"),
            Self::Copper => f.write_str("Copper"),
            Self::Bronze => f.write_str("Bronze"),
            Self::Silver => f.write_str("Silver"),
            Self::Gold => f.write_str("Gold"),
            Self::Platinum => f.write_str("Platinum"),
            Self::Diamond => f.write_str("Diamond"),
            Self::Elite => f.write_str("Elite"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TierLimits {
    order: BucketLimits,
    cancel: BucketLimits,
    negative_cancel_balance: bool,
}

impl TierLimits {
    const fn new(
        order_rate: u32,
        order_burst: u32,
        cancel_rate: u32,
        cancel_burst: u32,
        negative_cancel_balance: bool,
    ) -> Self {
        Self {
            order: BucketLimits::new(order_rate, order_burst),
            cancel: BucketLimits::new(cancel_rate, cancel_burst),
            negative_cancel_balance,
        }
    }

    const fn bucket(self, bucket: TradingBucket) -> BucketLimits {
        match bucket {
            TradingBucket::Order => self.order,
            TradingBucket::Cancel => self.cancel,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct BucketLimits {
    rate: f64,
    burst: u32,
}

impl BucketLimits {
    const fn new(rate: u32, burst: u32) -> Self {
        Self {
            rate: rate as f64,
            burst,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RateLimitHeaders {
    pub(crate) remaining: Option<f64>,
    pub(crate) reset: Option<f64>,
    tier: Option<RateLimitTier>,
    pub(crate) warning: bool,
    pub(crate) retry_after: Option<Duration>,
}

impl RateLimitHeaders {
    pub(crate) fn names() -> Vec<String> {
        RATE_LIMIT_HEADERS.into_iter().map(str::to_string).collect()
    }

    pub(crate) fn parse(headers: &HashMap<String, String>) -> Self {
        Self {
            remaining: parse_number(headers, HEADER_RATE_LIMIT_REMAINING, true),
            reset: parse_number(headers, HEADER_RATE_LIMIT_RESET, false),
            tier: parse_tier(headers),
            warning: parse_warning(headers),
            retry_after: parse_retry_after(headers),
        }
    }

    pub(crate) fn retry_after_ms(&self) -> Option<u64> {
        self.retry_after.map(|duration| {
            let partial_millisecond = u128::from(duration.subsec_nanos() % 1_000_000 != 0);
            u64::try_from(duration.as_millis().saturating_add(partial_millisecond))
                .unwrap_or(u64::MAX)
        })
    }
}

#[derive(Debug)]
pub(crate) struct PolymarketRateLimiter {
    state: Mutex<RateLimitState>,
}

impl PolymarketRateLimiter {
    pub(crate) fn for_signer(signer: &str) -> Arc<Self> {
        let signer = signer.to_ascii_lowercase();
        let mut limiters = SIGNER_LIMITERS
            .lock()
            .expect("Polymarket rate limiter registry mutex poisoned");
        limiters
            .entry(signer)
            .or_insert_with(|| {
                Arc::new(Self {
                    state: Mutex::new(RateLimitState::new()),
                })
            })
            .clone()
    }

    pub(crate) async fn acquire(
        &self,
        endpoint: &'static str,
        bucket: TradingBucket,
        cost: u32,
    ) -> Result<()> {
        if cost == 0 {
            return Err(Error::bad_request(format!(
                "{endpoint} token cost must be positive"
            )));
        }

        loop {
            let wait = {
                let mut state = self.state.lock().await;
                let now = Instant::now();
                state.refill(now);

                let limits = state.tier.limits().bucket(bucket);
                if cost > limits.burst {
                    return Err(Error::bad_request(format!(
                        "{endpoint} token cost {cost} exceeds {} tier {bucket} burst {}",
                        state.tier, limits.burst
                    )));
                }

                let bucket = state.bucket_mut(bucket);
                let blocked_for = bucket
                    .blocked_until
                    .and_then(|blocked_until| blocked_until.checked_duration_since(now))
                    .unwrap_or_default();
                let token_wait = if bucket.tokens >= f64::from(cost) {
                    Duration::ZERO
                } else {
                    Duration::from_secs_f64((f64::from(cost) - bucket.tokens) / limits.rate)
                };
                let wait = blocked_for.max(token_wait);

                if wait.is_zero() {
                    bucket.tokens -= f64::from(cost);
                    return Ok(());
                }

                wait
            };

            sleep(wait).await;
        }
    }

    pub(crate) async fn observe_response(
        &self,
        endpoint: &'static str,
        bucket: TradingBucket,
        request_cost: u32,
        post_response_cost: u32,
        headers: &RateLimitHeaders,
        rejected: bool,
    ) {
        let effective_tier = {
            let mut state = self.state.lock().await;
            let now = Instant::now();
            state.refill(now);

            if let Some(tier) = headers.tier
                && tier != state.tier
            {
                state.set_tier(tier);
            }

            let limits = state.tier.limits();
            let bucket_limits = limits.bucket(bucket);
            let bucket_state = state.bucket_mut(bucket);
            bucket_state.tokens -= f64::from(post_response_cost);

            if let Some(remaining) = headers.remaining {
                bucket_state.tokens = bucket_state
                    .tokens
                    .min(remaining)
                    .min(f64::from(bucket_limits.burst));
            }

            if bucket != TradingBucket::Cancel || !limits.negative_cancel_balance {
                bucket_state.tokens = bucket_state.tokens.max(0.0);
            }

            if let Some(retry_after) = headers.retry_after {
                bucket_state.block_for(now, retry_after);
            }

            if (rejected || bucket_state.tokens < 0.0)
                && let Some(reset_wait) = reset_wait(headers.reset)
            {
                bucket_state.block_for(now, reset_wait);
            }

            state.tier
        };

        if headers.warning {
            log::warn!(
                "{}",
                warning_message(
                    endpoint,
                    request_cost.saturating_add(post_response_cost),
                    effective_tier,
                    headers,
                )
            );
        }
    }
}

#[derive(Debug)]
struct RateLimitState {
    tier: RateLimitTier,
    order: BucketState,
    cancel: BucketState,
}

impl RateLimitState {
    fn new() -> Self {
        let now = Instant::now();
        let limits = RateLimitTier::Standard.limits();
        Self {
            tier: RateLimitTier::Standard,
            order: BucketState::new(limits.order, now),
            cancel: BucketState::new(limits.cancel, now),
        }
    }

    fn refill(&mut self, now: Instant) {
        let limits = self.tier.limits();
        self.order.refill(limits.order, now);
        self.cancel.refill(limits.cancel, now);
    }

    fn set_tier(&mut self, tier: RateLimitTier) {
        self.tier = tier;
        let limits = tier.limits();
        self.order.tokens = self.order.tokens.clamp(0.0, f64::from(limits.order.burst));
        self.cancel.tokens = self.cancel.tokens.min(f64::from(limits.cancel.burst));
        if !limits.negative_cancel_balance {
            self.cancel.tokens = self.cancel.tokens.max(0.0);
        }
    }

    fn bucket_mut(&mut self, bucket: TradingBucket) -> &mut BucketState {
        match bucket {
            TradingBucket::Order => &mut self.order,
            TradingBucket::Cancel => &mut self.cancel,
        }
    }
}

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    updated_at: Instant,
    blocked_until: Option<Instant>,
}

impl BucketState {
    fn new(limits: BucketLimits, now: Instant) -> Self {
        Self {
            tokens: f64::from(limits.burst),
            updated_at: now,
            blocked_until: None,
        }
    }

    fn refill(&mut self, limits: BucketLimits, now: Instant) {
        let elapsed = now.saturating_duration_since(self.updated_at);
        self.tokens =
            (self.tokens + elapsed.as_secs_f64() * limits.rate).min(f64::from(limits.burst));
        self.updated_at = now;

        if self
            .blocked_until
            .is_some_and(|blocked_until| blocked_until <= now)
        {
            self.blocked_until = None;
        }
    }

    fn block_for(&mut self, now: Instant, duration: Duration) {
        let Some(blocked_until) = now.checked_add(duration) else {
            return;
        };
        self.blocked_until = Some(
            self.blocked_until
                .map_or(blocked_until, |current| current.max(blocked_until)),
        );
    }
}

fn parse_number(
    headers: &HashMap<String, String>,
    name: &'static str,
    allow_negative: bool,
) -> Option<f64> {
    let value = headers.get(name)?;
    let parsed = value.parse::<f64>();
    match parsed {
        Ok(parsed) if parsed.is_finite() && (allow_negative || parsed >= 0.0) => Some(parsed),
        _ => {
            log::warn!("Invalid Polymarket rate-limit header {name}={value:?}");
            None
        }
    }
}

fn parse_tier(headers: &HashMap<String, String>) -> Option<RateLimitTier> {
    let value = headers.get(HEADER_RATE_LIMIT_TIER)?;
    let tier = RateLimitTier::parse(value);
    if tier.is_none() {
        log::warn!("Invalid Polymarket rate-limit header {HEADER_RATE_LIMIT_TIER}={value:?}");
    }
    tier
}

fn parse_warning(headers: &HashMap<String, String>) -> bool {
    let Some(value) = headers.get(HEADER_RATE_LIMIT_WARNING) else {
        return false;
    };

    match value.trim().to_ascii_lowercase().as_str() {
        "true" => true,
        "false" => false,
        _ => {
            log::warn!(
                "Invalid Polymarket rate-limit header {HEADER_RATE_LIMIT_WARNING}={value:?}"
            );
            false
        }
    }
}

fn parse_retry_after(headers: &HashMap<String, String>) -> Option<Duration> {
    let seconds = parse_number(headers, HEADER_RETRY_AFTER, false)?;
    let Ok(duration) = Duration::try_from_secs_f64(seconds) else {
        log::warn!("Invalid Polymarket rate-limit header {HEADER_RETRY_AFTER}={seconds:?}");
        return None;
    };

    if Instant::now().checked_add(duration).is_none() {
        log::warn!("Invalid Polymarket rate-limit header {HEADER_RETRY_AFTER}={seconds:?}");
        return None;
    }
    Some(duration)
}

fn reset_wait(reset: Option<f64>) -> Option<Duration> {
    let reset = reset?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    let wait = reset - now;
    if wait <= 0.0 {
        return None;
    }
    let duration = Duration::try_from_secs_f64(wait).ok()?;
    Instant::now().checked_add(duration).map(|_| duration)
}

fn warning_message(
    endpoint: &str,
    token_cost: u32,
    tier: RateLimitTier,
    headers: &RateLimitHeaders,
) -> String {
    let remaining = headers
        .remaining
        .map_or_else(|| "unknown".to_string(), |value| value.to_string());
    let reset = headers
        .reset
        .map_or_else(|| "unknown".to_string(), |value| value.to_string());
    format!(
        "Polymarket rate limit warning: endpoint={endpoint}, token_cost={token_cost}, \
         tier={tier}, remaining={remaining}, reset={reset}"
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn header_map(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
            .collect()
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_signer_limiters_share_state_and_keep_buckets_separate() {
        let first = PolymarketRateLimiter::for_signer("0xSigner-Separation");
        let second = PolymarketRateLimiter::for_signer("0xsigner-separation");
        let other = PolymarketRateLimiter::for_signer("0xother-signer-separation");

        first
            .acquire("/orders", TradingBucket::Order, 10)
            .await
            .unwrap();

        let state = second.state.lock().await;
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &other));
        assert_eq!(state.order.tokens, 50.0);
        assert_eq!(state.cancel.tokens, 120.0);
        drop(state);
        drop(first);
        drop(second);

        let replacement = PolymarketRateLimiter::for_signer("0xsigner-separation");
        let replacement_state = replacement.state.lock().await;
        assert_eq!(replacement_state.order.tokens, 50.0);
        assert_eq!(replacement_state.cancel.tokens, 120.0);
    }

    #[rstest]
    #[case::standard(RateLimitTier::Standard, 40, 60, 80, 120, true)]
    #[case::copper(RateLimitTier::Copper, 60, 90, 120, 180, true)]
    #[case::bronze(RateLimitTier::Bronze, 80, 120, 160, 240, true)]
    #[case::silver(RateLimitTier::Silver, 200, 300, 400, 600, true)]
    #[case::gold(RateLimitTier::Gold, 400, 600, 800, 1_200, true)]
    #[case::platinum(RateLimitTier::Platinum, 450, 675, 900, 1_350, false)]
    #[case::diamond(RateLimitTier::Diamond, 525, 787, 1_050, 1_575, false)]
    #[case::elite(RateLimitTier::Elite, 600, 900, 1_200, 1_800, false)]
    fn test_tier_limits_match_documented_contract(
        #[case] tier: RateLimitTier,
        #[case] order_rate: u32,
        #[case] order_burst: u32,
        #[case] cancel_rate: u32,
        #[case] cancel_burst: u32,
        #[case] negative_cancel_balance: bool,
    ) {
        let limits = tier.limits();

        assert_eq!(limits.order.rate, f64::from(order_rate));
        assert_eq!(limits.order.burst, order_burst);
        assert_eq!(limits.cancel.rate, f64::from(cancel_rate));
        assert_eq!(limits.cancel.burst, cancel_burst);
        assert_eq!(limits.negative_cancel_balance, negative_cancel_balance);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_weighted_batches_debit_entry_counts() {
        let limiter = PolymarketRateLimiter::for_signer("0xweighted-batches");

        limiter
            .acquire("/orders", TradingBucket::Order, 15)
            .await
            .unwrap();
        limiter
            .acquire("/orders", TradingBucket::Cancel, 7)
            .await
            .unwrap();

        let state = limiter.state.lock().await;
        assert_eq!(state.order.tokens, 45.0);
        assert_eq!(state.cancel.tokens, 113.0);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_stale_remaining_header_cannot_credit_spent_tokens() {
        let limiter = PolymarketRateLimiter::for_signer("0xstale-remaining");
        limiter
            .acquire("/order", TradingBucket::Order, 1)
            .await
            .unwrap();
        limiter
            .acquire("/order", TradingBucket::Order, 1)
            .await
            .unwrap();
        let current = RateLimitHeaders::parse(&header_map(&[(HEADER_RATE_LIMIT_REMAINING, "58")]));
        let stale = RateLimitHeaders::parse(&header_map(&[(HEADER_RATE_LIMIT_REMAINING, "59")]));

        limiter
            .observe_response("/order", TradingBucket::Order, 1, 0, &current, false)
            .await;
        limiter
            .observe_response("/order", TradingBucket::Order, 1, 0, &stale, false)
            .await;

        assert_eq!(limiter.state.lock().await.order.tokens, 58.0);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_response_tier_updates_both_bucket_limits() {
        let limiter = PolymarketRateLimiter::for_signer("0xtier-update");
        limiter
            .acquire("/order", TradingBucket::Order, 1)
            .await
            .unwrap();
        let headers = RateLimitHeaders::parse(&header_map(&[
            (HEADER_RATE_LIMIT_TIER, "Silver"),
            (HEADER_RATE_LIMIT_REMAINING, "299"),
        ]));

        limiter
            .observe_response("/order", TradingBucket::Order, 1, 0, &headers, false)
            .await;

        let state = limiter.state.lock().await;
        assert_eq!(state.tier, RateLimitTier::Silver);
        assert_eq!(state.tier.limits().order.rate, 200.0);
        assert_eq!(state.tier.limits().order.burst, 300);
        assert_eq!(state.tier.limits().cancel.rate, 400.0);
        assert_eq!(state.tier.limits().cancel.burst, 600);
        assert_eq!(state.order.tokens, 59.0);
    }

    #[rstest]
    fn test_warning_headers_parse_and_log_all_required_fields() {
        let headers = RateLimitHeaders::parse(&header_map(&[
            (HEADER_RATE_LIMIT_REMAINING, "-2.5"),
            (HEADER_RATE_LIMIT_RESET, "1786100000"),
            (HEADER_RATE_LIMIT_TIER, "Gold"),
            (HEADER_RATE_LIMIT_WARNING, "true"),
            (HEADER_RETRY_AFTER, "1.2500001"),
        ]));

        assert_eq!(
            RateLimitHeaders::names(),
            vec![
                HEADER_RATE_LIMIT_REMAINING.to_string(),
                HEADER_RATE_LIMIT_RESET.to_string(),
                HEADER_RATE_LIMIT_TIER.to_string(),
                HEADER_RATE_LIMIT_WARNING.to_string(),
                HEADER_RETRY_AFTER.to_string(),
            ]
        );
        assert_eq!(headers.remaining, Some(-2.5));
        assert_eq!(headers.reset, Some(1_786_100_000.0));
        assert_eq!(headers.tier, Some(RateLimitTier::Gold));
        assert!(headers.warning);
        assert_eq!(
            headers.retry_after,
            Some(Duration::from_nanos(1_250_000_100))
        );
        assert_eq!(headers.retry_after_ms(), Some(1_251));
        assert_eq!(
            warning_message("/orders", 8, RateLimitTier::Gold, &headers),
            "Polymarket rate limit warning: endpoint=/orders, token_cost=8, tier=Gold, \
             remaining=-2.5, reset=1786100000"
        );
    }

    #[rstest]
    fn test_duration_header_overflow_is_ignored() {
        let headers =
            RateLimitHeaders::parse(&header_map(&[(HEADER_RETRY_AFTER, "18446744073709551616")]));

        assert_eq!(headers.retry_after, None);
        assert_eq!(reset_wait(Some(18_446_744_073_709_552_000.0)), None);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_negative_cancel_balance_blocks_until_refilled() {
        let limiter = PolymarketRateLimiter::for_signer("0xnegative-cancel");
        let headers = RateLimitHeaders::parse(&header_map(&[
            (HEADER_RATE_LIMIT_REMAINING, "-5"),
            (HEADER_RATE_LIMIT_TIER, "Standard"),
        ]));
        limiter
            .observe_response("/cancel-all", TradingBucket::Cancel, 1, 0, &headers, false)
            .await;

        let limiter_clone = limiter.clone();
        let acquire = tokio::spawn(async move {
            limiter_clone
                .acquire("/order", TradingBucket::Cancel, 1)
                .await
        });
        tokio::task::yield_now().await;
        assert!(!acquire.is_finished());

        tokio::time::advance(Duration::from_millis(74)).await;
        tokio::task::yield_now().await;
        assert!(!acquire.is_finished());

        tokio::time::advance(Duration::from_millis(1)).await;
        acquire.await.unwrap().unwrap();
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_post_cancel_debit_allows_standard_debt_and_floors_platinum() {
        let standard = PolymarketRateLimiter::for_signer("0xpost-cancel-standard");
        let empty = RateLimitHeaders::default();
        standard
            .acquire("/cancel-all", TradingBucket::Cancel, 1)
            .await
            .unwrap();
        standard
            .observe_response("/cancel-all", TradingBucket::Cancel, 1, 3, &empty, false)
            .await;
        assert_eq!(standard.state.lock().await.cancel.tokens, 116.0);

        let zero_standard = RateLimitHeaders::parse(&header_map(&[
            (HEADER_RATE_LIMIT_REMAINING, "0"),
            (HEADER_RATE_LIMIT_TIER, "Standard"),
        ]));
        standard
            .observe_response(
                "/cancel-all",
                TradingBucket::Cancel,
                1,
                0,
                &zero_standard,
                false,
            )
            .await;
        standard
            .observe_response("/cancel-all", TradingBucket::Cancel, 1, 3, &empty, false)
            .await;

        let platinum = PolymarketRateLimiter::for_signer("0xpost-cancel-platinum");
        let zero_platinum = RateLimitHeaders::parse(&header_map(&[
            (HEADER_RATE_LIMIT_REMAINING, "0"),
            (HEADER_RATE_LIMIT_TIER, "Platinum"),
        ]));
        platinum
            .observe_response(
                "/cancel-market-orders",
                TradingBucket::Cancel,
                1,
                2,
                &zero_platinum,
                false,
            )
            .await;

        assert_eq!(standard.state.lock().await.cancel.tokens, -3.0);
        assert_eq!(platinum.state.lock().await.cancel.tokens, 0.0);
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_standard_order_burst_is_all_or_nothing() {
        let limiter = PolymarketRateLimiter::for_signer("0xorder-burst");
        limiter
            .acquire("/orders", TradingBucket::Order, 60)
            .await
            .unwrap();

        let limiter_clone = limiter.clone();
        let acquire = tokio::spawn(async move {
            limiter_clone
                .acquire("/order", TradingBucket::Order, 1)
                .await
        });
        tokio::task::yield_now().await;
        assert!(!acquire.is_finished());

        tokio::time::advance(Duration::from_millis(24)).await;
        tokio::task::yield_now().await;
        assert!(!acquire.is_finished());

        tokio::time::advance(Duration::from_millis(1)).await;
        acquire.await.unwrap().unwrap();

        let error = limiter
            .acquire("/orders", TradingBucket::Order, 61)
            .await
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "bad request: /orders token cost 61 exceeds Standard tier order burst 60"
        );
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_retry_after_blocks_next_attempt_for_required_delay() {
        let limiter = PolymarketRateLimiter::for_signer("0xretry-after");
        let headers = RateLimitHeaders::parse(&header_map(&[
            (HEADER_RATE_LIMIT_REMAINING, "60"),
            (HEADER_RETRY_AFTER, "2"),
        ]));
        limiter
            .observe_response("/order", TradingBucket::Order, 1, 0, &headers, true)
            .await;

        let limiter_clone = limiter.clone();
        let acquire = tokio::spawn(async move {
            limiter_clone
                .acquire("/order", TradingBucket::Order, 1)
                .await
        });
        tokio::task::yield_now().await;
        assert!(!acquire.is_finished());

        tokio::time::advance(Duration::from_millis(1_999)).await;
        tokio::task::yield_now().await;
        assert!(!acquire.is_finished());

        tokio::time::advance(Duration::from_millis(1)).await;
        acquire.await.unwrap().unwrap();
    }
}
