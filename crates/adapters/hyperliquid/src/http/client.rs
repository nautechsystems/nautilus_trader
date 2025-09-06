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

//! Hyperliquid HTTP client — NT-aware
//!  * Reuses NT HTTP sender + global RL when provided
//!  * Falls back to reqwest + local token-bucket otherwise
//!  * Idempotent /exchange (same nonce+signature on retries)
//!  * Deterministic JSON (stable field order from builders)

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use anyhow::Context;
use bytes::Bytes;
// Additional imports for rate limiting
use reqwest::header::RETRY_AFTER;
use reqwest::{
    Client as ReqwestClient, StatusCode,
    header::{CONTENT_TYPE, HeaderMap},
};
use serde_json::Value;
use tokio::time::sleep;
use tracing::debug;

use crate::{
    common::{
        consts::{HTTP_TIMEOUT, HyperliquidNetwork, exchange_url, info_url},
        credential::{Secrets, VaultAddress},
    },
    http::{
        error::{Error, Result},
        models::{
            HyperliquidExchangeRequest, HyperliquidExchangeResponse, HyperliquidFills,
            HyperliquidL2Book, HyperliquidMeta, HyperliquidOrderStatus,
        },
        query::{ExchangeAction, InfoRequest},
    },
    signing::{
        HyperliquidActionType, HyperliquidEip712Signer, NonceManager, SignRequest, types::SignerId,
    },
};

// -------------------------------------------------------------------------------------------------
// Rate Limiting Types and Implementation
// -------------------------------------------------------------------------------------------------

/// Per-endpoint logical kind used to look up weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EndpointKind {
    InfoMeta,
    InfoL2Book,
    InfoUserFills,
    InfoOrderStatus,
    ExchangeOrder,
    ExchangeCancel,
    ExchangeModify,
    Unknown,
}

impl EndpointKind {
    pub fn default_weight(self) -> u32 {
        match self {
            EndpointKind::InfoMeta => 1,
            EndpointKind::InfoL2Book => 1,
            EndpointKind::InfoUserFills => 2,
            EndpointKind::InfoOrderStatus => 1,
            EndpointKind::ExchangeOrder => 5,
            EndpointKind::ExchangeCancel => 2,
            EndpointKind::ExchangeModify => 2,
            EndpointKind::Unknown => 1,
        }
    }
}

/// Strategy for rate limiting.
#[derive(Debug, Clone)]
pub enum RateLimitMode {
    LocalTokenBucket,
    Noop, // Use when NT provides global limiter
}

/// Policy/config for the limiter.
#[derive(Debug, Clone)]
pub struct RateLimitPolicy {
    pub capacity: u32,
    pub refill_per_min: u32,
}

impl Default for RateLimitPolicy {
    fn default() -> Self {
        Self {
            capacity: 1200,
            refill_per_min: 1200,
        }
    }
}

/// Simple token-bucket fallback.
#[derive(Debug)]
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last_refill: std::time::Instant,
}

impl TokenBucket {
    fn new(capacity: u32, refill_per_min: u32) -> Self {
        let capacity = capacity as f64;
        let refill_per_sec = (refill_per_min as f64) / 60.0;
        Self {
            capacity,
            tokens: capacity,
            refill_per_sec,
            last_refill: std::time::Instant::now(),
        }
    }

    fn refill(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }

    fn try_consume(&mut self, weight: f64) -> Option<Duration> {
        self.refill();
        if self.tokens >= weight {
            self.tokens -= weight;
            None
        } else {
            let shortfall = weight - self.tokens;
            let secs = shortfall / self.refill_per_sec;
            Some(Duration::from_secs_f64(secs.max(0.0)))
        }
    }

    fn sleep_for(&mut self, _duration: Duration) {
        self.tokens = 0.0;
    }
}

/// Local rate limiter (used when NT deps not provided).
#[derive(Debug, Clone)]
pub struct RateLimiter {
    mode: RateLimitMode,
    bucket: Arc<tokio::sync::Mutex<TokenBucket>>,
}

impl RateLimiter {
    pub fn new(mode: RateLimitMode, policy: RateLimitPolicy) -> Self {
        let bucket = TokenBucket::new(policy.capacity, policy.refill_per_min);
        Self {
            mode,
            bucket: Arc::new(tokio::sync::Mutex::new(bucket)),
        }
    }

    pub async fn wait(&self, weight: u32) {
        if matches!(self.mode, RateLimitMode::Noop) {
            return;
        }
        let w = weight as f64;
        loop {
            let delay_opt = {
                let mut b = self.bucket.lock().await;
                b.try_consume(w)
            };
            if let Some(delay) = delay_opt {
                sleep(delay).await;
                continue;
            }
            break;
        }
    }

    pub async fn record_headers(&self, headers: &HeaderMap) {
        if matches!(self.mode, RateLimitMode::Noop) {
            return;
        }

        // Basic Retry-After handling
        if let Some(retry_after_value) = headers.get(RETRY_AFTER)
            && let Ok(s) = retry_after_value.to_str()
            && let Ok(secs) = s.trim().parse::<u64>()
        {
            {
                let mut b = self.bucket.lock().await;
                b.sleep_for(Duration::from_secs(secs));
            }
            sleep(Duration::from_secs(secs)).await;
        }
    }

    pub fn weight_for(&self, kind: EndpointKind) -> u32 {
        kind.default_weight()
    }

    pub async fn set_policy(&self, policy: RateLimitPolicy) {
        let mut b = self.bucket.lock().await;
        *b = TokenBucket::new(policy.capacity, policy.refill_per_min);
    }
}

/// Minimal normalized HTTP response for the adapter.
#[derive(Clone, Debug)]
pub struct HttpProviderResponse {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: Bytes,
}

/// Type alias for the complex return type of HttpProvider::post_json
type HttpProviderResult<'a> = Pin<
    Box<
        dyn Future<
                Output = std::result::Result<
                    HttpProviderResponse,
                    Box<dyn std::error::Error + Send + Sync>,
                >,
            > + Send
            + 'a,
    >,
>;

/// Async HTTP POST JSON sender.
/// Implement this by delegating to your NT HTTP client (middlewares, tracing, proxies, etc).
pub trait HttpProvider: Send + Sync + 'static {
    fn post_json<'a>(
        &'a self,
        url: &'a str,
        headers: HeaderMap,
        body: &'a str,
    ) -> HttpProviderResult<'a>;
}

/// Rate limiter facade owned by NT (global/shared). If you don't have one,
/// pass `None` to use the REST client's local token-bucket as a fallback.
#[allow(unused_variables)]
pub trait RateLimitProvider: Send + Sync + 'static {
    /// Acquire capacity for `weight`. Should await when over budget.
    fn acquire<'a>(&'a self, weight: u32) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

    /// Observe headers after a response (e.g., to track Remaining/Reset).
    fn on_headers(&self, status: u16, headers: &HeaderMap) {}

    /// 429 handling (e.g., honor Retry-After at a global gate).
    fn on_429(&self, headers: &HeaderMap) {}
}

/// Canonical JSON pretty-printer from NT (optional).
/// If you don't provide one, the client will log compact JSON and still serialize deterministically.
pub trait JsonProvider: Send + Sync + 'static {
    fn pretty(&self, value: &serde_json::Value) -> String;
}

/// All NT integration dependencies in one struct (each is optional, but `http` is the crucial piece).
#[derive(Clone)]
pub struct IntegrationDeps {
    pub http: Arc<dyn HttpProvider>,
    pub limiter: Option<Arc<dyn RateLimitProvider>>,
    pub canonical_json: Option<Arc<dyn JsonProvider>>,
}

impl std::fmt::Debug for IntegrationDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntegrationDeps")
            .field("http", &"<dyn HttpProvider>")
            .field(
                "limiter",
                &self.limiter.as_ref().map(|_| "<dyn RateLimitProvider>"),
            )
            .field(
                "canonical_json",
                &self.canonical_json.as_ref().map(|_| "<dyn JsonProvider>"),
            )
            .finish()
    }
}

// -------------------------------------------------------------------------------------------------
// Retry/backoff
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_secs(3),
            jitter: true,
        }
    }
}

fn next_backoff(p: &RetryPolicy, attempt: u32) -> Duration {
    let exp = 1u32.checked_shl(attempt.min(8)).unwrap_or(256);
    let mut d = p.base_delay.saturating_mul(exp);
    if d > p.max_delay {
        d = p.max_delay;
    }
    if !p.jitter {
        return d;
    }
    // ±25% jitter (deterministic per attempt)
    let nanos = d.as_nanos() as i128;
    let jitter = nanos / 4;
    let mix = ((attempt as i128).wrapping_mul(6364136223846793005i128)) & (2 * jitter);
    let adj = (nanos - jitter + mix).max(0) as u128;
    Duration::from_nanos(adj.min(u128::from(u64::MAX)) as u64)
}

// -------------------------------------------------------------------------------------------------
// Idempotency toggle (unsafe ops)
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Idempotency {
    NoRetry,
    RetryByNonce,
}

// -------------------------------------------------------------------------------------------------
// Client
// -------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct HyperliquidHttpClient {
    // External deps (preferred)
    nt: Option<IntegrationDeps>,

    // Local fallback
    http: ReqwestClient,
    limiter: RateLimiter,

    // Env/config
    #[allow(dead_code)]
    network: HyperliquidNetwork,
    base_info: String,
    base_exchange: String,
    retry: RetryPolicy,

    // Signing (only needed for /exchange)
    signer: Option<HyperliquidEip712Signer>,
    nonce_manager: Option<NonceManager>,
    vault_address: Option<VaultAddress>,
}

impl HyperliquidHttpClient {
    // ---------------- Builders -------------------------------------------------

    /// Public client (no signing), local token-bucket RL.
    pub fn public(network: HyperliquidNetwork) -> Result<Self> {
        let http = ReqwestClient::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(Error::from_reqwest)?;

        Ok(Self {
            nt: None,
            http,
            limiter: RateLimiter::new(RateLimitMode::LocalTokenBucket, RateLimitPolicy::default()),
            network,
            base_info: info_url(network).to_string(),
            base_exchange: exchange_url(network).to_string(),
            retry: RetryPolicy::default(),
            signer: None,
            nonce_manager: None,
            vault_address: None,
        })
    }

    /// Private client (signing enabled). `mode = Noop` if NT already enforces global RL.
    pub fn private(secrets: &Secrets, mode: RateLimitMode) -> Result<Self> {
        let http = ReqwestClient::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(Error::from_reqwest)?;

        let signer = HyperliquidEip712Signer::new(secrets.private_key.clone());

        Ok(Self {
            nt: None,
            http,
            limiter: RateLimiter::new(mode, RateLimitPolicy::default()),
            network: secrets.network,
            base_info: info_url(secrets.network).to_string(),
            base_exchange: exchange_url(secrets.network).to_string(),
            retry: RetryPolicy::default(),
            signer: Some(signer),
            nonce_manager: Some(NonceManager::new()),
            vault_address: secrets.vault_address,
        })
    }

    /// Plug in NT dependencies (HTTP sender + global RL + canonical JSON).
    pub fn with_nt(mut self, deps: IntegrationDeps) -> Self {
        self.nt = Some(deps);
        self
    }

    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub async fn set_rate_limit_policy(&self, policy: RateLimitPolicy) {
        self.limiter.set_policy(policy).await;
    }

    // ---------------- INFO -----------------------------------------------------

    pub async fn info_meta(&self) -> Result<HyperliquidMeta> {
        let v = self.info_raw(&InfoRequest::meta()).await?;
        serde_json::from_value(v).map_err(Error::Serde)
    }

    pub async fn info_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book> {
        let v = self.info_raw(&InfoRequest::l2_book(coin)).await?;
        serde_json::from_value(v).map_err(Error::Serde)
    }

    pub async fn info_user_fills(&self, user: &str) -> Result<HyperliquidFills> {
        let v = self.info_raw(&InfoRequest::user_fills(user)).await?;
        serde_json::from_value(v).map_err(Error::Serde)
    }

    pub async fn info_order_status(&self, user: &str, oid: u64) -> Result<HyperliquidOrderStatus> {
        let v = self.info_raw(&InfoRequest::order_status(user, oid)).await?;
        serde_json::from_value(v).map_err(Error::Serde)
    }

    /// Core /info POST returning raw JSON.
    pub async fn info_raw(&self, body: &InfoRequest) -> Result<Value> {
        let kind = match body.request_type.as_str() {
            "meta" => EndpointKind::InfoMeta,
            "l2Book" => EndpointKind::InfoL2Book,
            "userFills" => EndpointKind::InfoUserFills,
            "orderStatus" => EndpointKind::InfoOrderStatus,
            _ => EndpointKind::Unknown,
        };
        let weight = self.limiter.weight_for(kind);
        self.acquire(weight).await;

        let url = &self.base_info;
        let v = serde_json::to_value(body)?;
        self.post_json(url, &v, Idempotency::NoRetry).await
    }

    // ---------------- EXCHANGE -------------------------------------------------

    /// Signed /exchange with idempotent retries (same nonce+signature bytes).
    pub async fn post_action(
        &self,
        action: &ExchangeAction,
    ) -> Result<HyperliquidExchangeResponse> {
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| Error::auth("private client required"))?;
        let nonces = self
            .nonce_manager
            .as_ref()
            .ok_or_else(|| Error::auth("nonce manager missing"))?;

        let time_nonce = nonces.next(self.signer_id()?)?;
        nonces.validate_local(self.signer_id()?, time_nonce)?;

        let action_value = serde_json::to_value(action)
            .context("serialize exchange action")
            .map_err(|e| Error::bad_request(e.to_string()))?;

        let sig = signer
            .sign(&SignRequest {
                action: action_value.clone(),
                time_nonce,
                action_type: HyperliquidActionType::UserSigned,
            })?
            .signature;

        let req = if let Some(vault) = self.vault_address {
            HyperliquidExchangeRequest::with_vault(
                action.clone(),
                time_nonce.as_millis() as u64,
                sig,
                vault.to_string(),
            )
        } else {
            HyperliquidExchangeRequest::new(action.clone(), time_nonce.as_millis() as u64, sig)
        };

        self.acquire(self.limiter.weight_for(EndpointKind::ExchangeOrder))
            .await;

        let url = &self.base_exchange;
        let raw = serde_json::to_value(&req)?;
        let val = self.post_json(url, &raw, Idempotency::RetryByNonce).await?;
        serde_json::from_value(val).map_err(Error::Serde)
    }

    // ---------------- Internals ------------------------------------------------

    fn signer_id(&self) -> Result<SignerId> {
        Ok(SignerId("hyperliquid:default".into()))
    }

    async fn acquire(&self, weight: u32) {
        if let Some(nt) = &self.nt
            && let Some(lim) = &nt.limiter
        {
            lim.acquire(weight).await;
            return;
        }
        self.limiter.wait(weight).await;
    }

    async fn post_json(&self, url: &str, body: &Value, idemp: Idempotency) -> Result<Value> {
        // Deterministic body string
        let body_str = serde_json::to_string(body).map_err(Error::Serde)?;

        // Optional: pretty print via NT canonicalizer for logs
        if let Some(nt) = &self.nt
            && let Some(pretty) = &nt.canonical_json
        {
            debug!(target: "hl.http", "request (canonical):\n{}", pretty.pretty(body));
        }

        let mut attempt = 0u32;
        loop {
            attempt += 1;

            // Prepare headers
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

            // ---- Send via NT if present ----
            let send_res: Result<(u16, HeaderMap, Bytes)> = if let Some(nt) = &self.nt {
                let resp = (nt.http)
                    .post_json(url, headers.clone(), &body_str)
                    .await
                    .map_err(|e| Error::transport(e.to_string()))?;
                Ok((resp.status, resp.headers, resp.body))
            } else {
                // ---- Fallback: reqwest ----
                let rsp = self
                    .http
                    .post(url)
                    .headers(headers)
                    .body(body_str.clone())
                    .send()
                    .await
                    .map_err(Error::from_reqwest)?;

                let status = rsp.status().as_u16();
                let hdrs = rsp.headers().clone();
                let bytes = rsp.bytes().await.map_err(Error::from_reqwest)?;
                Ok((status, hdrs, bytes))
            };

            match send_res {
                Ok((status, hdrs, bytes)) => {
                    // Feed headers to NT/global limiter, else to local fallback
                    if let Some(nt) = &self.nt {
                        if let Some(lim) = &nt.limiter {
                            lim.on_headers(status, &hdrs);
                        }
                    } else {
                        self.limiter.record_headers(&hdrs).await;
                    }

                    if status == StatusCode::TOO_MANY_REQUESTS.as_u16() {
                        if let Some(nt) = &self.nt
                            && let Some(lim) = &nt.limiter
                        {
                            lim.on_429(&hdrs);
                        }
                        if attempt > self.retry.max_retries {
                            return Err(Error::http(429, "too many requests"));
                        }
                        let d = next_backoff(&self.retry, attempt - 1);
                        sleep(d).await;
                        continue;
                    }

                    if !(200..=299).contains(&status) {
                        // Map to typed error and decide retry
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        let err = Error::http(status, text);
                        if Error::is_retryable(&err) && attempt <= self.retry.max_retries {
                            let d = next_backoff(&self.retry, attempt - 1);
                            sleep(d).await;
                            continue;
                        }
                        return Err(err);
                    }

                    // Success
                    let v: Value = serde_json::from_slice(&bytes).map_err(Error::Serde)?;
                    return Ok(v);
                }
                Err(err) => {
                    if err.is_retryable() && attempt <= self.retry.max_retries {
                        // Don't retry unsafe /exchange if we weren't idempotent
                        if matches!(idemp, Idempotency::NoRetry) && url == self.base_exchange {
                            return Err(err);
                        }
                        let d = next_backoff(&self.retry, attempt - 1);
                        sleep(d).await;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::http::query::InfoRequest;

    #[test]
    fn stable_json_roundtrips() {
        let v = serde_json::json!({"type":"l2Book","coin":"BTC"});
        let s = serde_json::to_string(&v).unwrap();
        assert_eq!(s, r#"{"type":"l2Book","coin":"BTC"}"#);
    }

    #[test]
    fn info_pretty_shape() {
        let r = InfoRequest::l2_book("BTC");
        let val = serde_json::to_value(&r).unwrap();
        let pretty = serde_json::to_string_pretty(&val).unwrap();
        assert!(pretty.contains("\"type\": \"l2Book\""));
        assert!(pretty.contains("\"coin\": \"BTC\""));
    }
}
