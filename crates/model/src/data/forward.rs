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

//! Forward price data type for derivatives instruments.

use nautilus_core::UnixNanos;
use rust_decimal::Decimal;

use crate::identifiers::InstrumentId;

/// Represents a forward/underlying price for a derivatives instrument.
///
/// This is a general derivatives concept used for ATM determination in option chains
/// and other forward-price dependent calculations.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct ForwardPrice {
    /// The instrument ID this forward price applies to.
    pub instrument_id: InstrumentId,
    /// The forward/underlying price.
    pub forward_price: Decimal,
    /// The underlying index name (e.g. "SYN.BTC-28MAR25"). Exchange-specific metadata.
    pub underlying_index: Option<String>,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl ForwardPrice {
    /// Creates a new [`ForwardPrice`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        forward_price: Decimal,
        underlying_index: Option<String>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            forward_price,
            underlying_index,
            ts_event,
            ts_init,
        }
    }
}
