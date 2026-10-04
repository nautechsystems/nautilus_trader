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

use arrow::datatypes::{DataType, TimeUnit};
use nautilus_model::types::{price::PriceRaw, quantity::QuantityRaw};
use nautilus_testkit::common::{get_nautilus_test_data_file_path, get_test_data_file_path};
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

#[rstest]
#[case("quotes.parquet", Some("bid_price"))]
#[case("trades.parquet", Some("price"))]
#[case("bars.parquet", Some("open"))]
#[case("deltas.parquet", Some("price"))]
#[case("quotes-3-groups-filter-query.parquet", Some("bid_price"))]
#[case("depths.parquet", None)]
fn current_arrow_fixture_uses_decimal128_and_utc_ns(
    #[case] file_name: &str,
    #[case] fixed_field: Option<&str>,
) {
    let filepath = get_test_data_file_path(&format!("nautilus/arrow/{file_name}"));
    let file = File::open(filepath).unwrap();
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
    let schema = builder.schema();

    if let Some(fixed_field) = fixed_field {
        assert_eq!(
            schema.field_with_name(fixed_field).unwrap().data_type(),
            &DataType::Decimal128(38, 16),
        );
    }
    assert_eq!(
        schema.field_with_name("ts_init").unwrap().data_type(),
        &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
    );
}
