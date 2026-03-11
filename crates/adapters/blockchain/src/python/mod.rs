//! Python bindings from [PyO3](https://pyo3.rs).

pub mod config;
pub mod factories;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
#[cfg(feature = "hypersync")]
use nautilus_system::factories::DataClientFactory;
use nautilus_system::{
    factories::{ClientConfig, ExecutionClientFactory},
    get_global_pyo3_registry,
};
use pyo3::prelude::*;

#[pyfunction]
fn pancakeswap_v2_defaults_for_chain_id(chain_id: u32) -> PyResult<(String, String, String)> {
    match chain_id {
        // BSC mainnet: router, factory, wrapped native (WBNB).
        56 => Ok((
            "0x10ED43C718714eb63d5aA57B78B54704E256024E".to_string(),
            "0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73".to_string(),
            "0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c".to_string(),
        )),
        // BSC testnet: router, factory, wrapped native (WBNB).
        97 => Ok((
            "0xD99D1c33F9fC3444f8101754aBC46c52416550D1".to_string(),
            "0x6725F303b657a9451d8BA641348b6761A6CC7a17".to_string(),
            "0xae13d989dac2f0debff460ac112a837c89baa7cd".to_string(),
        )),
        _ => Err(to_pyvalue_err(format!(
            "No PancakeSwapV2 defaults configured for chain_id={chain_id}"
        ))),
    }
}

#[cfg(feature = "hypersync")]
/// Extractor function for `BlockchainDataClientFactory`.
fn extract_blockchain_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<crate::factories::BlockchainDataClientFactory>(py) {
        Ok(concrete_factory) => Ok(Box::new(concrete_factory)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BlockchainDataClientFactory: {e}"
        ))),
    }
}

/// Extractor function for `BlockchainExecutionClientFactory`.
fn extract_blockchain_execution_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<crate::factories::BlockchainExecutionClientFactory>(py) {
        Ok(concrete_factory) => Ok(Box::new(concrete_factory)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BlockchainExecutionClientFactory: {e}"
        ))),
    }
}

/// Extractor function for `BlockchainDataClientConfig`.
#[cfg(feature = "hypersync")]
fn extract_blockchain_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<crate::config::BlockchainDataClientConfig>(py) {
        Ok(concrete_config) => Ok(Box::new(concrete_config)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BlockchainDataClientConfig: {e}"
        ))),
    }
}

/// Extractor function for `BlockchainExecutionClientConfig`.
fn extract_blockchain_execution_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<crate::config::BlockchainExecutionClientConfig>(py) {
        Ok(concrete_config) => Ok(Box::new(concrete_config)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BlockchainExecutionClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.blockchain`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn blockchain(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::config::DexPoolFilters>()?;
    m.add_class::<crate::config::BlockchainDataClientConfig>()?;
    m.add_class::<crate::config::BlockchainExecutionClientConfig>()?;
    m.add_class::<crate::factories::BlockchainExecutionClientFactory>()?;
    #[cfg(feature = "hypersync")]
    m.add_class::<crate::factories::BlockchainDataClientFactory>()?;
    m.add_function(wrap_pyfunction!(pancakeswap_v2_defaults_for_chain_id, m)?)?;

    // Register extractors with the global registry
    let registry = get_global_pyo3_registry();

    #[cfg(feature = "hypersync")]
    if let Err(e) =
        registry.register_factory_extractor("BLOCKCHAIN".to_string(), extract_blockchain_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register blockchain factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_exec_factory_extractor(
        "BLOCKCHAIN".to_string(),
        extract_blockchain_execution_factory,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register blockchain execution factory extractor: {e}"
        )));
    }

    #[cfg(feature = "hypersync")]
    if let Err(e) = registry.register_config_extractor(
        "BlockchainDataClientConfig".to_string(),
        extract_blockchain_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register blockchain data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BlockchainExecutionClientConfig".to_string(),
        extract_blockchain_execution_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register blockchain execution config extractor: {e}"
        )));
    }

    Ok(())
}
