// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::Display;

use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};

use super::GetTsInit;
use crate::{identifiers::InstrumentId, types::Price};

// TODO: Development notes:
// TODO: - We avoid adding these to the `Data` enum for now, as we won't be using FFI or pycapsule.
// TODO: - Only defined in Rust with a view to minimizing Cython which will soon be removed.

/// Represents a mark price update.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct MarkPriceUpdate {
    /// The instrument ID for the mark price.
    pub instrument_id: InstrumentId,
    /// The mark price.
    pub value: Price,
    /// UNIX timestamp (nanoseconds) when the price event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl MarkPriceUpdate {
    /// Creates a new [`MarkPriceUpdate`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        value: Price,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            value,
            ts_event,
            ts_init,
        }
    }
}

impl Display for MarkPriceUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.instrument_id, self.value, self.ts_event, self.ts_init
        )
    }
}

impl Serializable for MarkPriceUpdate {}

impl GetTsInit for MarkPriceUpdate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

/// Represents an index price update.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct IndexPriceUpdate {
    /// The instrument ID for the index price.
    pub instrument_id: InstrumentId,
    /// The index price.
    pub value: Price,
    /// UNIX timestamp (nanoseconds) when the price event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl IndexPriceUpdate {
    /// Creates a new [`IndexPriceUpdate`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        value: Price,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            value,
            ts_event,
            ts_init,
        }
    }
}

impl Display for IndexPriceUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.instrument_id, self.value, self.ts_event, self.ts_init
        )
    }
}

impl Serializable for IndexPriceUpdate {}

impl GetTsInit for IndexPriceUpdate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    use nautilus_core::serialization::Serializable;
    use rstest::{fixture, rstest};
    use serde_json;

    use super::*;

    #[fixture]
    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTC-USDT.OKX")
    }

    #[fixture]
    fn price() -> Price {
        Price::from("150_500.10")
    }

    #[rstest]
    fn test_mark_price_update_new(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        assert_eq!(mark_price.instrument_id, instrument_id);
        assert_eq!(mark_price.value, price);
        assert_eq!(mark_price.ts_event, ts_event);
        assert_eq!(mark_price.ts_init, ts_init);
    }

    #[rstest]
    fn test_mark_price_update_display(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        assert_eq!(format!("{mark_price}"), "BTC-USDT.OKX,150500.10,1,2");
    }

    #[rstest]
    fn test_mark_price_update_get_ts_init(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        assert_eq!(mark_price.ts_init(), ts_init);
    }

    #[rstest]
    fn test_mark_price_update_eq_hash(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price1 = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);
        let mark_price2 = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);
        let mark_price3 =
            MarkPriceUpdate::new(instrument_id, Price::from("143_500.50"), ts_event, ts_init);

        assert_eq!(mark_price1, mark_price2);
        assert_ne!(mark_price1, mark_price3);

        // Test Hash implementation
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        mark_price1.hash(&mut hasher1);
        mark_price2.hash(&mut hasher2);
        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_mark_price_update_json_serialization(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        let serialized = mark_price.as_json_bytes().unwrap();
        let deserialized = MarkPriceUpdate::from_json_bytes(&serialized).unwrap();

        assert_eq!(mark_price, deserialized);
    }

    #[rstest]
    fn test_mark_price_update_msgpack_serialization(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        let serialized = mark_price.as_msgpack_bytes().unwrap();
        let deserialized = MarkPriceUpdate::from_msgpack_bytes(&serialized).unwrap();

        assert_eq!(mark_price, deserialized);
    }

    #[rstest]
    fn test_mark_price_update_clone(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);
        let cloned = mark_price.clone();

        assert_eq!(mark_price, cloned);
    }

    #[rstest]
    fn test_mark_price_update_serde_json(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        let json_str = serde_json::to_string(&mark_price).unwrap();
        let deserialized: MarkPriceUpdate = serde_json::from_str(&json_str).unwrap();

        assert_eq!(mark_price, deserialized);
    }

    #[rstest]
    fn test_index_price_update_new(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let index_price = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        assert_eq!(index_price.instrument_id, instrument_id);
        assert_eq!(index_price.value, price);
        assert_eq!(index_price.ts_event, ts_event);
        assert_eq!(index_price.ts_init, ts_init);
    }

    #[rstest]
    fn test_index_price_update_display(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let index_price = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        assert_eq!(format!("{index_price}"), "BTC-USDT.OKX,150500.10,1,2");
    }

    #[rstest]
    fn test_index_price_update_get_ts_init(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let index_price = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        assert_eq!(index_price.ts_init(), ts_init);
    }

    #[rstest]
    fn test_index_price_update_eq_hash(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let index_price1 = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);
        let index_price2 = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);
        let index_price3 = IndexPriceUpdate::new(instrument_id, price, UnixNanos::from(3), ts_init);

        assert_eq!(index_price1, index_price2);
        assert_ne!(index_price1, index_price3);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        index_price1.hash(&mut hasher1);
        index_price2.hash(&mut hasher2);
        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_index_price_update_json_serialization(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let index_price = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        let serialized = index_price.as_json_bytes().unwrap();
        let deserialized = IndexPriceUpdate::from_json_bytes(&serialized).unwrap();

        assert_eq!(index_price, deserialized);
    }

    #[rstest]
    fn test_index_price_update_msgpack_serialization(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let index_price = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        let serialized = index_price.as_msgpack_bytes().unwrap();
        let deserialized = IndexPriceUpdate::from_msgpack_bytes(&serialized).unwrap();

        assert_eq!(index_price, deserialized);
    }

    #[rstest]
    fn test_index_price_update_serde_json(instrument_id: InstrumentId, price: Price) {
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let index_price = IndexPriceUpdate::new(instrument_id, price, ts_event, ts_init);

        let json_str = serde_json::to_string(&index_price).unwrap();
        let deserialized: IndexPriceUpdate = serde_json::from_str(&json_str).unwrap();

        assert_eq!(index_price, deserialized);
    }
}
