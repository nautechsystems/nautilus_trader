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

//! Betfair HTTP client implementation.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use nautilus_network::{
    http::{HttpClient, Method},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;

use super::{
    error::BetfairHttpError,
    models::{LoginResponse, LoginStatus},
};
use crate::common::{
    consts::{
        BETFAIR_ACCOUNTS_URL, BETFAIR_BETTING_URL, BETFAIR_IDENTITY_LOGIN_URL,
        BETFAIR_KEEP_ALIVE_URL, BETFAIR_NAVIGATION_URL, BETFAIR_RATE_LIMIT_DEFAULT,
        BETFAIR_RATE_LIMIT_ORDERS,
    },
    credential::BetfairCredential,
};

/// Betfair JSON-RPC request envelope.
#[derive(Debug, Serialize)]
struct JsonRpcRequest<P: Serialize> {
    jsonrpc: &'static str,
    method: String,
    params: P,
    id: u64,
}

/// Betfair JSON-RPC response envelope.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

/// JSON-RPC error object.
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

/// Betfair HTTP client for raw API operations.
///
/// Handles session-token authentication, JSON-RPC protocol, form-encoded
/// identity requests, REST navigation, rate limiting, and retry logic.
#[derive(Debug)]
pub struct BetfairHttpClient {
    client: HttpClient,
    credential: BetfairCredential,
    session_token: Arc<tokio::sync::RwLock<Option<String>>>,
    retry_manager: RetryManager<BetfairHttpError>,
    cancellation_token: CancellationToken,
    request_id: AtomicU64,
}

impl BetfairHttpClient {
    /// Creates a new [`BetfairHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        credential: BetfairCredential,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self, BetfairHttpError> {
        let retry_config = RetryConfig {
            max_retries: max_retries.unwrap_or(3),
            initial_delay_ms: retry_delay_ms.unwrap_or(1000),
            max_delay_ms: 10_000,
            backoff_factor: 2.0,
            jitter_ms: 500,
            operation_timeout_ms: Some(30_000),
            immediate_first: false,
            max_elapsed_ms: Some(120_000),
        };

        Ok(Self {
            client: HttpClient::new(
                HashMap::new(),
                Vec::new(),
                Self::rate_limiter_quotas(),
                Self::default_quota(),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential,
            session_token: Arc::new(tokio::sync::RwLock::new(None)),
            retry_manager: RetryManager::new(retry_config),
            cancellation_token: CancellationToken::new(),
            request_id: AtomicU64::new(1),
        })
    }

    /// Returns the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Returns the current session token, if authenticated.
    pub async fn session_token(&self) -> Option<String> {
        self.session_token.read().await.clone()
    }

    /// Returns whether the client has an active session.
    pub async fn is_connected(&self) -> bool {
        self.session_token.read().await.is_some()
    }

    /// Returns the application key.
    #[must_use]
    pub fn app_key(&self) -> &str {
        self.credential.app_key()
    }

    /// Authenticates with Betfair using interactive (non-cert) login.
    ///
    /// Sends credentials to the Identity API and stores the returned
    /// session token for subsequent requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the login request fails or authentication
    /// is rejected.
    pub async fn connect(&self) -> Result<(), BetfairHttpError> {
        let form_body = format!(
            "username={}&password={}",
            urlencoding::encode(self.credential.username()),
            urlencoding::encode(self.credential.password()),
        );

        let resp_bytes = self
            .send_identity(BETFAIR_IDENTITY_LOGIN_URL, form_body.into_bytes())
            .await?;

        let resp: LoginResponse = serde_json::from_slice(&resp_bytes)?;

        if resp.status == LoginStatus::Success {
            log::info!("Betfair login successful");
            *self.session_token.write().await = Some(resp.token);
            Ok(())
        } else {
            Err(BetfairHttpError::LoginFailed {
                status: resp.error.unwrap_or_else(|| format!("{:?}", resp.status)),
            })
        }
    }

    /// Resets the session and re-authenticates.
    ///
    /// # Errors
    ///
    /// Returns an error if re-authentication fails.
    pub async fn reconnect(&self) -> Result<(), BetfairHttpError> {
        log::info!("Betfair reconnecting...");
        *self.session_token.write().await = None;
        self.connect().await
    }

    /// Clears the session token.
    pub async fn disconnect(&self) {
        log::info!("Betfair disconnecting...");
        *self.session_token.write().await = None;
    }

    /// Sends a keep-alive request to renew the session.
    ///
    /// # Errors
    ///
    /// Returns an error if the keep-alive request fails.
    pub async fn keep_alive(&self) -> Result<(), BetfairHttpError> {
        let resp_bytes = self
            .send_identity(BETFAIR_KEEP_ALIVE_URL, Vec::new())
            .await?;

        let resp: LoginResponse = serde_json::from_slice(&resp_bytes)?;

        if resp.status == LoginStatus::Success {
            *self.session_token.write().await = Some(resp.token);
            Ok(())
        } else {
            Err(BetfairHttpError::LoginFailed {
                status: resp.error.unwrap_or_else(|| format!("{:?}", resp.status)),
            })
        }
    }

    /// Sends a JSON-RPC request to the Betting API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, authentication is missing,
    /// or the response contains a JSON-RPC error.
    pub async fn send_betting<T, P>(&self, method: &str, params: P) -> Result<T, BetfairHttpError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        self.send_jsonrpc(BETFAIR_BETTING_URL, method, params, false)
            .await
    }

    /// Sends a JSON-RPC request to the Betting API with order rate limiting.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, authentication is missing,
    /// or the response contains a JSON-RPC error.
    pub async fn send_betting_order<T, P>(
        &self,
        method: &str,
        params: P,
    ) -> Result<T, BetfairHttpError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        self.send_jsonrpc(BETFAIR_BETTING_URL, method, params, true)
            .await
    }

    /// Sends a JSON-RPC request to the Accounts API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, authentication is missing,
    /// or the response contains a JSON-RPC error.
    pub async fn send_accounts<T, P>(&self, method: &str, params: P) -> Result<T, BetfairHttpError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        self.send_jsonrpc(BETFAIR_ACCOUNTS_URL, method, params, false)
            .await
    }

    /// Sends a GET request to the Navigation API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn send_navigation<T>(&self) -> Result<T, BetfairHttpError>
    where
        T: DeserializeOwned,
    {
        let headers = self.build_headers("application/json").await?;

        let resp = self
            .client
            .request(
                Method::GET,
                BETFAIR_NAVIGATION_URL.to_string(),
                None,
                Some(headers),
                None,
                None,
                Some(vec![BETFAIR_RATE_LIMIT_DEFAULT.to_string()]),
            )
            .await
            .map_err(|e| BetfairHttpError::NetworkError(e.to_string()))?;

        if resp.status.as_u16() != 200 {
            let body = String::from_utf8_lossy(&resp.body);
            return Err(BetfairHttpError::UnexpectedStatus {
                status: resp.status.as_u16(),
                body: body.to_string(),
            });
        }

        serde_json::from_slice(&resp.body).map_err(BetfairHttpError::from)
    }

    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![
            (
                BETFAIR_RATE_LIMIT_DEFAULT.to_string(),
                Quota::per_second(NonZeroU32::new(5).expect("non-zero")).expect("valid constant"),
            ),
            (
                BETFAIR_RATE_LIMIT_ORDERS.to_string(),
                Quota::per_second(NonZeroU32::new(20).expect("non-zero")).expect("valid constant"),
            ),
        ]
    }

    fn default_quota() -> Option<Quota> {
        Some(Quota::per_second(NonZeroU32::new(5).expect("non-zero")).expect("valid constant"))
    }

    async fn build_headers(
        &self,
        content_type: &str,
    ) -> Result<HashMap<String, String>, BetfairHttpError> {
        let token = self
            .session_token
            .read()
            .await
            .clone()
            .ok_or(BetfairHttpError::MissingCredentials)?;

        let mut headers = HashMap::new();
        headers.insert("X-Authentication".to_string(), token);
        headers.insert(
            "X-Application".to_string(),
            self.credential.app_key().to_string(),
        );
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("Content-Type".to_string(), content_type.to_string());
        Ok(headers)
    }

    async fn send_identity(&self, url: &str, body: Vec<u8>) -> Result<Vec<u8>, BetfairHttpError> {
        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        headers.insert(
            "X-Application".to_string(),
            self.credential.app_key().to_string(),
        );

        // Add session token if we have one (for keep-alive)
        if let Some(token) = self.session_token.read().await.as_ref() {
            headers.insert("X-Authentication".to_string(), token.clone());
        }

        let resp = self
            .client
            .request(
                Method::POST,
                url.to_string(),
                None,
                Some(headers),
                Some(body),
                None,
                Some(vec![BETFAIR_RATE_LIMIT_DEFAULT.to_string()]),
            )
            .await
            .map_err(|e| BetfairHttpError::NetworkError(e.to_string()))?;

        if resp.status.as_u16() != 200 {
            let body = String::from_utf8_lossy(&resp.body);
            return Err(BetfairHttpError::UnexpectedStatus {
                status: resp.status.as_u16(),
                body: body.to_string(),
            });
        }

        Ok(resp.body.to_vec())
    }

    async fn send_jsonrpc<T, P>(
        &self,
        base_url: &str,
        method: &str,
        params: P,
        is_order: bool,
    ) -> Result<T, BetfairHttpError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let operation_id = format!("{base_url}#{method}");
        let params_value = serde_json::to_value(&params)?;

        let operation = || {
            let method = method.to_string();
            let params_value = params_value.clone();

            async move {
                let id = self.request_id.fetch_add(1, Ordering::SeqCst);
                let request = JsonRpcRequest {
                    jsonrpc: "2.0",
                    method: method.clone(),
                    params: params_value.clone(),
                    id,
                };

                let body = serde_json::to_vec(&request)?;
                let headers = self.build_headers("application/json").await?;

                let rate_keys = if is_order {
                    vec![BETFAIR_RATE_LIMIT_ORDERS.to_string()]
                } else {
                    vec![BETFAIR_RATE_LIMIT_DEFAULT.to_string()]
                };

                let resp = self
                    .client
                    .request(
                        Method::POST,
                        base_url.to_string(),
                        None,
                        Some(headers),
                        Some(body),
                        None,
                        Some(rate_keys),
                    )
                    .await
                    .map_err(|e| BetfairHttpError::NetworkError(e.to_string()))?;

                let json_value: serde_json::Value = match serde_json::from_slice(&resp.body) {
                    Ok(json) => json,
                    Err(_) => {
                        let error_body = String::from_utf8_lossy(&resp.body);
                        let preview: String = error_body.chars().take(500).collect();
                        log::error!(
                            "Non-JSON response: method={method}, status={}, body={}",
                            resp.status.as_u16(),
                            preview,
                        );
                        return Err(BetfairHttpError::UnexpectedStatus {
                            status: resp.status.as_u16(),
                            body: error_body.to_string(),
                        });
                    }
                };

                let rpc_resp: JsonRpcResponse<T> =
                    serde_json::from_value(json_value).map_err(|e| {
                        log::error!(
                            "Failed to deserialize JSON-RPC response: method={method}, error={e}",
                        );
                        BetfairHttpError::JsonError(e.to_string())
                    })?;

                if let Some(result) = rpc_resp.result {
                    Ok(result)
                } else if let Some(error) = rpc_resp.error {
                    Err(BetfairHttpError::BetfairError {
                        code: error.code,
                        message: error.message,
                    })
                } else {
                    Err(BetfairHttpError::JsonError(
                        "Response contains neither result nor error".to_string(),
                    ))
                }
            }
        };

        let should_retry = |error: &BetfairHttpError| -> bool { error.is_retryable() };

        let create_error = |msg: String| -> BetfairHttpError {
            if msg == "canceled" {
                BetfairHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                BetfairHttpError::NetworkError(msg)
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
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::consts::{BETFAIR_RATE_LIMIT_DEFAULT, BETFAIR_RATE_LIMIT_ORDERS};

    #[rstest]
    fn test_rate_limiter_quotas_has_expected_keys() {
        let quotas = BetfairHttpClient::rate_limiter_quotas();
        let keys: Vec<&str> = quotas.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&BETFAIR_RATE_LIMIT_DEFAULT));
        assert!(keys.contains(&BETFAIR_RATE_LIMIT_ORDERS));
    }

    #[rstest]
    fn test_default_quota_is_some() {
        assert!(BetfairHttpClient::default_quota().is_some());
    }

    #[rstest]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: "SportsAPING/v1.0/listMarketCatalogue".to_string(),
            params: serde_json::json!({"filter": {}, "maxResults": 100}),
            id: 1,
        };

        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["jsonrpc"], "2.0");
        assert_eq!(json["method"], "SportsAPING/v1.0/listMarketCatalogue");
        assert_eq!(json["params"]["maxResults"], 100);
        assert_eq!(json["id"], 1);
    }

    #[rstest]
    fn test_json_rpc_response_success() {
        let json = r#"{"result": [1, 2, 3], "error": null}"#;
        let resp: JsonRpcResponse<Vec<i32>> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.result, Some(vec![1, 2, 3]));
        assert!(resp.error.is_none());
    }

    #[rstest]
    fn test_json_rpc_response_error() {
        let json = r#"{"result": null, "error": {"code": -32600, "message": "Invalid request"}}"#;
        let resp: JsonRpcResponse<serde_json::Value> = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let error = resp.error.unwrap();
        assert_eq!(error.code, -32600);
        assert_eq!(error.message, "Invalid request");
    }
}
