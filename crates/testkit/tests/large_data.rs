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

use nautilus_testkit::common::{
    ensure_histdata_eurusd_instrument_parquet, ensure_histdata_eurusd_quotes_parquet,
};
use rstest::rstest;

#[rstest]
fn ensure_histdata_eurusd_quotes_parquet_downloads() {
    let filepath = ensure_histdata_eurusd_quotes_parquet();

    assert!(filepath.exists());
    assert_eq!(
        filepath.file_name().unwrap().to_str().unwrap(),
        "histdata_EURUSD.SIM_2020-01_quotes.parquet",
    );
}

#[rstest]
fn ensure_histdata_eurusd_instrument_parquet_downloads() {
    let filepath = ensure_histdata_eurusd_instrument_parquet();

    assert!(filepath.exists());
    assert_eq!(
        filepath.file_name().unwrap().to_str().unwrap(),
        "histdata_EURUSD.SIM_2020-01_instrument.parquet",
    );
}
