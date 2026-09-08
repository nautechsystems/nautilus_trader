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
    custom_data,
    data::BarType,
    enums::AggressorSide,
    identifiers::{AccountId, InstrumentId},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_serialization::arrow_custom_data;

/// A simple Rust custom data type for roundtrip testing.
///
/// Used in persistence integration tests (`test_catalog.rs`) and Python roundtrip tests.
/// Tests call `ensure_custom_data_registered::<RustTestCustomData>()` before using the catalog.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "test data uses the custom data macro output under test"
    )
)]
#[arrow_custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
#[custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
pub struct RustTestCustomData {
    pub instrument_id: InstrumentId,
    pub value: f64,
    pub flag: bool,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises raw byte field support.
#[arrow_custom_data]
#[custom_data]
pub struct RustTestBytesCustomData {
    pub value: Vec<u8>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type with native fixed-point Arrow fields.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "test data uses the custom data macro output under test"
    )
)]
#[arrow_custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
#[custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
pub struct RustTestFixedCustomData {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub quantity: Quantity,
    #[custom_data_field(native_enum)]
    pub aggressor_side: AggressorSide,
    pub notional: Money,
    #[custom_data_field(native_enum)]
    pub nullable_aggressor_side: Option<AggressorSide>,
    pub nullable_notional: Option<Money>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// YieldCurveData-equivalent custom data type using the macro with `Vec<f64>` fields.
///
/// Tests `Vec<f64>` / `ListFloat64` support. Exposed to Python for roundtrip tests.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "test data uses the custom data macro output under test"
    )
)]
#[arrow_custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
#[custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
pub struct MacroYieldCurveData {
    pub curve_name: String,
    pub tenors: Vec<f64>,
    pub interest_rates: Vec<f64>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises `Params` field support in the macro.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "test data uses the custom data macro output under test"
    )
)]
#[arrow_custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
#[custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
pub struct RustTestParamsCustomData {
    pub name: String,
    pub params: Params,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises typed map field support in the macro.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "test data uses the custom data macro output under test"
    )
)]
#[arrow_custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
#[custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
pub struct RustTestPriceMapCustomData {
    pub name: String,
    #[custom_data_field(serde)]
    pub prices: IndexMap<InstrumentId, Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises typed JSON map values across PyO3-supported types.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "test data uses the custom data macro output under test"
    )
)]
#[arrow_custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
#[custom_data(pyo3, stub_module = "nautilus_trader.persistence")]
pub struct RustTestTypedMapCustomData {
    pub name: String,
    #[custom_data_field(serde)]
    pub instrument_ids: IndexMap<String, InstrumentId>,
    #[custom_data_field(serde)]
    pub account_ids: IndexMap<String, AccountId>,
    #[custom_data_field(serde)]
    pub currencies: IndexMap<String, Currency>,
    #[custom_data_field(serde)]
    pub bar_types: IndexMap<String, BarType>,
    #[custom_data_field(serde)]
    pub prices: IndexMap<String, Price>,
    #[custom_data_field(serde)]
    pub quantities: IndexMap<String, Quantity>,
    #[custom_data_field(serde)]
    pub monies: IndexMap<String, Money>,
    #[custom_data_field(serde)]
    pub prices_by_instrument: IndexMap<InstrumentId, Price>,
    #[custom_data_field(serde)]
    pub quantities_by_account: IndexMap<AccountId, Quantity>,
    #[custom_data_field(serde)]
    pub monies_by_currency: IndexMap<Currency, Money>,
    #[custom_data_field(serde)]
    pub prices_by_bar_type: IndexMap<BarType, Price>,
    #[custom_data_field(serde)]
    pub hash_prices_by_instrument: HashMap<InstrumentId, Price>,
    #[custom_data_field(serde)]
    pub strings: HashMap<String, String>,
    #[custom_data_field(serde)]
    pub floats_64: HashMap<String, f64>,
    #[custom_data_field(serde)]
    pub floats_32: HashMap<String, f32>,
    #[custom_data_field(serde)]
    pub booleans: HashMap<String, bool>,
    #[custom_data_field(serde)]
    pub integers_u64: HashMap<String, u64>,
    #[custom_data_field(serde)]
    pub integers_i64: HashMap<String, i64>,
    #[custom_data_field(serde)]
    pub integers_u32: HashMap<String, u32>,
    #[custom_data_field(serde)]
    pub integers_i32: HashMap<String, i32>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Rust custom data type that exercises generic JSON map field support.
#[arrow_custom_data]
#[custom_data]
pub struct RustTestHashMapCustomData {
    pub name: String,
    #[custom_data_field(serde)]
    pub prices: HashMap<String, Price>,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Plain Serde enum stored inside custom data as a field.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RustTestSerdeFieldKind {
    Alpha,
    Beta { count: u64 },
}

/// Plain Serde payload stored inside custom data without its own timestamps.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RustTestSerdeFieldPayload {
    pub kind: RustTestSerdeFieldKind,
    pub label: String,
    pub values: Vec<f64>,
}

/// Rust custom data type that exercises arbitrary Serde field support.
#[arrow_custom_data]
#[custom_data]
pub struct RustTestSerdeFieldCustomData {
    pub name: String,
    #[custom_data_field(serde)]
    pub payload: RustTestSerdeFieldPayload,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, Schema};
    use nautilus_model::data::get_arrow_schema;
    use nautilus_serialization::{
        arrow::{
            ArrowSchemaProvider, DecodeDataFromRecordBatch, EncodeToRecordBatch,
            timestamp_data_type,
        },
        ensure_custom_data_registered,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn registered_macro_custom_schemas_are_open() {
        ensure_custom_data_registered::<RustTestCustomData>();
        ensure_custom_data_registered::<RustTestFixedCustomData>();
        ensure_custom_data_registered::<MacroYieldCurveData>();
        ensure_custom_data_registered::<RustTestParamsCustomData>();
        ensure_custom_data_registered::<RustTestPriceMapCustomData>();
        ensure_custom_data_registered::<RustTestTypedMapCustomData>();
        ensure_custom_data_registered::<RustTestHashMapCustomData>();
        ensure_custom_data_registered::<RustTestSerdeFieldCustomData>();

        let schemas = [
            registered_schema::<RustTestCustomData>(),
            registered_schema::<RustTestFixedCustomData>(),
            registered_schema::<MacroYieldCurveData>(),
            registered_schema::<RustTestParamsCustomData>(),
            registered_schema::<RustTestPriceMapCustomData>(),
            registered_schema::<RustTestTypedMapCustomData>(),
            registered_schema::<RustTestHashMapCustomData>(),
            registered_schema::<RustTestSerdeFieldCustomData>(),
        ];

        for (name, schema) in schemas {
            for field in schema.fields() {
                assert_open_custom_field(name, field);
            }
        }
    }

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

    fn registered_schema<T>() -> (&'static str, Arc<Schema>)
    where
        T: ArrowSchemaProvider,
    {
        let name = std::any::type_name::<T>();
        (
            name,
            get_arrow_schema(name.rsplit("::").next().unwrap()).unwrap(),
        )
    }

    fn assert_open_custom_field(type_name: &str, field: &Field) {
        if field.name().starts_with("ts_") {
            assert_eq!(
                field.data_type(),
                &timestamp_data_type(),
                "registered custom schema {type_name} timestamp field {} is not a UTC nanosecond timestamp",
                field.name(),
            );
        }

        match field.data_type() {
            DataType::Binary
            | DataType::LargeBinary
            | DataType::BinaryView
            | DataType::FixedSizeBinary(_) => {
                panic!(
                    "registered custom schema {type_name} contains opaque field {}: {}",
                    field.name(),
                    field.data_type(),
                );
            }
            DataType::List(child)
            | DataType::LargeList(child)
            | DataType::ListView(child)
            | DataType::LargeListView(child)
            | DataType::FixedSizeList(child, _)
            | DataType::Map(child, _) => assert_open_custom_field(type_name, child),
            DataType::Struct(children) => {
                for child in children {
                    assert_open_custom_field(type_name, child);
                }
            }
            _ => {}
        }
    }

    #[rstest]
    fn test_rust_test_serde_field_custom_data_schema_uses_utf8_for_payload() {
        let schema = RustTestSerdeFieldCustomData::get_schema(None);
        let payload_field = schema.field_with_name("payload").unwrap();

        assert_eq!(payload_field.data_type(), &DataType::Utf8);
    }

    #[rstest]
    fn test_rust_test_serde_field_custom_data_roundtrip_decodes_exact_payload() {
        let original = RustTestSerdeFieldCustomData {
            name: "serde-field".to_string(),
            payload: RustTestSerdeFieldPayload {
                kind: RustTestSerdeFieldKind::Beta { count: 7 },
                label: "payload".to_string(),
                values: vec![1.0, 2.0, 3.0],
            },
            ts_event: UnixNanos::from(10),
            ts_init: UnixNanos::from(11),
        };
        let metadata = original.metadata();
        let batch =
            RustTestSerdeFieldCustomData::encode_batch(&metadata, std::slice::from_ref(&original))
                .unwrap();
        let decoded = RustTestSerdeFieldCustomData::decode_data_batch(&metadata, batch).unwrap();

        assert_eq!(decoded.len(), 1);
        let decoded =
            RustTestSerdeFieldCustomData::try_from(decoded.into_iter().next().unwrap()).unwrap();
        assert_eq!(decoded, original);
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
