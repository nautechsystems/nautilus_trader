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

//! Hyperliquid-specific custom data types.
//!
//! These types carry Hyperliquid domain data through the Nautilus data engine as
//! [`CustomData`](nautilus_model::data::CustomData).

use std::{collections::HashMap, sync::Arc};

use nautilus_core::UnixNanos;
use nautilus_model::{data::CustomDataTrait, identifiers::InstrumentId, types::Price};
use serde::{Deserialize, Serialize};

use nautilus_model::data::HasTsInit;

#[cfg(feature = "python")]
use pyo3::prelude::*;

/// Hyperliquid all mid prices data.
///
/// Emitted from the `allMids` WebSocket channel.
/// Contains a snapshot of mid-prices for all tradable coins.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.hyperliquid")
)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HyperliquidAllMids {
    /// Mapping of instrument ID to mid price for all tradable coins.
    pub mids: HashMap<InstrumentId, Price>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl HyperliquidAllMids {
    /// Creates a new [`HyperliquidAllMids`] instance.
    #[must_use]
    pub fn new(
        mids: HashMap<InstrumentId, Price>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            mids,
            ts_event,
            ts_init,
        }
    }
}

impl HasTsInit for HyperliquidAllMids {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

impl CustomDataTrait for HyperliquidAllMids {
    fn type_name(&self) -> &'static str {
        "HyperliquidAllMids"
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
        "HyperliquidAllMids"
    }

    fn from_json(value: serde_json::Value) -> anyhow::Result<Arc<dyn CustomDataTrait>> {
        // Price and InstrumentId deserialize from borrowed &str, so we must go through
        // a string representation rather than serde_json::from_value which
        // produces owned strings.
        let json_str = serde_json::to_string(&value)?;
        let parsed: Self = serde_json::from_str(&json_str)?;
        Ok(Arc::new(parsed))
    }
}

#[cfg(feature = "python")]
#[pymethods]
impl HyperliquidAllMids {
    #[new]
    fn py_new(ts_event: u64, ts_init: u64) -> Self {
        Self::new(
            HashMap::new(),
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_init),
        )
    }

    fn __repr__(&self) -> String {
        format!(
            "HyperliquidAllMids(mids_count={}, ts_event={}, ts_init={})",
            self.mids.len(),
            self.ts_event,
            self.ts_init
        )
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }

    #[getter]
    fn mids(&self) -> Vec<(String, String)> {
        self.mids
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
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
    use std::collections::HashMap;

    use super::*;
    use nautilus_model::{
        data::CustomDataTrait,
        identifiers::{InstrumentId, Symbol, Venue},
    };
    use rstest::rstest;

    fn create_test_instrument_id(symbol: &str) -> InstrumentId {
        InstrumentId::new(Symbol::from(symbol), Venue::from("HYPERLIQUID"))
    }

    #[rstest]
    fn test_hyperliquid_all_mids_new() {
        let mut mids = HashMap::new();
        mids.insert(
            create_test_instrument_id("BTC-PERP"),
            Price::from("65000.5"),
        );
        mids.insert(
            create_test_instrument_id("ETH-PERP"),
            Price::from("3200.25"),
        );

        let all_mids = HyperliquidAllMids::new(
            mids,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_001),
        );
        assert_eq!(all_mids.mids.len(), 2);
        assert_eq!(all_mids.ts_event.as_u64(), 1_000_000_000);
        assert_eq!(all_mids.ts_init.as_u64(), 1_000_000_001);
    }

    #[rstest]
    fn test_hyperliquid_all_mids_type_name() {
        let all_mids =
            HyperliquidAllMids::new(HashMap::new(), UnixNanos::default(), UnixNanos::default());
        assert_eq!(all_mids.type_name(), "HyperliquidAllMids");
        assert_eq!(HyperliquidAllMids::type_name_static(), "HyperliquidAllMids");
    }

    #[rstest]
    fn test_hyperliquid_all_mids_to_json() {
        let mut mids = HashMap::new();
        mids.insert(
            create_test_instrument_id("BTC-PERP"),
            Price::from("65000.5"),
        );

        let all_mids = HyperliquidAllMids::new(
            mids,
            UnixNanos::from(1_000_000_000),
            UnixNanos::from(1_000_000_001),
        );
        let json_str = all_mids.to_json().unwrap();
        assert!(json_str.contains("BTC-PERP"));
        assert!(json_str.contains("65000.5"));
    }

    #[rstest]
    fn test_hyperliquid_all_mids_from_json() {
        let json_value = serde_json::json!({
            "mids": {
                "BTC-PERP.HYPERLIQUID": "65000.5",
                "ETH-PERP.HYPERLIQUID": "3200.25"
            },
            "ts_event": 1_000_000_000u64,
            "ts_init": 1_000_000_001u64,
        });
        let arc = HyperliquidAllMids::from_json(json_value).unwrap();
        let all_mids = arc.as_any().downcast_ref::<HyperliquidAllMids>().unwrap();
        assert_eq!(all_mids.mids.len(), 2);
        assert_eq!(all_mids.ts_event.as_u64(), 1_000_000_000);
        assert_eq!(all_mids.ts_init.as_u64(), 1_000_000_001);
    }

    #[rstest]
    fn test_hyperliquid_all_mids_clone_arc() {
        let mut mids = HashMap::new();
        mids.insert(
            create_test_instrument_id("BTC-PERP"),
            Price::from("65000.5"),
        );

        let all_mids = HyperliquidAllMids::new(mids, UnixNanos::default(), UnixNanos::default());
        let cloned = all_mids.clone_arc();
        let downcast = cloned
            .as_any()
            .downcast_ref::<HyperliquidAllMids>()
            .unwrap();
        assert_eq!(downcast.mids.len(), 1);
    }

    #[rstest]
    fn test_hyperliquid_all_mids_eq_arc() {
        let mut mids1 = HashMap::new();
        mids1.insert(
            create_test_instrument_id("BTC-PERP"),
            Price::from("65000.5"),
        );

        let mut mids2 = HashMap::new();
        mids2.insert(
            create_test_instrument_id("BTC-PERP"),
            Price::from("65000.5"),
        );

        let mut mids3 = HashMap::new();
        mids3.insert(
            create_test_instrument_id("ETH-PERP"),
            Price::from("3200.25"),
        );

        let all_mids1 = HyperliquidAllMids::new(mids1, UnixNanos::default(), UnixNanos::default());
        let all_mids2 = HyperliquidAllMids::new(mids2, UnixNanos::default(), UnixNanos::default());
        let all_mids3 = HyperliquidAllMids::new(mids3, UnixNanos::default(), UnixNanos::default());

        assert!(all_mids1.eq_arc(&all_mids2));
        assert!(!all_mids1.eq_arc(&all_mids3));
    }
}
