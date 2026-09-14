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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use nautilus_core::string::{secret::SecretString, urlencoding};
use nautilus_network::{
    http::{HttpClient, Method},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryError, RetryManager},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use super::{
    error::BetfairHttpError,
    models::{LoginResponse, LoginStatus},
};
use crate::common::{
    consts::{
        BETFAIR_ACCOUNTS_URL, BETFAIR_BETTING_URL, BETFAIR_IDENTITY_LOGIN_URL,
        BETFAIR_KEEP_ALIVE_URL, BETFAIR_NAVIGATION_URL, BETFAIR_RATE_LIMIT_DEFAULT,
        BETFAIR_RATE_LIMIT_ORDERS, HEADER_X_APPLICATION, HEADER_X_AUTHENTICATION,
    },
    credential::BetfairCredential,
};

// Keep the final dispatch 15 seconds inside Betfair's 60-second customerRef window
const ORDER_RETRY_MAX_ELAPSED_MS: u64 = 45_000;

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
    data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JsonRpcApiException {
    error_code: String,
    error_details: Option<String>,
}

impl JsonRpcError {
    fn api_exception(&self) -> Option<JsonRpcApiException> {
        let data = self.data.as_ref()?;
        let exception_name = data.get("exceptionname")?.as_str()?;
        serde_json::from_value(data.get(exception_name)?.clone()).ok()
    }
}

/// Betfair HTTP client for raw API operations.
///
/// Handles session-token authentication, JSON-RPC protocol, form-encoded
/// identity requests, REST navigation, rate limiting, and retry logic.
#[derive(Debug)]
pub struct BetfairHttpClient {
    client: HttpClient,
    credential: BetfairCredential,
    session_token: Arc<tokio::sync::RwLock<Option<SecretString>>>,
    retry_manager: RetryManager<BetfairHttpError>,
    order_retry_manager: RetryManager<BetfairHttpError>,
    cancellation_token: parking_lot::Mutex<CancellationToken>,
    connect_lock: tokio::sync::Mutex<()>,
    request_id: AtomicU64,
    url_identity_login: String,
    url_keep_alive: String,
    url_betting: String,
    url_accounts: String,
    url_navigation: String,
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
        request_rate_per_second: Option<u32>,
        order_request_rate_per_second: Option<u32>,
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
        let order_retry_config = RetryConfig {
            max_elapsed_ms: Some(ORDER_RETRY_MAX_ELAPSED_MS),
            ..retry_config
        };

        Ok(Self {
            client: HttpClient::builder()
                .keyed_quotas(Self::rate_limiter_quotas(
                    request_rate_per_second.unwrap_or(5),
                    order_request_rate_per_second.unwrap_or(20),
                )?)
                .maybe_default_quota(Self::default_quota(request_rate_per_second.unwrap_or(5))?)
                .maybe_timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential,
            session_token: Arc::new(tokio::sync::RwLock::new(None)),
            retry_manager: RetryManager::new(retry_config),
            order_retry_manager: RetryManager::new(order_retry_config),
            cancellation_token: parking_lot::Mutex::new(CancellationToken::new()),
            connect_lock: tokio::sync::Mutex::new(()),
            request_id: AtomicU64::new(1),
            url_identity_login: BETFAIR_IDENTITY_LOGIN_URL.to_string(),
            url_keep_alive: BETFAIR_KEEP_ALIVE_URL.to_string(),
            url_betting: BETFAIR_BETTING_URL.to_string(),
            url_accounts: BETFAIR_ACCOUNTS_URL.to_string(),
            url_navigation: BETFAIR_NAVIGATION_URL.to_string(),
        })
    }

    /// Overrides the API base URLs (for testing with mock servers).
    ///
    /// The keep-alive URL is derived from `identity_login` by replacing the
    /// path with `/keepAlive`.
    #[must_use]
    pub fn with_urls(
        mut self,
        identity_login: String,
        betting: String,
        accounts: String,
        navigation: String,
    ) -> Self {
        // Derive keep-alive from same host as login
        if let Some(base) = identity_login.rfind('/') {
            self.url_keep_alive = format!("{}/keepAlive", &identity_login[..base]);
        }
        self.url_identity_login = identity_login;
        self.url_betting = betting;
        self.url_accounts = accounts;
        self.url_navigation = navigation;
        self
    }

    /// Returns a clone of the current cancellation token for this client.
    ///
    /// `disconnect()` cancels and replaces the token, so callers should fetch
    /// a fresh clone for each operation rather than holding one long-term.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.lock().clone()
    }

    /// Returns the current session token, if authenticated.
    pub async fn session_token(&self) -> Option<SecretString> {
        self.session_token.read().await.clone()
    }

    /// Runs a synchronous token publication while session mutation is serialized.
    pub(crate) async fn with_session_token<T>(
        &self,
        publish: impl FnOnce(&SecretString) -> T,
    ) -> Option<T> {
        let _guard = self.connect_lock.lock().await;
        self.session_token.read().await.as_ref().map(publish)
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
        // Serialize session mutations so an older keep-alive response cannot overwrite a newer
        // full login. This matches Python's per-instance asyncio.Lock around the same path.
        let _guard = self.connect_lock.lock().await;

        if self.session_token.read().await.is_some() {
            log::debug!("Session token exists (already connected), skipping");
            return Ok(());
        }

        let token = self.login().await?;
        *self.session_token.write().await = Some(token);
        Ok(())
    }

    /// Resets the session and re-authenticates.
    ///
    /// # Errors
    ///
    /// Returns an error if re-authentication fails.
    pub async fn reconnect(&self) -> Result<(), BetfairHttpError> {
        self.reconnect_with_token().await.map(drop)
    }

    /// Resets the session, re-authenticates, and returns the replacement token.
    ///
    /// # Errors
    ///
    /// Returns an error if re-authentication fails.
    pub(crate) async fn reconnect_with_token(&self) -> Result<SecretString, BetfairHttpError> {
        let _guard = self.connect_lock.lock().await;
        log::info!("Betfair reconnecting...");
        *self.session_token.write().await = None;
        let token = self.login().await?;
        *self.session_token.write().await = Some(token.clone());
        Ok(token)
    }

    async fn login(&self) -> Result<SecretString, BetfairHttpError> {
        let password = Zeroizing::new(urlencoding::encode(self.credential.password()).into_owned());
        let form_body = SecretString::from(format!(
            "username={}&password={}",
            urlencoding::encode(self.credential.username()),
            password.as_str(),
        ));

        let resp_bytes = self
            .send_identity(&self.url_identity_login, form_body)
            .await?;

        let mut resp: LoginResponse = serde_json::from_slice(&resp_bytes)?;

        if resp.status == LoginStatus::Success {
            log::debug!("Betfair login successful");
            Ok(std::mem::take(&mut resp.token))
        } else {
            Err(BetfairHttpError::LoginFailed {
                status: resp
                    .error
                    .take()
                    .unwrap_or_else(|| format!("{:?}", resp.status)),
            })
        }
    }

    /// Clears the session token, cancels any in-flight retries, and primes a
    /// fresh cancellation token for the next session.
    pub async fn disconnect(&self) {
        log::info!("Betfair disconnecting...");
        let _guard = self.connect_lock.lock().await;
        {
            let mut guard = self.cancellation_token.lock();
            guard.cancel();
            *guard = CancellationToken::new();
        }
        *self.session_token.write().await = None;
    }

    /// Sends a keep-alive request to renew the session.
    ///
    /// # Errors
    ///
    /// Returns an error if the keep-alive request fails.
    pub async fn keep_alive(&self) -> Result<(), BetfairHttpError> {
        self.keep_alive_with_token().await.map(drop)
    }

    /// Renews the session and returns the current token.
    ///
    /// # Errors
    ///
    /// Returns an error if the keep-alive request fails.
    pub(crate) async fn keep_alive_with_token(&self) -> Result<SecretString, BetfairHttpError> {
        let _guard = self.connect_lock.lock().await;
        let resp_bytes = self
            .send_identity(&self.url_keep_alive, SecretString::default())
            .await?;

        let mut resp: LoginResponse = serde_json::from_slice(&resp_bytes)?;

        if resp.status == LoginStatus::Success {
            let token = std::mem::take(&mut resp.token);
            *self.session_token.write().await = Some(token.clone());
            Ok(token)
        } else {
            Err(BetfairHttpError::LoginFailed {
                status: resp
                    .error
                    .take()
                    .unwrap_or_else(|| format!("{:?}", resp.status)),
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
        self.send_jsonrpc(&self.url_betting, method, params, false)
            .await
    }

    /// Sends a JSON-RPC request to the Betting API with order rate limiting.
    /// Ambiguous failures are retried only when the params include a request-level `customerRef`.
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
        self.send_jsonrpc(&self.url_betting, method, params, true)
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
        self.send_jsonrpc(&self.url_accounts, method, params, false)
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
                self.url_navigation.clone(),
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

    fn make_quota(requests_per_second: u32, label: &str) -> Result<Quota, BetfairHttpError> {
        let rate = NonZeroU32::new(requests_per_second).ok_or_else(|| {
            BetfairHttpError::InvalidConfiguration(format!("{label} must be greater than zero"))
        })?;

        Quota::per_second(rate).ok_or_else(|| {
            BetfairHttpError::InvalidConfiguration(format!("Invalid {label} quota configuration"))
        })
    }

    fn rate_limiter_quotas(
        request_rate_per_second: u32,
        order_request_rate_per_second: u32,
    ) -> Result<Vec<(String, Quota)>, BetfairHttpError> {
        Ok(vec![
            (
                BETFAIR_RATE_LIMIT_DEFAULT.to_string(),
                Self::make_quota(request_rate_per_second, "request_rate_per_second")?,
            ),
            (
                BETFAIR_RATE_LIMIT_ORDERS.to_string(),
                Self::make_quota(
                    order_request_rate_per_second,
                    "order_request_rate_per_second",
                )?,
            ),
        ])
    }

    fn default_quota(request_rate_per_second: u32) -> Result<Option<Quota>, BetfairHttpError> {
        Ok(Some(Self::make_quota(
            request_rate_per_second,
            "request_rate_per_second",
        )?))
    }

    async fn build_headers(
        &self,
        content_type: &str,
    ) -> Result<HashMap<String, String>, BetfairHttpError> {
        let token = self
            .session_token
            .read()
            .await
            .as_ref()
            .map(|token| token.expose_secret().to_owned())
            .ok_or(BetfairHttpError::MissingCredentials)?;

        let mut headers = HashMap::new();
        headers.insert(HEADER_X_AUTHENTICATION.to_string(), token);
        headers.insert(
            HEADER_X_APPLICATION.to_string(),
            self.credential.app_key().to_string(),
        );
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("Content-Type".to_string(), content_type.to_string());
        Ok(headers)
    }

    async fn send_identity(
        &self,
        url: &str,
        body: SecretString,
    ) -> Result<Vec<u8>, BetfairHttpError> {
        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        headers.insert(
            HEADER_X_APPLICATION.to_string(),
            self.credential.app_key().to_string(),
        );

        // Add session token if we have one (for keep-alive)
        if let Some(token) = self.session_token.read().await.as_ref() {
            headers.insert(
                HEADER_X_AUTHENTICATION.to_string(),
                token.expose_secret().to_owned(),
            );
        }

        let resp = self
            .client
            .request_with_secret_body(
                Method::POST,
                url.to_string(),
                None,
                Some(headers),
                body,
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
        let has_order_customer_ref = params_value
            .get("customerRef")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|customer_ref| !customer_ref.is_empty());
        let had_ambiguous_attempt = AtomicBool::new(false);

        let operation = || {
            let params_value = params_value.clone();
            let had_ambiguous_attempt = &had_ambiguous_attempt;

            async move {
                let result = self
                    .send_jsonrpc_once(base_url, method, params_value, is_order)
                    .await;

                if is_order && result.as_ref().is_err_and(|e| e.is_order_ambiguous()) {
                    had_ambiguous_attempt.store(true, Ordering::Relaxed);
                }

                result
            }
        };

        let should_retry = |error: &BetfairHttpError| -> bool {
            if is_order {
                error.is_order_retryable()
                    && (has_order_customer_ref || !error.is_order_ambiguous())
            } else {
                error.is_retryable()
            }
        };

        let create_error = |error: RetryError| -> BetfairHttpError {
            map_retry_error(error, is_order, &had_ambiguous_attempt)
        };

        // Snapshot the current token; `disconnect()` may swap it for a fresh
        // one mid-flight, but the in-flight retry loop should observe the
        // pre-disconnect token so a cancel actually unblocks it.
        let token = self.cancellation_token.lock().clone();

        let retry_manager = if is_order {
            &self.order_retry_manager
        } else {
            &self.retry_manager
        };
        let result = retry_manager
            .invocation(&operation_id, operation, should_retry, create_error)
            .cancellation_token(&token)
            .execute()
            .await;

        let result = match result {
            Err(e)
                if is_order
                    && had_ambiguous_attempt.load(Ordering::Relaxed)
                    && !e.is_order_ambiguous() =>
            {
                Err(BetfairHttpError::OrderRequestAmbiguous(format!(
                    "an earlier attempt had an unknown outcome; final error: {e}",
                )))
            }
            result => result,
        };

        if let Err(ref e) = result
            && should_retry(e)
        {
            log::error!("Request exhausted retries: method={method}, error={e}");
        }

        result
    }

    async fn send_jsonrpc_once<T>(
        &self,
        base_url: &str,
        method: &str,
        params: serde_json::Value,
        is_order: bool,
    ) -> Result<T, BetfairHttpError>
    where
        T: DeserializeOwned,
    {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
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

        if !resp.status.is_success() {
            let error_body = String::from_utf8_lossy(&resp.body);
            let preview: String = error_body.chars().take(500).collect();
            log::warn!(
                "HTTP error response: method={method}, status={}, body={}",
                resp.status.as_u16(),
                preview,
            );
            return Err(BetfairHttpError::UnexpectedStatus {
                status: resp.status.as_u16(),
                body: error_body.to_string(),
            });
        }

        let json_value: serde_json::Value = serde_json::from_slice(&resp.body).map_err(|e| {
            let preview: String = String::from_utf8_lossy(&resp.body)
                .chars()
                .take(500)
                .collect();
            log::warn!(
                "Non-JSON response: method={method}, status={}, body={preview}",
                resp.status.as_u16(),
            );
            BetfairHttpError::ResponseError(e.to_string())
        })?;

        let rpc_resp: JsonRpcResponse<T> = serde_json::from_value(json_value).map_err(|e| {
            log::warn!("Failed to deserialize JSON-RPC response: method={method}, error={e}",);
            BetfairHttpError::ResponseError(e.to_string())
        })?;

        if let Some(result) = rpc_resp.result {
            Ok(result)
        } else if let Some(error) = rpc_resp.error {
            let api_exception = error.api_exception();
            Err(BetfairHttpError::BetfairError {
                code: error.code,
                message: error.message,
                api_error_code: api_exception
                    .as_ref()
                    .map(|exception| exception.error_code.clone()),
                api_error_details: api_exception.and_then(|exception| exception.error_details),
            })
        } else {
            Err(BetfairHttpError::ResponseError(
                "Response contains neither result nor error".to_string(),
            ))
        }
    }
}

fn map_retry_error(
    error: RetryError,
    is_order: bool,
    had_ambiguous_attempt: &AtomicBool,
) -> BetfairHttpError {
    if is_order && matches!(&error, RetryError::OperationTimeout { .. }) {
        had_ambiguous_attempt.store(true, Ordering::Relaxed);
    }

    match error {
        RetryError::Canceled => {
            BetfairHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
        }
        error => BetfairHttpError::NetworkError(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use parking_lot::Mutex;
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;
    use crate::common::consts::{
        BETFAIR_RATE_LIMIT_DEFAULT, BETFAIR_RATE_LIMIT_ORDERS, METHOD_LIST_MARKET_CATALOGUE,
    };

    fn json_value_strategy() -> impl Strategy<Value = serde_json::Value> {
        let leaf = prop_oneof![
            Just(serde_json::Value::Null),
            any::<bool>().prop_map(serde_json::Value::Bool),
            any::<i64>().prop_map(|value| serde_json::Value::Number(value.into())),
            "[ -~]{0,32}".prop_map(serde_json::Value::String),
        ];

        leaf.prop_recursive(3, 64, 8, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..8).prop_map(serde_json::Value::Array),
                prop::collection::vec(("[A-Za-z0-9_]{0,16}", inner), 0..8)
                    .prop_map(|entries| serde_json::Value::Object(entries.into_iter().collect())),
            ]
        })
    }

    #[rstest]
    fn test_rate_limiter_quotas_has_expected_keys() {
        let quotas = BetfairHttpClient::rate_limiter_quotas(5, 20).unwrap();
        let keys: Vec<&str> = quotas.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&BETFAIR_RATE_LIMIT_DEFAULT));
        assert!(keys.contains(&BETFAIR_RATE_LIMIT_ORDERS));
    }

    #[rstest]
    fn test_default_quota_is_some() {
        assert!(BetfairHttpClient::default_quota(5).unwrap().is_some());
    }

    #[rstest]
    fn test_rate_limiter_quotas_reject_zero_rate_limit() {
        let result = BetfairHttpClient::rate_limiter_quotas(0, 20);

        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("request_rate_per_second")
        );
    }

    #[rstest]
    fn test_debug_redacts_session_token() {
        let client = BetfairHttpClient::new(
            BetfairCredential::new(
                "username".to_string(),
                "betfair-password-sentinel".to_string(),
                "application-key".to_string(),
            ),
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        *client.session_token.try_write().unwrap() =
            Some(SecretString::from("betfair-session-token-sentinel"));

        let debug = format!("{client:?}");

        assert!(debug.contains("session_token"));
        assert!(!debug.contains("betfair-session-token-sentinel"));
        assert!(!debug.contains("betfair-password-sentinel"));
    }

    #[rstest]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            method: METHOD_LIST_MARKET_CATALOGUE.to_string(),
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
        let json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_data/rest/betting_place_order_success.json"
        ));
        let resp: JsonRpcResponse<serde_json::Value> = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.result
                .as_ref()
                .and_then(|result| result["status"].as_str()),
            Some("SUCCESS")
        );
        assert!(resp.error.is_none());
    }

    #[rstest]
    fn test_json_rpc_response_error() {
        let json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test_data/rest/betting_jsonrpc_error_too_much_data_live.json"
        ));
        let resp: JsonRpcResponse<serde_json::Value> = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let error = resp.error.unwrap();
        assert_eq!(error.code, -32099);
        assert_eq!(error.message, "ANGX-0001");
        let api_exception = error.api_exception().unwrap();
        assert_eq!(api_exception.error_code, "TOO_MUCH_DATA");
        assert_eq!(
            api_exception.error_details.as_deref(),
            Some("MaxResults must be less than or equal to 1000")
        );
    }

    #[rstest]
    fn test_order_operation_timeout_records_ambiguous_attempt() {
        let had_ambiguous_attempt = AtomicBool::new(false);

        let error = map_retry_error(
            RetryError::OperationTimeout { timeout_ms: 30_000 },
            true,
            &had_ambiguous_attempt,
        );

        assert!(error.is_order_ambiguous());
        assert!(had_ambiguous_attempt.load(Ordering::Relaxed));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[rstest]
        fn arbitrary_json_rpc_error_data_preserves_outer_error(
            data in json_value_strategy(),
        ) {
            let response: JsonRpcResponse<serde_json::Value> = serde_json::from_value(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 73,
                    "error": {
                        "code": -32099,
                        "message": "ANGX-PROPERTY",
                        "data": data,
                    },
                }),
            )
            .unwrap();
            let error = response.error.unwrap();
            let api_exception = error.api_exception();
            let surfaced = BetfairHttpError::BetfairError {
                code: error.code,
                message: error.message.clone(),
                api_error_code: api_exception
                    .as_ref()
                    .map(|exception| exception.error_code.clone()),
                api_error_details: api_exception
                    .and_then(|exception| exception.error_details),
            };

            prop_assert_eq!(error.code, -32099);
            prop_assert_eq!(error.message, "ANGX-PROPERTY");

            if matches!(
                &surfaced,
                BetfairHttpError::BetfairError {
                    api_error_code: None,
                    ..
                }
            ) {
                prop_assert!(surfaced.is_order_ambiguous());
                prop_assert!(!surfaced.is_order_retryable());
            }
        }

        #[rstest]
        fn unknown_api_error_code_is_ambiguous_and_not_retried(
            api_error_code in "FUTURE_[A-Z0-9_]{1,24}",
        ) {
            let exception_name = "FutureAPINGException";
            let response: JsonRpcResponse<serde_json::Value> = serde_json::from_value(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": 74,
                    "error": {
                        "code": -32099,
                        "message": "ANGX-FUTURE",
                        "data": {
                            "exceptionname": exception_name,
                            (exception_name): {
                                "errorCode": api_error_code,
                                "errorDetails": "future wire shape",
                            },
                        },
                    },
                }),
            )
            .unwrap();
            let error = response.error.unwrap();
            let api_exception = error.api_exception().unwrap();
            let surfaced = BetfairHttpError::BetfairError {
                code: error.code,
                message: error.message.clone(),
                api_error_code: Some(api_exception.error_code.clone()),
                api_error_details: api_exception.error_details.clone(),
            };

            prop_assert_eq!(error.code, -32099);
            prop_assert_eq!(error.message, "ANGX-FUTURE");
            prop_assert_eq!(api_exception.error_code, api_error_code);
            prop_assert_eq!(
                api_exception.error_details.as_deref(),
                Some("future wire shape"),
            );
            prop_assert!(surfaced.is_order_ambiguous());
            prop_assert!(!surfaced.is_order_retryable());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_order_retry_budget_prevents_dispatch_at_45_seconds() {
        let client = BetfairHttpClient::new(
            BetfairCredential::new(
                "username".to_string(),
                "password".to_string(),
                "app-key".to_string(),
            ),
            None,
            Some(100),
            Some(10_000),
            None,
            Some(5),
            Some(20),
        )
        .unwrap();
        let started_at = tokio::time::Instant::now();
        let attempt_times = Arc::new(Mutex::new(Vec::new()));
        let attempt_times_for_operation = Arc::clone(&attempt_times);

        let result = client
            .order_retry_manager
            .invocation(
                "placeOrders",
                move || {
                    let attempt_times = Arc::clone(&attempt_times_for_operation);
                    async move {
                        attempt_times.lock().push(started_at.elapsed());
                        Err::<(), _>(BetfairHttpError::UnexpectedStatus {
                            status: 502,
                            body: "Bad Gateway".to_string(),
                        })
                    }
                },
                BetfairHttpError::is_order_retryable,
                |e| BetfairHttpError::NetworkError(e.to_string()),
            )
            .execute()
            .await;

        assert!(matches!(result, Err(BetfairHttpError::NetworkError(_))));
        assert_eq!(started_at.elapsed(), Duration::from_secs(45));
        let attempt_times = attempt_times.lock();
        assert!(attempt_times.len() > 1);
        assert_eq!(attempt_times[0], Duration::ZERO);
        assert!(
            attempt_times
                .iter()
                .all(|elapsed| *elapsed < Duration::from_secs(45)),
        );
    }
}
