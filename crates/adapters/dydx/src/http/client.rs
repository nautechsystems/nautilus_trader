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

//! Provides an ergonomic wrapper around the **dYdX v4 Indexer REST API** –
//! <https://docs.dydx.exchange/api_integration-indexer/indexer_api>.
//!
//! The core type exported by this module is [`DydxRawHttpClient`]. It offers an
//! interface to all exchange endpoints currently required by NautilusTrader.
//!
//! Key responsibilities handled internally:
//! • Rate-limiting based on the public dYdX specification.
//! • Zero-copy deserialization of large JSON payloads into domain models.
//! • Conversion of raw exchange errors into the rich [`DydxHttpError`] enum.
//!
//! # Important Note
//!
//! The dYdX v4 Indexer REST API does **NOT** require authentication or request signing.
//! All endpoints are publicly accessible using only wallet addresses and subaccount numbers
//! as query parameters. Order submission and trading operations use gRPC with blockchain
//! transaction signing, not REST API.
//!
//! # Official documentation
//!
//! | Endpoint                             | Reference                                              |
//! |--------------------------------------|--------------------------------------------------------|
//! | Market data                          | <https://docs.dydx.exchange/api_integration-indexer/indexer_api#markets> |
//! | Account data                         | <https://docs.dydx.exchange/api_integration-indexer/indexer_api#accounts> |
//! | Utility endpoints                    | <https://docs.dydx.exchange/api_integration-indexer/indexer_api#utility> |

use std::{fmt::Debug, num::NonZeroU32, sync::LazyLock};

use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, header::USER_AGENT};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;

use super::error::DydxHttpError;
use crate::common::{
    consts::{DYDX_HTTP_URL, DYDX_TESTNET_HTTP_URL},
    enums::DydxCandleResolution,
};

/// Default dYdX Indexer REST API rate limit.
///
/// The dYdX Indexer API rate limits are generous for read-only operations:
/// - General: 100 requests per 10 seconds per IP
/// - We use a conservative 10 requests per second as the default quota.
pub static DYDX_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(10).unwrap()));

/// Represents a dYdX HTTP response wrapper.
///
/// Most dYdX Indexer API endpoints return data directly without a wrapper,
/// but some endpoints may use this structure for consistency.
#[derive(Debug, Serialize, Deserialize)]
pub struct DydxResponse<T> {
    /// The typed data returned by the dYdX endpoint.
    pub data: T,
}

/// Provides a raw HTTP client for interacting with the [dYdX v4](https://dydx.exchange) Indexer REST API.
///
/// This client wraps the underlying [`HttpClient`] to handle functionality
/// specific to dYdX Indexer API, such as rate-limiting, forming request URLs,
/// and deserializing responses into dYdX specific data models.
///
/// **Note**: Unlike traditional centralized exchanges, the dYdX v4 Indexer REST API
/// does NOT require authentication, API keys, or request signing. All endpoints are
/// publicly accessible.
pub struct DydxRawHttpClient {
    base_url: String,
    client: HttpClient,
    retry_manager: RetryManager<DydxHttpError>,
    cancellation_token: CancellationToken,
    is_testnet: bool,
}

impl Default for DydxRawHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, false, None)
            .expect("Failed to create default DydxRawHttpClient")
    }
}

impl Debug for DydxRawHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DydxRawHttpClient))
            .field("base_url", &self.base_url)
            .field("is_testnet", &self.is_testnet)
            .finish_non_exhaustive()
    }
}

impl DydxRawHttpClient {
    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Creates a new [`DydxRawHttpClient`] using the default dYdX Indexer HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// **Note**: No credentials are required as the dYdX Indexer API is publicly accessible.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        is_testnet: bool,
        retry_config: Option<RetryConfig>,
    ) -> anyhow::Result<Self> {
        let base_url = if is_testnet {
            base_url.unwrap_or_else(|| DYDX_TESTNET_HTTP_URL.to_string())
        } else {
            base_url.unwrap_or_else(|| DYDX_HTTP_URL.to_string())
        };

        let retry_manager = RetryManager::new(retry_config.unwrap_or_default())
            .map_err(|e| DydxHttpError::ValidationError(e.to_string()))?;

        // Build headers
        let mut headers = std::collections::HashMap::new();
        headers.insert(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string());

        let client = HttpClient::new(
            headers,
            vec![], // No specific headers to extract from responses
            vec![], // No keyed quotas (we use a single global quota)
            Some(*DYDX_REST_QUOTA),
            timeout_secs,
            proxy_url,
        )
        .map_err(|e| {
            DydxHttpError::ValidationError(format!("Failed to create HTTP client: {e}"))
        })?;

        Ok(Self {
            base_url,
            client,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            is_testnet,
        })
    }

    /// Check if this client is configured for testnet.
    #[must_use]
    pub const fn is_testnet(&self) -> bool {
        self.is_testnet
    }

    /// Get the base URL being used by this client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Send a request to a dYdX Indexer API endpoint.
    ///
    /// **Note**: dYdX Indexer API does not require authentication headers.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails
    /// - The response has a non-success HTTP status code
    /// - The response body cannot be deserialized to type `T`
    /// - The request is canceled
    pub async fn send_request<T>(
        &self,
        method: Method,
        endpoint: &str,
        query_params: Option<&str>,
    ) -> Result<T, DydxHttpError>
    where
        T: DeserializeOwned,
    {
        let url = if let Some(params) = query_params {
            format!("{}{endpoint}?{params}", self.base_url)
        } else {
            format!("{}{endpoint}", self.base_url)
        };

        let operation = || async {
            let request = self
                .client
                .request_with_ustr_keys(
                    method.clone(),
                    url.clone(),
                    None, // No additional headers
                    None, // No body for GET requests
                    None, // Use default timeout
                    None, // No specific rate limit keys (using global quota)
                )
                .await
                .map_err(|e| DydxHttpError::HttpClientError(e.to_string()))?;

            // Check for HTTP errors
            if !request.status.is_success() {
                return Err(DydxHttpError::HttpStatus {
                    status: request.status.as_u16(),
                    message: String::from_utf8_lossy(&request.body).to_string(),
                });
            }

            Ok(request)
        };

        // Retry strategy for dYdX Indexer API:
        // 1. Network errors: always retry (transient connection issues)
        // 2. HTTP 429/5xx: rate limiting and server errors should be retried
        // 3. Client errors (4xx except 429): should NOT be retried
        let should_retry = |error: &DydxHttpError| -> bool {
            match error {
                DydxHttpError::HttpClientError(_) => true,
                DydxHttpError::HttpStatus { status, .. } => *status == 429 || *status >= 500,
                _ => false,
            }
        };

        let create_error = |msg: String| -> DydxHttpError {
            if msg == "canceled" {
                DydxHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                DydxHttpError::ValidationError(msg)
            }
        };

        // Execute request with retry logic
        let response = self
            .retry_manager
            .execute_with_retry_with_cancel(
                endpoint,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await?;

        // Deserialize response
        serde_json::from_slice(&response.body).map_err(|e| DydxHttpError::Deserialization {
            error: e.to_string(),
            body: String::from_utf8_lossy(&response.body).to_string(),
        })
    }

    /// Send a POST request to a dYdX Indexer API endpoint.
    ///
    /// Note: Most dYdX Indexer endpoints are GET-based. POST is rarely used.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request body cannot be serialized to JSON
    /// - The HTTP request fails
    /// - The response has a non-success HTTP status code
    /// - The response body cannot be deserialized to type `T`
    /// - The request is canceled
    pub async fn send_post_request<T, B>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T, DydxHttpError>
    where
        T: DeserializeOwned,
        B: Serialize,
    {
        let url = format!("{}{endpoint}", self.base_url);

        let body_bytes = serde_json::to_vec(body).map_err(|e| DydxHttpError::Serialization {
            error: e.to_string(),
        })?;

        let operation = || async {
            let request = self
                .client
                .request_with_ustr_keys(
                    Method::POST,
                    url.clone(),
                    None, // No additional headers (content-type handled by body)
                    Some(body_bytes.clone()),
                    None, // Use default timeout
                    None, // No specific rate limit keys (using global quota)
                )
                .await
                .map_err(|e| DydxHttpError::HttpClientError(e.to_string()))?;

            // Check for HTTP errors
            if !request.status.is_success() {
                return Err(DydxHttpError::HttpStatus {
                    status: request.status.as_u16(),
                    message: String::from_utf8_lossy(&request.body).to_string(),
                });
            }

            Ok(request)
        };

        // Retry strategy (same as GET requests)
        let should_retry = |error: &DydxHttpError| -> bool {
            match error {
                DydxHttpError::HttpClientError(_) => true,
                DydxHttpError::HttpStatus { status, .. } => *status == 429 || *status >= 500,
                _ => false,
            }
        };

        let create_error = |msg: String| -> DydxHttpError {
            if msg == "canceled" {
                DydxHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                DydxHttpError::ValidationError(msg)
            }
        };

        // Execute request with retry logic
        let response = self
            .retry_manager
            .execute_with_retry_with_cancel(
                endpoint,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await?;

        // Deserialize response
        serde_json::from_slice(&response.body).map_err(|e| DydxHttpError::Deserialization {
            error: e.to_string(),
            body: String::from_utf8_lossy(&response.body).to_string(),
        })
    }

    // ========================================================================
    // Markets Endpoints
    // ========================================================================

    /// Fetch all perpetual markets.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_markets(&self) -> Result<super::models::MarketsResponse, DydxHttpError> {
        self.send_request(Method::GET, "/v4/perpetualMarkets", None)
            .await
    }

    /// Fetch orderbook for a specific market.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_orderbook(
        &self,
        ticker: &str,
    ) -> Result<super::models::OrderbookResponse, DydxHttpError> {
        let endpoint = format!("/v4/orderbooks/perpetualMarket/{ticker}");
        self.send_request(Method::GET, &endpoint, None).await
    }

    /// Fetch recent trades for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_trades(
        &self,
        ticker: &str,
        limit: Option<u32>,
    ) -> Result<super::models::TradesResponse, DydxHttpError> {
        let endpoint = format!("/v4/trades/perpetualMarket/{ticker}");
        let query = limit.map(|l| format!("limit={l}"));
        self.send_request(Method::GET, &endpoint, query.as_deref())
            .await
    }

    /// Fetch candles/klines for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_candles(
        &self,
        ticker: &str,
        resolution: DydxCandleResolution,
        limit: Option<u32>,
    ) -> Result<super::models::CandlesResponse, DydxHttpError> {
        let endpoint = format!("/v4/candles/perpetualMarkets/{ticker}");
        let mut query_parts = vec![format!("resolution={}", resolution)];
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, &endpoint, Some(&query))
            .await
    }

    // ========================================================================
    // Account Endpoints
    // ========================================================================

    /// Fetch subaccount information.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> Result<super::models::SubaccountResponse, DydxHttpError> {
        let endpoint = format!("/v4/addresses/{address}/subaccountNumber/{subaccount_number}");
        self.send_request(Method::GET, &endpoint, None).await
    }

    /// Fetch fills for a subaccount.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_fills(
        &self,
        address: &str,
        subaccount_number: u32,
        market: Option<&str>,
        limit: Option<u32>,
    ) -> Result<super::models::FillsResponse, DydxHttpError> {
        let endpoint = "/v4/fills";
        let mut query_parts = vec![
            format!("address={address}"),
            format!("subaccountNumber={subaccount_number}"),
        ];
        if let Some(m) = market {
            query_parts.push(format!("market={m}"));
        }
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, endpoint, Some(&query)).await
    }

    /// Fetch orders for a subaccount.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_orders(
        &self,
        address: &str,
        subaccount_number: u32,
        market: Option<&str>,
        limit: Option<u32>,
    ) -> Result<super::models::OrdersResponse, DydxHttpError> {
        let endpoint = "/v4/orders";
        let mut query_parts = vec![
            format!("address={address}"),
            format!("subaccountNumber={subaccount_number}"),
        ];
        if let Some(m) = market {
            query_parts.push(format!("market={m}"));
        }
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, endpoint, Some(&query)).await
    }

    /// Fetch transfers for a subaccount.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_transfers(
        &self,
        address: &str,
        subaccount_number: u32,
        limit: Option<u32>,
    ) -> Result<super::models::TransfersResponse, DydxHttpError> {
        let endpoint = "/v4/transfers";
        let mut query_parts = vec![
            format!("address={address}"),
            format!("subaccountNumber={subaccount_number}"),
        ];
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, endpoint, Some(&query)).await
    }

    // ========================================================================
    // Utility Endpoints
    // ========================================================================

    /// Get current server time.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_time(&self) -> Result<super::models::TimeResponse, DydxHttpError> {
        self.send_request(Method::GET, "/v4/time", None).await
    }

    /// Get current blockchain height.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_height(&self) -> Result<super::models::HeightResponse, DydxHttpError> {
        self.send_request(Method::GET, "/v4/height", None).await
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let client = DydxRawHttpClient::new(None, Some(30), None, false, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(!client.is_testnet());
        assert_eq!(client.base_url(), DYDX_HTTP_URL);
    }

    #[tokio::test]
    async fn test_testnet_client() {
        let client = DydxRawHttpClient::new(None, Some(30), None, true, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.is_testnet());
        assert_eq!(client.base_url(), DYDX_TESTNET_HTTP_URL);
    }
}
