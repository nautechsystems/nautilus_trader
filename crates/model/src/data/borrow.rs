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

//! Domain types representing borrow rate data for margin-eligible currencies.

use std::{collections::HashMap, fmt::Display, hash::Hash};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, serialization::Serializable};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::HasTsInit;
use crate::{
    identifiers::Venue,
    types::{Currency, Money},
};

/// Represents a borrow rate for a margin-eligible currency at a venue.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BorrowRate {
    /// The currency that can be borrowed.
    pub currency: Currency,
    /// The venue reporting the borrow rate.
    pub venue: Venue,
    /// The annualized borrow rate (simple APR).
    pub rate: Decimal,
    /// Time interval (minutes) over which interest accrues.
    pub accrual_interval: u16,
    /// UNIX timestamp (nanoseconds) for the next interest accrual time.
    pub next_accrual_ns: Option<UnixNanos>,
    /// The maximum amount that can be borrowed.
    pub borrow_limit: Option<Money>,
    /// UNIX timestamp (nanoseconds) when the borrow rate event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was created.
    pub ts_init: UnixNanos,
}

impl PartialEq for BorrowRate {
    fn eq(&self, other: &Self) -> bool {
        self.currency == other.currency
            && self.venue == other.venue
            && self.rate == other.rate
            && self.accrual_interval == other.accrual_interval
            && self.next_accrual_ns == other.next_accrual_ns
            && self.borrow_limit == other.borrow_limit
    }
}

impl Hash for BorrowRate {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Hash only the fields used in PartialEq to maintain consistency
        self.currency.hash(state);
        self.venue.hash(state);
        self.rate.hash(state);
        self.accrual_interval.hash(state);
        self.next_accrual_ns.hash(state);
        self.borrow_limit.hash(state);
    }
}

impl BorrowRate {
    /// Creates a new [`BorrowRate`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        currency: Currency,
        venue: Venue,
        rate: Decimal,
        accrual_interval: u16,
        next_accrual_ns: Option<UnixNanos>,
        borrow_limit: Option<Money>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            currency,
            venue,
            rate,
            accrual_interval,
            next_accrual_ns,
            borrow_limit,
            ts_event,
            ts_init,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(currency: &Currency, venue: &Venue) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("currency".to_string(), currency.code.to_string());
        metadata.insert("venue".to_string(), venue.to_string());
        metadata
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[must_use]
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("rate".to_string(), "Decimal128".to_string());
        metadata.insert("accrual_interval".to_string(), "UInt16".to_string());
        metadata.insert("next_accrual_ns".to_string(), "UInt64".to_string());
        metadata.insert("borrow_limit".to_string(), "Int64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

impl Display for BorrowRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{:?},{:?},{},{}",
            self.currency,
            self.venue,
            self.rate,
            self.accrual_interval,
            self.next_accrual_ns.map(|ts| ts.as_u64()),
            self.borrow_limit.map(|m| m.to_string()),
            self.ts_event,
            self.ts_init
        )
    }
}

impl Serializable for BorrowRate {}

impl HasTsInit for BorrowRate {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

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
    fn currency() -> Currency {
        Currency::USD()
    }

    #[fixture]
    fn venue() -> Venue {
        Venue::from("BINANCE")
    }

    #[rstest]
    fn test_borrow_rate_new(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            None,
            borrow_limit,
            ts_event,
            ts_init,
        );

        assert_eq!(borrow_rate.currency, currency);
        assert_eq!(borrow_rate.venue, venue);
        assert_eq!(borrow_rate.rate, rate);
        assert_eq!(borrow_rate.accrual_interval, 60);
        assert_eq!(borrow_rate.next_accrual_ns, None);
        assert_eq!(borrow_rate.borrow_limit, borrow_limit);
        assert_eq!(borrow_rate.ts_event, ts_event);
        assert_eq!(borrow_rate.ts_init, ts_init);
    }

    #[rstest]
    fn test_borrow_rate_new_with_optional_fields(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_accrual_ns = Some(UnixNanos::from(1000));
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            next_accrual_ns,
            borrow_limit,
            ts_event,
            ts_init,
        );

        assert_eq!(borrow_rate.next_accrual_ns, next_accrual_ns);
        assert_eq!(borrow_rate.borrow_limit, borrow_limit);
    }

    #[rstest]
    fn test_borrow_rate_display(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_accrual_ns = Some(UnixNanos::from(1000));
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            next_accrual_ns,
            borrow_limit,
            ts_event,
            ts_init,
        );

        assert_eq!(
            format!("{borrow_rate}"),
            "USD,BINANCE,0.0001,60,Some(1000),Some(\"1000.00 USD\"),1,2"
        );
    }

    #[rstest]
    fn test_borrow_rate_get_ts_init(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            None,
            borrow_limit,
            ts_event,
            ts_init,
        );

        assert_eq!(borrow_rate.ts_init(), ts_init);
    }

    #[rstest]
    fn test_borrow_rate_eq_hash(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate1 = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            None,
            borrow_limit,
            ts_event,
            ts_init,
        );
        let borrow_rate2 = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            None,
            borrow_limit,
            ts_event,
            ts_init,
        );
        let borrow_rate3 = BorrowRate::new(
            currency,
            venue,
            Decimal::from_str("0.0002").unwrap(),
            60,
            None,
            borrow_limit,
            ts_event,
            ts_init,
        );

        assert_eq!(borrow_rate1, borrow_rate2);
        assert_ne!(borrow_rate1, borrow_rate3);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        borrow_rate1.hash(&mut hasher1);
        borrow_rate2.hash(&mut hasher2);
        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_borrow_rate_json_serialization(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_accrual_ns = Some(UnixNanos::from(1000));
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            next_accrual_ns,
            borrow_limit,
            ts_event,
            ts_init,
        );

        let serialized = borrow_rate.to_json_bytes().unwrap();
        let deserialized = BorrowRate::from_json_bytes(&serialized).unwrap();

        assert_eq!(borrow_rate, deserialized);
    }

    #[rstest]
    fn test_borrow_rate_msgpack_serialization(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_accrual_ns = Some(UnixNanos::from(1000));
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            next_accrual_ns,
            borrow_limit,
            ts_event,
            ts_init,
        );

        let serialized = borrow_rate.to_msgpack_bytes().unwrap();
        let deserialized = BorrowRate::from_msgpack_bytes(&serialized).unwrap();

        assert_eq!(borrow_rate, deserialized);
    }

    #[rstest]
    fn test_borrow_rate_serde_json(currency: Currency, venue: Venue) {
        let rate = Decimal::from_str("0.0001").unwrap();
        let next_accrual_ns = Some(UnixNanos::from(1000));
        let borrow_limit = Some(Money::new(1000.0, currency));
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let borrow_rate = BorrowRate::new(
            currency,
            venue,
            rate,
            60,
            next_accrual_ns,
            borrow_limit,
            ts_event,
            ts_init,
        );

        let json_str = serde_json::to_string(&borrow_rate).unwrap();
        let deserialized: BorrowRate = serde_json::from_str(&json_str).unwrap();

        assert_eq!(borrow_rate, deserialized);
    }
}
