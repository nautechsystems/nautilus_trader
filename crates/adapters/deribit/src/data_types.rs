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

use nautilus_core::UnixNanos;
use nautilus_persistence_macros::custom_data;

/// Deribit volatility index (DVOL) update.
///
/// Emitted from the `deribit_volatility_index.{index_name}` WebSocket channel.
#[custom_data(pyo3)]
pub struct DeribitVolatilityIndex {
    /// The index identifier (for example `"btc_usd"` or `"eth_usd"`).
    pub index_name: String,
    /// The DVOL value for `index_name`.
    pub volatility: f64,
    /// UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

/// Registers Deribit custom data types for JSON and Arrow encoding.
///
/// Safe to call multiple times (idempotent via internal `Once` guards).
pub fn register_deribit_custom_data() {
    nautilus_serialization::ensure_custom_data_registered::<DeribitVolatilityIndex>();
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_register_deribit_custom_data_is_idempotent() {
        register_deribit_custom_data();
        register_deribit_custom_data();
    }
}
