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

//! Instrument provider for the Derive adapter.

use std::{collections::HashMap, fmt::Debug};

use async_trait::async_trait;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};

use crate::{
    common::{
        consts::DERIVE_VENUE, enums::DeriveInstrumentType, parse::parse_derive_instrument_any,
    },
    http::{DeriveHttpClient, error::DeriveHttpError, models::DeriveInstrument},
};

const INSTRUMENT_NOT_FOUND_CODE: i64 = 12001;

/// Provides Derive instruments via the REST API.
///
/// The Derive `public/get_instruments` endpoint is scoped by underlying
/// currency, so callers configure the currency set up front or pass a
/// `currency`/`currencies` filter to `load_all()`.
pub struct DeriveInstrumentProvider {
    store: InstrumentStore,
    http_client: DeriveHttpClient,
    currencies: Vec<String>,
    include_expired: bool,
}

impl Debug for DeriveInstrumentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DeriveInstrumentProvider))
            .field("store", &self.store)
            .field("http_client", &self.http_client)
            .field("currencies", &self.currencies)
            .field("include_expired", &self.include_expired)
            .finish()
    }
}

impl DeriveInstrumentProvider {
    /// Creates a new provider with an empty store.
    #[must_use]
    pub fn new(http_client: DeriveHttpClient, currencies: Vec<String>) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            currencies,
            include_expired: false,
        }
    }

    /// Creates a new provider and controls whether expired instruments load.
    #[must_use]
    pub fn with_expired(
        http_client: DeriveHttpClient,
        currencies: Vec<String>,
        include_expired: bool,
    ) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            currencies,
            include_expired,
        }
    }

    /// Returns the configured currency filters.
    #[must_use]
    pub fn currencies(&self) -> &[String] {
        &self.currencies
    }

    /// Returns whether `load_all()` includes expired instruments by default.
    #[must_use]
    pub const fn include_expired(&self) -> bool {
        self.include_expired
    }

    /// Returns a reference to the underlying HTTP client.
    #[must_use]
    pub const fn http_client(&self) -> &DeriveHttpClient {
        &self.http_client
    }

    /// Adds instruments to the store.
    pub fn add_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        self.store.add_bulk(instruments);
    }

    async fn fetch_instruments(
        &self,
        currencies: &[String],
        expired: bool,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let mut instruments = Vec::new();

        for currency in currencies {
            let definitions =
                fetch_instrument_definitions(&self.http_client, currency, expired).await?;
            instruments.extend(parse_instrument_definitions(definitions)?);
        }

        Ok(instruments)
    }
}

#[async_trait(?Send)]
impl InstrumentProvider for DeriveInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(&mut self, filters: Option<&HashMap<String, String>>) -> anyhow::Result<()> {
        let (currencies, expired) =
            resolve_load_filters(&self.currencies, self.include_expired, filters)?;
        let instruments = self.fetch_instruments(&currencies, expired).await?;

        self.store.clear();
        self.add_instruments(instruments);
        self.store.set_initialized();

        Ok(())
    }

    async fn load_ids(
        &mut self,
        instrument_ids: &[InstrumentId],
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let missing: Vec<_> = instrument_ids
            .iter()
            .filter(|id| !self.store.contains(id))
            .collect();

        if missing.is_empty() {
            return Ok(());
        }

        let expired = resolve_expired_filter(self.include_expired, filters)?;
        let mut currencies: Vec<String> = missing
            .iter()
            .filter_map(|id| currency_from_instrument_id(id).map(ToOwned::to_owned))
            .collect();
        currencies.sort();
        currencies.dedup();

        if !currencies.is_empty() {
            let instruments = self.fetch_instruments(&currencies, expired).await?;
            self.add_instruments(instruments);
        }

        if missing.iter().all(|id| self.store.contains(id)) {
            return Ok(());
        }

        if !self.store.is_initialized() {
            let existing = self.store.get_all().values().cloned().collect::<Vec<_>>();
            self.load_all(filters).await?;

            for instrument in existing {
                if !self.store.contains(&instrument.id()) {
                    self.store.add(instrument);
                }
            }
        }

        let missing_ids: Vec<_> = instrument_ids
            .iter()
            .filter(|id| !self.store.contains(id))
            .collect();

        if missing_ids.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Derive instruments not found: {missing_ids:?}")
        }
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        if self.store.contains(instrument_id) {
            return Ok(());
        }

        self.load_ids(&[*instrument_id], filters).await
    }
}

pub(crate) fn parse_instrument_definitions(
    definitions: Vec<DeriveInstrument>,
) -> anyhow::Result<Vec<InstrumentAny>> {
    let ts_init = get_atomic_clock_realtime().get_time_ns();
    let mut instruments = Vec::with_capacity(definitions.len());

    for definition in definitions {
        if let Some(instrument) = parse_derive_instrument_any(&definition, ts_init)? {
            instruments.push(instrument);
        }
    }

    Ok(instruments)
}

pub(crate) async fn fetch_instrument_definitions(
    http_client: &DeriveHttpClient,
    currency: &str,
    expired: bool,
) -> anyhow::Result<Vec<DeriveInstrument>> {
    let (mut definitions, options, erc20s) = tokio::try_join!(
        http_client.get_instruments(currency, DeriveInstrumentType::Perp, expired),
        http_client.get_instruments(currency, DeriveInstrumentType::Option, expired),
        fetch_erc20_instruments(http_client, currency, expired),
    )?;
    definitions.extend(options);
    definitions.extend(erc20s);

    Ok(definitions)
}

// Venue returns JSON-RPC error 12001 (`Instrument not found`) when querying
// `erc20` for a currency with no spot listing (e.g. BTC has perp+option but
// no spot). Treat as empty so a missing spot listing does not fail the
// perp/option fetch.
async fn fetch_erc20_instruments(
    http_client: &DeriveHttpClient,
    currency: &str,
    expired: bool,
) -> Result<Vec<DeriveInstrument>, DeriveHttpError> {
    match http_client
        .get_instruments(currency, DeriveInstrumentType::Erc20, expired)
        .await
    {
        Ok(rows) => Ok(rows),
        Err(DeriveHttpError::JsonRpc { code, .. }) if code == INSTRUMENT_NOT_FOUND_CODE => {
            Ok(Vec::new())
        }
        Err(e) => Err(e),
    }
}

fn resolve_load_filters(
    default_currencies: &[String],
    default_expired: bool,
    filters: Option<&HashMap<String, String>>,
) -> anyhow::Result<(Vec<String>, bool)> {
    let currencies = filters
        .and_then(|map| {
            map.get("currency")
                .map(|currency| vec![currency.trim().to_string()])
                .or_else(|| map.get("currencies").map(|value| split_currencies(value)))
        })
        .unwrap_or_else(|| default_currencies.to_vec());

    let currencies = normalize_currencies(currencies);

    anyhow::ensure!(
        !currencies.is_empty(),
        "DeriveInstrumentProvider requires at least one currency",
    );

    let expired = resolve_expired_filter(default_expired, filters)?;

    Ok((currencies, expired))
}

fn resolve_expired_filter(
    default_expired: bool,
    filters: Option<&HashMap<String, String>>,
) -> anyhow::Result<bool> {
    filters
        .and_then(|map| map.get("expired"))
        .map(|value| value.parse::<bool>())
        .transpose()
        .map_err(|e| anyhow::anyhow!("invalid Derive `expired` filter: {e}"))
        .map(|value| value.unwrap_or(default_expired))
}

fn split_currencies(value: &str) -> Vec<String> {
    normalize_currencies(value.split(',').map(ToOwned::to_owned).collect())
}

fn normalize_currencies(currencies: Vec<String>) -> Vec<String> {
    let mut currencies: Vec<_> = currencies
        .into_iter()
        .map(|currency| currency.trim().to_string())
        .filter(|currency| !currency.is_empty())
        .collect();
    currencies.sort();
    currencies.dedup();
    currencies
}

fn currency_from_instrument_id(instrument_id: &InstrumentId) -> Option<&str> {
    if instrument_id.venue != *DERIVE_VENUE {
        return None;
    }

    instrument_id
        .symbol
        .as_str()
        .split_once('-')
        .and_then(|(currency, _)| (!currency.is_empty()).then_some(currency))
}
