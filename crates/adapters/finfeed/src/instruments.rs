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

//! Explicit instrument selection and fresh definitions from one FinFeed source.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use nautilus_core::UnixNanos;
use nautilus_model::{identifiers::InstrumentId, instruments::BinaryOption};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    common::symbol::{from_instrument_id, to_instrument_id},
    http::{
        client::FinFeedHttpClient,
        error::Error,
        parse::{TOKEN_ID_KEY, parse_instrument},
    },
};

/// Exact outcome IDs explicitly selected from one FinFeed exchange.
///
/// IDs come from discovery or are supplied directly. Loading does not implicitly include
/// other outcomes of the same market or event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.finfeed", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")
)]
pub struct FinFeedInstrumentSelection {
    /// Provider exchange identifier, such as `POLYMARKET`.
    pub exchange_id: Ustr,
    /// Exact outcome IDs, including `_YES` or `_NO`; related outcomes are not loaded implicitly.
    pub market_ids: Vec<Ustr>,
}

/// Loads and retains provider-owned definitions for caller publication.
///
/// Startup loads replace the catalog only after every selection succeeds. Individual requests
/// always fetch full metadata and replace only their requested definition after validation.
/// Errors preserve the prior catalog. Returned definitions are the publication boundary;
/// this provider does not own a message bus or a native adapter's instrument definitions.
#[derive(Debug, Clone)]
pub struct FinFeedInstrumentProvider {
    http: Arc<FinFeedHttpClient>,
    instruments: HashMap<InstrumentId, BinaryOption>,
}

impl FinFeedInstrumentProvider {
    /// Creates an empty provider sharing the caller's REST client and request budget.
    #[must_use]
    pub fn new(http: Arc<FinFeedHttpClient>) -> Self {
        Self {
            http,
            instruments: HashMap::new(),
        }
    }

    /// Loads exactly the selected outcomes, in selection order, from fresh full metadata.
    ///
    /// Callers can build selections using the REST client's bounded listings or active IDs.
    /// A successful load replaces the retained catalog, including removing prior selections.
    /// Failures retain the prior catalog and return no definitions.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A selection is empty, duplicate, unknown, or unavailable.
    /// - Metadata is incomplete, ambiguous, or inconsistent with the requested identity.
    /// - Two provider IDs map to the same Polymarket outcome token.
    /// - A REST request fails.
    pub async fn load(
        &mut self,
        selections: &[FinFeedInstrumentSelection],
        ts_init: UnixNanos,
    ) -> Result<Vec<BinaryOption>, Error> {
        let ids = selected_ids(selections)?;
        let exchanges = self.http.get_exchanges().await?;
        let available: HashSet<_> = exchanges
            .iter()
            .filter_map(|exchange| exchange.exchange_id.as_deref())
            .collect();

        for selection in selections {
            if !available.contains(selection.exchange_id.as_str()) {
                return Err(Error::Validation(format!(
                    "selected exchange {} is unavailable",
                    selection.exchange_id
                )));
            }
        }

        let mut instruments = HashMap::new();
        let mut loaded = Vec::with_capacity(ids.len());
        for instrument_id in ids {
            let instrument = self.fetch(instrument_id, ts_init).await?;
            check_duplicate_outcome(&instruments, &instrument)?;
            instruments.insert(instrument.id, instrument.clone());
            loaded.push(instrument);
        }

        self.instruments = instruments;
        Ok(loaded)
    }

    /// Returns a newly fetched definition, updating only that instrument on success.
    ///
    /// A valid FinFeed ID can be requested even when it was not part of startup selection.
    /// Failures retain the previous cache.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The ID, metadata, or mapping is invalid, unsupported, or unavailable.
    /// - Another cached ID owns the same Polymarket outcome token.
    /// - A REST request fails.
    pub async fn request_instrument(
        &mut self,
        instrument_id: InstrumentId,
        ts_init: UnixNanos,
    ) -> Result<BinaryOption, Error> {
        let instrument = self.fetch(instrument_id, ts_init).await?;
        check_duplicate_outcome(&self.instruments, &instrument)?;
        self.instruments.insert(instrument.id, instrument.clone());
        Ok(instrument)
    }

    /// Returns the current definition last accepted by a load or fresh request.
    #[must_use]
    pub fn get(&self, instrument_id: InstrumentId) -> Option<&BinaryOption> {
        self.instruments.get(&instrument_id)
    }

    async fn fetch(
        &self,
        instrument_id: InstrumentId,
        ts_init: UnixNanos,
    ) -> Result<BinaryOption, Error> {
        let (exchange_id, market_id) = from_instrument_id(instrument_id)?;
        let market = match self.http.get_market(&exchange_id, &market_id).await {
            Err(Error::Http { status: 404, .. }) => return Err(Error::Unavailable(instrument_id)),
            result => result?,
        };

        if market.exchange_id.as_deref() != Some(exchange_id.as_str())
            || market.market_id.as_deref() != Some(market_id.as_str())
        {
            return Err(Error::Definition {
                exchange_id,
                market_id,
                field: "identity",
                reason: "does not match the requested outcome",
            });
        }
        parse_instrument(&market, ts_init)
    }
}

pub(crate) fn selected_ids(
    selections: &[FinFeedInstrumentSelection],
) -> Result<Vec<InstrumentId>, Error> {
    if selections.is_empty() {
        return Err(Error::Validation("instrument selection is empty".into()));
    }
    let mut seen = HashSet::new();
    let mut ids = Vec::new();

    for selection in selections {
        if selection.market_ids.is_empty() {
            return Err(Error::Validation("selected market IDs are empty".into()));
        }

        for market_id in &selection.market_ids {
            let id = to_instrument_id(&selection.exchange_id, market_id)?;
            if !seen.insert(id) {
                return Err(Error::Validation(format!("duplicate selection {id}")));
            }
            ids.push(id);
        }
    }
    Ok(ids)
}

fn check_duplicate_outcome(
    instruments: &HashMap<InstrumentId, BinaryOption>,
    instrument: &BinaryOption,
) -> Result<(), Error> {
    let token = instrument
        .info
        .as_ref()
        .and_then(|info| info.get(TOKEN_ID_KEY));
    if instruments.values().any(|existing| {
        existing.id != instrument.id
            && token.is_some()
            && existing
                .info
                .as_ref()
                .and_then(|info| info.get(TOKEN_ID_KEY))
                == token
    }) {
        return Err(Error::Validation(format!(
            "duplicate outcome mapping for {}",
            instrument.id
        )));
    }
    Ok(())
}
