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

//! `reqwest`-backed REST client for the Derive API.
//!
//! [`DeriveHttpClient`] exposes typed `send_public` / `send_private`
//! dispatchers plus thin wrappers for the two endpoints that establish the
//! plumbing this crate needs to grow against: `public/get_instruments` and
//! `private/order`. Authenticated requests inject the EIP-191 session-key
//! headers built by [`crate::signing::auth`].

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use alloy::signers::local::PrivateKeySigner;
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpResponse},
    retry::{RetryConfig, RetryManager},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    common::{
        consts::{HEADER_LYRA_SIGNATURE, HEADER_LYRA_TIMESTAMP, HEADER_LYRA_WALLET, HTTP_TIMEOUT},
        enums::DeriveInstrumentType,
        rate_limit::{self, DERIVE_NON_MATCHING_RATE_KEY},
        retry::{http_retry_config, should_retry_http_error},
    },
    http::{
        error::{DeriveHttpError, Result},
        models::{
            DeriveEmptyResult, DeriveInstrument, DeriveOpenOrdersResult, DeriveOrder,
            DeriveOrderResult, DeriveOrdersResult, DerivePositionsResult, DerivePublicCandle,
            DerivePublicFundingRateHistoryResult, DerivePublicTradesResult, DeriveReplaceResult,
            DeriveSubaccount, DeriveTickerSnapshot, DeriveTickersResult, DeriveTradesResult,
            JsonRpcResponse,
        },
        query::{
            DeriveCancelAllParams, DeriveCancelByLabelParams, DeriveCancelParams,
            DeriveGetOpenOrdersParams, DeriveGetOrderHistoryParams, DeriveGetOrderParams,
            DeriveGetPositionsParams, DeriveGetSubaccountParams, DeriveGetTradeHistoryParams,
            DeriveGetTriggerOrdersParams, DeriveOrderParams, DeriveReplaceParams,
        },
    },
    signing::auth::{AuthHeaders, build_rest_auth_headers},
};

/// Credentials used to sign authenticated REST requests.
///
/// `Debug` is implemented manually so the session key never escapes through
/// loggers or Python `__repr__`.
#[derive(Clone)]
pub struct DeriveCredentials {
    /// Derive Chain smart-contract wallet address (`0x`-prefixed hex, 42 chars).
    pub wallet_address: String,
    /// secp256k1 session-key signer.
    pub signer: PrivateKeySigner,
}

impl DeriveCredentials {
    /// Constructs credentials by parsing `session_key_hex` into a signer.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::Auth`] when the session-key hex cannot be
    /// parsed.
    pub fn new(wallet_address: impl Into<String>, session_key_hex: &str) -> Result<Self> {
        let signer: PrivateKeySigner = session_key_hex
            .parse()
            .map_err(|e| DeriveHttpError::decode(format!("invalid session key: {e}")))?;
        Ok(Self {
            wallet_address: wallet_address.into(),
            signer,
        })
    }
}

impl Debug for DeriveCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DeriveCredentials))
            .field("wallet_address", &self.wallet_address)
            .field("signer", &"***redacted***")
            .finish()
    }
}

/// HTTP client for the Derive REST API.
///
/// The client carries an atomic `id` counter so every request frame has a
/// unique correlator; the REST transport ships only `params` on the wire but
/// the id is preserved for logs and reused by the upcoming WebSocket client.
/// Each call routes through a [`RetryManager`] that re-signs auth headers on
/// every attempt, so retries never replay a stale `X-LYRATIMESTAMP`.
#[derive(Debug, Clone)]
pub struct DeriveHttpClient {
    client: HttpClient,
    base_url: String,
    credentials: Option<DeriveCredentials>,
    next_id: Arc<AtomicU64>,
    timeout_secs: u64,
    retry_manager: Arc<RetryManager<DeriveHttpError>>,
}

impl DeriveHttpClient {
    /// Creates a public-only client.
    ///
    /// `retry_config` defaults to [`http_retry_config(3, 100, 5_000)`] when `None`.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::Transport`] when the underlying HTTP client
    /// (proxy URL, TLS init) cannot be constructed.
    pub fn new(
        base_url: impl Into<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> Result<Self> {
        let timeout_secs = timeout_secs.unwrap_or_else(|| HTTP_TIMEOUT.as_secs());
        let client = build_client(timeout_secs, proxy_url)?;
        let retry_config = retry_config.unwrap_or_else(|| http_retry_config(3, 100, 5_000));
        Ok(Self {
            client,
            base_url: trim_trailing_slash(base_url.into()),
            credentials: None,
            next_id: Arc::new(AtomicU64::new(1)),
            timeout_secs,
            retry_manager: Arc::new(RetryManager::new(retry_config)),
        })
    }

    /// Creates a client with credentials installed for `send_private` calls.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::Transport`] when the underlying HTTP client
    /// cannot be constructed.
    pub fn with_credentials(
        base_url: impl Into<String>,
        credentials: DeriveCredentials,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> Result<Self> {
        let mut client = Self::new(base_url, timeout_secs, proxy_url, retry_config)?;
        client.credentials = Some(credentials);
        Ok(client)
    }

    /// Returns the configured base URL (no trailing slash).
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns `true` when credentials are installed.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credentials.is_some()
    }

    /// Allocates the next correlator id.
    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Sends an unauthenticated request and decodes the JSON-RPC envelope.
    ///
    /// Public endpoints are idempotent reads; this path retries transient
    /// failures via the configured [`RetryManager`].
    ///
    /// # Errors
    ///
    /// Propagates transport, HTTP, and JSON-RPC errors. See [`DeriveHttpError`].
    pub async fn send_public<P, R>(&self, method: &str, params: &P) -> Result<R>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let id = self.next_id();
        self.dispatch(method, params, id, false, true).await
    }

    /// Sends an authenticated idempotent request (private reads).
    ///
    /// Used for `private/get_*` endpoints whose responses are pure reads of
    /// venue state. Transient failures retry via the configured
    /// [`RetryManager`].
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when the client was
    /// built without credentials. Other variants propagate from the transport
    /// or the venue.
    pub async fn send_private<P, R>(&self, method: &str, params: &P) -> Result<R>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        if self.credentials.is_none() {
            return Err(DeriveHttpError::MissingCredentials {
                method: method.to_owned(),
            });
        }
        let id = self.next_id();
        self.dispatch(method, params, id, true, true).await
    }

    /// Sends an authenticated request exactly once (no retry).
    ///
    /// Used for state-changing endpoints (`private/order`, `private/cancel`,
    /// `private/cancel_all`, `private/cancel_by_label`, `private/replace`)
    /// where a transport-level failure leaves the venue's view of the
    /// signed action ambiguous: the request may have been accepted before
    /// the network broke. Automatic replay would either double-submit (when
    /// the venue accepted) or trigger a duplicate-nonce rejection (which
    /// the caller would surface as `OrderRejected` even though the original
    /// is live). Callers are expected to resolve ambiguous outcomes via
    /// reconciliation rather than retry here.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when the client was
    /// built without credentials. Other variants propagate from the transport
    /// or the venue.
    pub async fn send_private_once<P, R>(&self, method: &str, params: &P) -> Result<R>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        if self.credentials.is_none() {
            return Err(DeriveHttpError::MissingCredentials {
                method: method.to_owned(),
            });
        }
        let id = self.next_id();
        self.dispatch(method, params, id, true, false).await
    }

    /// Fetches the venue's listed instruments.
    ///
    /// `currency` is the perpetual/option underlying (e.g. `"ETH"`). When
    /// `expired` is `true` the venue includes expired option strikes.
    ///
    /// # Errors
    ///
    /// Propagates [`DeriveHttpError`] for transport, HTTP, and JSON-RPC failures.
    pub async fn get_instruments(
        &self,
        currency: &str,
        instrument_type: DeriveInstrumentType,
        expired: bool,
    ) -> Result<Vec<DeriveInstrument>> {
        let params = serde_json::json!({
            "currency": currency,
            "instrument_type": instrument_type,
            "expired": expired,
        });
        self.send_public("public/get_instruments", &params).await
    }

    /// Fetches a single instrument definition by name.
    ///
    /// Mirrors `public/get_instrument`, which the venue documents as the
    /// per-asset variant of `public/get_instruments`. The returned record
    /// matches one row of the bulk endpoint.
    ///
    /// # Errors
    ///
    /// Propagates [`DeriveHttpError`] for transport, HTTP, and JSON-RPC failures.
    pub async fn get_instrument(&self, instrument_name: &str) -> Result<DeriveInstrument> {
        let params = serde_json::json!({
            "instrument_name": instrument_name,
        });
        self.send_public("public/get_instrument", &params).await
    }

    /// Fetches a page of public trade history for the instrument.
    ///
    /// `from_timestamp` / `to_timestamp` are UNIX milliseconds and bound the
    /// returned window. `page` is 1-indexed; `page_size` is capped by the venue
    /// at 1000.
    ///
    /// # Errors
    ///
    /// Propagates [`DeriveHttpError`] for transport, HTTP, and JSON-RPC failures.
    pub async fn get_trade_history(
        &self,
        instrument_name: &str,
        from_timestamp: Option<i64>,
        to_timestamp: Option<i64>,
        page: u32,
        page_size: u32,
    ) -> Result<DerivePublicTradesResult> {
        let mut params = serde_json::Map::new();
        params.insert("instrument_name".to_string(), instrument_name.into());
        params.insert("page".to_string(), page.into());
        params.insert("page_size".to_string(), page_size.into());
        if let Some(from) = from_timestamp {
            params.insert("from_timestamp".to_string(), from.into());
        }

        if let Some(to) = to_timestamp {
            params.insert("to_timestamp".to_string(), to.into());
        }

        self.send_public("public/get_trade_history", &Value::Object(params))
            .await
    }

    /// Fetches the public funding rate history for the instrument.
    ///
    /// `start_timestamp` / `end_timestamp` are UNIX milliseconds. `period`, if
    /// provided, selects the sample interval in seconds.
    ///
    /// # Errors
    ///
    /// Propagates [`DeriveHttpError`] for transport, HTTP, and JSON-RPC failures.
    pub async fn get_funding_rate_history(
        &self,
        instrument_name: &str,
        start_timestamp: Option<i64>,
        end_timestamp: Option<i64>,
        period: Option<u32>,
    ) -> Result<DerivePublicFundingRateHistoryResult> {
        let mut params = serde_json::Map::new();
        params.insert("instrument_name".to_string(), instrument_name.into());
        if let Some(start) = start_timestamp {
            params.insert("start_timestamp".to_string(), start.into());
        }

        if let Some(end) = end_timestamp {
            params.insert("end_timestamp".to_string(), end.into());
        }

        if let Some(period) = period {
            params.insert("period".to_string(), period.into());
        }

        self.send_public("public/get_funding_rate_history", &Value::Object(params))
            .await
    }

    /// Fetches OHLCV candles via `public/get_tradingview_chart_data`.
    ///
    /// `start_timestamp` / `end_timestamp` are UNIX **seconds** and bound the
    /// returned window. `period` is the bucket size in seconds; the venue
    /// accepts 60, 300, 900, 1800, 3600, 14400, 28800, 86400, and 604800.
    /// The venue ships `result` as a flat array; the client decodes it
    /// directly into `Vec<DerivePublicCandle>`.
    ///
    /// # Errors
    ///
    /// Propagates [`DeriveHttpError`] for transport, HTTP, and JSON-RPC failures.
    pub async fn get_candles(
        &self,
        instrument_name: &str,
        start_timestamp: i64,
        end_timestamp: i64,
        period: u32,
    ) -> Result<Vec<DerivePublicCandle>> {
        let params = serde_json::json!({
            "instrument_name": instrument_name,
            "start_timestamp": start_timestamp,
            "end_timestamp": end_timestamp,
            "period": period,
        });
        self.send_public("public/get_tradingview_chart_data", &params)
            .await
    }

    /// Fetches current ticker snapshots.
    ///
    /// `currency` is the underlying (`"ETH"`, `"BTC"`, etc.). Options require
    /// both `currency` and `expiry_date`; perps and ERC-20 spot pairs reject
    /// `expiry_date`.
    ///
    /// # Errors
    ///
    /// Propagates [`DeriveHttpError`] for transport, HTTP, and JSON-RPC failures.
    pub async fn get_tickers(
        &self,
        instrument_type: DeriveInstrumentType,
        currency: Option<&str>,
        expiry_date: Option<&str>,
    ) -> Result<DeriveTickersResult> {
        let mut params = serde_json::Map::new();
        params.insert(
            "instrument_type".to_string(),
            serde_json::to_value(instrument_type).map_err(DeriveHttpError::from)?,
        );

        if let Some(currency) = currency {
            params.insert("currency".to_string(), currency.into());
        }

        if let Some(expiry_date) = expiry_date {
            params.insert("expiry_date".to_string(), expiry_date.into());
        }

        self.send_public("public/get_tickers", &Value::Object(params))
            .await
    }

    /// Fetches the current ticker snapshot for one instrument.
    ///
    /// This is a single-instrument convenience wrapper over
    /// `public/get_tickers`, which replaced Derive's deprecated
    /// `public/get_ticker` RPC.
    ///
    /// # Errors
    ///
    /// Propagates [`DeriveHttpError`] for transport, HTTP, JSON-RPC failures,
    /// or when the response omits the requested instrument.
    pub async fn get_ticker(&self, instrument_name: &str) -> Result<DeriveTickerSnapshot> {
        let request = ticker_request(instrument_name)?;
        let result = self
            .get_tickers(
                request.instrument_type,
                Some(request.currency),
                request.expiry_date,
            )
            .await?;
        let mut ticker = result
            .tickers
            .get(instrument_name)
            .cloned()
            .ok_or_else(|| {
                DeriveHttpError::decode(format!(
                    "missing ticker `{instrument_name}` in public/get_tickers response"
                ))
            })?;
        ticker.instrument_name = instrument_name.into();
        Ok(ticker)
    }

    /// Submits a signed order to the venue.
    ///
    /// `params` must be the fully-built signed `private/order` body.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn submit_order(&self, params: &DeriveOrderParams) -> Result<DeriveOrder> {
        let result: DeriveOrderResult = self.send_private_once("private/order", params).await?;
        Ok(result.order)
    }

    /// Cancels a single order by venue order id.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn cancel_order(&self, params: &DeriveCancelParams) -> Result<DeriveEmptyResult> {
        self.send_private_once("private/cancel", params).await
    }

    /// Cancels every open order on the subaccount, optionally scoped to an
    /// instrument.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn cancel_all(&self, params: &DeriveCancelAllParams) -> Result<DeriveEmptyResult> {
        self.send_private_once("private/cancel_all", params).await
    }

    /// Cancels every open order for the given user label on the subaccount.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn cancel_by_label(
        &self,
        params: &DeriveCancelByLabelParams,
    ) -> Result<DeriveEmptyResult> {
        self.send_private_once("private/cancel_by_label", params)
            .await
    }

    /// Submits a signed `private/replace` request that atomically cancels one
    /// order and creates a new one.
    ///
    /// `params` must be the fully-built typed request body.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn replace_order(&self, params: &DeriveReplaceParams) -> Result<DeriveOrder> {
        let result: DeriveReplaceResult = self.send_private_once("private/replace", params).await?;
        Ok(result.order)
    }

    /// Returns the subaccount snapshot including margin, balances, and
    /// open orders.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn get_subaccount(
        &self,
        params: &DeriveGetSubaccountParams,
    ) -> Result<DeriveSubaccount> {
        self.send_private("private/get_subaccount", params).await
    }

    /// Returns currently open orders for the subaccount.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn get_open_orders(
        &self,
        params: &DeriveGetOpenOrdersParams,
    ) -> Result<DeriveOpenOrdersResult> {
        self.send_private("private/get_open_orders", params).await
    }

    /// Returns currently untriggered trigger orders for the subaccount.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn get_trigger_orders(
        &self,
        params: &DeriveGetTriggerOrdersParams,
    ) -> Result<DeriveOpenOrdersResult> {
        self.send_private("private/get_trigger_orders", params)
            .await
    }

    /// Returns a single order by venue order id.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn get_order(&self, params: &DeriveGetOrderParams) -> Result<DeriveOrder> {
        self.send_private("private/get_order", params).await
    }

    /// Returns one page of order history for the subaccount, optionally
    /// scoped to an instrument and time window.
    ///
    /// `from_timestamp` / `to_timestamp` are UNIX milliseconds. `page` is
    /// 1-indexed and `page_size` is capped by the venue at 1000.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn get_order_history(
        &self,
        params: &DeriveGetOrderHistoryParams,
    ) -> Result<DeriveOrdersResult> {
        self.send_private("private/get_order_history", params).await
    }

    /// Returns one page of subaccount trade history.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn get_private_trade_history(
        &self,
        params: &DeriveGetTradeHistoryParams,
    ) -> Result<DeriveTradesResult> {
        self.send_private("private/get_trade_history", params).await
    }

    /// Returns the positions held by the subaccount.
    ///
    /// # Errors
    ///
    /// Returns [`DeriveHttpError::MissingCredentials`] when no credentials
    /// were installed; otherwise propagates transport and venue errors.
    pub async fn get_positions(
        &self,
        params: &DeriveGetPositionsParams,
    ) -> Result<DerivePositionsResult> {
        self.send_private("private/get_positions", params).await
    }

    async fn dispatch<P, R>(
        &self,
        method: &str,
        params: &P,
        id: u64,
        authenticate: bool,
        retry: bool,
    ) -> Result<R>
    where
        P: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = format!("{}/{}", self.base_url, method.trim_start_matches('/'));
        let body_value = serde_json::to_value(params).map_err(DeriveHttpError::from)?;
        let body = serde_json::to_vec(&body_value).map_err(DeriveHttpError::from)?;

        // Every REST call is a non-matching read; gate it on the shared
        // non-matching quota. A non-empty key is required: the limiter skips
        // requests sent with no keys even when a default quota is configured.
        let rate_keys = vec![DERIVE_NON_MATCHING_RATE_KEY.to_string()];

        // Sign per-attempt so the venue never sees a stale `X-LYRATIMESTAMP`
        // after a long backoff window; single-shot writes still run the
        // closure once and use freshly built headers.
        let attempt = || async {
            let mut headers: AHashMap<String, String> = AHashMap::with_capacity(4);
            headers.insert("Content-Type".to_string(), "application/json".to_string());

            if authenticate {
                let auth = self.build_auth_headers(method)?;
                headers.insert(HEADER_LYRA_WALLET.to_string(), auth.wallet);
                headers.insert(HEADER_LYRA_TIMESTAMP.to_string(), auth.timestamp);
                headers.insert(HEADER_LYRA_SIGNATURE.to_string(), auth.signature);
            }

            let response = self
                .client
                .post(
                    url.clone(),
                    None,
                    Some(headers.into_iter().collect()),
                    Some(body.clone()),
                    Some(self.timeout_secs),
                    Some(rate_keys.clone()),
                )
                .await
                .map_err(DeriveHttpError::from)?;

            decode_envelope(method, id, response)
        };

        if retry {
            self.retry_manager
                .execute_with_retry(method, attempt, should_retry_http_error, |msg| {
                    DeriveHttpError::transport(msg)
                })
                .await
        } else {
            attempt().await
        }
    }

    fn build_auth_headers(&self, method: &str) -> Result<AuthHeaders> {
        let credentials =
            self.credentials
                .as_ref()
                .ok_or_else(|| DeriveHttpError::MissingCredentials {
                    method: method.to_owned(),
                })?;
        let auth = build_rest_auth_headers(&credentials.wallet_address, &credentials.signer)?;
        Ok(auth)
    }
}

#[derive(Debug, Clone, Copy)]
struct TickerRequest<'a> {
    instrument_type: DeriveInstrumentType,
    currency: &'a str,
    expiry_date: Option<&'a str>,
}

fn ticker_request(instrument_name: &str) -> Result<TickerRequest<'_>> {
    let Some((currency, suffix)) = instrument_name.split_once('-') else {
        return Err(DeriveHttpError::decode(format!(
            "invalid Derive instrument name `{instrument_name}`"
        )));
    };

    if suffix == "PERP" {
        return Ok(TickerRequest {
            instrument_type: DeriveInstrumentType::Perp,
            currency,
            expiry_date: None,
        });
    }

    let mut parts = suffix.split('-');
    let Some(expiry_date) = parts.next() else {
        return Ok(TickerRequest {
            instrument_type: DeriveInstrumentType::Erc20,
            currency,
            expiry_date: None,
        });
    };
    let has_option_tail = parts.clone().count() == 2;
    if expiry_date.len() == 8 && expiry_date.chars().all(|c| c.is_ascii_digit()) && has_option_tail
    {
        return Ok(TickerRequest {
            instrument_type: DeriveInstrumentType::Option,
            currency,
            expiry_date: Some(expiry_date),
        });
    }

    Ok(TickerRequest {
        instrument_type: DeriveInstrumentType::Erc20,
        currency,
        expiry_date: None,
    })
}

fn build_client(
    timeout_secs: u64,
    proxy_url: Option<String>,
) -> std::result::Result<HttpClient, HttpClientError> {
    // Every REST endpoint this client calls is a non-matching read, so a single
    // default quota (keyed by `DERIVE_NON_MATCHING_RATE_KEY` at the call site)
    // covers them; no per-endpoint keyed quotas are needed.
    HttpClient::new(
        HashMap::new(),
        Vec::new(),
        Vec::new(),
        Some(rate_limit::non_matching_quota()),
        Some(timeout_secs),
        proxy_url,
    )
}

fn trim_trailing_slash(url: String) -> String {
    if url.ends_with('/') {
        url.trim_end_matches('/').to_string()
    } else {
        url
    }
}

fn decode_envelope<R: DeserializeOwned>(
    method: &str,
    request_id: u64,
    response: HttpResponse,
) -> Result<R> {
    let status = response.status.as_u16();
    let is_success_status = (200..300).contains(&status);
    let body = response.body;

    let envelope: JsonRpcResponse<R> = match serde_json::from_slice(&body) {
        Ok(env) => env,
        Err(e) => {
            if !is_success_status {
                let text = String::from_utf8_lossy(&body).into_owned();
                return Err(DeriveHttpError::http(status, truncate(text, 512)));
            }
            return Err(DeriveHttpError::decode(format!(
                "failed to decode `{method}` response: {e}",
            )));
        }
    };

    if let Some(err) = envelope.error {
        return Err(DeriveHttpError::JsonRpc {
            code: err.code,
            message: err.message,
            data: err.data,
        });
    }

    // Gateways (Cloudflare, the wallet auth proxy) return non-2xx with a JSON body
    // like {"message": "Unauthorized"} that parses into an empty envelope. Surface
    // those as Http errors so retry/reconcile logic sees the real status code
    // instead of MissingResult.
    if !is_success_status {
        let text = String::from_utf8_lossy(&body).into_owned();
        return Err(DeriveHttpError::http(status, truncate(text, 512)));
    }

    if let Some(echoed) = envelope.id
        && echoed != request_id
    {
        log::debug!(
            "derive: id mismatch for `{method}` (sent={request_id}, recv={echoed}); accepting result",
        );
    }

    envelope
        .result
        .ok_or_else(|| DeriveHttpError::MissingResult {
            method: method.to_owned(),
        })
}

fn truncate(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut cutoff = max;
    while cutoff > 0 && !s.is_char_boundary(cutoff) {
        cutoff -= 1;
    }
    let mut out = String::with_capacity(cutoff + 3);
    out.push_str(&s[..cutoff]);
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use nautilus_network::http::{HttpStatus, StatusCode};
    use rstest::rstest;

    use super::*;

    const SESSION_KEY_HEX: &str =
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
    const TEST_WALLET: &str = "0x000000000000000000000000000000000000aaaa";

    fn test_client() -> DeriveHttpClient {
        DeriveHttpClient::new("https://api.example/", None, None, None).expect("client builds")
    }

    fn test_response(status: u16, body: &serde_json::Value) -> HttpResponse {
        let status_code = StatusCode::from_u16(status).unwrap();
        HttpResponse {
            status: HttpStatus::new(status_code),
            headers: HashMap::new(),
            body: serde_json::to_vec(body).unwrap().into(),
        }
    }

    #[rstest]
    fn test_credentials_debug_redacts_signer() {
        let creds = DeriveCredentials::new(TEST_WALLET, SESSION_KEY_HEX).unwrap();
        let dbg = format!("{creds:?}");
        assert!(dbg.contains("***redacted***"));
        assert!(dbg.contains(TEST_WALLET));
        assert!(!dbg.contains(SESSION_KEY_HEX));
    }

    #[rstest]
    fn test_credentials_rejects_invalid_session_key() {
        let err = DeriveCredentials::new(TEST_WALLET, "not-hex").expect_err("must reject");
        match err {
            DeriveHttpError::Decode(msg) => assert!(msg.contains("invalid session key")),
            other => panic!("expected Decode, was {other:?}"),
        }
    }

    #[rstest]
    fn test_base_url_trims_trailing_slash() {
        let client = test_client();
        assert_eq!(client.base_url(), "https://api.example");
    }

    #[rstest]
    fn test_new_has_no_credentials() {
        assert!(!test_client().has_credentials());
    }

    #[rstest]
    fn test_with_credentials_sets_creds() {
        let creds = DeriveCredentials::new(TEST_WALLET, SESSION_KEY_HEX).unwrap();
        let client =
            DeriveHttpClient::with_credentials("https://api.example", creds, None, None, None)
                .unwrap();
        assert!(client.has_credentials());
    }

    #[rstest]
    fn test_next_id_increments_monotonically() {
        let client = test_client();
        let a = client.next_id();
        let b = client.next_id();
        let c = client.next_id();
        assert_eq!(b, a + 1);
        assert_eq!(c, b + 1);
    }

    #[rstest]
    fn test_decode_envelope_returns_result() {
        let resp = test_response(200, &serde_json::json!({"id": 1, "result": {"ok": true}}));
        let value: Value = decode_envelope("public/get_instruments", 1, resp).unwrap();
        assert_eq!(value["ok"], true);
    }

    #[rstest]
    fn test_decode_envelope_propagates_jsonrpc_error() {
        let resp = test_response(
            200,
            &serde_json::json!({
                "id": 1,
                "error": {"code": -32601, "message": "Method not found"}
            }),
        );
        let err: DeriveHttpError = decode_envelope::<Value>("public/missing", 1, resp).unwrap_err();
        match err {
            DeriveHttpError::JsonRpc { code, message, .. } => {
                assert_eq!(code, -32601);
                assert_eq!(message, "Method not found");
            }
            other => panic!("expected JsonRpc, was {other:?}"),
        }
    }

    #[rstest]
    fn test_decode_envelope_flags_missing_result() {
        let resp = test_response(200, &serde_json::json!({"id": 1}));
        let err = decode_envelope::<Value>("public/get_instruments", 1, resp).unwrap_err();
        assert!(matches!(err, DeriveHttpError::MissingResult { .. }));
    }

    #[rstest]
    fn test_decode_envelope_flags_non_2xx_with_unparsable_body() {
        let status_code = StatusCode::from_u16(503).unwrap();
        let response = HttpResponse {
            status: HttpStatus::new(status_code),
            headers: HashMap::new(),
            body: bytes::Bytes::from_static(b"<html>upstream down</html>"),
        };
        let err = decode_envelope::<Value>("public/get_instruments", 1, response).unwrap_err();
        match err {
            DeriveHttpError::Http { status, message } => {
                assert_eq!(status, 503);
                assert!(message.contains("upstream down"));
            }
            other => panic!("expected Http, was {other:?}"),
        }
    }

    #[rstest]
    fn test_decode_envelope_flags_non_2xx_with_non_envelope_json() {
        // Gateways return non-2xx with JSON bodies like {"message": "Unauthorized"}.
        // These parse as an empty JsonRpcResponse; the status must still surface.
        let resp = test_response(401, &serde_json::json!({"message": "Unauthorized"}));
        let err = decode_envelope::<Value>("private/order", 1, resp).unwrap_err();
        match err {
            DeriveHttpError::Http { status, message } => {
                assert_eq!(status, 401);
                assert!(message.contains("Unauthorized"));
            }
            other => panic!("expected Http, was {other:?}"),
        }
    }

    #[rstest]
    fn test_decode_envelope_prefers_jsonrpc_error_over_http_status() {
        // When the venue returns a proper JSON-RPC error envelope with a non-2xx
        // status, the envelope wins because it carries richer venue context.
        let status_code = StatusCode::from_u16(400).unwrap();
        let body = serde_json::json!({
            "id": 1,
            "error": {"code": -32602, "message": "Invalid params"},
        });
        let response = HttpResponse {
            status: HttpStatus::new(status_code),
            headers: HashMap::new(),
            body: serde_json::to_vec(&body).unwrap().into(),
        };
        let err = decode_envelope::<Value>("private/order", 1, response).unwrap_err();
        assert!(matches!(err, DeriveHttpError::JsonRpc { code: -32602, .. }));
    }

    #[rstest]
    fn test_truncate_handles_multi_byte_char_at_boundary() {
        // "Ω" is two bytes (0xCE 0xA9). Truncating to a length that lands mid-glyph
        // must not panic; we step back to the prior char boundary.
        let s = "ΩΩΩΩΩΩΩΩΩΩ".to_string();
        assert_eq!(s.len(), 20);
        let out = truncate(s, 5);
        assert!(out.ends_with("..."));
        let prefix = out.trim_end_matches("...");
        assert!(prefix.is_char_boundary(prefix.len()));
        assert!(prefix.chars().all(|c| c == 'Ω'));
    }

    #[rstest]
    fn test_truncate_returns_input_when_under_limit() {
        let s = "short".to_string();
        assert_eq!(truncate(s, 16), "short");
    }

    #[rstest]
    fn test_decode_envelope_non_2xx_body_with_non_ascii_does_not_panic() {
        // Regression: a Cloudflare-style 503 page containing non-ASCII bytes near
        // the truncation cutoff must not panic.
        let glyph = "Ω";
        let body = glyph.repeat(600);
        let status_code = StatusCode::from_u16(503).unwrap();
        let response = HttpResponse {
            status: HttpStatus::new(status_code),
            headers: HashMap::new(),
            body: body.into_bytes().into(),
        };
        let err = decode_envelope::<Value>("public/get_instruments", 1, response).unwrap_err();
        assert!(matches!(err, DeriveHttpError::Http { status: 503, .. }));
    }

    #[rstest]
    fn test_decode_envelope_accepts_id_mismatch() {
        let resp = test_response(200, &serde_json::json!({"id": 99, "result": "ok"}));
        let value: Value = decode_envelope("public/get_instruments", 1, resp).unwrap();
        assert_eq!(value, serde_json::json!("ok"));
    }

    #[tokio::test]
    async fn test_send_private_without_credentials_errors() {
        let client = test_client();
        let err = client
            .send_private::<_, Value>("private/order", &serde_json::json!({}))
            .await
            .expect_err("must require credentials");

        match err {
            DeriveHttpError::MissingCredentials { method } => {
                assert_eq!(method, "private/order");
            }
            other => panic!("expected MissingCredentials, was {other:?}"),
        }
    }
}
