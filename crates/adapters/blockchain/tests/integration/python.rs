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

#![cfg(all(feature = "python", feature = "hypersync"))]

use std::{cell::RefCell, rc::Rc, sync::Arc};

use nautilus_blockchain::{
    config::{BlockchainDataClientConfig, BlockchainExecutionClientConfig, QuoteSpendLimit},
    constants::BLOCKCHAIN,
    factories::BlockchainDataClientFactory,
    python,
};
use nautilus_common::{
    cache::Cache, clock::TestClock, live::runner::replace_data_event_sender, messages::DataEvent,
};
use nautilus_model::{
    defi::{DexType, chain::chains},
    identifiers::{AccountId, ClientId},
};
use nautilus_network::{python as network_python, websocket::TransportBackend};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    Bound, IntoPyObjectExt, Py, Python,
    types::{PyAny, PyAnyMethods, PyDict, PyDictMethods, PyModule, PyTuple},
};
use rstest::rstest;

#[rstest]
fn test_blockchain_python_module_contract() {
    setup_data_event_sender();
    Python::initialize();

    Python::attach(|py| {
        let blockchain_module = register_blockchain_python_module(py);
        let network_module = register_network_python_module(py);
        assert_data_factory_extracts_from_python_object(py);
        assert_data_config_extracts_transport_backend_from_python_constructor(
            py,
            &blockchain_module,
            &network_module,
        );
        assert_execution_config_constructs_from_python(py, &blockchain_module);
        assert_execution_legacy_config_constructs_from_python(py, &blockchain_module);
    });
}

fn setup_data_event_sender() {
    let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
    replace_data_event_sender(sender);
}

fn register_blockchain_python_module(py: Python<'_>) -> Bound<'_, PyModule> {
    let module = PyModule::new(py, "blockchain").expect("Blockchain module should be created");
    python::blockchain(py, &module).expect("Blockchain Python module should register");
    module
}

fn register_network_python_module(py: Python<'_>) -> Bound<'_, PyModule> {
    let module = PyModule::new(py, "network").expect("Network module should be created");
    network_python::network(py, &module).expect("Network Python module should register");
    module
}

fn assert_data_factory_extracts_from_python_object(py: Python<'_>) {
    let factory = Py::new(py, BlockchainDataClientFactory::new())
        .expect("factory should convert to Python object")
        .into_any();
    let config = Py::new(
        py,
        BlockchainDataClientConfig::builder()
            .chain(Arc::new(chains::ETHEREUM.clone()))
            .http_rpc_url("https://eth-mainnet.example.com".into())
            .build(),
    )
    .expect("config should convert to Python object")
    .into_any();
    let registry = get_global_pyo3_registry();

    let extracted_factory = registry
        .extract_factory(py, factory)
        .expect("data factory should extract");
    let extracted_config = registry
        .extract_config(py, config)
        .expect("data config should extract");
    let blockchain_config = extracted_config
        .as_any()
        .downcast_ref::<BlockchainDataClientConfig>()
        .expect("data config should downcast");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let client = extracted_factory
        .create(
            "BLOCKCHAIN-DATA-EXTRACTED",
            extracted_config.as_ref(),
            cache.into(),
            clock,
        )
        .expect("extracted factory should create data client");

    assert_eq!(extracted_factory.name(), BLOCKCHAIN);
    assert_eq!(
        extracted_factory.config_type(),
        "BlockchainDataClientConfig"
    );
    assert_eq!(
        blockchain_config.http_rpc_url.expose_secret(),
        "https://eth-mainnet.example.com"
    );
    assert_eq!(
        client.client_id(),
        ClientId::from("BLOCKCHAIN-DATA-EXTRACTED")
    );
}

fn assert_execution_config_constructs_from_python(
    py: Python<'_>,
    blockchain_module: &Bound<'_, PyModule>,
) {
    const USERINFO_SECRET: &str = "python-execution-userinfo-secret";
    const PATH_SECRET: &str = "python-execution-path-secret";
    const QUERY_SECRET: &str = "python-execution-query-secret";
    let http_rpc_url = format!(
        "https://rpc-user:{USERINFO_SECRET}@rpc.example.com/{PATH_SECRET}?api_key={QUERY_SECRET}"
    );
    let config_type = blockchain_module
        .getattr("BlockchainExecutionClientConfig")
        .expect("BlockchainExecutionClientConfig should be available");
    let quote_spend_limit_type = blockchain_module
        .getattr("QuoteSpendLimit")
        .expect("QuoteSpendLimit should be available");
    let quote_spend_limit = quote_spend_limit_type
        .call1((
            "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
            "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
            "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
            6_u8,
            "1000000000",
        ))
        .expect("QuoteSpendLimit should construct from Python");

    let kwargs = PyDict::new(py);
    kwargs
        .set_item(
            "allowed_token_pairs",
            vec![
                (
                    "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
                    "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
                ),
                (
                    "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
                    "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
                ),
            ],
        )
        .expect("allowed_token_pairs kwarg should be set");
    kwargs
        .set_item("slippage_bps", 50_u32)
        .expect("slippage_bps kwarg should be set");
    kwargs
        .set_item("max_slippage_bps", 200_u32)
        .expect("max_slippage_bps kwarg should be set");
    kwargs
        .set_item("max_order_amount", 1_000_000_000_000_000_000_u64)
        .expect("max_order_amount kwarg should be set");
    kwargs
        .set_item("quote_spend_limits", vec![quote_spend_limit])
        .expect("quote_spend_limits kwarg should be set");
    kwargs
        .set_item("deadline_seconds", 300_u64)
        .expect("deadline_seconds kwarg should be set");
    kwargs
        .set_item("max_quote_age_blocks", 100_u64)
        .expect("max_quote_age_blocks kwarg should be set");
    kwargs
        .set_item("receipt_timeout_secs", 60_u64)
        .expect("receipt_timeout_secs kwarg should be set");

    let config = config_type
        .call(
            (
                AccountId::from("BLOCKCHAIN-001"),
                chains::ARBITRUM.clone(),
                "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
                http_rpc_url.clone(),
                "BLOCKCHAIN_PRIVATE_KEY",
                vec!["0xE592427A0AEce92De3Edee1F18E0157C05861564"],
                "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
                1_000_000_000_u64,
                2_000_u32,
                1_000_000_u64,
                2_000_u32,
            ),
            Some(&kwargs),
        )
        .expect("BlockchainExecutionClientConfig should construct from Python");

    let repr: String = config
        .repr()
        .expect("execution config repr should succeed")
        .extract()
        .expect("execution config repr should be a string");
    assert!(!config.hasattr("http_rpc_url").unwrap());

    let getter_value: String = config
        .getattr("signer_private_key_env")
        .expect("signer_private_key_env getter should exist")
        .extract()
        .expect("signer_private_key_env getter should return a string");
    assert_eq!(getter_value, "BLOCKCHAIN_PRIVATE_KEY");

    let getter_quote_spend_limits: Option<Vec<QuoteSpendLimit>> = config
        .getattr("quote_spend_limits")
        .expect("quote_spend_limits getter should exist")
        .extract()
        .expect("quote_spend_limits getter should return optional limits");
    assert_eq!(
        getter_quote_spend_limits,
        Some(vec![QuoteSpendLimit {
            token_in: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            token_out: "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
            spend_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            spend_token_decimals: 6,
            max_amount: "1000000000".to_string(),
        }]),
    );

    let getter_pairs: Option<Vec<(String, String)>> = config
        .getattr("allowed_token_pairs")
        .expect("allowed_token_pairs getter should exist")
        .extract()
        .expect("allowed_token_pairs getter should return optional pairs");
    assert_eq!(
        getter_pairs,
        Some(vec![
            (
                "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
                "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            ),
            (
                "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
                "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
            )
        ])
    );

    let extracted = config
        .extract::<BlockchainExecutionClientConfig>()
        .expect("execution config should extract");

    assert!(repr.contains("http_rpc_url=<redacted>"));
    assert!(!repr.contains(USERINFO_SECRET));
    assert!(!repr.contains(PATH_SECRET));
    assert!(!repr.contains(QUERY_SECRET));
    assert!(!repr.contains(&http_rpc_url));
    assert_eq!(extracted.chain.chain_id, 42161);
    assert_eq!(extracted.signer_private_key_env, "BLOCKCHAIN_PRIVATE_KEY");
    assert_eq!(
        extracted.router_addresses,
        vec!["0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string()]
    );
    assert_eq!(
        extracted.weth_address,
        "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"
    );
    assert!(!extracted.unlimited_approval);
    assert_eq!(extracted.max_fee_per_gas_wei, 1_000_000_000);
    assert_eq!(extracted.base_fee_buffer_bps, 2_000);
    assert_eq!(extracted.gas_limit, 1_000_000);
    assert_eq!(extracted.gas_buffer_bps, 2_000);
    assert_eq!(
        extracted.allowed_token_pairs,
        Some(vec![
            (
                "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
                "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            ),
            (
                "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
                "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
            )
        ])
    );
    assert_eq!(extracted.slippage_bps, Some(50));
    assert_eq!(extracted.max_slippage_bps, Some(200));
    assert_eq!(extracted.max_order_amount, Some(1_000_000_000_000_000_000));
    assert_eq!(extracted.quote_spend_limits, getter_quote_spend_limits);
    assert_eq!(extracted.deadline_seconds, Some(300));
    assert_eq!(extracted.max_quote_age_blocks, Some(100));
    assert_eq!(extracted.receipt_timeout_secs, Some(60));
    assert!(extracted.postgres_cache_database_config.is_none());
    assert_eq!(extracted.transport_backend, TransportBackend::default());
}

fn assert_execution_legacy_config_constructs_from_python(
    py: Python<'_>,
    blockchain_module: &Bound<'_, PyModule>,
) {
    let config_type = blockchain_module
        .getattr("BlockchainExecutionClientConfig")
        .expect("BlockchainExecutionClientConfig should be available");
    let args = PyTuple::new(
        py,
        [
            py_object(py, AccountId::from("BLOCKCHAIN-001")),
            py_object(py, chains::ARBITRUM.clone()),
            py_object(py, "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"),
            py_object(py, "https://rpc.example.com"),
            py_object(py, "BLOCKCHAIN_PRIVATE_KEY"),
            py_object(py, vec!["0xE592427A0AEce92De3Edee1F18E0157C05861564"]),
            py_object(py, "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            py_object(py, 1_000_000_000_u64),
            py_object(py, 2_000_u32),
            py_object(py, 1_000_000_u64),
            py_object(py, 2_000_u32),
            py_object(py, vec!["0x1111111111111111111111111111111111111111"]),
            py_object(py, 42_u32),
            py_object(py, true),
        ],
    )
    .expect("legacy execution config args should build");
    let config = config_type
        .call1(args)
        .expect("legacy BlockchainExecutionClientConfig should construct from Python");
    let extracted = config
        .extract::<BlockchainExecutionClientConfig>()
        .expect("legacy execution config should extract");

    assert_eq!(
        extracted.tokens,
        Some(vec![
            "0x1111111111111111111111111111111111111111".to_string()
        ])
    );
    assert_eq!(extracted.rpc_requests_per_second, Some(42));
    assert!(extracted.unlimited_approval);
    assert!(extracted.postgres_cache_database_config.is_none());
    assert_eq!(extracted.transport_backend, TransportBackend::default());
    assert!(extracted.allowed_token_pairs.is_none());
    assert!(extracted.quote_spend_limits.is_none());
    assert!(extracted.slippage_bps.is_none());
    assert!(extracted.max_slippage_bps.is_none());
    assert!(extracted.max_order_amount.is_none());
    assert!(extracted.deadline_seconds.is_none());
    assert!(extracted.max_quote_age_blocks.is_none());
    assert!(extracted.receipt_timeout_secs.is_none());
}

fn py_object<'py>(py: Python<'py>, value: impl IntoPyObjectExt<'py>) -> Py<PyAny> {
    value
        .into_py_any(py)
        .expect("value should convert to a Python object")
}

fn assert_data_config_extracts_transport_backend_from_python_constructor(
    py: Python<'_>,
    blockchain_module: &Bound<'_, PyModule>,
    network_module: &Bound<'_, PyModule>,
) {
    const HTTP_PATH_SECRET: &str = "python-data-http-path-secret";
    const WSS_QUERY_SECRET: &str = "python-data-wss-query-secret";
    let http_rpc_url = format!("https://rpc.example.com/{HTTP_PATH_SECRET}");
    let wss_rpc_url = format!("wss://rpc.example.com/ws?api_key={WSS_QUERY_SECRET}");
    let config_type = blockchain_module
        .getattr("BlockchainDataClientConfig")
        .expect("BlockchainDataClientConfig should be available");
    let transport_backend = network_module
        .getattr("TransportBackend")
        .expect("TransportBackend should be available")
        .getattr("TUNGSTENITE")
        .expect("TransportBackend.TUNGSTENITE should be available");
    let kwargs = PyDict::new(py);
    kwargs
        .set_item("transport_backend", transport_backend)
        .expect("transport_backend kwarg should be set");
    kwargs
        .set_item("wss_rpc_url", wss_rpc_url.clone())
        .expect("wss_rpc_url kwarg should be set");
    let config = config_type
        .call(
            (
                chains::ETHEREUM.clone(),
                vec![DexType::UniswapV3],
                http_rpc_url.clone(),
            ),
            Some(&kwargs),
        )
        .expect("BlockchainDataClientConfig should construct from Python");
    let repr: String = config
        .repr()
        .expect("data config repr should succeed")
        .extract()
        .expect("data config repr should be a string");
    let registry = get_global_pyo3_registry();
    let extracted_config = registry
        .extract_config(py, config.into())
        .expect("data config should extract");
    let blockchain_config = extracted_config
        .as_any()
        .downcast_ref::<BlockchainDataClientConfig>()
        .expect("data config should downcast");

    assert!(repr.contains("http_rpc_url=<redacted>"));
    assert!(repr.contains("wss_rpc_url=Some(\"<redacted>\")"));
    assert!(!repr.contains(HTTP_PATH_SECRET));
    assert!(!repr.contains(WSS_QUERY_SECRET));
    assert!(!repr.contains(&http_rpc_url));
    assert!(!repr.contains(&wss_rpc_url));
    assert_eq!(blockchain_config.http_rpc_url.expose_secret(), http_rpc_url);
    assert_eq!(
        blockchain_config
            .wss_rpc_url
            .as_ref()
            .map(|value| value.expose_secret()),
        Some(wss_rpc_url.as_str())
    );
    assert_eq!(
        blockchain_config.transport_backend,
        TransportBackend::Tungstenite,
    );
}
