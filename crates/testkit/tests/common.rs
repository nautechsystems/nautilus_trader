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

use std::{fs::File, mem::size_of};

use arrow::datatypes::DataType;
use nautilus_model::types::{price::PriceRaw, quantity::QuantityRaw};
use nautilus_testkit::common::get_nautilus_test_data_file_path;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rstest::rstest;

#[rstest]
fn selected_fixture_fixed_widths_match_model_raw_types() {
    let filepath = get_nautilus_test_data_file_path("quotes.parquet");
    let file = File::open(filepath).unwrap();
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
    let schema = builder.schema();

    assert_eq!(
        schema.field_with_name("bid_price").unwrap().data_type(),
        &DataType::FixedSizeBinary(i32::try_from(size_of::<PriceRaw>()).unwrap()),
        "selected fixture price width must match PriceRaw",
    );
    assert_eq!(
        schema.field_with_name("bid_size").unwrap().data_type(),
        &DataType::FixedSizeBinary(i32::try_from(size_of::<QuantityRaw>()).unwrap()),
        "selected fixture quantity width must match QuantityRaw",
    );
}
