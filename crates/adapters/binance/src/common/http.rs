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

//! HTTP admission and retry timing for the Binance adapter.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::Duration,
};

use ahash::AHashMap;
use aws_lc_rs::digest;
use futures_util::FutureExt;
use jiff::Timestamp;
use nautilus_core::collections::into_ustr_vec;
use nautilus_network::{
    dst::{self, time::Instant},
    http::{HttpClient, parse_retry_after},
    retry::RetryConfig,
};
use parking_lot::Mutex;

use crate::common::{
    consts::BINANCE_RETRY_AFTER_HEADER,
    credential::SigningCredential,
    enums::{BinanceEnvironment, BinanceProductType},
    error::{BINANCE_IP_BAN_STATUS, BINANCE_ORDER_RATE_LIMIT_CODE, BINANCE_RATE_LIMIT_STATUS},
    urls::get_http_base_url,
};

type SharedBlock = Arc<Mutex<Option<HttpBlock>>>;

static HTTP_BACKOFF: LazyLock<Mutex<AHashMap<HttpScope, SharedBlock>>> =
    LazyLock::new(|| Mutex::new(AHashMap::new()));

#[derive(Debug, Clone)]
pub(crate) struct BinanceHttpBackoff {
    ip: SharedBlock,
    orders: Option<SharedBlock>,
}

impl BinanceHttpBackoff {
    pub(crate) fn new(
        product: BinanceProductType,
        environment: BinanceEnvironment,
        base_url: Option<&str>,
        proxy_url: Option<&str>,
        credential: Option<&SigningCredential>,
    ) -> Self {
        let product = match product {
            BinanceProductType::CoinM => BinanceProductType::UsdM,
            product => product,
        };

        let endpoint = BinanceHttpEndpointScope::new(product, environment, base_url);

        let ip = Self::shared(HttpScope {
            product,
            endpoint: endpoint.clone(),
            proxy: proxy_url.map(sha256),
            account: None,
        });

        let orders = credential.map(|credential| {
            Self::shared(HttpScope {
                product,
                endpoint,
                proxy: None,
                account: Some(sha256(credential.api_key())),
            })
        });

        Self { ip, orders }
    }

    fn shared(scope: HttpScope) -> SharedBlock {
        let mut registry = HTTP_BACKOFF.lock();
        registry.retain(|_, block| {
            Arc::strong_count(block) > 1
                || match *block.lock() {
                    Some(HttpBlock::Until(deadline)) => deadline > Instant::now(),
                    Some(HttpBlock::Unknown) => true,
                    None => false,
                }
        });

        registry.entry(scope).or_default().clone()
    }

    pub(crate) async fn admit(
        &self,
        client: &HttpClient,
        keys: Vec<String>,
        use_orders: bool,
        wait: bool,
    ) -> Result<(), ()> {
        let keys = into_ustr_vec(keys);

        loop {
            let remaining = self.remaining(use_orders)?;
            if !remaining.is_zero() {
                if !wait {
                    return Err(());
                }

                dst::time::sleep(remaining).await;
                continue;
            }

            let quota = client.await_rate_limits(Some(&keys));
            if wait {
                quota.await;
            } else if quota.now_or_never().is_none() {
                return Err(());
            }

            if self.remaining(use_orders)?.is_zero() {
                return Ok(());
            }

            if !wait {
                return Err(());
            }
        }
    }

    fn remaining(&self, use_orders: bool) -> Result<Duration, ()> {
        let ip = self.ip.lock();
        let orders = self
            .orders
            .as_ref()
            .filter(|_| use_orders)
            .map(|orders| orders.lock());
        let now = Instant::now();
        let mut remaining = Duration::ZERO;

        for block in [
            ip.as_ref(),
            orders.as_ref().and_then(|orders| orders.as_ref()),
        ] {
            match block {
                Some(HttpBlock::Until(deadline)) => {
                    remaining = remaining.max(deadline.saturating_duration_since(now));
                }
                Some(HttpBlock::Unknown) => return Err(()),
                None => {}
            }
        }

        Ok(remaining)
    }

    pub(crate) fn observe(&self, status: u16, code: Option<i64>, delay: Option<Duration>) {
        if status != BINANCE_RATE_LIMIT_STATUS && status != BINANCE_IP_BAN_STATUS {
            return;
        }

        let block =
            if status == BINANCE_RATE_LIMIT_STATUS && code == Some(BINANCE_ORDER_RATE_LIMIT_CODE) {
                let Some(orders) = &self.orders else {
                    return;
                };

                orders
            } else {
                &self.ip
            };

        let next = match delay {
            Some(delay) => Instant::now().checked_add(delay).map(HttpBlock::Until),
            None if status == BINANCE_RATE_LIMIT_STATUS => return,
            None => None,
        }
        .unwrap_or(HttpBlock::Unknown);

        let mut current = block.lock();

        match (*current, next) {
            (Some(HttpBlock::Unknown), _) => {}
            (_, HttpBlock::Unknown) => {
                *current = Some(HttpBlock::Unknown);
                log::error!(
                    "Binance HTTP rate-limit expiry is unavailable, requests in this quota scope are blocked until process restart"
                );
            }
            (Some(HttpBlock::Until(deadline)), HttpBlock::Until(next)) => {
                *current = Some(HttpBlock::Until(deadline.max(next)));
            }
            (None, next) => *current = Some(next),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum BinanceHttpEndpointScope {
    Environment(BinanceEnvironment),
    Custom {
        environment: BinanceEnvironment,
        base_url: String,
    },
}

impl BinanceHttpEndpointScope {
    pub(crate) fn new(
        product: BinanceProductType,
        environment: BinanceEnvironment,
        base_url: Option<&str>,
    ) -> Self {
        let Some(base_url) = base_url else {
            return Self::Environment(environment);
        };

        let normalized = base_url.trim_end_matches('/');
        let official = normalized == get_http_base_url(product, environment)
            || (product.is_futures()
                && normalized == get_http_base_url(BinanceProductType::CoinM, environment));
        if official {
            Self::Environment(environment)
        } else {
            Self::Custom {
                environment,
                base_url: normalized.to_string(),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HttpScope {
    product: BinanceProductType,
    endpoint: BinanceHttpEndpointScope,
    proxy: Option<[u8; 32]>,
    account: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Copy)]
enum HttpBlock {
    Until(Instant),
    Unknown,
}

pub(crate) fn sha256(value: &str) -> [u8; 32] {
    digest::digest(&digest::SHA256, value.as_bytes())
        .as_ref()
        .try_into()
        .expect("SHA-256 digest must contain 32 bytes")
}

pub(crate) fn retry_after(headers: &HashMap<String, String>, now: Timestamp) -> Option<Duration> {
    let value = headers.get(BINANCE_RETRY_AFTER_HEADER)?;
    let delay = parse_retry_after(value, now);
    if delay.is_none() {
        log::warn!("Invalid Binance response header {BINANCE_RETRY_AFTER_HEADER}={value:?}");
    }
    delay
}

pub(crate) fn retry_config() -> RetryConfig {
    RetryConfig {
        max_elapsed_ms: Some(180_000),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        num::NonZeroU32,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_network::ratelimiter::quota::Quota;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        book::pacing::SnapshotPacer,
        futures::http::{
            client::BinanceFuturesHttpClient, error::BinanceFuturesHttpError,
            query::BinanceDepthParams,
        },
        spot::http::{
            client::BinanceSpotHttpClient, error::BinanceSpotHttpError, query::DepthParams,
        },
    };

    #[rstest]
    #[case::spot(BinanceProductType::Spot)]
    #[case::usd_m(BinanceProductType::UsdM)]
    #[case::coin_m(BinanceProductType::CoinM)]
    #[tokio::test]
    async fn test_depth_retry_reacquires_snapshot_weight(#[case] product: BinanceProductType) {
        let requests = Arc::new(AtomicUsize::new(0));
        let captured = requests.clone();
        let app = Router::new().fallback(get(move || {
            let captured = captured.clone();
            async move {
                if captured.fetch_add(1, Ordering::Relaxed) == 0 {
                    (StatusCode::SERVICE_UNAVAILABLE, "retry").into_response()
                } else {
                    (StatusCode::OK, r#"{"lastUpdateId":42,"bids":[],"asks":[]}"#).into_response()
                }
            }
        }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());

        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let config = RetryConfig {
            max_retries: 1,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            jitter_ms: 0,
            operation_timeout_ms: Some(250),
            ..retry_config()
        };

        let pacer = Arc::new(SnapshotPacer::new(NonZeroU32::new(500).unwrap()));
        let permit = pacer.acquire(250).await;
        let clock = get_atomic_clock_realtime();

        let timed_out = if product == BinanceProductType::Spot {
            let client = BinanceSpotHttpClient::new_with_json_responses(
                BinanceEnvironment::Live,
                clock,
                None,
                None,
                Some(url),
                None,
                None,
                None,
                true,
            )
            .unwrap()
            .with_retry_config(config);

            let result = client
                .inner()
                .depth_paced(
                    &DepthParams {
                        symbol: "BTCUSDT".into(),
                        limit: Some(5000),
                    },
                    permit,
                )
                .await;

            matches!(result, Err(BinanceSpotHttpError::Timeout(message)) if message == "Request exceeded 250ms")
        } else {
            let client = BinanceFuturesHttpClient::new(
                product,
                BinanceEnvironment::Live,
                clock,
                None,
                None,
                Some(url),
                None,
                None,
                None,
                false,
            )
            .unwrap()
            .with_retry_config(config);

            let result = client
                .inner()
                .depth_paced(
                    &BinanceDepthParams {
                        symbol: "BTCUSDT".into(),
                        limit: Some(1000),
                    },
                    permit,
                )
                .await;

            matches!(result, Err(BinanceFuturesHttpError::Timeout(message)) if message == "Request exceeded 250ms")
        };

        server.abort();

        assert_eq!(requests.load(Ordering::Relaxed), 1);
        assert!(timed_out);
    }

    #[rstest]
    #[case("0", 0)]
    #[case("1", 1)]
    #[case("120", 120)]
    fn test_parse_retry_after_delay_seconds(#[case] value: &str, #[case] expected_secs: u64) {
        let now = Timestamp::now();
        assert_eq!(
            parse_retry_after(value, now),
            Some(Duration::from_secs(expected_secs))
        );
    }

    #[rstest]
    fn test_parse_retry_after_http_date() {
        let now: Timestamp = "1994-11-06T08:49:36Z".parse().unwrap();
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT", now),
            Some(Duration::from_secs(1))
        );
    }

    #[rstest]
    fn test_parse_retry_after_http_date_in_past_is_zero() {
        let now: Timestamp = "1994-11-06T08:49:36Z".parse().unwrap();
        assert_eq!(
            parse_retry_after("Sun, 06 Nov 1994 08:49:35 GMT", now),
            Some(Duration::ZERO)
        );
    }

    #[rstest]
    fn test_parse_retry_after_invalid() {
        let now = Timestamp::now();
        assert_eq!(parse_retry_after("not-a-delay", now), None);
    }

    #[rstest]
    fn test_retry_after_missing_header() {
        let headers = HashMap::new();
        assert_eq!(retry_after(&headers, Timestamp::now()), None);
    }

    #[rstest]
    #[case(429)]
    #[case(418)]
    #[tokio::test(start_paused = true)]
    async fn test_backoff_only_extends(#[case] status: u16) {
        let backoff = BinanceHttpBackoff {
            ip: Default::default(),
            orders: Some(Default::default()),
        };

        backoff.observe(status, None, Some(Duration::from_secs(10)));
        tokio::time::advance(Duration::from_secs(2)).await;
        backoff.observe(status, None, Some(Duration::from_secs(1)));
        backoff.observe(200, None, Some(Duration::from_secs(20)));
        backoff.observe(503, None, Some(Duration::from_secs(30)));
        backoff.observe(
            429,
            Some(BINANCE_ORDER_RATE_LIMIT_CODE),
            Some(Duration::from_secs(5)),
        );

        assert_eq!(backoff.remaining(false), Ok(Duration::from_secs(8)));
        assert_eq!(backoff.remaining(true), Ok(Duration::from_secs(8)));

        tokio::time::advance(Duration::from_secs(8)).await;

        assert_eq!(backoff.remaining(false), Ok(Duration::ZERO));
        assert_eq!(backoff.remaining(true), Ok(Duration::ZERO));
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_command_does_not_wait_for_local_quota() {
        let backoff = BinanceHttpBackoff {
            ip: Default::default(),
            orders: None,
        };

        let client = HttpClient::builder()
            .keyed_quotas(vec![(
                "request".to_string(),
                Quota::per_second(NonZeroU32::MIN).unwrap(),
            )])
            .build()
            .unwrap();
        client
            .await_rate_limits(Some(&[Ustr::from("request")]))
            .await;
        let start = Instant::now();
        let blocked = backoff
            .admit(&client, vec!["request".to_string()], false, false)
            .await;
        let elapsed = start.elapsed();
        tokio::time::advance(Duration::from_secs(1)).await;
        let admitted = backoff
            .admit(&client, vec!["request".to_string()], false, false)
            .await;

        assert_eq!(blocked, Err(()));
        assert_eq!(elapsed, Duration::ZERO);
        assert_eq!(admitted, Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn test_backoff_rechecks_after_quota_wait() {
        let backoff = BinanceHttpBackoff {
            ip: Default::default(),
            orders: Some(Default::default()),
        };

        let client = HttpClient::builder()
            .keyed_quotas(vec![(
                "request".to_string(),
                Quota::per_second(NonZeroU32::MIN).unwrap(),
            )])
            .build()
            .unwrap();
        client
            .await_rate_limits(Some(&[Ustr::from("request")]))
            .await;
        let other = backoff.clone();
        let start = Instant::now();

        let waiter = tokio::spawn(async move {
            other
                .admit(&client, vec!["request".to_string()], false, true)
                .await
        });

        tokio::task::yield_now().await;
        backoff.observe(429, None, Some(Duration::from_secs(3)));
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;

        assert!(!waiter.is_finished());
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(waiter.await.unwrap(), Ok(()));
        assert_eq!(start.elapsed(), Duration::from_secs(3));
    }

    #[tokio::test(start_paused = true)]
    async fn test_backoff_rechecks_extended_deadline() {
        let backoff = BinanceHttpBackoff {
            ip: Default::default(),
            orders: None,
        };

        backoff.observe(418, None, Some(Duration::from_secs(3)));
        let other = backoff.clone();
        let client = HttpClient::builder().build().unwrap();
        let start = Instant::now();

        let waiter = tokio::spawn(async move { other.admit(&client, vec![], false, true).await });
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        backoff.observe(418, None, Some(Duration::from_secs(10)));
        tokio::time::advance(Duration::from_secs(2)).await;
        tokio::task::yield_now().await;

        assert!(!waiter.is_finished());

        tokio::time::advance(Duration::from_secs(8)).await;

        assert_eq!(waiter.await.unwrap(), Ok(()));
        assert_eq!(start.elapsed(), Duration::from_secs(11));
    }

    #[rstest]
    #[case::missing_throttle(429, None, Ok(Duration::ZERO))]
    #[case::zero_throttle(429, Some(Duration::ZERO), Ok(Duration::ZERO))]
    #[case::missing_ban(418, None, Err(()))]
    #[case::unrepresentable_ban(418, Some(Duration::MAX), Err(()))]
    #[case::unrepresentable_throttle(429, Some(Duration::MAX), Err(()))]
    #[tokio::test(start_paused = true)]
    async fn test_backoff_missing_or_unrepresentable_expiry(
        #[case] status: u16,
        #[case] delay: Option<Duration>,
        #[case] expected: Result<Duration, ()>,
    ) {
        let backoff = BinanceHttpBackoff {
            ip: Default::default(),
            orders: None,
        };

        backoff.observe(status, None, delay);
        let before = backoff.remaining(false);
        backoff.observe(200, None, None);
        backoff.observe(429, None, Some(Duration::ZERO));

        assert_eq!(before, expected);
        assert_eq!(backoff.remaining(false), expected);
    }

    #[rstest]
    #[case::same_futures(BinanceProductType::UsdM, BinanceEnvironment::Live, "http://backoff-scope.invalid", Some("proxy-a"), Some("key-a"), (true, true))]
    #[case::coin_m(BinanceProductType::CoinM, BinanceEnvironment::Live, "http://backoff-scope.invalid/", Some("proxy-a"), Some("key-a"), (true, true))]
    #[case::spot(BinanceProductType::Spot, BinanceEnvironment::Live, "http://backoff-scope.invalid", Some("proxy-a"), Some("key-a"), (false, false))]
    #[case::testnet(BinanceProductType::UsdM, BinanceEnvironment::Testnet, "http://backoff-scope.invalid", Some("proxy-a"), Some("key-a"), (false, false))]
    #[case::demo(BinanceProductType::UsdM, BinanceEnvironment::Demo, "http://backoff-scope.invalid", Some("proxy-a"), Some("key-a"), (false, false))]
    #[case::endpoint(BinanceProductType::UsdM, BinanceEnvironment::Live, "http://other-backoff-scope.invalid", Some("proxy-a"), Some("key-a"), (false, false))]
    #[case::proxy(BinanceProductType::UsdM, BinanceEnvironment::Live, "http://backoff-scope.invalid", Some("proxy-b"), Some("key-a"), (false, true))]
    #[case::account(BinanceProductType::UsdM, BinanceEnvironment::Live, "http://backoff-scope.invalid", Some("proxy-a"), Some("key-b"), (true, false))]
    #[case::unsigned(BinanceProductType::UsdM, BinanceEnvironment::Live, "http://backoff-scope.invalid", Some("proxy-a"), None, (true, false))]
    fn test_backoff_scope_isolation(
        #[case] product: BinanceProductType,
        #[case] environment: BinanceEnvironment,
        #[case] url: &str,
        #[case] proxy: Option<&str>,
        #[case] key: Option<&str>,
        #[case] expected: (bool, bool),
    ) {
        let credential = SigningCredential::new("key-a".to_string(), "secret-a".to_string());

        let baseline = BinanceHttpBackoff::new(
            BinanceProductType::UsdM,
            BinanceEnvironment::Live,
            Some("http://backoff-scope.invalid"),
            Some("proxy-a"),
            Some(&credential),
        );
        let credential =
            key.map(|key| SigningCredential::new(key.to_string(), "secret-b".to_string()));
        let other =
            BinanceHttpBackoff::new(product, environment, Some(url), proxy, credential.as_ref());

        assert_eq!(Arc::ptr_eq(&baseline.ip, &other.ip), expected.0);
        assert_eq!(
            other
                .orders
                .as_ref()
                .is_some_and(|orders| Arc::ptr_eq(baseline.orders.as_ref().unwrap(), orders)),
            expected.1
        );
    }

    #[tokio::test(start_paused = true)]
    async fn test_backoff_account_limit_preserves_ip_traffic() {
        let backoff = BinanceHttpBackoff {
            ip: Default::default(),
            orders: Some(Default::default()),
        };

        backoff.observe(
            429,
            Some(BINANCE_ORDER_RATE_LIMIT_CODE),
            Some(Duration::from_secs(7)),
        );
        backoff.observe(429, None, Some(Duration::from_secs(2)));

        assert_eq!(backoff.remaining(false), Ok(Duration::from_secs(2)));
        assert_eq!(backoff.remaining(true), Ok(Duration::from_secs(7)));
    }
}
