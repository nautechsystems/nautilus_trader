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

//! Python bindings for blockchain configuration.

use std::sync::Arc;

use nautilus_core::string::secret::REDACTED;
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{Chain, DexType},
    identifiers::{AccountId, TraderId},
};
use nautilus_network::websocket::TransportBackend;
use pyo3::prelude::*;

use crate::config::{BlockchainDataClientConfig, BlockchainExecutionClientConfig, DexPoolFilters};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl DexPoolFilters {
    /// Defines filtering criteria for the DEX pool universe that the data client will operate on.
    #[new]
    #[must_use]
    pub fn py_new(remove_pools_with_empty_erc20_fields: Option<bool>) -> Self {
        Self::builder()
            .maybe_remove_pools_with_empty_erc20fields(remove_pools_with_empty_erc20_fields)
            .build()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl BlockchainDataClientConfig {
    /// Configuration for blockchain data clients.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (chain, dex_ids, http_rpc_url, rpc_requests_per_second=None, multicall_calls_per_rpc_request=None, wss_rpc_url=None, use_hypersync_for_live_data=true, from_block=None, pool_filters=None, postgres_cache_database_config=None, proxy_url=None, transport_backend=None))]
    fn py_new(
        #[gen_stub(
            override_type(
                type_repr = "nautilus_trader.model.Chain",
                imports = ("nautilus_trader.model",),
            ),
        )]
        chain: &Chain,
        #[gen_stub(
            override_type(
                type_repr = "typing.Sequence[nautilus_trader.model.DexType]",
                imports = ("typing", "nautilus_trader.model"),
            ),
        )]
        dex_ids: Vec<DexType>,
        http_rpc_url: String,
        rpc_requests_per_second: Option<u32>,
        multicall_calls_per_rpc_request: Option<u32>,
        wss_rpc_url: Option<String>,
        use_hypersync_for_live_data: bool,
        from_block: Option<u64>,
        pool_filters: Option<DexPoolFilters>,
        #[gen_stub(
            override_type(
                type_repr = "typing.Optional[nautilus_trader.infrastructure.PostgresConnectOptions]",
                imports = ("typing", "nautilus_trader.infrastructure"),
            ),
        )]
        postgres_cache_database_config: Option<PostgresConnectOptions>,
        proxy_url: Option<String>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        Self::builder()
            .chain(Arc::new(chain.clone()))
            .dex_ids(dex_ids)
            .http_rpc_url(http_rpc_url)
            .maybe_rpc_requests_per_second(rpc_requests_per_second)
            .maybe_multicall_calls_per_rpc_request(multicall_calls_per_rpc_request)
            .maybe_wss_rpc_url(wss_rpc_url)
            .use_hypersync_for_live_data(use_hypersync_for_live_data)
            .maybe_from_block(from_block)
            .maybe_pool_filters(pool_filters)
            .maybe_postgres_cache_database_config(postgres_cache_database_config)
            .maybe_proxy_url(proxy_url)
            .transport_backend(transport_backend.unwrap_or_default())
            .build()
    }

    /// Returns the chain configuration.
    #[getter]
    #[gen_stub(
        override_return_type(
            type_repr = "nautilus_trader.model.Chain",
            imports = ("nautilus_trader.model",),
        ),
    )]
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
    #[expect(clippy::wrong_self_convention)]
    const fn from_block(&self) -> Option<u64> {
        self.from_block
    }

    #[getter]
    const fn has_postgres_cache_database_config(&self) -> bool {
        self.postgres_cache_database_config.is_some()
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    /// Returns a string representation of the configuration.
    fn __repr__(&self) -> String {
        format!(
            "BlockchainDataClientConfig(chain={:?}, http_rpc_url={}, wss_rpc_url={:?}, use_hypersync_for_live_data={}, from_block={:?})",
            self.chain.name,
            REDACTED,
            self.wss_rpc_url.as_ref().map(|_| REDACTED),
            self.use_hypersync_for_live_data,
            self.from_block
        )
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl BlockchainExecutionClientConfig {
    /// Configuration for blockchain execution clients.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[pyo3(signature = (trader_id, client_id, chain, wallet_address, http_rpc_url, signer_private_key_env, router_addresses, weth_address, max_fee_per_gas_wei, base_fee_buffer_bps, gas_limit, gas_buffer_bps, tokens=None, rpc_requests_per_second=None, unlimited_approval=false, postgres_cache_database_config=None, transport_backend=None))]
    fn py_new(
        trader_id: TraderId,
        client_id: AccountId,
        #[gen_stub(
            override_type(
                type_repr = "nautilus_trader.model.Chain",
                imports = ("nautilus_trader.model",),
            ),
        )]
        chain: &Chain,
        wallet_address: String,
        http_rpc_url: String,
        signer_private_key_env: String,
        router_addresses: Vec<String>,
        weth_address: String,
        max_fee_per_gas_wei: u64,
        base_fee_buffer_bps: u32,
        gas_limit: u64,
        gas_buffer_bps: u32,
        tokens: Option<Vec<String>>,
        rpc_requests_per_second: Option<u32>,
        unlimited_approval: bool,
        #[gen_stub(
            override_type(
                type_repr = "typing.Optional[nautilus_trader.infrastructure.PostgresConnectOptions]",
                imports = ("typing", "nautilus_trader.infrastructure"),
            ),
        )]
        postgres_cache_database_config: Option<PostgresConnectOptions>,
        transport_backend: Option<TransportBackend>,
    ) -> Self {
        Self::builder()
            .trader_id(trader_id)
            .client_id(client_id)
            .chain(chain.clone())
            .wallet_address(wallet_address)
            .http_rpc_url(http_rpc_url)
            .signer_private_key_env(signer_private_key_env)
            .router_addresses(router_addresses)
            .weth_address(weth_address)
            .max_fee_per_gas_wei(max_fee_per_gas_wei)
            .base_fee_buffer_bps(base_fee_buffer_bps)
            .gas_limit(gas_limit)
            .gas_buffer_bps(gas_buffer_bps)
            .maybe_tokens(tokens)
            .maybe_rpc_requests_per_second(rpc_requests_per_second)
            .unlimited_approval(unlimited_approval)
            .maybe_postgres_cache_database_config(postgres_cache_database_config)
            .transport_backend(transport_backend.unwrap_or_default())
            .build()
    }

    /// Returns the trader ID.
    #[getter]
    const fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    /// Returns the account ID.
    #[getter]
    const fn client_id(&self) -> AccountId {
        self.client_id
    }

    /// Returns the chain configuration.
    #[getter]
    #[gen_stub(
        override_return_type(
            type_repr = "nautilus_trader.model.Chain",
            imports = ("nautilus_trader.model",),
        ),
    )]
    fn chain(&self) -> Chain {
        self.chain.clone()
    }

    /// Returns the RPC requests per second limit.
    #[getter]
    const fn rpc_requests_per_second(&self) -> Option<u32> {
        self.rpc_requests_per_second
    }

    #[getter]
    const fn has_postgres_cache_database_config(&self) -> bool {
        self.postgres_cache_database_config.is_some()
    }

    /// Returns a string representation of the configuration.
    fn __repr__(&self) -> String {
        format!(
            "BlockchainExecutionClientConfig(chain={:?}, wallet_address={}, http_rpc_url={})",
            self.chain.name, self.wallet_address, REDACTED
        )
    }
}
