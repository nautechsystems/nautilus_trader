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
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::consts::IB,
    config::{InteractiveBrokersDataClientConfig, InteractiveBrokersExecClientConfig},
    factories::{InteractiveBrokersDataClientFactory, InteractiveBrokersExecutionClientFactory},
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
    match factory.extract::<InteractiveBrokersDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract InteractiveBrokersDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_interactive_brokers_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<InteractiveBrokersExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract InteractiveBrokersExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_interactive_brokers_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<InteractiveBrokersDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract InteractiveBrokersDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_interactive_brokers_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<InteractiveBrokersExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract InteractiveBrokersExecClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.interactive_brokers`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn interactive_brokers(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::config::MarketDataType>()?;
    m.add_class::<crate::common::enums::IbAccountSummaryEvent>()?;
    m.add_class::<crate::common::enums::IbAccountUpdateEvent>()?;
    m.add_class::<crate::common::enums::IbAccountUpdateMultiEvent>()?;
    m.add_class::<crate::common::enums::IbAction>()?;
    m.add_class::<crate::common::enums::IbArticleType>()?;
    m.add_class::<crate::common::enums::IbAuctionStrategy>()?;
    m.add_class::<crate::common::enums::IbAuctionType>()?;
    m.add_class::<crate::common::enums::IbBondIdentifierKind>()?;
    m.add_class::<crate::common::enums::IbBuilderTimeInForce>()?;
    m.add_class::<crate::common::enums::IbCancelOrderEvent>()?;
    m.add_class::<crate::common::enums::IbComboLegOpenClose>()?;
    m.add_class::<crate::common::enums::IbConditionConjunction>()?;
    m.add_class::<crate::common::enums::IbConditionKind>()?;
    m.add_class::<crate::common::enums::IbExecutionsEvent>()?;
    m.add_class::<crate::common::enums::IbExerciseAction>()?;
    m.add_class::<crate::common::enums::IbExerciseOptionsEvent>()?;
    m.add_class::<crate::common::enums::IbFundAssetType>()?;
    m.add_class::<crate::common::enums::IbFundDistributionPolicyIndicator>()?;
    m.add_class::<crate::common::enums::IbHistoricalBarSize>()?;
    m.add_class::<crate::common::enums::IbHistoricalBarUpdateEvent>()?;
    m.add_class::<crate::common::enums::IbHistoricalTickType>()?;
    m.add_class::<crate::common::enums::IbHistoricalWhatToShow>()?;
    m.add_class::<crate::common::enums::IbLegAction>()?;
    m.add_class::<crate::common::enums::IbLiquidity>()?;
    m.add_class::<crate::common::enums::IbMarketDepthEvent>()?;
    m.add_class::<crate::common::enums::IbOcaType>()?;
    m.add_class::<crate::common::enums::IbOrderOpenClose>()?;
    m.add_class::<crate::common::enums::IbOrderOrigin>()?;
    m.add_class::<crate::common::enums::IbOptionRight>()?;
    m.add_class::<crate::common::enums::IbOrderStatus>()?;
    m.add_class::<crate::common::enums::IbOrderType>()?;
    m.add_class::<crate::common::enums::IbOrderUpdateEvent>()?;
    m.add_class::<crate::common::enums::IbOrdersEvent>()?;
    m.add_class::<crate::common::enums::IbPlaceOrderEvent>()?;
    m.add_class::<crate::common::enums::IbPositionUpdateEvent>()?;
    m.add_class::<crate::common::enums::IbPositionUpdateMultiEvent>()?;
    m.add_class::<crate::common::enums::IbRealtimeBarSize>()?;
    m.add_class::<crate::common::enums::IbRealtimeWhatToShow>()?;
    m.add_class::<crate::common::enums::IbReferencePriceType>()?;
    m.add_class::<crate::common::enums::IbRiskAversion>()?;
    m.add_class::<crate::common::enums::IbRule80A>()?;
    m.add_class::<crate::common::enums::IbSecurityType>()?;
    m.add_class::<crate::common::enums::IbShortSaleSlot>()?;
    m.add_class::<crate::common::enums::IbTickEvent>()?;
    m.add_class::<crate::common::enums::IbTickType>()?;
    m.add_class::<crate::common::enums::IbTimeInForce>()?;
    m.add_class::<crate::common::enums::IbTradingHours>()?;
    m.add_class::<crate::common::enums::IbTriggerMethod>()?;
    m.add_class::<crate::common::enums::IbTwapStrategyType>()?;
    m.add_class::<crate::common::enums::IbVolatilityType>()?;
    m.add_class::<crate::config::SymbologyMethod>()?;
    m.add_class::<crate::error::ErrorCategory>()?;
    m.add_class::<crate::error::InteractiveBrokersErrorKind>()?;
    m.add_class::<crate::config::InteractiveBrokersDataClientConfig>()?;
    m.add_class::<crate::config::InteractiveBrokersExecClientConfig>()?;
    m.add_class::<crate::config::InteractiveBrokersInstrumentProviderConfig>()?;
    m.add_class::<crate::config::DockerizedIBGatewayConfig>()?;
    m.add_class::<crate::config::TradingMode>()?;
    m.add_class::<crate::factories::InteractiveBrokersDataClientFactory>()?;
    m.add_class::<crate::factories::InteractiveBrokersExecutionClientFactory>()?;
    m.add_class::<crate::historical::HistoricalInteractiveBrokersClient>()?;
    m.add_class::<crate::providers::instruments::InteractiveBrokersInstrumentProvider>()?;

    #[cfg(feature = "gateway")]
    {
        m.add_class::<crate::gateway::dockerized::ContainerStatus>()?;
        m.add_class::<crate::gateway::dockerized::DockerizedIBGateway>()?;
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
        "InteractiveBrokersExecClientConfig".to_string(),
        extract_interactive_brokers_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Interactive Brokers exec config extractor: {e}"
        )));
    }

    Ok(())
}
