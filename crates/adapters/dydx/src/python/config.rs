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

//! Python bindings for dYdX configuration.

use nautilus_model::identifiers::{AccountId, TraderId};
use pyo3::prelude::*;

use crate::{
    common::enums::DydxNetwork,
    config::{DydxDataClientConfig, DydxExecClientConfig},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DydxDataClientConfig {
    /// Configuration for the dYdX data client.
    #[new]
    #[pyo3(signature = (proxy_url=None, network=None))]
    fn py_new(proxy_url: Option<String>, network: Option<DydxNetwork>) -> Self {
        Self {
            network: network.unwrap_or_default(),
            proxy_url,
            ..Self::default()
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DydxExecClientConfig {
    /// Configuration for the dYdX execution client.
    #[new]
    #[pyo3(signature = (
        trader_id,
        account_id,
        proxy_url=None,
        network=None,
        private_key=None,
        wallet_address=None,
        subaccount_number=0,
    ))]
    fn py_new(
        trader_id: TraderId,
        account_id: AccountId,
        proxy_url: Option<String>,
        network: Option<DydxNetwork>,
        private_key: Option<String>,
        wallet_address: Option<String>,
        subaccount_number: u32,
    ) -> Self {
        Self {
            trader_id,
            account_id,
            network: network.unwrap_or_default(),
            private_key,
            wallet_address,
            subaccount_number,
            proxy_url,
            ..Self::default()
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }
}
