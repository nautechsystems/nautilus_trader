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

//! Python bindings for Hyperliquid factory types.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::{
    config::HyperliquidExecClientConfig,
    factories::{
        HyperliquidDataClientFactory, HyperliquidExecFactoryConfig,
        HyperliquidExecutionClientFactory,
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidDataClientFactory {
    /// Factory for creating Hyperliquid data clients.
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &'static str {
        "HYPERLIQUID"
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidExecutionClientFactory {
    /// Factory for creating Hyperliquid execution clients.
    #[new]
    fn py_new() -> Self {
        Self
    }

    #[pyo3(name = "name")]
    fn py_name(&self) -> &'static str {
        "HYPERLIQUID"
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl HyperliquidExecFactoryConfig {
    /// Configuration for creating Hyperliquid execution clients via factory.
    ///
    /// This wraps `HyperliquidExecClientConfig` with the additional trader and account
    /// identifiers required by the `ExecutionClientCore`.
    #[new]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        config: HyperliquidExecClientConfig,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            config,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
