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

//! Provides the HTTP client integration for the [Hyperliquid](https://hyperliquid.xyz/) REST API.
//!
//! This module defines and implements a [`HyperliquidHttpClient`] for sending requests to various
//! Hyperliquid endpoints. It handles request signing (when credentials are provided), constructs
//! valid HTTP requests using the [`HttpClient`], and parses the responses back into structured
//! data or an [`Error`].

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock},
    time::Duration,
};

use anyhow::Context;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::{Method, header::USER_AGENT};
use serde_json::Value;
use tokio::time::sleep;

use crate::{
    common::{
        consts::{exchange_url, info_url},
        credential::{Secrets, VaultAddress},
    },
    http::{
        error::{Error, Result},
        models::{
            HyperliquidExchangeRequest, HyperliquidExchangeResponse, HyperliquidFills,
            HyperliquidL2Book, HyperliquidMeta, HyperliquidOrderStatus, PerpMeta, PerpMetaAndCtxs,
            SpotMeta, SpotMetaAndCtxs,
        },
        parse::{
            HyperliquidInstrumentDef, instruments_from_defs_owned, parse_perp_instruments,
            parse_spot_instruments,
        },
        query::{ExchangeAction, InfoRequest},
        rate_limits::{
            RateLimitSnapshot, WeightedLimiter, backoff_full_jitter, exchange_weight,
            info_base_weight, info_extra_weight,
        },
    },
    signing::{
        HyperliquidActionType, HyperliquidEip712Signer, NonceManager, SignRequest, types::SignerId,
    },
};

// https://hyperliquid.xyz/docs/api#rate-limits
pub static HYPERLIQUID_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_minute(NonZeroU32::new(1200).unwrap()));

/// Provides a lower-level HTTP client for connecting to the [Hyperliquid](https://hyperliquid.xyz/) REST API.
///
/// This client wraps the underlying `HttpClient` to handle functionality
/// specific to Hyperliquid, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct HyperliquidHttpClient {
    client: HttpClient,
    is_testnet: bool,
    base_info: String,
    base_exchange: String,
    signer: Option<HyperliquidEip712Signer>,
    nonce_manager: Option<Arc<NonceManager>>,
    vault_address: Option<VaultAddress>,
    rest_limiter: Arc<WeightedLimiter>,
    rate_limit_backoff_base: Duration,
    rate_limit_backoff_cap: Duration,
    rate_limit_max_attempts_info: u32,
}

impl Default for HyperliquidHttpClient {
    fn default() -> Self {
        Self::new(true, None) // Default to testnet
    }
}

impl HyperliquidHttpClient {
    /// Creates a new [`HyperliquidHttpClient`] using the default Hyperliquid HTTP URL,
    /// optionally overridden with a custom timeout.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    #[must_use]
    pub fn new(is_testnet: bool, timeout_secs: Option<u64>) -> Self {
        Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*HYPERLIQUID_REST_QUOTA),
                timeout_secs,
            ),
            is_testnet,
            base_info: info_url(is_testnet).to_string(),
            base_exchange: exchange_url(is_testnet).to_string(),
            signer: None,
            nonce_manager: None,
            vault_address: None,
            rest_limiter: Arc::new(WeightedLimiter::per_minute(1200)),
            rate_limit_backoff_base: Duration::from_millis(125),
            rate_limit_backoff_cap: Duration::from_secs(5),
            rate_limit_max_attempts_info: 3,
        }
    }

    /// Creates a new [`HyperliquidHttpClient`] configured with credentials
    /// for authenticated requests.
    #[must_use]
    pub fn with_credentials(secrets: &Secrets, timeout_secs: Option<u64>) -> Self {
        let signer = HyperliquidEip712Signer::new(secrets.private_key.clone());
        let nonce_manager = Arc::new(NonceManager::new());

        Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*HYPERLIQUID_REST_QUOTA),
                timeout_secs,
            ),
            is_testnet: secrets.is_testnet,
            base_info: info_url(secrets.is_testnet).to_string(),
            base_exchange: exchange_url(secrets.is_testnet).to_string(),
            signer: Some(signer),
            nonce_manager: Some(nonce_manager),
            vault_address: secrets.vault_address,
            rest_limiter: Arc::new(WeightedLimiter::per_minute(1200)),
            rate_limit_backoff_base: Duration::from_millis(125),
            rate_limit_backoff_cap: Duration::from_secs(5),
            rate_limit_max_attempts_info: 3,
        }
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables
    /// are not set.
    pub fn from_env() -> Result<Self> {
        let secrets =
            Secrets::from_env().map_err(|_| Error::auth("missing credentials in environment"))?;
        Ok(Self::with_credentials(&secrets, None))
    }

    /// Configure rate limiting parameters (chainable).
    pub fn with_rate_limits(mut self) -> Self {
        self.rest_limiter = Arc::new(WeightedLimiter::per_minute(1200));
        self.rate_limit_backoff_base = Duration::from_millis(125);
        self.rate_limit_backoff_cap = Duration::from_secs(5);
        self.rate_limit_max_attempts_info = 3;
        self
    }

    /// Returns whether this client is configured for testnet.
    #[must_use]
    pub fn is_testnet(&self) -> bool {
        self.is_testnet
    }

    /// Gets the user address derived from the private key (if client has credentials).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if the client has no signer configured.
    pub fn get_user_address(&self) -> Result<String> {
        self.signer
            .as_ref()
            .ok_or_else(|| Error::auth("No signer configured"))?
            .address()
    }

    /// Builds the default headers to include with each request (e.g., `User-Agent`).
    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ])
    }

    // ---------------- INFO ENDPOINTS --------------------------------------------

    /// Get metadata about available markets.
    pub async fn info_meta(&self) -> Result<HyperliquidMeta> {
        let request = InfoRequest::meta();
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get complete spot metadata (tokens and pairs).
    pub async fn get_spot_meta(&self) -> Result<SpotMeta> {
        let request = InfoRequest::spot_meta();
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get perpetuals metadata with asset contexts (for price precision refinement).
    pub async fn get_perp_meta_and_ctxs(&self) -> Result<PerpMetaAndCtxs> {
        let request = InfoRequest::meta_and_asset_ctxs();
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get spot metadata with asset contexts (for price precision refinement).
    pub async fn get_spot_meta_and_ctxs(&self) -> Result<SpotMetaAndCtxs> {
        let request = InfoRequest::spot_meta_and_asset_ctxs();
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Fetch and parse all available instrument definitions from Hyperliquid.
    pub async fn request_instruments(&self) -> Result<Vec<InstrumentAny>> {
        let mut defs: Vec<HyperliquidInstrumentDef> = Vec::new();

        match self.load_perp_meta().await {
            Ok(perp_meta) => match parse_perp_instruments(&perp_meta) {
                Ok(perp_defs) => {
                    tracing::debug!(
                        count = perp_defs.len(),
                        "Loaded Hyperliquid perp definitions"
                    );
                    defs.extend(perp_defs);
                }
                Err(err) => {
                    tracing::warn!(%err, "Failed to parse Hyperliquid perp instruments");
                }
            },
            Err(err) => {
                tracing::warn!(%err, "Failed to load Hyperliquid perp metadata");
            }
        }

        match self.get_spot_meta().await {
            Ok(spot_meta) => match parse_spot_instruments(&spot_meta) {
                Ok(spot_defs) => {
                    tracing::debug!(
                        count = spot_defs.len(),
                        "Loaded Hyperliquid spot definitions"
                    );
                    defs.extend(spot_defs);
                }
                Err(err) => {
                    tracing::warn!(%err, "Failed to parse Hyperliquid spot instruments");
                }
            },
            Err(err) => {
                tracing::warn!(%err, "Failed to load Hyperliquid spot metadata");
            }
        }

        Ok(instruments_from_defs_owned(defs))
    }

    pub(crate) async fn load_perp_meta(&self) -> Result<PerpMeta> {
        let request = InfoRequest::meta();
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get L2 order book for a coin.
    pub async fn info_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book> {
        let request = InfoRequest::l2_book(coin);
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get user fills (trading history).
    pub async fn info_user_fills(&self, user: &str) -> Result<HyperliquidFills> {
        let request = InfoRequest::user_fills(user);
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get order status for a user.
    pub async fn info_order_status(&self, user: &str, oid: u64) -> Result<HyperliquidOrderStatus> {
        let request = InfoRequest::order_status(user, oid);
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Get all open orders for a user.
    pub async fn info_open_orders(&self, user: &str) -> Result<Value> {
        let request = InfoRequest::open_orders(user);
        self.send_info_request(&request).await
    }

    /// Get frontend open orders (includes more detail) for a user.
    pub async fn info_frontend_open_orders(&self, user: &str) -> Result<Value> {
        let request = InfoRequest::frontend_open_orders(user);
        self.send_info_request(&request).await
    }

    /// Get clearinghouse state (balances, positions, margin) for a user.
    pub async fn info_clearinghouse_state(&self, user: &str) -> Result<Value> {
        let request = InfoRequest::clearinghouse_state(user);
        self.send_info_request(&request).await
    }

    /// Get candle/bar data for a coin.
    ///
    /// # Arguments
    /// * `coin` - The coin symbol (e.g., "BTC")
    /// * `interval` - The timeframe (e.g., "1m", "5m", "15m", "1h", "4h", "1d")
    /// * `start_time` - Start timestamp in milliseconds
    /// * `end_time` - End timestamp in milliseconds
    pub async fn info_candle_snapshot(
        &self,
        coin: &str,
        interval: &str,
        start_time: u64,
        end_time: u64,
    ) -> Result<crate::http::models::HyperliquidCandleSnapshot> {
        let request = InfoRequest::candle_snapshot(coin, interval, start_time, end_time);
        let response = self.send_info_request(&request).await?;
        serde_json::from_value(response).map_err(Error::Serde)
    }

    /// Generic info request method that returns raw JSON (useful for new endpoints and testing).
    pub async fn send_info_request_raw(&self, request: &InfoRequest) -> Result<Value> {
        self.send_info_request(request).await
    }

    /// Send a raw info request and return the JSON response.
    async fn send_info_request(&self, request: &InfoRequest) -> Result<Value> {
        let base_w = info_base_weight(request);
        self.rest_limiter.acquire(base_w).await;

        let mut attempt = 0u32;
        loop {
            let response = self.http_roundtrip_info(request).await?;

            if response.status.is_success() {
                // decode once to count items, then materialize T
                let val: Value = serde_json::from_slice(&response.body).map_err(Error::Serde)?;
                let extra = info_extra_weight(request, &val);
                if extra > 0 {
                    self.rest_limiter.debit_extra(extra).await;
                    tracing::debug!(endpoint=?request, base_w, extra, "info: debited extra weight");
                }
                return Ok(val);
            }

            // 429 → respect Retry-After; else jittered backoff. Retry Info only.
            if response.status.as_u16() == 429 {
                if attempt >= self.rate_limit_max_attempts_info {
                    let ra = self.parse_retry_after_simple(&response.headers);
                    return Err(Error::rate_limit("info", base_w, ra));
                }
                let delay = self
                    .parse_retry_after_simple(&response.headers)
                    .map(Duration::from_millis)
                    .unwrap_or_else(|| {
                        backoff_full_jitter(
                            attempt,
                            self.rate_limit_backoff_base,
                            self.rate_limit_backoff_cap,
                        )
                    });
                tracing::warn!(endpoint=?request, attempt, wait_ms=?delay.as_millis(), "429 Too Many Requests; backing off");
                attempt += 1;
                sleep(delay).await;
                // tiny re-acquire to avoid stampede exactly on minute boundary
                self.rest_limiter.acquire(1).await;
                continue;
            }

            // transient 5xx: treat like retryable Info (bounded)
            if (response.status.is_server_error() || response.status.as_u16() == 408)
                && attempt < self.rate_limit_max_attempts_info
            {
                let delay = backoff_full_jitter(
                    attempt,
                    self.rate_limit_backoff_base,
                    self.rate_limit_backoff_cap,
                );
                tracing::warn!(endpoint=?request, attempt, status=?response.status.as_u16(), wait_ms=?delay.as_millis(), "transient error; retrying");
                attempt += 1;
                sleep(delay).await;
                continue;
            }

            // non-retryable or exhausted
            let error_body = String::from_utf8_lossy(&response.body);
            return Err(Error::http(
                response.status.as_u16(),
                error_body.to_string(),
            ));
        }
    }

    /// Raw HTTP roundtrip for info requests - returns the original HttpResponse
    async fn http_roundtrip_info(
        &self,
        request: &InfoRequest,
    ) -> Result<nautilus_network::http::HttpResponse> {
        let url = &self.base_info;
        let body = serde_json::to_value(request).map_err(Error::Serde)?;
        let body_bytes = serde_json::to_string(&body)
            .map_err(Error::Serde)?
            .into_bytes();

        self.client
            .request(
                Method::POST,
                url.clone(),
                None,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)
    }

    /// Parse Retry-After from response headers (simplified)
    fn parse_retry_after_simple(&self, headers: &HashMap<String, String>) -> Option<u64> {
        let retry_after = headers.get("retry-after")?;
        retry_after.parse::<u64>().ok().map(|s| s * 1000) // convert seconds to ms
    }

    // ---------------- EXCHANGE ENDPOINTS ---------------------------------------

    /// Send a signed action to the exchange.
    pub async fn post_action(
        &self,
        action: &ExchangeAction,
    ) -> Result<HyperliquidExchangeResponse> {
        let w = exchange_weight(action);
        self.rest_limiter.acquire(w).await;

        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| Error::auth("credentials required for exchange operations"))?;

        let nonce_manager = self
            .nonce_manager
            .as_ref()
            .ok_or_else(|| Error::auth("nonce manager missing"))?;

        let signer_id = self.signer_id()?;
        let time_nonce = nonce_manager.next(signer_id.clone())?;
        nonce_manager.validate_local(signer_id, time_nonce)?;

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

        let request = if let Some(vault) = self.vault_address {
            HyperliquidExchangeRequest::with_vault(
                action.clone(),
                time_nonce.as_millis() as u64,
                sig,
                vault.to_string(),
            )
        } else {
            HyperliquidExchangeRequest::new(action.clone(), time_nonce.as_millis() as u64, sig)
        };

        let response = self.http_roundtrip_exchange(&request).await?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else if response.status.as_u16() == 429 {
            let ra = self.parse_retry_after_simple(&response.headers);
            Err(Error::rate_limit("exchange", w, ra))
        } else {
            let error_body = String::from_utf8_lossy(&response.body);
            Err(Error::http(
                response.status.as_u16(),
                error_body.to_string(),
            ))
        }
    }

    /// Raw HTTP roundtrip for exchange requests
    async fn http_roundtrip_exchange(
        &self,
        request: &HyperliquidExchangeRequest<ExchangeAction>,
    ) -> Result<nautilus_network::http::HttpResponse> {
        let url = &self.base_exchange;
        let body = serde_json::to_string(&request).map_err(Error::Serde)?;
        let body_bytes = body.into_bytes();

        self.client
            .request(
                Method::POST,
                url.clone(),
                None,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)
    }

    /// Best-effort gauge for diagnostics/metrics
    pub async fn rest_limiter_snapshot(&self) -> RateLimitSnapshot {
        self.rest_limiter.snapshot().await
    }

    // ---------------- INTERNALS -----------------------------------------------

    fn signer_id(&self) -> Result<SignerId> {
        Ok(SignerId("hyperliquid:default".into()))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::http::query::InfoRequest;

    #[rstest]
    fn stable_json_roundtrips() {
        let v = serde_json::json!({"type":"l2Book","coin":"BTC"});
        let s = serde_json::to_string(&v).unwrap();
        // Parse back to ensure JSON structure is correct, regardless of field order
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["type"], "l2Book");
        assert_eq!(parsed["coin"], "BTC");
        assert_eq!(parsed, v);
    }

    #[rstest]
    fn info_pretty_shape() {
        let r = InfoRequest::l2_book("BTC");
        let val = serde_json::to_value(&r).unwrap();
        let pretty = serde_json::to_string_pretty(&val).unwrap();
        assert!(pretty.contains("\"type\": \"l2Book\""));
        assert!(pretty.contains("\"coin\": \"BTC\""));
    }
}
