// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    cmp,
    collections::HashMap,
    fmt::{Display, Formatter},
    hash::Hash,
};

use indexmap::IndexMap;
use nautilus_core::{correctness::check_equal_u8, serialization::Serializable, time::UnixNanos};
use serde::{Deserialize, Serialize};

use crate::{
    enums::PriceType,
    identifiers::instrument_id::InstrumentId,
    types::{fixed::FIXED_PRECISION, price::Price, quantity::Quantity},
};

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct QuoteTick {
    /// The quotes instrument ID.
    pub instrument_id: InstrumentId,
    /// The top of book bid price.
    pub bid_price: Price,
    /// The top of book ask price.
    pub ask_price: Price,
    /// The top of book bid size.
    pub bid_size: Quantity,
    /// The top of book ask size.
    pub ask_size: Quantity,
    /// The UNIX timestamp (nanoseconds) when the tick event occurred.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl QuoteTick {
    pub fn new(
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Self> {
        check_equal_u8(
            bid_price.precision,
            ask_price.precision,
            "bid_price.precision",
            "ask_price.precision",
        )?;
        check_equal_u8(
            bid_size.precision,
            ask_size.precision,
            "bid_size.precision",
            "ask_size.precision",
        )?;
        Ok(Self {
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        })
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata.insert("size_precision".to_string(), size_precision.to_string());
        metadata
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[must_use]
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("bid_price".to_string(), "Int64".to_string());
        metadata.insert("ask_price".to_string(), "Int64".to_string());
        metadata.insert("bid_size".to_string(), "UInt64".to_string());
        metadata.insert("ask_size".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }

    #[must_use]
    pub fn extract_price(&self, price_type: PriceType) -> Price {
        match price_type {
            PriceType::Bid => self.bid_price,
            PriceType::Ask => self.ask_price,
            PriceType::Mid => Price::from_raw(
                (self.bid_price.raw + self.ask_price.raw) / 2,
                cmp::min(self.bid_price.precision + 1, FIXED_PRECISION),
            )
            .unwrap(), // Already a valid `Price`
            _ => panic!("Cannot extract with price type {price_type}"),
        }
    }

    #[must_use]
    pub fn extract_volume(&self, price_type: PriceType) -> Quantity {
        match price_type {
            PriceType::Bid => self.bid_size,
            PriceType::Ask => self.ask_size,
            PriceType::Mid => Quantity::from_raw(
                (self.bid_size.raw + self.ask_size.raw) / 2,
                cmp::min(self.bid_size.precision + 1, FIXED_PRECISION),
            )
            .unwrap(), // Already a valid `Quantity`
            _ => panic!("Cannot extract with price type {price_type}"),
        }
    }
}

impl Display for QuoteTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id,
            self.bid_price,
            self.ask_price,
            self.bid_size,
            self.ask_size,
            self.ts_event,
        )
    }
}

impl Serializable for QuoteTick {}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "stubs")]
pub mod stubs {
    use rstest::fixture;

    use crate::{
        data::quote::QuoteTick,
        identifiers::instrument_id::InstrumentId,
        types::{price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn quote_tick_ethusdt_binance() -> QuoteTick {
        QuoteTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            bid_price: Price::from("10000.0000"),
            ask_price: Price::from("10001.0000"),
            bid_size: Quantity::from("1.00000000"),
            ask_size: Quantity::from("1.00000000"),
            ts_event: 0,
            ts_init: 1,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::serialization::Serializable;
    use pyo3::{IntoPy, Python};
    use rstest::rstest;

    use super::stubs::*;
    use crate::{data::quote::QuoteTick, enums::PriceType};

    #[rstest]
    fn test_to_string(quote_tick_ethusdt_binance: QuoteTick) {
        let tick = quote_tick_ethusdt_binance;
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,10001.0000,1.00000000,1.00000000,0"
        );
    }

    #[rstest]
    #[case(PriceType::Bid, 10_000_000_000_000)]
    #[case(PriceType::Ask, 10_001_000_000_000)]
    #[case(PriceType::Mid, 10_000_500_000_000)]
    fn test_extract_price(
        #[case] input: PriceType,
        #[case] expected: i64,
        quote_tick_ethusdt_binance: QuoteTick,
    ) {
        let tick = quote_tick_ethusdt_binance;
        let result = tick.extract_price(input).raw;
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_from_pyobject(quote_tick_ethusdt_binance: QuoteTick) {
        pyo3::prepare_freethreaded_python();
        let tick = quote_tick_ethusdt_binance;

        Python::with_gil(|py| {
            let tick_pyobject = tick.into_py(py);
            let parsed_tick = QuoteTick::from_pyobject(tick_pyobject.as_ref(py)).unwrap();
            assert_eq!(parsed_tick, tick);
        });
    }

    #[rstest]
    fn test_json_serialization(quote_tick_ethusdt_binance: QuoteTick) {
        let tick = quote_tick_ethusdt_binance;
        let serialized = tick.as_json_bytes().unwrap();
        let deserialized = QuoteTick::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }

    #[rstest]
    fn test_msgpack_serialization(quote_tick_ethusdt_binance: QuoteTick) {
        let tick = quote_tick_ethusdt_binance;
        let serialized = tick.as_msgpack_bytes().unwrap();
        let deserialized = QuoteTick::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }
}
