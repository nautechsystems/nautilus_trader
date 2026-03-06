//! Python bindings for blockchain factories.

use pyo3::prelude::*;

use crate::factories::{BlockchainDataClientFactory, BlockchainExecutionClientFactory};

#[pymethods]
impl BlockchainDataClientFactory {
    /// Creates a new `BlockchainDataClientFactory` instance.
    #[new]
    const fn py_new() -> Self {
        Self::new()
    }

    /// Returns the factory name.
    const fn name(&self) -> &'static str {
        "BLOCKCHAIN"
    }

    /// Returns the configuration type.
    const fn config_type(&self) -> &'static str {
        "BlockchainDataClientConfig"
    }

    /// Returns a string representation of the factory.
    fn __repr__(&self) -> String {
        format!("BlockchainDataClientFactory(name={})", self.name())
    }
}

#[pymethods]
impl BlockchainExecutionClientFactory {
    /// Creates a new `BlockchainExecutionClientFactory` instance.
    #[new]
    const fn py_new() -> Self {
        Self::new()
    }
}
