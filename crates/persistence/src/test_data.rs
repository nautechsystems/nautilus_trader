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

use nautilus_core::{Params, UnixNanos};
use nautilus_model::identifiers::InstrumentId;
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
}
