// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under the
//  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND,
//  either express or implied. See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Shared harness for the pinned-block Anvil fork integration suites.
//!
//! Forks Arbitrum at a fixed block with a local Anvil process against localhost only. The
//! fork-source RPC only reads chain state; signed transactions never leave localhost.

use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use alloy::primitives::{Address, B256, U256, address, keccak256};
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::post,
};
use nautilus_blockchain::{
    config::{
        BlockchainCallEdgeManifest, BlockchainChainAnchorConfig, BlockchainContractManifest,
        BlockchainContractProbe, BlockchainContractRole, BlockchainDeploymentManifest,
        BlockchainPoolManifest, BlockchainProviderIdentity, BlockchainProxyManifest,
        BlockchainTokenManifest, BlockchainVerificationConfig,
        BlockchainVerificationProviderConfig,
    },
    contracts::uniswap_v3_pool::{FeeProtocolEncoding, UniswapV3PoolContract},
    exchanges::arbitrum::UNISWAP_V3,
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_core::{UnixNanos, hex};
use nautilus_model::defi::{
    Pool, PoolIdentifier, PoolProfiler, Token,
    chain::chains,
    data::block::{BLOCK_SCOPED_SNAPSHOT_INDEX, BlockPosition},
    pool_analysis::{
        position::PoolPosition,
        snapshot::{PoolAnalytics, PoolSnapshot},
    },
    tick_map::tick::PoolTick,
};
use nautilus_network::http::{HttpClient, Method};
use sqlx::PgPool;

/// Arbitrum block the fork is pinned to.
pub(crate) const FORK_BLOCK: u64 = 489_000_000;
/// Arbitrum chain ID.
pub(crate) const CHAIN_ID: u64 = 42161;
pub(crate) const SIGNER_ENV: &str = "BLOCKCHAIN_FORK_TEST_PRIVATE_KEY";
pub(crate) const PAYLOAD_KEY_ENV: &str = "BLOCKCHAIN_FORK_TEST_PAYLOAD_KEY";
pub(crate) const PAYLOAD_KEY_HEX: &str =
    "5f573818412f4c7c25d86a4f8d719a4f972e3c028634c95ab9bb49c439ec2198";
pub(crate) const PAYLOAD_DEPLOYMENT_ID: &str = "blockchain-fork-tests";
pub(crate) const ROUTER: &str = "0xE592427A0AEce92De3Edee1F18E0157C05861564";
pub(crate) const FACTORY: &str = "0x1F98431c8aD98523631AE4a59f267346ea31F984";
pub(crate) const WETH: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";
const WETH_IMPLEMENTATION: &str = "0x8b194bEae1d3e0788A1a35173978001ACDFba668";
const EIP1967_IMPLEMENTATION_SLOT: &str =
    "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc";
pub(crate) const USDC: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";
const USDC_IMPLEMENTATION: &str = "0x86e721b43d4ecfa71119dd38c0f938a75fdb57b3";
const ZEPPELINOS_IMPLEMENTATION_SLOT: &str =
    "0x7050c9e0f4ca769c69bd3a8ef740bc37934f8e2c036e5a723fd8ee048ed3f8c3";
pub(crate) const QUOTE: &str = "0x61fFE014bA17989E743c5F6cB21bF9697530B21e";
pub(crate) const POOL: &str = "0xC6962004f452bE9203591991D15f6b388e09E8D0";
pub(crate) const FUND_AMOUNT_WEI: u128 = 100_000_000_000_000_000_000;
pub(crate) const WRAP_AMOUNT_WEI: u128 = 1_000_000_000_000_000;
pub(crate) const SWAP_AMOUNT: &str = "0.001";
pub(crate) const SLIPPAGE_BPS: u32 = 50;
pub(crate) const ANVIL_READY_TIMEOUT: Duration = Duration::from_secs(120);
const TRACE_PROBE_ACCOUNT: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const TRACE_PROBE_REQUEST_HASH: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000000";

pub(crate) struct AnvilProcess(Child);

impl Drop for AnvilProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Startup information parsed from the Anvil stdout banner.
pub(crate) struct AnvilStartup {
    pub port: u16,
    pub version: String,
}

#[derive(Clone)]
struct RpcProxyState {
    upstream: String,
    trace_probe_hash: String,
    allow_broadcast: bool,
    requests: Arc<AtomicU64>,
    broadcasts: Arc<AtomicU64>,
}

struct RpcProxy {
    url: String,
    requests: Arc<AtomicU64>,
    broadcasts: Arc<AtomicU64>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RpcProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Three distinct local RPC origins sharing one deterministic Anvil fixture.
///
/// The verifier origins enforce the production read-only boundary in the test transport. Their
/// distinct identities model the configured topology; they do not claim that one Anvil process is
/// operationally independent.
pub(crate) struct ExecutionRpcTopology {
    authoritative: RpcProxy,
    verifiers: [RpcProxy; 2],
    verification: BlockchainVerificationConfig,
}

impl ExecutionRpcTopology {
    pub(crate) fn authoritative_url(&self) -> String {
        self.authoritative.url.clone()
    }

    pub(crate) fn verification(&self) -> BlockchainVerificationConfig {
        self.verification.clone()
    }

    pub(crate) fn assert_broadcast_isolation(&self) {
        assert!(
            self.authoritative.requests.load(Ordering::Relaxed) > 0,
            "authoritative proxy received no RPC requests"
        );
        assert!(
            self.authoritative.broadcasts.load(Ordering::Relaxed) > 0,
            "authoritative proxy received no broadcast"
        );

        for verifier in &self.verifiers {
            assert!(
                verifier.requests.load(Ordering::Relaxed) > 0,
                "verification proxy received no RPC requests"
            );
            assert_eq!(
                verifier.broadcasts.load(Ordering::Relaxed),
                0,
                "verification proxy received a broadcast attempt"
            );
        }
    }
}

pub(crate) async fn start_execution_rpc_topology(anvil_url: &str) -> ExecutionRpcTopology {
    let trace_probe_hash = create_trace_probe_transaction(anvil_url).await;
    let authoritative = start_rpc_proxy(anvil_url, &trace_probe_hash, true).await;
    let verifier_a = start_rpc_proxy(anvil_url, &trace_probe_hash, false).await;
    let verifier_b = start_rpc_proxy(anvil_url, &trace_probe_hash, false).await;
    let verification =
        fork_verification_config(anvil_url, [&verifier_a.url, &verifier_b.url]).await;

    ExecutionRpcTopology {
        authoritative,
        verifiers: [verifier_a, verifier_b],
        verification,
    }
}

async fn start_rpc_proxy(
    upstream: &str,
    trace_probe_hash: &B256,
    allow_broadcast: bool,
) -> RpcProxy {
    let requests = Arc::new(AtomicU64::new(0));
    let broadcasts = Arc::new(AtomicU64::new(0));
    let state = RpcProxyState {
        upstream: upstream.to_string(),
        trace_probe_hash: trace_probe_hash.to_string(),
        allow_broadcast,
        requests: requests.clone(),
        broadcasts: broadcasts.clone(),
    };
    let app = Router::new().route("/", post(proxy_rpc)).with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    RpcProxy {
        url: format!("http://{address}"),
        requests,
        broadcasts,
        task,
    }
}

async fn proxy_rpc(State(state): State<RpcProxyState>, body: Bytes) -> Response {
    state.requests.fetch_add(1, Ordering::Relaxed);
    let mut request = match serde_json::from_slice::<serde_json::Value>(&body) {
        Ok(request) => request,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };

    if request["method"] == "eth_sendRawTransaction" {
        state.broadcasts.fetch_add(1, Ordering::Relaxed);
        if !state.allow_broadcast {
            return (
                StatusCode::FORBIDDEN,
                [(CONTENT_TYPE, "application/json")],
                serde_json::to_vec(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {"code": -32601, "message": "read-only endpoint"}
                }))
                .unwrap(),
            )
                .into_response();
        }
    }

    if request["method"] == "debug_traceTransaction"
        && request["params"][0].as_str() == Some(TRACE_PROBE_REQUEST_HASH)
    {
        request["params"][0] = serde_json::Value::String(state.trace_probe_hash.clone());
    }
    let body = serde_json::to_vec(&request).unwrap();

    let client = HttpClient::builder().timeout_secs(120).build().unwrap();
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());

    match client
        .request(
            Method::POST,
            state.upstream,
            None,
            Some(headers),
            Some(body),
            Some(120),
            None,
        )
        .await
    {
        Ok(response) => (
            StatusCode::OK,
            [(CONTENT_TYPE, "application/json")],
            response.body,
        )
            .into_response(),
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

async fn create_trace_probe_transaction(anvil_url: &str) -> B256 {
    let response = anvil_rpc(
        anvil_url,
        "eth_sendTransaction",
        serde_json::json!([{
            "from": TRACE_PROBE_ACCOUNT,
            "to": TRACE_PROBE_ACCOUNT,
            "value": "0x0"
        }]),
    )
    .await;
    assert!(
        response.get("error").is_none(),
        "failed to submit local trace probe transaction: {response}"
    );
    let tx_hash = response["result"]
        .as_str()
        .unwrap()
        .parse::<B256>()
        .unwrap();
    anvil_mine(anvil_url, 1).await;
    let trace = anvil_rpc(
        anvil_url,
        "debug_traceTransaction",
        serde_json::json!([
            tx_hash,
            {
                "tracer": "callTracer",
                "tracerConfig": {"onlyTopCall": false, "withLog": false}
            }
        ]),
    )
    .await;
    assert!(
        trace.get("result").is_some(),
        "local call trace probe failed: {trace}"
    );
    tx_hash
}

async fn fork_verification_config(
    anvil_url: &str,
    verifier_urls: [&str; 2],
) -> BlockchainVerificationConfig {
    let rpc = BlockchainHttpRpcClient::new(anvil_url.to_string(), None, None);
    let checkpoint = rpc.finalized_block().await.unwrap();
    let block = format!("0x{:x}", checkpoint.number);
    let probe = |call_data: &str, expected_output: String| BlockchainContractProbe {
        call_data: call_data.to_string(),
        expected_output,
    };

    let router_factory = rpc_call_result(anvil_url, ROUTER, "0xc45a0155", &block).await;
    let router_weth = rpc_call_result(anvil_url, ROUTER, "0x4aa4a4fc", &block).await;
    let factory_pool_call = factory_pool_call();
    let factory_pool = rpc_call_result(anvil_url, FACTORY, &factory_pool_call, &block).await;
    let quote_factory = rpc_call_result(anvil_url, QUOTE, "0xc45a0155", &block).await;
    let weth_decimals = rpc_call_result(anvil_url, WETH, "0x313ce567", &block).await;
    let weth_storage_value = rpc_result(
        anvil_url,
        "eth_getStorageAt",
        serde_json::json!([WETH, EIP1967_IMPLEMENTATION_SLOT, block]),
    )
    .await;
    assert_eq!(
        weth_storage_value.to_ascii_lowercase(),
        format!(
            "0x000000000000000000000000{}",
            WETH_IMPLEMENTATION.trim_start_matches("0x")
        )
        .to_ascii_lowercase()
    );
    let weth_implementation_code_hash =
        runtime_code_hash(anvil_url, WETH_IMPLEMENTATION, &block).await;
    let usdc_decimals = rpc_call_result(anvil_url, USDC, "0x313ce567", &block).await;
    let usdc_storage_value = rpc_result(
        anvil_url,
        "eth_getStorageAt",
        serde_json::json!([USDC, ZEPPELINOS_IMPLEMENTATION_SLOT, block]),
    )
    .await;
    assert_eq!(
        usdc_storage_value.to_ascii_lowercase(),
        format!(
            "0x000000000000000000000000{}",
            USDC_IMPLEMENTATION.trim_start_matches("0x")
        )
        .to_ascii_lowercase()
    );
    let usdc_implementation_code_hash =
        runtime_code_hash(anvil_url, USDC_IMPLEMENTATION, &block).await;
    let pool_token0 = rpc_call_result(anvil_url, POOL, "0x0dfe1681", &block).await;
    let pool_token1 = rpc_call_result(anvil_url, POOL, "0xd21220a7", &block).await;
    let pool_fee = rpc_call_result(anvil_url, POOL, "0xddca3f43", &block).await;

    let contracts = vec![
        BlockchainContractManifest {
            address: ROUTER.to_string(),
            role: BlockchainContractRole::Router,
            runtime_code_hash: runtime_code_hash(anvil_url, ROUTER, &block).await,
            proxy: None,
            probes: vec![
                probe("0xc45a0155", router_factory),
                probe("0x4aa4a4fc", router_weth),
            ],
        },
        BlockchainContractManifest {
            address: FACTORY.to_string(),
            role: BlockchainContractRole::Factory,
            runtime_code_hash: runtime_code_hash(anvil_url, FACTORY, &block).await,
            proxy: None,
            probes: vec![probe(&factory_pool_call, factory_pool)],
        },
        BlockchainContractManifest {
            address: WETH.to_string(),
            role: BlockchainContractRole::WrappedNative,
            runtime_code_hash: runtime_code_hash(anvil_url, WETH, &block).await,
            proxy: Some(BlockchainProxyManifest {
                kind: "eip1967_implementation".to_string(),
                storage_slot: EIP1967_IMPLEMENTATION_SLOT.to_string(),
                storage_value: weth_storage_value,
                target_address: WETH_IMPLEMENTATION.to_string(),
                target_code_hash: weth_implementation_code_hash.clone(),
            }),
            probes: vec![probe("0x313ce567", weth_decimals)],
        },
        BlockchainContractManifest {
            address: WETH_IMPLEMENTATION.to_string(),
            role: BlockchainContractRole::Implementation,
            runtime_code_hash: weth_implementation_code_hash,
            proxy: None,
            probes: Vec::new(),
        },
        BlockchainContractManifest {
            address: QUOTE.to_string(),
            role: BlockchainContractRole::Quote,
            runtime_code_hash: runtime_code_hash(anvil_url, QUOTE, &block).await,
            proxy: None,
            probes: vec![probe("0xc45a0155", quote_factory)],
        },
        BlockchainContractManifest {
            address: USDC.to_string(),
            role: BlockchainContractRole::Token,
            runtime_code_hash: runtime_code_hash(anvil_url, USDC, &block).await,
            proxy: Some(BlockchainProxyManifest {
                kind: "zeppelinos_implementation".to_string(),
                storage_slot: ZEPPELINOS_IMPLEMENTATION_SLOT.to_string(),
                storage_value: usdc_storage_value,
                target_address: USDC_IMPLEMENTATION.to_string(),
                target_code_hash: usdc_implementation_code_hash.clone(),
            }),
            probes: vec![probe("0x313ce567", usdc_decimals)],
        },
        BlockchainContractManifest {
            address: USDC_IMPLEMENTATION.to_string(),
            role: BlockchainContractRole::Implementation,
            runtime_code_hash: usdc_implementation_code_hash,
            proxy: None,
            probes: Vec::new(),
        },
        BlockchainContractManifest {
            address: POOL.to_string(),
            role: BlockchainContractRole::Pool,
            runtime_code_hash: runtime_code_hash(anvil_url, POOL, &block).await,
            proxy: None,
            probes: vec![
                probe("0x0dfe1681", pool_token0),
                probe("0xd21220a7", pool_token1),
                probe("0xddca3f43", pool_fee),
            ],
        },
    ];
    let mut call_edges = Vec::new();
    for purpose in ["wrap", "approve", "swap_sell", "swap_buy"] {
        call_edges.push(BlockchainCallEdgeManifest {
            purpose: purpose.to_string(),
            caller: WETH.to_string(),
            target: WETH_IMPLEMENTATION.to_string(),
            call_type: "delegatecall".to_string(),
        });
    }

    for purpose in ["approve", "swap_sell", "swap_buy"] {
        call_edges.push(BlockchainCallEdgeManifest {
            purpose: purpose.to_string(),
            caller: USDC.to_string(),
            target: USDC_IMPLEMENTATION.to_string(),
            call_type: "delegatecall".to_string(),
        });
    }

    for purpose in ["swap_sell", "swap_buy"] {
        for (caller, target) in [
            (ROUTER, POOL),
            (POOL, ROUTER),
            (ROUTER, WETH),
            (ROUTER, USDC),
            (POOL, WETH),
            (POOL, USDC),
        ] {
            call_edges.push(BlockchainCallEdgeManifest {
                purpose: purpose.to_string(),
                caller: caller.to_string(),
                target: target.to_string(),
                call_type: "call".to_string(),
            });
        }

        for target in [WETH, USDC] {
            call_edges.push(BlockchainCallEdgeManifest {
                purpose: purpose.to_string(),
                caller: POOL.to_string(),
                target: target.to_string(),
                call_type: "staticcall".to_string(),
            });
        }
    }
    let deployment_manifest = BlockchainDeploymentManifest {
        version: "anvil-fork-v1".to_string(),
        chain_id: CHAIN_ID as u32,
        chain_name: "Arbitrum".to_string(),
        contracts,
        tokens: vec![
            BlockchainTokenManifest {
                address: WETH.to_string(),
                name: "Wrapped Ether".to_string(),
                symbol: "WETH".to_string(),
                decimals: 18,
                asset_role: "both".to_string(),
            },
            BlockchainTokenManifest {
                address: USDC.to_string(),
                name: "USD Coin".to_string(),
                symbol: "USDC".to_string(),
                decimals: 6,
                asset_role: "both".to_string(),
            },
        ],
        pools: vec![BlockchainPoolManifest {
            address: POOL.to_string(),
            token0: WETH.to_string(),
            token1: USDC.to_string(),
            fee: 500,
            factory: FACTORY.to_string(),
            quote_contract: QUOTE.to_string(),
        }],
        call_edges,
    };
    let manifest_digest = keccak256(serde_json::to_vec(&deployment_manifest).unwrap()).to_string();

    BlockchainVerificationConfig {
        authoritative: provider_identity("authoritative", "operator-a", "domain-a"),
        verifiers: vec![
            BlockchainVerificationProviderConfig {
                identity: provider_identity("verifier-a", "operator-b", "domain-b"),
                http_rpc_url: verifier_urls[0].into(),
            },
            BlockchainVerificationProviderConfig {
                identity: provider_identity("verifier-b", "operator-c", "domain-c"),
                http_rpc_url: verifier_urls[1].into(),
            },
        ],
        chain_anchor: BlockchainChainAnchorConfig {
            chain_id: CHAIN_ID as u32,
            chain_name: "Arbitrum".to_string(),
            checkpoint_height: checkpoint.number,
            checkpoint_hash: checkpoint.hash.to_string(),
            checkpoint_timestamp: checkpoint.timestamp,
            max_head_skew_blocks: 3,
            max_head_age_secs: u64::MAX,
            max_future_drift_secs: u64::MAX,
        },
        manifest_version: deployment_manifest.version.clone(),
        manifest_digest,
        deployment_manifest,
    }
}

fn provider_identity(
    provider_id: &str,
    operator_id: &str,
    failure_domain_id: &str,
) -> BlockchainProviderIdentity {
    BlockchainProviderIdentity {
        provider_id: provider_id.to_string(),
        operator_id: operator_id.to_string(),
        failure_domain_ids: vec![failure_domain_id.to_string()],
    }
}

fn factory_pool_call() -> String {
    let mut call = hex::decode("1698ee82").unwrap();
    call.extend_from_slice(&[0; 12]);
    call.extend_from_slice(WETH.parse::<Address>().unwrap().as_slice());
    call.extend_from_slice(&[0; 12]);
    call.extend_from_slice(USDC.parse::<Address>().unwrap().as_slice());
    call.extend_from_slice(&U256::from(500u64).to_be_bytes::<32>());
    hex::encode_prefixed(call)
}

async fn runtime_code_hash(anvil_url: &str, address: &str, block: &str) -> String {
    let code = rpc_result(
        anvil_url,
        "eth_getCode",
        serde_json::json!([address, block]),
    )
    .await;
    let code = hex::decode(code.trim_start_matches("0x")).unwrap();
    assert!(!code.is_empty(), "missing fork bytecode for {address}");
    keccak256(code).to_string()
}

async fn rpc_call_result(anvil_url: &str, to: &str, data: &str, block: &str) -> String {
    rpc_result(
        anvil_url,
        "eth_call",
        serde_json::json!([{"to": to, "data": data}, block]),
    )
    .await
}

async fn rpc_result(anvil_url: &str, method: &str, params: serde_json::Value) -> String {
    let response = anvil_rpc(anvil_url, method, params).await;
    assert!(
        response.get("error").is_none(),
        "{method} failed while building the fork manifest: {response}"
    );
    response["result"].as_str().unwrap().to_string()
}

pub(crate) fn weth_usdc_pool() -> Pool {
    let chain = Arc::new(chains::ARBITRUM.clone());
    let dex = UNISWAP_V3.dex.clone();
    let weth = Token::new(
        chain.clone(),
        address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
        "Wrapped Ether".to_string(),
        "WETH".to_string(),
        18,
    );
    let usdc = Token::new(
        chain.clone(),
        address!("af88d065e77c8cC2239327C5EDb3A432268e5831"),
        "USD Coin".to_string(),
        "USDC".to_string(),
        6,
    );

    Pool::new(
        chain,
        dex,
        address!("C6962004f452bE9203591991D15f6b388e09E8D0"),
        PoolIdentifier::from_address(address!("C6962004f452bE9203591991D15f6b388e09E8D0")),
        FORK_BLOCK,
        weth,
        usdc,
        Some(500),
        Some(10),
        UnixNanos::default(),
    )
}

pub(crate) async fn start_anvil(fork_rpc_url: &str) -> Option<(AnvilProcess, AnvilStartup)> {
    start_anvil_at(fork_rpc_url, Some(FORK_BLOCK)).await
}

pub(crate) async fn start_anvil_at(
    fork_rpc_url: &str,
    fork_block: Option<u64>,
) -> Option<(AnvilProcess, AnvilStartup)> {
    let mut command = Command::new("anvil");
    command
        .arg("--fork-url")
        .arg(fork_rpc_url)
        .arg("--retries")
        .arg("8")
        .arg("--fork-retry-backoff")
        .arg("1500")
        .arg("--accounts")
        .arg("1")
        .arg("--chain-id")
        .arg(CHAIN_ID.to_string())
        .arg("--block-time")
        .arg("1")
        .arg("--mixed-mining")
        .arg("--steps-tracing")
        .arg("--slots-in-an-epoch")
        .arg("1")
        .arg("--port")
        .arg("0");
    if let Some(block) = fork_block {
        command.arg("--fork-block-number").arg(block.to_string());
    }
    let mut child = match command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("anvil binary not found on PATH: {e}");
            return None;
        }
    };

    // Read the startup banner for the bound port and version, then keep draining stdout
    // and stderr in the background so the pipes cannot block the process
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (startup_tx, startup_rx) = std::sync::mpsc::channel::<Option<AnvilStartup>>();
    let (output_tx, output_rx) = std::sync::mpsc::channel::<String>();
    let stderr_tx = output_tx.clone();

    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            let _ = stderr_tx.send(line);
        }
    });

    std::thread::spawn(move || {
        let mut version = String::new();
        let reader = BufReader::new(stdout);
        let mut startup_sent = false;

        for line in reader.lines() {
            let Ok(line) = line else { break };

            if !startup_sent {
                let _ = output_tx.send(line.clone());
                if version.is_empty()
                    && let Some(start) = line.find(char::is_numeric)
                    && line[..start].trim().is_empty()
                    && line.contains('.')
                {
                    version = line.trim().to_string();
                }

                if let Some(listen_at) = line.find("Listening on 127.0.0.1:") {
                    let port_text = &line[listen_at + "Listening on 127.0.0.1:".len()..];
                    let port = port_text.trim().parse::<u16>().ok();
                    let startup = port.map(|port| AnvilStartup {
                        port,
                        version: version.clone(),
                    });
                    let _ = startup_tx.send(startup);
                    startup_sent = true;
                }
            }
        }

        if !startup_sent {
            let _ = startup_tx.send(None);
        }
    });

    let startup = match startup_rx.recv_timeout(ANVIL_READY_TIMEOUT) {
        Ok(Some(startup)) => startup,
        Ok(None) => {
            let mut output = String::new();
            while let Ok(line) = output_rx.try_recv() {
                output.push_str(&line);
                output.push('\n');
            }
            eprintln!("Anvil exited before binding:\n{output}");
            return None;
        }
        Err(_) => {
            eprintln!("Anvil did not print a listening address in time");
            return None;
        }
    };

    let rpc_client =
        BlockchainHttpRpcClient::new(format!("http://127.0.0.1:{}", startup.port), None, None);
    let deadline = Instant::now() + ANVIL_READY_TIMEOUT;

    loop {
        match rpc_client.chain_id().await {
            Ok(chain_id) if chain_id == CHAIN_ID => break,
            _ if Instant::now() < deadline => tokio::time::sleep(Duration::from_secs(1)).await,
            _ => {
                eprintln!("Anvil did not become ready within {ANVIL_READY_TIMEOUT:?}");
                return None;
            }
        }
    }

    Some((AnvilProcess(child), startup))
}

pub(crate) async fn fund_anvil_wallet(anvil_url: &str, wallet: Address) {
    let response = anvil_rpc(
        anvil_url,
        "anvil_setBalance",
        serde_json::json!([wallet.to_string(), format!("0x{FUND_AMOUNT_WEI:x}")]),
    )
    .await;
    assert_eq!(response["result"], serde_json::Value::Null);
}

#[allow(
    dead_code,
    reason = "used by execution_fork; this harness is compiled into each fork binary"
)]
pub(crate) async fn anvil_set_automine(anvil_url: &str, enabled: bool) {
    let response = anvil_rpc(anvil_url, "evm_setAutomine", serde_json::json!([enabled])).await;
    assert!(
        response.get("error").is_none(),
        "evm_setAutomine failed: {response}"
    );
}

#[allow(
    dead_code,
    reason = "used by execution_fork; this harness is compiled into each fork binary"
)]
pub(crate) async fn anvil_set_interval_mining(anvil_url: &str, seconds: u64) {
    let response = anvil_rpc(
        anvil_url,
        "evm_setIntervalMining",
        serde_json::json!([seconds]),
    )
    .await;
    assert!(
        response.get("error").is_none(),
        "evm_setIntervalMining failed: {response}"
    );
}

#[allow(
    dead_code,
    reason = "used by execution_fork; this harness is compiled into each fork binary"
)]
pub(crate) async fn anvil_mine(anvil_url: &str, blocks: u64) {
    let response = anvil_rpc(
        anvil_url,
        "anvil_mine",
        serde_json::json!([format!("0x{blocks:x}"), "0x0"]),
    )
    .await;
    assert!(
        response.get("error").is_none(),
        "anvil_mine failed: {response}"
    );
}

#[allow(
    dead_code,
    reason = "used by execution_fork; this harness is compiled into each fork binary"
)]
pub(crate) async fn anvil_drop_transaction(anvil_url: &str, transaction_hash: &str) {
    let response = anvil_rpc(
        anvil_url,
        "anvil_dropTransaction",
        serde_json::json!([transaction_hash]),
    )
    .await;
    assert_eq!(
        response["result"].as_str(),
        Some(transaction_hash),
        "anvil_dropTransaction failed: {response}"
    );
}

async fn anvil_rpc(anvil_url: &str, method: &str, params: serde_json::Value) -> serde_json::Value {
    let client = HttpClient::builder().timeout_secs(10).build().unwrap();
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    let body = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params
    }))
    .unwrap();
    let response = client
        .request(
            Method::POST,
            anvil_url.to_string(),
            None,
            Some(headers),
            Some(body),
            Some(10),
            None,
        )
        .await
        .unwrap();
    serde_json::from_slice(&response.body).unwrap()
}

// Additive schema only: create the tables this slice needs when absent, never touch
// existing data
pub(crate) async fn ensure_execution_schema(admin_pool: &PgPool) {
    for statement in [
        r#"CREATE TABLE IF NOT EXISTS "chain" (
            chain_id INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL
        )"#,
        r#"INSERT INTO "chain" (chain_id, name) VALUES (42161, 'Arbitrum') ON CONFLICT DO NOTHING"#,
        r#"CREATE TABLE IF NOT EXISTS "execution_transaction" (
            id BIGSERIAL PRIMARY KEY,
            chain_id INTEGER NOT NULL REFERENCES chain(chain_id) ON DELETE CASCADE,
            nonce BIGINT NOT NULL,
            transaction_hash TEXT NOT NULL,
            purpose TEXT NOT NULL,
            status TEXT NOT NULL,
            UNIQUE (chain_id, transaction_hash)
        )"#,
    ] {
        sqlx::query(statement).execute(admin_pool).await.unwrap();
    }
}

/// Builds a full-range snapshot carrying the fork's real active liquidity so a restored
/// profiler quotes with the fork's execution math, plus the quoted USDC output for the
/// configured swap amount.
///
/// A synthetic full-range position carries the pool's real active liquidity: the tiny swap
/// never crosses a tick, so the local quote matches the fork's execution math.
pub(crate) async fn build_full_range_snapshot(
    rpc_client: &Arc<BlockchainHttpRpcClient>,
    pool: &Pool,
) -> (PoolSnapshot, U256) {
    let pool_contract = UniswapV3PoolContract::new(rpc_client.clone(), 100);
    let pool_state = pool_contract
        .get_global_state(&pool.address, None, FeeProtocolEncoding::UniswapV3Packed)
        .await
        .unwrap();
    let head = rpc_client.latest_block().await.unwrap();
    let head_hash = head.hash.to_string();
    let liquidity = pool_state.liquidity;
    let snapshot = PoolSnapshot::new(
        pool.instrument_id,
        pool_state,
        vec![PoolPosition::new(
            pool.address,
            -887_220,
            887_220,
            liquidity as i128,
        )],
        vec![
            PoolTick::new(
                -887_220,
                liquidity,
                liquidity as i128,
                U256::ZERO,
                U256::ZERO,
                true,
                0,
            ),
            PoolTick::new(
                887_220,
                liquidity,
                -(liquidity as i128),
                U256::ZERO,
                U256::ZERO,
                true,
                0,
            ),
        ],
        PoolAnalytics::default(),
        BlockPosition::new(
            head.number,
            head_hash.clone(),
            BLOCK_SCOPED_SNAPSHOT_INDEX,
            BLOCK_SCOPED_SNAPSHOT_INDEX,
        )
        .with_block_hash(Some(head_hash)),
        UnixNanos::default(),
        UnixNanos::default(),
    );

    let mut profiler = PoolProfiler::new(Arc::new(pool.clone()));
    profiler.restore_from_snapshot(snapshot.clone()).unwrap();
    let quote = profiler
        .swap_exact_in(U256::from(WRAP_AMOUNT_WEI), true, None)
        .unwrap();
    let quoted_out = quote.amount1.unsigned_abs();

    (snapshot, quoted_out)
}

#[allow(
    dead_code,
    reason = "used by execution_fork and execution_livenode_fork; this harness is compiled into each fork binary"
)]
pub(crate) fn quote_buy_amount_in(snapshot: &PoolSnapshot, pool: &Pool) -> U256 {
    let mut profiler = PoolProfiler::new(Arc::new(pool.clone()));
    profiler.restore_from_snapshot(snapshot.clone()).unwrap();
    profiler
        .swap_exact_out(U256::from(WRAP_AMOUNT_WEI), false, None)
        .unwrap()
        .get_input_amount()
}

pub(crate) fn git_diff_sha256(args: &[&str]) -> String {
    Command::new("git")
        .arg("diff")
        .arg("--binary")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || "unknown".to_string(),
            |output| {
                let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &output.stdout);
                nautilus_core::hex::encode(digest.as_ref())
            },
        )
}
