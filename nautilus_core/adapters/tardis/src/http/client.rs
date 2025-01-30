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

use std::{env, time::Duration};

use nautilus_core::{consts::USER_AGENT, UnixNanos};
use nautilus_model::instruments::InstrumentAny;
use reqwest::Response;
use serde::Deserialize;

use super::{parse::parse_instrument_any, types::InstrumentInfo, TARDIS_BASE_URL};
use crate::enums::Exchange;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Deserialize)]
struct TardisErrorResponse {
    code: u64,
    message: String,
}

/// HTTP errors for the Tardis HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("Tardis API error [{code}]: {message}")]
    ApiError {
        status: u16,
        code: u64,
        message: String,
    },

    #[error("Failed to parse response body as JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Failed to parse response as Tardis type: {0}")]
    ResponseParse(String),
}

/// A Tardis HTTP API client.
/// See <https://docs.tardis.dev/api/http>.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
#[derive(Debug, Clone)]
pub struct TardisHttpClient {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
    normalize_symbols: bool,
}

impl TardisHttpClient {
    /// Creates a new [`TardisHttpClient`] instance.
    pub fn new(
        api_key: Option<&str>,
        base_url: Option<&str>,
        timeout_secs: Option<u64>,
        normalize_symbols: bool,
    ) -> anyhow::Result<Self> {
        let api_key = match api_key {
            Some(key) => key.to_string(),
            None => env::var("TARDIS_API_KEY").map_err(|_| {
                anyhow::anyhow!(
                    "API key must be provided or set in the 'TARDIS_API_KEY' environment variable"
                )
            })?,
        };

        let base_url = base_url.map_or_else(|| TARDIS_BASE_URL.to_string(), ToString::to_string);
        let timeout = timeout_secs.map_or_else(|| Duration::from_secs(60), Duration::from_secs);

        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .build()?;

        Ok(Self {
            base_url,
            api_key,
            client,
            normalize_symbols,
        })
    }

    async fn handle_error_response<T>(resp: Response) -> Result<T> {
        let status = resp.status().as_u16();
        let error_text = resp.text().await.unwrap_or_default();

        if let Ok(error) = serde_json::from_str::<TardisErrorResponse>(&error_text) {
            Err(Error::ApiError {
                status,
                code: error.code,
                message: error.message,
            })
        } else {
            Err(Error::ApiError {
                status,
                code: 0,
                message: error_text,
            })
        }
    }

    /// Returns all Tardis instrument definitions for the given `exchange`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>
    pub async fn instruments_info(&self, exchange: Exchange) -> Result<Vec<InstrumentInfo>> {
        tracing::debug!("Requesting instruments for {exchange}");

        let resp = self
            .client
            .get(format!("{}/instruments/{exchange}", &self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Self::handle_error_response(resp).await;
        }

        tracing::debug!("Response status: {}", resp.status());
        let body = resp.text().await?;

        match serde_json::from_str(&body) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                tracing::error!("Failed to parse response: {}", e);
                tracing::debug!("Response body was: {}", body);
                Err(Error::ResponseParse(e.to_string()))
            }
        }
    }

    /// Returns the Tardis instrument definition for a given `exchange` and `symbol`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api#single-instrument-info-endpoint>
    pub async fn instrument_info(
        &self,
        exchange: Exchange,
        symbol: &str,
    ) -> Result<InstrumentInfo> {
        tracing::debug!("Requesting instrument {exchange} {symbol}");

        let resp = self
            .client
            .get(format!(
                "{}/instruments/{exchange}/{symbol}",
                &self.base_url
            ))
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Self::handle_error_response(resp).await;
        }

        tracing::debug!("Response status: {}", resp.status());
        let body = resp.text().await?;

        match serde_json::from_str(&body) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                tracing::error!("Failed to parse response: {}", e);
                tracing::debug!("Response body was: {}", body);
                Err(Error::ResponseParse(e.to_string()))
            }
        }
    }

    /// Returns all Nautilus instrument definitions for the given `exchange`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>
    pub async fn instruments(
        &self,
        exchange: Exchange,
        start: Option<u64>,
        end: Option<u64>,
        ts_init: Option<u64>,
    ) -> Result<Vec<InstrumentAny>> {
        let response = self.instruments_info(exchange).await?;
        let ts_init = ts_init.map(UnixNanos::from);

        Ok(response
            .into_iter()
            .flat_map(|info| {
                parse_instrument_any(info, start, end, ts_init, self.normalize_symbols)
            })
            .collect())
    }

    /// Returns a Nautilus instrument definition for the given `exchange` and `symbol`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>
    pub async fn instrument(
        &self,
        exchange: Exchange,
        symbol: &str,
        start: Option<u64>,
        end: Option<u64>,
        ts_init: Option<u64>,
    ) -> Result<Vec<InstrumentAny>> {
        let response = self.instrument_info(exchange, symbol).await?;
        let ts_init = ts_init.map(UnixNanos::from);

        Ok(parse_instrument_any(
            response,
            start,
            end,
            ts_init,
            self.normalize_symbols,
        ))
    }
}
