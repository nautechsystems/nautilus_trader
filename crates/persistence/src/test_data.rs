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

//! Rust custom data types used for catalog roundtrip testing.
//!
//! Exposed to Python via the persistence PyO3 module so Python tests can exercise
//! custom data write/query roundtrips.

use std::collections::HashMap;

use indexmap::IndexMap;
use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_persistence_macros::custom_data;

/// A simple Rust custom data type for roundtrip testing.
///
/// Used in persistence integration tests (`test_catalog.rs`) and Python roundtrip tests.
/// Tests call `ensure_custom_data_registered::<RustTestCustomData>()` before using the catalog.
#[custom_data(pyo3)]
pub struct RustTestCustomData {
    pub instrument_id: InstrumentId,
    pub value: f64,
    pub flag: bool,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// YieldCurveData-equivalent custom data type using the macro with `Vec<f64>` fields.
///
/// Tests `Vec<f64>` / ListFloat64 support. Exposed to Python for roundtrip tests.
#[custom_data(pyo3)]
pub struct MacroYieldCurveData {
    pub curve_name: String,
    pub tenors: Vec<f64>,
    pub interest_rates: Vec<f64>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises `Params` field support in the macro.
#[custom_data(pyo3)]
pub struct RustTestParamsCustomData {
    pub name: String,
    pub params: Params,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises typed map field support in the macro.
#[custom_data(pyo3)]
pub struct RustTestPriceMapCustomData {
    pub name: String,
    #[custom_data_field(json)]
    pub prices: IndexMap<InstrumentId, Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises typed JSON map values across PyO3-supported types.
#[custom_data(pyo3)]
pub struct RustTestTypedMapCustomData {
    pub name: String,
    #[custom_data_field(json)]
    pub instrument_ids: IndexMap<String, InstrumentId>,
    #[custom_data_field(json)]
    pub account_ids: IndexMap<String, AccountId>,
    #[custom_data_field(json)]
    pub currencies: IndexMap<String, Currency>,
    #[custom_data_field(json)]
    pub bar_types: IndexMap<String, BarType>,
    #[custom_data_field(json)]
    pub prices: IndexMap<String, Price>,
    #[custom_data_field(json)]
    pub quantities: IndexMap<String, Quantity>,
    #[custom_data_field(json)]
    pub monies: IndexMap<String, Money>,
    #[custom_data_field(json)]
    pub prices_by_instrument: IndexMap<InstrumentId, Price>,
    #[custom_data_field(json)]
    pub quantities_by_account: IndexMap<AccountId, Quantity>,
    #[custom_data_field(json)]
    pub monies_by_currency: IndexMap<Currency, Money>,
    #[custom_data_field(json)]
    pub prices_by_bar_type: IndexMap<BarType, Price>,
    #[custom_data_field(json)]
    pub hash_prices_by_instrument: HashMap<InstrumentId, Price>,
    #[custom_data_field(json)]
    pub strings: HashMap<String, String>,
    #[custom_data_field(json)]
    pub floats_64: HashMap<String, f64>,
    #[custom_data_field(json)]
    pub floats_32: HashMap<String, f32>,
    #[custom_data_field(json)]
    pub booleans: HashMap<String, bool>,
    #[custom_data_field(json)]
    pub integers_u64: HashMap<String, u64>,
    #[custom_data_field(json)]
    pub integers_i64: HashMap<String, i64>,
    #[custom_data_field(json)]
    pub integers_u32: HashMap<String, u32>,
    #[custom_data_field(json)]
    pub integers_i32: HashMap<String, i32>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises generic JSON map field support.
#[custom_data]
pub struct RustTestHashMapCustomData {
    pub name: String,
    #[custom_data_field(json)]
    pub prices: HashMap<String, Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[cfg(test)]
mod tests {
    use arrow::datatypes::DataType;
    use nautilus_serialization::arrow::ArrowSchemaProvider;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_macro_yield_curve_data_schema_has_ts_init() {
        let schema = MacroYieldCurveData::get_schema(None);
        let field_names: Vec<_> = schema.fields().iter().map(|f| f.name().clone()).collect();
        assert!(
            field_names.iter().any(|f| f == "ts_init"),
            "Schema must have ts_init for DataFusion ORDER BY; got: {field_names:?}",
        );
        assert!(
            field_names.iter().any(|f| f == "ts_event"),
            "Schema must have ts_event; got: {field_names:?}",
        );
    }

    #[rstest]
    fn test_rust_test_params_custom_data_schema_uses_utf8_for_params() {
        let schema = RustTestParamsCustomData::get_schema(None);
        let params_field = schema.field_with_name("params").unwrap();

        assert_eq!(params_field.data_type(), &DataType::Utf8);
    }

    #[rstest]
    fn test_rust_test_price_map_custom_data_schema_uses_utf8_for_prices() {
        let schema = RustTestPriceMapCustomData::get_schema(None);
        let prices_field = schema.field_with_name("prices").unwrap();

        assert_eq!(prices_field.data_type(), &DataType::Utf8);
    }

    #[rstest]
    fn test_rust_test_hash_map_custom_data_schema_uses_utf8_for_prices() {
        let schema = RustTestHashMapCustomData::get_schema(None);
        let prices_field = schema.field_with_name("prices").unwrap();

        assert_eq!(prices_field.data_type(), &DataType::Utf8);
    }

    #[rstest]
    fn test_rust_test_typed_map_custom_data_schema_uses_utf8_for_json_maps() {
        let schema = RustTestTypedMapCustomData::get_schema(None);

        for field_name in [
            "instrument_ids",
            "account_ids",
            "currencies",
            "bar_types",
            "prices",
            "quantities",
            "monies",
            "prices_by_instrument",
            "quantities_by_account",
            "monies_by_currency",
            "prices_by_bar_type",
            "hash_prices_by_instrument",
            "strings",
            "floats_64",
            "floats_32",
            "booleans",
            "integers_u64",
            "integers_i64",
            "integers_u32",
            "integers_i32",
        ] {
            let field = schema.field_with_name(field_name).unwrap();
            assert_eq!(field.data_type(), &DataType::Utf8);
        }
    }
}
