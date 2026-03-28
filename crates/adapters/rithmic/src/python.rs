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

//! PyO3 bindings for Python interoperability.
//!
//! This module exposes Rust functionality to Python via PyO3,
//! enabling integration with NautilusTrader's Python layer.
//!
//! # Architecture
//!
//! The Python bindings follow a gateway-centric architecture:
//!
//! 1. `RithmicGateway` - Central connection manager for all Rithmic plants
//! 2. `RithmicDataClient` - Market data subscriptions (requires connected gateway)
//! 3. `RithmicExecutionClient` - Order management (requires connected gateway)
//!
//! For NautilusTrader integration, use the high-level Python classes:
//! - `RithmicLiveDataClient` - from `nautilus_trader.adapters.rithmic.data`
//! - `RithmicLiveExecutionClient` - from `nautilus_trader.adapters.rithmic.execution`
//!
//! These classes handle gateway lifecycle and async operations internally.

#[cfg(feature = "python")]
mod config;
#[cfg(feature = "python")]
mod data;
#[cfg(feature = "python")]
mod enums;
#[cfg(feature = "python")]
mod events;
#[cfg(feature = "python")]
mod execution;
#[cfg(feature = "python")]
mod gateway;
#[cfg(feature = "python")]
mod instruments;

#[cfg(feature = "python")]
use pyo3::prelude::*;

/// Registers the Rithmic submodule for `nautilus_trader.core.nautilus_pyo3`.
#[cfg(feature = "python")]
#[pymodule]
pub fn rithmic(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register submodules
    config::register(m)?;
    enums::register(m)?;
    events::register(m)?;
    gateway::register(m)?;
    data::register(m)?;
    execution::register(m)?;
    instruments::register(m)?;

    Ok(())
}
