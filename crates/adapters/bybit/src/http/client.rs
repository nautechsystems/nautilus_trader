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

//! Provides the HTTP client integration for the [Bybit](https://bybit.com) REST API.
//!
//! Bybit API reference <https://bybit-exchange.github.io/docs/>.

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};

use nautilus_core::{consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, header::USER_AGENT};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;

use super::{
    error::BybitHttpError,
    models::{
        BybitInstrumentInverseResponse, BybitInstrumentLinearResponse,
        BybitInstrumentOptionResponse, BybitInstrumentSpotResponse, BybitKlinesResponse,
        BybitOpenOrdersResponse, BybitPlaceOrderResponse, BybitServerTimeResponse,
        BybitTradesResponse,
    },
    query::{BybitInstrumentsInfoParams, BybitKlinesParams, BybitTradesParams},
};
use crate::common::{
    consts::BYBIT_NAUTILUS_BROKER_ID,
    credential::Credential,
    enums::{BybitEnvironment, BybitProductType},
    models::BybitResponse,
    urls::bybit_http_base_url,
};

const DEFAULT_RECV_WINDOW_MS: u64 = 5_000;

/// Default Bybit REST API rate limit.
///
/// Bybit implements rate limiting per endpoint with varying limits.
/// We use a conservative 10 requests per second as a general default.
pub static BYBIT_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(10).expect("10 is a valid non-zero u32")));

const BYBIT_GLOBAL_RATE_KEY: &str = "bybit:global";

/// Inner HTTP client implementation containing the actual HTTP logic.
pub struct BybitHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    recv_window_ms: u64,
    retry_manager: RetryManager<BybitHttpError>,
    cancellation_token: CancellationToken,
}

impl Default for BybitHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None)
            .expect("Failed to create default BybitHttpInnerClient")
    }
}

impl Debug for BybitHttpInnerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BybitHttpInnerClient")
            .field("base_url", &self.base_url)
            .field("has_credentials", &self.credential.is_some())
            .field("recv_window_ms", &self.recv_window_ms)
            .finish()
    }
}

impl BybitHttpInnerClient {
    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Creates a new [`BybitHttpInnerClient`] using the default Bybit HTTP URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
        let retry_config = RetryConfig {
            max_retries: max_retries.unwrap_or(3),
            initial_delay_ms: retry_delay_ms.unwrap_or(1000),
            max_delay_ms: retry_delay_max_ms.unwrap_or(10_000),
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config).map_err(|e| {
            BybitHttpError::NetworkError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url: base_url
                .unwrap_or_else(|| bybit_http_base_url(BybitEnvironment::Mainnet).to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*BYBIT_REST_QUOTA),
                timeout_secs,
            ),
            credential: None,
            recv_window_ms: DEFAULT_RECV_WINDOW_MS,
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates a new [`BybitHttpInnerClient`] configured with credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: String,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
        let retry_config = RetryConfig {
            max_retries: max_retries.unwrap_or(3),
            initial_delay_ms: retry_delay_ms.unwrap_or(1000),
            max_delay_ms: retry_delay_max_ms.unwrap_or(10_000),
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config).map_err(|e| {
            BybitHttpError::NetworkError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*BYBIT_REST_QUOTA),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret)),
            recv_window_ms: DEFAULT_RECV_WINDOW_MS,
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Referer".to_string(), BYBIT_NAUTILUS_BROKER_ID.to_string()),
        ])
    }

    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![(BYBIT_GLOBAL_RATE_KEY.to_string(), *BYBIT_REST_QUOTA)]
    }

    fn rate_limit_keys(endpoint: &str) -> Vec<String> {
        let normalized = endpoint.split('?').next().unwrap_or(endpoint);
        let route = format!("bybit:{normalized}");

        vec![BYBIT_GLOBAL_RATE_KEY.to_string(), route]
    }

    fn sign_request(
        &self,
        timestamp: &str,
        params: Option<&str>,
    ) -> Result<HashMap<String, String>, BybitHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(BybitHttpError::MissingCredentials)?;

        let signature = credential.sign_with_payload(timestamp, self.recv_window_ms, params);

        let mut headers = HashMap::new();
        headers.insert(
            "X-BAPI-API-KEY".to_string(),
            credential.api_key().to_string(),
        );
        headers.insert("X-BAPI-TIMESTAMP".to_string(), timestamp.to_string());
        headers.insert("X-BAPI-SIGN".to_string(), signature);
        headers.insert(
            "X-BAPI-RECV-WINDOW".to_string(),
            self.recv_window_ms.to_string(),
        );

        Ok(headers)
    }

    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, BybitHttpError> {
        let endpoint = endpoint.to_string();
        let url = format!("{}{endpoint}", self.base_url);
        let method_clone = method.clone();
        let body_clone = body.clone();

        let operation = || {
            let url = url.clone();
            let method = method_clone.clone();
            let body = body_clone.clone();
            let endpoint = endpoint.clone();

            async move {
                let mut headers = Self::default_headers();

                if authenticate {
                    let timestamp = get_atomic_clock_realtime().get_time_ms().to_string();
                    let params_str = if method == Method::GET {
                        endpoint.split('?').nth(1)
                    } else {
                        body.as_ref().and_then(|b| std::str::from_utf8(b).ok())
                    };

                    let auth_headers = self.sign_request(&timestamp, params_str)?;
                    headers.extend(auth_headers);
                }

                if method == Method::POST || method == Method::PUT {
                    headers.insert("Content-Type".to_string(), "application/json".to_string());
                }

                let rate_limit_keys = Self::rate_limit_keys(&endpoint);

                let response = self
                    .client
                    .request(
                        method,
                        url,
                        Some(headers),
                        body,
                        None,
                        Some(rate_limit_keys),
                    )
                    .await?;

                if response.status.as_u16() >= 400 {
                    let body = String::from_utf8_lossy(&response.body).to_string();
                    return Err(BybitHttpError::UnexpectedStatus {
                        status: response.status.as_u16(),
                        body,
                    });
                }

                // Parse as BybitResponse to check retCode
                let bybit_response: BybitResponse<serde_json::Value> =
                    serde_json::from_slice(&response.body)?;

                if bybit_response.ret_code != 0 {
                    return Err(BybitHttpError::BybitError {
                        error_code: bybit_response.ret_code as i32,
                        message: bybit_response.ret_msg,
                    });
                }

                // Deserialize the full response
                let result: T = serde_json::from_slice(&response.body)?;
                Ok(result)
            }
        };

        let should_retry = |error: &BybitHttpError| -> bool {
            match error {
                BybitHttpError::NetworkError(_) => true,
                BybitHttpError::UnexpectedStatus { status, .. } => *status >= 500,
                _ => false,
            }
        };

        let create_error = |msg: String| -> BybitHttpError {
            if msg == "canceled" {
                BybitHttpError::NetworkError("Request canceled".to_string())
            } else {
                BybitHttpError::NetworkError(msg)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                endpoint.as_str(),
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await
    }

    fn build_path<S: Serialize>(base: &str, params: &S) -> Result<String, BybitHttpError> {
        let query = serde_urlencoded::to_string(params)
            .map_err(|e| BybitHttpError::JsonError(e.to_string()))?;
        if query.is_empty() {
            Ok(base.to_owned())
        } else {
            Ok(format!("{base}?{query}"))
        }
    }

    // =========================================================================
    // Low-level HTTP API methods
    // =========================================================================

    /// Fetches the current server time from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/time>
    pub async fn http_get_server_time(&self) -> Result<BybitServerTimeResponse, BybitHttpError> {
        self.send_request(Method::GET, "/v5/market/time", None, false)
            .await
    }

    /// Fetches instrument information from Bybit for a given product category.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments<T: DeserializeOwned>(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<T, BybitHttpError> {
        let path = Self::build_path("/v5/market/instruments-info", params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Fetches spot instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_spot(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentSpotResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches linear instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_linear(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentLinearResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches inverse instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_inverse(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentInverseResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches option instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_option(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentOptionResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches kline/candlestick data from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/kline>
    pub async fn http_get_klines(
        &self,
        params: &BybitKlinesParams,
    ) -> Result<BybitKlinesResponse, BybitHttpError> {
        let path = Self::build_path("/v5/market/kline", params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Fetches recent trades from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
    pub async fn http_get_recent_trades(
        &self,
        params: &BybitTradesParams,
    ) -> Result<BybitTradesResponse, BybitHttpError> {
        let path = Self::build_path("/v5/market/recent-trade", params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Fetches open orders (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/open-order>
    pub async fn http_get_open_orders(
        &self,
        category: BybitProductType,
        symbol: Option<&str>,
    ) -> Result<BybitOpenOrdersResponse, BybitHttpError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params<'a> {
            category: BybitProductType,
            #[serde(skip_serializing_if = "Option::is_none")]
            symbol: Option<&'a str>,
        }

        let params = Params { category, symbol };
        let path = Self::build_path("/v5/order/realtime", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Places a new order (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/create-order>
    pub async fn http_place_order(
        &self,
        request: &serde_json::Value,
    ) -> Result<BybitPlaceOrderResponse, BybitHttpError> {
        let body = serde_json::to_vec(request)?;
        self.send_request(Method::POST, "/v5/order/create", Some(body), true)
            .await
    }

    /// Returns the base URL used for requests.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the configured receive window in milliseconds.
    #[must_use]
    pub fn recv_window_ms(&self) -> u64 {
        self.recv_window_ms
    }

    /// Returns the API credential if configured.
    #[must_use]
    pub fn credential(&self) -> Option<&Credential> {
        self.credential.as_ref()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Outer Client
////////////////////////////////////////////////////////////////////////////////

/// Provides a HTTP client for connecting to the [Bybit](https://bybit.com) REST API.
#[derive(Clone)]
pub struct BybitHttpClient {
    pub(crate) inner: Arc<BybitHttpInnerClient>,
}

impl Default for BybitHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None)
            .expect("Failed to create default BybitHttpClient")
    }
}

impl Debug for BybitHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BybitHttpClient")
            .field("inner", &self.inner)
            .finish()
    }
}

impl BybitHttpClient {
    /// Creates a new [`BybitHttpClient`] using the default Bybit HTTP URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
        Ok(Self {
            inner: Arc::new(BybitHttpInnerClient::new(
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            )?),
        })
    }

    /// Creates a new [`BybitHttpClient`] configured with credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: String,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
        Ok(Self {
            inner: Arc::new(BybitHttpInnerClient::with_credentials(
                api_key,
                api_secret,
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            )?),
        })
    }

    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.inner.cancel_all_requests();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        self.inner.cancellation_token()
    }

    // =========================================================================
    // Low-level HTTP API methods
    // =========================================================================

    /// Fetches the current server time from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/time>
    pub async fn http_get_server_time(&self) -> Result<BybitServerTimeResponse, BybitHttpError> {
        self.inner.http_get_server_time().await
    }

    /// Fetches instrument information from Bybit for a given product category.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments<T: DeserializeOwned>(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<T, BybitHttpError> {
        self.inner.http_get_instruments(params).await
    }

    /// Fetches spot instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_spot(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentSpotResponse, BybitHttpError> {
        self.inner.http_get_instruments_spot(params).await
    }

    /// Fetches linear instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_linear(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentLinearResponse, BybitHttpError> {
        self.inner.http_get_instruments_linear(params).await
    }

    /// Fetches inverse instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_inverse(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentInverseResponse, BybitHttpError> {
        self.inner.http_get_instruments_inverse(params).await
    }

    /// Fetches option instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_option(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentOptionResponse, BybitHttpError> {
        self.inner.http_get_instruments_option(params).await
    }

    /// Fetches kline/candlestick data from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/kline>
    pub async fn http_get_klines(
        &self,
        params: &BybitKlinesParams,
    ) -> Result<BybitKlinesResponse, BybitHttpError> {
        self.inner.http_get_klines(params).await
    }

    /// Fetches recent trades from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
    pub async fn http_get_recent_trades(
        &self,
        params: &BybitTradesParams,
    ) -> Result<BybitTradesResponse, BybitHttpError> {
        self.inner.http_get_recent_trades(params).await
    }

    /// Fetches open orders (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/open-order>
    pub async fn http_get_open_orders(
        &self,
        category: BybitProductType,
        symbol: Option<&str>,
    ) -> Result<BybitOpenOrdersResponse, BybitHttpError> {
        self.inner.http_get_open_orders(category, symbol).await
    }

    /// Places a new order (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/create-order>
    pub async fn http_place_order(
        &self,
        request: &serde_json::Value,
    ) -> Result<BybitPlaceOrderResponse, BybitHttpError> {
        self.inner.http_place_order(request).await
    }

    // =========================================================================
    // High-level methods using Nautilus domain objects
    // =========================================================================
    // TODO: Implement submit_order, cancel_order, cancel_all_orders, etc.
    // These will take Nautilus domain types (InstrumentId, ClientOrderId, etc.)
    // and convert them to Bybit-specific types before calling the http_* methods

    /// Returns the base URL used for requests.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Returns the configured receive window in milliseconds.
    #[must_use]
    pub fn recv_window_ms(&self) -> u64 {
        self.inner.recv_window_ms()
    }

    /// Returns the API credential if configured.
    #[must_use]
    pub fn credential(&self) -> Option<&Credential> {
        self.inner.credential()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_client_creation() {
        let client = BybitHttpClient::new(None, Some(60), None, None, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.base_url().contains("bybit.com"));
        assert!(client.credential().is_none());
    }

    #[rstest]
    fn test_client_with_credentials() {
        let client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            "https://api-testnet.bybit.com".to_string(),
            Some(60),
            None,
            None,
            None,
        );
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.credential().is_some());
    }

    #[rstest]
    fn test_build_path_with_params() {
        #[derive(Serialize)]
        struct TestParams {
            category: String,
            symbol: String,
        }

        let params = TestParams {
            category: "linear".to_string(),
            symbol: "BTCUSDT".to_string(),
        };

        let path = BybitHttpInnerClient::build_path("/v5/market/test", &params);
        assert!(path.is_ok());
        assert!(path.unwrap().contains("category=linear"));
    }

    #[rstest]
    fn test_build_path_without_params() {
        let params = ();
        let path = BybitHttpInnerClient::build_path("/v5/market/time", &params);
        assert!(path.is_ok());
        assert_eq!(path.unwrap(), "/v5/market/time");
    }
}
