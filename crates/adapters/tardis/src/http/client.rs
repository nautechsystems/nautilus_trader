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

use nautilus_core::{UnixNanos, consts::NAUTILUS_USER_AGENT};
use nautilus_model::instruments::InstrumentAny;
use reqwest::Response;

use super::{
    TARDIS_BASE_URL,
    error::{Error, TardisErrorResponse},
    instruments::is_available,
    models::TardisInstrumentInfo,
    parse::parse_instrument_any,
    query::InstrumentFilter,
};
use crate::enums::TardisExchange;

pub type Result<T> = std::result::Result<T, Error>;

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
    ///
    /// # Errors
    ///
    /// Returns an error if no API key is provided (argument or `TARDIS_API_KEY` env var),
    /// or if the HTTP client cannot be built.
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
            .user_agent(NAUTILUS_USER_AGENT)
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
        let error_text = match resp.text().await {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!("Failed to extract error response body: {e}");
                String::from("Failed to extract error response")
            }
        };

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
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    ///
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>.
    pub async fn instruments_info(
        &self,
        exchange: TardisExchange,
        symbol: Option<&str>,
        filter: Option<&InstrumentFilter>,
    ) -> Result<Vec<TardisInstrumentInfo>> {
        let mut url = format!("{}/instruments/{exchange}", &self.base_url);
        if let Some(symbol) = symbol {
            url.push_str(&format!("/{symbol}"));
        }
        if let Some(filter) = filter
            && let Ok(filter_json) = serde_json::to_string(filter)
        {
            url.push_str(&format!("?filter={}", urlencoding::encode(&filter_json)));
        }
        tracing::debug!("Requesting: {url}");

        let resp = self
            .client
            .get(url)
            .bearer_auth(&self.api_key)
            .send()
            .await?;
        tracing::debug!("Response status: {}", resp.status());

        if !resp.status().is_success() {
            return Self::handle_error_response(resp).await;
        }

        let body = resp.text().await?;
        tracing::trace!("{body}");

        if let Ok(instrument) = serde_json::from_str::<TardisInstrumentInfo>(&body) {
            return Ok(vec![instrument]);
        }

        match serde_json::from_str(&body) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                tracing::error!("Failed to parse response: {e}");
                tracing::debug!("Response body was: {body}");
                Err(Error::ResponseParse(e.to_string()))
            }
        }
    }

    /// Returns all Nautilus instrument definitions for the given `exchange`, and filter params.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching instrument info or parsing into domain types fails.
    ///
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>.
    #[allow(clippy::too_many_arguments)]
    pub async fn instruments(
        &self,
        exchange: TardisExchange,
        symbol: Option<&str>,
        filter: Option<&InstrumentFilter>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        available_offset: Option<UnixNanos>,
        effective: Option<UnixNanos>,
        ts_init: Option<UnixNanos>,
    ) -> Result<Vec<InstrumentAny>> {
        let response = self.instruments_info(exchange, symbol, filter).await?;

        Ok(response
            .into_iter()
            .filter(|info| is_available(info, start, end, available_offset, effective))
            .flat_map(|info| parse_instrument_any(info, effective, ts_init, self.normalize_symbols))
            .collect())
    }
}
