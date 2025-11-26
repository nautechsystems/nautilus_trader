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
    str::FromStr,
    sync::{Arc, LazyLock, RwLock},
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use nautilus_core::{UUID4, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::{BarAggregation, OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpResponse},
    ratelimiter::quota::Quota,
};
use reqwest::{Method, header::USER_AGENT};
use rust_decimal::Decimal;
use serde_json::Value;
use tokio::time::sleep;
use ustr::Ustr;

use crate::{
    common::{
        consts::{HYPERLIQUID_VENUE, exchange_url, info_url},
        credential::{Secrets, VaultAddress},
        enums::{
            HyperliquidBarInterval, HyperliquidOrderStatus as HyperliquidOrderStatusEnum,
            HyperliquidProductType,
        },
        parse::{
            bar_type_to_interval, extract_asset_id_from_symbol, orders_to_hyperliquid_requests,
        },
    },
    http::{
        error::{Error, Result},
        models::{
            Cloid, HyperliquidCandleSnapshot, HyperliquidExchangeRequest,
            HyperliquidExchangeResponse, HyperliquidExecAction,
            HyperliquidExecCancelByCloidRequest, HyperliquidExecCancelOrderRequest,
            HyperliquidExecGrouping, HyperliquidExecLimitParams, HyperliquidExecOrderKind,
            HyperliquidExecOrderResponseData, HyperliquidExecOrderStatus,
            HyperliquidExecPlaceOrderRequest, HyperliquidExecTif, HyperliquidExecTpSl,
            HyperliquidExecTriggerParams, HyperliquidFills, HyperliquidL2Book, HyperliquidMeta,
            HyperliquidOrderStatus, PerpMeta, PerpMetaAndCtxs, SpotMeta, SpotMetaAndCtxs,
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

/// Provides a raw HTTP client for low-level Hyperliquid REST API operations.
///
/// This client handles HTTP infrastructure, request signing, and raw API calls
/// that closely match Hyperliquid endpoint specifications.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct HyperliquidRawHttpClient {
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

impl HyperliquidRawHttpClient {
    /// Creates a new [`HyperliquidRawHttpClient`] for public endpoints only.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        is_testnet: bool,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*HYPERLIQUID_REST_QUOTA),
                timeout_secs,
                proxy_url,
            )?,
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
        })
    }

    /// Creates a new [`HyperliquidRawHttpClient`] configured with credentials
    /// for authenticated requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_credentials(
        secrets: &Secrets,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        let signer = HyperliquidEip712Signer::new(secrets.private_key.clone());
        let nonce_manager = Arc::new(NonceManager::new());

        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*HYPERLIQUID_REST_QUOTA),
                timeout_secs,
                proxy_url,
            )?,
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
        })
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables are not set.
    pub fn from_env() -> Result<Self> {
        let secrets =
            Secrets::from_env().map_err(|_| Error::auth("missing credentials in environment"))?;
        Self::with_credentials(&secrets, None, None)
            .map_err(|e| Error::auth(format!("Failed to create HTTP client: {e}")))
    }

    /// Creates a new [`HyperliquidRawHttpClient`] configured with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if the private key is invalid or cannot be parsed.
    pub fn from_credentials(
        private_key: &str,
        vault_address: Option<&str>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self> {
        let secrets = Secrets::from_private_key(private_key, vault_address, is_testnet)
            .map_err(|e| Error::auth(format!("invalid credentials: {e}")))?;
        Self::with_credentials(&secrets, timeout_secs, proxy_url)
            .map_err(|e| Error::auth(format!("Failed to create HTTP client: {e}")))
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

    fn signer_id(&self) -> Result<SignerId> {
        Ok(SignerId("hyperliquid:default".into()))
    }

    /// Parse Retry-After from response headers (simplified)
    fn parse_retry_after_simple(&self, headers: &HashMap<String, String>) -> Option<u64> {
        let retry_after = headers.get("retry-after")?;
        retry_after.parse::<u64>().ok().map(|s| s * 1000) // convert seconds to ms
    }

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
    pub async fn info_candle_snapshot(
        &self,
        coin: &str,
        interval: HyperliquidBarInterval,
        start_time: u64,
        end_time: u64,
    ) -> Result<HyperliquidCandleSnapshot> {
        let request = InfoRequest::candle_snapshot(coin, interval, start_time, end_time);
        let response = self.send_info_request(&request).await?;

        tracing::trace!(
            "Candle snapshot raw response (len={}): {:?}",
            response.as_array().map_or(0, |a| a.len()),
            response
        );

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
                    .map_or_else(
                        || {
                            backoff_full_jitter(
                                attempt,
                                self.rate_limit_backoff_base,
                                self.rate_limit_backoff_cap,
                            )
                        },
                        Duration::from_millis,
                    );
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

    /// Raw HTTP roundtrip for info requests - returns the original HttpResponse.
    async fn http_roundtrip_info(&self, request: &InfoRequest) -> Result<HttpResponse> {
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
                None,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)
    }

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
        let time_nonce = nonce_manager.next(signer_id)?;

        let action_value = serde_json::to_value(action)
            .context("serialize exchange action")
            .map_err(|e| Error::bad_request(e.to_string()))?;

        // Serialize the original action struct with MessagePack for L1 signing
        let action_bytes = rmp_serde::to_vec_named(action)
            .context("serialize action with MessagePack")
            .map_err(|e| Error::bad_request(e.to_string()))?;

        let sign_request = SignRequest {
            action: action_value.clone(),
            action_bytes: Some(action_bytes),
            time_nonce,
            action_type: HyperliquidActionType::L1,
            is_testnet: self.is_testnet,
            vault_address: self.vault_address.as_ref().map(|v| v.to_hex()),
        };

        let sig = signer.sign(&sign_request)?.signature;

        let nonce_u64 = time_nonce.as_millis() as u64;

        let request = if let Some(vault) = self.vault_address {
            HyperliquidExchangeRequest::with_vault(
                action.clone(),
                nonce_u64,
                sig,
                vault.to_string(),
            )
            .map_err(|e| Error::bad_request(format!("Failed to create request: {e}")))?
        } else {
            HyperliquidExchangeRequest::new(action.clone(), nonce_u64, sig)
                .map_err(|e| Error::bad_request(format!("Failed to create request: {e}")))?
        };

        let response = self.http_roundtrip_exchange(&request).await?;

        if response.status.is_success() {
            let parsed_response: HyperliquidExchangeResponse =
                serde_json::from_slice(&response.body).map_err(Error::Serde)?;

            // Check if the response contains an error status
            match &parsed_response {
                HyperliquidExchangeResponse::Status {
                    status,
                    response: response_data,
                } if status == "err" => {
                    let error_msg = response_data
                        .as_str()
                        .map_or_else(|| response_data.to_string(), |s| s.to_string());
                    tracing::error!("Hyperliquid API returned error: {error_msg}");
                    Err(Error::bad_request(format!("API error: {error_msg}")))
                }
                HyperliquidExchangeResponse::Error { error } => {
                    tracing::error!("Hyperliquid API returned error: {error}");
                    Err(Error::bad_request(format!("API error: {error}")))
                }
                _ => Ok(parsed_response),
            }
        } else if response.status.as_u16() == 429 {
            let ra = self.parse_retry_after_simple(&response.headers);
            Err(Error::rate_limit("exchange", w, ra))
        } else {
            let error_body = String::from_utf8_lossy(&response.body);
            tracing::error!(
                "Exchange API error (status {}): {}",
                response.status.as_u16(),
                error_body
            );
            Err(Error::http(
                response.status.as_u16(),
                error_body.to_string(),
            ))
        }
    }

    /// Send a signed action to the exchange using the typed HyperliquidExecAction enum.
    ///
    /// This is the preferred method for placing orders as it uses properly typed
    /// structures that match Hyperliquid's API expectations exactly.
    pub async fn post_action_exec(
        &self,
        action: &HyperliquidExecAction,
    ) -> Result<HyperliquidExchangeResponse> {
        let w = match action {
            HyperliquidExecAction::Order { orders, .. } => 1 + (orders.len() as u32 / 40),
            HyperliquidExecAction::Cancel { cancels } => 1 + (cancels.len() as u32 / 40),
            HyperliquidExecAction::CancelByCloid { cancels } => 1 + (cancels.len() as u32 / 40),
            HyperliquidExecAction::BatchModify { modifies } => 1 + (modifies.len() as u32 / 40),
            _ => 1,
        };
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
        let time_nonce = nonce_manager.next(signer_id)?;
        // No need to validate - next() guarantees a valid, unused nonce

        let action_value = serde_json::to_value(action)
            .context("serialize exchange action")
            .map_err(|e| Error::bad_request(e.to_string()))?;

        // Serialize the original action struct with MessagePack for L1 signing
        let action_bytes = rmp_serde::to_vec_named(action)
            .context("serialize action with MessagePack")
            .map_err(|e| Error::bad_request(e.to_string()))?;

        let sig = signer
            .sign(&SignRequest {
                action: action_value.clone(),
                action_bytes: Some(action_bytes),
                time_nonce,
                action_type: HyperliquidActionType::L1,
                is_testnet: self.is_testnet,
                vault_address: self.vault_address.as_ref().map(|v| v.to_hex()),
            })?
            .signature;

        let request = if let Some(vault) = self.vault_address {
            HyperliquidExchangeRequest::with_vault(
                action.clone(),
                time_nonce.as_millis() as u64,
                sig,
                vault.to_string(),
            )
            .map_err(|e| Error::bad_request(format!("Failed to create request: {e}")))?
        } else {
            HyperliquidExchangeRequest::new(action.clone(), time_nonce.as_millis() as u64, sig)
                .map_err(|e| Error::bad_request(format!("Failed to create request: {e}")))?
        };

        let response = self.http_roundtrip_exchange(&request).await?;

        if response.status.is_success() {
            let parsed_response: HyperliquidExchangeResponse =
                serde_json::from_slice(&response.body).map_err(Error::Serde)?;

            // Check if the response contains an error status
            match &parsed_response {
                HyperliquidExchangeResponse::Status {
                    status,
                    response: response_data,
                } if status == "err" => {
                    let error_msg = response_data
                        .as_str()
                        .map_or_else(|| response_data.to_string(), |s| s.to_string());
                    tracing::error!("Hyperliquid API returned error: {error_msg}");
                    Err(Error::bad_request(format!("API error: {error_msg}")))
                }
                HyperliquidExchangeResponse::Error { error } => {
                    tracing::error!("Hyperliquid API returned error: {error}");
                    Err(Error::bad_request(format!("API error: {error}")))
                }
                _ => Ok(parsed_response),
            }
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

    /// Submit a single order to the Hyperliquid exchange.
    ///
    pub async fn rest_limiter_snapshot(&self) -> RateLimitSnapshot {
        self.rest_limiter.snapshot().await
    }
    async fn http_roundtrip_exchange<T>(
        &self,
        request: &HyperliquidExchangeRequest<T>,
    ) -> Result<nautilus_network::http::HttpResponse>
    where
        T: serde::Serialize,
    {
        let url = &self.base_exchange;
        let body = serde_json::to_string(&request).map_err(Error::Serde)?;
        let body_bytes = body.into_bytes();

        let response = self
            .client
            .request(
                Method::POST,
                url.clone(),
                None,
                None,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;

        Ok(response)
    }
}

/// Provides a high-level HTTP client for the [Hyperliquid](https://hyperliquid.xyz/) REST API.
///
/// This domain client wraps [`HyperliquidRawHttpClient`] and provides methods that work
/// with Nautilus domain types. It maintains an instrument cache and handles conversions
/// between Hyperliquid API responses and Nautilus domain models.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct HyperliquidHttpClient {
    pub(crate) inner: Arc<HyperliquidRawHttpClient>,
    instruments: Arc<RwLock<AHashMap<Ustr, InstrumentAny>>>,
    instruments_by_coin: Arc<RwLock<AHashMap<(Ustr, HyperliquidProductType), InstrumentAny>>>,
    account_id: Option<AccountId>,
}

impl Default for HyperliquidHttpClient {
    fn default() -> Self {
        Self::new(true, None, None).expect("Failed to create default Hyperliquid HTTP client")
    }
}

impl HyperliquidHttpClient {
    /// Creates a new [`HyperliquidHttpClient`] for public endpoints only.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        is_testnet: bool,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        let raw_client = HyperliquidRawHttpClient::new(is_testnet, timeout_secs, proxy_url)?;
        Ok(Self {
            inner: Arc::new(raw_client),
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            instruments_by_coin: Arc::new(RwLock::new(AHashMap::new())),
            account_id: None,
        })
    }

    /// Creates a new [`HyperliquidHttpClient`] configured with credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_credentials(
        secrets: &Secrets,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> std::result::Result<Self, HttpClientError> {
        let raw_client =
            HyperliquidRawHttpClient::with_credentials(secrets, timeout_secs, proxy_url)?;
        Ok(Self {
            inner: Arc::new(raw_client),
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            instruments_by_coin: Arc::new(RwLock::new(AHashMap::new())),
            account_id: None,
        })
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables are not set.
    pub fn from_env() -> Result<Self> {
        let raw_client = HyperliquidRawHttpClient::from_env()?;
        Ok(Self {
            inner: Arc::new(raw_client),
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            instruments_by_coin: Arc::new(RwLock::new(AHashMap::new())),
            account_id: None,
        })
    }

    /// Creates a new [`HyperliquidHttpClient`] configured with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if the private key is invalid or cannot be parsed.
    pub fn from_credentials(
        private_key: &str,
        vault_address: Option<&str>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self> {
        let raw_client = HyperliquidRawHttpClient::from_credentials(
            private_key,
            vault_address,
            is_testnet,
            timeout_secs,
            proxy_url,
        )?;
        Ok(Self {
            inner: Arc::new(raw_client),
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            instruments_by_coin: Arc::new(RwLock::new(AHashMap::new())),
            account_id: None,
        })
    }

    /// Returns whether this client is configured for testnet.
    #[must_use]
    pub fn is_testnet(&self) -> bool {
        self.inner.is_testnet()
    }

    /// Gets the user address derived from the private key (if client has credentials).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if the client has no signer configured.
    pub fn get_user_address(&self) -> Result<String> {
        self.inner.get_user_address()
    }

    /// Caches a single instrument.
    ///
    /// This is required for parsing orders, fills, and positions into reports.
    /// Any existing instrument with the same symbol will be replaced.
    ///
    /// # Panics
    ///
    /// Panics if the instrument lock cannot be acquired.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let full_symbol = instrument.symbol().inner();
        let coin = instrument.raw_symbol().inner();

        {
            let mut instruments = self
                .instruments
                .write()
                .expect("Failed to acquire write lock");

            instruments.insert(full_symbol, instrument.clone());

            // HTTP responses only include coins, external code may lookup by coin
            instruments.insert(coin, instrument.clone());
        }

        // Composite key allows disambiguating same coin across PERP and SPOT
        if let Ok(product_type) = HyperliquidProductType::from_symbol(full_symbol.as_str()) {
            let mut instruments_by_coin = self
                .instruments_by_coin
                .write()
                .expect("Failed to acquire write lock");
            instruments_by_coin.insert((coin, product_type), instrument);
        } else {
            tracing::warn!(
                "Unable to determine product type for symbol: {}",
                full_symbol
            );
        }
    }

    /// Get an instrument from cache, or create a synthetic one for vault tokens.
    ///
    /// Vault tokens (starting with "vntls:") are not available in the standard spotMeta API.
    /// This method creates synthetic CurrencyPair instruments for vault tokens on-the-fly
    /// to allow order/fill/position parsing to continue.
    ///
    /// For non-vault tokens that are not in cache, returns None and logs a warning.
    /// This can happen if instruments weren't loaded properly or if there are new instruments
    /// that weren't present during initialization.
    ///
    /// The synthetic instruments use reasonable defaults:
    /// - Quote currency: USDC (most common quote for vault tokens)
    /// - Price/size decimals: 8 (standard precision)
    /// - Price increment: 0.00000001
    /// - Size increment: 0.00000001
    fn get_or_create_instrument(
        &self,
        coin: &Ustr,
        product_type: Option<HyperliquidProductType>,
    ) -> Option<InstrumentAny> {
        if let Some(pt) = product_type {
            let instruments_by_coin = self
                .instruments_by_coin
                .read()
                .expect("Failed to acquire read lock");

            if let Some(instrument) = instruments_by_coin.get(&(*coin, pt)) {
                return Some(instrument.clone());
            }
        }

        // HTTP responses lack product type context, try PERP then SPOT
        if product_type.is_none() {
            let instruments_by_coin = self
                .instruments_by_coin
                .read()
                .expect("Failed to acquire read lock");

            if let Some(instrument) =
                instruments_by_coin.get(&(*coin, HyperliquidProductType::Perp))
            {
                return Some(instrument.clone());
            }
            if let Some(instrument) =
                instruments_by_coin.get(&(*coin, HyperliquidProductType::Spot))
            {
                return Some(instrument.clone());
            }
        }

        // Vault tokens aren't in standard API, create synthetic instruments
        if coin.as_str().starts_with("vntls:") {
            tracing::info!("Creating synthetic instrument for vault token: {coin}");

            let clock = nautilus_core::time::get_atomic_clock_realtime();
            let ts_event = clock.get_time_ns();

            // Create synthetic vault token instrument
            let symbol_str = format!("{coin}-USDC-SPOT");
            let symbol = nautilus_model::identifiers::Symbol::new(&symbol_str);
            let venue = *HYPERLIQUID_VENUE;
            let instrument_id = nautilus_model::identifiers::InstrumentId::new(symbol, venue);

            // Create currencies
            let base_currency = nautilus_model::types::Currency::new(
                coin.as_str(),
                8, // precision
                0, // ISO code (not applicable)
                coin.as_str(),
                nautilus_model::enums::CurrencyType::Crypto,
            );

            let quote_currency = nautilus_model::types::Currency::new(
                "USDC",
                6, // USDC standard precision
                0,
                "USDC",
                nautilus_model::enums::CurrencyType::Crypto,
            );

            let price_increment = nautilus_model::types::Price::from("0.00000001");
            let size_increment = nautilus_model::types::Quantity::from("0.00000001");

            let instrument =
                InstrumentAny::CurrencyPair(nautilus_model::instruments::CurrencyPair::new(
                    instrument_id,
                    symbol,
                    base_currency,
                    quote_currency,
                    8, // price_precision
                    8, // size_precision
                    price_increment,
                    size_increment,
                    None, // price_increment
                    None, // size_increment
                    None, // maker_fee
                    None, // taker_fee
                    None, // margin_init
                    None, // margin_maint
                    None, // lot_size
                    None, // max_quantity
                    None, // min_quantity
                    None, // max_notional
                    None, // min_notional
                    None, // max_price
                    ts_event,
                    ts_event,
                ));

            self.cache_instrument(instrument.clone());

            Some(instrument)
        } else {
            // For non-vault tokens, log warning and return None
            tracing::warn!("Instrument not found in cache: {coin}");
            None
        }
    }

    /// Set the account ID for this client.
    ///
    /// This is required for generating reports with the correct account ID.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
    }

    /// Fetch and parse all available instrument definitions from Hyperliquid.
    pub async fn request_instruments(&self) -> Result<Vec<InstrumentAny>> {
        let mut defs: Vec<HyperliquidInstrumentDef> = Vec::new();

        match self.inner.load_perp_meta().await {
            Ok(perp_meta) => match parse_perp_instruments(&perp_meta) {
                Ok(perp_defs) => {
                    tracing::debug!(
                        count = perp_defs.len(),
                        "Loaded Hyperliquid perp definitions"
                    );
                    defs.extend(perp_defs);
                }
                Err(e) => {
                    tracing::warn!(%e, "Failed to parse Hyperliquid perp instruments");
                }
            },
            Err(e) => {
                tracing::warn!(%e, "Failed to load Hyperliquid perp metadata");
            }
        }

        match self.inner.get_spot_meta().await {
            Ok(spot_meta) => match parse_spot_instruments(&spot_meta) {
                Ok(spot_defs) => {
                    tracing::debug!(
                        count = spot_defs.len(),
                        "Loaded Hyperliquid spot definitions"
                    );
                    defs.extend(spot_defs);
                }
                Err(e) => {
                    tracing::warn!(%e, "Failed to parse Hyperliquid spot instruments");
                }
            },
            Err(e) => {
                tracing::warn!(%e, "Failed to load Hyperliquid spot metadata");
            }
        }

        Ok(instruments_from_defs_owned(defs))
    }

    /// Get perpetuals metadata (internal helper).
    pub(crate) async fn load_perp_meta(&self) -> Result<PerpMeta> {
        self.inner.load_perp_meta().await
    }

    /// Get spot metadata (internal helper).
    pub(crate) async fn get_spot_meta(&self) -> Result<SpotMeta> {
        self.inner.get_spot_meta().await
    }

    /// Get L2 order book for a coin.
    pub async fn info_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book> {
        self.inner.info_l2_book(coin).await
    }

    /// Get user fills (trading history).
    pub async fn info_user_fills(&self, user: &str) -> Result<HyperliquidFills> {
        self.inner.info_user_fills(user).await
    }

    /// Get order status for a user.
    pub async fn info_order_status(&self, user: &str, oid: u64) -> Result<HyperliquidOrderStatus> {
        self.inner.info_order_status(user, oid).await
    }

    /// Get all open orders for a user.
    pub async fn info_open_orders(&self, user: &str) -> Result<Value> {
        self.inner.info_open_orders(user).await
    }

    /// Get frontend open orders (includes more detail) for a user.
    pub async fn info_frontend_open_orders(&self, user: &str) -> Result<Value> {
        self.inner.info_frontend_open_orders(user).await
    }

    /// Get clearinghouse state (balances, positions, margin) for a user.
    pub async fn info_clearinghouse_state(&self, user: &str) -> Result<Value> {
        self.inner.info_clearinghouse_state(user).await
    }

    /// Get candle/bar data for a coin.
    pub async fn info_candle_snapshot(
        &self,
        coin: &str,
        interval: HyperliquidBarInterval,
        start_time: u64,
        end_time: u64,
    ) -> Result<HyperliquidCandleSnapshot> {
        self.inner
            .info_candle_snapshot(coin, interval, start_time, end_time)
            .await
    }

    /// Post an action to the exchange endpoint (low-level delegation).
    pub async fn post_action(
        &self,
        action: &ExchangeAction,
    ) -> Result<HyperliquidExchangeResponse> {
        self.inner.post_action(action).await
    }

    /// Post an execution action (low-level delegation).
    pub async fn post_action_exec(
        &self,
        action: &HyperliquidExecAction,
    ) -> Result<HyperliquidExchangeResponse> {
        self.inner.post_action_exec(action).await
    }

    /// Get metadata about available markets (low-level delegation).
    pub async fn info_meta(&self) -> Result<HyperliquidMeta> {
        self.inner.info_meta().await
    }

    /// Cancel an order on the Hyperliquid exchange.
    ///
    /// Can cancel either by venue order ID or client order ID.
    /// At least one ID must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, no order ID is provided,
    /// or the API returns an error.
    pub async fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> Result<()> {
        // Extract asset ID from instrument symbol
        let symbol = instrument_id.symbol.as_str();
        let asset_id = extract_asset_id_from_symbol(symbol)
            .map_err(|e| Error::bad_request(format!("Failed to extract asset ID: {e}")))?;

        // Create cancel action based on which ID we have
        let action = if let Some(cloid) = client_order_id {
            let cloid_hex = Cloid::from_hex(cloid)
                .map_err(|e| Error::bad_request(format!("Invalid client order ID format: {e}")))?;
            let cancel_req = HyperliquidExecCancelByCloidRequest {
                asset: asset_id,
                cloid: cloid_hex,
            };
            HyperliquidExecAction::CancelByCloid {
                cancels: vec![cancel_req],
            }
        } else if let Some(oid) = venue_order_id {
            let oid_u64 = oid
                .as_str()
                .parse::<u64>()
                .map_err(|_| Error::bad_request("Invalid venue order ID format"))?;
            let cancel_req = HyperliquidExecCancelOrderRequest {
                asset: asset_id,
                oid: oid_u64,
            };
            HyperliquidExecAction::Cancel {
                cancels: vec![cancel_req],
            }
        } else {
            return Err(Error::bad_request(
                "Either client_order_id or venue_order_id must be provided",
            ));
        };

        // Submit cancellation
        let response = self.inner.post_action_exec(&action).await?;

        // Check response - only check for error status
        match response {
            HyperliquidExchangeResponse::Status { status, .. } if status == "ok" => Ok(()),
            HyperliquidExchangeResponse::Status {
                status,
                response: error_data,
            } => Err(Error::bad_request(format!(
                "Cancel order failed: status={status}, error={error_data}"
            ))),
            HyperliquidExchangeResponse::Error { error } => {
                Err(Error::bad_request(format!("Cancel order error: {error}")))
            }
        }
    }

    /// Request order status reports for a user.
    ///
    /// Fetches open orders via `info_frontend_open_orders` and parses them into OrderStatusReports.
    /// This method requires instruments to be added to the client cache via `cache_instrument()`.
    ///
    /// For vault tokens (starting with "vntls:") that are not in the cache, synthetic instruments
    /// will be created automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or parsing fails.
    pub async fn request_order_status_reports(
        &self,
        user: &str,
        instrument_id: Option<nautilus_model::identifiers::InstrumentId>,
    ) -> Result<Vec<OrderStatusReport>> {
        let response = self.info_frontend_open_orders(user).await?;

        // Parse the JSON response into a vector of orders
        let orders: Vec<serde_json::Value> = serde_json::from_value(response)
            .map_err(|e| Error::bad_request(format!("Failed to parse orders: {e}")))?;

        let mut reports = Vec::new();
        let ts_init = nautilus_core::UnixNanos::default();

        for order_value in orders {
            // Parse the order data
            let order: crate::websocket::messages::WsBasicOrderData =
                match serde_json::from_value(order_value.clone()) {
                    Ok(o) => o,
                    Err(e) => {
                        tracing::warn!("Failed to parse order: {e}");
                        continue;
                    }
                };

            // Get instrument from cache or create synthetic for vault tokens
            let instrument = match self.get_or_create_instrument(&order.coin, None) {
                Some(inst) => inst,
                None => continue, // Skip if instrument not found
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Determine status from order data - orders from frontend_open_orders are open
            let status = HyperliquidOrderStatusEnum::Open;

            // Parse to OrderStatusReport
            match crate::http::parse::parse_order_status_report_from_basic(
                &order,
                &status,
                &instrument,
                self.account_id.unwrap_or_default(),
                ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse order status report: {e}"),
            }
        }

        Ok(reports)
    }

    /// Request fill reports for a user.
    ///
    /// Fetches user fills via `info_user_fills` and parses them into FillReports.
    /// This method requires instruments to be added to the client cache via `cache_instrument()`.
    ///
    /// For vault tokens (starting with "vntls:") that are not in the cache, synthetic instruments
    /// will be created automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or parsing fails.
    pub async fn request_fill_reports(
        &self,
        user: &str,
        instrument_id: Option<nautilus_model::identifiers::InstrumentId>,
    ) -> Result<Vec<FillReport>> {
        let fills_response = self.info_user_fills(user).await?;

        let mut reports = Vec::new();
        let ts_init = nautilus_core::UnixNanos::default();

        for fill in fills_response {
            // Get instrument from cache or create synthetic for vault tokens
            let instrument = match self.get_or_create_instrument(&fill.coin, None) {
                Some(inst) => inst,
                None => continue, // Skip if instrument not found
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to FillReport
            match crate::http::parse::parse_fill_report(
                &fill,
                &instrument,
                self.account_id.unwrap_or_default(),
                ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse fill report: {e}"),
            }
        }

        Ok(reports)
    }

    /// Request position status reports for a user.
    ///
    /// Fetches clearinghouse state via `info_clearinghouse_state` and parses positions into PositionStatusReports.
    /// This method requires instruments to be added to the client cache via `cache_instrument()`.
    ///
    /// For vault tokens (starting with "vntls:") that are not in the cache, synthetic instruments
    /// will be created automatically.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or parsing fails.
    pub async fn request_position_status_reports(
        &self,
        user: &str,
        instrument_id: Option<nautilus_model::identifiers::InstrumentId>,
    ) -> Result<Vec<nautilus_model::reports::PositionStatusReport>> {
        let state_response = self.info_clearinghouse_state(user).await?;

        // Extract asset positions from the clearinghouse state
        let asset_positions: Vec<serde_json::Value> = state_response
            .get("assetPositions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::bad_request("assetPositions not found in clearinghouse state"))?
            .clone();

        let mut reports = Vec::new();
        let ts_init = nautilus_core::UnixNanos::default();

        for position_value in asset_positions {
            // Extract coin from position data
            let coin = position_value
                .get("position")
                .and_then(|p| p.get("coin"))
                .and_then(|c| c.as_str())
                .ok_or_else(|| Error::bad_request("coin not found in position"))?;

            // Get instrument from cache - convert &str to Ustr for lookup
            let coin_ustr = Ustr::from(coin);
            let instrument = match self.get_or_create_instrument(&coin_ustr, None) {
                Some(inst) => inst,
                None => continue, // Skip if instrument not found
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to PositionStatusReport
            match crate::http::parse::parse_position_status_report(
                &position_value,
                &instrument,
                self.account_id.unwrap_or_default(),
                ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse position status report: {e}"),
            }
        }

        Ok(reports)
    }

    /// Request historical bars for an instrument.
    ///
    /// Fetches candle data from the Hyperliquid API and converts it to Nautilus bars.
    /// Incomplete bars (where end_timestamp >= current time) are filtered out.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in cache.
    /// - The bar aggregation is unsupported by Hyperliquid.
    /// - The API request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://hyperliquid.gitbook.io/hyperliquid-docs/for-developers/api/info-endpoint#candles-snapshot>
    pub async fn request_bars(
        &self,
        bar_type: nautilus_model::data::BarType,
        start: Option<chrono::DateTime<chrono::Utc>>,
        end: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<u32>,
    ) -> Result<Vec<nautilus_model::data::bar::Bar>> {
        let instrument_id = bar_type.instrument_id();
        let symbol = instrument_id.symbol;

        let coin = Ustr::from(
            symbol
                .as_str()
                .split('-')
                .next()
                .ok_or_else(|| Error::bad_request("Invalid instrument symbol"))?,
        );

        let product_type = HyperliquidProductType::from_symbol(symbol.as_str()).ok();
        let instrument = self
            .get_or_create_instrument(&coin, product_type)
            .ok_or_else(|| {
                Error::bad_request(format!("Instrument not found in cache: {instrument_id}"))
            })?;

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let interval =
            bar_type_to_interval(&bar_type).map_err(|e| Error::bad_request(e.to_string()))?;

        // Hyperliquid uses millisecond timestamps
        let now = chrono::Utc::now();
        let end_time = end.unwrap_or(now).timestamp_millis() as u64;
        let start_time = if let Some(start) = start {
            start.timestamp_millis() as u64
        } else {
            // Default to 1000 bars before end_time
            let spec = bar_type.spec();
            let step_ms = match spec.aggregation {
                BarAggregation::Minute => spec.step.get() as u64 * 60_000,
                BarAggregation::Hour => spec.step.get() as u64 * 3_600_000,
                BarAggregation::Day => spec.step.get() as u64 * 86_400_000,
                BarAggregation::Week => spec.step.get() as u64 * 604_800_000,
                BarAggregation::Month => spec.step.get() as u64 * 2_592_000_000,
                _ => 60_000,
            };
            end_time.saturating_sub(1000 * step_ms)
        };

        let candles = self
            .info_candle_snapshot(coin.as_str(), interval, start_time, end_time)
            .await?;

        // Filter out incomplete bars where end_timestamp >= current time
        let now_ms = now.timestamp_millis() as u64;

        let mut bars: Vec<nautilus_model::data::bar::Bar> = candles
            .iter()
            .filter(|candle| candle.end_timestamp < now_ms)
            .enumerate()
            .filter_map(|(i, candle)| {
                crate::data::candle_to_bar(candle, bar_type, price_precision, size_precision)
                    .map_err(|e| {
                        tracing::error!(
                            "Failed to convert candle {} to bar: {:?} error: {e}",
                            i,
                            candle
                        );
                        e
                    })
                    .ok()
            })
            .collect();

        // 0 means no limit
        if let Some(limit) = limit
            && limit > 0
            && bars.len() > limit as usize
        {
            bars.truncate(limit as usize);
        }

        tracing::debug!(
            "Received {} bars for {} (filtered {} incomplete)",
            bars.len(),
            bar_type,
            candles.len() - bars.len()
        );
        Ok(bars)
    }
    /// Uses the existing order conversion logic from `common::parse::order_to_hyperliquid_request`
    /// to avoid code duplication and ensure consistency.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, order validation fails, serialization fails,
    /// or the API returns an error.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: bool,
        reduce_only: bool,
    ) -> Result<OrderStatusReport> {
        let symbol = instrument_id.symbol.as_str();
        let asset = extract_asset_id_from_symbol(symbol)
            .map_err(|e| Error::bad_request(format!("Failed to extract asset ID: {e}")))?;

        let is_buy = matches!(order_side, OrderSide::Buy);

        // Convert price to decimal
        let price_decimal = match price {
            Some(px) => Decimal::from_str(&px.to_string())
                .map_err(|e| Error::bad_request(format!("Failed to convert price: {e}")))?,
            None => {
                if matches!(
                    order_type,
                    OrderType::Market | OrderType::StopMarket | OrderType::MarketIfTouched
                ) {
                    Decimal::ZERO
                } else {
                    return Err(Error::bad_request("Limit orders require a price"));
                }
            }
        };

        // Convert quantity to decimal
        let size_decimal = Decimal::from_str(&quantity.to_string())
            .map_err(|e| Error::bad_request(format!("Failed to convert quantity: {e}")))?;

        // Determine order kind based on order type
        let kind = match order_type {
            OrderType::Market => HyperliquidExecOrderKind::Limit {
                limit: HyperliquidExecLimitParams {
                    tif: HyperliquidExecTif::Ioc,
                },
            },
            OrderType::Limit => {
                let tif = if post_only {
                    HyperliquidExecTif::Alo
                } else {
                    match time_in_force {
                        TimeInForce::Gtc => HyperliquidExecTif::Gtc,
                        TimeInForce::Ioc => HyperliquidExecTif::Ioc,
                        TimeInForce::Fok => HyperliquidExecTif::Ioc, // Hyperliquid doesn't have FOK
                        TimeInForce::Day
                        | TimeInForce::Gtd
                        | TimeInForce::AtTheOpen
                        | TimeInForce::AtTheClose => {
                            return Err(Error::bad_request(format!(
                                "Time in force {:?} not supported",
                                time_in_force
                            )));
                        }
                    }
                };
                HyperliquidExecOrderKind::Limit {
                    limit: HyperliquidExecLimitParams { tif },
                }
            }
            OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched => {
                if let Some(trig_px) = trigger_price {
                    let trigger_price_decimal =
                        Decimal::from_str(&trig_px.to_string()).map_err(|e| {
                            Error::bad_request(format!("Failed to convert trigger price: {e}"))
                        })?;

                    // Determine TP/SL type based on order type
                    // StopMarket/StopLimit are always Sl (protective stops)
                    // MarketIfTouched/LimitIfTouched are always Tp (profit-taking/entry)
                    let tpsl = match order_type {
                        OrderType::StopMarket | OrderType::StopLimit => HyperliquidExecTpSl::Sl,
                        OrderType::MarketIfTouched | OrderType::LimitIfTouched => {
                            HyperliquidExecTpSl::Tp
                        }
                        _ => unreachable!(),
                    };

                    let is_market = matches!(
                        order_type,
                        OrderType::StopMarket | OrderType::MarketIfTouched
                    );

                    HyperliquidExecOrderKind::Trigger {
                        trigger: HyperliquidExecTriggerParams {
                            is_market,
                            trigger_px: trigger_price_decimal,
                            tpsl,
                        },
                    }
                } else {
                    return Err(Error::bad_request("Trigger orders require a trigger price"));
                }
            }
            _ => {
                return Err(Error::bad_request(format!(
                    "Order type {:?} not supported",
                    order_type
                )));
            }
        };

        // Build the order request
        let hyperliquid_order =
            HyperliquidExecPlaceOrderRequest {
                asset,
                is_buy,
                price: price_decimal,
                size: size_decimal,
                reduce_only,
                kind,
                cloid: Some(Cloid::from_hex(client_order_id).map_err(|e| {
                    Error::bad_request(format!("Invalid client order ID format: {e}"))
                })?),
            };

        // Create action
        let action = HyperliquidExecAction::Order {
            orders: vec![hyperliquid_order],
            grouping: HyperliquidExecGrouping::Na,
            builder: None,
        };

        // Submit to exchange
        let response = self.inner.post_action_exec(&action).await?;

        // Parse response
        match response {
            HyperliquidExchangeResponse::Status {
                status,
                response: response_data,
            } if status == "ok" => {
                let data_value = if let Some(data) = response_data.get("data") {
                    data.clone()
                } else {
                    response_data
                };

                let order_response: HyperliquidExecOrderResponseData =
                    serde_json::from_value(data_value).map_err(|e| {
                        Error::bad_request(format!("Failed to parse order response: {e}"))
                    })?;

                let order_status = order_response
                    .statuses
                    .first()
                    .ok_or_else(|| Error::bad_request("No order status in response"))?;

                let symbol_str = instrument_id.symbol.as_str();
                let asset_str = symbol_str
                    .trim_end_matches("-PERP")
                    .trim_end_matches("-USD");

                let product_type = HyperliquidProductType::from_symbol(symbol_str).ok();
                let instrument = self
                    .get_or_create_instrument(&Ustr::from(asset_str), product_type)
                    .ok_or_else(|| {
                        Error::bad_request(format!("Instrument not found for {asset_str}"))
                    })?;

                let account_id = self
                    .account_id
                    .ok_or_else(|| Error::bad_request("Account ID not set"))?;
                let ts_init = nautilus_core::UnixNanos::default();

                match order_status {
                    HyperliquidExecOrderStatus::Resting { resting } => self
                        .create_order_status_report(
                            instrument_id,
                            Some(client_order_id),
                            nautilus_model::identifiers::VenueOrderId::new(resting.oid.to_string()),
                            order_side,
                            order_type,
                            quantity,
                            time_in_force,
                            price,
                            trigger_price,
                            nautilus_model::enums::OrderStatus::Accepted,
                            nautilus_model::types::Quantity::new(0.0, instrument.size_precision()),
                            &instrument,
                            account_id,
                            ts_init,
                        ),
                    HyperliquidExecOrderStatus::Filled { filled } => {
                        let filled_qty = nautilus_model::types::Quantity::new(
                            filled.total_sz.to_string().parse::<f64>().unwrap_or(0.0),
                            instrument.size_precision(),
                        );
                        self.create_order_status_report(
                            instrument_id,
                            Some(client_order_id),
                            nautilus_model::identifiers::VenueOrderId::new(filled.oid.to_string()),
                            order_side,
                            order_type,
                            quantity,
                            time_in_force,
                            price,
                            trigger_price,
                            nautilus_model::enums::OrderStatus::Filled,
                            filled_qty,
                            &instrument,
                            account_id,
                            ts_init,
                        )
                    }
                    HyperliquidExecOrderStatus::Error { error } => {
                        Err(Error::bad_request(format!("Order rejected: {error}")))
                    }
                }
            }
            HyperliquidExchangeResponse::Error { error } => Err(Error::bad_request(format!(
                "Order submission failed: {error}"
            ))),
            _ => Err(Error::bad_request("Unexpected response format")),
        }
    }

    /// Submit an order using an OrderAny object.
    ///
    /// This is a convenience method that wraps submit_order.
    pub async fn submit_order_from_order_any(&self, order: &OrderAny) -> Result<OrderStatusReport> {
        self.submit_order(
            order.instrument_id(),
            order.client_order_id(),
            order.order_side(),
            order.order_type(),
            order.quantity(),
            order.time_in_force(),
            order.price(),
            order.trigger_price(),
            order.is_post_only(),
            order.is_reduce_only(),
        )
        .await
    }

    /// Create an OrderStatusReport from order submission details.
    #[allow(clippy::too_many_arguments)]
    fn create_order_status_report(
        &self,
        instrument_id: nautilus_model::identifiers::InstrumentId,
        client_order_id: Option<nautilus_model::identifiers::ClientOrderId>,
        venue_order_id: nautilus_model::identifiers::VenueOrderId,
        order_side: nautilus_model::enums::OrderSide,
        order_type: nautilus_model::enums::OrderType,
        quantity: nautilus_model::types::Quantity,
        time_in_force: nautilus_model::enums::TimeInForce,
        price: Option<nautilus_model::types::Price>,
        trigger_price: Option<nautilus_model::types::Price>,
        order_status: nautilus_model::enums::OrderStatus,
        filled_qty: nautilus_model::types::Quantity,
        _instrument: &nautilus_model::instruments::InstrumentAny,
        account_id: nautilus_model::identifiers::AccountId,
        ts_init: nautilus_core::UnixNanos,
    ) -> Result<OrderStatusReport> {
        let clock = get_atomic_clock_realtime();
        let ts_accepted = clock.get_time_ns();
        let ts_last = ts_accepted;
        let report_id = UUID4::new();

        let mut report = OrderStatusReport::new(
            account_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            order_side,
            order_type,
            time_in_force,
            order_status,
            quantity,
            filled_qty,
            ts_accepted,
            ts_last,
            ts_init,
            Some(report_id),
        );

        // Add price if present
        if let Some(px) = price {
            report = report.with_price(px);
        }

        // Add trigger price if present
        if let Some(trig_px) = trigger_price {
            report = report
                .with_trigger_price(trig_px)
                .with_trigger_type(nautilus_model::enums::TriggerType::Default);
        }

        Ok(report)
    }

    /// Submit multiple orders to the Hyperliquid exchange in a single request.
    ///
    /// Uses the existing order conversion logic from `common::parse::orders_to_hyperliquid_requests`
    /// to avoid code duplication and ensure consistency.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, order validation fails, serialization fails,
    /// or the API returns an error.
    pub async fn submit_orders(&self, orders: &[&OrderAny]) -> Result<Vec<OrderStatusReport>> {
        // Use the existing parsing function from common::parse
        let hyperliquid_orders = orders_to_hyperliquid_requests(orders)
            .map_err(|e| Error::bad_request(format!("Failed to convert orders: {e}")))?;

        // Create typed action using HyperliquidExecAction (same as working Rust binary)
        let action = HyperliquidExecAction::Order {
            orders: hyperliquid_orders,
            grouping: HyperliquidExecGrouping::Na,
            builder: None,
        };

        // Submit to exchange using the typed exec endpoint
        let response = self.inner.post_action_exec(&action).await?;

        // Parse the response to extract order statuses
        match response {
            HyperliquidExchangeResponse::Status {
                status,
                response: response_data,
            } if status == "ok" => {
                // Extract the 'data' field from the response if it exists (new format)
                // Otherwise use response_data directly (old format)
                let data_value = if let Some(data) = response_data.get("data") {
                    data.clone()
                } else {
                    response_data
                };

                // Parse the response data to extract order statuses
                let order_response: HyperliquidExecOrderResponseData =
                    serde_json::from_value(data_value).map_err(|e| {
                        Error::bad_request(format!("Failed to parse order response: {e}"))
                    })?;

                let account_id = self
                    .account_id
                    .ok_or_else(|| Error::bad_request("Account ID not set"))?;
                let ts_init = nautilus_core::UnixNanos::default();

                // Validate we have the same number of statuses as orders submitted
                if order_response.statuses.len() != orders.len() {
                    return Err(Error::bad_request(format!(
                        "Mismatch between submitted orders ({}) and response statuses ({})",
                        orders.len(),
                        order_response.statuses.len()
                    )));
                }

                let mut reports = Vec::new();

                // Create OrderStatusReport for each order
                for (order, order_status) in orders.iter().zip(order_response.statuses.iter()) {
                    // Extract asset from instrument symbol
                    let instrument_id = order.instrument_id();
                    let symbol = instrument_id.symbol.as_str();
                    let asset = symbol.trim_end_matches("-PERP").trim_end_matches("-USD");

                    let product_type = HyperliquidProductType::from_symbol(symbol).ok();
                    let instrument = self
                        .get_or_create_instrument(&Ustr::from(asset), product_type)
                        .ok_or_else(|| {
                            Error::bad_request(format!("Instrument not found for {asset}"))
                        })?;

                    // Create OrderStatusReport based on the order status
                    let report = match order_status {
                        HyperliquidExecOrderStatus::Resting { resting } => {
                            // Order is resting on the order book
                            self.create_order_status_report(
                                order.instrument_id(),
                                Some(order.client_order_id()),
                                nautilus_model::identifiers::VenueOrderId::new(
                                    resting.oid.to_string(),
                                ),
                                order.order_side(),
                                order.order_type(),
                                order.quantity(),
                                order.time_in_force(),
                                order.price(),
                                order.trigger_price(),
                                nautilus_model::enums::OrderStatus::Accepted,
                                nautilus_model::types::Quantity::new(
                                    0.0,
                                    instrument.size_precision(),
                                ),
                                &instrument,
                                account_id,
                                ts_init,
                            )?
                        }
                        HyperliquidExecOrderStatus::Filled { filled } => {
                            // Order was filled immediately
                            let filled_qty = nautilus_model::types::Quantity::new(
                                filled.total_sz.to_string().parse::<f64>().unwrap_or(0.0),
                                instrument.size_precision(),
                            );
                            self.create_order_status_report(
                                order.instrument_id(),
                                Some(order.client_order_id()),
                                nautilus_model::identifiers::VenueOrderId::new(
                                    filled.oid.to_string(),
                                ),
                                order.order_side(),
                                order.order_type(),
                                order.quantity(),
                                order.time_in_force(),
                                order.price(),
                                order.trigger_price(),
                                nautilus_model::enums::OrderStatus::Filled,
                                filled_qty,
                                &instrument,
                                account_id,
                                ts_init,
                            )?
                        }
                        HyperliquidExecOrderStatus::Error { error } => {
                            return Err(Error::bad_request(format!(
                                "Order {} rejected: {error}",
                                order.client_order_id()
                            )));
                        }
                    };

                    reports.push(report);
                }

                Ok(reports)
            }
            HyperliquidExchangeResponse::Error { error } => Err(Error::bad_request(format!(
                "Order submission failed: {error}"
            ))),
            _ => Err(Error::bad_request("Unexpected response format")),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::MUTEX_POISONED;
    use nautilus_model::instruments::{Instrument, InstrumentAny};
    use rstest::rstest;
    use ustr::Ustr;

    use super::HyperliquidHttpClient;
    use crate::{common::enums::HyperliquidProductType, http::query::InfoRequest};

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

    #[rstest]
    fn test_cache_instrument_by_raw_symbol() {
        use nautilus_core::time::get_atomic_clock_realtime;
        use nautilus_model::{
            currencies::CURRENCY_MAP,
            enums::CurrencyType,
            identifiers::{InstrumentId, Symbol},
            instruments::CurrencyPair,
            types::{Currency, Price, Quantity},
        };

        let client = HyperliquidHttpClient::new(true, None, None).unwrap();

        // Create a test instrument with base currency "vntls:vCURSOR"
        let base_code = "vntls:vCURSOR";
        let quote_code = "USDC";

        // Register the custom currency
        {
            let mut currency_map = CURRENCY_MAP.lock().expect(MUTEX_POISONED);
            if !currency_map.contains_key(base_code) {
                currency_map.insert(
                    base_code.to_string(),
                    Currency::new(base_code, 8, 0, base_code, CurrencyType::Crypto),
                );
            }
        }

        let base_currency = Currency::new(base_code, 8, 0, base_code, CurrencyType::Crypto);
        let quote_currency = Currency::new(quote_code, 6, 0, quote_code, CurrencyType::Crypto);

        // Nautilus symbol is "vntls:vCURSOR-USDC-SPOT"
        let symbol = Symbol::new("vntls:vCURSOR-USDC-SPOT");
        let venue = *crate::common::consts::HYPERLIQUID_VENUE;
        let instrument_id = InstrumentId::new(symbol, venue);

        // raw_symbol is set to the base currency "vntls:vCURSOR" (see parse.rs)
        let raw_symbol = Symbol::new(base_code);

        let clock = get_atomic_clock_realtime();
        let ts = clock.get_time_ns();

        let instrument = InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            raw_symbol,
            base_currency,
            quote_currency,
            8,
            8,
            Price::from("0.00000001"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            ts,
            ts,
        ));

        // Cache the instrument
        client.cache_instrument(instrument.clone());

        // Verify it can be looked up by full symbol
        let instruments = client.instruments.read().unwrap();
        let by_full_symbol = instruments.get(&Ustr::from("vntls:vCURSOR-USDC-SPOT"));
        assert!(
            by_full_symbol.is_some(),
            "Instrument should be accessible by full symbol"
        );
        assert_eq!(by_full_symbol.unwrap().id(), instrument.id());

        // Verify it can be looked up by raw_symbol (coin) - backward compatibility
        let by_raw_symbol = instruments.get(&Ustr::from("vntls:vCURSOR"));
        assert!(
            by_raw_symbol.is_some(),
            "Instrument should be accessible by raw_symbol (Hyperliquid coin identifier)"
        );
        assert_eq!(by_raw_symbol.unwrap().id(), instrument.id());
        drop(instruments);

        // Verify it can be looked up by composite key (coin, product_type)
        let instruments_by_coin = client.instruments_by_coin.read().unwrap();
        let by_coin =
            instruments_by_coin.get(&(Ustr::from("vntls:vCURSOR"), HyperliquidProductType::Spot));
        assert!(
            by_coin.is_some(),
            "Instrument should be accessible by coin and product type"
        );
        assert_eq!(by_coin.unwrap().id(), instrument.id());
        drop(instruments_by_coin);

        // Verify get_or_create_instrument works with product type
        let retrieved_with_type = client.get_or_create_instrument(
            &Ustr::from("vntls:vCURSOR"),
            Some(HyperliquidProductType::Spot),
        );
        assert!(retrieved_with_type.is_some());
        assert_eq!(retrieved_with_type.unwrap().id(), instrument.id());

        // Verify get_or_create_instrument works without product type (fallback)
        let retrieved_without_type =
            client.get_or_create_instrument(&Ustr::from("vntls:vCURSOR"), None);
        assert!(retrieved_without_type.is_some());
        assert_eq!(retrieved_without_type.unwrap().id(), instrument.id());
    }
}
