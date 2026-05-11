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

use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::InstrumentId,
    types::{Price, Quantity},
};
use nautilus_persistence_macros::custom_data;
use nautilus_serialization::ensure_custom_data_registered;

/// CryptoHFTData open interest update.
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.cryptohftdata")
)]
#[custom_data(pyo3)]
pub struct CryptoHFTDataOpenInterest {
    pub instrument_id: InstrumentId,
    #[custom_data_field(json)]
    pub open_interest: Quantity,
    pub open_interest_value: String,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// CryptoHFTData liquidation event.
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.cryptohftdata")
)]
#[custom_data(pyo3)]
pub struct CryptoHFTDataLiquidation {
    pub instrument_id: InstrumentId,
    pub side: String,
    #[custom_data_field(json)]
    pub price: Price,
    #[custom_data_field(json)]
    pub quantity: Quantity,
    pub order_id: String,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

/// Registers CHD custom data types for catalog encoding and decoding.
pub fn register_cryptohftdata_custom_data() {
    ensure_custom_data_registered::<CryptoHFTDataOpenInterest>();
    ensure_custom_data_registered::<CryptoHFTDataLiquidation>();
}
