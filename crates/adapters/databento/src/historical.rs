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

//! Core Databento historical client for both Rust and Python usage.

use std::{
    fs,
    num::NonZeroU64,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, RwLock},
};

use ahash::AHashMap;
use databento::{
    dbn::{self, decode::DbnMetadata},
    historical::timeseries::GetRangeParams,
};
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, consts::NAUTILUS_USER_AGENT, time::AtomicTime};
use nautilus_model::{
    data::{Bar, Data, InstrumentStatus, OrderBookDepth10, QuoteTick, TradeTick},
    enums::BarAggregation,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::InstrumentAny,
    types::Currency,
};
use tokio::sync::Mutex;

use crate::{
    common::get_date_time_range,
    decode::{
        decode_imbalance_msg, decode_instrument_def_msg, decode_mbp10_msg, decode_record,
        decode_statistics_msg, decode_status_msg,
    },
    symbology::{
        MetadataCache, check_consistent_symbology, decode_nautilus_instrument_id,
        infer_symbology_type, instrument_id_to_symbol_string,
    },
    types::{DatabentoImbalance, DatabentoPublisher, DatabentoStatistics, PublisherId},
};

/// Core Databento historical client for fetching historical market data.
///
/// This client provides both synchronous and asynchronous interfaces for fetching
/// various types of historical market data from Databento.
#[derive(Debug, Clone)]
pub struct DatabentoHistoricalClient {
    pub key: String,
    clock: &'static AtomicTime,
    inner: Arc<Mutex<databento::HistoricalClient>>,
    publisher_venue_map: Arc<IndexMap<PublisherId, Venue>>,
    symbol_venue_map: Arc<RwLock<AHashMap<Symbol, Venue>>>,
    use_exchange_as_venue: bool,
}

/// Parameters for range queries to Databento historical API.
#[derive(Debug)]
pub struct RangeQueryParams {
    pub dataset: String,
    pub symbols: Vec<String>,
    pub start: UnixNanos,
    pub end: Option<UnixNanos>,
    pub limit: Option<u64>,
    pub price_precision: Option<u8>,
}

/// Result containing dataset date range information.
#[derive(Debug, Clone)]
pub struct DatasetRange {
    pub start: String,
    pub end: String,
}

impl DatabentoHistoricalClient {
    /// Creates a new [`DatabentoHistoricalClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if client creation or publisher loading fails.
    pub fn new(
        key: String,
        publishers_filepath: PathBuf,
        clock: &'static AtomicTime,
        use_exchange_as_venue: bool,
    ) -> anyhow::Result<Self> {
        let client = databento::HistoricalClient::builder()
            .user_agent_extension(NAUTILUS_USER_AGENT.into())
            .key(key.clone())
            .map_err(|e| anyhow::anyhow!("Failed to create client builder: {e}"))?
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build client: {e}"))?;

        let file_content = fs::read_to_string(publishers_filepath)?;
        let publishers_vec: Vec<DatabentoPublisher> = serde_json::from_str(&file_content)?;

        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        Ok(Self {
            clock,
            inner: Arc::new(Mutex::new(client)),
            publisher_venue_map: Arc::new(publisher_venue_map),
            symbol_venue_map: Arc::new(RwLock::new(AHashMap::new())),
            key,
            use_exchange_as_venue,
        })
    }

    /// Gets the date range for a specific dataset.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn get_dataset_range(&self, dataset: &str) -> anyhow::Result<DatasetRange> {
        let mut client = self.inner.lock().await;
        let response = client
            .metadata()
            .get_dataset_range(dataset)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get dataset range: {e}"))?;

        Ok(DatasetRange {
            start: response.start.to_string(),
            end: response.end.to_string(),
        })
    }

    /// Fetches instrument definitions for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_instruments(
        &self,
        params: RangeQueryParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Definition)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut instruments = Vec::new();

        while let Ok(Some(msg)) = decoder.decode_record::<dbn::InstrumentDefMsg>().await {
            let record = dbn::RecordRef::from(msg);
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let mut instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            if self.use_exchange_as_venue && instrument_id.venue == Venue::GLBX() {
                let exchange = msg
                    .exchange()
                    .map_err(|e| anyhow::anyhow!("Missing exchange in record: {e}"))?;
                let venue = Venue::from_code(exchange)
                    .map_err(|e| anyhow::anyhow!("Venue not found for exchange {exchange}: {e}"))?;
                instrument_id.venue = venue;
            }

            match decode_instrument_def_msg(msg, instrument_id, None) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => tracing::error!("Failed to decode instrument: {e:?}"),
            }
        }

        Ok(instruments)
    }

    /// Fetches quote ticks for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_quotes(
        &self,
        params: RangeQueryParams,
        schema: Option<String>,
    ) -> anyhow::Result<Vec<QuoteTick>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;
        let schema = schema.unwrap_or_else(|| "mbp-1".to_string());
        let dbn_schema = dbn::Schema::from_str(&schema)?;

        match dbn_schema {
            dbn::Schema::Mbp1 | dbn::Schema::Bbo1S | dbn::Schema::Bbo1M => (),
            _ => anyhow::bail!("Invalid schema. Must be one of: mbp-1, bbo-1s, bbo-1m"),
        }

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn_schema)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = params.price_precision.unwrap_or(Currency::USD().precision);

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut result: Vec<QuoteTick> = Vec::new();

        let mut process_record = |record: dbn::RecordRef| -> anyhow::Result<()> {
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            let (data, _) = decode_record(
                &record,
                instrument_id,
                price_precision,
                None,
                false, // Don't include trades
                true,
            )?;

            match data {
                Some(Data::Quote(quote)) => {
                    result.push(quote);
                    Ok(())
                }
                _ => anyhow::bail!("Invalid data element not `QuoteTick`, was {data:?}"),
            }
        };

        match dbn_schema {
            dbn::Schema::Mbp1 => {
                while let Ok(Some(msg)) = decoder.decode_record::<dbn::Mbp1Msg>().await {
                    process_record(dbn::RecordRef::from(msg))?;
                }
            }
            dbn::Schema::Bbo1M => {
                while let Ok(Some(msg)) = decoder.decode_record::<dbn::Bbo1MMsg>().await {
                    process_record(dbn::RecordRef::from(msg))?;
                }
            }
            dbn::Schema::Bbo1S => {
                while let Ok(Some(msg)) = decoder.decode_record::<dbn::Bbo1SMsg>().await {
                    process_record(dbn::RecordRef::from(msg))?;
                }
            }
            _ => anyhow::bail!("Invalid schema {dbn_schema}"),
        }

        Ok(result)
    }

    /// Fetches order book depth10 snapshots for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_order_book_depth10(
        &self,
        params: RangeQueryParams,
        depth: Option<usize>,
    ) -> anyhow::Result<Vec<OrderBookDepth10>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;

        // For now, only support MBP_10 schema for depth 10
        let _depth = depth.unwrap_or(10);
        if _depth != 10 {
            anyhow::bail!("Only depth=10 is currently supported for order book depths");
        }

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Mbp10)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = params.price_precision.unwrap_or(Currency::USD().precision);

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut result: Vec<OrderBookDepth10> = Vec::new();

        let mut process_record = |record: dbn::RecordRef| -> anyhow::Result<()> {
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            if let Some(msg) = record.get::<dbn::Mbp10Msg>() {
                let depth = decode_mbp10_msg(msg, instrument_id, price_precision, None)?;
                result.push(depth);
            }

            Ok(())
        };

        while let Ok(Some(msg)) = decoder.decode_record::<dbn::Mbp10Msg>().await {
            process_record(dbn::RecordRef::from(msg))?;
        }

        Ok(result)
    }

    /// Fetches trade ticks for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_trades(
        &self,
        params: RangeQueryParams,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Trades)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = params.price_precision.unwrap_or(Currency::USD().precision);

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut result: Vec<TradeTick> = Vec::new();

        while let Ok(Some(msg)) = decoder.decode_record::<dbn::TradeMsg>().await {
            let record = dbn::RecordRef::from(msg);
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            let (data, _) = decode_record(
                &record,
                instrument_id,
                price_precision,
                None,
                false, // Not applicable (trade will be decoded regardless)
                true,
            )?;

            match data {
                Some(Data::Trade(trade)) => {
                    result.push(trade);
                }
                _ => anyhow::bail!("Invalid data element not `TradeTick`, was {data:?}"),
            }
        }

        Ok(result)
    }

    /// Fetches bars for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_bars(
        &self,
        params: RangeQueryParams,
        aggregation: BarAggregation,
        timestamp_on_close: bool,
    ) -> anyhow::Result<Vec<Bar>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let schema = match aggregation {
            BarAggregation::Second => dbn::Schema::Ohlcv1S,
            BarAggregation::Minute => dbn::Schema::Ohlcv1M,
            BarAggregation::Hour => dbn::Schema::Ohlcv1H,
            BarAggregation::Day => dbn::Schema::Ohlcv1D,
            _ => anyhow::bail!("Invalid `BarAggregation` for request, was {aggregation}"),
        };

        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(schema)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = params.price_precision.unwrap_or(Currency::USD().precision);

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut result: Vec<Bar> = Vec::new();

        while let Ok(Some(msg)) = decoder.decode_record::<dbn::OhlcvMsg>().await {
            let record = dbn::RecordRef::from(msg);
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            let (data, _) = decode_record(
                &record,
                instrument_id,
                price_precision,
                None,
                false, // Not applicable
                timestamp_on_close,
            )?;

            match data {
                Some(Data::Bar(bar)) => {
                    result.push(bar);
                }
                _ => anyhow::bail!("Invalid data element not `Bar`, was {data:?}"),
            }
        }

        Ok(result)
    }

    /// Fetches imbalance data for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_imbalance(
        &self,
        params: RangeQueryParams,
    ) -> anyhow::Result<Vec<DatabentoImbalance>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Imbalance)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = params.price_precision.unwrap_or(Currency::USD().precision);

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut result: Vec<DatabentoImbalance> = Vec::new();

        while let Ok(Some(msg)) = decoder.decode_record::<dbn::ImbalanceMsg>().await {
            let record = dbn::RecordRef::from(msg);
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            let imbalance = decode_imbalance_msg(msg, instrument_id, price_precision, None)?;
            result.push(imbalance);
        }

        Ok(result)
    }

    /// Fetches statistics data for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_statistics(
        &self,
        params: RangeQueryParams,
    ) -> anyhow::Result<Vec<DatabentoStatistics>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Statistics)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let price_precision = params.price_precision.unwrap_or(Currency::USD().precision);

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut result: Vec<DatabentoStatistics> = Vec::new();

        while let Ok(Some(msg)) = decoder.decode_record::<dbn::StatMsg>().await {
            let record = dbn::RecordRef::from(msg);
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            let statistics = decode_statistics_msg(msg, instrument_id, price_precision, None)?;
            result.push(statistics);
        }

        Ok(result)
    }

    /// Fetches status data for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request or data processing fails.
    pub async fn get_range_status(
        &self,
        params: RangeQueryParams,
    ) -> anyhow::Result<Vec<InstrumentStatus>> {
        let symbols: Vec<&str> = params.symbols.iter().map(String::as_str).collect();
        check_consistent_symbology(&symbols)?;

        let first_symbol = params
            .symbols
            .first()
            .ok_or_else(|| anyhow::anyhow!("No symbols provided"))?;
        let stype_in = infer_symbology_type(first_symbol);
        let end = params.end.unwrap_or_else(|| self.clock.get_time_ns());
        let time_range = get_date_time_range(params.start, end)?;

        let range_params = GetRangeParams::builder()
            .dataset(params.dataset)
            .date_time_range(time_range)
            .symbols(symbols)
            .stype_in(stype_in)
            .schema(dbn::Schema::Status)
            .limit(params.limit.and_then(NonZeroU64::new))
            .build();

        let mut client = self.inner.lock().await;
        let mut decoder = client
            .timeseries()
            .get_range(&range_params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get range: {e}"))?;

        let metadata = decoder.metadata().clone();
        let mut metadata_cache = MetadataCache::new(metadata);
        let mut result: Vec<InstrumentStatus> = Vec::new();

        while let Ok(Some(msg)) = decoder.decode_record::<dbn::StatusMsg>().await {
            let record = dbn::RecordRef::from(msg);
            let sym_map = self
                .symbol_venue_map
                .read()
                .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;
            let instrument_id = decode_nautilus_instrument_id(
                &record,
                &mut metadata_cache,
                &self.publisher_venue_map,
                &sym_map,
            )?;

            let status = decode_status_msg(msg, instrument_id, None)?;
            result.push(status);
        }

        Ok(result)
    }

    /// Helper method to prepare symbols from instrument IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if the symbol venue map lock is poisoned.
    pub fn prepare_symbols_from_instrument_ids(
        &self,
        instrument_ids: &[InstrumentId],
    ) -> anyhow::Result<Vec<String>> {
        let mut symbol_venue_map = self
            .symbol_venue_map
            .write()
            .map_err(|e| anyhow::anyhow!("symbol_venue_map lock poisoned: {e}"))?;

        let symbols: Vec<String> = instrument_ids
            .iter()
            .map(|instrument_id| {
                instrument_id_to_symbol_string(*instrument_id, &mut symbol_venue_map)
            })
            .collect();

        Ok(symbols)
    }
}
