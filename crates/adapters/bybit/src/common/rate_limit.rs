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

//! Shared Bybit IP and UID rate-limit coordination.

use std::{
    collections::VecDeque,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use aws_lc_rs::digest;
use nautilus_network::ratelimiter::{RateLimiter, clock::MonotonicClock, quota::Quota};
use ustr::Ustr;

use super::enums::BybitProductType;

const WS_CONNECTION_KEY: &str = "connection";

const HTTP_IP_LIMIT: u32 = 600;
const HTTP_IP_WINDOW: Duration = Duration::from_secs(5);
const WS_ORDER_IP_LIMIT: u32 = 3_000;
const WS_ORDER_IP_WINDOW: Duration = Duration::from_secs(1);
const WS_CONNECTION_PERIOD: Duration = Duration::from_millis(600);

pub(crate) const BYBIT_RATE_LIMIT_HEADER: &str = "X-Bapi-Limit";
pub(crate) const BYBIT_RATE_LIMIT_STATUS_HEADER: &str = "X-Bapi-Limit-Status";
pub(crate) const BYBIT_RATE_LIMIT_RESET_HEADER: &str = "X-Bapi-Limit-Reset-Timestamp";
pub(crate) const BYBIT_HTTP_IP_COOLDOWN: Duration = Duration::from_secs(10 * 60);
pub(crate) const BYBIT_OPTION_SUBSCRIPTION_LIMIT: usize = 2_000;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TransportScope {
    host: String,
    proxy_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct AccountScope {
    environment: String,
    api_key_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum LimitKey {
    HttpIp(TransportScope),
    WsOrderIp(TransportScope),
    Account {
        scope: AccountScope,
        route: String,
    },
    #[cfg(test)]
    Test(String),
}

impl LimitKey {
    fn label(&self) -> &str {
        match self {
            Self::HttpIp(_) => "HTTP IP",
            Self::WsOrderIp(_) => "WebSocket order IP",
            Self::Account { route, .. } => route,
            #[cfg(test)]
            Self::Test(name) => name,
        }
    }
}

#[derive(Debug, Default)]
struct SlidingWindows {
    windows: Mutex<AHashMap<LimitKey, Window>>,
}

#[derive(Debug)]
struct Window {
    timestamps: VecDeque<tokio::time::Instant>,
    configured_limit: u32,
    observed_limit: Option<u32>,
    period: Duration,
    blocked_until: Option<tokio::time::Instant>,
}

impl Window {
    fn new(limit: u32, period: Duration) -> Self {
        Self {
            timestamps: VecDeque::new(),
            configured_limit: limit,
            observed_limit: None,
            period,
            blocked_until: None,
        }
    }

    fn limit(&self) -> u32 {
        self.observed_limit.unwrap_or(self.configured_limit)
    }

    fn prune(&mut self, now: tokio::time::Instant) {
        while self
            .timestamps
            .front()
            .is_some_and(|timestamp| now.duration_since(*timestamp) >= self.period)
        {
            self.timestamps.pop_front();
        }

        if self.blocked_until.is_some_and(|until| until <= now) {
            self.blocked_until = None;
        }
    }
}

#[derive(Debug)]
struct Reservation<'a> {
    key: &'a LimitKey,
    limit: u32,
    period: Duration,
    weight: u32,
}

impl SlidingWindows {
    async fn acquire(&self, reservations: &[Reservation<'_>]) -> Result<(), String> {
        loop {
            let wait = {
                let now = tokio::time::Instant::now();
                let mut windows = self
                    .windows
                    .lock()
                    .expect("Bybit rate-limit mutex poisoned");
                let mut wait = Duration::ZERO;

                for reservation in reservations {
                    let window = windows
                        .entry(reservation.key.clone())
                        .or_insert_with(|| Window::new(reservation.limit, reservation.period));
                    window.configured_limit = reservation.limit;
                    window.period = reservation.period;
                    window.prune(now);

                    if let Some(blocked_until) = window.blocked_until {
                        wait = wait.max(blocked_until.duration_since(now));
                    }

                    let limit = window.limit();
                    if reservation.weight > limit {
                        return Err(format!(
                            "Bybit request weight {} exceeds quota {} for {}",
                            reservation.weight,
                            limit,
                            reservation.key.label(),
                        ));
                    }
                    let used = u32::try_from(window.timestamps.len()).unwrap_or(u32::MAX);
                    if used.saturating_add(reservation.weight) > limit {
                        let needed = used.saturating_add(reservation.weight) - limit;
                        let index =
                            usize::try_from(needed - 1).expect("reservation index overflow");
                        let ready_at = window.timestamps[index] + window.period;
                        wait = wait.max(ready_at.duration_since(now));
                    }
                }

                if wait.is_zero() {
                    for reservation in reservations {
                        let window = windows
                            .get_mut(reservation.key)
                            .expect("Bybit rate-limit window missing after planning");
                        window
                            .timestamps
                            .extend(std::iter::repeat_n(now, reservation.weight as usize));
                    }
                }

                wait
            };

            if wait.is_zero() {
                return Ok(());
            }
            tokio::time::sleep(wait).await;
        }
    }

    fn observe(
        &self,
        key: &LimitKey,
        configured_limit: u32,
        period: Duration,
        limit: u32,
        remaining: u32,
        reset_timestamp_ms: Option<i64>,
    ) {
        if limit == 0 {
            return;
        }

        let now = tokio::time::Instant::now();
        let mut windows = self
            .windows
            .lock()
            .expect("Bybit rate-limit mutex poisoned");
        let window = windows
            .entry(key.clone())
            .or_insert_with(|| Window::new(configured_limit, period));
        window.configured_limit = configured_limit;
        window.observed_limit = Some(limit);
        window.period = period;
        window.prune(now);

        let observed_used = limit.saturating_sub(remaining);
        let local_used = u32::try_from(window.timestamps.len()).unwrap_or(u32::MAX);
        if observed_used > local_used {
            window.timestamps.extend(std::iter::repeat_n(
                now,
                (observed_used - local_used) as usize,
            ));
        }

        if remaining == 0
            && let Some(reset_timestamp_ms) = reset_timestamp_ms
        {
            let now_ms = jiff::Timestamp::now().as_millisecond();
            if reset_timestamp_ms > now_ms {
                window.blocked_until =
                    Some(now + Duration::from_millis((reset_timestamp_ms - now_ms) as u64));
            }
        }
    }

    fn block(&self, key: &LimitKey, limit: u32, period: Duration, duration: Duration) {
        let now = tokio::time::Instant::now();
        let mut windows = self
            .windows
            .lock()
            .expect("Bybit rate-limit mutex poisoned");
        let window = windows
            .entry(key.clone())
            .or_insert_with(|| Window::new(limit, period));
        window.blocked_until = Some(now + duration);
    }

    #[cfg(test)]
    fn observed_limit(&self, key: &LimitKey) -> Option<u32> {
        self.windows
            .lock()
            .expect("Bybit rate-limit mutex poisoned")
            .get(key)
            .and_then(|window| window.observed_limit)
    }
}

type ConnectionLimiter = RateLimiter<Ustr, MonotonicClock>;
type ConnectionRegistry = Mutex<AHashMap<TransportScope, Arc<ConnectionLimiter>>>;
type SessionRegistry = Mutex<AHashMap<TransportScope, Arc<AtomicU64>>>;

static LIMITS: LazyLock<Arc<SlidingWindows>> =
    LazyLock::new(|| Arc::new(SlidingWindows::default()));
static HTTP_SESSION_GENERATIONS: LazyLock<SessionRegistry> =
    LazyLock::new(|| Mutex::new(AHashMap::new()));
static WS_CONNECTION_LIMITERS: LazyLock<ConnectionRegistry> =
    LazyLock::new(|| Mutex::new(AHashMap::new()));

/// Coordinates all Bybit quotas visible to one adapter process.
#[derive(Clone, Debug)]
pub(crate) struct BybitRateLimiter {
    limits: Arc<SlidingWindows>,
    ip_key: LimitKey,
    account_scope: Option<AccountScope>,
    http_session_generation: Option<Arc<AtomicU64>>,
}

impl BybitRateLimiter {
    #[must_use]
    pub(crate) fn for_http(base_url: &str, api_key: Option<&str>, proxy_url: Option<&str>) -> Self {
        let transport_scope = transport_scope(base_url, proxy_url);
        let http_session_generation = shared_session_generation(&transport_scope);
        Self {
            limits: Arc::clone(&LIMITS),
            ip_key: LimitKey::HttpIp(transport_scope),
            account_scope: api_key.map(|key| account_scope(base_url, key)),
            http_session_generation: Some(http_session_generation),
        }
    }

    #[must_use]
    pub(crate) fn for_websocket(url: &str, api_key: Option<&str>, proxy_url: Option<&str>) -> Self {
        Self {
            limits: Arc::clone(&LIMITS),
            ip_key: LimitKey::WsOrderIp(transport_scope(url, proxy_url)),
            account_scope: api_key.map(|key| account_scope(url, key)),
            http_session_generation: None,
        }
    }

    pub(crate) async fn acquire_http(
        &self,
        endpoint: &str,
        category: Option<BybitProductType>,
        weight: u32,
        authenticated: bool,
    ) -> Result<(), String> {
        let account_reservation = if authenticated {
            self.account_reservation(endpoint, category, weight)
        } else {
            None
        };
        let mut reservations = Vec::with_capacity(2);
        if let Some(reservation) = &account_reservation {
            reservations.push(reservation.as_ref());
        }
        reservations.push(Reservation {
            key: &self.ip_key,
            limit: HTTP_IP_LIMIT,
            period: HTTP_IP_WINDOW,
            weight: 1,
        });
        self.limits.acquire(&reservations).await
    }

    pub(crate) async fn acquire_ws_order(
        &self,
        endpoint: &str,
        category: BybitProductType,
        weight: u32,
    ) -> Result<(), String> {
        let account_reservation = self.account_reservation(endpoint, Some(category), weight);
        let mut reservations = Vec::with_capacity(2);
        if let Some(reservation) = &account_reservation {
            reservations.push(reservation.as_ref());
        }
        reservations.push(Reservation {
            key: &self.ip_key,
            limit: WS_ORDER_IP_LIMIT,
            period: WS_ORDER_IP_WINDOW,
            weight: 1,
        });
        self.limits.acquire(&reservations).await
    }

    pub(crate) fn observe_account(
        &self,
        endpoint: &str,
        category: Option<BybitProductType>,
        limit: u32,
        remaining: u32,
        reset_timestamp_ms: Option<i64>,
    ) {
        if let Some(scope) = &self.account_scope {
            let (route, configured_limit) = account_limit(endpoint, category);
            self.limits.observe(
                &LimitKey::Account {
                    scope: scope.clone(),
                    route,
                },
                configured_limit,
                Duration::from_secs(1),
                limit,
                remaining,
                reset_timestamp_ms,
            );
        }
    }

    pub(crate) fn block_http_ip(&self) {
        self.limits.block(
            &self.ip_key,
            HTTP_IP_LIMIT,
            HTTP_IP_WINDOW,
            BYBIT_HTTP_IP_COOLDOWN,
        );
    }

    #[must_use]
    pub(crate) fn http_session_generation(&self) -> u64 {
        self.http_session_generation
            .as_ref()
            .map_or(0, |generation| generation.load(Ordering::Acquire))
    }

    pub(crate) fn reset_http_sessions(&self) -> u64 {
        self.block_http_ip();
        self.http_session_generation
            .as_ref()
            .map_or(0, |generation| {
                generation.fetch_add(1, Ordering::AcqRel) + 1
            })
    }

    fn account_reservation(
        &self,
        endpoint: &str,
        category: Option<BybitProductType>,
        weight: u32,
    ) -> Option<OwnedReservation> {
        let scope = self.account_scope.clone()?;
        let (route, limit) = account_limit(endpoint, category);
        Some(OwnedReservation {
            key: LimitKey::Account { scope, route },
            limit,
            period: Duration::from_secs(1),
            weight,
        })
    }
}

#[derive(Debug)]
struct OwnedReservation {
    key: LimitKey,
    limit: u32,
    period: Duration,
    weight: u32,
}

impl OwnedReservation {
    fn as_ref(&self) -> Reservation<'_> {
        Reservation {
            key: &self.key,
            limit: self.limit,
            period: self.period,
            weight: self.weight,
        }
    }
}

#[must_use]
pub(crate) fn websocket_connection_limiter(
    url: &str,
    proxy_url: Option<&str>,
) -> Arc<ConnectionLimiter> {
    let scope = transport_scope(url, proxy_url);
    let mut registry = WS_CONNECTION_LIMITERS
        .lock()
        .expect("Bybit WebSocket connection limiter registry mutex poisoned");
    if let Some(limiter) = registry.get(&scope) {
        return Arc::clone(limiter);
    }

    let quota = Quota::with_period(WS_CONNECTION_PERIOD).expect("valid constant");
    let limiter = Arc::new(RateLimiter::new_with_quota(
        None,
        vec![(Ustr::from(WS_CONNECTION_KEY), quota)],
    ));
    registry.insert(scope, Arc::clone(&limiter));
    limiter
}

#[must_use]
pub(crate) fn websocket_connection_key() -> Ustr {
    Ustr::from(WS_CONNECTION_KEY)
}

#[must_use]
pub(crate) const fn batch_call_limit(category: BybitProductType) -> usize {
    match category {
        BybitProductType::Option => 20,
        _ => batch_endpoint_limit(category),
    }
}

#[must_use]
pub(crate) const fn batch_endpoint_limit(category: BybitProductType) -> usize {
    match category {
        BybitProductType::Spot => 10,
        BybitProductType::Linear | BybitProductType::Inverse => 20,
        BybitProductType::Option => 5,
    }
}

#[must_use]
pub(crate) const fn batch_send_limit(category: BybitProductType) -> usize {
    match category {
        BybitProductType::Spot => 10,
        BybitProductType::Linear | BybitProductType::Inverse => 10,
        BybitProductType::Option => 5,
    }
}

#[must_use]
pub(crate) fn batch_weight(category: BybitProductType, order_count: usize) -> u32 {
    match category {
        BybitProductType::Option => 1,
        BybitProductType::Spot | BybitProductType::Linear | BybitProductType::Inverse => {
            u32::try_from(order_count).unwrap_or(u32::MAX)
        }
    }
}

#[must_use]
pub(crate) fn category_from_payload(payload: Option<&str>) -> Option<BybitProductType> {
    let payload = payload?;
    let category = if payload.trim_start().starts_with('{') {
        serde_json::from_str::<serde_json::Value>(payload)
            .ok()?
            .get("category")?
            .as_str()?
            .to_string()
    } else {
        serde_urlencoded::from_str::<Vec<(String, String)>>(payload)
            .ok()?
            .into_iter()
            .find_map(|(key, value)| (key == "category").then_some(value))?
    };

    match category.as_str() {
        "spot" => Some(BybitProductType::Spot),
        "linear" => Some(BybitProductType::Linear),
        "inverse" => Some(BybitProductType::Inverse),
        "option" => Some(BybitProductType::Option),
        _ => None,
    }
}

fn shared_session_generation(scope: &TransportScope) -> Arc<AtomicU64> {
    let mut registry = HTTP_SESSION_GENERATIONS
        .lock()
        .expect("Bybit HTTP session registry mutex poisoned");
    if let Some(generation) = registry.get(scope) {
        return Arc::clone(generation);
    }

    let generation = Arc::new(AtomicU64::new(0));
    registry.insert(scope.clone(), Arc::clone(&generation));
    generation
}

fn transport_scope(url: &str, proxy_url: Option<&str>) -> TransportScope {
    TransportScope {
        host: host(url),
        proxy_url: proxy_url.map(ToOwned::to_owned),
    }
}

fn account_scope(url: &str, api_key: &str) -> AccountScope {
    let digest = digest::digest(&digest::SHA256, api_key.as_bytes());
    let api_key_digest = digest
        .as_ref()
        .try_into()
        .expect("SHA-256 digest must contain 32 bytes");
    AccountScope {
        environment: environment_scope(url),
        api_key_digest,
    }
}

fn host(url: &str) -> String {
    url.split_once("://")
        .map_or(url, |(_, remainder)| remainder)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_ascii_lowercase()
}

fn environment_scope(url: &str) -> String {
    match host(url).as_str() {
        "api.bybit.com" | "stream.bybit.com" => "mainnet".to_string(),
        "api-testnet.bybit.com" | "stream-testnet.bybit.com" => "testnet".to_string(),
        "api-demo.bybit.com" | "stream-demo.bybit.com" => "demo".to_string(),
        custom => format!("custom:{custom}"),
    }
}

fn account_limit(endpoint: &str, category: Option<BybitProductType>) -> (String, u32) {
    let endpoint = endpoint.split('?').next().unwrap_or(endpoint);
    let category_name = category.map_or("all", BybitProductType::as_str);
    let key = format!("{endpoint}:{category_name}");
    let limit = match endpoint {
        "/v5/account/borrow" | "/v5/account/no-convert-repay" | "/v5/account/repay" => 1,
        "/v5/account/fee-rate"
        | "/v5/account/set-margin-mode"
        | "/v5/user/escrow_sub_members"
        | "/v5/user/submembers"
        | "/v5/user/update-api"
        | "/v5/user/update-sub-api" => 5,
        "/v5/account/info"
        | "/v5/account/wallet-balance"
        | "/v5/execution/list"
        | "/v5/order/history"
        | "/v5/order/realtime"
        | "/v5/position/list" => 50,
        "/v5/order/create" | "/v5/order/cancel" => match category {
            Some(BybitProductType::Spot) => 20,
            _ => 10,
        },
        "/v5/order/cancel-all" => match category {
            Some(BybitProductType::Spot) => 20,
            Some(BybitProductType::Option) => 1,
            _ => 10,
        },
        "/v5/order/amend" => 10,
        "/v5/order/amend-batch" | "/v5/order/cancel-batch" | "/v5/order/create-batch" => {
            match category {
                Some(BybitProductType::Spot) => 20,
                _ => 10,
            }
        }
        "/v5/position/set-leverage"
        | "/v5/position/switch-mode"
        | "/v5/position/trading-stop"
        | "/v5/user/query-api"
        | "/v5/user/query-sub-members"
        | "/v5/user/sub-apikeys" => 10,
        _ => 10,
    };
    (key, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key(name: &str) -> LimitKey {
        LimitKey::Test(name.to_string())
    }

    #[tokio::test(start_paused = true)]
    async fn sliding_window_does_not_refill_before_window_boundary() {
        let windows = Arc::new(SlidingWindows::default());
        let key = test_key("orders");
        let reservation = [Reservation {
            key: &key,
            limit: 2,
            period: Duration::from_secs(1),
            weight: 1,
        }];
        windows.acquire(&reservation).await.unwrap();
        windows.acquire(&reservation).await.unwrap();

        let waiting = {
            let windows = Arc::clone(&windows);

            tokio::spawn(async move {
                let key = test_key("orders");
                let reservation = [Reservation {
                    key: &key,
                    limit: 2,
                    period: Duration::from_secs(1),
                    weight: 1,
                }];
                windows.acquire(&reservation).await.unwrap();
                tokio::time::Instant::now()
            })
        };
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());

        tokio::time::advance(Duration::from_millis(999)).await;
        assert!(!waiting.is_finished());
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(waiting.await.unwrap(), tokio::time::Instant::now());
    }

    #[tokio::test(start_paused = true)]
    async fn weighted_reservation_is_atomic() {
        let windows = SlidingWindows::default();
        let key = test_key("batch");
        windows
            .acquire(&[Reservation {
                key: &key,
                limit: 10,
                period: Duration::from_secs(1),
                weight: 10,
            }])
            .await
            .unwrap();

        let reservation = [Reservation {
            key: &key,
            limit: 10,
            period: Duration::from_secs(1),
            weight: 1,
        }];
        let waiting = windows.acquire(&reservation);
        tokio::pin!(waiting);
        assert!(futures_util::poll!(&mut waiting).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        waiting.await.unwrap();
    }

    #[tokio::test]
    async fn oversized_weight_is_rejected_without_partial_reservation() {
        let windows = SlidingWindows::default();
        let key = test_key("oversized");

        let error = windows
            .acquire(&[Reservation {
                key: &key,
                limit: 10,
                period: Duration::from_secs(1),
                weight: 11,
            }])
            .await
            .unwrap_err();

        assert_eq!(
            error,
            "Bybit request weight 11 exceeds quota 10 for oversized"
        );
        windows
            .acquire(&[Reservation {
                key: &key,
                limit: 10,
                period: Duration::from_secs(1),
                weight: 10,
            }])
            .await
            .unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn observed_limit_changes_subsequent_waits() {
        let windows = SlidingWindows::default();
        let key = test_key("observed-orders");
        windows.observe(&key, 10, Duration::from_secs(1), 2, 0, None);

        let reservation = [Reservation {
            key: &key,
            limit: 10,
            period: Duration::from_secs(1),
            weight: 1,
        }];
        let waiting = windows.acquire(&reservation);
        tokio::pin!(waiting);
        assert!(futures_util::poll!(&mut waiting).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        waiting.await.unwrap();
    }

    #[rstest::rstest]
    fn authoritative_limit_is_retained() {
        let windows = SlidingWindows::default();
        let key = test_key("authoritative-orders");
        windows.observe(&key, 10, Duration::from_secs(1), 20, 19, None);
        assert_eq!(windows.observed_limit(&key), Some(20));
    }

    #[rstest::rstest]
    #[case("/v5/account/borrow", None, 1)]
    #[case("/v5/account/no-convert-repay", None, 1)]
    #[case("/v5/account/repay", None, 1)]
    #[case("/v5/account/fee-rate", None, 5)]
    #[case("/v5/account/set-margin-mode", None, 5)]
    #[case("/v5/user/escrow_sub_members", None, 5)]
    #[case("/v5/user/submembers", None, 5)]
    #[case("/v5/user/update-api", None, 5)]
    #[case("/v5/user/update-sub-api", None, 5)]
    #[case("/v5/account/info", None, 50)]
    #[case("/v5/account/wallet-balance", None, 50)]
    #[case("/v5/execution/list", None, 50)]
    #[case("/v5/order/history", None, 50)]
    #[case("/v5/order/realtime", None, 50)]
    #[case("/v5/position/list", None, 50)]
    #[case("/v5/order/create", Some(BybitProductType::Spot), 20)]
    #[case("/v5/order/create", Some(BybitProductType::Linear), 10)]
    #[case("/v5/order/cancel", Some(BybitProductType::Spot), 20)]
    #[case("/v5/order/cancel", Some(BybitProductType::Inverse), 10)]
    #[case("/v5/order/cancel-all", Some(BybitProductType::Spot), 20)]
    #[case("/v5/order/cancel-all", Some(BybitProductType::Option), 1)]
    #[case("/v5/order/cancel-all", Some(BybitProductType::Linear), 10)]
    #[case("/v5/order/amend", Some(BybitProductType::Spot), 10)]
    #[case("/v5/order/amend-batch", Some(BybitProductType::Linear), 10)]
    #[case("/v5/order/amend-batch", Some(BybitProductType::Option), 10)]
    #[case("/v5/order/amend-batch", Some(BybitProductType::Spot), 20)]
    #[case("/v5/order/cancel-batch", Some(BybitProductType::Inverse), 10)]
    #[case("/v5/order/cancel-batch", Some(BybitProductType::Option), 10)]
    #[case("/v5/order/cancel-batch", Some(BybitProductType::Spot), 20)]
    #[case("/v5/order/create-batch", Some(BybitProductType::Linear), 10)]
    #[case("/v5/order/create-batch", Some(BybitProductType::Option), 10)]
    #[case("/v5/order/create-batch", Some(BybitProductType::Spot), 20)]
    #[case("/v5/position/set-leverage", Some(BybitProductType::Linear), 10)]
    #[case("/v5/position/switch-mode", Some(BybitProductType::Inverse), 10)]
    #[case("/v5/position/trading-stop", Some(BybitProductType::Linear), 10)]
    #[case("/v5/user/query-api", None, 10)]
    #[case("/v5/user/query-sub-members", None, 10)]
    #[case("/v5/user/sub-apikeys", None, 10)]
    #[case("/v5/unknown", None, 10)]
    fn resolves_documented_endpoint_limits(
        #[case] endpoint: &str,
        #[case] category: Option<BybitProductType>,
        #[case] expected: u32,
    ) {
        assert_eq!(account_limit(endpoint, category).1, expected);
    }

    #[rstest::rstest]
    #[case(BybitProductType::Spot, 10, 10, 10, 10)]
    #[case(BybitProductType::Linear, 20, 20, 10, 10)]
    #[case(BybitProductType::Inverse, 20, 20, 10, 10)]
    #[case(BybitProductType::Option, 20, 5, 5, 1)]
    fn batch_limits_match_product_rules(
        #[case] category: BybitProductType,
        #[case] call_limit: usize,
        #[case] endpoint_limit: usize,
        #[case] send_limit: usize,
        #[case] weight: u32,
    ) {
        assert_eq!(batch_call_limit(category), call_limit);
        assert_eq!(batch_endpoint_limit(category), endpoint_limit);
        assert_eq!(batch_send_limit(category), send_limit);
        assert_eq!(batch_weight(category, 10), weight);
    }

    #[rstest::rstest]
    #[case(Some("category=linear&symbol=BTCUSDT"), Some(BybitProductType::Linear))]
    #[case(
        Some(r#"{"category":"option","symbol":"BTC-30AUG26-100000-C"}"#),
        Some(BybitProductType::Option)
    )]
    #[case(Some(r#"{"orderLinkId":"category=spot"}"#), None)]
    #[case(None, None)]
    fn parses_category_from_request_payload(
        #[case] payload: Option<&str>,
        #[case] expected: Option<BybitProductType>,
    ) {
        assert_eq!(category_from_payload(payload), expected);
    }

    #[tokio::test(start_paused = true)]
    async fn http_and_websocket_share_uid_state() {
        let http =
            BybitRateLimiter::for_http("https://shared-uid.invalid", Some("shared-key"), None);
        let websocket = BybitRateLimiter::for_websocket(
            "wss://shared-uid.invalid/v5/trade",
            Some("shared-key"),
            None,
        );

        for _ in 0..10 {
            http.acquire_http("/v5/order/amend", Some(BybitProductType::Linear), 1, true)
                .await
                .unwrap();
        }

        let waiting = websocket.acquire_ws_order("/v5/order/amend", BybitProductType::Linear, 1);
        tokio::pin!(waiting);
        assert!(futures_util::poll!(&mut waiting).is_pending());
        tokio::time::advance(Duration::from_secs(1)).await;
        waiting.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn account_and_ip_reservations_are_atomic() {
        let http = Arc::new(BybitRateLimiter::for_http(
            "https://atomic-scopes.invalid",
            Some("atomic-key"),
            None,
        ));
        let websocket = BybitRateLimiter::for_websocket(
            "wss://atomic-scopes.invalid/v5/trade",
            Some("atomic-key"),
            None,
        );

        for _ in 0..HTTP_IP_LIMIT {
            http.acquire_http("/v5/market/time", None, 1, false)
                .await
                .unwrap();
        }

        let waiting = {
            let http = Arc::clone(&http);
            tokio::spawn(async move {
                http.acquire_http("/v5/order/amend", Some(BybitProductType::Linear), 1, true)
                    .await
                    .unwrap();
            })
        };
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());

        for _ in 0..10 {
            websocket
                .acquire_ws_order("/v5/order/amend", BybitProductType::Linear, 1)
                .await
                .unwrap();
        }

        tokio::time::advance(HTTP_IP_WINDOW).await;
        waiting.await.unwrap();
    }

    #[rstest::rstest]
    fn shared_state_survives_client_recreation() {
        let generation = {
            let first = BybitRateLimiter::for_http(
                "https://retained-scope.invalid",
                Some("retained-key"),
                None,
            );
            first.reset_http_sessions()
        };
        let second = BybitRateLimiter::for_http(
            "https://retained-scope.invalid",
            Some("retained-key"),
            None,
        );

        assert_eq!(second.http_session_generation(), generation);
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limit_403_blocks_shared_http_scope_for_ten_minutes() {
        let first =
            BybitRateLimiter::for_http("https://cooldown-test.invalid", Some("first-key"), None);
        let second =
            BybitRateLimiter::for_http("https://cooldown-test.invalid", Some("second-key"), None);
        let generation = first.reset_http_sessions();
        assert_eq!(second.http_session_generation(), generation);

        let waiting = tokio::spawn(async move {
            second
                .acquire_http(
                    "/v5/order/realtime",
                    Some(BybitProductType::Linear),
                    1,
                    true,
                )
                .await
                .unwrap();
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        tokio::time::advance(
            BYBIT_HTTP_IP_COOLDOWN
                .checked_sub(Duration::from_millis(1))
                .unwrap(),
        )
        .await;
        assert!(!waiting.is_finished());
        tokio::time::advance(Duration::from_millis(1)).await;
        waiting.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn websocket_connection_attempts_share_domain_budget() {
        let first = websocket_connection_limiter("wss://connection-test.invalid/v5/trade", None);
        let second = websocket_connection_limiter("wss://connection-test.invalid/v5/private", None);
        assert!(Arc::ptr_eq(&first, &second));
        let key = websocket_connection_key();
        first.await_keys_ready(Some(&[key])).await;

        let waiting = tokio::spawn(async move {
            second
                .await_keys_ready(Some(&[websocket_connection_key()]))
                .await;
        });
        tokio::task::yield_now().await;
        assert!(!waiting.is_finished());
        tokio::time::advance(
            WS_CONNECTION_PERIOD
                .checked_sub(Duration::from_millis(1))
                .unwrap(),
        )
        .await;
        assert!(!waiting.is_finished());
        tokio::time::advance(Duration::from_millis(1)).await;
        waiting.await.unwrap();
    }
}
