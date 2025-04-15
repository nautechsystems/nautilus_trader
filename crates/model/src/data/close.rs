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

//! An `InstrumentClose` data type representing an instrument close at a venue.

use std::{collections::HashMap, fmt::Display, hash::Hash};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};

use super::GetTsInit;
use crate::{
    enums::InstrumentCloseType,
    identifiers::InstrumentId,
    types::{Price, fixed::FIXED_SIZE_BINARY},
};

/// Represents an instrument close at a venue.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct InstrumentClose {
    /// The instrument ID.
    pub instrument_id: InstrumentId,
    /// The closing price for the instrument.
    pub close_price: Price,
    /// The type of closing price.
    pub close_type: InstrumentCloseType,
    /// UNIX timestamp (nanoseconds) when the close price event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
}

impl InstrumentClose {
    /// Creates a new [`InstrumentClose`] instance.
    pub fn new(
        instrument_id: InstrumentId,
        close_price: Price,
        close_type: InstrumentCloseType,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            close_price,
            close_type,
            ts_event,
            ts_init,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata
    }

    /// Returns the field map for the type, for use with Arrow schemas.
    #[must_use]
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("close_price".to_string(), FIXED_SIZE_BINARY.to_string());
        metadata.insert("close_type".to_string(), "UInt8".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }
}

impl Display for InstrumentClose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.instrument_id, self.close_price, self.close_type, self.ts_event
        )
    }
}

impl Serializable for InstrumentClose {}

impl GetTsInit for InstrumentClose {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{identifiers::InstrumentId, types::Price};

    #[rstest]
    fn test_new() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let close_price = Price::from("150.20");
        let close_type = InstrumentCloseType::EndOfSession;
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let instrument_close =
            InstrumentClose::new(instrument_id, close_price, close_type, ts_event, ts_init);

        assert_eq!(instrument_close.instrument_id, instrument_id);
        assert_eq!(instrument_close.close_price, close_price);
        assert_eq!(instrument_close.close_type, close_type);
        assert_eq!(instrument_close.ts_event, ts_event);
        assert_eq!(instrument_close.ts_init, ts_init);
    }

    #[rstest]
    fn test_to_string() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let close_price = Price::from("150.20");
        let close_type = InstrumentCloseType::EndOfSession;
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let instrument_close =
            InstrumentClose::new(instrument_id, close_price, close_type, ts_event, ts_init);

        assert_eq!(
            format!("{instrument_close}"),
            "AAPL.XNAS,150.20,END_OF_SESSION,1"
        );
    }

    #[rstest]
    fn test_json_serialization() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let close_price = Price::from("150.20");
        let close_type = InstrumentCloseType::EndOfSession;
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let instrument_close =
            InstrumentClose::new(instrument_id, close_price, close_type, ts_event, ts_init);

        let serialized = instrument_close.as_json_bytes().unwrap();
        let deserialized = InstrumentClose::from_json_bytes(serialized.as_ref()).unwrap();

        assert_eq!(deserialized, instrument_close);
    }

    #[rstest]
    fn test_msgpack_serialization() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let close_price = Price::from("150.20");
        let close_type = InstrumentCloseType::EndOfSession;
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let instrument_close =
            InstrumentClose::new(instrument_id, close_price, close_type, ts_event, ts_init);

        let serialized = instrument_close.as_msgpack_bytes().unwrap();
        let deserialized = InstrumentClose::from_msgpack_bytes(serialized.as_ref()).unwrap();

        assert_eq!(deserialized, instrument_close);
    }
}
