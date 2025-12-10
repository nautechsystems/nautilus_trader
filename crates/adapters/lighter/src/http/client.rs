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

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::Context;
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::identifiers::InstrumentId;
use nautilus_model::instruments::InstrumentAny;
use reqwest::{Client, Proxy};
use tracing::{debug, trace, warn};

use crate::common::LighterNetwork;
use crate::data::models::LighterOrderBookDepth;
use crate::urls::get_http_base_url;

use super::models::{LighterOrderBook, OrderBookSnapshotResponse, OrderBooksResponse};
use super::parse::{LighterInstrumentDef, ParseReport};
use super::parse::{instruments_from_defs, parse_instrument_defs};

/// Cached metadata for instruments to support downstream components (WS subscriptions, etc.).
#[derive(Debug, Clone)]
pub struct LighterInstrumentMeta {
    /// Market index as used by Lighter WS/REST.
    pub market_index: u32,
    /// Venue symbol used in upstream payloads.
    pub venue_symbol: String,
}

/// HTTP client for Lighter public endpoints.
#[derive(Debug, Clone)]
pub struct LighterHttpClient {
    http: Client,
    base_url: String,
    instrument_meta: Arc<RwLock<HashMap<InstrumentId, LighterInstrumentMeta>>>,
}

impl LighterHttpClient {
    /// Create a new client configured for the given network.
    ///
    /// # Errors
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(
        network: LighterNetwork,
        base_url_override: Option<&str>,
        timeout_secs: Option<u64>,
        proxy_url: Option<&str>,
    ) -> anyhow::Result<Self> {
        let mut builder = Client::builder().user_agent("nautilus-lighter");

        if let Some(timeout) = timeout_secs {
            builder = builder.timeout(Duration::from_secs(timeout));
        }

        if let Some(proxy) = proxy_url {
            builder = builder.proxy(Proxy::all(proxy).context("invalid proxy configuration")?);
        }

        let http = builder.build().context("failed to build HTTP client")?;
        let base_url = get_http_base_url(network, base_url_override);

        Ok(Self {
            http,
            base_url,
            instrument_meta: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Fetch order book metadata from the public endpoint.
    ///
    /// # Errors
    /// Returns an error on request failure or invalid JSON.
    pub async fn get_order_books(&self) -> anyhow::Result<Vec<LighterOrderBook>> {
        let url = format!("{}/orderBooks", self.base_url);
        trace!(%url, "Requesting Lighter orderBooks");

        let response = self
            .http
            .get(url)
            .send()
            .await
            .context("failed to send orderBooks request")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read orderBooks response body")?;

        if !status.is_success() {
            anyhow::bail!("orderBooks request failed ({status}): {body}");
        }

        let parsed: OrderBooksResponse =
            serde_json::from_str(&body).context("failed to deserialize orderBooks response")?;

        Ok(parsed.into_books())
    }

    /// Fetch a full depth snapshot for the given market index.
    ///
    /// # Errors
    /// Returns an error on request failure or invalid JSON.
    pub async fn get_order_book_snapshot(
        &self,
        market_index: u32,
    ) -> anyhow::Result<LighterOrderBookDepth> {
        self.get_order_book_snapshot_with_limit(market_index, 100)
            .await
    }

    /// Fetch a depth snapshot with a custom limit for the given market index.
    ///
    /// # Errors
    /// Returns an error on request failure or invalid JSON.
    pub async fn get_order_book_snapshot_with_limit(
        &self,
        market_index: u32,
        limit: u32,
    ) -> anyhow::Result<LighterOrderBookDepth> {
        let url = format!(
            "{}/orderBookOrders?market_id={market_index}&limit={limit}",
            self.base_url
        );
        trace!(%url, "Requesting Lighter orderBookOrders");

        let response = self
            .http
            .get(url)
            .send()
            .await
            .context("failed to send orderBookOrders request")?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read orderBookOrders response body")?;

        if !status.is_success() {
            anyhow::bail!("orderBookOrders request failed ({status}): {body}");
        }

        let parsed: OrderBookSnapshotResponse = serde_json::from_str(&body)
            .context("failed to deserialize orderBookOrders response")?;

        Ok(parsed.into_depth())
    }

    /// Load instrument definitions and convert them into Nautilus instrument types.
    ///
    /// # Errors
    /// Returns an error on request failure or parse failure.
    pub async fn load_instrument_definitions(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let books = self.get_order_books().await?;
        let (defs, report) = parse_instrument_defs(&books)?;
        log_parse_report(&report);

        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let instruments = instruments_from_defs(&defs, ts_init)?;

        self.cache_instrument_meta(&defs);

        Ok(instruments)
    }

    /// Return the cached market index for a given instrument ID.
    pub fn get_market_index(&self, instrument_id: &InstrumentId) -> Option<u32> {
        self.instrument_meta
            .read()
            .ok()
            .and_then(|map| map.get(instrument_id).map(|meta| meta.market_index))
    }

    fn cache_instrument_meta(&self, defs: &[LighterInstrumentDef]) {
        if let Ok(mut map) = self.instrument_meta.write() {
            map.clear();
            for def in defs.iter().filter(|d| d.active) {
                map.insert(
                    def.instrument_id,
                    LighterInstrumentMeta {
                        market_index: def.market_index,
                        venue_symbol: def.venue_symbol.to_string(),
                    },
                );
            }
        }
    }
}

fn log_parse_report(report: &ParseReport) {
    if report.skipped == 0 {
        debug!("Parsed Lighter instrument definitions");
        return;
    }

    warn!(
        skipped = report.skipped,
        errors = ?report.errors,
        "Some Lighter instrument definitions were skipped"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use nautilus_core::time::get_atomic_clock_realtime;

    #[test]
    fn parses_and_caches_market_indices() {
        let client = LighterHttpClient::new(
            LighterNetwork::Testnet,
            Some("http://localhost:12345/api/v1"),
            Some(5),
            None,
        )
        .expect("client");

        // Inject fixture without performing HTTP request.
        let data = std::fs::read_to_string(fixture_path()).unwrap();
        let resp: OrderBooksResponse = serde_json::from_str(&data).unwrap();
        let (defs, report) = parse_instrument_defs(&resp.into_books()).unwrap();
        log_parse_report(&report);

        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let instruments = instruments_from_defs(&defs, ts_init).unwrap();
        client.cache_instrument_meta(&defs);

        assert_eq!(instruments.len(), 1);
        let id = match &instruments[0] {
            nautilus_model::instruments::InstrumentAny::CryptoPerpetual(cp) => cp.id,
            _ => panic!("expected crypto perpetual"),
        };
        assert_eq!(client.get_market_index(&id), Some(1));

        // Verify inactive market is not cached.
        assert_eq!(client.instrument_meta.read().unwrap().len(), 1);
    }

    fn fixture_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/test_data/lighter/http/orderbooks.json")
    }
}
