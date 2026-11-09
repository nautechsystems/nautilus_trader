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

//! Standalone historical ingestion with reusable outcome definitions.

use std::{collections::HashMap, num::NonZeroUsize, sync::Arc};

use anyhow::Context;
use nautilus_core::{UnixNanos, string::secret::SecretString, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Bar, BarType, OrderBookDelta, QuoteTick, TradeTick},
    identifiers::InstrumentId,
    instruments::BinaryOption,
};
use parking_lot::{Mutex, RwLock};

use crate::{
    book,
    common::symbol::from_instrument_id,
    history,
    http::client::{FinFeedHttpClient, FinFeedHttpConfig},
    instruments::{
        FinFeedInstrumentSelection, check_duplicate_outcome, fetch_instrument, selected_ids,
    },
};

/// Fetches historical Nautilus data using retained outcome definitions.
///
/// Construction sends no requests and resolves credentials on the first network operation.
/// Load or register definitions before requesting market data. Requests share one HTTP client,
/// including its attempt allowance. Definition refreshes are explicit; each history request uses
/// a snapshot of its registered definition and returns no partial result on failure.
#[derive(Debug)]
pub struct FinFeedDataLoader {
    api_key: Option<SecretString>,
    http_config: FinFeedHttpConfig,
    http: Mutex<Option<Arc<FinFeedHttpClient>>>,
    instruments: RwLock<HashMap<InstrumentId, BinaryOption>>,
}

impl FinFeedDataLoader {
    /// Creates a loader with an empty instrument registry and lazy HTTP transport.
    #[must_use]
    pub fn new(api_key: Option<SecretString>) -> Self {
        Self::with_http_config(api_key, FinFeedHttpConfig::default())
    }

    /// Creates a loader with explicit HTTP controls, without resolving credentials or sending requests.
    #[must_use]
    pub fn with_http_config(api_key: Option<SecretString>, http_config: FinFeedHttpConfig) -> Self {
        Self {
            api_key,
            http_config,
            http: Mutex::new(None),
            instruments: RwLock::new(HashMap::new()),
        }
    }

    /// Registers an outcome definition, replacing the same ID after validation.
    ///
    /// # Errors
    ///
    /// Returns an error if the ID is not a FinFeed outcome or an outcome token has another owner.
    pub fn add_instrument(&self, instrument: BinaryOption) -> anyhow::Result<()> {
        from_instrument_id(instrument.id)?;
        let mut instruments = self.instruments.write();
        check_duplicate_outcome(&instruments, &instrument)?;
        instruments.insert(instrument.id, instrument);
        Ok(())
    }

    /// Returns a retained definition.
    ///
    /// # Errors
    ///
    /// Returns an error if the definition has not been loaded or registered.
    pub fn instrument(&self, instrument_id: InstrumentId) -> anyhow::Result<BinaryOption> {
        self.instruments
            .read()
            .get(&instrument_id)
            .cloned()
            .with_context(|| format!("No loaded instrument {instrument_id}"))
    }

    /// Fetches and retains a fresh outcome definition.
    ///
    /// # Errors
    ///
    /// Returns an error if retrieval, identity checks, or definition validation fails.
    pub async fn request_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<BinaryOption> {
        let http = self.http()?;
        let now = get_atomic_clock_realtime().get_time_ns();
        let instrument = fetch_instrument(&http, instrument_id, now).await?;
        self.add_instrument(instrument.clone())?;
        Ok(instrument)
    }

    /// Fetches the supplied outcome selections and retains them atomically.
    ///
    /// # Errors
    ///
    /// Returns an error if selections, retrieval, or any definition fails validation.
    /// Failures preserve all previously registered definitions.
    pub async fn request_instruments(
        &self,
        instruments: &[FinFeedInstrumentSelection],
    ) -> anyhow::Result<Vec<BinaryOption>> {
        let ids = selected_ids(instruments)?;
        let http = self.http()?;
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut loaded = Vec::with_capacity(ids.len());
        for id in ids {
            loaded.push(fetch_instrument(&http, id, now).await?);
        }

        let mut instruments = self.instruments.write();
        let mut candidate = instruments.clone();
        for instrument in &loaded {
            check_duplicate_outcome(&candidate, instrument)?;
            candidate.insert(instrument.id, instrument.clone());
        }

        *instruments = candidate;
        Ok(loaded)
    }

    /// Retrieves historical book deltas using the retained definition.
    ///
    /// Bounds are inclusive on the adapter's ordering clock. The local limit preserves complete
    /// book batches. A missing snapshot seed or ambiguous continuation rejects book history.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is missing, bounds are invalid, or retrieval or conversion fails.
    pub async fn request_book_deltas(
        &self,
        instrument_id: InstrumentId,
        start: UnixNanos,
        end: UnixNanos,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        let instrument = self.instrument(instrument_id)?;
        let now = get_atomic_clock_realtime().get_time_ns();

        let window = history::Window::new(
            Some(start.to_datetime_utc()),
            Some(end.to_datetime_utc()),
            limit,
            now.to_datetime_utc(),
        )?;

        let http = self.http()?;
        let batches = history::books(&http, &instrument, &window).await?;
        book::deltas(batches, limit)
    }

    /// Retrieves historical quotes using the retained definition.
    ///
    /// Bounds are inclusive on the adapter's ordering clock.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is missing, bounds are invalid, or retrieval or conversion fails.
    pub async fn request_quotes(
        &self,
        instrument_id: InstrumentId,
        start: UnixNanos,
        end: UnixNanos,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        let instrument = self.instrument(instrument_id)?;
        let now = get_atomic_clock_realtime().get_time_ns();

        let window = history::Window::new(
            Some(start.to_datetime_utc()),
            Some(end.to_datetime_utc()),
            limit,
            now.to_datetime_utc(),
        )?;

        let http = self.http()?;
        history::quotes(&http, &instrument, &window).await
    }

    /// Retrieves historical trades using the retained definition.
    ///
    /// Bounds are inclusive on the adapter's ordering clock.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is missing, bounds are invalid, or retrieval or conversion fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: UnixNanos,
        end: UnixNanos,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let instrument = self.instrument(instrument_id)?;
        let now = get_atomic_clock_realtime().get_time_ns();

        let window = history::Window::new(
            Some(start.to_datetime_utc()),
            Some(end.to_datetime_utc()),
            limit,
            now.to_datetime_utc(),
        )?;

        let http = self.http()?;
        history::trades(&http, &instrument, &window).await
    }

    /// Retrieves historical bars using the retained definition.
    ///
    /// Bounds are inclusive on the adapter's ordering clock.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is missing, bounds are invalid, or retrieval or conversion fails.
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: UnixNanos,
        end: UnixNanos,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<Vec<Bar>> {
        let instrument_id = bar_type.instrument_id();
        let instrument = self.instrument(instrument_id)?;
        let now = get_atomic_clock_realtime().get_time_ns();

        let window = history::Window::new(
            Some(start.to_datetime_utc()),
            Some(end.to_datetime_utc()),
            limit,
            now.to_datetime_utc(),
        )?;

        let http = self.http()?;
        history::bars(
            &http,
            &instrument,
            bar_type,
            &window,
            now.to_datetime_utc(),
            history::period_length(bar_type)?,
        )
        .await
    }

    fn http(&self) -> anyhow::Result<Arc<FinFeedHttpClient>> {
        let mut http = self.http.lock();
        if let Some(client) = http.as_ref() {
            return Ok(Arc::clone(client));
        }

        let client = FinFeedHttpClient::new(self.api_key.clone(), self.http_config.clone())?;
        let client = Arc::new(client);
        *http = Some(Arc::clone(&client));
        Ok(client)
    }
}
