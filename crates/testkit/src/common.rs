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
    types::fixed::PRECISION_BYTES,
};
use nautilus_serialization::arrow::DecodeFromRecordBatch;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

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
pub fn get_nautilus_test_data_file_path(filename: &str) -> String {
    let precision_directory = format!("{}-bit", PRECISION_BYTES * 8);
    let path = get_test_data_path()
        .join("nautilus")
        .join(precision_directory);

    path.join(filename).to_str().unwrap().to_string()
}

/// Returns the path to the checksums file for large test data files.
#[must_use]
pub fn get_test_data_large_checksums_filepath() -> PathBuf {
    get_test_data_path().join("large").join("checksums.json")
}

/// Returns the path to a large test data file that is already present locally.
///
/// # Panics
///
/// Panics if the file is missing, with the command to prepare test data.
#[must_use]
pub fn ensure_test_data_exists(filename: &str) -> PathBuf {
    let filepath = get_test_data_path().join("large").join(filename);
    assert!(
        filepath.is_file(),
        "Missing test data file: {}. Run `cargo run --locked -p nautilus-testkit --bin prepare-test-data` before testing.",
        filepath.display(),
    );
    filepath
}

/// Returns the path to the local NASDAQ ITCH AAPL deltas Parquet file.
///
/// # Panics
///
/// Panics if the file is missing, with the command to prepare test data.
#[must_use]
pub fn ensure_itch_aapl_deltas_parquet() -> PathBuf {
    ensure_test_data_exists("itch_AAPL.XNAS_2019-01-30_deltas.parquet")
}

/// Returns the path to the local Tardis Deribit BTC-PERPETUAL deltas Parquet file.
///
/// # Panics
///
/// Panics if the file is missing, with the command to prepare test data.
#[must_use]
pub fn ensure_tardis_deribit_deltas_parquet() -> PathBuf {
    ensure_test_data_exists("tardis_BTC-PERPETUAL.DERIBIT_2020-04-01_deltas.parquet")
}

/// Returns the path to the local HISTDATA EURUSD.SIM quotes Parquet file.
///
/// # Panics
///
/// Panics if the file is missing, with the command to prepare test data.
#[must_use]
pub fn ensure_histdata_eurusd_quotes_parquet() -> PathBuf {
    ensure_test_data_exists("histdata_EURUSD.SIM_2020-01_quotes.parquet")
}

/// Returns the path to the local HISTDATA EURUSD.SIM instrument Parquet file.
///
/// # Panics
///
/// Panics if the file is missing, with the command to prepare test data.
#[must_use]
pub fn ensure_histdata_eurusd_instrument_parquet() -> PathBuf {
    ensure_test_data_exists("histdata_EURUSD.SIM_2020-01_instrument.parquet")
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
/// (`price_precision=4`, `price_increment=0.0001`).
#[must_use]
pub fn itch_aapl_equity() -> InstrumentAny {
    InstrumentAny::Equity(equity_aapl_itch())
}

/// Loads ITCH AAPL order book deltas from the parquet test dataset.
///
/// Requires prepared local test data. Pass `limit` to subsample.
#[must_use]
pub fn load_itch_aapl_deltas(limit: Option<usize>) -> Vec<OrderBookDelta> {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    let filepath = PATH.get_or_init(ensure_itch_aapl_deltas_parquet);
    load_deltas_from_parquet(filepath, limit)
}

/// Loads Tardis Deribit BTC-PERPETUAL order book deltas from the parquet test dataset.
///
/// Requires prepared local test data. Pass `limit` to subsample.
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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tempfile::TempDir;

    use super::*;

    #[rstest]
    #[case::file("file")]
    #[case::missing("missing")]
    #[case::directory("directory")]
    fn test_ensure_test_data_exists(#[case] state: &str) {
        let directory = TempDir::new().unwrap();
        let filepath = directory.path().join("fixture.parquet");
        if state == "file" {
            std::fs::write(&filepath, "local fixture").unwrap();
        } else if state == "directory" {
            std::fs::create_dir(&filepath).unwrap();
        }

        // The absolute path isolates this test without changing the shared test data root
        let result =
            std::panic::catch_unwind(|| ensure_test_data_exists(filepath.to_str().unwrap()));

        if state == "file" {
            assert_eq!(result.unwrap(), filepath);
            assert_eq!(std::fs::read_to_string(&filepath).unwrap(), "local fixture");
        } else {
            let panic = result.unwrap_err().downcast::<String>().unwrap();
            assert_eq!(
                *panic,
                format!(
                    "Missing test data file: {}. Run `cargo run --locked -p nautilus-testkit --bin prepare-test-data` before testing.",
                    filepath.display(),
                ),
            );
            assert_eq!(filepath.exists(), state == "directory");
        }
    }
}
