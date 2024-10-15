// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::path::PathBuf;

use crate::files::ensure_file_exists_or_download_http;

#[must_use]
pub fn get_workspace_root_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("Failed to get project root")
        .to_path_buf()
}

#[must_use]
pub fn get_project_root_path() -> PathBuf {
    get_workspace_root_path()
        .parent()
        .expect("Failed to get workspace root")
        .to_path_buf()
}

#[must_use]
pub fn get_test_data_path() -> PathBuf {
    if let Ok(test_data_root_path) = std::env::var("TEST_DATA_ROOT_PATH") {
        get_project_root_path()
            .join(test_data_root_path)
            .join("test_data")
    } else {
        get_project_root_path().join("tests").join("test_data")
    }
}

#[must_use]
pub fn get_test_data_file_path(path: &str) -> String {
    get_test_data_path()
        .join(path)
        .to_str()
        .unwrap()
        .to_string()
}

#[must_use]
pub fn get_test_data_large_checksums_filepath() -> PathBuf {
    get_test_data_path().join("large").join("checksums.json")
}

#[must_use]
pub fn ensure_test_data_exists(filename: &str, url: &str) -> PathBuf {
    let filepath = get_test_data_path().join("large").join(filename);
    let checksums_filepath = get_test_data_large_checksums_filepath();
    ensure_file_exists_or_download_http(&filepath, url, Some(&checksums_filepath)).unwrap();
    filepath
}

#[must_use]
pub fn ensure_data_exists_tardis_deribit_book_l2() -> PathBuf {
    let filename = "tardis_deribit_incremental_book_L2_2020-04-01_BTC-PERPETUAL.csv.gz";
    let base_url = "https://datasets.tardis.dev";
    let url = format!("{base_url}/v1/deribit/incremental_book_L2/2020/04/01/BTC-PERPETUAL.csv.gz");
    ensure_test_data_exists(filename, &url)
}

#[must_use]
pub fn ensure_data_exists_tardis_binance_snapshot5() -> PathBuf {
    let filename = "tardis_binance-futures_book_snapshot_5_2020-09-01_BTCUSDT.csv.gz";
    let base_url = "https://datasets.tardis.dev";
    let url = format!("{base_url}/v1/binance-futures/book_snapshot_5/2020/09/01/BTCUSDT.csv.gz");
    ensure_test_data_exists(filename, &url)
}

#[must_use]
pub fn ensure_data_exists_tardis_binance_snapshot25() -> PathBuf {
    let filename = "tardis_binance-futures_book_snapshot_25_2020-09-01_BTCUSDT.csv.gz";
    let base_url = "https://datasets.tardis.dev";
    let url = format!("{base_url}/v1/binance-futures/book_snapshot_25/2020/09/01/BTCUSDT.csv.gz");
    ensure_test_data_exists(filename, &url)
}

#[must_use]
pub fn ensure_data_exists_tardis_huobi_quotes() -> PathBuf {
    let filename = "tardis_huobi-dm-swap_quotes_2020-05-01_BTC-USD.csv.gz";
    let base_url = "https://datasets.tardis.dev";
    let url = format!("{base_url}/v1/huobi-dm-swap/quotes/2020/05/01/BTC-USD.csv.gz");
    ensure_test_data_exists(filename, &url)
}

#[must_use]
pub fn ensure_data_exists_tardis_bitmex_trades() -> PathBuf {
    let filename = "tardis_bitmex_trades_2020-03-01_XBTUSD.csv.gz";
    let base_url = "https://datasets.tardis.dev";
    let url = format!("{base_url}/v1/bitmex/trades/2020/03/01/XBTUSD.csv.gz");
    ensure_test_data_exists(filename, &url)
}
