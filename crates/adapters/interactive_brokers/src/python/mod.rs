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

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::data::ensure_rust_extractor_registered;
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

#[cfg(feature = "gateway")]
use crate::gateway::dockerized::{ContainerStatus, DockerizedIBGateway};
use crate::{
    common::{consts::IB, enums::*},
    config::{
        DockerizedIBGatewayConfig, InteractiveBrokersDataClientConfig,
        InteractiveBrokersExecutionClientConfig, InteractiveBrokersInstrumentProviderConfig,
        MarketDataType, SymbologyMethod, TradingMode,
    },
    data_types::{InteractiveBrokersSubscriptionIdle, register_ib_custom_data},
    factories::{InteractiveBrokersDataClientFactory, InteractiveBrokersExecutionClientFactory},
    historical::HistoricalInteractiveBrokersClient,
    providers::instruments::InteractiveBrokersInstrumentProvider,
};

pub mod config;
pub mod conversion;
pub mod enums;
pub mod factories;
pub mod historical;
pub mod providers;

#[cfg(feature = "gateway")]
pub mod gateway;

#[expect(clippy::needless_pass_by_value)]
fn extract_interactive_brokers_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    factory
        .extract::<InteractiveBrokersDataClientFactory>(py)
        .map(|factory| Box::new(factory) as Box<dyn DataClientFactory>)
        .map_err(|e| {
            to_pyvalue_err(format!(
                "Failed to extract InteractiveBrokersDataClientFactory: {e}"
            ))
        })
}

#[expect(clippy::needless_pass_by_value)]
fn extract_interactive_brokers_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    factory
        .extract::<InteractiveBrokersExecutionClientFactory>(py)
        .map(|factory| Box::new(factory) as Box<dyn ExecutionClientFactory>)
        .map_err(|e| {
            to_pyvalue_err(format!(
                "Failed to extract InteractiveBrokersExecutionClientFactory: {e}"
            ))
        })
}

#[expect(clippy::needless_pass_by_value)]
fn extract_interactive_brokers_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    config
        .extract::<InteractiveBrokersDataClientConfig>(py)
        .map(|config| Box::new(config) as Box<dyn ClientConfig>)
        .map_err(|e| {
            to_pyvalue_err(format!(
                "Failed to extract InteractiveBrokersDataClientConfig: {e}"
            ))
        })
}

#[expect(clippy::needless_pass_by_value)]
fn extract_interactive_brokers_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    config
        .extract::<InteractiveBrokersExecutionClientConfig>(py)
        .map(|config| Box::new(config) as Box<dyn ClientConfig>)
        .map_err(|e| {
            to_pyvalue_err(format!(
                "Failed to extract InteractiveBrokersExecutionClientConfig: {e}"
            ))
        })
}

/// Exposed through `nautilus_trader.adapters.interactive_brokers`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn interactive_brokers(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<MarketDataType>()?;
    m.add_class::<IbAction>()?;
    m.add_class::<IbArticleType>()?;
    m.add_class::<IbAuctionStrategy>()?;
    m.add_class::<IbAuctionType>()?;
    m.add_class::<IbBondIdentifierKind>()?;
    m.add_class::<IbBuilderTimeInForce>()?;
    m.add_class::<IbComboLegOpenClose>()?;
    m.add_class::<IbConditionConjunction>()?;
    m.add_class::<IbConditionKind>()?;
    m.add_class::<IbExerciseAction>()?;
    m.add_class::<IbFundAssetType>()?;
    m.add_class::<IbFundDistributionPolicyIndicator>()?;
    m.add_class::<IbHistoricalBarSize>()?;
    m.add_class::<IbHistoricalTickType>()?;
    m.add_class::<IbHistoricalWhatToShow>()?;
    m.add_class::<IbLegAction>()?;
    m.add_class::<IbLiquidity>()?;
    m.add_class::<IbOcaType>()?;
    m.add_class::<IbOrderOpenClose>()?;
    m.add_class::<IbOrderOrigin>()?;
    m.add_class::<IbOptionRight>()?;
    m.add_class::<IbOrderStatus>()?;
    m.add_class::<IbOrderType>()?;
    m.add_class::<IbRealtimeBarSize>()?;
    m.add_class::<IbRealtimeWhatToShow>()?;
    m.add_class::<IbReferencePriceType>()?;
    m.add_class::<IbRiskAversion>()?;
    m.add_class::<IbRule80A>()?;
    m.add_class::<IbSecurityType>()?;
    m.add_class::<IbShortSaleSlot>()?;
    m.add_class::<IbTickType>()?;
    m.add_class::<IbTimeInForce>()?;
    m.add_class::<IbTradingHours>()?;
    m.add_class::<IbTriggerMethod>()?;
    m.add_class::<IbTwapStrategyType>()?;
    m.add_class::<IbVolatilityType>()?;
    m.add_class::<SymbologyMethod>()?;
    m.add_class::<InteractiveBrokersSubscriptionIdle>()?;
    register_ib_custom_data();
    let _ = ensure_rust_extractor_registered::<InteractiveBrokersSubscriptionIdle>();
    m.add_class::<InteractiveBrokersDataClientConfig>()?;
    m.add_class::<InteractiveBrokersExecutionClientConfig>()?;
    m.add_class::<InteractiveBrokersInstrumentProviderConfig>()?;
    m.add_class::<DockerizedIBGatewayConfig>()?;
    m.add_class::<TradingMode>()?;
    m.add_class::<InteractiveBrokersDataClientFactory>()?;
    m.add_class::<InteractiveBrokersExecutionClientFactory>()?;
    m.add_class::<HistoricalInteractiveBrokersClient>()?;
    m.add_class::<InteractiveBrokersInstrumentProvider>()?;

    #[cfg(feature = "gateway")]
    {
        m.add_class::<ContainerStatus>()?;
        m.add_class::<DockerizedIBGateway>()?;
    }

    let registry = get_global_pyo3_registry();

    if let Err(e) = registry
        .register_factory_extractor(IB.to_string(), extract_interactive_brokers_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Interactive Brokers data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor(IB.to_string(), extract_interactive_brokers_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Interactive Brokers exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "InteractiveBrokersDataClientConfig".to_string(),
        extract_interactive_brokers_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Interactive Brokers data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "InteractiveBrokersExecutionClientConfig".to_string(),
        extract_interactive_brokers_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Interactive Brokers exec config extractor: {e}"
        )));
    }

    Ok(())
}
