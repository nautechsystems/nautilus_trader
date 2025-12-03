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

//! HTTP client for Gate.io REST API.

use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::http::{HttpClient, HttpResponse};
use reqwest::Method;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error};

use crate::common::{
    credential::GateioCredentials,
    models::{
        GateioFuturesAccount, GateioFuturesContract, GateioOrder, GateioOrderBook,
        GateioSpotAccount, GateioSpotCurrencyPair, GateioTrade,
    },
    parse::{parse_futures_instrument, parse_spot_instrument},
    urls::GateioUrls,
};

use super::error::{GateioHttpError, GateioHttpResult};

/// Inner state for the Gate.io HTTP client.
struct GateioHttpClientInner {
    client: HttpClient,
    urls: GateioUrls,
    credentials: Option<GateioCredentials>,
    instruments: RwLock<HashMap<String, InstrumentAny>>,
}

/// HTTP client for interacting with the Gate.io REST API.
#[derive(Clone)]
pub struct GateioHttpClient {
    inner: Arc<GateioHttpClientInner>,
}

impl GateioHttpClient {
    /// Creates a new Gate.io HTTP client.
    ///
    /// # Arguments
    ///
    /// * `base_http_url` - Optional base HTTP URL (uses default if None)
    /// * `base_ws_spot_url` - Optional base WebSocket spot URL
    /// * `base_ws_futures_url` - Optional base WebSocket futures URL
    /// * `base_ws_options_url` - Optional base WebSocket options URL
    /// * `credentials` - Optional credentials for authenticated requests
    #[must_use]
    pub fn new(
        base_http_url: Option<String>,
        base_ws_spot_url: Option<String>,
        base_ws_futures_url: Option<String>,
        base_ws_options_url: Option<String>,
        credentials: Option<GateioCredentials>,
    ) -> Self {
        let urls = GateioUrls::new(
            base_http_url,
            base_ws_spot_url,
            base_ws_futures_url,
            base_ws_options_url,
        );

        // Create HTTP client with default configuration
        let client = HttpClient::new(
            HashMap::new(), // headers
            Vec::new(),     // header_keys
            Vec::new(),     // keyed_quotas
            None,           // default_quota
            None,           // timeout_secs
            None,           // proxy_url
        )
        .expect("Failed to create HTTP client");

        Self {
            inner: Arc::new(GateioHttpClientInner {
                client,
                urls,
                credentials,
                instruments: RwLock::new(HashMap::new()),
            }),
        }
    }

    /// Returns a reference to the loaded instruments.
    pub async fn instruments(&self) -> HashMap<String, InstrumentAny> {
        self.inner.instruments.read().await.clone()
    }

    /// Fetches spot currency pairs from Gate.io.
    pub async fn request_spot_currency_pairs(&self) -> GateioHttpResult<Vec<GateioSpotCurrencyPair>> {
        let url = self.inner.urls.spot_currency_pairs();
        let response = self.send_request(Method::GET, &url, "", "", false).await?;

        serde_json::from_slice(&response.body)
            .map_err(|e| GateioHttpError::JsonError(e.to_string()))
    }

    /// Fetches futures contracts from Gate.io.
    ///
    /// # Arguments
    ///
    /// * `settle` - Settlement currency (e.g., "usdt", "btc")
    pub async fn request_futures_contracts(
        &self,
        settle: &str,
    ) -> GateioHttpResult<Vec<GateioFuturesContract>> {
        let url = self.inner.urls.futures_contracts(settle);
        let response = self.send_request(Method::GET, &url, "", "", false).await?;

        serde_json::from_slice(&response.body)
            .map_err(|e| GateioHttpError::JsonError(e.to_string()))
    }

    /// Loads all instruments (spot and futures) and stores them.
    pub async fn load_instruments(&self) -> GateioHttpResult<Vec<InstrumentAny>> {
        let mut instruments = Vec::new();

        // Load spot instruments
        match self.request_spot_currency_pairs().await {
            Ok(pairs) => {
                for pair in pairs {
                    match parse_spot_instrument(&pair) {
                        Ok(instrument) => instruments.push(instrument),
                        Err(e) => error!("Failed to parse spot instrument {}: {}", pair.id, e),
                    }
                }
            }
            Err(e) => error!("Failed to fetch spot currency pairs: {}", e),
        }

        // Load USDT-margined futures
        match self.request_futures_contracts("usdt").await {
            Ok(contracts) => {
                for contract in contracts {
                    match parse_futures_instrument(&contract) {
                        Ok(instrument) => instruments.push(instrument),
                        Err(e) => error!("Failed to parse futures contract {}: {}", contract.name, e),
                    }
                }
            }
            Err(e) => error!("Failed to fetch USDT futures contracts: {}", e),
        }

        // Store instruments
        let mut instruments_map = self.inner.instruments.write().await;
        for instrument in &instruments {
            let symbol = instrument.id().symbol.to_string();
            instruments_map.insert(symbol, instrument.clone());
        }

        debug!("Loaded {} instruments", instruments.len());
        Ok(instruments)
    }

    /// Requests spot account information (authenticated).
    pub async fn request_spot_account(&self) -> GateioHttpResult<GateioSpotAccount> {
        self.check_credentials()?;
        let url = self.inner.urls.spot_accounts();
        let response = self.send_request(Method::GET, &url, "", "", true).await?;

        // Gate.io returns array of balances directly
        let balances = serde_json::from_slice(&response.body)
            .map_err(|e| GateioHttpError::JsonError(e.to_string()))?;

        Ok(GateioSpotAccount { balances })
    }

    /// Requests futures account information (authenticated).
    ///
    /// # Arguments
    ///
    /// * `settle` - Settlement currency (e.g., "usdt", "btc")
    pub async fn request_futures_account(
        &self,
        settle: &str,
    ) -> GateioHttpResult<GateioFuturesAccount> {
        self.check_credentials()?;
        let url = self.inner.urls.futures_accounts(settle);
        let response = self.send_request(Method::GET, &url, "", "", true).await?;

        serde_json::from_slice(&response.body)
            .map_err(|e| GateioHttpError::JsonError(e.to_string()))
    }

    /// Requests spot order book for a currency pair.
    pub async fn request_spot_order_book(
        &self,
        currency_pair: &str,
    ) -> GateioHttpResult<GateioOrderBook> {
        let url = self.inner.urls.spot_order_book(currency_pair);
        let response = self.send_request(Method::GET, &url, "", "", false).await?;

        serde_json::from_slice(&response.body)
            .map_err(|e| GateioHttpError::JsonError(e.to_string()))
    }

    /// Requests spot trades for a currency pair.
    pub async fn request_spot_trades(
        &self,
        currency_pair: &str,
    ) -> GateioHttpResult<Vec<GateioTrade>> {
        let url = self.inner.urls.spot_trades(currency_pair);
        let response = self.send_request(Method::GET, &url, "", "", false).await?;

        serde_json::from_slice(&response.body)
            .map_err(|e| GateioHttpError::JsonError(e.to_string()))
    }

    /// Sends an HTTP request to Gate.io API.
    ///
    /// # Arguments
    ///
    /// * `method` - HTTP method
    /// * `url` - Full URL
    /// * `query_string` - Query string parameters
    /// * `body` - Request body
    /// * `requires_auth` - Whether authentication is required
    async fn send_request(
        &self,
        method: Method,
        url: &str,
        query_string: &str,
        body: &str,
        requires_auth: bool,
    ) -> GateioHttpResult<HttpResponse> {
        // Parse URL to get path
        let parsed_url = url::Url::parse(url)
            .map_err(|e| GateioHttpError::InvalidRequest(e.to_string()))?;
        let url_path = parsed_url.path();

        // Build headers
        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        if requires_auth {
            if let Some(credentials) = &self.inner.credentials {
                let (signature, timestamp) = credentials.sign_request(
                    method.as_str(),
                    url_path,
                    query_string,
                    body,
                );

                headers.insert("KEY".to_string(), credentials.api_key().to_string());
                headers.insert("Timestamp".to_string(), timestamp.to_string());
                headers.insert("SIGN".to_string(), signature);
            } else {
                return Err(GateioHttpError::AuthError(
                    "Credentials required for authenticated request".to_string(),
                ));
            }
        }

        let response = self
            .inner
            .client
            .request(
                method,
                url.to_string(),
                None,
                Some(headers),
                Some(body.as_bytes().to_vec()),
                None,
                None,
            )
            .await
            .map_err(|e| GateioHttpError::HttpError(e.to_string()))?;

        // Check for API errors
        if response.status.as_u16() >= 400 {
            let error_msg = String::from_utf8_lossy(&response.body).to_string();
            return Err(GateioHttpError::ApiError {
                label: format!("{}", response.status.as_u16()),
                message: error_msg,
            });
        }

        Ok(response)
    }

    /// Checks if credentials are set.
    fn check_credentials(&self) -> GateioHttpResult<()> {
        if self.inner.credentials.is_none() {
            return Err(GateioHttpError::AuthError(
                "Credentials not set".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = GateioHttpClient::new(None, None, None, None, None);
        assert_eq!(client.inner.credentials.is_none(), true);
    }

    #[test]
    fn test_client_with_credentials() {
        let credentials =
            GateioCredentials::new("test_key".to_string(), "test_secret".to_string()).unwrap();
        let client = GateioHttpClient::new(None, None, None, None, Some(credentials));
        assert_eq!(client.inner.credentials.is_some(), true);
    }
}
