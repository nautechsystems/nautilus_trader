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

//! Binance Futures HTTP client for USD-M and COIN-M markets.
//!
//! This client wraps the generic [`BinanceHttpClient`] and provides futures-specific
//! endpoints such as mark price, funding rate, and open interest.

use nautilus_core::nanos::UnixNanos;
use nautilus_model::instruments::any::InstrumentAny;
use serde::{Deserialize, Serialize};

use crate::{
    common::{
        enums::{BinanceEnvironment, BinanceProductType},
        parse::parse_usdm_instrument,
    },
    http::{
        client::BinanceHttpClient,
        error::{BinanceHttpError, BinanceHttpResult},
        models::{
            BinanceBookTicker, BinanceFuturesMarkPrice, BinanceFuturesTicker24hr,
            BinanceFuturesUsdExchangeInfo, BinanceOrderBook,
        },
        query::{BinanceBookTickerParams, BinanceDepthParams, BinanceTicker24hrParams},
    },
};

/// Query parameters for mark price endpoints.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkPriceParams {
    /// Trading symbol (optional - if omitted, returns all symbols).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Response wrapper for mark price endpoint.
///
/// Binance returns a single object when symbol is provided, or an array when omitted.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MarkPriceResponse {
    /// Single mark price when querying specific symbol.
    Single(BinanceFuturesMarkPrice),
    /// Multiple mark prices when querying all symbols.
    Multiple(Vec<BinanceFuturesMarkPrice>),
}

impl From<MarkPriceResponse> for Vec<BinanceFuturesMarkPrice> {
    fn from(response: MarkPriceResponse) -> Self {
        match response {
            MarkPriceResponse::Single(price) => vec![price],
            MarkPriceResponse::Multiple(prices) => prices,
        }
    }
}

/// Query parameters for funding rate history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingRateParams {
    /// Trading symbol.
    pub symbol: String,
    /// Start time in milliseconds (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Limit results (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for open interest endpoints.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenInterestParams {
    /// Trading symbol.
    pub symbol: String,
}

/// Open interest response from `/fapi/v1/openInterest`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceOpenInterest {
    /// Trading symbol.
    pub symbol: String,
    /// Total open interest.
    pub open_interest: String,
    /// Response timestamp.
    pub time: i64,
}

/// Funding rate history entry from `/fapi/v1/fundingRate`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFundingRate {
    /// Trading symbol.
    pub symbol: String,
    /// Funding rate value.
    pub funding_rate: String,
    /// Funding time in milliseconds.
    pub funding_time: i64,
    /// Mark price at funding time.
    #[serde(default)]
    pub mark_price: Option<String>,
}

/// Binance Futures HTTP client for USD-M and COIN-M perpetuals.
///
/// This client wraps the generic HTTP client and provides convenience methods
/// for futures-specific endpoints. Use [`BinanceProductType::UsdM`] for USD-margined
/// or [`BinanceProductType::CoinM`] for coin-margined futures.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance")
)]
pub struct BinanceFuturesHttpClient {
    inner: BinanceHttpClient,
    product_type: BinanceProductType,
}

impl BinanceFuturesHttpClient {
    /// Creates a new [`BinanceFuturesHttpClient`] instance.
    ///
    /// # Arguments
    ///
    /// * `product_type` - Must be `UsdM` or `CoinM`.
    /// * `environment` - Mainnet or testnet.
    /// * `api_key` - Optional API key for authenticated endpoints.
    /// * `api_secret` - Optional API secret for signing requests.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `product_type` is not a futures type (UsdM or CoinM).
    /// - Credential creation fails.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        product_type: BinanceProductType,
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceHttpResult<Self> {
        match product_type {
            BinanceProductType::UsdM | BinanceProductType::CoinM => {}
            _ => {
                return Err(BinanceHttpError::ValidationError(format!(
                    "BinanceFuturesHttpClient requires UsdM or CoinM product type, got {product_type:?}"
                )));
            }
        }

        let inner = BinanceHttpClient::new(
            product_type,
            environment,
            api_key,
            api_secret,
            base_url_override,
            recv_window,
            timeout_secs,
            proxy_url,
        )?;

        Ok(Self {
            inner,
            product_type,
        })
    }

    /// Returns the product type (UsdM or CoinM).
    #[must_use]
    pub const fn product_type(&self) -> BinanceProductType {
        self.product_type
    }

    /// Returns a reference to the underlying generic HTTP client.
    #[must_use]
    pub const fn inner(&self) -> &BinanceHttpClient {
        &self.inner
    }

    /// Fetches exchange information and populates the instrument cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn exchange_info(&self) -> BinanceHttpResult<()> {
        self.inner.exchange_info().await
    }

    /// Fetches exchange information and returns parsed Nautilus instruments.
    ///
    /// Only returns perpetual contracts. Non-perpetual contracts (quarterly futures)
    /// are filtered out.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn instruments(&self) -> BinanceHttpResult<Vec<InstrumentAny>> {
        let info: BinanceFuturesUsdExchangeInfo = self
            .inner
            .raw()
            .get("exchangeInfo", None::<&()>, false, false)
            .await?;

        let ts_init = UnixNanos::default();
        let mut instruments = Vec::with_capacity(info.symbols.len());

        for symbol in &info.symbols {
            match parse_usdm_instrument(symbol, ts_init, ts_init) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => {
                    // Log and skip non-perpetual or malformed symbols
                    tracing::debug!(
                        symbol = %symbol.symbol,
                        error = %e,
                        "Skipping symbol during instrument parsing"
                    );
                }
            }
        }

        tracing::info!(
            count = instruments.len(),
            "Loaded USD-M perpetual instruments"
        );
        Ok(instruments)
    }

    /// Fetches 24hr ticker statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn ticker_24h(
        &self,
        params: &BinanceTicker24hrParams,
    ) -> BinanceHttpResult<Vec<BinanceFuturesTicker24hr>> {
        self.inner
            .raw()
            .get("ticker/24hr", Some(params), false, false)
            .await
    }

    /// Fetches best bid/ask prices.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn book_ticker(
        &self,
        params: &BinanceBookTickerParams,
    ) -> BinanceHttpResult<Vec<BinanceBookTicker>> {
        self.inner.book_ticker(params).await
    }

    /// Fetches order book depth.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn depth(&self, params: &BinanceDepthParams) -> BinanceHttpResult<BinanceOrderBook> {
        self.inner.depth(params).await
    }

    /// Fetches mark price and funding rate.
    ///
    /// If `symbol` is None, returns mark price for all symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn mark_price(
        &self,
        params: &MarkPriceParams,
    ) -> BinanceHttpResult<Vec<BinanceFuturesMarkPrice>> {
        let response: MarkPriceResponse = self
            .inner
            .raw()
            .get("premiumIndex", Some(params), false, false)
            .await?;
        Ok(response.into())
    }

    /// Fetches funding rate history.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn funding_rate(
        &self,
        params: &FundingRateParams,
    ) -> BinanceHttpResult<Vec<BinanceFundingRate>> {
        self.inner
            .raw()
            .get("fundingRate", Some(params), false, false)
            .await
    }

    /// Fetches current open interest for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn open_interest(
        &self,
        params: &OpenInterestParams,
    ) -> BinanceHttpResult<BinanceOpenInterest> {
        self.inner
            .raw()
            .get("openInterest", Some(params), false, false)
            .await
    }

    /// Creates a listen key for user data stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or credentials are missing.
    pub async fn create_listen_key(&self) -> BinanceHttpResult<ListenKeyResponse> {
        self.inner
            .raw()
            .post::<(), ListenKeyResponse>("listenKey", None, None, true, false)
            .await
    }

    /// Keeps alive an existing listen key.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn keepalive_listen_key(&self, listen_key: &str) -> BinanceHttpResult<()> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        let _: serde_json::Value = self
            .inner
            .raw()
            .request_put("listenKey", Some(&params), true, false)
            .await?;
        Ok(())
    }

    /// Closes an existing listen key.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn close_listen_key(&self, listen_key: &str) -> BinanceHttpResult<()> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        let _: serde_json::Value = self
            .inner
            .raw()
            .request_delete("listenKey", Some(&params), true, false)
            .await?;
        Ok(())
    }
}

/// Listen key response from user data stream endpoints.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyResponse {
    /// The listen key for WebSocket user data stream.
    pub listen_key: String,
}

/// Listen key request parameters.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListenKeyParams {
    listen_key: String,
}
