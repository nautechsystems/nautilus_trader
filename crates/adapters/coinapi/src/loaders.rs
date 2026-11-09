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

//! Standalone ingestion with reusable instrument definitions and provider mappings.

use std::{collections::HashMap, num::NonZeroUsize, path::Path, sync::Arc};

use anyhow::Context;
use jiff::civil::Date;
use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Bar, BarType, OrderBookDelta, QuoteTick, TradeTick},
    enums::AggregationSource,
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use parking_lot::{Mutex, RwLock};
use ustr::Ustr;

use crate::{
    common::credential::resolve_api_key,
    config::{CoinApiDataClientConfig, validate_price_precisions},
    csv,
    http::{
        client::{CoinApiHttpClient, HistoryPages},
        parse::{
            parse_bar, parse_instruments_with_precisions, parse_order_book, parse_quote_tick,
            parse_trade_tick,
        },
        query::{BookQuery, HistoryRange},
    },
    websocket::parse::bar_spec_to_period_id,
};

/// Loads Nautilus data using retained instruments and CoinAPI symbol mappings.
///
/// Local file operations require no credentials. The first network request resolves credentials
/// and creates a shared HTTP client with the configured credit allowance. Register definitions
/// for file loading, or request the configured selections before REST market-data requests.
/// Tick and book values must fit the registered instrument precision without rounding.
/// Consolidated bar volumes retain observed precision after bounded export normalization. REST history
/// rejects truncated responses and invalid records rather than returning a partial result.
#[derive(Debug)]
pub struct CoinApiDataLoader {
    config: CoinApiDataClientConfig,
    http: Mutex<Option<Arc<CoinApiHttpClient>>>,
    instruments: RwLock<HashMap<InstrumentId, LoadedInstrument>>,
}

impl CoinApiDataLoader {
    /// Creates an empty loader without resolving credentials or sending requests.
    #[must_use]
    pub fn new(config: CoinApiDataClientConfig) -> Self {
        Self {
            config,
            http: Mutex::new(None),
            instruments: RwLock::new(HashMap::new()),
        }
    }

    /// Registers a definition and optional REST and consolidated-file symbol IDs.
    ///
    /// Updating an existing instrument replaces its provider mappings as well as its definition.
    ///
    /// # Errors
    ///
    /// Returns an error if a symbol ID is empty or belongs to another registered instrument.
    pub fn add_instrument(
        &self,
        instrument: InstrumentAny,
        symbol_id: Option<Ustr>,
        symbol_id_int: Option<u64>,
    ) -> anyhow::Result<()> {
        let mut instruments = self.instruments.write();
        let id = instrument.id();

        let loaded = LoadedInstrument {
            instrument,
            symbol_id,
            symbol_id_int,
        };

        insert_instrument(&mut instruments, id, loaded)
    }

    /// Returns a retained definition.
    ///
    /// # Errors
    ///
    /// Returns an error if the definition has not been loaded or registered.
    pub fn instrument(&self, instrument_id: InstrumentId) -> anyhow::Result<InstrumentAny> {
        Ok(self.loaded(instrument_id)?.instrument)
    }

    /// Fetches the configured selections and retains their metadata atomically.
    ///
    /// # Errors
    ///
    /// Returns an error if selections are invalid, a selected definition is unavailable, or
    /// retrieval or mapping validation fails. Failures preserve the existing registry.
    pub async fn request_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        self.config.validate()?;
        validate_price_precisions(&self.config.instruments, &self.config.price_precisions)?;
        anyhow::ensure!(
            !self.config.instruments.is_empty(),
            "CoinAPI loader requires instrument selections"
        );
        let http = self.http()?;
        let now = get_atomic_clock_realtime().get_time_ns();
        let mut loaded = Vec::new();

        for selection in &self.config.instruments {
            let symbols = http
                .get_symbols(&selection.exchange_id, &selection.query)
                .await?;
            let parsed =
                parse_instruments_with_precisions(&symbols, &self.config.price_precisions, now);

            for id in &selection.instrument_ids {
                let item = parsed
                    .iter()
                    .find(|item| item.instrument.id() == *id)
                    .with_context(|| format!("CoinAPI returned no instrument {id}"))?;
                let symbol_id_int = symbols
                    .iter()
                    .find(|symbol| symbol.symbol_id == item.symbol_id)
                    .and_then(|symbol| symbol.symbol_id_int);
                loaded.push(LoadedInstrument {
                    instrument: item.instrument.clone(),
                    symbol_id: Some(item.symbol_id),
                    symbol_id_int,
                });
            }
        }

        let mut instruments = self.instruments.write();
        let mut candidate = instruments.clone();
        for item in &loaded {
            insert_instrument(&mut candidate, item.instrument.id(), item.clone())?;
        }

        *instruments = candidate;
        Ok(loaded.into_iter().map(|item| item.instrument).collect())
    }

    /// Loads deltas from a local file using the retained instrument precision.
    ///
    /// Supply `date` or use a path with a `D-` partition.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata or the date is missing, the file cannot be read, or a value
    /// cannot be converted exactly.
    pub fn load_book_deltas(
        &self,
        path: impl AsRef<Path>,
        instrument_id: InstrumentId,
        date: Option<Date>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<OrderBookDelta>> {
        let instrument = self.instrument(instrument_id)?;
        csv::load_deltas(
            path,
            instrument_id,
            date,
            Some(instrument.price_precision()),
            Some(instrument.size_precision()),
            limit,
        )
    }

    /// Streams deltas from a local file using the retained instrument precision.
    ///
    /// Supply `date` or use a path with a `D-` partition. Chunks end at event boundaries and can
    /// exceed `chunk_size`, which must be in `[1, 1_000_000]`.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata, date, file access, or chunk bounds are invalid. Iteration
    /// fails if any file value cannot be converted exactly. Stream state belongs to the iterator.
    pub fn stream_book_deltas<P: AsRef<Path>>(
        &self,
        path: P,
        instrument_id: InstrumentId,
        date: Option<Date>,
        chunk_size: usize,
        limit: Option<usize>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<OrderBookDelta>>> + use<P>> {
        let instrument = self.instrument(instrument_id)?;
        csv::stream_deltas(
            path,
            instrument_id,
            date,
            chunk_size,
            Some(instrument.price_precision()),
            Some(instrument.size_precision()),
            limit,
        )
    }

    /// Loads quotes from a local file using the retained instrument precision.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is missing or any file value cannot be converted exactly.
    pub fn load_quotes(
        &self,
        path: impl AsRef<Path>,
        instrument_id: InstrumentId,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        let instrument = self.instrument(instrument_id)?;
        csv::load_quotes(
            path,
            instrument_id,
            Some(instrument.price_precision()),
            Some(instrument.size_precision()),
            limit,
        )
    }

    /// Streams quotes from a local file using the retained instrument precision.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata, file access, or chunk bounds are invalid. Iteration
    /// fails if any file value cannot be converted exactly. Stream state belongs to the iterator.
    pub fn stream_quotes<P: AsRef<Path>>(
        &self,
        path: P,
        instrument_id: InstrumentId,
        chunk_size: usize,
        limit: Option<usize>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<QuoteTick>>> + use<P>> {
        let instrument = self.instrument(instrument_id)?;
        csv::stream_quotes(
            path,
            instrument_id,
            chunk_size,
            Some(instrument.price_precision()),
            Some(instrument.size_precision()),
            limit,
        )
    }

    /// Loads trades from a local file using the retained instrument precision.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is missing or any file value cannot be converted exactly.
    pub fn load_trades(
        &self,
        path: impl AsRef<Path>,
        instrument_id: InstrumentId,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let instrument = self.instrument(instrument_id)?;
        csv::load_trades(
            path,
            instrument_id,
            Some(instrument.price_precision()),
            Some(instrument.size_precision()),
            limit,
        )
    }

    /// Streams trades from a local file using the retained instrument precision.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata, file access, or chunk bounds are invalid. Iteration
    /// fails if any file value cannot be converted exactly. Stream state belongs to the iterator.
    pub fn stream_trades<P: AsRef<Path>>(
        &self,
        path: P,
        instrument_id: InstrumentId,
        chunk_size: usize,
        limit: Option<usize>,
    ) -> anyhow::Result<impl Iterator<Item = anyhow::Result<Vec<TradeTick>>> + use<P>> {
        let instrument = self.instrument(instrument_id)?;
        csv::stream_trades(
            path,
            instrument_id,
            chunk_size,
            Some(instrument.price_precision()),
            Some(instrument.size_precision()),
            limit,
        )
    }

    /// Loads consolidated OHLCV bars for registered numeric symbol IDs.
    ///
    /// Reads the period from `period_id` or the `TP-` file name. Rows for unmapped symbols are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if no numeric mappings exist, the file is invalid, or values exceed
    /// their registered price precision. Volumes may be finer than the trading lot increment.
    pub fn load_bars(
        &self,
        path: impl AsRef<Path>,
        period_id: Option<&str>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<Bar>> {
        let instruments = self.instruments.read();

        let ids = instruments
            .values()
            .filter_map(|item| {
                Some((
                    item.symbol_id_int?,
                    (item.instrument.id(), item.instrument.price_precision()),
                ))
            })
            .collect::<HashMap<_, _>>();

        anyhow::ensure!(
            !ids.is_empty(),
            "CoinAPI bars require numeric symbol mappings"
        );
        drop(instruments);
        csv::load_bars(path, &ids, period_id, limit)
    }

    /// Retrieves and converts a current L2 book for a registered REST symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata is missing or retrieval or conversion fails.
    pub async fn request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> anyhow::Result<OrderBook> {
        let item = self.loaded(instrument_id)?;
        let symbol_id = item
            .symbol_id
            .context("CoinAPI instrument has no REST symbol mapping")?;

        let query = BookQuery {
            limit_levels: depth,
        };

        let http = self.http()?;
        let source = http.get_book(symbol_id.as_str(), &query).await?;
        parse_order_book(&source, &item.instrument)
    }

    /// Retrieves historical quotes using retained metadata, rejecting partial results.
    ///
    /// The requested window is `[start, end)`.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata or bounds are invalid, credits truncate retrieval,
    /// or any request or record conversion fails.
    pub async fn request_quotes(
        &self,
        instrument_id: InstrumentId,
        start: UnixNanos,
        end: UnixNanos,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        let item = self.loaded(instrument_id)?;
        let symbol_id = item
            .symbol_id
            .context("CoinAPI instrument has no REST symbol mapping")?;

        let range = HistoryRange {
            time_start: start.to_datetime_utc(),
            time_end: end.to_datetime_utc(),
            limit,
        };

        let http = self.http()?;
        let pages = http.get_quotes_range(symbol_id.as_str(), &range).await?;
        check_complete(&pages, http.credits_remaining())?;
        let records = pages.records.iter();
        records
            .map(|row| parse_quote_tick(row, &item.instrument))
            .collect()
    }

    /// Retrieves historical trades using retained metadata, rejecting partial results.
    ///
    /// The requested window is `[start, end)`.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata or bounds are invalid, credits truncate retrieval,
    /// or any request or record conversion fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: UnixNanos,
        end: UnixNanos,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let item = self.loaded(instrument_id)?;
        let symbol_id = item
            .symbol_id
            .context("CoinAPI instrument has no REST symbol mapping")?;

        let range = HistoryRange {
            time_start: start.to_datetime_utc(),
            time_end: end.to_datetime_utc(),
            limit,
        };

        let http = self.http()?;
        let pages = http.get_trades_range(symbol_id.as_str(), &range).await?;
        check_complete(&pages, http.credits_remaining())?;
        let records = pages.records.iter();
        records
            .map(|row| parse_trade_tick(row, &item.instrument))
            .collect()
    }

    /// Retrieves historical bars using retained metadata, rejecting partial results.
    ///
    /// The requested window is `[start, end)`. Bars must close within the window and by now.
    ///
    /// # Errors
    ///
    /// Returns an error if metadata or bounds are invalid, credits truncate retrieval,
    /// or any request or record conversion fails.
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: UnixNanos,
        end: UnixNanos,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<Vec<Bar>> {
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "CoinAPI provides only external bars"
        );
        let period = bar_spec_to_period_id(bar_type.spec())?;
        let instrument_id = bar_type.instrument_id();
        let item = self.loaded(instrument_id)?;
        let symbol_id = item
            .symbol_id
            .context("CoinAPI instrument has no REST symbol mapping")?;

        let range = HistoryRange {
            time_start: start.to_datetime_utc(),
            time_end: end.to_datetime_utc(),
            limit,
        };

        let http = self.http()?;
        let pages = http
            .get_ohlcv_range(symbol_id.as_str(), period, &range)
            .await?;
        check_complete(&pages, http.credits_remaining())?;
        let closed_by = end
            .to_datetime_utc()
            .min(get_atomic_clock_realtime().get_time_ns().to_datetime_utc());
        let records = pages
            .records
            .iter()
            .filter(|row| row.time_period_end <= closed_by);
        records
            .map(|row| parse_bar(row, bar_type, &item.instrument))
            .collect()
    }

    fn loaded(&self, instrument_id: InstrumentId) -> anyhow::Result<LoadedInstrument> {
        self.instruments
            .read()
            .get(&instrument_id)
            .cloned()
            .with_context(|| format!("No loaded instrument {instrument_id}"))
    }

    fn http(&self) -> anyhow::Result<Arc<CoinApiHttpClient>> {
        let mut http = self.http.lock();
        if let Some(client) = http.as_ref() {
            return Ok(Arc::clone(client));
        }

        let key = resolve_api_key(self.config.api_key.clone())?;
        let client = CoinApiHttpClient::new(key, self.config.http_config())?;
        let client = Arc::new(client);
        *http = Some(Arc::clone(&client));
        Ok(client)
    }
}

#[derive(Debug, Clone)]
struct LoadedInstrument {
    instrument: InstrumentAny,
    symbol_id: Option<Ustr>,
    symbol_id_int: Option<u64>,
}

fn insert_instrument(
    instruments: &mut HashMap<InstrumentId, LoadedInstrument>,
    id: InstrumentId,
    loaded: LoadedInstrument,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        loaded.symbol_id.is_none_or(|symbol| !symbol.is_empty()),
        "CoinAPI symbol ID must not be empty"
    );

    for (other_id, other) in instruments.iter() {
        if *other_id == id {
            continue;
        }

        anyhow::ensure!(
            loaded.symbol_id.is_none() || loaded.symbol_id != other.symbol_id,
            "CoinAPI REST symbol already belongs to {other_id}"
        );
        anyhow::ensure!(
            loaded.symbol_id_int.is_none() || loaded.symbol_id_int != other.symbol_id_int,
            "CoinAPI numeric symbol already belongs to {other_id}"
        );
    }

    instruments.insert(id, loaded);
    Ok(())
}

fn check_complete<T>(pages: &HistoryPages<T>, credits_remaining: u64) -> anyhow::Result<()> {
    anyhow::ensure!(
        !pages.is_truncated,
        "CoinAPI history truncated after {} records with {credits_remaining} credits remaining; no partial result returned; use a shorter window or a new loader with a larger credit_budget",
        pages.records.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        data::OrderBookDelta,
        enums::{BookAction, RecordFlag},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        config::CoinApiInstrumentSelection,
        http::{models::Symbol, parse::parse_instruments, query::SymbolsQuery},
    };

    fn instrument() -> InstrumentAny {
        let symbols: Vec<Symbol> =
            serde_json::from_str(include_str!("../test_data/symbols_spot.json")).unwrap();
        let (mut instruments, _) = parse_instruments(&symbols, UnixNanos::default());
        instruments.remove(0).instrument
    }

    #[tokio::test]
    async fn metadata_rejects_unselected_price_override_before_credentials() {
        let selected = InstrumentId::from("BTCUSDT.BINANCE");
        let overridden = InstrumentId::from("BTC-USD.COINBASE");

        let loader = CoinApiDataLoader::new(CoinApiDataClientConfig {
            base_url_http: Some("invalid-url".to_owned()),
            instruments: vec![CoinApiInstrumentSelection {
                exchange_id: "BINANCE".into(),
                query: SymbolsQuery::default(),
                instrument_ids: vec![selected],
            }],
            price_precisions: HashMap::from([(overridden, 3)]),
            ..Default::default()
        });

        let error = loader.request_instruments().await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "price precision for BTC-USD.COINBASE, which is not a selected instrument"
        );
    }

    #[rstest]
    fn local_loads_use_registered_precision_without_credentials() {
        let loader = CoinApiDataLoader::new(CoinApiDataClientConfig::default());
        let instrument = instrument();
        let id = instrument.id();
        loader
            .add_instrument(instrument.clone(), None, None)
            .unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_data/csv/trades_binance_spot_btc_usdt_20250901.csv");
        let trades = loader.load_trades(&path, id, Some(2)).unwrap();
        let streamed: Vec<_> = loader
            .stream_trades(&path, id, 1, Some(2))
            .unwrap()
            .collect::<anyhow::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .flatten()
            .collect();
        assert_eq!(loader.instrument(id).unwrap(), instrument);
        assert_eq!(trades, streamed);
        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].price.to_string(), "108246.36");
        assert_eq!(trades[0].size.as_decimal().to_string(), "0.00023");
        assert_eq!(trades[0].size.precision, instrument.size_precision());
    }

    #[rstest]
    fn missing_metadata_and_duplicate_mappings_preserve_registry() {
        let loader = CoinApiDataLoader::new(CoinApiDataClientConfig::default());
        let instrument = instrument();
        let id = instrument.id();
        let missing = loader.load_trades("absent.csv", id, None).unwrap_err();
        assert_eq!(missing.to_string(), format!("No loaded instrument {id}"));
        loader
            .add_instrument(
                instrument.clone(),
                Some("BINANCE_SPOT_BTC_USDT".into()),
                Some(42),
            )
            .unwrap();
        let mut other = instrument.clone();
        if let InstrumentAny::CurrencyPair(pair) = &mut other {
            pair.id = InstrumentId::from("OTHER.BINANCE");
        }

        let error = loader
            .add_instrument(other.clone(), None, Some(42))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("CoinAPI numeric symbol already belongs to {id}")
        );
        assert_eq!(loader.instrument(id).unwrap(), instrument);
        assert_eq!(
            loader.instrument(other.id()).unwrap_err().to_string(),
            format!("No loaded instrument {}", other.id())
        );
    }

    #[rstest]
    fn book_stream_preserves_complete_batches_and_definition_snapshot() {
        let loader = CoinApiDataLoader::new(CoinApiDataClientConfig::default());
        let instrument = instrument();
        let id = instrument.id();
        loader.add_instrument(instrument, None, None).unwrap();
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test_data/csv/limitbook_full_binance_spot_btc_usdt_20250901.csv");
        let stream = loader
            .stream_book_deltas(&path, id, Some(Date::new(2025, 9, 1).unwrap()), 5, None)
            .unwrap();
        drop(loader);
        let chunks = stream.collect::<anyhow::Result<Vec<_>>>().unwrap();
        let first: &OrderBookDelta = &chunks[0][0];
        assert_eq!(first.action, BookAction::Clear);
        assert_eq!(first.instrument_id, id);
        assert!(
            chunks
                .iter()
                .all(|chunk| { RecordFlag::F_LAST.matches(chunk.last().unwrap().flags) })
        );
    }
}
