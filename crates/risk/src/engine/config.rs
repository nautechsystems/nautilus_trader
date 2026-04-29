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

//! Provides a configuration for `RiskEngine` instances.

use ahash::AHashMap;
use nautilus_common::throttler::RateLimit;
use nautilus_core::datetime::NANOSECONDS_IN_SECOND;
use nautilus_model::identifiers::InstrumentId;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuration for `RiskEngineConfig` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.risk", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.risk")
)]
#[derive(Debug, Clone, Deserialize, Serialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct RiskEngineConfig {
    #[builder(default)]
    pub bypass: bool,
    #[builder(default = RateLimit::new(100, NANOSECONDS_IN_SECOND))]
    pub max_order_submit: RateLimit,
    #[builder(default = RateLimit::new(100, NANOSECONDS_IN_SECOND))]
    pub max_order_modify: RateLimit,
    #[builder(default)]
    pub max_notional_per_order: AHashMap<InstrumentId, Decimal>,
    #[builder(default)]
    pub debug: bool,
}

impl Default for RiskEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}
