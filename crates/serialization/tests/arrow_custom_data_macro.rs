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

// Links the workspace core from one shared library to collapse the binary's link time.
use std::sync::Arc;

use arrow::{
    array::{Array, DictionaryArray, ListArray, StructArray, UInt64Array},
    datatypes::{DataType, Field, Float64Type, Int32Type, TimeUnit},
    record_batch::RecordBatch,
};
use nautilus_core::{Params, UnixNanos};
#[cfg(feature = "arrow-display")]
use nautilus_model::data::NautilusDataType;
use nautilus_model::{
    data::{CustomDataTrait, Data, HasTsInit},
    enums::{AggressorSide, CurrencyType},
    identifiers::InstrumentId,
    types::{Currency, Money, Price, Quantity},
};
#[cfg(feature = "arrow-display")]
use nautilus_serialization::arrow::catalog_display::catalog_record_batch_to_display;
use nautilus_serialization::{
    arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch},
    arrow_custom_data, ensure_custom_data_registered,
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[arrow_custom_data(pyo3)]
#[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct ManualCustomData {
    #[custom_data_field(serde)]
    values: Vec<(String, u64)>,
    curve: Vec<f64>,
    value: f64,
    price: Price,
    quantity: Quantity,
    inventory: Quantity,
    optional_price: Option<Price>,
    optional_quantity: Option<Quantity>,
    decimal: Decimal,
    optional_decimal: Option<Decimal>,
    money: Money,
    optional_money: Option<Money>,
    #[custom_data_field(native_enum)]
    aggressor_side: AggressorSide,
    #[custom_data_field(native_enum)]
    optional_aggressor_side: Option<AggressorSide>,
    optional_count: Option<u64>,
    optional_small_count: Option<u32>,
    optional_ratio: Option<f64>,
    optional_float: Option<f32>,
    optional_active: Option<bool>,
    optional_signed: Option<i64>,
    optional_small_signed: Option<i32>,
    optional_label: Option<String>,
    optional_instrument_id: Option<InstrumentId>,
    optional_params: Option<Params>,
    optional_timestamp: Option<UnixNanos>,
    payload: Vec<u8>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

impl HasTsInit for ManualCustomData {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for ManualCustomData {
    fn type_name(&self) -> &'static str {
        "ManualCustomData"
    }

    fn type_name_static() -> &'static str {
        "ManualCustomData"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        serde_json::to_string(self).map_err(Into::into)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        other.as_any().downcast_ref::<Self>() == Some(self)
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        Ok(Arc::new(serde_json::from_value::<Self>(value)?))
    }
}

fn manual_custom_data_for_decode() -> ManualCustomData {
    ManualCustomData {
        values: vec![],
        curve: vec![1.0, 2.0],
        value: 1.0,
        price: Price::from("1"),
        quantity: Quantity::from("1"),
        inventory: Quantity::from("1"),
        optional_price: None,
        optional_quantity: None,
        decimal: Decimal::ONE,
        optional_decimal: None,
        money: Money::new(1.0, Currency::USD()),
        optional_money: None,
        aggressor_side: AggressorSide::Buy,
        optional_aggressor_side: None,
        optional_count: None,
        optional_small_count: Some(1),
        optional_ratio: None,
        optional_float: None,
        optional_active: None,
        optional_signed: None,
        optional_small_signed: None,
        optional_label: None,
        optional_instrument_id: None,
        optional_params: None,
        optional_timestamp: None,
        payload: vec![],
        ts_event: UnixNanos::from(1),
        ts_init: UnixNanos::from(1),
    }
}

#[rstest]
fn arrow_custom_data_supports_manual_model_implementations() {
    ensure_custom_data_registered::<ManualCustomData>();
    #[cfg(feature = "high-precision")]
    let inventory = Quantity::from("20000000000000");
    #[cfg(not(feature = "high-precision"))]
    let inventory = Quantity::from("10000000000");
    let original = ManualCustomData {
        values: vec![("manual".to_string(), 7)],
        curve: vec![1.0, 2.0],
        value: 42.5,
        price: Price::from("1.2345"),
        quantity: Quantity::from("12.345"),
        inventory,
        optional_price: None,
        optional_quantity: Some(Quantity::from("0.125")),
        decimal: Decimal::from_str_exact("123.4567890123456789").unwrap(),
        optional_decimal: None,
        money: Money::new(12.34, Currency::USD()),
        optional_money: None,
        aggressor_side: AggressorSide::Buy,
        optional_aggressor_side: Some(AggressorSide::Sell),
        optional_count: None,
        optional_small_count: Some(7),
        optional_ratio: Some(1.25),
        optional_float: None,
        optional_active: Some(true),
        optional_signed: Some(-9),
        optional_small_signed: None,
        optional_label: Some("native".to_string()),
        optional_instrument_id: Some(InstrumentId::from("ETHUSDT.BINANCE")),
        optional_params: Some(Params::new()),
        optional_timestamp: Some(UnixNanos::from(150)),
        payload: vec![0, 1, 2, 255],
        ts_event: UnixNanos::from(100),
        ts_init: UnixNanos::from(200),
    };
    let metadata = original.metadata();
    let borrowed = [&original];
    let batch = ManualCustomData::encode_batch(&metadata, &borrowed).unwrap();
    let metadata = batch.schema().metadata().clone();
    let decoded = ManualCustomData::decode_data_batch(&metadata, batch.clone()).unwrap();

    assert_eq!(
        batch.schema().field_with_name("price").unwrap().data_type(),
        &arrow::datatypes::DataType::Decimal128(38, 16),
    );
    assert!(batch.column_by_name("optional_price").unwrap().is_null(0));
    assert!(batch.column_by_name("optional_decimal").unwrap().is_null(0));
    assert!(batch.column_by_name("optional_money").unwrap().is_null(0));
    let optional_money = batch
        .column_by_name("optional_money")
        .unwrap()
        .as_any()
        .downcast_ref::<StructArray>()
        .unwrap();
    assert!(
        optional_money
            .column_by_name("currency")
            .unwrap()
            .is_null(0),
    );
    assert!(batch.column_by_name("optional_count").unwrap().is_null(0));
    assert_eq!(
        batch
            .schema()
            .field_with_name("aggressor_side")
            .unwrap()
            .data_type(),
        &nautilus_serialization::arrow::enum_dictionary_data_type(),
    );
    assert_eq!(
        batch.schema().field_with_name("money").unwrap().data_type(),
        &DataType::Struct(
            vec![
                Field::new("amount", DataType::Decimal128(38, 16), false),
                Field::new(
                    "currency",
                    DataType::Dictionary(Box::new(DataType::Int32), Box::new(DataType::Utf8)),
                    false,
                ),
            ]
            .into(),
        ),
    );
    assert_eq!(
        batch
            .schema()
            .field_with_name("values")
            .unwrap()
            .extension_type_name(),
        Some("arrow.json"),
    );
    assert_eq!(
        batch
            .schema()
            .field_with_name("ts_event")
            .unwrap()
            .data_type(),
        &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
    );
    assert_eq!(
        batch
            .schema()
            .field_with_name("ts_init")
            .unwrap()
            .data_type(),
        &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
    );
    assert_eq!(
        metadata.get("price_kind").map(String::as_str),
        Some("price")
    );
    assert_eq!(
        metadata.get("inventory_kind").map(String::as_str),
        Some("quantity"),
    );
    #[cfg(feature = "arrow-display")]
    {
        let display = catalog_record_batch_to_display(
            &NautilusDataType::Custom {
                type_name: "ManualCustomData".to_string(),
            },
            &metadata,
            &batch,
        )
        .unwrap();
        assert_eq!(
            display
                .schema()
                .field_with_name("inventory")
                .unwrap()
                .data_type(),
            &arrow::datatypes::DataType::Float64,
        );
    }

    for field in batch.schema().fields() {
        let opaque = matches!(
            field.data_type(),
            arrow::datatypes::DataType::Binary
                | arrow::datatypes::DataType::LargeBinary
                | arrow::datatypes::DataType::BinaryView
                | arrow::datatypes::DataType::FixedSizeBinary(_)
        );
        assert!(
            !opaque || field.name() == "payload",
            "unexpected opaque custom-data field {}",
            field.name(),
        );
    }
    assert_eq!(decoded.len(), 1);
    let Data::Custom(custom) = &decoded[0] else {
        panic!("expected custom data");
    };
    let decoded = custom
        .data
        .as_any()
        .downcast_ref::<ManualCustomData>()
        .unwrap();
    assert_eq!(decoded, &original);
}

#[rstest]
fn arrow_custom_data_round_trips_all_optional_some_fields() {
    ensure_custom_data_registered::<ManualCustomData>();
    let mut optional_params = Params::new();
    optional_params.insert("source".to_string(), serde_json::json!("unit-test"));
    let original = ManualCustomData {
        values: vec![("optional".to_string(), 11)],
        curve: vec![3.0, 4.0],
        value: 1.5,
        price: Price::from("2.3456"),
        quantity: Quantity::from("23.456"),
        inventory: Quantity::from("34.567"),
        optional_price: Some(Price::from("3.4567")),
        optional_quantity: Some(Quantity::from("0.375")),
        decimal: Decimal::from_str_exact("123.456789").unwrap(),
        optional_decimal: Some(Decimal::from_str_exact("987.654321").unwrap()),
        money: Money::new(56.78, Currency::USD()),
        optional_money: Some(Money::new(91.23, Currency::EUR())),
        aggressor_side: AggressorSide::Buy,
        optional_aggressor_side: Some(AggressorSide::Sell),
        optional_count: Some(17),
        optional_small_count: Some(19),
        optional_ratio: Some(2.25),
        optional_float: Some(3.5),
        optional_active: Some(true),
        optional_signed: Some(-21),
        optional_small_signed: Some(-23),
        optional_label: Some("present".to_string()),
        optional_instrument_id: Some(InstrumentId::from("BTCUSDT.BINANCE")),
        optional_params: Some(optional_params),
        optional_timestamp: Some(UnixNanos::from(250)),
        payload: vec![3, 2, 1],
        ts_event: UnixNanos::from(300),
        ts_init: UnixNanos::from(400),
    };

    let batch = ManualCustomData::encode_batch(&original.metadata(), &[&original]).unwrap();
    let metadata = batch.schema().metadata().clone();
    let decoded = ManualCustomData::decode_data_batch(&metadata, batch).unwrap();
    let Data::Custom(custom) = &decoded[0] else {
        panic!("expected custom data");
    };
    let decoded = custom
        .data
        .as_any()
        .downcast_ref::<ManualCustomData>()
        .unwrap();

    assert_eq!(decoded, &original);
}

#[rstest]
#[cfg(feature = "high-precision")]
fn arrow_custom_data_rejects_precision_above_decimal_scale() {
    let mut original = manual_custom_data_for_decode();
    original.price = Price::from_raw(1, 17);

    let error = ManualCustomData::encode_batch(&original.metadata(), &[&original]).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Invalid argument error: Price field 'price' precision 17 exceeds Arrow decimal scale 16",
    );
}

#[rstest]
fn arrow_custom_data_rejects_out_of_range_u32() {
    let original = manual_custom_data_for_decode();
    let batch = ManualCustomData::encode_batch(&original.metadata(), &[&original]).unwrap();
    let index = batch.schema().index_of("optional_small_count").unwrap();
    let mut columns = batch.columns().to_vec();
    columns[index] = Arc::new(UInt64Array::from(vec![Some(u64::from(u32::MAX) + 1)]));
    let batch = RecordBatch::try_new(batch.schema(), columns).unwrap();
    let metadata = batch.schema().metadata().clone();

    let error = ManualCustomData::decode_data_batch(&metadata, batch).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Error parsing `optional_small_count`: row 0: value is outside the u32 range",
    );
}

#[rstest]
fn arrow_custom_data_rejects_null_float_list_element() {
    let original = manual_custom_data_for_decode();
    let batch = ManualCustomData::encode_batch(&original.metadata(), &[&original]).unwrap();
    let index = batch.schema().index_of("curve").unwrap();
    let mut columns = batch.columns().to_vec();
    columns[index] = Arc::new(ListArray::from_iter_primitive::<Float64Type, _, _>([Some(
        vec![Some(1.0), None],
    )]));
    let batch = RecordBatch::try_new(batch.schema(), columns).unwrap();
    let metadata = batch.schema().metadata().clone();

    let error = ManualCustomData::decode_data_batch(&metadata, batch).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Error parsing `curve`: row 0: list element 1 is null",
    );
}

#[rstest]
fn money_currency_dictionary_supports_more_than_int8_cardinality() {
    let values = (0..130)
        .map(|index| {
            let code = format!("ARROW_CURRENCY_{index:03}");
            let currency = Currency::new(code.as_str(), 2, 0, code.as_str(), CurrencyType::Crypto);
            Some(Money::new(1.0, currency))
        })
        .collect::<Vec<_>>();

    let array = nautilus_serialization::arrow::money_array(values).unwrap();
    let currencies = array
        .column_by_name("currency")
        .unwrap()
        .as_any()
        .downcast_ref::<DictionaryArray<Int32Type>>()
        .unwrap();

    assert_eq!(currencies.len(), 130);
    assert_eq!(currencies.values().len(), 130);
}

#[rstest]
fn money_decode_rejects_unregistered_currency() {
    let code = "ARROW_UNREGISTERED_MONEY_CURRENCY";
    let currency = Currency::new(code, 2, 0, code, CurrencyType::Crypto);
    let array =
        nautilus_serialization::arrow::money_array([Some(Money::new(1.0, currency))]).unwrap();

    let error = nautilus_serialization::arrow::decode_money(&array, "money", 0).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Error parsing `money`: row 0: currency 'ARROW_UNREGISTERED_MONEY_CURRENCY' must be registered before decoding Money: Unknown currency: ARROW_UNREGISTERED_MONEY_CURRENCY",
    );
}

#[rstest]
fn arrow_custom_data_rejects_decimal_scale_above_sixteen() {
    let original = ManualCustomData {
        values: vec![],
        curve: vec![],
        value: 0.0,
        price: Price::from("1"),
        quantity: Quantity::from("1"),
        inventory: Quantity::from("1"),
        optional_price: None,
        optional_quantity: None,
        decimal: Decimal::from_i128_with_scale(1, 17),
        optional_decimal: None,
        money: Money::new(1.0, Currency::USD()),
        optional_money: None,
        aggressor_side: AggressorSide::Buy,
        optional_aggressor_side: None,
        optional_count: None,
        optional_small_count: None,
        optional_ratio: None,
        optional_float: None,
        optional_active: None,
        optional_signed: None,
        optional_small_signed: None,
        optional_label: None,
        optional_instrument_id: None,
        optional_params: None,
        optional_timestamp: None,
        payload: vec![],
        ts_event: UnixNanos::from(1),
        ts_init: UnixNanos::from(1),
    };

    let error = ManualCustomData::encode_batch(&original.metadata(), &[&original]).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Invalid argument error: Decimal field 'decimal' has scale 17, maximum supported scale is 16",
    );
}
