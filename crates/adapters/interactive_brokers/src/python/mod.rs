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

//! Python bindings from `pyo3`.

pub mod config;
pub mod conversion;

#[cfg(feature = "python")]
pub mod data;

#[cfg(feature = "python")]
pub mod execution;

#[cfg(feature = "gateway")]
#[cfg(feature = "python")]
pub mod gateway;

#[cfg(feature = "python")]
pub mod historical;

#[cfg(feature = "python")]
pub mod providers;

use pyo3::prelude::*;

/// Loaded as `nautilus_pyo3.interactive_brokers`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
#[allow(unused_variables)]
pub fn interactive_brokers(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::config::MarketDataType>()?;
    m.add_class::<crate::config::InteractiveBrokersDataClientConfig>()?;
    m.add_class::<crate::config::InteractiveBrokersExecClientConfig>()?;
    m.add_class::<crate::config::InteractiveBrokersInstrumentProviderConfig>()?;
    m.add_class::<crate::config::DockerizedIBGatewayConfig>()?;
    m.add_class::<crate::config::TradingMode>()?;
    m.add_class::<crate::data::InteractiveBrokersDataClient>()?;
    m.add_class::<crate::execution::InteractiveBrokersExecutionClient>()?;
    m.add_class::<crate::historical::HistoricalInteractiveBrokersClient>()?;
    m.add_class::<crate::providers::instruments::InteractiveBrokersInstrumentProvider>()?;

    #[cfg(feature = "gateway")]
    {
        m.add_class::<crate::gateway::dockerized::ContainerStatus>()?;
        m.add_class::<crate::gateway::dockerized::DockerizedIBGateway>()?;
    }

    Ok(())
}
