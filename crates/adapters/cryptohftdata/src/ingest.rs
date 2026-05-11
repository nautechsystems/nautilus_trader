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

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use arrow::record_batch::RecordBatch;
use chrono::{Days, NaiveDate};
use futures::StreamExt;
use nautilus_model::data::{CustomData, DataType, custom::CustomDataTrait};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;

use crate::{
    config::CryptoHFTDataCatalogIngestConfig,
    enums::CryptoHFTDataType,
    http::{CryptoHFTDataClient, CryptoHFTDataFileRequest},
    loader::CryptoHFTDataDataLoader,
    types::{
        CryptoHFTDataLiquidation, CryptoHFTDataOpenInterest, register_cryptohftdata_custom_data,
    },
};

/// Runs CHD ingestion from a JSON config file.
///
/// # Errors
///
/// Returns an error when the config cannot be read/parsed or ingestion fails.
pub async fn run_cryptohftdata_ingest_from_config_file(config_path: &Path) -> anyhow::Result<()> {
    let config_data = fs::read_to_string(config_path)?;
    let config: CryptoHFTDataCatalogIngestConfig = serde_json::from_str(&config_data)?;
    run_cryptohftdata_ingest(config).await
}

/// Runs CHD ingestion from a parsed config.
///
/// # Errors
///
/// Returns an error when downloads, decoding, or catalog writes fail.
pub async fn run_cryptohftdata_ingest(
    config: CryptoHFTDataCatalogIngestConfig,
) -> anyhow::Result<()> {
    register_cryptohftdata_custom_data();

    let output_path = resolve_output_path(config.output_path.as_deref())?;
    fs::create_dir_all(&output_path)?;
    let cache_dir = config.cache_dir.as_deref().map(PathBuf::from);
    let compression = config
        .compression
        .clone()
        .unwrap_or_default()
        .as_parquet_compression();

    let catalog = Arc::new(ParquetDataCatalog::new(
        &output_path,
        None,
        config.batch_size,
        Some(compression),
        config.max_row_group_size,
    ));
    let result = async {
        let client = CryptoHFTDataClient::new(config.client_config())?;
        let loader = CryptoHFTDataDataLoader::new(config.batch_size, config.gap_policy);
        let max_concurrent_downloads = config.max_concurrent_downloads.unwrap_or(4).max(1);

        let from = NaiveDate::parse_from_str(&config.from, "%Y-%m-%d")?;
        let to = NaiveDate::parse_from_str(&config.to, "%Y-%m-%d")?;
        if from > to {
            anyhow::bail!(
                "invalid CHD ingest date range: from={} is after to={}",
                config.from,
                config.to
            );
        }

        let mut date = from;
        while date <= to {
            let date_str = date.format("%Y-%m-%d").to_string();
            for symbol in &config.symbols {
                for data_type in &config.data_types {
                    let hourly_batches = download_hourly_batches(
                        &client,
                        &loader,
                        cache_dir.as_deref(),
                        config.exchange,
                        symbol,
                        *data_type,
                        &date_str,
                        max_concurrent_downloads,
                    )
                    .await?;

                    for (_hour, batches) in hourly_batches {
                        write_batches_to_catalog_blocking(
                            Arc::clone(&catalog),
                            loader.clone(),
                            batches,
                            config.exchange,
                            symbol,
                            *data_type,
                        )
                        .await?;
                    }
                }
            }

            date = date
                .checked_add_days(Days::new(1))
                .ok_or_else(|| anyhow::anyhow!("date overflow after {date}"))?;
        }

        Ok(())
    }
    .await;

    // The catalog owns a DataFusion runtime. Always drop it outside the async
    // task context, including error paths, to avoid Tokio shutdown panics.
    tokio::task::spawn_blocking(move || drop(catalog)).await?;

    result
}

#[expect(clippy::too_many_arguments)]
async fn download_hourly_batches(
    client: &CryptoHFTDataClient,
    loader: &CryptoHFTDataDataLoader,
    cache_dir: Option<&Path>,
    exchange: crate::enums::CryptoHFTDataExchange,
    symbol: &str,
    data_type: CryptoHFTDataType,
    date: &str,
    max_concurrent_downloads: usize,
) -> anyhow::Result<Vec<(u8, Vec<RecordBatch>)>> {
    let requests = (0..24)
        .map(|hour| {
            CryptoHFTDataFileRequest::new(exchange, symbol.to_string(), data_type, date, hour)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut hourly_batches =
        futures::stream::iter(requests.into_iter().map(|request| async move {
            let hour = request.hour;
            let Some(bytes) = client.download_file_cached(&request, cache_dir).await? else {
                return Ok(None);
            };

            let batches = loader.record_batches_from_bytes(&bytes)?;
            if batches.is_empty() {
                return Ok(None);
            }

            Ok(Some((hour, batches)))
        }))
        .buffer_unordered(max_concurrent_downloads)
        .collect::<Vec<anyhow::Result<Option<(u8, Vec<RecordBatch>)>>>>()
        .await
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    hourly_batches.sort_by_key(|(hour, _)| *hour);
    Ok(hourly_batches)
}

async fn write_batches_to_catalog_blocking(
    catalog: Arc<ParquetDataCatalog>,
    loader: CryptoHFTDataDataLoader,
    batches: Vec<RecordBatch>,
    exchange: crate::enums::CryptoHFTDataExchange,
    symbol: &str,
    data_type: CryptoHFTDataType,
) -> anyhow::Result<()> {
    let symbol = symbol.to_string();
    tokio::task::spawn_blocking(move || {
        write_batches_to_catalog(
            catalog.as_ref(),
            &loader,
            &batches,
            exchange,
            &symbol,
            data_type,
        )
    })
    .await?
}

fn write_batches_to_catalog(
    catalog: &ParquetDataCatalog,
    loader: &CryptoHFTDataDataLoader,
    batches: &[arrow::record_batch::RecordBatch],
    exchange: crate::enums::CryptoHFTDataExchange,
    symbol: &str,
    data_type: CryptoHFTDataType,
) -> anyhow::Result<()> {
    match data_type {
        CryptoHFTDataType::Trades => {
            let data = loader.load_trades(batches, exchange, symbol, None)?;
            catalog.write_to_parquet(data, None, None, Some(false))?;
        }
        CryptoHFTDataType::Orderbook => {
            let data = loader.load_order_book_deltas(batches, exchange, symbol, None)?;
            catalog.write_to_parquet(data, None, None, Some(false))?;
        }
        CryptoHFTDataType::Klines => {
            let data = loader.load_bars(batches, exchange, symbol, None)?;
            catalog.write_to_parquet(data, None, None, Some(false))?;
        }
        CryptoHFTDataType::MarkPrice => {
            let data = loader.load_price_updates(batches, exchange, symbol, None)?;
            catalog.write_to_parquet(data.mark_prices, None, None, Some(false))?;
            catalog.write_to_parquet(data.index_prices, None, None, Some(false))?;
            catalog.write_to_parquet(data.funding_rates, None, None, Some(false))?;
        }
        CryptoHFTDataType::OpenInterest => {
            let data = loader.load_open_interest(batches, exchange, symbol, None)?;
            write_custom(catalog, data)?;
        }
        CryptoHFTDataType::Liquidations => {
            let data = loader.load_liquidations(batches, exchange, symbol, None)?;
            write_custom(catalog, data)?;
        }
        CryptoHFTDataType::Ticker => {
            log::debug!("Skipping CHD ticker data; use trades/bars/mark_price for Nautilus types");
        }
    }

    Ok(())
}

fn write_custom<T>(catalog: &ParquetDataCatalog, data: Vec<T>) -> anyhow::Result<()>
where
    T: CustomDataTrait + Clone + 'static,
{
    let custom = data
        .into_iter()
        .map(|item| {
            let instrument_id = custom_instrument_id(&item);
            let data_type = DataType::new(item.type_name(), None, instrument_id);
            CustomData::new(Arc::new(item), data_type)
        })
        .collect::<Vec<_>>();

    catalog.write_custom_data_batch(custom, None, None, Some(false))?;
    Ok(())
}

fn custom_instrument_id<T>(item: &T) -> Option<String>
where
    T: CustomDataTrait + 'static,
{
    if let Some(open_interest) = item.as_any().downcast_ref::<CryptoHFTDataOpenInterest>() {
        return Some(open_interest.instrument_id.to_string());
    }
    if let Some(liquidation) = item.as_any().downcast_ref::<CryptoHFTDataLiquidation>() {
        return Some(liquidation.instrument_id.to_string());
    }
    None
}

fn resolve_output_path(output_path: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(path) = output_path {
        return Ok(PathBuf::from(path));
    }

    let nautilus_path = std::env::var("NAUTILUS_PATH")
        .map_err(|_| anyhow::anyhow!("output_path or NAUTILUS_PATH must be set"))?;
    Ok(PathBuf::from(nautilus_path).join("catalog"))
}
