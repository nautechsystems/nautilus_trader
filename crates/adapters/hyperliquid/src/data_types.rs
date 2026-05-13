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

use std::collections::HashMap;

use nautilus_core::UnixNanos;
use nautilus_model::{identifiers::InstrumentId, types::Price};
use nautilus_persistence_macros::custom_data;

/// Hyperliquid all mid prices snapshot from the `allMids` WebSocket channel.
#[custom_data(pyo3)]
pub struct HyperliquidAllMids {
    /// Mapping of instrument ID to mid price for all tradable coins.
    #[custom_data_field(json)]
    pub mids: HashMap<InstrumentId, Price>,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Registers Hyperliquid custom data types for JSON and Arrow serialization.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_hyperliquid_custom_data() {
    nautilus_serialization::ensure_custom_data_registered::<HyperliquidAllMids>();
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_register_hyperliquid_custom_data_is_idempotent() {
        register_hyperliquid_custom_data();
        register_hyperliquid_custom_data();
    }
}
