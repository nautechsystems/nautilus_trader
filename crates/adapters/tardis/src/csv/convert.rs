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

use std::{path::PathBuf, time::Duration};

use ahash::AHashMap;
use anyhow::Context;
use csv::StringRecord;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Data, HasTsInit},
    enums::OptionKind,
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoOption, InstrumentAny},
    types::{Currency, Price, Quantity, fixed::FIXED_PRECISION},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use rust_decimal::Decimal;

use crate::{
    common::{
        enums::{TardisExchange, TardisOptionType},
        parse::{parse_instrument_id, parse_option_kind, parse_timestamp},
    },
    csv::{
        create_csv_reader, infer_precision, load::OptionsChainPrecision, matches_underlying_filter,
        normalize_underlying_filters, parse_options_chain_record,
        parse_options_chain_record_as_quote, record::TardisOptionsChainRecord,
    },
};

const DATA_FLUSH_ROWS: usize = 100_000;

/// Configuration for converting Tardis `options_chain` CSV files into a Nautilus catalog.
#[derive(Debug, Clone, bon::Builder)]
pub struct TardisOptionsChainCSVConverterConfig {
    /// Tardis daily `options_chain` CSV file paths, processed in order.
    pub filepaths: Vec<PathBuf>,
    /// Nautilus catalog path.
    pub catalog_path: PathBuf,
    /// Optional underlying prefixes, such as `BTC-`.
    pub underlyings: Option<Vec<String>>,
    /// Optional thinning interval. Keeps the last row per instrument per bucket.
    pub snapshot_interval: Option<Duration>,
    /// Whether to emit quotes from best bid/offer fields.
    #[builder(default = true)]
    pub extract_bbo_as_quotes: bool,
    /// Whether to derive and write instrument definitions from the CSV rows.
    #[builder(default = true)]
    pub write_instruments: bool,
    /// Optional explicit price precision.
    pub price_precision: Option<u8>,
    /// Optional explicit size precision.
    pub size_precision: Option<u8>,
}

/// Converts Tardis `options_chain` CSV files into `QuoteTick`, `OptionGreeks`, and instruments.
///
/// # Errors
///
/// Returns an error if a CSV file cannot be read, a row cannot be parsed, a complete best
/// bid/offer row contains invalid values, instrument derivation fails, or catalog writes fail.
pub fn convert_options_chain_csv(
    config: &TardisOptionsChainCSVConverterConfig,
) -> anyhow::Result<()> {
    let underlyings = normalize_underlying_filters(config.underlyings.clone());
    let catalog = ParquetDataCatalog::new(&config.catalog_path, None, None, None, None);
    let mut precision_by_instrument: AHashMap<InstrumentId, OptionsChainPrecision> =
        AHashMap::new();
    let mut instrument_states: AHashMap<InstrumentId, InstrumentBuildState> = AHashMap::new();
    let mut data_buffers: AHashMap<InstrumentId, DataBuffer> = AHashMap::new();
    let mut pending_records: AHashMap<(InstrumentId, u64), TardisOptionsChainRecord> =
        AHashMap::new();
    let mut current_bucket = None;

    for filepath in &config.filepaths {
        let mut reader = create_csv_reader(filepath)
            .with_context(|| format!("failed to open CSV file {}", filepath.display()))?;
        let mut csv_record = StringRecord::new();

        while reader
            .read_record(&mut csv_record)
            .with_context(|| format!("failed to read CSV file {}", filepath.display()))?
        {
            if let Some(underlyings) = underlyings.as_deref() {
                let Some(symbol) = csv_record.get(1) else {
                    continue;
                };
                let symbol = symbol.to_uppercase();
                if !matches_underlying_filter(&symbol, Some(underlyings)) {
                    continue;
                }
            }

            let record: TardisOptionsChainRecord = csv_record
                .deserialize(None)
                .with_context(|| format!("failed to parse CSV file {}", filepath.display()))?;
            let instrument_id = parse_instrument_id(&record.exchange, record.symbol);
            precision_by_instrument
                .entry(instrument_id)
                .or_insert_with(|| {
                    OptionsChainPrecision::new(config.price_precision, config.size_precision)
                })
                .update(&record, config.price_precision, config.size_precision);
            instrument_states
                .entry(instrument_id)
                .and_modify(|state| state.update_activation(record.local_timestamp))
                .or_insert_with(|| InstrumentBuildState::new(record.clone()));

            if let Some(interval) = config.snapshot_interval {
                let interval_us = u64::try_from(interval.as_micros())
                    .context("snapshot interval exceeds u64 microseconds")?;
                anyhow::ensure!(interval_us > 0, "snapshot interval must be positive");
                let bucket = record.local_timestamp / interval_us;

                if let Some(current_bucket) = current_bucket {
                    anyhow::ensure!(
                        bucket >= current_bucket,
                        "options_chain CSV rows must be ordered by local_timestamp when thinning"
                    );
                }

                if current_bucket.is_none_or(|current| bucket > current) {
                    flush_pending_records_before(
                        &catalog,
                        &mut pending_records,
                        &mut data_buffers,
                        &precision_by_instrument,
                        bucket,
                        config.extract_bbo_as_quotes,
                    )?;
                    current_bucket = Some(bucket);
                }
                pending_records
                    .entry((instrument_id, bucket))
                    .and_modify(|pending| {
                        if record.local_timestamp >= pending.local_timestamp {
                            *pending = record.clone();
                        }
                    })
                    .or_insert(record);
            } else {
                flush_data_buffer_if_ready(
                    &catalog,
                    &mut data_buffers,
                    instrument_id,
                    parse_timestamp(record.local_timestamp),
                )?;
                let data = options_chain_record_to_data(
                    &record,
                    &precision_by_instrument,
                    config.extract_bbo_as_quotes,
                )?;
                data_buffers
                    .entry(instrument_id)
                    .or_default()
                    .extend(data, parse_timestamp(record.local_timestamp));
            }
        }

        if config.snapshot_interval.is_some() {
            flush_pending_records_before(
                &catalog,
                &mut pending_records,
                &mut data_buffers,
                &precision_by_instrument,
                u64::MAX,
                config.extract_bbo_as_quotes,
            )?;
            flush_data_buffers(&catalog, &mut data_buffers)?;
            current_bucket = None;
        }
    }

    flush_data_buffers(&catalog, &mut data_buffers)?;

    if config.write_instruments {
        let instruments = build_instruments(instrument_states, &precision_by_instrument)?;
        catalog.write_instruments(instruments)?;
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct InstrumentBuildState {
    record: TardisOptionsChainRecord,
    activation: UnixNanos,
}

impl InstrumentBuildState {
    fn new(record: TardisOptionsChainRecord) -> Self {
        Self {
            activation: parse_timestamp(record.local_timestamp),
            record,
        }
    }

    fn update_activation(&mut self, local_timestamp: u64) {
        self.activation = self.activation.min(parse_timestamp(local_timestamp));
    }
}

#[derive(Debug, Default)]
struct DataBuffer {
    data: Vec<Data>,
    last_ts_init: Option<UnixNanos>,
}

impl DataBuffer {
    fn extend(&mut self, mut data: Vec<Data>, ts_init: UnixNanos) {
        self.data.append(&mut data);
        self.last_ts_init = Some(ts_init);
    }
}

fn flush_data_buffer_if_ready(
    catalog: &ParquetDataCatalog,
    data_buffers: &mut AHashMap<InstrumentId, DataBuffer>,
    instrument_id: InstrumentId,
    ts_init: UnixNanos,
) -> anyhow::Result<()> {
    let Some(buffer) = data_buffers.get_mut(&instrument_id) else {
        return Ok(());
    };

    if buffer.data.len() >= DATA_FLUSH_ROWS
        && buffer
            .last_ts_init
            .is_some_and(|last_ts_init| last_ts_init < ts_init)
    {
        write_data_buffer(catalog, &mut buffer.data)?;
    }

    Ok(())
}

fn flush_data_buffers(
    catalog: &ParquetDataCatalog,
    data_buffers: &mut AHashMap<InstrumentId, DataBuffer>,
) -> anyhow::Result<()> {
    for buffer in data_buffers.values_mut() {
        write_data_buffer(catalog, &mut buffer.data)?;
    }
    data_buffers.clear();
    Ok(())
}

fn flush_pending_records_before(
    catalog: &ParquetDataCatalog,
    pending_records: &mut AHashMap<(InstrumentId, u64), TardisOptionsChainRecord>,
    data_buffers: &mut AHashMap<InstrumentId, DataBuffer>,
    precision_by_instrument: &AHashMap<InstrumentId, OptionsChainPrecision>,
    next_bucket: u64,
    extract_bbo_as_quotes: bool,
) -> anyhow::Result<()> {
    let mut ready = Vec::new();
    pending_records.retain(|(_, bucket), record| {
        if *bucket < next_bucket {
            ready.push(record.clone());
            false
        } else {
            true
        }
    });
    ready.sort_by_key(|record| (record.local_timestamp, record.symbol));

    for record in ready {
        let instrument_id = parse_instrument_id(&record.exchange, record.symbol);
        let ts_init = parse_timestamp(record.local_timestamp);
        flush_data_buffer_if_ready(catalog, data_buffers, instrument_id, ts_init)?;
        let data =
            options_chain_record_to_data(&record, precision_by_instrument, extract_bbo_as_quotes)?;
        data_buffers
            .entry(instrument_id)
            .or_default()
            .extend(data, ts_init);
    }
    Ok(())
}

fn options_chain_record_to_data(
    record: &TardisOptionsChainRecord,
    precision_by_instrument: &AHashMap<InstrumentId, OptionsChainPrecision>,
    extract_bbo_as_quotes: bool,
) -> anyhow::Result<Vec<Data>> {
    let instrument_id = parse_instrument_id(&record.exchange, record.symbol);
    let precision = precision_by_instrument
        .get(&instrument_id)
        .copied()
        .unwrap_or_else(|| OptionsChainPrecision::new(None, None));
    let mut data = Vec::with_capacity(2);

    if extract_bbo_as_quotes
        && let Some(quote) = parse_options_chain_record_as_quote(
            record,
            precision.price,
            precision.size,
            instrument_id,
        )?
    {
        data.push(Data::Quote(quote));
    }

    data.push(Data::OptionGreeks(parse_options_chain_record(
        record,
        instrument_id,
    )));
    Ok(data)
}

fn write_data_buffer(catalog: &ParquetDataCatalog, data: &mut Vec<Data>) -> anyhow::Result<()> {
    if data.is_empty() {
        return Ok(());
    }

    let mut data_by_instrument: AHashMap<InstrumentId, Vec<Data>> = AHashMap::new();
    for item in data.drain(..) {
        data_by_instrument
            .entry(item.instrument_id())
            .or_default()
            .push(item);
    }

    for instrument_data in data_by_instrument.values_mut() {
        instrument_data.sort_by_key(HasTsInit::ts_init);
        catalog.write_data_enum(instrument_data, None, None, None)?;
    }

    data.clear();
    Ok(())
}

fn build_instruments(
    instrument_states: AHashMap<InstrumentId, InstrumentBuildState>,
    precision_by_instrument: &AHashMap<InstrumentId, OptionsChainPrecision>,
) -> anyhow::Result<Vec<InstrumentAny>> {
    let mut states = instrument_states.into_iter().collect::<Vec<_>>();
    states.sort_by_key(|(instrument_id, _)| instrument_id.to_string());

    states
        .into_iter()
        .map(|(instrument_id, state)| {
            let precision = precision_by_instrument
                .get(&instrument_id)
                .copied()
                .unwrap_or_else(|| OptionsChainPrecision::new(None, None));
            create_crypto_option_from_options_chain_record(
                &state.record,
                instrument_id,
                state.activation,
                precision,
            )
        })
        .collect()
}

fn create_crypto_option_from_options_chain_record(
    record: &TardisOptionsChainRecord,
    instrument_id: InstrumentId,
    activation: UnixNanos,
    precision: OptionsChainPrecision,
) -> anyhow::Result<InstrumentAny> {
    let underlying = record
        .symbol
        .as_str()
        .split_once('-')
        .map(|(underlying, _)| underlying)
        .context("options_chain symbol missing underlying prefix")?;
    let underlying_currency = Currency::get_or_create_crypto(underlying);
    let (quote_currency, settlement_currency, is_inverse) =
        option_currency_mapping(record.exchange, underlying_currency)?;
    let instrument_price_precision = precision
        .price
        .max(infer_precision(record.strike_price).min(FIXED_PRECISION));
    let price_increment = decimal_increment_price(instrument_price_precision)?;
    let size_increment = decimal_increment_quantity(precision.size)?;
    let strike_price = Price::from_decimal_dp(
        Decimal::try_from(record.strike_price)?,
        instrument_price_precision,
    )?;
    let expiration = parse_timestamp(record.expiration);
    let option_kind = option_kind(record.option_type);

    Ok(InstrumentAny::CryptoOption(CryptoOption::new_checked(
        instrument_id,
        Symbol::from_ustr_unchecked(record.symbol),
        underlying_currency,
        quote_currency,
        settlement_currency,
        is_inverse,
        option_kind,
        strike_price,
        activation,
        expiration,
        instrument_price_precision,
        precision.size,
        price_increment,
        size_increment,
        None,
        Some(size_increment),
        None,
        Some(size_increment),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        parse_timestamp(record.timestamp),
        parse_timestamp(record.local_timestamp),
    )?))
}

fn option_currency_mapping(
    exchange: TardisExchange,
    underlying_currency: Currency,
) -> anyhow::Result<(Currency, Currency, bool)> {
    match exchange {
        TardisExchange::Deribit => Ok((underlying_currency, underlying_currency, true)),
        exchange => anyhow::bail!(
            "options_chain instrument derivation supports Deribit only, received {exchange}"
        ),
    }
}

fn decimal_increment_price(precision: u8) -> anyhow::Result<Price> {
    Ok(Price::from_decimal_dp(
        Decimal::new(1, u32::from(precision)),
        precision,
    )?)
}

fn decimal_increment_quantity(precision: u8) -> anyhow::Result<Quantity> {
    Ok(Quantity::from_decimal_dp(
        Decimal::new(1, u32::from(precision)),
        precision,
    )?)
}

const fn option_kind(value: TardisOptionType) -> OptionKind {
    parse_option_kind(value)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::Duration,
    };

    use nautilus_model::{
        data::{OptionGreeks, QuoteTick},
        enums::OptionKind,
        instruments::Instrument,
    };
    use nautilus_persistence::backend::catalog::ParquetDataCatalog;
    use rstest::rstest;
    use tempfile::TempDir;
    use ustr::Ustr;

    use super::*;
    use crate::common::testing::get_test_data_path;

    #[rstest]
    fn test_options_chain_converter_config_defaults_extract_bbo_as_quotes() {
        let config = TardisOptionsChainCSVConverterConfig::builder()
            .filepaths(Vec::<PathBuf>::new())
            .catalog_path(PathBuf::from("/tmp/options-chain-catalog"))
            .build();

        assert!(config.extract_bbo_as_quotes);
        assert!(config.write_instruments);
    }

    #[rstest]
    fn test_convert_options_chain_csv_thins_and_round_trips_catalog() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = get_test_data_path("options_chain.csv");
        let config = TardisOptionsChainCSVConverterConfig {
            filepaths: vec![filepath],
            catalog_path: temp_dir.path().to_path_buf(),
            underlyings: Some(vec!["BTC-9JUN20-9875".to_string()]),
            snapshot_interval: Some(Duration::from_secs(60)),
            extract_bbo_as_quotes: true,
            write_instruments: true,
            price_precision: None,
            size_precision: None,
        };

        convert_options_chain_csv(&config).unwrap();

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let instrument_id = "BTC-9JUN20-9875-P.DERIBIT".to_string();
        let quotes = catalog
            .query_typed_data::<QuoteTick>(
                Some(vec![instrument_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        let greeks = catalog
            .query_typed_data::<OptionGreeks>(
                Some(vec![instrument_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        let instruments = catalog
            .query_instruments(Some(std::slice::from_ref(&instrument_id)))
            .unwrap();

        assert_eq!(quotes.len(), 2);
        assert_eq!(greeks.len(), 2);
        assert_eq!(instruments.len(), 1);

        assert_eq!(quotes[0].bid_price, Price::from("0.0206"));
        assert_eq!(quotes[0].ask_price, Price::from("0.0236"));
        assert_eq!(quotes[0].ts_init.as_u64(), 1_591_574_400_473_112_000);
        assert_eq!(quotes[1].bid_price, Price::from("0.0207"));
        assert_eq!(quotes[1].ts_init.as_u64(), 1_591_574_460_473_112_000);

        let InstrumentAny::CryptoOption(option) = &instruments[0] else {
            panic!("Expected CryptoOption");
        };
        assert_eq!(option.id().to_string(), instrument_id);
        assert_eq!(option.raw_symbol(), Symbol::from("BTC-9JUN20-9875-P"));
        assert_eq!(option.option_kind, OptionKind::Put);
        assert_eq!(option.strike_price, Price::from("9875.0000"));
        assert_eq!(
            option.expiration_ns,
            UnixNanos::from(1_591_689_600_000_000_000)
        );
        assert_eq!(
            option.activation_ns,
            UnixNanos::from(1_591_574_400_196_008_000)
        );
        assert_eq!(option.price_increment, Price::from("0.0001"));
        assert_eq!(option.size_increment, Quantity::from("0.1"));
        assert!(option.is_inverse);
        assert_eq!(option.quote_currency, Currency::from("BTC"));
        assert_eq!(option.settlement_currency, Currency::from("BTC"));
    }

    #[rstest]
    fn test_convert_options_chain_csv_thinned_multi_instrument_writes_separate_catalogs() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = get_test_data_path("options_chain.csv");
        let config = TardisOptionsChainCSVConverterConfig {
            filepaths: vec![filepath],
            catalog_path: temp_dir.path().to_path_buf(),
            underlyings: Some(vec!["BTC-".to_string()]),
            snapshot_interval: Some(Duration::from_secs(60)),
            extract_bbo_as_quotes: true,
            write_instruments: false,
            price_precision: None,
            size_precision: None,
        };

        convert_options_chain_csv(&config).unwrap();

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let call_id = "BTC-9JUN20-10000-C.DERIBIT".to_string();
        let next_expiry_id = "BTC-10JUN20-10000-C.DERIBIT".to_string();
        let call_quotes = catalog
            .query_typed_data::<QuoteTick>(
                Some(vec![call_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        let next_expiry_greeks = catalog
            .query_typed_data::<OptionGreeks>(
                Some(vec![next_expiry_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();

        assert_eq!(call_quotes.len(), 1);
        assert_eq!(call_quotes[0].instrument_id.to_string(), call_id);
        assert_eq!(call_quotes[0].bid_price, Price::from("0.0305"));
        assert_eq!(next_expiry_greeks.len(), 1);
        assert_eq!(
            next_expiry_greeks[0].instrument_id.to_string(),
            next_expiry_id
        );
        assert_eq!(next_expiry_greeks[0].greeks.delta, 0.0);
    }

    #[rstest]
    fn test_convert_options_chain_csv_unthinned_round_trips_catalog() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = get_test_data_path("options_chain.csv");
        let config = TardisOptionsChainCSVConverterConfig {
            filepaths: vec![filepath],
            catalog_path: temp_dir.path().to_path_buf(),
            underlyings: Some(vec!["BTC-9JUN20-9875".to_string()]),
            snapshot_interval: None,
            extract_bbo_as_quotes: true,
            write_instruments: false,
            price_precision: None,
            size_precision: None,
        };

        convert_options_chain_csv(&config).unwrap();

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let instrument_id = "BTC-9JUN20-9875-P.DERIBIT".to_string();
        let quotes = catalog
            .query_typed_data::<QuoteTick>(
                Some(vec![instrument_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        let greeks = catalog
            .query_typed_data::<OptionGreeks>(
                Some(vec![instrument_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();

        assert_eq!(quotes.len(), 3);
        assert_eq!(greeks.len(), 3);
        assert_eq!(quotes[0].bid_price, Price::from("0.0205"));
        assert_eq!(quotes[1].bid_price, Price::from("0.0206"));
        assert_eq!(quotes[2].bid_price, Price::from("0.0207"));
        assert_eq!(quotes[0].ts_init.as_u64(), 1_591_574_400_196_008_000);
        assert_eq!(quotes[2].ts_init.as_u64(), 1_591_574_460_473_112_000);
        assert!(
            greeks
                .iter()
                .all(|greek| greek.instrument_id.to_string() == instrument_id)
        );
    }

    #[rstest]
    fn test_convert_options_chain_csv_can_suppress_bbo_quotes() {
        let temp_dir = TempDir::new().unwrap();
        let filepath = get_test_data_path("options_chain.csv");
        let config = TardisOptionsChainCSVConverterConfig {
            filepaths: vec![filepath],
            catalog_path: temp_dir.path().to_path_buf(),
            underlyings: Some(vec!["BTC-9JUN20-9875".to_string()]),
            snapshot_interval: None,
            extract_bbo_as_quotes: false,
            write_instruments: false,
            price_precision: None,
            size_precision: None,
        };

        convert_options_chain_csv(&config).unwrap();

        let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
        let instrument_id = "BTC-9JUN20-9875-P.DERIBIT".to_string();
        let quotes = catalog
            .query_typed_data::<QuoteTick>(
                Some(vec![instrument_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();
        let greeks = catalog
            .query_typed_data::<OptionGreeks>(
                Some(vec![instrument_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();

        assert!(quotes.is_empty());
        assert_eq!(greeks.len(), 3);
        assert!(
            greeks
                .iter()
                .all(|greek| greek.instrument_id.to_string() == instrument_id)
        );
    }

    #[rstest]
    fn test_convert_options_chain_csv_thinned_multi_file_resets_bucket_state() {
        let temp_dir = TempDir::new().unwrap();
        let fixture = fs::read_to_string(get_test_data_path("options_chain.csv")).unwrap();
        let lines = fixture.lines().collect::<Vec<_>>();
        let first_filepath = temp_dir.path().join("late_btc_options_chain.csv");
        let second_filepath = temp_dir.path().join("early_eth_options_chain.csv");
        let catalog_path = temp_dir.path().join("catalog");
        fs::create_dir(&catalog_path).unwrap();
        write_options_chain_rows(&first_filepath, lines[0], &[lines[7]]);
        write_options_chain_rows(&second_filepath, lines[0], &[lines[5]]);

        let config = TardisOptionsChainCSVConverterConfig {
            filepaths: vec![first_filepath, second_filepath],
            catalog_path: catalog_path.clone(),
            underlyings: None,
            snapshot_interval: Some(Duration::from_secs(60)),
            extract_bbo_as_quotes: true,
            write_instruments: false,
            price_precision: None,
            size_precision: None,
        };

        convert_options_chain_csv(&config).unwrap();

        let mut catalog = ParquetDataCatalog::new(&catalog_path, None, None, None, None);
        let btc_id = "BTC-9JUN20-9875-P.DERIBIT".to_string();
        let eth_id = "ETH-9JUN20-250-P.DERIBIT".to_string();
        let btc_quotes = catalog
            .query_typed_data::<QuoteTick>(Some(vec![btc_id.clone()]), None, None, None, None, true)
            .unwrap();
        let eth_quotes = catalog
            .query_typed_data::<QuoteTick>(Some(vec![eth_id.clone()]), None, None, None, None, true)
            .unwrap();
        let eth_greeks = catalog
            .query_typed_data::<OptionGreeks>(
                Some(vec![eth_id.clone()]),
                None,
                None,
                None,
                None,
                true,
            )
            .unwrap();

        assert_eq!(btc_quotes.len(), 1);
        assert_eq!(btc_quotes[0].instrument_id.to_string(), btc_id);
        assert_eq!(btc_quotes[0].bid_price, Price::from("0.0207"));
        assert_eq!(eth_quotes.len(), 1);
        assert_eq!(eth_quotes[0].instrument_id.to_string(), eth_id);
        assert_eq!(eth_quotes[0].bid_price, Price::from("0.12345"));
        assert_eq!(eth_greeks.len(), 1);
        assert_eq!(eth_greeks[0].instrument_id.to_string(), eth_id);
    }

    #[rstest]
    fn test_create_crypto_option_preserves_fractional_strike_and_new_underlying() {
        let record = TardisOptionsChainRecord {
            exchange: TardisExchange::Deribit,
            symbol: Ustr::from("NEW-9JUN20-2.05-C"),
            timestamp: 1,
            local_timestamp: 2,
            option_type: TardisOptionType::Call,
            strike_price: 2.05,
            expiration: 1_591_689_600_000_000,
            open_interest: None,
            last_price: Some(0.1),
            bid_price: Some(0.1),
            bid_amount: Some(1.0),
            bid_iv: None,
            ask_price: Some(0.2),
            ask_amount: Some(1.0),
            ask_iv: None,
            mark_price: None,
            mark_iv: None,
            underlying_index: "SYN.NEW-9JUN20".to_string(),
            underlying_price: Some(2.0),
            delta: None,
            gamma: None,
            vega: None,
            theta: None,
            rho: None,
        };
        let instrument_id = InstrumentId::from("NEW-9JUN20-2.05-C.DERIBIT");
        let instrument = create_crypto_option_from_options_chain_record(
            &record,
            instrument_id,
            UnixNanos::from(2_000),
            OptionsChainPrecision { price: 1, size: 0 },
        )
        .unwrap();

        let InstrumentAny::CryptoOption(option) = instrument else {
            panic!("Expected CryptoOption");
        };

        assert_eq!(option.underlying, Currency::get_or_create_crypto("NEW"));
        assert_eq!(option.strike_price, Price::from("2.05"));
        assert_eq!(option.price_precision, 2);
        assert_eq!(option.price_increment, Price::from("0.01"));
    }

    fn write_options_chain_rows(path: &Path, header: &str, rows: &[&str]) {
        let mut contents = String::from(header);
        contents.push('\n');
        for row in rows {
            contents.push_str(row);
            contents.push('\n');
        }
        fs::write(path, contents).unwrap();
    }
}
