// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python bindings for blockchain configuration.

use std::sync::Arc;

use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::defi::Chain;
use pyo3::prelude::*;

use crate::config::BlockchainDataClientConfig;

#[pymethods]
impl BlockchainDataClientConfig {
    /// Creates a new `BlockchainDataClientConfig` instance.
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (chain, dex_ids, http_rpc_url, rpc_requests_per_second=None, wss_rpc_url=None, use_hypersync_for_live_data=true, from_block=None, postgres_cache_database_config=None))]
    fn py_new(
        chain: &Chain,
        dex_ids: Vec<String>,
        http_rpc_url: String,
        rpc_requests_per_second: Option<u32>,
        wss_rpc_url: Option<String>,
        use_hypersync_for_live_data: bool,
        from_block: Option<u64>,
        postgres_cache_database_config: Option<PostgresConnectOptions>,
    ) -> Self {
        Self::new(
            Arc::new(chain.clone()),
            dex_ids,
            http_rpc_url,
            rpc_requests_per_second,
            wss_rpc_url,
            use_hypersync_for_live_data,
            from_block,
            postgres_cache_database_config,
        )
    }

    /// Returns the chain configuration.
    #[getter]
    fn chain(&self) -> Chain {
        (*self.chain).clone()
    }

    /// Returns the HTTP RPC URL.
    #[getter]
    fn http_rpc_url(&self) -> String {
        self.http_rpc_url.clone()
    }

    /// Returns the WebSocket RPC URL.
    #[getter]
    fn wss_rpc_url(&self) -> Option<String> {
        self.wss_rpc_url.clone()
    }

    /// Returns the RPC requests per second limit.
    #[getter]
    const fn rpc_requests_per_second(&self) -> Option<u32> {
        self.rpc_requests_per_second
    }

    /// Returns whether to use HyperSync for live data.
    #[getter]
    const fn use_hypersync_for_live_data(&self) -> bool {
        self.use_hypersync_for_live_data
    }

    /// Returns the starting block for sync.
    #[getter]
    #[allow(clippy::wrong_self_convention)]
    const fn from_block(&self) -> Option<u64> {
        self.from_block
    }

    /// Returns a string representation of the configuration.
    fn __repr__(&self) -> String {
        format!(
            "BlockchainDataClientConfig(chain={:?}, http_rpc_url={}, wss_rpc_url={:?}, use_hypersync_for_live_data={}, from_block={:?})",
            self.chain.name,
            self.http_rpc_url,
            self.wss_rpc_url,
            self.use_hypersync_for_live_data,
            self.from_block
        )
    }
}
