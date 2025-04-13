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

use std::path::Path;

use nautilus_core::paths::get_workspace_root_path;
use nautilus_databento::loader::DatabentoDataLoader;
use nautilus_model::{enums::BookType, identifiers::InstrumentId, orderbook::OrderBook};
use nautilus_testkit::{
    common::{get_test_data_file_path, get_test_data_large_checksums_filepath},
    files::ensure_file_exists_or_download_http,
};
use rstest::*;

#[rstest]
pub fn test_order_book_databento_mbo_nasdaq() {
    let checksums = get_test_data_large_checksums_filepath();
    let filename = "databento_mbo_xnas_itch.csv";
    let file_path = get_test_data_file_path(format!("large/{filename}").as_str());
    let url = "https://hist.databento.com/v0/dataset/sample/download/xnas.itch/mbo";
    ensure_file_exists_or_download_http(Path::new(file_path.as_str()), url, Some(&checksums))
        .unwrap();

    let instrument_id = InstrumentId::from("AAPL.XNAS");
    let mut _book = OrderBook::new(instrument_id, BookType::L3_MBO);

    let publishers_filepath = get_workspace_root_path()
        .join("adapters")
        .join("databento")
        .join("publishers.json");
    let _loader = DatabentoDataLoader::new(Some(publishers_filepath)).unwrap();
    // let deltas = loader
    //     .load_order_book_deltas(filepath, Some(instrument_id))
    //     .unwrap();
    //
    // for delta in deltas.iter() {
    //     book.apply_delta(delta);
    // }

    // assert_eq!(book.best_bid_price().unwrap(), price);
    // assert_eq!(book.best_ask_price().unwrap(), price);
    // assert_eq!(book.best_bid_size().unwrap(), size);
    // assert_eq!(book.best_ask_size().unwrap(), size);
}
