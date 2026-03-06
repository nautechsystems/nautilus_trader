//! Python bindings for blockchain configuration.

use std::sync::Arc;

use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::defi::{Chain, DexType};
use pyo3::prelude::*;

use crate::config::{BlockchainDataClientConfig, DexPoolFilters};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl DexPoolFilters {
    /// Creates a new `DexPoolFilters` instance.
    #[new]
    #[must_use]
    pub fn py_new(remove_pools_with_empty_erc20_fields: Option<bool>) -> Self {
        Self::new(remove_pools_with_empty_erc20_fields)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods(module = "nautilus_trader.adapters.blockchain")]
impl BlockchainDataClientConfig {
    /// Creates a new `BlockchainDataClientConfig` instance.
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (chain, dex_ids, http_rpc_url, rpc_requests_per_second=None, multicall_calls_per_rpc_request=None, wss_rpc_url=None, use_hypersync_for_live_data=true, from_block=None, pool_filters=None, postgres_cache_database_config=None))]
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
    ) -> Self {
        Self::new(
            Arc::new(chain.clone()),
            dex_ids,
            http_rpc_url,
            rpc_requests_per_second,
            multicall_calls_per_rpc_request,
            wss_rpc_url,
            use_hypersync_for_live_data,
            from_block,
            pool_filters,
            postgres_cache_database_config,
        )
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
