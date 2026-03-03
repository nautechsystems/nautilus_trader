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
    fs::File,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use nautilus_core::paths::get_test_data_path;
use nautilus_model::{
    data::OrderBookDelta,
    instruments::{InstrumentAny, stubs::equity_aapl_itch},
};
use nautilus_serialization::arrow::DecodeFromRecordBatch;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use crate::files::ensure_file_exists_or_download_http;

/// Returns the full path to the test data file at the specified relative `path` within the standard test data directory.
///
/// # Panics
///
/// Panics if the computed path cannot be represented as a valid UTF-8 string.
#[must_use]
pub fn get_test_data_file_path(path: &str) -> String {
    get_test_data_path()
        .join(path)
        .to_str()
        .unwrap()
        .to_string()
}

/// Returns the full path to the Nautilus-specific test data file given by `filename`, within the configured precision directory ("64-bit" or "128-bit").
///
/// # Panics
///
/// Panics if the computed path cannot be represented as a valid UTF-8 string.
#[must_use]
#[allow(unused_mut)]
pub fn get_nautilus_test_data_file_path(filename: &str) -> String {
    let mut path = get_test_data_path().join("nautilus");

    #[cfg(feature = "high-precision")]
    {
        path = path.join("128-bit");
    }
    #[cfg(not(feature = "high-precision"))]
    {
        path = path.join("64-bit");
    }

    path.join(filename).to_str().unwrap().to_string()
}

/// Returns the path to the checksums file for large test data files.
#[must_use]
pub fn get_test_data_large_checksums_filepath() -> PathBuf {
    get_test_data_path().join("large").join("checksums.json")
}

/// Ensures that the specified test data file exists locally by downloading it if necessary, using the provided `url`.
///
/// # Panics
///
/// Panics if the download or checksum verification fails, or if the resulting path cannot be represented as a valid UTF-8 string.
#[must_use]
pub fn ensure_test_data_exists(filename: &str, url: &str) -> PathBuf {
    let filepath = get_test_data_path().join("large").join(filename);
    let checksums_filepath = get_test_data_large_checksums_filepath();
    ensure_file_exists_or_download_http(&filepath, url, Some(&checksums_filepath), None).unwrap();
    filepath
}

/// Ensures the NASDAQ ITCH AAPL deltas Parquet file exists locally, downloading from R2 if necessary.
///
/// # Panics
///
/// Panics if the download or checksum verification fails.
#[must_use]
pub fn ensure_itch_aapl_deltas_parquet() -> PathBuf {
    ensure_test_data_exists(
        "itch_AAPL.XNAS_2019-01-30_deltas.parquet",
        "https://test-data.nautechsystems.io/large/itch_AAPL.XNAS_2019-01-30_deltas.parquet",
    )
}

/// Ensures the Tardis Deribit BTC-PERPETUAL deltas Parquet file exists locally, downloading from R2 if necessary.
///
/// # Panics
///
/// Panics if the download or checksum verification fails.
#[must_use]
pub fn ensure_tardis_deribit_deltas_parquet() -> PathBuf {
    ensure_test_data_exists(
        "tardis_BTC-PERPETUAL.DERIBIT_2020-04-01_deltas.parquet",
        "https://test-data.nautechsystems.io/large/tardis_BTC-PERPETUAL.DERIBIT_2020-04-01_deltas.parquet",
    )
}

/// Returns the path to the Tardis Deribit incremental book L2 test data.
#[must_use]
pub fn get_tardis_deribit_book_l2_path() -> PathBuf {
    get_test_data_path()
        .join("tardis")
        .join("deribit_incremental_book_L2_BTC-PERPETUAL.csv")
}

/// Returns the path to the Tardis Binance Futures book snapshot (depth 5) test data.
#[must_use]
pub fn get_tardis_binance_snapshot5_path() -> PathBuf {
    get_test_data_path()
        .join("tardis")
        .join("binance-futures_book_snapshot_5_BTCUSDT.csv")
}

/// Returns the path to the Tardis Binance Futures book snapshot (depth 25) test data.
#[must_use]
pub fn get_tardis_binance_snapshot25_path() -> PathBuf {
    get_test_data_path()
        .join("tardis")
        .join("binance-futures_book_snapshot_25_BTCUSDT.csv")
}

/// Returns the path to the Tardis Huobi quotes test data.
#[must_use]
pub fn get_tardis_huobi_quotes_path() -> PathBuf {
    get_test_data_path()
        .join("tardis")
        .join("huobi-dm-swap_quotes_BTC-USD.csv")
}

/// Returns the path to the Tardis Bitmex trades test data.
#[must_use]
pub fn get_tardis_bitmex_trades_path() -> PathBuf {
    get_test_data_path()
        .join("tardis")
        .join("bitmex_trades_XBTUSD.csv")
}

/// Returns an AAPL equity instrument with ITCH-compatible precision
/// (price_precision=4, price_increment=0.0001).
#[must_use]
pub fn itch_aapl_equity() -> InstrumentAny {
    InstrumentAny::Equity(equity_aapl_itch())
}

/// Loads ITCH AAPL order book deltas from the parquet test dataset.
///
/// Downloads the file on first access. Pass `limit` to subsample.
#[must_use]
pub fn load_itch_aapl_deltas(limit: Option<usize>) -> Vec<OrderBookDelta> {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    let filepath = PATH.get_or_init(ensure_itch_aapl_deltas_parquet);
    load_deltas_from_parquet(filepath, limit)
}

/// Loads Tardis Deribit BTC-PERPETUAL order book deltas from the parquet test dataset.
///
/// Downloads the file on first access. Pass `limit` to subsample.
#[must_use]
pub fn load_tardis_deribit_deltas(limit: Option<usize>) -> Vec<OrderBookDelta> {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    let filepath = PATH.get_or_init(ensure_tardis_deribit_deltas_parquet);
    load_deltas_from_parquet(filepath, limit)
}

fn load_deltas_from_parquet(filepath: &Path, limit: Option<usize>) -> Vec<OrderBookDelta> {
    let file = File::open(filepath).unwrap();
    let mut builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
    let metadata = builder.schema().metadata().clone();

    if let Some(limit) = limit {
        builder = builder.with_limit(limit);
    }
    let reader = builder.build().unwrap();

    let mut deltas = Vec::new();
    for batch_result in reader {
        let batch = batch_result.unwrap();
        let batch_deltas = OrderBookDelta::decode_batch(&metadata, batch).unwrap();
        deltas.extend(batch_deltas);
    }
    deltas
}
