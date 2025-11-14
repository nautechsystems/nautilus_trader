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

//! Domain types representing funding rate data for perpetual swap instruments.

use std::{collections::HashMap, fmt::Display, hash::Hash};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, serialization::Serializable};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::HasTsInit;
use crate::identifiers::InstrumentId;

/// Represents a funding rate update for perpetual swap instruments.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct FundingRateUpdate {
    /// The instrument ID for the funding rate.
    pub instrument_id: InstrumentId,
    /// The current funding rate.
    pub rate: Decimal,
    /// UNIX timestamp (nanoseconds) for the next funding time.
    pub next_funding_ns: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the funding rate event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl PartialEq for FundingRateUpdate {
    fn eq(&self, other: &Self) -> bool {
        self.instrument_id == other.instrument_id
            && self.rate == other.rate
            && self.next_funding_ns == other.next_funding_ns
    }
}

impl Hash for FundingRateUpdate {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash only the fields used in PartialEq to maintain consistency
        self.instrument_id.hash(state);
        self.rate.hash(state);
        self.next_funding_ns.hash(state);
    }
}

impl FundingRateUpdate {
    /// Creates a new [`FundingRateUpdate`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        rate: Decimal,
        next_funding_ns: Option<UnixNanos>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            rate,
            next_funding_ns,
            ts_event,
            ts_init,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(instrument_id: &InstrumentId) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[must_use]
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("rate".to_string(), "Decimal128".to_string());
        metadata.insert("next_funding_ns".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

impl Display for FundingRateUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{:?},{},{}",
            self.instrument_id,
            self.rate,
            self.next_funding_ns.map(|ts| ts.as_u64()),
            self.ts_event,
            self.ts_init
        )
    }
}

impl Serializable for FundingRateUpdate {}

impl HasTsInit for FundingRateUpdate {
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
        str::FromStr,
    };

    use nautilus_core::serialization::{
        Serializable,
        msgpack::{FromMsgPack, ToMsgPack},
    };
    use rstest::{fixture, rstest};
    use serde_json;

    use super::*;

    #[fixture]
    fn instrument_id() -> InstrumentId {
        InstrumentId::from("BTCUSDT-PERP.BINANCE")
    }

    #[rstest]
    fn test_funding_rate_update_new(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate = FundingRateUpdate::new(instrument_id, rate, None, ts_event, ts_init);

        assert_eq!(funding_rate.instrument_id, instrument_id);
        assert_eq!(funding_rate.rate, rate);
        assert_eq!(funding_rate.next_funding_ns, None);
        assert_eq!(funding_rate.ts_event, ts_event);
        assert_eq!(funding_rate.ts_init, ts_init);
    }

    #[rstest]
    fn test_funding_rate_update_new_with_optional_fields(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_funding_ns = Some(UnixNanos::from(1000));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate =
            FundingRateUpdate::new(instrument_id, rate, next_funding_ns, ts_event, ts_init);

        assert_eq!(funding_rate.instrument_id, instrument_id);
        assert_eq!(funding_rate.rate, rate);
        assert_eq!(funding_rate.next_funding_ns, next_funding_ns);
        assert_eq!(funding_rate.ts_event, ts_event);
        assert_eq!(funding_rate.ts_init, ts_init);
    }

    #[rstest]
    fn test_funding_rate_update_display(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_funding_ns = Some(UnixNanos::from(1000));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate =
            FundingRateUpdate::new(instrument_id, rate, next_funding_ns, ts_event, ts_init);

        assert_eq!(
            format!("{funding_rate}"),
            "BTCUSDT-PERP.BINANCE,0.0001,Some(1000),1,2"
        );
    }

    #[rstest]
    fn test_funding_rate_update_get_ts_init(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate = FundingRateUpdate::new(instrument_id, rate, None, ts_event, ts_init);

        assert_eq!(funding_rate.ts_init(), ts_init);
    }

    #[rstest]
    fn test_funding_rate_update_eq_hash(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate1 = FundingRateUpdate::new(instrument_id, rate, None, ts_event, ts_init);
        let funding_rate2 = FundingRateUpdate::new(instrument_id, rate, None, ts_event, ts_init);
        let funding_rate3 = FundingRateUpdate::new(
            instrument_id,
            Decimal::from_str("0.0002").unwrap(),
            None,
            ts_event,
            ts_init,
        );

        assert_eq!(funding_rate1, funding_rate2);
        assert_ne!(funding_rate1, funding_rate3);

        // Test Hash implementation
        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        funding_rate1.hash(&mut hasher1);
        funding_rate2.hash(&mut hasher2);
        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_funding_rate_update_json_serialization(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_funding_ns = Some(UnixNanos::from(1000));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate =
            FundingRateUpdate::new(instrument_id, rate, next_funding_ns, ts_event, ts_init);

        let serialized = funding_rate.to_json_bytes().unwrap();
        let deserialized = FundingRateUpdate::from_json_bytes(&serialized).unwrap();

        assert_eq!(funding_rate, deserialized);
    }

    #[rstest]
    fn test_funding_rate_update_msgpack_serialization(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_funding_ns = Some(UnixNanos::from(1000));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate =
            FundingRateUpdate::new(instrument_id, rate, next_funding_ns, ts_event, ts_init);

        let serialized = funding_rate.to_msgpack_bytes().unwrap();
        let deserialized = FundingRateUpdate::from_msgpack_bytes(&serialized).unwrap();

        assert_eq!(funding_rate, deserialized);
    }

    #[rstest]
    fn test_funding_rate_update_serde_json(instrument_id: InstrumentId) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_funding_ns = Some(UnixNanos::from(1000));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let funding_rate =
            FundingRateUpdate::new(instrument_id, rate, next_funding_ns, ts_event, ts_init);

        let json_str = serde_json::to_string(&funding_rate).unwrap();
        let deserialized: FundingRateUpdate = serde_json::from_str(&json_str).unwrap();

        assert_eq!(funding_rate, deserialized);
    }
}
