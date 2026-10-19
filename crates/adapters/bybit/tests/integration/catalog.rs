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

#![cfg(feature = "arrow")]

use std::sync::Arc;

use nautilus_bybit::data_types::{BybitLiquidation, register_bybit_custom_data};
use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    data::{CustomData, Data, DataType},
    enums::PositionSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence::backend::parquet::catalog::ParquetDataCatalog;
use rstest::rstest;
use tempfile::TempDir;

fn liquidation_data_type(instrument_id: InstrumentId) -> DataType {
    let mut metadata = Params::new();
    metadata.insert(
        "instrument_id".to_string(),
        serde_json::Value::String(instrument_id.to_string()),
    );

    DataType::new(
        "BybitLiquidation",
        Some(metadata),
        Some(instrument_id.to_string()),
    )
}

#[rstest]
fn liquidation_catalog_round_trip_preserves_fields() {
    register_bybit_custom_data();
    let temp_dir = TempDir::new().unwrap();
    let mut catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);
    let instrument_id = InstrumentId::from("BTCUSDT-LINEAR.BYBIT");
    let data_type = liquidation_data_type(instrument_id);

    let long = BybitLiquidation::new(
        instrument_id,
        PositionSide::Long,
        Price::from("96250.5"),
        Quantity::from("0.015"),
        UnixNanos::from(1_739_502_302_929_000_000),
        UnixNanos::from(1_739_502_303_500_000_000),
    );

    let short = BybitLiquidation::new(
        instrument_id,
        PositionSide::Short,
        Price::from("97410.0"),
        Quantity::from("1.250"),
        UnixNanos::from(1_739_502_303_011_000_000),
        UnixNanos::from(1_739_502_303_600_000_000),
    );

    let path = catalog
        .write_custom_data_batch(
            vec![
                CustomData::new(Arc::new(long.clone()), data_type.clone()),
                CustomData::new(Arc::new(short.clone()), data_type),
            ],
            None,
            None,
            Some(false),
        )
        .unwrap();

    let ids = vec![instrument_id.to_string()];
    let loaded: Vec<Data> = catalog
        .query_custom_data_dynamic("BybitLiquidation", Some(&ids), None, None, None, None, true)
        .unwrap();

    let rows: Vec<&BybitLiquidation> = loaded
        .iter()
        .map(|data| {
            let Data::Custom(custom) = data else {
                panic!("Expected Data::Custom, was {data:?}");
            };

            custom
                .data
                .as_any()
                .downcast_ref::<BybitLiquidation>()
                .expect("expected BybitLiquidation")
        })
        .collect();

    assert!(
        path.to_string_lossy()
            .contains("data/custom/BybitLiquidation/BTCUSDT-LINEAR.BYBIT")
    );
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], &long);
    assert_eq!(rows[0].position_side, PositionSide::Long);
    assert_eq!(rows[0].bankruptcy_price, Price::from("96250.5"));
    assert_eq!(rows[0].quantity, Quantity::from("0.015"));
    assert_eq!(rows[1], &short);
    assert_eq!(rows[1].position_side, PositionSide::Short);
    assert_eq!(rows[1].bankruptcy_price, Price::from("97410.0"));
    assert_eq!(rows[1].quantity, Quantity::from("1.250"));
}
