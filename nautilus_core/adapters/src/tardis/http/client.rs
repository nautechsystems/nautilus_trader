// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use chrono::Utc;
use nautilus_core::{nanos::UnixNanos, version::USER_AGENT};
use nautilus_model::instruments::any::InstrumentAny;

use super::{
    parse::parse_instrument_any,
    types::{InstrumentInfo, Response},
    TARDIS_BASE_URL,
};
use crate::tardis::enums::Exchange;

pub type Result<T> = std::result::Result<T, Error>;

/// HTTP errors for the Tardis HTTP client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error when sending a request to the server.
    #[error("Error sending request: {0}")]
    Request(#[from] reqwest::Error),
    /// An API error returned by Tardis.
    #[error("Tardis API error {code}: {message}")]
    ApiError { code: u64, message: String },
    /// An error when deserializing the response from the server.
    #[error("Error deserializing message: {0}")]
    Deserialization(#[from] serde_json::Error),
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
}

impl TardisHttpClient {
    /// Creates a new [`TardisHttpClient`] instance.
    pub fn new(
        api_key: Option<&str>,
        base_url: Option<&str>,
        timeout_secs: Option<u64>,
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
        })
    }

    /// Returns all Tardis instrument definitions for the given `exchange`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>
    pub async fn instruments_info(
        &self,
        exchange: Exchange,
    ) -> Result<Response<Vec<InstrumentInfo>>> {
        tracing::debug!("Requesting instruments for {exchange}");

        Ok(self
            .client
            .get(format!("{}/instruments/{exchange}", &self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .json::<Response<Vec<InstrumentInfo>>>()
            .await?)
    }

    /// Returns the Tardis instrument definition for a given `exchange` and `symbol`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api#single-instrument-info-endpoint>
    pub async fn instrument_info(
        &self,
        exchange: Exchange,
        symbol: &str,
    ) -> Result<Response<InstrumentInfo>> {
        tracing::debug!("Requesting instrument {exchange} {symbol}");

        Ok(self
            .client
            .get(format!(
                "{}/instruments/{exchange}/{symbol}",
                &self.base_url
            ))
            .bearer_auth(&self.api_key)
            .send()
            .await?
            .json::<Response<InstrumentInfo>>()
            .await?)
    }

    /// Returns all Nautilus instrument definitions for the given `exchange`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>
    pub async fn instruments(&self, exchange: Exchange) -> Result<Vec<InstrumentAny>> {
        let response = self.instruments_info(exchange).await?;

        let infos = match response {
            Response::Success(data) => data,
            Response::Error { code, message } => {
                return Err(Error::ApiError { code, message });
            }
        };

        let now = Utc::now();
        let ts_init = UnixNanos::from(now.timestamp_nanos_opt().unwrap() as u64);

        infos
            .into_iter()
            .map(|info| Ok(parse_instrument_any(info, ts_init)))
            .collect()
    }

    /// Returns a Nautilus instrument definition for the given `exchange` and `symbol`.
    /// See <https://docs.tardis.dev/api/instruments-metadata-api>
    pub async fn instrument(&self, exchange: Exchange, symbol: &str) -> Result<InstrumentAny> {
        let response = self.instrument_info(exchange, symbol).await?;

        let info = match response {
            Response::Success(data) => data,
            Response::Error { code, message } => {
                return Err(Error::ApiError { code, message });
            }
        };

        let now = Utc::now();
        let ts_init = UnixNanos::from(now.timestamp_nanos_opt().unwrap() as u64);

        Ok(parse_instrument_any(info, ts_init))
    }
}
