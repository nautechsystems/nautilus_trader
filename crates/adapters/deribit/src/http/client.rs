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

//! Deribit HTTP client implementation.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use dashmap::DashMap;
use nautilus_core::{nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    http::{HttpClient, Method},
    retry::{RetryConfig, RetryManager},
};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::DeribitHttpError,
    models::{DeribitCurrency, DeribitInstrument, DeribitJsonRpcRequest, DeribitJsonRpcResponse},
    query::{GetInstrumentParams, GetInstrumentsParams},
};
use crate::common::{
    consts::{DERIBIT_API_PATH, JSONRPC_VERSION, should_retry_error_code},
    parse::parse_deribit_instrument_any,
    urls::get_http_base_url,
};

#[allow(dead_code)]
const DERIBIT_SUCCESS_CODE: i64 = 0;

/// Low-level Deribit HTTP client for raw API operations.
///
/// This client handles JSON-RPC 2.0 protocol, request signing, rate limiting,
/// and retry logic. It returns venue-specific response types.
#[derive(Debug)]
pub struct DeribitRawHttpClient {
    base_url: String,
    client: HttpClient,
    #[allow(dead_code)]
    retry_manager: RetryManager<DeribitHttpError>,
    cancellation_token: CancellationToken,
    request_id: AtomicU64,
}

impl DeribitRawHttpClient {
    /// Creates a new [`DeribitRawHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self, DeribitHttpError> {
        let base_url = format!("{}{}", get_http_base_url(is_testnet), DERIBIT_API_PATH);
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

        let retry_manager = RetryManager::new(retry_config);

        Ok(Self {
            base_url,
            client: HttpClient::new(
                std::collections::HashMap::new(), // headers
                Vec::new(),                       // header_keys
                Vec::new(),                       // keyed_quotas
                None,                             // default_quota
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            request_id: AtomicU64::new(1),
        })
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    #[doc(hidden)]
    /// Creates a test client with custom base URL (test only).
    pub fn new_with_base_url(
        base_url: String,
        timeout_secs: Option<u64>,
    ) -> Result<Self, DeribitHttpError> {
        let retry_config = RetryConfig {
            max_retries: 0,      // No retries in tests
            initial_delay_ms: 1, // Must be non-zero
            max_delay_ms: 1,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(5000),
            immediate_first: false,
            max_elapsed_ms: Some(10000),
        };

        Ok(Self {
            base_url,
            client: HttpClient::new(
                std::collections::HashMap::new(),
                Vec::new(),
                Vec::new(),
                None,
                timeout_secs,
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            retry_manager: RetryManager::new(retry_config),
            cancellation_token: CancellationToken::new(),
            request_id: AtomicU64::new(1),
        })
    }

    /// Sends a JSON-RPC 2.0 request to the Deribit API.
    async fn send_request<T, P>(&self, method: &str, params: P) -> Result<T, DeribitHttpError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        // Create operation identifier combining URL and RPC method
        let operation_id = format!("{}#{}", self.base_url, method);
        let operation = || {
            let method = method.to_string();
            let params_clone = serde_json::to_value(&params).unwrap();

            async move {
                // Build JSON-RPC request
                let id = self.request_id.fetch_add(1, Ordering::SeqCst);
                let request = DeribitJsonRpcRequest {
                    jsonrpc: JSONRPC_VERSION.to_string(),
                    id,
                    method: method.clone(),
                    params: params_clone.clone(),
                };

                let body = serde_json::to_vec(&request)?;

                // Execute HTTP POST
                let mut headers = std::collections::HashMap::new();
                headers.insert("Content-Type".to_string(), "application/json".to_string());

                let resp = self
                    .client
                    .request(
                        Method::POST,
                        self.base_url.clone(),
                        None,
                        Some(headers),
                        Some(body),
                        None,
                        None,
                    )
                    .await
                    .map_err(|e| DeribitHttpError::NetworkError(e.to_string()))?;

                // Parse JSON-RPC response
                // Note: Deribit may return JSON-RPC errors with non-2xx HTTP status (e.g., 400)
                // Always try to parse as JSON-RPC first, then fall back to HTTP error handling

                // Try to parse as JSON first
                let json_value: serde_json::Value = match serde_json::from_slice(&resp.body) {
                    Ok(json) => json,
                    Err(_) => {
                        // Not valid JSON - treat as HTTP error
                        let error_body = String::from_utf8_lossy(&resp.body);
                        tracing::error!(
                            method = %method,
                            status = resp.status.as_u16(),
                            "Non-JSON response: {error_body}"
                        );
                        return Err(DeribitHttpError::UnexpectedStatus {
                            status: resp.status.as_u16(),
                            body: error_body.to_string(),
                        });
                    }
                };

                // Try to parse as JSON-RPC response
                let json_rpc_response: DeribitJsonRpcResponse<T> =
                    serde_json::from_value(json_value.clone()).map_err(|e| {
                        tracing::error!(
                            method = %method,
                            status = resp.status.as_u16(),
                            error = %e,
                            "Failed to deserialize Deribit JSON-RPC response"
                        );
                        tracing::debug!(
                            "Response JSON (first 2000 chars): {}",
                            &json_value
                                .to_string()
                                .chars()
                                .take(2000)
                                .collect::<String>()
                        );
                        DeribitHttpError::JsonError(e.to_string())
                    })?;

                // Check if it's a success or error result
                if let Some(result) = json_rpc_response.result {
                    Ok(result)
                } else if let Some(error) = json_rpc_response.error {
                    // JSON-RPC error (may come with any HTTP status)
                    tracing::warn!(
                        method = %method,
                        http_status = resp.status.as_u16(),
                        error_code = error.code,
                        error_message = %error.message,
                        error_data = ?error.data,
                        "Deribit RPC error response"
                    );

                    // Map JSON-RPC error to appropriate error variant
                    Err(DeribitHttpError::from_jsonrpc_error(
                        error.code,
                        error.message,
                        error.data,
                    ))
                } else {
                    tracing::error!(
                        method = %method,
                        status = resp.status.as_u16(),
                        request_id = json_rpc_response.id,
                        "Response contains neither result nor error field"
                    );
                    Err(DeribitHttpError::JsonError(
                        "Response contains neither result nor error".to_string(),
                    ))
                }
            }
        };

        // Retry strategy based on Deribit error responses and HTTP status codes:
        //
        // 1. Network errors: always retry (transient connection issues)
        // 2. HTTP 5xx/429: server errors and rate limiting should be retried
        // 3. Deribit-specific retryable error codes (defined in common::consts)
        //
        // Note: Deribit returns many permanent errors which should NOT be retried
        // (e.g., "invalid_credentials", "not_enough_funds", "order_not_found")
        let should_retry = |error: &DeribitHttpError| -> bool {
            match error {
                DeribitHttpError::NetworkError(_) => true,
                DeribitHttpError::UnexpectedStatus { status, .. } => {
                    *status >= 500 || *status == 429
                }
                DeribitHttpError::DeribitError { error_code, .. } => {
                    should_retry_error_code(*error_code)
                }
                _ => false,
            }
        };

        let create_error = |msg: String| -> DeribitHttpError {
            if msg == "canceled" {
                DeribitHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                DeribitHttpError::NetworkError(msg)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                &operation_id,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Gets available trading instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_instruments(
        &self,
        params: GetInstrumentsParams,
    ) -> Result<Vec<DeribitInstrument>, DeribitHttpError> {
        self.send_request("public/get_instruments", params).await
    }

    /// Gets details for a specific trading instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_instrument(
        &self,
        params: GetInstrumentParams,
    ) -> Result<DeribitInstrument, DeribitHttpError> {
        self.send_request("public/get_instrument", params).await
    }
}

/// High-level Deribit HTTP client with domain-level abstractions.
///
/// This client wraps the raw HTTP client and provides methods that use Nautilus
/// domain types. It maintains an instrument cache for efficient lookups.
#[derive(Debug)]
pub struct DeribitHttpClient {
    pub(crate) inner: Arc<DeribitRawHttpClient>,
    pub(crate) instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    cache_initialized: AtomicBool,
}

impl DeribitHttpClient {
    /// Creates a new [`DeribitHttpClient`] with default configuration.
    ///
    /// # Parameters
    /// - `is_testnet`: Whether to use the testnet environment
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let raw_client = Arc::new(DeribitRawHttpClient::new(
            is_testnet,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            proxy_url,
        )?);

        Ok(Self {
            inner: raw_client,
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: AtomicBool::new(false),
        })
    }

    /// Requests instruments for a specific currency.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or instruments cannot be parsed.
    pub async fn request_instruments(
        &self,
        currency: DeribitCurrency,
        kind: Option<super::models::DeribitInstrumentKind>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        // Build parameters
        let params = if let Some(k) = kind {
            GetInstrumentsParams::with_kind(currency, k)
        } else {
            GetInstrumentsParams::new(currency)
        };

        // Call raw client
        let resp = self.inner.get_instruments(params).await?;

        // Use current timestamp for ts_init
        let ts_init = self.generate_ts_init();

        // Parse each instrument
        let mut instruments = Vec::new();
        let mut skipped_count = 0;
        let mut error_count = 0;

        for raw_instrument in resp {
            match parse_deribit_instrument_any(&raw_instrument, ts_init) {
                Ok(Some(instrument)) => {
                    instruments.push(instrument);
                }
                Ok(None) => {
                    // Unsupported instrument type (e.g., combos)
                    skipped_count += 1;
                    tracing::debug!(
                        "Skipped unsupported instrument type: {} (kind: {:?})",
                        raw_instrument.instrument_name,
                        raw_instrument.kind
                    );
                }
                Err(e) => {
                    error_count += 1;
                    tracing::warn!(
                        "Failed to parse instrument {}: {}",
                        raw_instrument.instrument_name,
                        e
                    );
                }
            }
        }

        tracing::info!(
            "Parsed {} instruments ({} skipped, {} errors)",
            instruments.len(),
            skipped_count,
            error_count
        );

        Ok(instruments)
    }

    /// Requests a specific instrument by its Nautilus instrument ID.
    ///
    /// This is a high-level method that fetches the raw instrument data from Deribit
    /// and converts it to a Nautilus `InstrumentAny` type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument name format is invalid (error code `-32602`)
    /// - The instrument doesn't exist (error code `13020`)
    /// - Network or API errors occur
    pub async fn request_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<InstrumentAny> {
        let params = GetInstrumentParams {
            instrument_name: instrument_id.symbol.to_string(),
        };

        let response = self.inner.get_instrument(params).await?;
        let ts_init = self.generate_ts_init();

        match parse_deribit_instrument_any(&response, ts_init)? {
            Some(instrument) => Ok(instrument),
            None => anyhow::bail!(
                "Unsupported instrument type: {} (kind: {:?})",
                response.instrument_name,
                response.kind
            ),
        }
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Caches instruments for later retrieval.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for inst in instruments {
            self.instruments_cache
                .insert(inst.raw_symbol().inner(), inst);
        }
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Retrieves a cached instrument by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache
            .get(symbol)
            .map(|entry| entry.value().clone())
    }

    /// Checks if the instrument cache has been initialized.
    #[must_use]
    pub fn is_cache_initialized(&self) -> bool {
        self.cache_initialized.load(Ordering::Acquire)
    }
}
