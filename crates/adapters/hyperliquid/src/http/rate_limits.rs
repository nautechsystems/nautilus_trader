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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::{Arc, LazyLock, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ahash::AHashMap;
use parking_lot::Mutex;
use serde_json::Value;

use crate::{
    common::{
        consts::HYPERLIQUID_REST_WEIGHT_PER_MINUTE,
        enums::{HyperliquidEnvironment, HyperliquidInfoRequestType},
        rate_limits::HyperliquidRouteScope,
    },
    http::{
        models::HyperliquidExchangeAction,
        query::{ExchangeAction, ExchangeActionParams, InfoRequest},
    },
};

type WeightedLimiterRegistry = Mutex<AHashMap<HyperliquidRouteScope, Weak<WeightedLimiter>>>;

static REST_LIMITERS: LazyLock<WeightedLimiterRegistry> =
    LazyLock::new(|| Mutex::new(AHashMap::new()));

#[derive(Debug)]
pub struct WeightedLimiter {
    capacity: f64,       // tokens per minute (e.g., 1200)
    refill_per_sec: f64, // capacity / 60
    state: tokio::sync::Mutex<State>,
}

#[derive(Debug)]
struct State {
    tokens: f64,
    last_refill: tokio::time::Instant,
}

impl WeightedLimiter {
    pub fn per_minute(capacity: u32) -> Self {
        let cap = capacity as f64;
        Self {
            capacity: cap,
            refill_per_sec: cap / 60.0,
            state: tokio::sync::Mutex::new(State {
                tokens: cap,
                last_refill: tokio::time::Instant::now(),
            }),
        }
    }

    /// Acquire `weight` tokens, sleeping until available.
    pub async fn acquire(&self, weight: u32) {
        let need = weight as f64;

        loop {
            let mut st = self.state.lock().await;
            Self::refill_locked(&mut st, self.refill_per_sec, self.capacity);

            if st.tokens >= need {
                st.tokens -= need;
                return;
            }
            let deficit = need - st.tokens;
            let secs = deficit / self.refill_per_sec;
            drop(st);
            tokio::time::sleep(Duration::from_secs_f64(secs.max(0.01))).await;
        }
    }

    /// Post-response debit for per-item adders.
    pub async fn debit_extra(&self, extra: u32) {
        if extra == 0 {
            return;
        }
        let mut st = self.state.lock().await;
        Self::refill_locked(&mut st, self.refill_per_sec, self.capacity);
        st.tokens -= extra as f64;
    }

    pub async fn snapshot(&self) -> RateLimitSnapshot {
        let mut st = self.state.lock().await;
        Self::refill_locked(&mut st, self.refill_per_sec, self.capacity);
        RateLimitSnapshot {
            capacity: self.capacity as u32,
            tokens: st.tokens.max(0.0) as u32,
        }
    }

    fn refill_locked(st: &mut State, per_sec: f64, cap: f64) {
        let now = tokio::time::Instant::now();
        let dt = now.duration_since(st.last_refill).as_secs_f64();
        if dt > 0.0 {
            st.tokens = (st.tokens + dt * per_sec).min(cap);
            st.last_refill = now;
        }
    }
}

pub(crate) fn shared_rest_limiter(
    environment: HyperliquidEnvironment,
    endpoint_url: &str,
    proxy_url: Option<&str>,
) -> Arc<WeightedLimiter> {
    let scope = HyperliquidRouteScope::new(environment, endpoint_url, proxy_url);
    let mut registry = REST_LIMITERS.lock();

    if let Some(limiter) = registry.get(&scope).and_then(Weak::upgrade) {
        return limiter;
    }

    let limiter = Arc::new(WeightedLimiter::per_minute(
        HYPERLIQUID_REST_WEIGHT_PER_MINUTE,
    ));
    registry.insert(scope, Arc::downgrade(&limiter));
    limiter
}

#[derive(Debug, Clone, Copy)]
pub struct RateLimitSnapshot {
    pub capacity: u32,
    pub tokens: u32,
}

pub fn backoff_full_jitter(attempt: u32, base: Duration, cap: Duration) -> Duration {
    let mut hasher = DefaultHasher::new();
    attempt.hash(&mut hasher);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    nanos.hash(&mut hasher);
    let hash = hasher.finish();

    let max = (base.as_millis() as u64)
        .saturating_mul(1u64 << attempt.min(16))
        .min(cap.as_millis() as u64)
        .max(base.as_millis() as u64);

    // Floor at 1ms to prevent zero-duration backoff
    Duration::from_millis((hash % max).max(1))
}

/// Classify Info requests into weight classes based on request type.
pub fn info_base_weight(req: &InfoRequest) -> u32 {
    match req.request_type {
        HyperliquidInfoRequestType::L2Book
        | HyperliquidInfoRequestType::AllMids
        | HyperliquidInfoRequestType::ClearinghouseState
        | HyperliquidInfoRequestType::OrderStatus
        | HyperliquidInfoRequestType::SpotClearinghouseState
        | HyperliquidInfoRequestType::ExchangeStatus => 2,
        HyperliquidInfoRequestType::UserRole => 60,
        _ => 20,
    }
}

/// Extra weight for heavy Info endpoints: +1 per 20 (most), +1 per 60 for candleSnapshot.
/// We count the largest array in the response (robust to schema variants).
pub fn info_extra_weight(req: &InfoRequest, json: &Value) -> u32 {
    let items = match json {
        Value::Array(a) => a.len(),
        Value::Object(m) => m
            .values()
            .filter_map(|v| v.as_array().map(|a| a.len()))
            .max()
            .unwrap_or(0),
        _ => 0,
    };

    let unit = match req.request_type {
        HyperliquidInfoRequestType::CandleSnapshot => 60usize,
        HyperliquidInfoRequestType::RecentTrades
        | HyperliquidInfoRequestType::HistoricalOrders
        | HyperliquidInfoRequestType::UserFills
        | HyperliquidInfoRequestType::UserFillsByTime
        | HyperliquidInfoRequestType::FundingHistory
        | HyperliquidInfoRequestType::UserFunding
        | HyperliquidInfoRequestType::NonUserFundingUpdates
        | HyperliquidInfoRequestType::TwapHistory
        | HyperliquidInfoRequestType::UserTwapSliceFills
        | HyperliquidInfoRequestType::UserTwapSliceFillsByTime
        | HyperliquidInfoRequestType::DelegatorHistory
        | HyperliquidInfoRequestType::DelegatorRewards
        | HyperliquidInfoRequestType::ValidatorStats => 20usize,
        _ => return 0,
    };
    (items / unit) as u32
}

pub(crate) const fn exchange_weight_for_batch(batch_size: usize) -> u32 {
    1 + (batch_size as u32 / 40)
}

/// Exchange: 1 + floor(batch_len / 40)
pub fn exchange_weight(action: &ExchangeAction) -> u32 {
    // Extract batch size from typed params
    let batch_size = match &action.params {
        ExchangeActionParams::Order(params) => params.orders.len(),
        ExchangeActionParams::Cancel(params) => params.cancels.len(),
        ExchangeActionParams::Modify(_) => {
            // Modify is for a single order
            1
        }
        ExchangeActionParams::UpdateLeverage(_) | ExchangeActionParams::UpdateIsolatedMargin(_) => {
            0
        }
    };
    exchange_weight_for_batch(batch_size)
}

/// Exchange weight for the canonical typed execution action model.
pub fn exec_action_weight(action: &HyperliquidExchangeAction) -> u32 {
    let batch_size = match action {
        HyperliquidExchangeAction::Order { orders, .. } => orders.len(),
        HyperliquidExchangeAction::Cancel { cancels, .. } => cancels.len(),
        HyperliquidExchangeAction::CancelByCloid { cancels, .. } => cancels.len(),
        HyperliquidExchangeAction::Modify { .. } => 1,
        HyperliquidExchangeAction::BatchModify { modifies } => modifies.len(),
        HyperliquidExchangeAction::UpdateLeverage { .. }
        | HyperliquidExchangeAction::UpdateIsolatedMargin { .. }
        | HyperliquidExchangeAction::ScheduleCancel { .. }
        | HyperliquidExchangeAction::UsdClassTransfer { .. }
        | HyperliquidExchangeAction::UserOutcome { .. }
        | HyperliquidExchangeAction::TwapPlace { .. }
        | HyperliquidExchangeAction::TwapCancel { .. }
        | HyperliquidExchangeAction::Noop => 0,
    };
    exchange_weight_for_batch(batch_size)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;
    use strum::IntoEnumIterator;

    use super::{
        super::models::{
            Cloid, HyperliquidExchangeAction, HyperliquidExchangeCancelByCloidRequest,
            HyperliquidExchangeCancelOrderRequest, HyperliquidExchangeGrouping,
            HyperliquidExchangeLimitParams, HyperliquidExchangeModifyOrderRequest,
            HyperliquidExchangeOrderKind, HyperliquidExchangePlaceOrderRequest,
            HyperliquidExchangeTif,
        },
        *,
    };
    use crate::{
        common::enums::HyperliquidEnvironment,
        http::query::{
            CancelParams, ExchangeAction, ExchangeActionParams, ExchangeActionType, InfoRequest,
            InfoRequestParams, OrderParams, UpdateLeverageParams,
        },
    };

    fn info_request(request_type: HyperliquidInfoRequestType) -> InfoRequest {
        InfoRequest {
            request_type,
            params: InfoRequestParams::None,
        }
    }

    #[rstest]
    fn test_info_base_weights_match_official_table() {
        let weight_two = [
            HyperliquidInfoRequestType::L2Book,
            HyperliquidInfoRequestType::AllMids,
            HyperliquidInfoRequestType::ClearinghouseState,
            HyperliquidInfoRequestType::OrderStatus,
            HyperliquidInfoRequestType::SpotClearinghouseState,
            HyperliquidInfoRequestType::ExchangeStatus,
        ];

        for request_type in HyperliquidInfoRequestType::iter() {
            let expected = if weight_two.contains(&request_type) {
                2
            } else if request_type == HyperliquidInfoRequestType::UserRole {
                60
            } else {
                20
            };

            assert_eq!(
                info_base_weight(&info_request(request_type)),
                expected,
                "unexpected base weight for {request_type:?}",
            );
        }
    }

    #[rstest]
    #[case(HyperliquidInfoRequestType::RecentTrades, 19, 0)]
    #[case(HyperliquidInfoRequestType::RecentTrades, 20, 1)]
    #[case(HyperliquidInfoRequestType::RecentTrades, 39, 1)]
    #[case(HyperliquidInfoRequestType::RecentTrades, 40, 2)]
    #[case(HyperliquidInfoRequestType::HistoricalOrders, 20, 1)]
    #[case(HyperliquidInfoRequestType::UserFills, 20, 1)]
    #[case(HyperliquidInfoRequestType::UserFillsByTime, 20, 1)]
    #[case(HyperliquidInfoRequestType::FundingHistory, 20, 1)]
    #[case(HyperliquidInfoRequestType::UserFunding, 20, 1)]
    #[case(HyperliquidInfoRequestType::NonUserFundingUpdates, 20, 1)]
    #[case(HyperliquidInfoRequestType::TwapHistory, 20, 1)]
    #[case(HyperliquidInfoRequestType::UserTwapSliceFills, 20, 1)]
    #[case(HyperliquidInfoRequestType::UserTwapSliceFillsByTime, 20, 1)]
    #[case(HyperliquidInfoRequestType::DelegatorHistory, 20, 1)]
    #[case(HyperliquidInfoRequestType::DelegatorRewards, 20, 1)]
    #[case(HyperliquidInfoRequestType::ValidatorStats, 20, 1)]
    #[case(HyperliquidInfoRequestType::CandleSnapshot, 59, 0)]
    #[case(HyperliquidInfoRequestType::CandleSnapshot, 60, 1)]
    #[case(HyperliquidInfoRequestType::CandleSnapshot, 119, 1)]
    #[case(HyperliquidInfoRequestType::CandleSnapshot, 120, 2)]
    fn test_info_extra_weights_match_official_table(
        #[case] request_type: HyperliquidInfoRequestType,
        #[case] item_count: usize,
        #[case] expected: u32,
    ) {
        let response = Value::Array(vec![Value::Null; item_count]);

        assert_eq!(
            info_extra_weight(&info_request(request_type), &response),
            expected,
        );
    }

    #[rstest]
    fn test_info_extra_weight_uses_largest_wrapped_array() {
        let response = serde_json::json!({
            "metadata": [1],
            "fills": vec![Value::Null; 40],
        });

        assert_eq!(
            info_extra_weight(
                &info_request(HyperliquidInfoRequestType::UserFills),
                &response,
            ),
            2,
        );
    }

    #[rstest]
    fn test_rest_limiter_shares_route_scope() {
        let info = shared_rest_limiter(
            HyperliquidEnvironment::Testnet,
            "https://rate-limit-share.example/info",
            None,
        );
        let exchange = shared_rest_limiter(
            HyperliquidEnvironment::Testnet,
            "https://rate-limit-share.example/exchange",
            None,
        );
        let proxied = shared_rest_limiter(
            HyperliquidEnvironment::Testnet,
            "https://rate-limit-share.example/info",
            Some("http://proxy.example:8080"),
        );

        assert!(Arc::ptr_eq(&info, &exchange));
        assert!(!Arc::ptr_eq(&info, &proxied));
    }

    fn exec_order() -> HyperliquidExchangePlaceOrderRequest {
        HyperliquidExchangePlaceOrderRequest {
            asset: 0,
            is_buy: true,
            price: Decimal::new(50000, 0),
            size: Decimal::new(1, 0),
            reduce_only: false,
            kind: HyperliquidExchangeOrderKind::Limit {
                limit: HyperliquidExchangeLimitParams {
                    tif: HyperliquidExchangeTif::Gtc,
                },
            },
            cloid: Some(Cloid::from_hex("0x00000000000000000000000000000000").unwrap()),
        }
    }

    fn exec_modify() -> HyperliquidExchangeModifyOrderRequest {
        HyperliquidExchangeModifyOrderRequest {
            oid: 12345.into(),
            order: exec_order(),
        }
    }

    fn exec_cancel_by_cloid() -> HyperliquidExchangeCancelByCloidRequest {
        HyperliquidExchangeCancelByCloidRequest {
            asset: 0,
            cloid: Cloid::from_hex("0x00000000000000000000000000000000").unwrap(),
        }
    }

    #[rstest]
    #[case(1, 1)]
    #[case(39, 1)]
    #[case(40, 2)]
    #[case(79, 2)]
    #[case(80, 3)]
    fn test_exchange_weight_order_steps_every_40(
        #[case] array_len: usize,
        #[case] expected_weight: u32,
    ) {
        let orders: Vec<HyperliquidExchangePlaceOrderRequest> =
            (0..array_len).map(|_| exec_order()).collect();

        let action = ExchangeAction {
            action_type: ExchangeActionType::Order,
            params: ExchangeActionParams::Order(OrderParams {
                orders,
                grouping: HyperliquidExchangeGrouping::Na,
                builder: None,
            }),
        };
        assert_eq!(exchange_weight(&action), expected_weight);
    }

    #[rstest]
    #[case(1, 1)]
    #[case(39, 1)]
    #[case(40, 2)]
    #[case(79, 2)]
    #[case(80, 3)]
    fn test_exec_action_weight_order_steps_every_40(
        #[case] array_len: usize,
        #[case] expected_weight: u32,
    ) {
        let action = HyperliquidExchangeAction::Order {
            orders: (0..array_len).map(|_| exec_order()).collect(),
            grouping: HyperliquidExchangeGrouping::Na,
            builder: None,
        };

        assert_eq!(exec_action_weight(&action), expected_weight);
    }

    #[rstest]
    #[case(1, 1)]
    #[case(39, 1)]
    #[case(40, 2)]
    #[case(79, 2)]
    #[case(80, 3)]
    fn test_exec_action_weight_cancel_by_oid_steps_every_40(
        #[case] array_len: usize,
        #[case] expected_weight: u32,
    ) {
        let action = HyperliquidExchangeAction::Cancel {
            cancels: (0..array_len)
                .map(|i| HyperliquidExchangeCancelOrderRequest {
                    asset: 0,
                    oid: i as u64,
                })
                .collect(),
            fast: None,
        };

        assert_eq!(exec_action_weight(&action), expected_weight);
    }

    #[rstest]
    #[case(1, 1)]
    #[case(39, 1)]
    #[case(40, 2)]
    #[case(79, 2)]
    #[case(80, 3)]
    fn test_exec_action_weight_cancel_by_cloid_steps_every_40(
        #[case] array_len: usize,
        #[case] expected_weight: u32,
    ) {
        let action = HyperliquidExchangeAction::CancelByCloid {
            cancels: (0..array_len).map(|_| exec_cancel_by_cloid()).collect(),
            fast: None,
        };

        assert_eq!(exec_action_weight(&action), expected_weight);
    }

    #[rstest]
    #[case(1, 1)]
    #[case(39, 1)]
    #[case(40, 2)]
    #[case(79, 2)]
    #[case(80, 3)]
    fn test_exec_action_weight_batch_modify_steps_every_40(
        #[case] array_len: usize,
        #[case] expected_weight: u32,
    ) {
        let action = HyperliquidExchangeAction::BatchModify {
            modifies: (0..array_len).map(|_| exec_modify()).collect(),
        };

        assert_eq!(exec_action_weight(&action), expected_weight);
    }

    #[rstest]
    fn test_exec_action_weight_modify() {
        let action = HyperliquidExchangeAction::Modify {
            modify: exec_modify(),
        };

        assert_eq!(exec_action_weight(&action), 1);
    }

    #[rstest]
    fn test_exec_action_weight_non_batch_action() {
        let action = HyperliquidExchangeAction::UpdateLeverage {
            asset: 1,
            is_cross: true,
            leverage: 10,
        };

        assert_eq!(exec_action_weight(&action), 1);
    }

    #[rstest]
    fn test_exchange_weight_cancel() {
        let cancels: Vec<HyperliquidExchangeCancelByCloidRequest> =
            (0..40).map(|_| exec_cancel_by_cloid()).collect();

        let action = ExchangeAction {
            action_type: ExchangeActionType::Cancel,
            params: ExchangeActionParams::Cancel(CancelParams {
                cancels,
                fast: None,
            }),
        };
        assert_eq!(exchange_weight(&action), 2);
    }

    #[rstest]
    fn test_exchange_weight_non_batch_action() {
        let update_leverage = ExchangeAction {
            action_type: ExchangeActionType::UpdateLeverage,
            params: ExchangeActionParams::UpdateLeverage(UpdateLeverageParams {
                asset: 1,
                is_cross: true,
                leverage: 10,
            }),
        };
        assert_eq!(exchange_weight(&update_leverage), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn test_limiter_roughly_caps_to_capacity() {
        let limiter = WeightedLimiter::per_minute(1200);

        // Consume ~1200 in quick succession
        for _ in 0..60 {
            limiter.acquire(20).await; // 60 * 20 = 1200
        }

        // The next acquire should take time for tokens to refill
        let t0 = tokio::time::Instant::now();
        limiter.acquire(20).await;
        let elapsed = t0.elapsed();

        assert_eq!(elapsed, Duration::from_secs(1));
    }

    #[tokio::test]
    async fn test_limiter_debit_extra_works() {
        let limiter = WeightedLimiter::per_minute(100);

        // Start with full bucket
        let snapshot = limiter.snapshot().await;
        assert_eq!(snapshot.capacity, 100);
        assert_eq!(snapshot.tokens, 100);

        // Acquire some tokens
        limiter.acquire(30).await;
        let snapshot = limiter.snapshot().await;
        assert_eq!(snapshot.tokens, 70);

        // Debit extra
        limiter.debit_extra(20).await;
        let snapshot = limiter.snapshot().await;
        assert_eq!(snapshot.tokens, 50);

        // Debit more than available. The snapshot stays nonnegative.
        limiter.debit_extra(100).await;
        let snapshot = limiter.snapshot().await;
        assert_eq!(snapshot.tokens, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn test_limiter_retains_response_weight_debt() {
        let limiter = WeightedLimiter::per_minute(60);
        limiter.acquire(50).await;
        limiter.debit_extra(20).await;
        let started = tokio::time::Instant::now();

        limiter.acquire(1).await;

        assert_eq!(
            tokio::time::Instant::now() - started,
            Duration::from_secs(11)
        );
    }

    #[rstest]
    #[case(0, 100)]
    #[case(1, 200)]
    #[case(2, 400)]
    fn test_backoff_full_jitter_increases(#[case] attempt: u32, #[case] max_expected_ms: u64) {
        let base = Duration::from_millis(100);
        let cap = Duration::from_secs(5);

        let delay = backoff_full_jitter(attempt, base, cap);

        assert!(delay.as_millis() >= 1);
        assert!(delay.as_millis() <= max_expected_ms as u128);
    }

    #[rstest]
    fn test_backoff_full_jitter_respects_cap() {
        let base = Duration::from_millis(100);
        let cap = Duration::from_secs(5);

        let delay_high = backoff_full_jitter(10, base, cap);
        assert!(delay_high.as_millis() <= cap.as_millis());
    }
}
