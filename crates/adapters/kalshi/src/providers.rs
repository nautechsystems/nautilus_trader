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

//! Instrument provider for the Kalshi exchange.
//!
//! The provider loads the markets of the configured events and series, and can refresh them so a
//! long-running node picks up newly listed markets.

use std::collections::HashMap;

use async_trait::async_trait;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_model::identifiers::InstrumentId;

use crate::{
    common::{consts::KALSHI_VENUE, enums::KalshiMarketStatus},
    http::{client::KalshiHttpClient, parse::create_instrument_from_market},
};

/// The filter key naming an event ticker.
pub const FILTER_EVENT_TICKER: &str = "event_ticker";

/// The filter key naming a series ticker.
pub const FILTER_SERIES_TICKER: &str = "series_ticker";

/// Provides Kalshi instruments to the data and execution engines.
#[derive(Debug)]
pub struct KalshiInstrumentProvider {
    client: KalshiHttpClient,
    store: InstrumentStore,
    event_tickers: Vec<String>,
    series_ticker: Option<String>,
}

impl KalshiInstrumentProvider {
    /// Creates a new [`KalshiInstrumentProvider`].
    #[must_use]
    pub fn new(
        client: KalshiHttpClient,
        event_tickers: Vec<String>,
        series_ticker: Option<String>,
    ) -> Self {
        Self {
            client,
            store: InstrumentStore::new(),
            event_tickers,
            series_ticker,
        }
    }

    /// Returns the ticker a Kalshi instrument identifier refers to.
    ///
    /// Returns `None` for an identifier this provider does not serve.
    #[must_use]
    pub fn ticker_for(instrument_id: &InstrumentId) -> Option<&str> {
        let symbol = instrument_id.symbol.as_str();

        if instrument_id.venue.as_str() != KALSHI_VENUE {
            return None;
        }

        Some(symbol)
    }

    /// Loads the markets of the given event ticker, replacing any previously stored copy.
    ///
    /// # Errors
    ///
    /// Returns an error if the markets cannot be fetched or any market cannot be converted.
    pub async fn load_event(&mut self, event_ticker: &str) -> anyhow::Result<usize> {
        let markets = self
            .client
            .get_all_markets(None, Some(event_ticker), None)
            .await?;

        Ok(self.store_markets(markets))
    }

    /// Loads every open market, or the markets of the configured events and series when set.
    ///
    /// # Errors
    ///
    /// Returns an error if the markets cannot be fetched or any market cannot be converted.
    pub async fn load_markets(&mut self) -> anyhow::Result<usize> {
        let mut count = 0;

        if self.event_tickers.is_empty() {
            let markets = self
                .client
                .get_all_markets(
                    Some(KalshiMarketStatus::Active),
                    None,
                    self.series_ticker.as_deref(),
                )
                .await?;

            count += self.store_markets(markets);
        } else {
            for event_ticker in self.event_tickers.clone() {
                count += self.load_event(&event_ticker).await?;
            }
        }

        Ok(count)
    }

    /// Stores the given markets as instruments, returning how many were converted.
    fn store_markets(&mut self, markets: Vec<crate::http::models::KalshiMarket>) -> usize {
        let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
        let mut count = 0;

        for market in markets {
            match create_instrument_from_market(&market, ts_init) {
                Ok(instrument) => {
                    self.store.add(instrument);
                    count += 1;
                }
                Err(e) => {
                    log::warn!("Skipping Kalshi market {}: {e}", market.ticker);
                }
            }
        }

        count
    }
}

#[async_trait(?Send)]
impl InstrumentProvider for KalshiInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(&mut self, filters: Option<&HashMap<String, String>>) -> anyhow::Result<()> {
        if let Some(filters) = filters {
            if let Some(event_ticker) = filters.get(FILTER_EVENT_TICKER) {
                self.load_event(event_ticker).await?;

                return Ok(());
            }

            if let Some(series_ticker) = filters.get(FILTER_SERIES_TICKER) {
                let markets = self
                    .client
                    .get_all_markets(None, None, Some(series_ticker))
                    .await?;

                self.store_markets(markets);

                return Ok(());
            }
        }

        self.load_markets().await?;

        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        _filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let ticker = Self::ticker_for(instrument_id).ok_or_else(|| {
            anyhow::anyhow!(
                "Kalshi provider does not serve instrument {instrument_id}; expected a {KALSHI_VENUE} market ticker"
            )
        })?;
        let market = self.client.get_market(ticker).await?;
        let ts_init = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
        let instrument = create_instrument_from_market(&market, ts_init)?;

        self.store.add(instrument);

        Ok(())
    }
}
