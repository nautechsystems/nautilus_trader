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

//! Bybit-specific custom data types.
//!
//! These types carry Bybit domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use nautilus_core::UnixNanos;
use nautilus_model::{
    custom_data,
    enums::PositionSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
#[cfg(feature = "arrow")]
use nautilus_serialization::arrow_custom_data;

/// Bybit public liquidation from the `allLiquidation` WebSocket stream.
///
/// Bybit publishes this stream for linear and inverse contracts only.
#[cfg_attr(
    feature = "arrow",
    arrow_custom_data(pyo3, stub_module = "nautilus_trader.adapters.bybit")
)]
#[custom_data(pyo3, stub_module = "nautilus_trader.adapters.bybit")]
pub struct BybitLiquidation {
    /// The instrument for this liquidation event.
    pub instrument_id: InstrumentId,
    /// The side of the liquidated position (Bybit `Buy` means a long position was liquidated).
    #[custom_data_field(native_enum)]
    pub position_side: PositionSide,
    /// The bankruptcy price of the liquidated position.
    pub bankruptcy_price: Price,
    /// The executed liquidation size.
    pub quantity: Quantity,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Registers Bybit custom data types.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_bybit_custom_data() {
    #[cfg(feature = "arrow")]
    {
        nautilus_serialization::ensure_custom_data_registered::<BybitLiquidation>();
    }

    #[cfg(not(feature = "arrow"))]
    {
        let _ = nautilus_model::data::ensure_custom_data_json_registered::<BybitLiquidation>();
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::{custom::CustomDataTrait, register_custom_data_json};
    use rstest::rstest;

    use super::*;

    fn liquidation() -> BybitLiquidation {
        BybitLiquidation::new(
            InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
            PositionSide::Short,
            Price::from("97410.5"),
            Quantity::from("1.250"),
            UnixNanos::from(1_739_502_302_929_000_000),
            UnixNanos::from(1_739_502_303_500_000_000),
        )
    }

    #[rstest]
    fn test_register_bybit_custom_data_is_idempotent() {
        register_bybit_custom_data();
        register_bybit_custom_data();
    }

    #[rstest]
    fn test_register_bybit_custom_data_registers_json_deserializer() {
        register_bybit_custom_data();

        let err = register_custom_data_json::<BybitLiquidation>().unwrap_err();

        assert_eq!(
            err.to_string(),
            "Custom data type \"BybitLiquidation\" is already registered for JSON",
        );
    }

    #[rstest]
    fn test_bybit_liquidation_json_round_trip() {
        let original = liquidation();

        let value: serde_json::Value = serde_json::from_str(&original.to_json().unwrap()).unwrap();
        let restored = <BybitLiquidation as CustomDataTrait>::from_json(value).unwrap();
        let restored = restored
            .as_any()
            .downcast_ref::<BybitLiquidation>()
            .unwrap();

        assert_eq!(restored, &original);
        assert_eq!(restored.instrument_id, original.instrument_id);
        assert_eq!(restored.position_side, PositionSide::Short);
        assert_eq!(restored.bankruptcy_price, Price::from("97410.5"));
        assert_eq!(restored.quantity, Quantity::from("1.250"));
        assert_eq!(restored.ts_event, original.ts_event);
        assert_eq!(restored.ts_init, original.ts_init);
    }

    #[cfg(feature = "arrow")]
    #[rstest]
    fn test_bybit_liquidation_arrow_schema_uses_native_types() {
        use arrow::datatypes::DataType;
        use nautilus_serialization::arrow::ArrowSchemaProvider;

        let schema = BybitLiquidation::get_schema(None);
        let names: Vec<_> = schema.fields().iter().map(|f| f.name().as_str()).collect();

        assert_eq!(
            names,
            vec![
                "instrument_id",
                "position_side",
                "bankruptcy_price",
                "quantity",
                "ts_event",
                "ts_init",
            ],
        );
        assert!(matches!(
            schema.field(0).data_type(),
            DataType::Utf8 | DataType::Utf8View
        ));
        assert_eq!(
            schema.field(1).data_type(),
            &nautilus_serialization::arrow::enum_dictionary_data_type(),
        );
        assert_eq!(schema.field(2).data_type(), &DataType::Decimal128(38, 16));
        assert_eq!(schema.field(3).data_type(), &DataType::Decimal128(38, 16));
        assert_eq!(
            schema.field(4).data_type(),
            &nautilus_serialization::arrow::timestamp_data_type(),
        );
        assert_eq!(
            schema.field(5).data_type(),
            &nautilus_serialization::arrow::timestamp_data_type(),
        );
    }
}
