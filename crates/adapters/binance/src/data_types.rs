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
use serde::{Deserialize, Serialize};

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
    let _ = nautilus_model::data::ensure_custom_data_json_registered::<BinanceFuturesLiquidation>();
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_register_binance_custom_data_is_idempotent() {
        register_binance_custom_data();
        register_binance_custom_data();
    }
}
