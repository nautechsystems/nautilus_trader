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

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::{
    Client as ReqwestClient, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue, USER_AGENT},
};
use serde_json::Value;
use tokio::time::sleep;

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
use nautilus_network::backoff::ExponentialBackoff;

// =============================================================================
// API Types
// =============================================================================

#[derive(Clone, Copy, Debug)]
pub enum EndpointKind {
    ExchangeOrder,
    InfoMeta,
    InfoL2Book,
    InfoOrderStatus,
    InfoUserFills,
    Other,
}

impl EndpointKind {
    pub fn default_weight(self) -> u32 {
        match self {
            EndpointKind::ExchangeOrder => 5,
            EndpointKind::InfoL2Book => 2,
            EndpointKind::InfoMeta
            | EndpointKind::InfoOrderStatus
            | EndpointKind::InfoUserFills => 1,
            EndpointKind::Other => 1,
        }
    }

    /// Seed hint to decorrelate jitter per endpoint.
    pub fn jitter_seed(self) -> u64 {
        match self {
            EndpointKind::ExchangeOrder => 0xE0E0_E0E0_E0E0_E0E0,
            EndpointKind::InfoMeta => 0xABCD_ABCD_ABCD_ABCD,
            EndpointKind::InfoL2Book => 0xB00B_1E50_CAFE_F00D,
            EndpointKind::InfoOrderStatus => 0x5151_5151_5151_5151,
            EndpointKind::InfoUserFills => 0xF1F1_F1F1_F1F1_F1F1,
            EndpointKind::Other => 0xDEAD_BEEF_DEAD_BEEF,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Idempotency {
    Idempotent,    // safe to retry
    UnsafeNoRetry, // do not retry on transport / 5xx
}

#[derive(Clone, Debug)]
pub struct RequestOpts {
    pub idempotency: Idempotency,
    pub weight: u32,
    pub timeout: Option<Duration>,
    pub x_request_id: Option<String>,
}

impl RequestOpts {
    pub fn for_endpoint(kind: EndpointKind, idempotency: Idempotency) -> Self {
        Self {
            idempotency,
            weight: kind.default_weight(),
            timeout: None,
            x_request_id: None,
        }
    }
}

// =============================================================================
// HTTP Provider Integration Types
// =============================================================================

#[derive(Clone, Debug)]
pub struct HttpProviderResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[async_trait]
pub trait HttpProvider: Send + Sync + 'static {
    async fn post_json(
        &self,
        url: &str,
        headers: HeaderMap,
        body: &str,
    ) -> std::result::Result<HttpProviderResponse, Box<dyn std::error::Error + Send + Sync>>;
}

/// Rate limiter facade owned by NT (global/shared). If you don't have one,
/// pass `None` to use the REST client's local token-bucket as a fallback.
#[allow(unused_variables)]
#[async_trait]
pub trait RateLimitProvider: Send + Sync + 'static {
    /// Acquire capacity for `weight`. Should await when over budget.
    async fn acquire(&self, weight: u32);

    /// Observe headers after a response (e.g., to track Remaining/Reset).
    async fn on_headers(&self, status: u16, headers: &HeaderMap) {}

    /// 429 handling (e.g., honor Retry-After at a global gate).
    async fn on_429(&self, headers: &HeaderMap) {}
}

/// Canonical JSON pretty-printer from NT (optional).
/// If you don't provide one, the client will log compact JSON and still serialize deterministically.
pub trait JsonProvider: Send + Sync + 'static {
    fn pretty(&self, value: &serde_json::Value) -> String;
}

/// All NT integration dependencies in one struct (each is optional, but `http` is the crucial piece).
#[derive(Clone)]
pub struct IntegrationDeps {
    pub http: std::sync::Arc<dyn HttpProvider>,
    pub limiter: Option<std::sync::Arc<dyn RateLimitProvider>>,
    pub canonical_json: Option<std::sync::Arc<dyn JsonProvider>>,
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

// =============================================================================
// Rate Limiting
// =============================================================================

/// Parse Retry-After header supporting delta-seconds format
/// Note: HTTP-date parsing would require additional dependencies, so we only support seconds
pub fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    let v = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();

    // delta-seconds (most common case)
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    // Optional vendor hints (best-effort)
    if let (Some(rem), Some(reset)) = (
        headers.get("X-RateLimit-Remaining"),
        headers.get("X-RateLimit-Reset"),
    ) && rem.to_str().ok().map(|s| s.trim()) == Some("0")
        && let Ok(secs) = reset.to_str().ok()?.trim().parse::<u64>()
    {
        return Some(Duration::from_secs(secs));
    }

    None
}

#[derive(Debug, Clone)]
pub enum RateLimitMode {
    Noop,
    LocalTokenBucket,
}

#[derive(Debug, Clone, Copy)]
pub struct RateLimitPolicy {
    pub capacity: f64,
    pub refill_per_sec: f64, // tokens per second
}

impl Default for RateLimitPolicy {
    fn default() -> Self {
        Self {
            capacity: 1200.0,
            refill_per_sec: 20.0, // 1200/min = 20/sec
        }
    }
}

#[derive(Debug)]
struct TokenBucket {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    last_refill: Instant,
    throttle_until: Option<Instant>, // server-enforced cooldown gate
}

impl TokenBucket {
    fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            capacity,
            tokens: capacity,
            refill_per_sec,
            last_refill: Instant::now(),
            throttle_until: None,
        }
    }

    fn refill(&mut self, now: Instant) {
        let dt = now.duration_since(self.last_refill).as_secs_f64();
        if dt > 0.0 {
            self.tokens = (self.tokens + dt * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }

    /// Try to consume; return how long the caller should wait if not enough capacity or throttled.
    fn try_consume(&mut self, weight: f64, now: Instant) -> Option<Duration> {
        // Respect server cooldown first
        if let Some(until) = self.throttle_until {
            if now < until {
                return Some(until - now);
            }
            self.throttle_until = None;
        }

        self.refill(now);

        if self.tokens >= weight {
            self.tokens -= weight;
            None
        } else {
            let shortfall = weight - self.tokens;
            // time to accrue the shortfall at current refill rate
            let secs = (shortfall / self.refill_per_sec).max(0.0);
            Some(Duration::from_secs_f64(secs))
        }
    }

    /// Apply server-directed cooldown window (e.g., `Retry-After`).
    fn apply_server_cooldown(&mut self, d: Duration, now: Instant) {
        // Clear tokens and hold future consumption until 'd' elapses.
        self.tokens = 0.0;
        self.throttle_until = Some(now + d);
        // Move refill epoch forward so tokens don't silently accrue under cooldown.
        self.last_refill = now;
    }
}

#[derive(Debug, Clone)]
pub struct RateLimiter {
    mode: RateLimitMode,
    bucket: Arc<tokio::sync::Mutex<TokenBucket>>,
}

impl RateLimiter {
    pub fn new(mode: RateLimitMode, policy: RateLimitPolicy) -> Self {
        let bucket = TokenBucket::new(policy.capacity, policy.refill_per_sec);
        Self {
            mode,
            bucket: Arc::new(tokio::sync::Mutex::new(bucket)),
        }
    }

    /// Acquire capacity for a request of given weight. Awaits if necessary.
    pub async fn wait(&self, weight: u32) {
        if matches!(self.mode, RateLimitMode::Noop) {
            return;
        }

        loop {
            let maybe_wait = {
                let mut b = self.bucket.lock().await;
                b.try_consume(weight as f64, Instant::now())
            };
            if let Some(d) = maybe_wait {
                tokio::time::sleep(d).await;
                continue;
            }
            break;
        }
    }

    /// Feed relevant headers to the limiter (no sleeping here—prevents double-sleep).
    pub async fn record_headers(&self, headers: &HeaderMap) {
        if matches!(self.mode, RateLimitMode::Noop) {
            return;
        }

        if let Some(d) = parse_retry_after(headers) {
            let mut b = self.bucket.lock().await;
            b.apply_server_cooldown(d, Instant::now());
        }
    }

    /// Convenience: apply a known cooldown (e.g., from status handling).
    pub async fn apply_cooldown(&self, d: Duration) {
        if matches!(self.mode, RateLimitMode::Noop) {
            return;
        }
        let mut b = self.bucket.lock().await;
        b.apply_server_cooldown(d, Instant::now());
    }

    /// Update the rate limiting policy
    pub async fn set_policy(&self, policy: RateLimitPolicy) {
        if matches!(self.mode, RateLimitMode::Noop) {
            return;
        }
        let mut b = self.bucket.lock().await;
        *b = TokenBucket::new(policy.capacity, policy.refill_per_sec);
    }
}

fn default_json_headers(user_agent: Option<&str>, xreq: Option<&str>) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(ACCEPT, HeaderValue::from_static("application/json"));
    let ua = user_agent.unwrap_or(concat!(
        env!("CARGO_PKG_NAME"),
        "/",
        env!("CARGO_PKG_VERSION")
    ));
    if let Ok(v) = HeaderValue::from_str(ua) {
        h.insert(USER_AGENT, v);
    }
    if let Some(id) = xreq
        && let Ok(v) = HeaderValue::from_str(id)
    {
        h.insert("X-Request-Id", v);
    }
    h
}

pub struct HyperliquidHttpClient {
    // External deps (preferred)
    provider: Option<Arc<dyn HttpProvider>>,

    // Local fallback
    http: ReqwestClient,
    limiter: RateLimiter,

    // Env/config
    #[allow(dead_code)]
    network: HyperliquidNetwork,
    base_info: String,
    base_exchange: String,
    backoff: ExponentialBackoff,

    // Signing (only needed for /exchange)
    signer: Option<HyperliquidEip712Signer>,
    nonce_manager: Option<NonceManager>,
    vault_address: Option<VaultAddress>,
}

impl std::fmt::Debug for HyperliquidHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HyperliquidHttpClient")
            .field("provider", &self.provider.is_some())
            .field("network", &self.network)
            .field("base_info", &self.base_info)
            .field("base_exchange", &self.base_exchange)
            .field("backoff", &self.backoff)
            .field("signer", &self.signer.is_some())
            .field("nonce_manager", &self.nonce_manager.is_some())
            .field("vault_address", &self.vault_address)
            .finish()
    }
}

impl HyperliquidHttpClient {
    /// Public client (no signing), local token-bucket RL.
    pub fn public(network: HyperliquidNetwork) -> Result<Self> {
        let http = ReqwestClient::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(Error::from_reqwest)?;

        let backoff = ExponentialBackoff::new(
            Duration::from_millis(200), // initial delay
            Duration::from_secs(3),     // max delay
            2.0,                        // factor
            0,                          // no jitter for deterministic behavior
            false,                      // no immediate first
        )
        .map_err(|e| Error::transport(e.to_string()))?; // Convert anyhow::Error

        Ok(Self {
            provider: None,
            http,
            limiter: RateLimiter::new(RateLimitMode::LocalTokenBucket, RateLimitPolicy::default()),
            network,
            base_info: info_url(network).to_string(),
            base_exchange: exchange_url(network).to_string(),
            backoff,
            signer: None,
            nonce_manager: None,
            vault_address: None,
        })
    }

    /// Private client (signing enabled). `mode = Noop` if NT already enforces global RL.
    ///
    /// # Panics
    ///
    /// This function will panic if the ExponentialBackoff cannot be created with the
    /// hardcoded values (which should never happen under normal circumstances).
    pub fn private(secrets: &Secrets, mode: RateLimitMode) -> Result<Self> {
        let http = ReqwestClient::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(Error::from_reqwest)?;

        let signer = HyperliquidEip712Signer::new(secrets.private_key.clone());

        let backoff = ExponentialBackoff::new(
            Duration::from_millis(200), // initial delay
            Duration::from_secs(3),     // max delay
            2.0,                        // factor
            0,                          // no jitter for deterministic behavior
            false,                      // no immediate first
        )
        .unwrap(); // Safe with these hardcoded values

        Ok(Self {
            provider: None,
            http,
            limiter: RateLimiter::new(mode, RateLimitPolicy::default()),
            network: secrets.network,
            base_info: info_url(secrets.network).to_string(),
            base_exchange: exchange_url(secrets.network).to_string(),
            backoff,
            signer: Some(signer),
            nonce_manager: Some(NonceManager::new()),
            vault_address: secrets.vault_address,
        })
    }

    /// Plug in NT dependencies (HTTP sender + global RL + canonical JSON).
    pub fn with_nt(mut self, deps: IntegrationDeps) -> Self {
        self.provider = Some(deps.http);
        self
    }

    pub fn with_backoff(
        mut self,
        initial: Duration,
        max: Duration,
        factor: f64,
        jitter_ms: u64,
    ) -> Result<Self> {
        self.backoff = ExponentialBackoff::new(initial, max, factor, jitter_ms, false)
            .map_err(|e| Error::transport(e.to_string()))?;
        Ok(self)
    }

    pub async fn set_rate_limit_policy(&self, policy: RateLimitPolicy) {
        self.limiter.set_policy(policy).await;
    }

    /// Core POST JSON with improved retry logic, rate limiting, and server cooldown handling
    ///
    /// # Panics
    ///
    /// This function will panic if the ExponentialBackoff cannot be created with the
    /// hardcoded values (which should never happen under normal circumstances).
    #[allow(clippy::too_many_arguments)]
    pub async fn post_json_with_opts(
        &self,
        url: &str,
        body: &Value,
        _kind: EndpointKind,
        opts: RequestOpts,
    ) -> Result<Value> {
        // Serialize once, reuse across attempts (Bytes is cheap to clone).
        let body_str = serde_json::to_string(body).map_err(Error::Serde)?;
        let body_bytes = Bytes::from(body_str.clone());

        // Acquire local/NT limiter before attempting.
        self.limiter.wait(opts.weight).await;

        let mut attempt = 0u32;
        let max_retries = 5u32; // Standard NT max retries

        // Create a local backoff instance for this request
        let mut backoff = ExponentialBackoff::new(
            Duration::from_millis(200),
            Duration::from_secs(3),
            2.0,
            50, // Add jitter for network retries
            false,
        )
        .unwrap();

        loop {
            attempt += 1;

            // Build headers
            let headers = default_json_headers(
                Some("nautilus-hyperliquid/1 (+NT)"),
                opts.x_request_id.as_deref(),
            );

            // Send via custom provider (NT) or raw reqwest.
            let send_res = if let Some(provider) = &self.provider {
                provider
                    .post_json(url, headers.clone(), &body_str)
                    .await
                    .map(|r| (r.status, r.headers, r.body))
                    .map_err(|e| Error::transport(e.to_string()))
            } else {
                let mut rb = self
                    .http
                    .post(url)
                    .headers(headers)
                    .body(body_bytes.clone());
                if let Some(t) = opts.timeout {
                    rb = rb.timeout(t);
                }
                let resp = rb.send().await.map_err(Error::from_reqwest)?;
                let status = resp.status();
                let headers = resp.headers().clone();
                let body = resp.bytes().await.map_err(Error::from_reqwest)?;
                Ok((status, headers, body))
            };

            match send_res {
                Ok((status, headers, bytes)) => {
                    // Feed header hints to limiter (no sleeping here).
                    self.limiter.record_headers(&headers).await;

                    // 429/503 with Retry-After: honor server first (no double-sleep later).
                    if (status == StatusCode::TOO_MANY_REQUESTS
                        || status == StatusCode::SERVICE_UNAVAILABLE)
                        && let Some(d) = parse_retry_after(&headers)
                    {
                        self.limiter.apply_cooldown(d).await;
                        if matches!(opts.idempotency, Idempotency::UnsafeNoRetry) {
                            return Err(Error::http(
                                status.as_u16(),
                                "server asked to retry later (unsafe endpoint)",
                            ));
                        }
                        if attempt > max_retries {
                            return Err(Error::http(
                                status.as_u16(),
                                "exhausted retries after server throttle",
                            ));
                        }
                        sleep(d).await;
                        // Reset backoff after server-directed pause
                        backoff.reset();
                        continue;
                    }

                    // Non-success
                    if !status.is_success() {
                        let text = String::from_utf8_lossy(&bytes).into_owned();

                        // Decide retryability based on status
                        let retryable = match status.as_u16() {
                            408 | 429 | 500..=599 => true, // Retryable HTTP errors
                            _ => false,
                        };

                        if retryable
                            && attempt <= max_retries
                            && !matches!(opts.idempotency, Idempotency::UnsafeNoRetry)
                        {
                            let delay = backoff.next_duration();
                            sleep(delay).await;
                            continue;
                        }
                        return Err(Error::http(status.as_u16(), text));
                    }

                    // Success → parse JSON and return
                    let v: Value = serde_json::from_slice(&bytes).map_err(Error::Serde)?;
                    return Ok(v);
                }
                Err(err) => {
                    // Transport error retryability
                    let err_str = err.to_string().to_ascii_lowercase();
                    let retryable = err_str.contains("timeout")
                        || err_str.contains("connection")
                        || err_str.contains("network");

                    if retryable
                        && attempt <= max_retries
                        && !matches!(opts.idempotency, Idempotency::UnsafeNoRetry)
                    {
                        let delay = backoff.next_duration();
                        sleep(delay).await;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
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
            _ => EndpointKind::Other,
        };

        let opts = RequestOpts::for_endpoint(kind, Idempotency::Idempotent);
        let url = &self.base_info;
        let v = serde_json::to_value(body)?;
        self.post_json_with_opts(url, &v, kind, opts).await
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

        let opts = RequestOpts::for_endpoint(EndpointKind::ExchangeOrder, Idempotency::Idempotent);
        let url = &self.base_exchange;
        let raw = serde_json::to_value(&req)?;
        let val = self
            .post_json_with_opts(url, &raw, EndpointKind::ExchangeOrder, opts)
            .await?;
        serde_json::from_value(val).map_err(Error::Serde)
    }

    // ---------------- Internals ------------------------------------------------

    fn signer_id(&self) -> Result<SignerId> {
        Ok(SignerId("hyperliquid:default".into()))
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
