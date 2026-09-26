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

use std::{borrow::Cow, collections::HashMap, fmt::Debug, io::Read, sync::Arc};

use ahash::{AHashMap, AHashSet};
use flate2::read::GzDecoder;
use nautilus_core::{
    DurationNanos, UnixNanos,
    string::{parsing::precision_from_str, secret::REDACTED, urlencoding},
};
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::http::{
    HttpClient, HttpRedirectPolicy, HttpResponse, create_standard_nautilus_headers,
};

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

const CONTENT_ENCODING: &str = "content-encoding";
const GZIP: &str = "gzip";

// Deribit's all-symbol instrument list decompresses to about 284 MB
const MAX_DECOMPRESSED_BYTES: u64 = 1024 * 1024 * 1024;

/// A Tardis HTTP API client.
/// See <https://docs.tardis.dev/api/http>.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.tardis", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.tardis")
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

        let mut headers: HashMap<String, String> =
            create_standard_nautilus_headers().into_iter().collect();

        if let Some(ref cred) = credential {
            headers.insert(
                "Authorization".to_string(),
                format!("Bearer {}", cred.api_key()),
            );
        }

        // Uncompressed all-symbol instrument lists exceed the default HTTP response limit
        headers.insert("Accept-Encoding".to_string(), GZIP.to_string());

        let keyed_quotas = vec![(TARDIS_REST_RATE_KEY.to_string(), *TARDIS_REST_QUOTA)];
        let client = HttpClient::builder()
            .redirect_policy(HttpRedirectPolicy::Reject)
            .headers(headers)
            .keyed_quotas(keyed_quotas)
            .default_quota(*TARDIS_REST_QUOTA)
            .maybe_timeout_secs(timeout_secs.or(Some(60)))
            .maybe_proxy_url(proxy_url)
            .header_keys(vec![CONTENT_ENCODING.to_string()])
            .build()?;

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
        let mut url = format!("{}/instruments/{exchange}", self.base_url);

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

        let body = decode_body(&response)?;

        if !response.status.is_success() {
            let body = String::from_utf8_lossy(&body).to_string();
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

        let body = String::from_utf8_lossy(&body);
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
        available_offset: Option<DurationNanos>,
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
            log::debug!("Fetching instruments for {exchange}");

            let instruments_info = match self.instruments_info(*exchange, None, None).await {
                Ok(info) => info,
                Err(e) => {
                    log::error!("Failed to fetch instruments for {exchange}: {e}");
                    continue;
                }
            };

            log::debug!(
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
                    Some(inst.id),
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

fn decode_body(response: &HttpResponse) -> Result<Cow<'_, [u8]>> {
    match response.headers.get(CONTENT_ENCODING) {
        None => Ok(Cow::Borrowed(response.body.as_ref())),
        Some(encoding) if encoding.eq_ignore_ascii_case(GZIP) => {
            decompress_gzip(&response.body, MAX_DECOMPRESSED_BYTES).map(Cow::Owned)
        }
        Some(encoding) => Err(Error::Request(format!(
            "unsupported response content encoding '{encoding}'"
        ))),
    }
}

fn decompress_gzip(body: &[u8], max_bytes: u64) -> Result<Vec<u8>> {
    let mut reader = GzDecoder::new(body).take(max_bytes + 1);
    let mut decompressed = Vec::new();
    reader
        .read_to_end(&mut decompressed)
        .map_err(|e| Error::Request(format!("failed to decompress response body: {e}")))?;

    if reader.limit() == 0 {
        return Err(Error::Request(format!(
            "decompressed response body exceeds maximum of {max_bytes} bytes"
        )));
    }

    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use nautilus_network::http::{HttpStatus, StatusCode};
    use nautilus_testkit::http::assert_http_redirect_rejected;
    use rstest::rstest;

    use super::*;

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn http_response(encoding: Option<&str>, body: Vec<u8>) -> HttpResponse {
        HttpResponse {
            status: HttpStatus::new(StatusCode::OK),
            headers: encoding
                .map(|encoding| (CONTENT_ENCODING.to_string(), encoding.to_string()))
                .into_iter()
                .collect(),
            body: body.into(),
        }
    }

    #[tokio::test]
    async fn test_authenticated_client_rejects_redirects() {
        let client = TardisHttpClient::new(Some("test-key"), None, Some(3), false, None)
            .unwrap()
            .client;
        assert_http_redirect_rejected(|url| async move {
            client
                .get(url, None, None, Some(3), None)
                .await
                .unwrap()
                .status
                .as_u16()
        })
        .await;
    }

    #[rstest]
    #[case::identity(None, b"[7]".to_vec())]
    #[case::gzip(Some("gzip"), gzip(b"[7]"))]
    #[case::gzip_uppercase(Some("GZIP"), gzip(b"[7]"))]
    fn test_decode_body(#[case] encoding: Option<&str>, #[case] body: Vec<u8>) {
        let response = http_response(encoding, body);

        let decoded = decode_body(&response).unwrap();

        assert_eq!(decoded.as_ref(), b"[7]");
    }

    #[rstest]
    #[case::unsupported(Some("br"), b"[7]".to_vec(), "unsupported response content encoding 'br'")]
    #[case::corrupt(
        Some("gzip"),
        b"[7]".to_vec(),
        "failed to decompress response body: unexpected end of file"
    )]
    fn test_decode_body_rejects(
        #[case] encoding: Option<&str>,
        #[case] body: Vec<u8>,
        #[case] expected: &str,
    ) {
        let response = http_response(encoding, body);

        let error = decode_body(&response).unwrap_err();

        let Error::Request(message) = error else {
            panic!("expected request error, was {error:?}");
        };

        assert_eq!(message, expected);
    }

    #[rstest]
    #[case::at_max(10)]
    #[case::below_max(11)]
    fn test_decompress_gzip_at_or_below_max_bytes(#[case] max_bytes: u64) {
        let decompressed = decompress_gzip(&gzip(b"0123456789"), max_bytes).unwrap();

        assert_eq!(decompressed, b"0123456789");
    }

    #[rstest]
    fn test_decompress_gzip_above_max_bytes() {
        let error = decompress_gzip(&gzip(b"0123456789"), 9).unwrap_err();

        let Error::Request(message) = error else {
            panic!("expected request error, was {error:?}");
        };

        assert_eq!(
            message,
            "decompressed response body exceeds maximum of 9 bytes"
        );
    }
}
