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

//! Deribit-specific custom data types.
//!
//! These types carry Deribit domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use std::sync::Arc;

use nautilus_core::UnixNanos;
use serde::{Deserialize, Serialize};

use nautilus_model::data::{CustomDataTrait, HasTsInit};
#[cfg(feature = "python")]
use pyo3::prelude::*;

/// Deribit volatility index (DVOL) data.
///
/// Emitted from the `deribit_volatility_index.{index_name}` WebSocket channel.
/// The DVOL index is the market-implied volatility benchmark for BTC and ETH,
/// calculated from Deribit's options order book.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.deribit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.deribit")
)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeribitVolatilityIndex {
    /// The index name (e.g. "btc_usd" or "eth_usd").
    pub index_name: String,
    /// The DVOL value.
    pub value: f64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl DeribitVolatilityIndex {
    /// Creates a new [`DeribitVolatilityIndex`] instance.
    #[must_use]
    pub fn new(index_name: String, value: f64, ts_event: UnixNanos, ts_init: UnixNanos) -> Self {
        Self {
            index_name,
            value,
            ts_event,
            ts_init,
        }
    }
}

impl HasTsInit for DeribitVolatilityIndex {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for DeribitVolatilityIndex {
    fn type_name(&self) -> &'static str {
        "DeribitVolatilityIndex"
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
        "DeribitVolatilityIndex"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        let parsed: Self = serde_json::from_value(value)?;
        Ok(Arc::new(parsed))
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl DeribitVolatilityIndex {
    #[new]
    fn py_new(index_name: String, value: f64, ts_event: u64, ts_init: u64) -> Self {
        Self::new(
            index_name,
            value,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    fn __repr__(&self) -> String {
        format!(
            "DeribitVolatilityIndex(index_name={}, value={}, ts_event={}, ts_init={})",
            self.index_name, self.value, self.ts_event, self.ts_init
        )
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }

    #[getter]
    fn index_name(&self) -> String {
        self.index_name.clone()
    }

    #[getter]
    fn value(&self) -> f64 {
        self.value
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nautilus_model::data::CustomDataTrait;
    use rstest::rstest;

    #[rstest]
    fn test_deribit_volatility_index_new() {
        let dvol = DeribitVolatilityIndex::new(
            "btc_usd".to_string(),
            72.5,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_001),
        );
        assert_eq!(dvol.index_name, "btc_usd");
        assert_eq!(dvol.value, 72.5);
        assert_eq!(dvol.ts_event.as_u64(), 1_000_000_000);
        assert_eq!(dvol.ts_init.as_u64(), 1_000_000_001);
    }

    #[rstest]
    fn test_deribit_volatility_index_type_name() {
        let dvol = DeribitVolatilityIndex::new(
            "eth_usd".to_string(),
            65.0,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        assert_eq!(dvol.type_name(), "DeribitVolatilityIndex");
        assert_eq!(
            DeribitVolatilityIndex::type_name_static(),
            "DeribitVolatilityIndex"
        );
    }

    #[rstest]
    fn test_deribit_volatility_index_to_json() {
        let dvol = DeribitVolatilityIndex::new(
            "btc_usd".to_string(),
            72.5,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_001),
        );
        let json_str = dvol.to_json().unwrap();
        assert!(json_str.contains("btc_usd"));
        assert!(json_str.contains("72.5"));
    }

    #[rstest]
    fn test_deribit_volatility_index_from_json() {
        let json_value = serde_json::json!({
            "index_name": "btc_usd",
            "value": 72.5,
            "ts_event": 1_000_000_000u64,
            "ts_init": 1_000_000_001u64,
        });
        let arc = DeribitVolatilityIndex::from_json(json_value).unwrap();
        let dvol = arc
            .as_any()
            .downcast_ref::<DeribitVolatilityIndex>()
            .unwrap();
        assert_eq!(dvol.index_name, "btc_usd");
        assert_eq!(dvol.value, 72.5);
        assert_eq!(dvol.ts_event.as_u64(), 1_000_000_000);
        assert_eq!(dvol.ts_init.as_u64(), 1_000_000_001);
    }

    #[rstest]
    fn test_deribit_volatility_index_clone_arc() {
        let dvol = DeribitVolatilityIndex::new(
            "btc_usd".to_string(),
            72.5,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let cloned = dvol.clone_arc();
        let downcast = cloned
            .as_any()
            .downcast_ref::<DeribitVolatilityIndex>()
            .unwrap();
        assert_eq!(downcast.index_name, "btc_usd");
        assert_eq!(downcast.value, 72.5);
    }

    #[rstest]
    fn test_deribit_volatility_index_eq_arc() {
        let dvol1 = DeribitVolatilityIndex::new(
            "btc_usd".to_string(),
            72.5,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let dvol2 = DeribitVolatilityIndex::new(
            "btc_usd".to_string(),
            72.5,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let dvol3 = DeribitVolatilityIndex::new(
            "eth_usd".to_string(),
            65.0,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        assert!(dvol1.eq_arc(&dvol2));
        assert!(!dvol1.eq_arc(&dvol3));
    }
}
