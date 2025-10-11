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
    sync::{Arc, LazyLock, RwLock},
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_model::{
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::{Method, header::USER_AGENT};
use serde_json::Value;
use tokio::time::sleep;
use ustr::Ustr;

use crate::{
    common::{
        consts::{HYPERLIQUID_VENUE, exchange_url, info_url},
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
    instruments: Arc<RwLock<AHashMap<Ustr, InstrumentAny>>>,
    account_id: Option<AccountId>,
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
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            account_id: None,
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
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            account_id: None,
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

    /// Add an instrument to the internal cache for report generation.
    ///
    /// This is required for parsing orders, fills, and positions into reports.
    /// Instruments are stored under two keys:
    /// 1. The Nautilus symbol (e.g., "BTC-USD-PERP")
    /// 2. The Hyperliquid coin identifier (base currency, e.g., "BTC" or "vntls:vCURSOR")
    ///
    /// # Panics
    ///
    /// Panics if the instrument lock cannot be acquired.
    pub fn add_instrument(&self, instrument: InstrumentAny) {
        let mut instruments = self
            .instruments
            .write()
            .expect("Failed to acquire write lock");

        // Store by Nautilus symbol
        let nautilus_symbol = instrument.id().symbol.inner();
        instruments.insert(nautilus_symbol, instrument.clone());

        // Store by Hyperliquid coin identifier (base currency)
        // This allows lookup by the "coin" field returned in API responses
        if let Some(base_currency) = instrument.base_currency() {
            let coin_key = Ustr::from(base_currency.code.as_str());
            instruments.insert(coin_key, instrument);
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
    fn get_or_create_instrument(&self, coin: &Ustr) -> Option<InstrumentAny> {
        // Try to get from cache first
        {
            let instruments = self
                .instruments
                .read()
                .expect("Failed to acquire read lock");
            if let Some(instrument) = instruments.get(coin) {
                return Some(instrument.clone());
            }
        }

        // If not found and it's a vault token, create a synthetic instrument
        if coin.as_str().starts_with("vntls:") {
            tracing::info!("Creating synthetic instrument for vault token: {}", coin);

            let clock = nautilus_core::time::get_atomic_clock_realtime();
            let ts_event = clock.get_time_ns();

            // Create synthetic vault token instrument
            let symbol_str = format!("{}-USDC-SPOT", coin);
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

            // Add to cache for future lookups
            self.add_instrument(instrument.clone());

            Some(instrument)
        } else {
            // For non-vault tokens, log warning and return None
            tracing::warn!("Instrument not found in cache: {}", coin);
            None
        }
    }

    /// Set the account ID for this client.
    ///
    /// This is required for generating reports with the correct account ID.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
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

    /// Submit a single order to the Hyperliquid exchange.
    ///
    /// Takes Nautilus domain types and handles serialization to Hyperliquid format internally.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, order validation fails, serialization fails,
    /// or the API returns an error.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        instrument_id: nautilus_model::identifiers::InstrumentId,
        client_order_id: nautilus_model::identifiers::ClientOrderId,
        order_side: nautilus_model::enums::OrderSide,
        order_type: nautilus_model::enums::OrderType,
        quantity: nautilus_model::types::Quantity,
        time_in_force: nautilus_model::enums::TimeInForce,
        price: Option<nautilus_model::types::Price>,
        trigger_price: Option<nautilus_model::types::Price>,
        post_only: bool,
        reduce_only: bool,
    ) -> Result<HyperliquidExchangeResponse> {
        use nautilus_model::enums::{OrderSide, OrderType, TimeInForce};

        // Extract asset from instrument symbol
        let symbol = instrument_id.symbol.as_str();
        let asset = symbol
            .trim_end_matches("-PERP")
            .trim_end_matches("-USD")
            .to_string();

        // Determine order side (isBuy)
        let is_buy = matches!(order_side, OrderSide::Buy);

        // Determine price
        let limit_px = match (price, order_type) {
            (Some(p), _) => p.to_string(),
            (None, OrderType::Market) => "0".to_string(), // Market orders use IOC with price 0
            _ => return Err(Error::bad_request("Price required for non-market orders")),
        };

        // Determine order type and time-in-force
        let order_type_obj = match order_type {
            OrderType::Market => serde_json::json!({
                "limit": {
                    "tif": "Ioc"
                }
            }),
            OrderType::Limit => {
                let tif = match time_in_force {
                    TimeInForce::Gtc if post_only => "Alo", // Add Liquidity Only for post-only
                    TimeInForce::Gtc => "Gtc",
                    TimeInForce::Ioc => "Ioc",
                    TimeInForce::Fok => "Ioc", // FOK maps to IOC (no native FOK support)
                    _ => "Gtc",                // Default to GTC for other TIF values
                };
                serde_json::json!({
                    "limit": {
                        "tif": tif
                    }
                })
            }
            OrderType::StopLimit | OrderType::StopMarket => {
                // Handle stop/trigger orders
                let trigger_px = trigger_price
                    .ok_or_else(|| Error::bad_request("Trigger price required for stop orders"))?
                    .to_string();
                let is_market = matches!(order_type, OrderType::StopMarket);
                serde_json::json!({
                    "trigger": {
                        "isMarket": is_market,
                        "triggerPx": trigger_px,
                        "tpsl": "tp" // Default to take-profit; can be enhanced later
                    }
                })
            }
            _ => {
                return Err(Error::bad_request(format!(
                    "Unsupported order type: {order_type:?}"
                )));
            }
        };

        // Build the order request
        let order_request = serde_json::json!({
            "asset": asset,
            "isBuy": is_buy,
            "limitPx": limit_px,
            "sz": quantity.to_string(),
            "reduceOnly": reduce_only,
            "orderType": order_type_obj,
            "cloid": client_order_id.to_string(),
        });

        // Create orders array with single order
        let orders_value = serde_json::json!([order_request]);

        // Create exchange action
        let action = ExchangeAction::order(orders_value);

        // Submit to exchange
        self.post_action(&action).await
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

    /// Request order status reports for a user.
    ///
    /// Fetches open orders via `info_frontend_open_orders` and parses them into OrderStatusReports.
    /// This method requires instruments to be added to the client cache via `add_instrument()`.
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
    ) -> Result<Vec<nautilus_model::reports::OrderStatusReport>> {
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
                        tracing::warn!("Failed to parse order: {}", e);
                        continue;
                    }
                };

            // Get instrument from cache or create synthetic for vault tokens
            let instrument = match self.get_or_create_instrument(&order.coin) {
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
            let status = "open";

            // Parse to OrderStatusReport
            match crate::http::parse::parse_order_status_report_from_basic(
                &order,
                status,
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
    /// This method requires instruments to be added to the client cache via `add_instrument()`.
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
    ) -> Result<Vec<nautilus_model::reports::FillReport>> {
        let fills_response = self.info_user_fills(user).await?;

        let mut reports = Vec::new();
        let ts_init = nautilus_core::UnixNanos::default();

        for fill in fills_response {
            // Get instrument from cache or create synthetic for vault tokens
            let instrument = match self.get_or_create_instrument(&fill.coin) {
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
    /// This method requires instruments to be added to the client cache via `add_instrument()`.
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
            let instrument = match self.get_or_create_instrument(&coin_ustr) {
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
    use nautilus_model::instruments::{Instrument, InstrumentAny};
    use rstest::rstest;
    use ustr::Ustr;

    use super::HyperliquidHttpClient;
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

    #[rstest]
    fn test_add_instrument_dual_key_storage() {
        use nautilus_core::time::get_atomic_clock_realtime;
        use nautilus_model::{
            currencies::CURRENCY_MAP,
            enums::CurrencyType,
            identifiers::{InstrumentId, Symbol},
            instruments::CurrencyPair,
            types::{Currency, Price, Quantity},
        };

        let client = HyperliquidHttpClient::new(true, None);

        // Create a test instrument with base currency "vntls:vCURSOR"
        let base_code = "vntls:vCURSOR";
        let quote_code = "USDC";

        // Register the custom currency
        {
            let mut currency_map = CURRENCY_MAP.lock().unwrap();
            if !currency_map.contains_key(base_code) {
                currency_map.insert(
                    base_code.to_string(),
                    Currency::new(base_code, 8, 0, base_code, CurrencyType::Crypto),
                );
            }
        }

        let base_currency = Currency::new(base_code, 8, 0, base_code, CurrencyType::Crypto);
        let quote_currency = Currency::new(quote_code, 6, 0, quote_code, CurrencyType::Crypto);

        let symbol = Symbol::new("vntls:vCURSOR-USDC-SPOT");
        let venue = *crate::common::consts::HYPERLIQUID_VENUE;
        let instrument_id = InstrumentId::new(symbol, venue);

        let clock = get_atomic_clock_realtime();
        let ts = clock.get_time_ns();

        let instrument = InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            symbol,
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

        // Add the instrument
        client.add_instrument(instrument.clone());

        // Verify it can be looked up by Nautilus symbol
        let instruments = client.instruments.read().unwrap();
        let by_symbol = instruments.get(&Ustr::from("vntls:vCURSOR-USDC-SPOT"));
        assert!(
            by_symbol.is_some(),
            "Instrument should be accessible by Nautilus symbol"
        );

        // Verify it can be looked up by Hyperliquid coin identifier (base currency)
        let by_coin = instruments.get(&Ustr::from("vntls:vCURSOR"));
        assert!(
            by_coin.is_some(),
            "Instrument should be accessible by Hyperliquid coin identifier"
        );

        // Verify both lookups return the same instrument
        assert_eq!(by_symbol.unwrap().id(), by_coin.unwrap().id());
    }
}
