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

use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct WeightedLimiter {
    capacity: f64,       // tokens per minute (e.g., 1200)
    refill_per_sec: f64, // capacity / 60
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    tokens: f64,
    last_refill: Instant,
}

impl WeightedLimiter {
    pub fn per_minute(capacity: u32) -> Self {
        let cap = capacity as f64;
        Self {
            capacity: cap,
            refill_per_sec: cap / 60.0,
            state: Mutex::new(State {
                tokens: cap,
                last_refill: Instant::now(),
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

    /// Post-response debit for per-items adders (can temporarily clamp to 0).
    pub async fn debit_extra(&self, extra: u32) {
        if extra == 0 {
            return;
        }
        let mut st = self.state.lock().await;
        Self::refill_locked(&mut st, self.refill_per_sec, self.capacity);
        st.tokens = (st.tokens - extra as f64).max(0.0);
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
        let dt = Instant::now().duration_since(st.last_refill).as_secs_f64();
        if dt > 0.0 {
            st.tokens = (st.tokens + dt * per_sec).min(cap);
            st.last_refill = Instant::now();
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RateLimitSnapshot {
    pub capacity: u32,
    pub tokens: u32,
}

pub fn backoff_full_jitter(attempt: u32, base: Duration, cap: Duration) -> Duration {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    // Simple pseudo-random based on attempt and time
    let mut hasher = DefaultHasher::new();
    attempt.hash(&mut hasher);
    Instant::now().elapsed().as_nanos().hash(&mut hasher);
    let hash = hasher.finish();

    let max = (base.as_millis() as u64)
        .saturating_mul(1u64 << attempt.min(16))
        .min(cap.as_millis() as u64)
        .max(base.as_millis() as u64);
    Duration::from_millis(hash % max)
}

/// Classify Info requests into weight classes based on request_type.
/// Since InfoRequest uses struct with request_type string, we match on that.
pub fn info_base_weight(req: &crate::http::query::InfoRequest) -> u32 {
    match req.request_type.as_str() {
        // Cheap (2)
        "l2Book"
        | "allMids"
        | "clearinghouseState"
        | "orderStatus"
        | "spotClearinghouseState"
        | "exchangeStatus" => 2,
        // Very expensive (60)
        "userRole" => 60,
        // Default (20)
        _ => 20,
    }
}

/// Extra weight for heavy Info endpoints: +1 per 20 (most), +1 per 60 for candleSnapshot.
/// We count the largest array in the response (robust to schema variants).
pub fn info_extra_weight(req: &crate::http::query::InfoRequest, json: &Value) -> u32 {
    let items = match json {
        Value::Array(a) => a.len(),
        Value::Object(m) => m
            .values()
            .filter_map(|v| v.as_array().map(|a| a.len()))
            .max()
            .unwrap_or(0),
        _ => 0,
    };

    let unit = match req.request_type.as_str() {
        "candleSnapshot" => 60usize, // +1 per 60
        "recentTrades"
        | "historicalOrders"
        | "userFills"
        | "userFillsByTime"
        | "fundingHistory"
        | "userFunding"
        | "nonUserFundingUpdates"
        | "twapHistory"
        | "userTwapSliceFills"
        | "userTwapSliceFillsByTime"
        | "delegatorHistory"
        | "delegatorRewards"
        | "validatorStats" => 20usize, // +1 per 20
        _ => return 0,
    };
    (items / unit) as u32
}

/// Exchange: 1 + floor(batch_len / 40)
pub fn exchange_weight(action: &crate::http::query::ExchangeAction) -> u32 {
    use crate::http::query::ExchangeActionParams;

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
    1 + (batch_size as u32 / 40)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::http::query::{
        CancelParams, ExchangeAction, ExchangeActionParams, ExchangeActionType, InfoRequest,
        InfoRequestParams, L2BookParams, OrderParams, UpdateLeverageParams, UserFillsParams,
    };

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
        use rust_decimal::Decimal;

        use super::super::models::{
            Cloid, HyperliquidExecLimitParams, HyperliquidExecOrderKind,
            HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
        };

        let orders: Vec<HyperliquidExecPlaceOrderRequest> = (0..array_len)
            .map(|_| HyperliquidExecPlaceOrderRequest {
                asset: 0,
                is_buy: true,
                price: Decimal::new(50000, 0),
                size: Decimal::new(1, 0),
                reduce_only: false,
                kind: HyperliquidExecOrderKind::Limit {
                    limit: HyperliquidExecLimitParams {
                        tif: HyperliquidExecTif::Gtc,
                    },
                },
                cloid: Some(Cloid::from_hex("0x00000000000000000000000000000000").unwrap()),
            })
            .collect();

        let action = ExchangeAction {
            action_type: ExchangeActionType::Order,
            params: ExchangeActionParams::Order(OrderParams {
                orders,
                grouping: "na".to_string(),
            }),
        };
        assert_eq!(exchange_weight(&action), expected_weight);
    }

    #[rstest]
    fn test_exchange_weight_cancel() {
        use super::super::models::{Cloid, HyperliquidExecCancelByCloidRequest};

        let cancels: Vec<HyperliquidExecCancelByCloidRequest> = (0..40)
            .map(|_| HyperliquidExecCancelByCloidRequest {
                asset: 0,
                cloid: Cloid::from_hex("0x00000000000000000000000000000000").unwrap(),
            })
            .collect();

        let action = ExchangeAction {
            action_type: ExchangeActionType::Cancel,
            params: ExchangeActionParams::Cancel(CancelParams { cancels }),
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

    #[rstest]
    #[case("l2Book", 2)]
    #[case("allMids", 2)]
    #[case("clearinghouseState", 2)]
    #[case("orderStatus", 2)]
    #[case("spotClearinghouseState", 2)]
    #[case("exchangeStatus", 2)]
    #[case("userRole", 60)]
    #[case("userFills", 20)]
    #[case("unknownEndpoint", 20)]
    fn test_info_base_weights(#[case] request_type: &str, #[case] expected_weight: u32) {
        let request = InfoRequest {
            request_type: request_type.to_string(),
            params: InfoRequestParams::L2Book(L2BookParams {
                coin: "BTC".to_string(),
            }),
        };
        assert_eq!(info_base_weight(&request), expected_weight);
    }

    #[rstest]
    fn test_info_extra_weight_no_charging() {
        let l2_book = InfoRequest {
            request_type: "l2Book".to_string(),
            params: InfoRequestParams::L2Book(L2BookParams {
                coin: "BTC".to_string(),
            }),
        };
        let large_json = json!(vec![1; 1000]);
        assert_eq!(info_extra_weight(&l2_book, &large_json), 0);
    }

    #[rstest]
    fn test_info_extra_weight_complex_json() {
        let user_fills = InfoRequest {
            request_type: "userFills".to_string(),
            params: InfoRequestParams::UserFills(UserFillsParams {
                user: "0x123".to_string(),
            }),
        };
        let complex_json = json!({
            "fills": vec![1; 40],
            "orders": vec![1; 20],
            "other": "data"
        });
        assert_eq!(info_extra_weight(&user_fills, &complex_json), 2); // largest array is 40, 40/20 = 2
    }

    #[tokio::test]
    async fn test_limiter_roughly_caps_to_capacity() {
        let limiter = WeightedLimiter::per_minute(1200);

        // Consume ~1200 in quick succession
        for _ in 0..60 {
            limiter.acquire(20).await; // 60 * 20 = 1200
        }

        // The next acquire should take time for tokens to refill
        let t0 = std::time::Instant::now();
        limiter.acquire(20).await;
        let elapsed = t0.elapsed();

        // Should take at least some time to refill (allow some jitter/timing variance)
        assert!(
            elapsed.as_millis() >= 500,
            "Expected significant delay, got {}ms",
            elapsed.as_millis()
        );
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

        // Debit more than available (should clamp to 0)
        limiter.debit_extra(100).await;
        let snapshot = limiter.snapshot().await;
        assert_eq!(snapshot.tokens, 0);
    }

    #[rstest]
    #[case(0, 100)]
    #[case(1, 200)]
    #[case(2, 400)]
    fn test_backoff_full_jitter_increases(#[case] attempt: u32, #[case] max_expected_ms: u64) {
        let base = Duration::from_millis(100);
        let cap = Duration::from_secs(5);

        let delay = backoff_full_jitter(attempt, base, cap);

        // Should be in expected ranges (allowing for jitter)
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
