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

//! An `InstrumentStatus` data type representing a change in an instrument market status.

use std::{collections::HashMap, fmt::Display, hash::Hash};

use derive_builder::Builder;
use nautilus_core::{UnixNanos, serialization::Serializable};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::GetTsInit;
use crate::{enums::MarketStatusAction, identifiers::InstrumentId};

/// Represents an event that indicates a change in an instrument market status.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Builder)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct InstrumentStatus {
    /// The instrument ID for the status change.
    pub instrument_id: InstrumentId,
    /// The instrument market status action.
    pub action: MarketStatusAction,
    /// UNIX timestamp (nanoseconds) when the status event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the struct was initialized.
    pub ts_init: UnixNanos,
    /// Additional details about the cause of the status change.
    pub reason: Option<Ustr>,
    /// Further information about the status change (if provided).
    pub trading_event: Option<Ustr>,
    /// The state of trading in the instrument.
    pub is_trading: Option<bool>,
    /// The state of quoting in the instrument.
    pub is_quoting: Option<bool>,
    /// The state of short sell restrictions for the instrument (if applicable).
    pub is_short_sell_restricted: Option<bool>,
}

impl InstrumentStatus {
    /// Creates a new [`InstrumentStatus`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        action: MarketStatusAction,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        reason: Option<Ustr>,
        trading_event: Option<Ustr>,
        is_trading: Option<bool>,
        is_quoting: Option<bool>,
        is_short_sell_restricted: Option<bool>,
    ) -> Self {
        Self {
            instrument_id,
            action,
            ts_event,
            ts_init,
            reason,
            trading_event,
            is_trading,
            is_quoting,
            is_short_sell_restricted,
        }
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[must_use]
    pub fn get_metadata(instrument_id: &InstrumentId) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata
    }
}

// TODO: Revisit this
impl Display for InstrumentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.instrument_id, self.action, self.ts_event, self.ts_init,
        )
    }
}

impl Serializable for InstrumentStatus {}

impl GetTsInit for InstrumentStatus {
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::serialization::Serializable;
    use rstest::rstest;

    use super::*;
    use crate::data::stubs::stub_instrument_status;

    #[rstest]
    fn test_to_string(stub_instrument_status: InstrumentStatus) {
        assert_eq!(stub_instrument_status.to_string(), "MSFT.XNAS,TRADING,1,2");
    }

    #[rstest]
    fn test_json_serialization(stub_instrument_status: InstrumentStatus) {
        let serialized = stub_instrument_status.as_json_bytes().unwrap();
        let deserialized = InstrumentStatus::from_json_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, stub_instrument_status);
    }

    #[rstest]
    fn test_msgpack_serialization(stub_instrument_status: InstrumentStatus) {
        let serialized = stub_instrument_status.as_msgpack_bytes().unwrap();
        let deserialized = InstrumentStatus::from_msgpack_bytes(serialized.as_ref()).unwrap();
        assert_eq!(deserialized, stub_instrument_status);
    }
}
