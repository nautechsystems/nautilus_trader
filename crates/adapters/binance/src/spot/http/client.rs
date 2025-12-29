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

//! Binance Spot HTTP client with SBE encoding.
//!
//! This client communicates with Binance Spot REST API using SBE (Simple Binary
//! Encoding) for all request/response payloads, providing microsecond timestamp
//! precision and reduced latency compared to JSON.
//!
//! ## Architecture
//!
//! Two-layer client pattern:
//! - [`BinanceRawSpotHttpClient`]: Low-level API methods returning raw bytes.
//! - [`BinanceSpotHttpClient`]: High-level methods with SBE decoding.
//!
//! ## SBE Headers
//!
//! All requests include:
//! - `Accept: application/sbe`
//! - `X-MBX-SBE: 3:2` (schema ID:version)

use std::{collections::HashMap, num::NonZeroU32, sync::Arc};

use chrono::Utc;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{
    http::{HttpClient, HttpResponse, Method},
    ratelimiter::quota::Quota,
};
use serde::Serialize;

use super::{
    error::{BinanceSpotHttpError, BinanceSpotHttpResult},
    models::{BinanceDepth, BinanceTrades},
    parse,
    query::{DepthParams, TradesParams},
};
use crate::common::{
    consts::BINANCE_SPOT_RATE_LIMITS,
    credential::Credential,
    enums::{BinanceEnvironment, BinanceProductType},
    models::BinanceErrorResponse,
    sbe::spot::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION},
    urls::get_http_base_url,
};

/// SBE schema header value for Spot API.
pub const SBE_SCHEMA_HEADER: &str = "3:2";

/// Binance Spot API path.
const SPOT_API_PATH: &str = "/api/v3";

/// Global rate limit key.
const BINANCE_GLOBAL_RATE_KEY: &str = "binance:spot:global";

/// Orders rate limit key prefix.
const BINANCE_ORDERS_RATE_KEY: &str = "binance:spot:orders";

/// Low-level HTTP client for Binance Spot REST API with SBE encoding.
///
/// Handles:
/// - Base URL resolution by environment.
/// - Optional HMAC SHA256 signing for private endpoints.
/// - Rate limiting using Spot API quotas.
/// - SBE decoding to Binance-specific response types.
///
/// Methods are named to match Binance API endpoints and return
/// venue-specific types (decoded from SBE).
#[derive(Debug, Clone)]
pub struct BinanceRawSpotHttpClient {
    client: HttpClient,
    base_url: String,
    credential: Option<Credential>,
    recv_window: Option<u64>,
    order_rate_keys: Vec<String>,
}

impl BinanceRawSpotHttpClient {
    /// Creates a new Binance Spot raw HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`HttpClient`] fails to build.
    pub fn new(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceSpotHttpResult<Self> {
        let RateLimitConfig {
            default_quota,
            keyed_quotas,
            order_keys,
        } = Self::rate_limit_config();

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key, secret)),
            (None, None) => None,
            _ => return Err(BinanceSpotHttpError::MissingCredentials),
        };

        let base_url = base_url_override.unwrap_or_else(|| {
            get_http_base_url(BinanceProductType::Spot, environment).to_string()
        });

        let headers = Self::default_headers(&credential);

        let client = HttpClient::new(
            headers,
            vec!["X-MBX-APIKEY".to_string()],
            keyed_quotas,
            default_quota,
            timeout_secs,
            proxy_url,
        )?;

        Ok(Self {
            client,
            base_url,
            credential,
            recv_window,
            order_rate_keys: order_keys,
        })
    }

    /// Returns the SBE schema ID.
    #[must_use]
    pub const fn schema_id() -> u16 {
        SBE_SCHEMA_ID
    }

    /// Returns the SBE schema version.
    #[must_use]
    pub const fn schema_version() -> u16 {
        SBE_SCHEMA_VERSION
    }

    /// Performs a GET request and returns raw response bytes.
    pub async fn get<P>(&self, path: &str, params: Option<&P>) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::GET, path, params, false, false).await
    }

    /// Performs a signed GET request and returns raw response bytes.
    pub async fn get_signed<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::GET, path, params, true, false).await
    }

    /// Performs a signed POST request for order operations.
    pub async fn post_order<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::POST, path, params, true, true).await
    }

    /// Tests connectivity to the API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn ping(&self) -> BinanceSpotHttpResult<()> {
        let bytes = self.get("ping", None::<&()>).await?;
        parse::decode_ping(&bytes)?;
        Ok(())
    }

    /// Returns the server time in **microseconds** since epoch.
    ///
    /// Note: SBE provides microsecond precision vs JSON's milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn server_time(&self) -> BinanceSpotHttpResult<i64> {
        let bytes = self.get("time", None::<&()>).await?;
        let timestamp = parse::decode_server_time(&bytes)?;
        Ok(timestamp)
    }

    /// Returns order book depth for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn depth(&self, params: &DepthParams) -> BinanceSpotHttpResult<BinanceDepth> {
        let bytes = self.get("depth", Some(params)).await?;
        let depth = parse::decode_depth(&bytes)?;
        Ok(depth)
    }

    /// Returns recent trades for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn trades(&self, params: &TradesParams) -> BinanceSpotHttpResult<BinanceTrades> {
        let bytes = self.get("trades", Some(params)).await?;
        let trades = parse::decode_trades(&bytes)?;
        Ok(trades)
    }

    async fn request<P>(
        &self,
        method: Method,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        let mut query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let mut headers = HashMap::new();
        if signed {
            let cred = self
                .credential
                .as_ref()
                .ok_or(BinanceSpotHttpError::MissingCredentials)?;

            if !query.is_empty() {
                query.push('&');
            }

            let timestamp = Utc::now().timestamp_millis();
            query.push_str(&format!("timestamp={timestamp}"));

            if let Some(recv_window) = self.recv_window {
                query.push_str(&format!("&recvWindow={recv_window}"));
            }

            let signature = cred.sign(&query);
            query.push_str(&format!("&signature={signature}"));
            headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());
        }

        let url = self.build_url(path, &query);
        let keys = self.rate_limit_keys(use_order_quota);

        let response = self
            .client
            .request(
                method,
                url,
                None::<&HashMap<String, Vec<String>>>,
                Some(headers),
                None,
                None,
                Some(keys),
            )
            .await?;

        if !response.status.is_success() {
            return self.parse_error_response(response);
        }

        Ok(response.body.to_vec())
    }

    fn build_url(&self, path: &str, query: &str) -> String {
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let mut url = format!("{}{}{}", self.base_url, SPOT_API_PATH, normalized_path);
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        url
    }

    fn rate_limit_keys(&self, use_orders: bool) -> Vec<String> {
        if use_orders {
            let mut keys = Vec::with_capacity(1 + self.order_rate_keys.len());
            keys.push(BINANCE_GLOBAL_RATE_KEY.to_string());
            keys.extend(self.order_rate_keys.iter().cloned());
            keys
        } else {
            vec![BINANCE_GLOBAL_RATE_KEY.to_string()]
        }
    }

    fn parse_error_response<T>(&self, response: HttpResponse) -> BinanceSpotHttpResult<T> {
        let status = response.status.as_u16();
        let body_hex = hex::encode(&response.body);

        // Binance may return JSON errors even when SBE was requested
        if let Ok(body_str) = std::str::from_utf8(&response.body)
            && let Ok(err) = serde_json::from_str::<BinanceErrorResponse>(body_str)
        {
            return Err(BinanceSpotHttpError::BinanceError {
                code: err.code,
                message: err.msg,
            });
        }

        Err(BinanceSpotHttpError::UnexpectedStatus {
            status,
            body: body_hex,
        })
    }

    fn default_headers(credential: &Option<Credential>) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string());
        headers.insert("Accept".to_string(), "application/sbe".to_string());
        headers.insert("X-MBX-SBE".to_string(), SBE_SCHEMA_HEADER.to_string());
        if let Some(cred) = credential {
            headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());
        }
        headers
    }

    fn rate_limit_config() -> RateLimitConfig {
        let quotas = BINANCE_SPOT_RATE_LIMITS;
        let mut keyed = Vec::new();
        let mut order_keys = Vec::new();
        let mut default = None;

        for quota in quotas {
            if let Some(q) = Self::quota_from(quota) {
                if quota.rate_limit_type == "REQUEST_WEIGHT" && default.is_none() {
                    default = Some(q);
                } else if quota.rate_limit_type == "ORDERS" {
                    let key = format!("{}:{}", BINANCE_ORDERS_RATE_KEY, quota.interval);
                    order_keys.push(key.clone());
                    keyed.push((key, q));
                }
            }
        }

        let default_quota =
            default.unwrap_or_else(|| Quota::per_second(NonZeroU32::new(10).unwrap()));

        keyed.push((BINANCE_GLOBAL_RATE_KEY.to_string(), default_quota));

        RateLimitConfig {
            default_quota: Some(default_quota),
            keyed_quotas: keyed,
            order_keys,
        }
    }

    fn quota_from(quota: &crate::common::consts::BinanceRateLimitQuota) -> Option<Quota> {
        let burst = NonZeroU32::new(quota.limit)?;
        match quota.interval {
            "SECOND" => Some(Quota::per_second(burst)),
            "MINUTE" => Some(Quota::per_minute(burst)),
            "DAY" => Quota::with_period(std::time::Duration::from_secs(86_400))
                .map(|q| q.allow_burst(burst)),
            _ => None,
        }
    }
}

struct RateLimitConfig {
    default_quota: Option<Quota>,
    keyed_quotas: Vec<(String, Quota)>,
    order_keys: Vec<String>,
}

/// High-level HTTP client for Binance Spot API.
///
/// Wraps [`BinanceRawSpotHttpClient`] and provides domain-level methods:
/// - Simple types (ping, server_time): Pass through from raw client.
/// - Complex types (instruments, orders): Transform to Nautilus domain types.
#[derive(Debug, Clone)]
pub struct BinanceSpotHttpClient {
    inner: Arc<BinanceRawSpotHttpClient>,
}

impl BinanceSpotHttpClient {
    /// Creates a new Binance Spot HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceSpotHttpResult<Self> {
        let inner = BinanceRawSpotHttpClient::new(
            environment,
            api_key,
            api_secret,
            base_url_override,
            recv_window,
            timeout_secs,
            proxy_url,
        )?;

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Returns a reference to the inner raw client.
    #[must_use]
    pub fn inner(&self) -> &BinanceRawSpotHttpClient {
        &self.inner
    }

    /// Returns the SBE schema ID.
    #[must_use]
    pub const fn schema_id() -> u16 {
        SBE_SCHEMA_ID
    }

    /// Returns the SBE schema version.
    #[must_use]
    pub const fn schema_version() -> u16 {
        SBE_SCHEMA_VERSION
    }

    /// Tests connectivity to the API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn ping(&self) -> BinanceSpotHttpResult<()> {
        self.inner.ping().await
    }

    /// Returns the server time in **microseconds** since epoch.
    ///
    /// Note: SBE provides microsecond precision vs JSON's milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn server_time(&self) -> BinanceSpotHttpResult<i64> {
        self.inner.server_time().await
    }

    /// Returns order book depth for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn depth(&self, params: &DepthParams) -> BinanceSpotHttpResult<BinanceDepth> {
        self.inner.depth(params).await
    }

    /// Returns recent trades for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn trades(&self, params: &TradesParams) -> BinanceSpotHttpResult<BinanceTrades> {
        self.inner.trades(params).await
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_schema_constants() {
        assert_eq!(BinanceRawSpotHttpClient::schema_id(), 3);
        assert_eq!(BinanceRawSpotHttpClient::schema_version(), 2);
        assert_eq!(BinanceSpotHttpClient::schema_id(), 3);
        assert_eq!(BinanceSpotHttpClient::schema_version(), 2);
    }

    #[rstest]
    fn test_sbe_schema_header() {
        assert_eq!(SBE_SCHEMA_HEADER, "3:2");
    }

    #[rstest]
    fn test_default_headers_include_sbe() {
        let headers = BinanceRawSpotHttpClient::default_headers(&None);

        assert_eq!(headers.get("Accept"), Some(&"application/sbe".to_string()));
        assert_eq!(headers.get("X-MBX-SBE"), Some(&"3:2".to_string()));
    }

    #[rstest]
    fn test_rate_limit_config() {
        let config = BinanceRawSpotHttpClient::rate_limit_config();

        assert!(config.default_quota.is_some());
        // Spot has 2 ORDERS quotas (SECOND and DAY)
        assert_eq!(config.order_keys.len(), 2);
    }
}
