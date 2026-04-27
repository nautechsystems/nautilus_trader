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

use std::{collections::HashMap, fmt::Debug, sync::Arc};

use ahash::{AHashMap, AHashSet};
use nautilus_core::{
    UnixNanos,
    consts::NAUTILUS_USER_AGENT,
    string::{parsing::precision_from_str, secret::REDACTED, urlencoding},
};
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::http::HttpClient;
use ustr::Ustr;

use super::{
    error::{Error, TardisErrorResponse},
    instruments::is_available,
    models::TardisInstrumentInfo,
    parse::parse_instrument_any,
    query::InstrumentFilter,
};
use crate::{
    common::{
        consts::{TARDIS_REST_QUOTA, TARDIS_REST_RATE_KEY},
        credential::Credential,
        enums::TardisExchange,
        parse::{normalize_instrument_id, parse_instrument_id},
        urls::TARDIS_HTTP_BASE_URL,
    },
    machine::types::{TardisInstrumentKey, TardisInstrumentMiniInfo},
};

pub type Result<T> = std::result::Result<T, Error>;

/// A Tardis HTTP API client.
/// See <https://docs.tardis.dev/api/http>.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.tardis", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.tardis")
)]
#[derive(Clone)]
pub struct TardisHttpClient {
    base_url: String,
    credential: Option<Credential>,
    client: HttpClient,
    normalize_symbols: bool,
}

impl Debug for TardisHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TardisHttpClient))
            .field("base_url", &self.base_url)
            .field("credential", &self.credential.as_ref().map(|_| REDACTED))
            .field("normalize_symbols", &self.normalize_symbols)
            .finish()
    }
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
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let credential = Credential::resolve(api_key.map(ToString::to_string));

        if credential.is_none() {
            anyhow::bail!(
                "API key must be provided or set in the 'TARDIS_API_KEY' environment variable"
            );
        }

        let base_url =
            base_url.map_or_else(|| TARDIS_HTTP_BASE_URL.to_string(), ToString::to_string);

        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string());

        if let Some(ref cred) = credential {
            headers.insert(
                "Authorization".to_string(),
                format!("Bearer {}", cred.api_key()),
            );
        }

        let keyed_quotas = vec![(TARDIS_REST_RATE_KEY.to_string(), *TARDIS_REST_QUOTA)];
        let client = HttpClient::new(
            headers,
            vec![],
            keyed_quotas,
            Some(*TARDIS_REST_QUOTA),
            timeout_secs.or(Some(60)),
            proxy_url,
        )?;

        Ok(Self {
            base_url,
            credential,
            client,
            normalize_symbols,
        })
    }

    /// Returns the credential associated with this client.
    #[must_use]
    pub const fn credential(&self) -> Option<&Credential> {
        self.credential.as_ref()
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
        log::debug!("Requesting: {url}");

        let rate_keys = Some(vec![TARDIS_REST_RATE_KEY.to_string()]);
        let response = self
            .client
            .get(url, None, None, None, rate_keys)
            .await
            .map_err(|e| Error::Request(e.to_string()))?;

        let status = response.status.as_u16();
        log::debug!("Response status: {status}");

        if !response.status.is_success() {
            let body = String::from_utf8_lossy(&response.body).to_string();
            return if let Ok(error) = serde_json::from_str::<TardisErrorResponse>(&body) {
                Err(Error::ApiError {
                    status,
                    code: error.code,
                    message: error.message,
                })
            } else {
                Err(Error::ApiError {
                    status,
                    code: 0,
                    message: body,
                })
            };
        }

        let body = String::from_utf8_lossy(&response.body);
        log::trace!("{body}");

        if let Ok(instrument) = serde_json::from_str::<TardisInstrumentInfo>(&body) {
            return Ok(vec![instrument]);
        }

        match serde_json::from_str(&body) {
            Ok(parsed) => Ok(parsed),
            Err(e) => {
                log::error!("Failed to parse response: {e}");
                log::debug!("Response body was: {body}");
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
    #[expect(clippy::too_many_arguments)]
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
            .flat_map(|info| {
                parse_instrument_any(&info, effective, ts_init, self.normalize_symbols)
            })
            .collect())
    }

    /// Fetches instruments for the given exchanges, builds the mini-info map
    /// for WS message parsing, and parses Nautilus instrument definitions.
    ///
    /// Returns a tuple of `(instrument_map, nautilus_instruments)`. The caller
    /// decides how to use each half: `data.rs` emits instruments via the data
    /// sender; `replay.rs` only needs the map.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching instrument info for any exchange fails.
    pub async fn bootstrap_instruments(
        &self,
        exchanges: &AHashSet<TardisExchange>,
    ) -> Result<(
        AHashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>>,
        Vec<InstrumentAny>,
    )> {
        let mut instrument_map: AHashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>> =
            AHashMap::new();
        let mut nautilus_instruments: Vec<InstrumentAny> = Vec::new();

        for exchange in exchanges {
            log::info!("Fetching instruments for {exchange}");

            let instruments_info = match self.instruments_info(*exchange, None, None).await {
                Ok(info) => info,
                Err(e) => {
                    log::error!("Failed to fetch instruments for {exchange}: {e}");
                    continue;
                }
            };

            log::info!(
                "Received {} instruments for {exchange}",
                instruments_info.len()
            );

            for inst in &instruments_info {
                let instrument_type = inst.instrument_type;
                let price_precision = precision_from_str(&inst.price_increment.to_string());
                let size_precision = precision_from_str(&inst.amount_increment.to_string());

                let instrument_id = if self.normalize_symbols {
                    normalize_instrument_id(exchange, inst.id, &instrument_type, inst.inverse)
                } else {
                    parse_instrument_id(exchange, inst.id)
                };

                let info = TardisInstrumentMiniInfo::new(
                    instrument_id,
                    Some(Ustr::from(&inst.id)),
                    *exchange,
                    price_precision,
                    size_precision,
                );
                let key = info.as_tardis_instrument_key();
                instrument_map.insert(key, Arc::new(info));
            }

            for inst in instruments_info {
                nautilus_instruments.extend(parse_instrument_any(
                    &inst,
                    None,
                    None,
                    self.normalize_symbols,
                ));
            }
        }

        Ok((instrument_map, nautilus_instruments))
    }
}
