use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::http::HttpClient;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, error};

use crate::common::{
    parse::{parse_futures_instrument, parse_spot_instrument},
    AsterdexCredentials, AsterdexUrls,
};
use crate::http::error::{AsterdexApiError, AsterdexHttpError};

pub struct AsterdexHttpClientInner {
    pub client: HttpClient,
    pub urls: AsterdexUrls,
    pub credentials: Option<AsterdexCredentials>,
    pub instruments: RwLock<HashMap<String, InstrumentAny>>,
}

#[derive(Clone)]
pub struct AsterdexHttpClient {
    inner: Arc<AsterdexHttpClientInner>,
}

impl AsterdexHttpClient {
    pub fn new(
        base_url_http_spot: Option<String>,
        base_url_http_futures: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> anyhow::Result<Self> {
        let client = HttpClient::new(
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        )
        .expect("Failed to create HTTP client");

        let urls = AsterdexUrls::new(
            base_url_http_spot,
            base_url_http_futures,
            None,
            None,
        );

        let credentials = if let (Some(key), Some(secret)) = (api_key, api_secret) {
            Some(AsterdexCredentials::new(key, secret)?)
        } else {
            None
        };

        Ok(Self {
            inner: Arc::new(AsterdexHttpClientInner {
                client,
                urls,
                credentials,
                instruments: RwLock::new(HashMap::new()),
            }),
        })
    }

    // ========== Spot Market Endpoints ==========

    pub async fn request_spot_exchange_info(&self) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.spot_exchange_info();
        self.get_public(&url).await
    }

    pub async fn request_spot_order_book(
        &self,
        symbol: &str,
        limit: Option<u32>,
    ) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.spot_order_book(symbol, limit);
        self.get_public(&url).await
    }

    pub async fn request_spot_trades(&self, symbol: &str) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.spot_trades(symbol);
        self.get_public(&url).await
    }

    pub async fn request_spot_account(&self) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.spot_account();
        self.get_private(&url, &[]).await
    }

    // ========== Futures Market Endpoints ==========

    pub async fn request_futures_exchange_info(&self) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.futures_exchange_info();
        self.get_public(&url).await
    }

    pub async fn request_futures_order_book(
        &self,
        symbol: &str,
        limit: Option<u32>,
    ) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.futures_order_book(symbol, limit);
        self.get_public(&url).await
    }

    pub async fn request_futures_trades(&self, symbol: &str) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.futures_trades(symbol);
        self.get_public(&url).await
    }

    pub async fn request_futures_account(&self) -> Result<Value, AsterdexHttpError> {
        let url = self.inner.urls.futures_account();
        self.get_private(&url, &[]).await
    }

    // ========== Instrument Loading ==========

    pub async fn load_instruments(&self) -> Result<Vec<InstrumentAny>, AsterdexHttpError> {
        let mut instruments = Vec::new();
        let mut instruments_map = self.inner.instruments.write().await;

        // Load spot instruments
        match self.request_spot_exchange_info().await {
            Ok(info) => {
                if let Some(symbols) = info.get("symbols").and_then(|s| s.as_array()) {
                    for symbol_data in symbols {
                        match serde_json::from_value(symbol_data.clone()) {
                            Ok(symbol) => match parse_spot_instrument(&symbol) {
                                Ok(instrument) => {
                                    let symbol = instrument.id().symbol.to_string();
                                    instruments_map.insert(symbol, instrument.clone());
                                    instruments.push(instrument);
                                }
                                Err(e) => {
                                    error!("Failed to parse spot instrument: {}", e);
                                }
                            },
                            Err(e) => {
                                error!("Failed to deserialize spot symbol: {}", e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to load spot exchange info: {}", e);
            }
        }

        // Load futures instruments
        match self.request_futures_exchange_info().await {
            Ok(info) => {
                if let Some(symbols) = info.get("symbols").and_then(|s| s.as_array()) {
                    for symbol_data in symbols {
                        match serde_json::from_value(symbol_data.clone()) {
                            Ok(symbol) => match parse_futures_instrument(&symbol) {
                                Ok(instrument) => {
                                    let symbol = instrument.id().symbol.to_string();
                                    instruments_map.insert(symbol, instrument.clone());
                                    instruments.push(instrument);
                                }
                                Err(e) => {
                                    error!("Failed to parse futures instrument: {}", e);
                                }
                            },
                            Err(e) => {
                                error!("Failed to deserialize futures symbol: {}", e);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to load futures exchange info: {}", e);
            }
        }

        debug!("Loaded {} instruments from Asterdex", instruments.len());
        Ok(instruments)
    }

    /// Returns the loaded instruments.
    pub async fn instruments(&self) -> Vec<InstrumentAny> {
        let instruments_map = self.inner.instruments.read().await;
        instruments_map.values().cloned().collect()
    }

    // ========== Private Helper Methods ==========

    async fn get_public(&self, url: &str) -> Result<Value, AsterdexHttpError> {
        debug!("GET (public) {}", url);

        let response = self
            .inner
            .client
            .request(
                reqwest::Method::GET,
                url.to_string(),
                None,
                None,
                None,
                None,
            )
            .await
            .map_err(|e| AsterdexHttpError::HttpClient(e.to_string()))?;

        if response.status.as_u16() >= 400 {
            return Err(AsterdexHttpError::ApiError {
                code: response.status.as_u16() as i32,
                msg: format!("HTTP {}", response.status.as_u16()),
            });
        }

        let body = String::from_utf8_lossy(&response.body);
        let json: Value = serde_json::from_str(&body)?;

        // Check for API error
        if let Some(code) = json.get("code").and_then(|c| c.as_i64()) {
            if code != 200 {
                let msg = json
                    .get("msg")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();
                return Err(AsterdexHttpError::ApiError {
                    code: code as i32,
                    msg,
                });
            }
        }

        Ok(json)
    }

    async fn get_private(
        &self,
        url: &str,
        params: &[(&str, String)],
    ) -> Result<Value, AsterdexHttpError> {
        let credentials = self.inner.credentials.as_ref().ok_or_else(|| {
            AsterdexHttpError::HttpClient("No credentials provided for private endpoint".into())
        })?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // Build query string with timestamp
        let mut all_params = params.to_vec();
        all_params.push(("timestamp", timestamp.to_string()));

        let query_string = all_params
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");

        // Sign the request
        let signature = credentials.sign_request(&query_string);

        // Append signature to query string
        let signed_query = format!("{}&signature={}", query_string, signature);
        let full_url = format!("{}?{}", url, signed_query);

        debug!("GET (private) {}", url);

        // Prepare headers with API key
        let mut headers = HashMap::new();
        headers.insert("X-MBX-APIKEY".to_string(), credentials.api_key().to_string());

        let response = self
            .inner
            .client
            .request(
                reqwest::Method::GET,
                full_url,
                Some(headers),
                None,
                None,
                None,
            )
            .await
            .map_err(|e| AsterdexHttpError::HttpClient(e.to_string()))?;

        if response.status.as_u16() >= 400 {
            let body = String::from_utf8_lossy(&response.body);
            if let Ok(api_error) = serde_json::from_str::<AsterdexApiError>(&body) {
                return Err(AsterdexHttpError::ApiError {
                    code: api_error.code,
                    msg: api_error.msg,
                });
            }
            return Err(AsterdexHttpError::ApiError {
                code: response.status.as_u16() as i32,
                msg: format!("HTTP {}", response.status.as_u16()),
            });
        }

        let body = String::from_utf8_lossy(&response.body);
        let json: Value = serde_json::from_str(&body)?;

        // Check for API error in response
        if let Some(code) = json.get("code").and_then(|c| c.as_i64()) {
            if code != 200 {
                let msg = json
                    .get("msg")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();
                return Err(AsterdexHttpError::ApiError {
                    code: code as i32,
                    msg,
                });
            }
        }

        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_client_creation() {
        let client = AsterdexHttpClient::new(None, None, None, None);
        assert!(client.is_ok());
    }

    #[test]
    fn test_http_client_with_credentials() {
        let client = AsterdexHttpClient::new(
            None,
            None,
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
        );
        assert!(client.is_ok());
    }
}
