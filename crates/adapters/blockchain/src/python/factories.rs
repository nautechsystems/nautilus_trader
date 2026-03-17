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

//! Python bindings for blockchain factories.

use pyo3::prelude::*;

use crate::factories::{BlockchainDataClientFactory, BlockchainExecutionClientFactory};

#[pymethods]
impl BlockchainDataClientFactory {
    /// Factory for creating blockchain data clients.
    ///
    /// This factory creates `BlockchainDataClient` instances configured for different blockchain networks
    /// (Ethereum, Arbitrum, Base, Polygon) with appropriate RPC and HyperSync configurations.
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
    /// Factory for creating blockchain execution clients.
    #[new]
    const fn py_new() -> Self {
        Self::new()
    }
}
