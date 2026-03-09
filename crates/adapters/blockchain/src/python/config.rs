//! Python bindings for blockchain configuration.

use std::sync::Arc;

use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::defi::{Chain, DexType};
use nautilus_model::identifiers::{AccountId, TraderId, Venue};
use pyo3::prelude::*;
use url::Url;

use crate::config::{BlockchainDataClientConfig, BlockchainExecutionClientConfig, DexPoolFilters};

fn redact_url(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else {
        return String::from("<redacted>");
    };
    let Some(host) = parsed.host_str() else {
        return String::from("<redacted>");
    };
    let authority = parsed
        .port()
        .map_or_else(|| host.to_string(), |port| format!("{host}:{port}"));
    format!("{}://{authority}/...", parsed.scheme())
}

fn redact_optional_url(url: &Option<String>) -> Option<String> {
    url.as_ref().map(|value| redact_url(value))
}

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
            redact_url(&self.http_rpc_url),
            redact_optional_url(&self.wss_rpc_url),
            self.use_hypersync_for_live_data,
            self.from_block
        )
    }
}

#[pymethods]
impl BlockchainExecutionClientConfig {
    /// Creates a new `BlockchainExecutionClientConfig` instance.
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (
        trader_id,
        client_id,
        venue,
        chain,
        wallet_address,
        http_rpc_url,
        tokens=None,
        rpc_requests_per_second=None,
        *,
        wallet_extra_tokens=None,
        wallet_wnative_address=None,
        wallet_allowance_spenders=None,
        wallet_snapshot_ttl_secs=30,
        wallet_max_tokens_per_refresh=256,
        wallet_refresh_on_connect=true,
        multicall_max_batch_size=64,
        multicall_min_batch_size=4,
        signer_endpoint=None,
        signer_route="/sign/eth",
        signer_timeout_ms=5_000,
        signer_require_tls=true,
        signer_wallet_address=None,
        execution_router_address=None,
        execution_default_slippage_bps=100,
        execution_default_deadline_secs=120,
        execution_confirmations_required=1,
        execution_receipt_max_polls=60,
        execution_receipt_poll_interval_ms=1_000,
        execution_max_inflight_txs_per_wallet=1,
        execution_require_preapproved_allowance=true,
        execution_max_fee_per_gas=1_000_000_000,
        execution_max_priority_fee_per_gas=1_000_000_000,
        execution_journal_path=None,
        execution_unsupported_token_addresses=None
    ))]
    fn py_new(
        trader_id: TraderId,
        client_id: AccountId,
        venue: Venue,
        chain: Chain,
        wallet_address: String,
        http_rpc_url: String,
        tokens: Option<Vec<String>>,
        rpc_requests_per_second: Option<u32>,
        wallet_extra_tokens: Option<Vec<String>>,
        wallet_wnative_address: Option<String>,
        wallet_allowance_spenders: Option<Vec<String>>,
        wallet_snapshot_ttl_secs: u32,
        wallet_max_tokens_per_refresh: u32,
        wallet_refresh_on_connect: bool,
        multicall_max_batch_size: u32,
        multicall_min_batch_size: u32,
        signer_endpoint: Option<String>,
        signer_route: &str,
        signer_timeout_ms: u64,
        signer_require_tls: bool,
        signer_wallet_address: Option<String>,
        execution_router_address: Option<String>,
        execution_default_slippage_bps: u32,
        execution_default_deadline_secs: u64,
        execution_confirmations_required: u64,
        execution_receipt_max_polls: u32,
        execution_receipt_poll_interval_ms: u64,
        execution_max_inflight_txs_per_wallet: u32,
        execution_require_preapproved_allowance: bool,
        execution_max_fee_per_gas: u64,
        execution_max_priority_fee_per_gas: u64,
        execution_journal_path: Option<String>,
        execution_unsupported_token_addresses: Option<Vec<String>>,
    ) -> Self {
        let mut config = Self::new(
            trader_id,
            client_id,
            venue,
            chain,
            wallet_address,
            tokens,
            http_rpc_url,
            rpc_requests_per_second,
        );

        config.wallet_extra_tokens = wallet_extra_tokens.unwrap_or_default();
        config.wallet_wnative_address = wallet_wnative_address;
        config.wallet_allowance_spenders = wallet_allowance_spenders.unwrap_or_default();
        config.wallet_snapshot_ttl_secs = wallet_snapshot_ttl_secs;
        config.wallet_max_tokens_per_refresh = wallet_max_tokens_per_refresh;
        config.wallet_refresh_on_connect = wallet_refresh_on_connect;
        config.multicall_max_batch_size = multicall_max_batch_size;
        config.multicall_min_batch_size = multicall_min_batch_size;
        config.signer_endpoint = signer_endpoint;
        config.signer_route = signer_route.to_string();
        config.signer_timeout_ms = signer_timeout_ms;
        config.signer_require_tls = signer_require_tls;
        config.signer_wallet_address = signer_wallet_address;
        config.execution_router_address = execution_router_address;
        config.execution_default_slippage_bps = execution_default_slippage_bps;
        config.execution_default_deadline_secs = execution_default_deadline_secs;
        config.execution_confirmations_required = execution_confirmations_required;
        config.execution_receipt_max_polls = execution_receipt_max_polls;
        config.execution_receipt_poll_interval_ms = execution_receipt_poll_interval_ms;
        config.execution_max_inflight_txs_per_wallet = execution_max_inflight_txs_per_wallet;
        config.execution_require_preapproved_allowance = execution_require_preapproved_allowance;
        config.execution_max_fee_per_gas = execution_max_fee_per_gas;
        config.execution_max_priority_fee_per_gas = execution_max_priority_fee_per_gas;
        config.execution_journal_path = execution_journal_path;
        config.execution_unsupported_token_addresses =
            execution_unsupported_token_addresses.unwrap_or_default();

        config
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

    /// Returns the execution venue.
    #[getter]
    const fn venue(&self) -> Venue {
        self.venue
    }

    /// Returns the chain configuration.
    #[getter]
    fn chain(&self) -> Chain {
        self.chain.clone()
    }

    /// Returns the wallet address.
    #[getter]
    fn wallet_address(&self) -> String {
        self.wallet_address.clone()
    }

    /// Returns the token addresses to monitor.
    #[getter]
    fn tokens(&self) -> Option<Vec<String>> {
        self.tokens.clone()
    }

    /// Returns additional token addresses tracked in wallet snapshots.
    #[getter]
    fn wallet_extra_tokens(&self) -> Vec<String> {
        self.wallet_extra_tokens.clone()
    }

    /// Returns wrapped-native token address tracked in snapshots.
    #[getter]
    fn wallet_wnative_address(&self) -> Option<String> {
        self.wallet_wnative_address.clone()
    }

    /// Returns spender addresses tracked in allowance snapshots.
    #[getter]
    fn wallet_allowance_spenders(&self) -> Vec<String> {
        self.wallet_allowance_spenders.clone()
    }

    /// Returns the wallet snapshot TTL in seconds.
    #[getter]
    const fn wallet_snapshot_ttl_secs(&self) -> u32 {
        self.wallet_snapshot_ttl_secs
    }

    /// Returns the wallet refresh token cap.
    #[getter]
    const fn wallet_max_tokens_per_refresh(&self) -> u32 {
        self.wallet_max_tokens_per_refresh
    }

    /// Returns whether connect triggers wallet refresh.
    #[getter]
    const fn wallet_refresh_on_connect(&self) -> bool {
        self.wallet_refresh_on_connect
    }

    /// Returns multicall max batch size for wallet refresh.
    #[getter]
    const fn multicall_max_batch_size(&self) -> u32 {
        self.multicall_max_batch_size
    }

    /// Returns multicall minimum adaptive batch size.
    #[getter]
    const fn multicall_min_batch_size(&self) -> u32 {
        self.multicall_min_batch_size
    }

    /// Returns the HTTP RPC URL.
    #[getter]
    fn http_rpc_url(&self) -> String {
        self.http_rpc_url.clone()
    }

    /// Returns the RPC requests per second limit.
    #[getter]
    const fn rpc_requests_per_second(&self) -> Option<u32> {
        self.rpc_requests_per_second
    }

    /// Returns the remote signer endpoint URL.
    #[getter]
    fn signer_endpoint(&self) -> Option<String> {
        self.signer_endpoint.clone()
    }

    /// Returns the signer route path.
    #[getter]
    fn signer_route(&self) -> String {
        self.signer_route.clone()
    }

    /// Returns signer timeout in milliseconds.
    #[getter]
    const fn signer_timeout_ms(&self) -> u64 {
        self.signer_timeout_ms
    }

    /// Returns whether signer endpoint must be TLS.
    #[getter]
    const fn signer_require_tls(&self) -> bool {
        self.signer_require_tls
    }

    /// Returns optional signer wallet override.
    #[getter]
    fn signer_wallet_address(&self) -> Option<String> {
        self.signer_wallet_address.clone()
    }

    /// Returns optional router address override for execution.
    #[getter]
    fn execution_router_address(&self) -> Option<String> {
        self.execution_router_address.clone()
    }

    /// Returns default execution slippage (bps).
    #[getter]
    const fn execution_default_slippage_bps(&self) -> u32 {
        self.execution_default_slippage_bps
    }

    /// Returns default execution deadline (seconds).
    #[getter]
    const fn execution_default_deadline_secs(&self) -> u64 {
        self.execution_default_deadline_secs
    }

    /// Returns required confirmations before terminalization.
    #[getter]
    const fn execution_confirmations_required(&self) -> u64 {
        self.execution_confirmations_required
    }

    /// Returns max receipt polling attempts.
    #[getter]
    const fn execution_receipt_max_polls(&self) -> u32 {
        self.execution_receipt_max_polls
    }

    /// Returns receipt polling interval (ms).
    #[getter]
    const fn execution_receipt_poll_interval_ms(&self) -> u64 {
        self.execution_receipt_poll_interval_ms
    }

    /// Returns max in-flight transactions per wallet.
    #[getter]
    const fn execution_max_inflight_txs_per_wallet(&self) -> u32 {
        self.execution_max_inflight_txs_per_wallet
    }

    /// Returns whether preapproved allowance is required.
    #[getter]
    const fn execution_require_preapproved_allowance(&self) -> bool {
        self.execution_require_preapproved_allowance
    }

    /// Returns max fee per gas.
    #[getter]
    const fn execution_max_fee_per_gas(&self) -> u64 {
        self.execution_max_fee_per_gas
    }

    /// Returns max priority fee per gas.
    #[getter]
    const fn execution_max_priority_fee_per_gas(&self) -> u64 {
        self.execution_max_priority_fee_per_gas
    }

    /// Returns optional execution journal path.
    #[getter]
    fn execution_journal_path(&self) -> Option<String> {
        self.execution_journal_path.clone()
    }

    /// Returns explicit unsupported token addresses for execution decode.
    #[getter]
    fn execution_unsupported_token_addresses(&self) -> Vec<String> {
        self.execution_unsupported_token_addresses.clone()
    }

    /// Returns a string representation of the configuration.
    fn __repr__(&self) -> String {
        format!(
            "BlockchainExecutionClientConfig(trader_id={}, client_id={}, venue={}, chain={:?}, wallet_address={}, tokens={:?}, wallet_extra_tokens={:?}, wallet_wnative_address={:?}, wallet_allowance_spenders={:?}, wallet_snapshot_ttl_secs={}, wallet_max_tokens_per_refresh={}, wallet_refresh_on_connect={}, multicall_max_batch_size={}, multicall_min_batch_size={}, http_rpc_url={}, rpc_requests_per_second={:?})",
            self.trader_id,
            self.client_id,
            self.venue,
            self.chain.name,
            self.wallet_address,
            self.tokens,
            self.wallet_extra_tokens,
            self.wallet_wnative_address,
            self.wallet_allowance_spenders,
            self.wallet_snapshot_ttl_secs,
            self.wallet_max_tokens_per_refresh,
            self.wallet_refresh_on_connect,
            self.multicall_max_batch_size,
            self.multicall_min_batch_size,
            redact_url(&self.http_rpc_url),
            self.rpc_requests_per_second
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nautilus_model::{
        defi::chain::chains,
        identifiers::{AccountId, TraderId, Venue},
        stubs::TestDefault,
    };
    use pyo3::{
        Python,
        types::{PyDict, PyModule},
    };

    fn signer_route_from_python_config(signer_route: Option<&str>) -> String {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::new(py, "blockchain").expect("Module creation should succeed");
            module
                .add_class::<crate::config::BlockchainExecutionClientConfig>()
                .expect("Execution config class should be added to test module");

            let config_class = module
                .getattr("BlockchainExecutionClientConfig")
                .expect("Execution config class should be exposed");

            let kwargs = signer_route.map(|signer_route| {
                let kwargs = PyDict::new(py);
                kwargs
                    .set_item("signer_route", signer_route)
                    .expect("Should set signer_route kwarg");
                kwargs
            });

            let py_config = config_class
                .call(
                    (
                        TraderId::test_default(),
                        AccountId::test_default(),
                        Venue::new("Bsc:PancakeSwapV2"),
                        chains::BSC.clone(),
                        String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
                        String::from("https://bsc.example.com"),
                        Option::<Vec<String>>::None,
                        Option::<u32>::None,
                    ),
                    kwargs.as_ref(),
                )
                .expect("Execution config construction should succeed");

            py_config
                .getattr("signer_route")
                .expect("Config should expose signer_route")
                .extract()
                .expect("signer_route should extract")
        })
    }

    #[test]
    fn test_blockchain_execution_config_python_constructor_uses_default_signer_route() {
        let signer_route = signer_route_from_python_config(None);

        assert_eq!(signer_route, "/sign/eth");
    }

    #[test]
    fn test_blockchain_execution_config_python_constructor_preserves_custom_signer_route() {
        let signer_route = signer_route_from_python_config(Some("/sign/custom"));

        assert_eq!(signer_route, "/sign/custom");
    }

    #[test]
    fn test_blockchain_data_config_repr_redacts_rpc_urls() {
        let config = BlockchainDataClientConfig::new(
            Arc::new(chains::BSC.clone()),
            vec![DexType::PancakeSwapV2],
            "https://rpc-user:secret@bsc.example.com/v3/api-key?token=abc".to_string(),
            Some(10),
            Some(200),
            Some("wss://ws-user:secret@bsc.example.com/socket?token=abc".to_string()),
            true,
            Some(123),
            None,
            None,
        );

        let repr = config.__repr__();
        assert!(repr.contains("https://bsc.example.com/..."));
        assert!(repr.contains("Some(\"wss://bsc.example.com/...\")"));
        assert!(!repr.contains("secret"));
        assert!(!repr.contains("api-key"));
        assert!(!repr.contains("token=abc"));
    }

    #[test]
    fn test_blockchain_execution_config_repr_redacts_rpc_urls() {
        let config = BlockchainExecutionClientConfig::new(
            TraderId::test_default(),
            AccountId::test_default(),
            Venue::new("Bsc:PancakeSwapV2"),
            chains::BSC.clone(),
            String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
            None,
            "https://rpc-user:secret@bsc.example.com/v3/api-key?token=abc".to_string(),
            Some(10),
        );

        let repr = config.__repr__();
        assert!(repr.contains("https://bsc.example.com/..."));
        assert!(!repr.contains("secret"));
        assert!(!repr.contains("api-key"));
        assert!(!repr.contains("token=abc"));
    }

    #[test]
    fn test_blockchain_config_repr_redacts_at_signs_outside_userinfo() {
        let config = BlockchainExecutionClientConfig::new(
            TraderId::test_default(),
            AccountId::test_default(),
            Venue::new("Bsc:PancakeSwapV2"),
            chains::BSC.clone(),
            String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
            None,
            "https://rpc-user:secret@bsc.example.com/path?token=abc@LEAK".to_string(),
            Some(10),
        );

        let repr = config.__repr__();
        assert!(repr.contains("https://bsc.example.com/..."));
        assert!(!repr.contains("secret"));
        assert!(!repr.contains("token=abc"));
        assert!(!repr.contains("LEAK"));
    }
}
