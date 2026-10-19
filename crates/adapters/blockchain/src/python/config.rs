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

use nautilus_core::{
    python::to_pyvalue_err,
    string::secret::{REDACTED, SecretString},
};
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{Chain, DexType},
    identifiers::AccountId,
};
use nautilus_network::websocket::TransportBackend;
use pyo3::prelude::*;

use crate::config::{
    BlockchainChainAnchorConfig, BlockchainDataClientConfig, BlockchainExecutionClientConfig,
    BlockchainProviderIdentity, BlockchainVerificationConfig, BlockchainVerificationProviderConfig,
    DexPoolFilters, QuoteSpendLimit,
};

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
impl QuoteSpendLimit {
    /// Defines the maximum quote-token spend for a directed BUY swap pair.
    #[new]
    #[must_use]
    fn py_new(
        token_in: String,
        token_out: String,
        spend_token: String,
        spend_token_decimals: u8,
        max_amount: String,
    ) -> Self {
        Self::builder()
            .token_in(token_in)
            .token_out(token_out)
            .spend_token(spend_token)
            .spend_token_decimals(spend_token_decimals)
            .max_amount(max_amount)
            .build()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl BlockchainProviderIdentity {
    /// Stable local identity for one RPC provider and its failure domains.
    #[new]
    #[must_use]
    fn py_new(provider_id: String, operator_id: String, failure_domain_ids: Vec<String>) -> Self {
        Self::builder()
            .provider_id(provider_id)
            .operator_id(operator_id)
            .failure_domain_ids(failure_domain_ids)
            .build()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl BlockchainVerificationProviderConfig {
    /// Configuration for one read-only verification RPC provider.
    #[new]
    #[must_use]
    fn py_new(identity: BlockchainProviderIdentity, http_rpc_url: String) -> Self {
        Self::builder()
            .identity(identity)
            .http_rpc_url(SecretString::from(http_rpc_url))
            .build()
    }

    fn __repr__(&self) -> String {
        format!(
            "BlockchainVerificationProviderConfig(identity={:?}, http_rpc_url={REDACTED})",
            self.identity
        )
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl BlockchainChainAnchorConfig {
    /// Locally trusted finalized chain checkpoint and freshness policy.
    #[new]
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    fn py_new(
        chain_id: u32,
        chain_name: String,
        checkpoint_height: u64,
        checkpoint_hash: String,
        checkpoint_timestamp: u64,
        max_head_skew_blocks: u64,
        max_head_age_secs: u64,
        max_future_drift_secs: u64,
    ) -> Self {
        Self::builder()
            .chain_id(chain_id)
            .chain_name(chain_name)
            .checkpoint_height(checkpoint_height)
            .checkpoint_hash(checkpoint_hash)
            .checkpoint_timestamp(checkpoint_timestamp)
            .max_head_skew_blocks(max_head_skew_blocks)
            .max_head_age_secs(max_head_age_secs)
            .max_future_drift_secs(max_future_drift_secs)
            .build()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl BlockchainVerificationConfig {
    /// Independent verification topology and reviewed local deployment identity.
    #[new]
    fn py_new(
        authoritative: BlockchainProviderIdentity,
        verifiers: Vec<BlockchainVerificationProviderConfig>,
        chain_anchor: BlockchainChainAnchorConfig,
        manifest_version: String,
        manifest_digest: String,
        deployment_manifest_json: String,
    ) -> PyResult<Self> {
        let deployment_manifest = serde_json::from_str(&deployment_manifest_json)
            .map_err(|_| to_pyvalue_err("Invalid deployment manifest JSON"))?;
        Ok(Self::builder()
            .authoritative(authoritative)
            .verifiers(verifiers)
            .chain_anchor(chain_anchor)
            .manifest_version(manifest_version)
            .manifest_digest(manifest_digest)
            .deployment_manifest(deployment_manifest)
            .build())
    }

    #[getter]
    fn deployment_manifest_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.deployment_manifest).map_err(to_pyvalue_err)
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
            .http_rpc_url(SecretString::from(http_rpc_url))
            .maybe_rpc_requests_per_second(rpc_requests_per_second)
            .maybe_multicall_calls_per_rpc_request(multicall_calls_per_rpc_request)
            .maybe_wss_rpc_url(wss_rpc_url.map(SecretString::from))
            .use_hypersync_for_live_data(use_hypersync_for_live_data)
            .maybe_from_block(from_block)
            .maybe_pool_filters(pool_filters)
            .maybe_postgres_cache_database_config(postgres_cache_database_config)
            .maybe_proxy_url(proxy_url.map(SecretString::from))
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
            "BlockchainDataClientConfig(chain={:?}, http_rpc_url={REDACTED}, wss_rpc_url={:?}, use_hypersync_for_live_data={}, from_block={:?})",
            self.chain.name,
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
    #[pyo3(signature = (client_id, chain, wallet_address, http_rpc_url, signer_private_key_env, router_addresses, weth_address, max_fee_per_gas_wei, base_fee_buffer_bps, gas_limit, gas_buffer_bps, tokens=None, rpc_requests_per_second=None, unlimited_approval=false, postgres_cache_database_config=None, transport_backend=None, *, allowed_token_pairs=None, quote_spend_limits=None, slippage_bps=None, max_slippage_bps=None, max_order_amount=None, deadline_seconds=None, max_quote_age_blocks=None, receipt_timeout_secs=None, payload_key_env=None, payload_key_retired_env=None, payload_deployment_id=None, verification=None))]
    fn py_new(
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
        allowed_token_pairs: Option<Vec<(String, String)>>,
        quote_spend_limits: Option<Vec<QuoteSpendLimit>>,
        slippage_bps: Option<u32>,
        max_slippage_bps: Option<u32>,
        max_order_amount: Option<u64>,
        deadline_seconds: Option<u64>,
        max_quote_age_blocks: Option<u64>,
        receipt_timeout_secs: Option<u64>,
        payload_key_env: Option<String>,
        payload_key_retired_env: Option<Vec<String>>,
        payload_deployment_id: Option<String>,
        verification: Option<BlockchainVerificationConfig>,
    ) -> Self {
        Self::builder()
            .client_id(client_id)
            .chain(chain.clone())
            .wallet_address(wallet_address)
            .http_rpc_url(SecretString::from(http_rpc_url))
            .signer_private_key_env(signer_private_key_env)
            .router_addresses(router_addresses)
            .weth_address(weth_address)
            .max_fee_per_gas_wei(max_fee_per_gas_wei)
            .base_fee_buffer_bps(base_fee_buffer_bps)
            .gas_limit(gas_limit)
            .gas_buffer_bps(gas_buffer_bps)
            .maybe_allowed_token_pairs(allowed_token_pairs)
            .maybe_quote_spend_limits(quote_spend_limits)
            .maybe_slippage_bps(slippage_bps)
            .maybe_max_slippage_bps(max_slippage_bps)
            .maybe_max_order_amount(max_order_amount)
            .maybe_deadline_seconds(deadline_seconds)
            .maybe_max_quote_age_blocks(max_quote_age_blocks)
            .maybe_receipt_timeout_secs(receipt_timeout_secs)
            .maybe_payload_key_env(payload_key_env)
            .payload_key_retired_env(payload_key_retired_env.unwrap_or_default())
            .maybe_payload_deployment_id(payload_deployment_id)
            .maybe_verification(verification)
            .maybe_tokens(tokens)
            .maybe_rpc_requests_per_second(rpc_requests_per_second)
            .unlimited_approval(unlimited_approval)
            .maybe_postgres_cache_database_config(postgres_cache_database_config)
            .transport_backend(transport_backend.unwrap_or_default())
            .build()
    }

    /// Returns the allowed (input token, output token) address pairs.
    #[getter]
    #[gen_stub(override_return_type(type_repr = "list[tuple[str, str]] | None",))]
    fn allowed_token_pairs(&self) -> Option<Vec<(String, String)>> {
        self.allowed_token_pairs.clone()
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
            "BlockchainExecutionClientConfig(chain={:?}, wallet_address={}, http_rpc_url={REDACTED})",
            self.chain.name, self.wallet_address
        )
    }
}
