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

//! Python bindings from [PyO3](https://pyo3.rs).

use std::path::PathBuf;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta, TradeTick,
        ensure_rust_extractor_registered,
    },
    identifiers::InstrumentId,
};
use nautilus_serialization::ensure_custom_data_registered;
use pyo3::prelude::*;
use strum::IntoEnumIterator;

use crate::{
    config::{CryptoHFTDataCatalogIngestConfig, CryptoHFTDataClientConfig, ParquetCompression},
    enums::{CryptoHFTDataExchange, CryptoHFTDataType, GapPolicy},
    http::CryptoHFTDataClient,
    ingest::run_cryptohftdata_ingest_from_config_file,
    loader::{CryptoHFTDataDataLoader, CryptoHFTDataPriceUpdates},
    types::{CryptoHFTDataLiquidation, CryptoHFTDataOpenInterest},
};

fn parse_exchange(value: &str) -> PyResult<CryptoHFTDataExchange> {
    value
        .parse()
        .map_err(|e| to_pyvalue_err(format!("invalid CHD exchange '{value}': {e}")))
}

fn parse_data_type(value: &str) -> PyResult<CryptoHFTDataType> {
    value
        .parse()
        .map_err(|e| to_pyvalue_err(format!("invalid CHD data type '{value}': {e}")))
}

fn parse_gap_policy(value: Option<&str>) -> PyResult<Option<GapPolicy>> {
    value
        .map(|value| {
            value
                .parse()
                .map_err(|e| to_pyvalue_err(format!("invalid CHD gap policy '{value}': {e}")))
        })
        .transpose()
}

fn parse_compression(value: Option<&str>) -> PyResult<Option<ParquetCompression>> {
    match value {
        None => Ok(None),
        Some("zstd") => Ok(Some(ParquetCompression::Zstd)),
        Some("snappy") => Ok(Some(ParquetCompression::Snappy)),
        Some("uncompressed") => Ok(Some(ParquetCompression::Uncompressed)),
        Some(value) => Err(to_pyvalue_err(format!(
            "invalid parquet compression '{value}'"
        ))),
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CryptoHFTDataClientConfig {
    /// Configuration for a CHD HTTP client.
    #[new]
    #[pyo3(signature = (
        api_key=None,
        base_url=None,
        use_jwt=None,
        proxy_url=None,
        timeout_secs=None,
        rate_limit_per_sec=None,
    ))]
    fn py_new(
        api_key: Option<String>,
        base_url: Option<String>,
        use_jwt: Option<bool>,
        proxy_url: Option<String>,
        timeout_secs: Option<u64>,
        rate_limit_per_sec: Option<usize>,
    ) -> Self {
        let defaults = Self::default();
        Self {
            api_key,
            base_url: base_url.or(defaults.base_url),
            use_jwt: use_jwt.or(defaults.use_jwt),
            proxy_url,
            timeout_secs: timeout_secs.or(defaults.timeout_secs),
            rate_limit_per_sec: rate_limit_per_sec.or(defaults.rate_limit_per_sec),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CryptoHFTDataCatalogIngestConfig {
    /// Configuration for direct CHD -> Nautilus catalog ingestion.
    #[new]
    #[pyo3(signature = (
        exchange,
        symbols,
        data_types,
        from,
        to,
        output_path=None,
        cache_dir=None,
        api_key=None,
        base_url=None,
        use_jwt=None,
        proxy_url=None,
        timeout_secs=None,
        max_concurrent_downloads=None,
        batch_size=None,
        max_row_group_size=None,
        compression=None,
        gap_policy=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        exchange: &str,
        symbols: Vec<String>,
        data_types: Vec<String>,
        from: String,
        to: String,
        output_path: Option<String>,
        cache_dir: Option<String>,
        api_key: Option<String>,
        base_url: Option<String>,
        use_jwt: Option<bool>,
        proxy_url: Option<String>,
        timeout_secs: Option<u64>,
        max_concurrent_downloads: Option<usize>,
        batch_size: Option<usize>,
        max_row_group_size: Option<usize>,
        compression: Option<&str>,
        gap_policy: Option<&str>,
    ) -> PyResult<Self> {
        let data_types = data_types
            .into_iter()
            .map(|value| parse_data_type(&value))
            .collect::<PyResult<Vec<_>>>()?;

        Ok(Self {
            exchange: parse_exchange(exchange)?,
            symbols,
            data_types,
            from,
            to,
            output_path,
            cache_dir,
            api_key,
            base_url,
            use_jwt,
            proxy_url,
            timeout_secs,
            max_concurrent_downloads,
            batch_size,
            max_row_group_size,
            compression: parse_compression(compression)?,
            gap_policy: parse_gap_policy(gap_policy)?,
        })
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CryptoHFTDataClient {
    /// HTTP client for CHD parquet downloads.
    #[new]
    #[pyo3(signature = (config=None))]
    fn py_new(config: Option<CryptoHFTDataClientConfig>) -> PyResult<Self> {
        Self::new(config.unwrap_or_default()).map_err(to_pyruntime_err)
    }

    /// Returns a masked API key if a key is configured.
    #[getter]
    #[pyo3(name = "api_key_masked")]
    fn py_api_key_masked(&self) -> Option<String> {
        self.api_key_masked()
    }

    /// Returns the configured CHD base URL.
    #[getter]
    #[pyo3(name = "base_url")]
    fn py_base_url(&self) -> String {
        self.base_url().to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[expect(clippy::needless_pass_by_value)]
impl CryptoHFTDataDataLoader {
    /// CHD parquet loader.
    #[new]
    #[pyo3(signature = (batch_size=None, gap_policy=None))]
    fn py_new(batch_size: Option<usize>, gap_policy: Option<&str>) -> PyResult<Self> {
        Ok(Self::new(batch_size, parse_gap_policy(gap_policy)?))
    }

    /// Loads CHD trades from Arrow record batches.
    #[pyo3(name = "load_trades")]
    #[pyo3(signature = (filepath, exchange, symbol, instrument_id=None))]
    fn py_load_trades(
        &self,
        filepath: PathBuf,
        exchange: &str,
        symbol: &str,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<TradeTick>> {
        let batches = self
            .record_batches_from_path(&filepath)
            .map_err(to_pyvalue_err)?;
        self.load_trades(&batches, parse_exchange(exchange)?, symbol, instrument_id)
            .map_err(to_pyvalue_err)
    }

    /// Loads CHD order book rows as Nautilus order book deltas.
    #[pyo3(name = "load_order_book_deltas")]
    #[pyo3(signature = (filepath, exchange, symbol, instrument_id=None))]
    fn py_load_order_book_deltas(
        &self,
        filepath: PathBuf,
        exchange: &str,
        symbol: &str,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<OrderBookDelta>> {
        let batches = self
            .record_batches_from_path(&filepath)
            .map_err(to_pyvalue_err)?;
        self.load_order_book_deltas(&batches, parse_exchange(exchange)?, symbol, instrument_id)
            .map_err(to_pyvalue_err)
    }

    /// Loads CHD klines as 1-minute externally aggregated last-price bars.
    #[pyo3(name = "load_bars")]
    #[pyo3(signature = (filepath, exchange, symbol, instrument_id=None))]
    fn py_load_bars(
        &self,
        filepath: PathBuf,
        exchange: &str,
        symbol: &str,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<Bar>> {
        let batches = self
            .record_batches_from_path(&filepath)
            .map_err(to_pyvalue_err)?;
        self.load_bars(&batches, parse_exchange(exchange)?, symbol, instrument_id)
            .map_err(to_pyvalue_err)
    }

    /// Loads CHD mark, index and funding updates.
    #[pyo3(name = "load_price_updates")]
    #[pyo3(signature = (filepath, exchange, symbol, instrument_id=None))]
    fn py_load_price_updates(
        &self,
        filepath: PathBuf,
        exchange: &str,
        symbol: &str,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<(
        Vec<MarkPriceUpdate>,
        Vec<IndexPriceUpdate>,
        Vec<FundingRateUpdate>,
    )> {
        let batches = self
            .record_batches_from_path(&filepath)
            .map_err(to_pyvalue_err)?;
        let updates: CryptoHFTDataPriceUpdates = self
            .load_price_updates(&batches, parse_exchange(exchange)?, symbol, instrument_id)
            .map_err(to_pyvalue_err)?;
        Ok((
            updates.mark_prices,
            updates.index_prices,
            updates.funding_rates,
        ))
    }

    /// Loads CHD open interest snapshots.
    #[pyo3(name = "load_open_interest")]
    #[pyo3(signature = (filepath, exchange, symbol, instrument_id=None))]
    fn py_load_open_interest(
        &self,
        filepath: PathBuf,
        exchange: &str,
        symbol: &str,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<CryptoHFTDataOpenInterest>> {
        let batches = self
            .record_batches_from_path(&filepath)
            .map_err(to_pyvalue_err)?;
        self.load_open_interest(&batches, parse_exchange(exchange)?, symbol, instrument_id)
            .map_err(to_pyvalue_err)
    }

    /// Loads CHD liquidation events.
    #[pyo3(name = "load_liquidations")]
    #[pyo3(signature = (filepath, exchange, symbol, instrument_id=None))]
    fn py_load_liquidations(
        &self,
        filepath: PathBuf,
        exchange: &str,
        symbol: &str,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Vec<CryptoHFTDataLiquidation>> {
        let batches = self
            .record_batches_from_path(&filepath)
            .map_err(to_pyvalue_err)?;
        self.load_liquidations(&batches, parse_exchange(exchange)?, symbol, instrument_id)
            .map_err(to_pyvalue_err)
    }
}

/// Runs CHD catalog ingestion from a JSON config file.
///
/// # Errors
///
/// Returns a Python exception if the config cannot be read or ingestion fails.
#[pyfunction(name = "run_cryptohftdata_ingest_from_config")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.cryptohftdata")]
fn py_run_cryptohftdata_ingest_from_config<'py>(
    py: Python<'py>,
    config_path: PathBuf,
) -> PyResult<Bound<'py, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        run_cryptohftdata_ingest_from_config_file(&config_path)
            .await
            .map_err(to_pyruntime_err)?;
        Ok(())
    })
}

/// Returns supported CHD exchange identifiers.
#[pyfunction(name = "cryptohftdata_exchanges")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.cryptohftdata")]
fn py_cryptohftdata_exchanges() -> Vec<String> {
    CryptoHFTDataExchange::iter()
        .map(|exchange| exchange.as_chd_str().to_string())
        .collect()
}

/// Returns supported CHD data type identifiers.
#[pyfunction(name = "cryptohftdata_data_types")]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.cryptohftdata")]
fn py_cryptohftdata_data_types() -> Vec<String> {
    CryptoHFTDataType::iter()
        .map(|data_type| data_type.as_chd_str().to_string())
        .collect()
}

/// CryptoHFTData Python module.
///
/// # Errors
///
/// Returns a `PyErr` if module registration fails.
#[pymodule]
pub fn cryptohftdata(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<CryptoHFTDataClientConfig>()?;
    m.add_class::<CryptoHFTDataCatalogIngestConfig>()?;
    m.add_class::<CryptoHFTDataClient>()?;
    m.add_class::<CryptoHFTDataDataLoader>()?;
    m.add_class::<CryptoHFTDataOpenInterest>()?;
    m.add_class::<CryptoHFTDataLiquidation>()?;
    m.add_function(wrap_pyfunction!(
        py_run_cryptohftdata_ingest_from_config,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(py_cryptohftdata_exchanges, m)?)?;
    m.add_function(wrap_pyfunction!(py_cryptohftdata_data_types, m)?)?;

    ensure_custom_data_registered::<CryptoHFTDataOpenInterest>();
    ensure_custom_data_registered::<CryptoHFTDataLiquidation>();
    let _result = ensure_rust_extractor_registered::<CryptoHFTDataOpenInterest>();
    let _result = ensure_rust_extractor_registered::<CryptoHFTDataLiquidation>();

    Ok(())
}
