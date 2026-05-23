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

//! Binance-specific custom data types.
//!
//! These types carry Binance domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use std::sync::Arc;

use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{HasTsInit, custom::CustomDataTrait},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Binance Futures current open interest snapshot.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.binance")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceFuturesOpenInterest {
    /// The instrument for this snapshot.
    pub instrument_id: InstrumentId,
    /// The total open interest value.
    pub open_interest: Decimal,
    /// UNIX timestamp (nanoseconds) when the snapshot event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl BinanceFuturesOpenInterest {
    /// Creates a new [`BinanceFuturesOpenInterest`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        open_interest: Decimal,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            open_interest,
            ts_event,
            ts_init,
        }
    }
}

impl HasTsInit for BinanceFuturesOpenInterest {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BinanceFuturesOpenInterest {
    fn type_name(&self) -> &'static str {
        "BinanceFuturesOpenInterest"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "BinanceFuturesOpenInterest"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let json_str = serde_json::to_string(&value)?;
        let parsed: Self = serde_json::from_str(&json_str)?;
        Ok(Arc::new(parsed))
    }
}

/// Binance Futures historical open interest point.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.binance")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceFuturesOpenInterestHistPoint {
    /// The total open interest value.
    pub sum_open_interest: Decimal,
    /// The total open interest notional value.
    pub sum_open_interest_value: Decimal,
    /// UNIX timestamp (nanoseconds) for the bucket represented by this point.
    pub ts_event: UnixNanos,
}

impl BinanceFuturesOpenInterestHistPoint {
    /// Creates a new [`BinanceFuturesOpenInterestHistPoint`] instance.
    #[must_use]
    pub fn new(
        sum_open_interest: Decimal,
        sum_open_interest_value: Decimal,
        ts_event: UnixNanos,
    ) -> Self {
        Self {
            sum_open_interest,
            sum_open_interest_value,
            ts_event,
        }
    }
}

/// Binance Futures historical open interest batch.
///
/// For COIN-M requests, the current Binance adapter support is limited to
/// perpetual instruments. Although Binance also exposes quarter-delivery
/// contract types on the historical OI endpoint, the futures instrument
/// parsing/symbology path in this adapter is still perpetual-only.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.binance")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceFuturesOpenInterestHist {
    /// The instrument for this batch.
    pub instrument_id: InstrumentId,
    /// The Binance period string used for the request (e.g. "5m").
    pub period: String,
    /// Ordered open interest history points returned by Binance.
    pub points: Vec<BinanceFuturesOpenInterestHistPoint>,
    /// UNIX timestamp (nanoseconds) for the batch, represented by the final point.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl BinanceFuturesOpenInterestHist {
    /// Creates a new [`BinanceFuturesOpenInterestHist`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        period: String,
        points: Vec<BinanceFuturesOpenInterestHistPoint>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            period,
            points,
            ts_event,
            ts_init,
        }
    }
}

impl HasTsInit for BinanceFuturesOpenInterestHist {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BinanceFuturesOpenInterestHist {
    fn type_name(&self) -> &'static str {
        "BinanceFuturesOpenInterestHist"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "BinanceFuturesOpenInterestHist"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let json_str = serde_json::to_string(&value)?;
        let parsed: Self = serde_json::from_str(&json_str)?;
        Ok(Arc::new(parsed))
    }
}

/// Binance Futures liquidation update from the `forceOrder` stream.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.binance")
)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BinanceFuturesLiquidation {
    /// The instrument for this liquidation event.
    pub instrument_id: InstrumentId,
    /// The liquidation order side.
    pub side: OrderSide,
    /// The order price.
    pub price: Price,
    /// The average fill price.
    pub average_price: Price,
    /// The last filled quantity.
    pub last_filled_qty: Quantity,
    /// The cumulative filled quantity.
    pub accumulated_qty: Quantity,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl BinanceFuturesLiquidation {
    /// Creates a new [`BinanceFuturesLiquidation`] instance.
    #[must_use]
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        side: OrderSide,
        price: Price,
        average_price: Price,
        last_filled_qty: Quantity,
        accumulated_qty: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            side,
            price,
            average_price,
            last_filled_qty,
            accumulated_qty,
            ts_event,
            ts_init,
        }
    }
}

impl HasTsInit for BinanceFuturesLiquidation {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for BinanceFuturesLiquidation {
    fn type_name(&self) -> &'static str {
        "BinanceFuturesLiquidation"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }

    fn clone_arc(&self) -> Arc<dyn CustomDataTrait> {
        Arc::new(self.clone())
    }

    fn eq_arc(&self, other: &dyn CustomDataTrait) -> bool {
        if let Some(o) = other.as_any().downcast_ref::<Self>() {
            self == o
        } else {
            false
        }
    }

    #[cfg(feature = "python")]
    fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
        nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
    }

    fn type_name_static() -> &'static str {
        "BinanceFuturesLiquidation"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let json_str = serde_json::to_string(&value)?;
        let parsed: Self = serde_json::from_str(&json_str)?;
        Ok(Arc::new(parsed))
    }
}

/// Registers Binance custom data types.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_binance_custom_data() {
    let _ =
        nautilus_model::data::ensure_custom_data_json_registered::<BinanceFuturesOpenInterest>();
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<
        BinanceFuturesOpenInterestHist,
    >();
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<BinanceFuturesLiquidation>();
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "python")]
    use std::sync::Arc;

    #[cfg(feature = "python")]
    use nautilus_core::Params;
    #[cfg(feature = "python")]
    use nautilus_model::data::{CustomData, DataType};
    #[cfg(feature = "python")]
    use pyo3::{prelude::*, types::PyList};
    use rstest::rstest;
    #[cfg(feature = "python")]
    use rust_decimal::Decimal;

    use super::*;

    #[rstest]
    fn test_register_binance_custom_data_is_idempotent() {
        register_binance_custom_data();
        register_binance_custom_data();
    }

    #[cfg(feature = "python")]
    #[rstest]
    fn test_open_interest_hist_points_roundtrip_as_typed_python_list() {
        pyo3::Python::initialize();
        register_binance_custom_data();

        Python::attach(|py| {
            let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
            let points = vec![
                BinanceFuturesOpenInterestHistPoint::new(
                    Decimal::from_str_exact("100.0").unwrap(),
                    Decimal::from_str_exact("1000.0").unwrap(),
                    UnixNanos::from_millis(1_700_000_000_000),
                ),
                BinanceFuturesOpenInterestHistPoint::new(
                    Decimal::from_str_exact("101.0").unwrap(),
                    Decimal::from_str_exact("1005.0").unwrap(),
                    UnixNanos::from_millis(1_700_000_300_000),
                ),
            ];
            let payload = BinanceFuturesOpenInterestHist::new(
                instrument_id,
                "5m".to_string(),
                points,
                UnixNanos::from_millis(1_700_000_300_000),
                UnixNanos::from(42_u64),
            );

            let mut metadata = Params::new();
            metadata.insert(
                "instrument_id".to_string(),
                serde_json::Value::String("BTCUSDT-PERP.BINANCE".to_string()),
            );
            metadata.insert(
                "period".to_string(),
                serde_json::Value::String("5m".to_string()),
            );

            let custom = CustomData::new(
                Arc::new(payload),
                DataType::new(
                    "BinanceFuturesOpenInterestHist",
                    Some(metadata),
                    Some("BTCUSDT-PERP.BINANCE".to_string()),
                ),
            );

            let py_custom = Py::new(py, custom).unwrap();
            let py_payload = py_custom.bind(py).getattr("data").unwrap();
            let py_points = py_payload
                .getattr("points")
                .unwrap()
                .cast_into::<PyList>()
                .unwrap();

            assert_eq!(py_points.len(), 2);
            assert!(
                py_points
                    .get_item(0)
                    .unwrap()
                    .is_instance_of::<BinanceFuturesOpenInterestHistPoint>()
            );

            let point0 = py_points
                .get_item(0)
                .unwrap()
                .extract::<BinanceFuturesOpenInterestHistPoint>()
                .unwrap();
            let point1 = py_points
                .get_item(1)
                .unwrap()
                .extract::<BinanceFuturesOpenInterestHistPoint>()
                .unwrap();

            assert_eq!(
                point0.sum_open_interest,
                Decimal::from_str_exact("100.0").unwrap()
            );
            assert_eq!(
                point1.sum_open_interest_value,
                Decimal::from_str_exact("1005.0").unwrap()
            );
        });
    }
}
