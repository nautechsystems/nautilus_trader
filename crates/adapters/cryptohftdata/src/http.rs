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

use std::{
    collections::HashMap,
    fs,
    num::NonZeroU32,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::Bytes;
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use serde::Deserialize;

use crate::{
    config::{CRYPTOHFTDATA_API_KEY_ENV, CRYPTOHFTDATA_BASE_URL, CryptoHFTDataClientConfig},
    enums::{CryptoHFTDataExchange, CryptoHFTDataType},
};

const MAX_DOWNLOAD_RETRIES: usize = 3;
const JWT_REFRESH_MARGIN_SECS: u64 = 300;

#[derive(Clone, Debug)]
struct CachedJwt {
    token: String,
    expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct JwtTokenResponse {
    jwt_token: String,
    expires_in: Option<u64>,
}

/// A single CHD hourly file request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CryptoHFTDataFileRequest {
    /// CHD exchange.
    pub exchange: CryptoHFTDataExchange,
    /// Raw CHD symbol.
    pub symbol: String,
    /// CHD data type.
    pub data_type: CryptoHFTDataType,
    /// UTC date in `YYYY-MM-DD` format.
    pub date: String,
    /// UTC hour in `[0, 23]`.
    pub hour: u8,
}

impl CryptoHFTDataFileRequest {
    /// Creates a new [`CryptoHFTDataFileRequest`].
    ///
    /// # Errors
    ///
    /// Returns an error if `hour` is outside `[0, 23]`.
    pub fn new(
        exchange: CryptoHFTDataExchange,
        symbol: impl Into<String>,
        data_type: CryptoHFTDataType,
        date: impl Into<String>,
        hour: u8,
    ) -> anyhow::Result<Self> {
        if hour > 23 {
            anyhow::bail!("invalid CHD hour {hour}; expected 0..=23");
        }

        Ok(Self {
            exchange,
            symbol: symbol.into(),
            data_type,
            date: date.into(),
            hour,
        })
    }

    /// Returns the CHD object path for this hourly file.
    #[must_use]
    pub fn file_path(&self) -> String {
        format!(
            "{}/{}/{:02}/{}_{}.parquet.zst",
            self.exchange.as_chd_str(),
            self.date,
            self.hour,
            self.symbol,
            self.data_type.as_chd_str(),
        )
    }
}

/// HTTP client for CHD parquet downloads.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.cryptohftdata",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.cryptohftdata")
)]
pub struct CryptoHFTDataClient {
    config: CryptoHFTDataClientConfig,
    http_client: HttpClient,
    jwt_token: Arc<Mutex<Option<CachedJwt>>>,
}

impl CryptoHFTDataClient {
    /// Creates a new [`CryptoHFTDataClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(config: CryptoHFTDataClientConfig) -> anyhow::Result<Self> {
        let timeout_secs = config.timeout_secs;
        let proxy_url = config.proxy_url.clone();
        let default_quota = rate_limit_quota(config.rate_limit_per_sec)?;
        let http_client = HttpClient::new(
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            default_quota,
            timeout_secs,
            proxy_url,
        )?;

        Ok(Self {
            config,
            http_client,
            jwt_token: Arc::new(Mutex::new(None)),
        })
    }

    /// Returns the configured CHD base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or(CRYPTOHFTDATA_BASE_URL)
    }

    /// Returns a masked API key if a key is configured.
    #[must_use]
    pub fn api_key_masked(&self) -> Option<String> {
        self.api_key().as_deref().map(mask_api_key)
    }

    fn api_key(&self) -> Option<String> {
        self.config
            .api_key
            .clone()
            .or_else(|| std::env::var(CRYPTOHFTDATA_API_KEY_ENV).ok())
    }

    fn use_jwt(&self) -> bool {
        self.config.use_jwt.unwrap_or(true)
    }

    fn invalidate_jwt_token(&self) -> anyhow::Result<()> {
        *self
            .jwt_token
            .lock()
            .map_err(|_| anyhow::anyhow!("CHD JWT cache lock poisoned"))? = None;
        Ok(())
    }

    fn cached_jwt_token(&self) -> anyhow::Result<Option<String>> {
        let guard = self
            .jwt_token
            .lock()
            .map_err(|_| anyhow::anyhow!("CHD JWT cache lock poisoned"))?;
        let Some(cached) = guard.as_ref() else {
            return Ok(None);
        };

        let refresh_after = Instant::now() + Duration::from_secs(JWT_REFRESH_MARGIN_SECS);
        if cached.expires_at > refresh_after {
            return Ok(Some(cached.token.clone()));
        }

        Ok(None)
    }

    async fn jwt_token(&self, api_key: &str) -> anyhow::Result<String> {
        if let Some(token) = self.cached_jwt_token()? {
            return Ok(token);
        }

        let token = self.generate_jwt_token(api_key).await?;
        *self
            .jwt_token
            .lock()
            .map_err(|_| anyhow::anyhow!("CHD JWT cache lock poisoned"))? = Some(token.clone());
        Ok(token.token)
    }

    async fn generate_jwt_token(&self, api_key: &str) -> anyhow::Result<CachedJwt> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("X-API-Key".to_string(), api_key.to_string());

        let url = format!("{}/jwt-token", self.base_url().trim_end_matches('/'));
        let response = self
            .http_client
            .post(
                url,
                None,
                Some(headers),
                None,
                self.config.timeout_secs,
                Some(vec!["jwt-token".to_string()]),
            )
            .await?;

        let status = response.status.as_u16();
        if status == 401 || status == 403 {
            anyhow::bail!("CHD JWT authentication failed with HTTP {status}");
        }
        if !(200..300).contains(&status) {
            anyhow::bail!("CHD JWT token request failed with HTTP {status}");
        }

        let token: JwtTokenResponse = serde_json::from_slice(&response.body)?;
        let expires_in = token.expires_in.unwrap_or(3600);
        Ok(CachedJwt {
            token: token.jwt_token,
            expires_at: Instant::now() + Duration::from_secs(expires_in),
        })
    }

    async fn auth_headers(&self, api_key: &str) -> anyhow::Result<(HashMap<String, String>, bool)> {
        if self.use_jwt() {
            match self.jwt_token(api_key).await {
                Ok(token) => {
                    let mut headers = HashMap::new();
                    headers.insert("Authorization".to_string(), format!("Bearer {token}"));
                    return Ok((headers, true));
                }
                Err(e) => {
                    log::warn!("CHD JWT token generation failed; falling back to X-API-Key: {e}");
                }
            }
        }

        let mut headers = HashMap::new();
        headers.insert("X-API-Key".to_string(), api_key.to_string());
        Ok((headers, false))
    }

    /// Downloads a CHD hourly file and returns the raw response body.
    ///
    /// The returned bytes are normally zstd-compressed parquet. Some CHD responses
    /// may already contain plain parquet bytes; loaders detect both forms.
    ///
    /// # Errors
    ///
    /// Returns an error for authentication failures, transport failures, or
    /// unexpected non-success HTTP statuses. A 404 returns `Ok(None)`.
    pub async fn download_file(
        &self,
        request: &CryptoHFTDataFileRequest,
    ) -> anyhow::Result<Option<Bytes>> {
        let api_key = self
            .api_key()
            .ok_or_else(|| anyhow::anyhow!("{CRYPTOHFTDATA_API_KEY_ENV} is required"))?;

        let mut params = HashMap::new();
        params.insert("file".to_string(), vec![request.file_path()]);

        let url = format!("{}/download", self.base_url().trim_end_matches('/'));
        let mut last_error = None;

        for attempt in 0..MAX_DOWNLOAD_RETRIES {
            let (headers, used_jwt) = self.auth_headers(&api_key).await?;
            let response = self
                .http_client
                .get(
                    url.clone(),
                    Some(&params),
                    Some(headers.clone()),
                    self.config.timeout_secs,
                    None,
                )
                .await;

            match response {
                Ok(response) => {
                    let status = response.status.as_u16();
                    if status == 404 {
                        return Ok(None);
                    }
                    if status == 401 || status == 403 {
                        if used_jwt && attempt + 1 < MAX_DOWNLOAD_RETRIES {
                            self.invalidate_jwt_token()?;
                            last_error = Some(anyhow::anyhow!(
                                "CHD bearer token rejected with HTTP {status}"
                            ));
                            continue;
                        }
                        anyhow::bail!("CHD authentication failed with HTTP {status}");
                    }
                    if (200..300).contains(&status) {
                        if response.body.is_empty() {
                            return Ok(None);
                        }
                        return Ok(Some(response.body));
                    }

                    last_error = Some(anyhow::anyhow!("CHD download failed with HTTP {status}"));
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("CHD download request failed: {e}"));
                }
            }

            if attempt + 1 < MAX_DOWNLOAD_RETRIES {
                tokio::time::sleep(std::time::Duration::from_millis(250 * (attempt as u64 + 1)))
                    .await;
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("CHD download failed")))
    }

    /// Downloads a CHD hourly file using a local compressed-file cache when configured.
    ///
    /// # Errors
    ///
    /// Returns an error if cache I/O or the network request fails.
    pub async fn download_file_cached(
        &self,
        request: &CryptoHFTDataFileRequest,
        cache_dir: Option<&Path>,
    ) -> anyhow::Result<Option<Bytes>> {
        let Some(cache_dir) = cache_dir else {
            return self.download_file(request).await;
        };

        let cache_path = cache_dir.join(request.file_path());
        if cache_path.exists() {
            return Ok(Some(fs::read(cache_path)?.into()));
        }

        let Some(bytes) = self.download_file(request).await? else {
            return Ok(None);
        };

        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(cache_path, &bytes)?;

        Ok(Some(bytes))
    }
}

impl Default for CryptoHFTDataClient {
    fn default() -> Self {
        Self::new(CryptoHFTDataClientConfig::default())
            .expect("default CHD HTTP client configuration must be valid")
    }
}

fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".to_string();
    }
    format!("{}...{}", &key[..4], &key[key.len() - 4..])
}

fn rate_limit_quota(limit: Option<usize>) -> anyhow::Result<Option<Quota>> {
    let Some(limit) = limit else {
        return Ok(None);
    };

    let limit = u32::try_from(limit)
        .map_err(|_| anyhow::anyhow!("rate_limit_per_sec must fit into u32"))?;
    let limit = NonZeroU32::new(limit)
        .ok_or_else(|| anyhow::anyhow!("rate_limit_per_sec must be greater than zero"))?;
    let quota = Quota::per_second(limit)
        .ok_or_else(|| anyhow::anyhow!("rate_limit_per_sec is too high"))?;
    Ok(Some(quota))
}

/// Returns a cache path for diagnostics and tests.
#[must_use]
pub fn cache_path_for(cache_dir: &Path, request: &CryptoHFTDataFileRequest) -> PathBuf {
    cache_dir.join(request.file_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_path_uses_chd_layout() {
        let request = CryptoHFTDataFileRequest::new(
            CryptoHFTDataExchange::BinanceFutures,
            "BTCUSDT",
            CryptoHFTDataType::Trades,
            "2025-07-16",
            3,
        )
        .unwrap();

        assert_eq!(
            request.file_path(),
            "binance_futures/2025-07-16/03/BTCUSDT_trades.parquet.zst"
        );
    }

    #[test]
    fn invalid_hour_is_rejected() {
        let result = CryptoHFTDataFileRequest::new(
            CryptoHFTDataExchange::BinanceFutures,
            "BTCUSDT",
            CryptoHFTDataType::Trades,
            "2025-07-16",
            24,
        );

        assert!(result.is_err());
    }
}
