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

use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex as StdMutex},
};

use alloy::{
    primitives::{Address, U256, address},
    sol,
    sol_types::{SolCall, SolValue},
};
use async_trait::async_trait;
use nautilus_blockchain::{
    config::BlockchainExecutionClientConfig,
    contracts::{
        pancakeswap_v2_router::PancakeSwapV2Router, uniswap_v2_pair::SWAP_EVENT_TOPIC0_HEX,
    },
    execution::{
        client::{BlockchainExecutionClient, ExecutionTxSigner},
        journal::{
            JournalEvent, JournalEventStatus, JournalIntentKind, OrderIdempotencyKey,
            append_event_jsonl,
        },
        metadata_store::{InMemoryMetadataStore, PoolMetadataStore},
        signer::{SignRequest, SignedTx, tx_hash_hex_from_signer_raw_tx},
    },
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    messages::execution::{GenerateFillReports, SubmitOrder},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    defi::{
        AmmType, Dex, DexType, Pool, PoolIdentifier, Token, chain::chains,
        validation::validate_address,
    },
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::OrderInitialized,
    identifiers::{AccountId, ClientId, ClientOrderId, StrategyId, TraderId, Venue},
    stubs::TestDefault,
    types::Quantity,
};
use serde_json::{Value, json};

mod common;

use common::{MockRpcState, start_mock_rpc_server};

const KNOWN_RAW_TX_HEX: &str = "0x02f8af380384068e778084068e77808286ed947977bf3e7e0c954d12cdca3e013adaf57e0b06e080b844a9059cbb000000000000000000000000c7bd78fa510c234b7e6f70f41452948e41d9a9210000000000000000000000000000000000000000000000007e2b7f14c776c000c001a08875591e4e10637a3498a8a05b1d4e498c1d027b2f763a77db767173955a8722a039defee4d3fb717502dcfcff5ccb9122c473e5efac8ad3c5c9c2e97e64a0885f";
const WALLET_ADDRESS: &str = "0x058e41Ae42e322e5E6ea6Fc9930776d67CDd3115";
const ROUTER_ADDRESS: &str = "0x7977BF3e7e0c954D12cdcA3E013ADAf57E0B06E0";

sol! {
    struct SwapEventDataFixture {
        uint256 amount0In;
        uint256 amount1In;
        uint256 amount0Out;
        uint256 amount1Out;
    }
}

#[derive(Debug)]
struct StaticSigner {
    signed: SignedTx,
    requests: StdMutex<Vec<SignRequest>>,
}

impl StaticSigner {
    fn new(signed: SignedTx) -> Self {
        Self {
            signed,
            requests: StdMutex::new(Vec::new()),
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().expect("signer requests lock").len()
    }
}

#[async_trait(?Send)]
impl ExecutionTxSigner for StaticSigner {
    async fn sign_evm_tx(&self, request: SignRequest) -> anyhow::Result<SignedTx> {
        self.requests
            .lock()
            .expect("signer requests lock")
            .push(request);
        Ok(self.signed.clone())
    }
}

fn rpc_result(result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": result,
    })
}

fn topic_from_address(address: Address) -> String {
    let mut word = [0u8; 32];
    word[12..].copy_from_slice(address.as_slice());
    format!("0x{}", hex::encode(word))
}

fn encode_swap_data(
    amount0_in: U256,
    amount1_in: U256,
    amount0_out: U256,
    amount1_out: U256,
) -> String {
    let encoded = SwapEventDataFixture {
        amount0In: amount0_in,
        amount1In: amount1_in,
        amount0Out: amount0_out,
        amount1Out: amount1_out,
    }
    .abi_encode();
    format!("0x{}", hex::encode(encoded))
}

fn make_swap_log(
    pool: Address,
    sender: Address,
    to: Address,
    amount0_in: U256,
    amount1_in: U256,
    amount0_out: U256,
    amount1_out: U256,
    log_index: u64,
) -> Value {
    json!({
        "address": pool.to_string(),
        "topics": vec![
            SWAP_EVENT_TOPIC0_HEX.to_string(),
            topic_from_address(sender),
            topic_from_address(to),
        ],
        "data": encode_swap_data(amount0_in, amount1_in, amount0_out, amount1_out),
        "logIndex": format!("0x{log_index:x}"),
        "transactionIndex": "0x1",
        "transactionHash": "0x4f6a905f6ac0f749f2f7d4fe53cc994f94935271528ef2f7f5f51775a8d238f9",
        "blockHash": "0x5f567bb0f2a4e22a5f8cc05610b18d5f8ef01cf15b8b7b8d0a8e1756e7f64f5e",
        "blockNumber": "0x2a",
        "removed": false
    })
}

fn make_receipt(tx_hash: &str, wallet: Address, router: Address, pool: Address) -> Value {
    json!({
        "transactionHash": tx_hash,
        "blockHash": "0x5f567bb0f2a4e22a5f8cc05610b18d5f8ef01cf15b8b7b8d0a8e1756e7f64f5e",
        "blockNumber": "0x2a",
        "from": wallet.to_string(),
        "to": router.to_string(),
        "cumulativeGasUsed": "0x186a0",
        "gasUsed": "0x15f90",
        "effectiveGasPrice": "0x12a05f200",
        "status": "0x1",
        "logs": vec![make_swap_log(
            pool,
            address!("0x9999999999999999999999999999999999999999"),
            wallet,
            U256::ZERO,
            U256::from(2_500_000_000_000_000_000_000u128),
            U256::from(1_000_000_000_000_000_000_000u128),
            U256::ZERO,
            7,
        )],
    })
}

fn encode_amounts_response(amounts: Vec<U256>) -> String {
    let encoded = PancakeSwapV2Router::getAmountsOutCall::abi_encode_returns(&amounts);
    format!("0x{}", hex::encode(encoded))
}

fn make_signed_tx() -> SignedTx {
    let tx_hash = tx_hash_hex_from_signer_raw_tx(KNOWN_RAW_TX_HEX).expect("known tx hash");
    SignedTx {
        raw_tx_hex: KNOWN_RAW_TX_HEX.to_string(),
        r: "0x01".to_string(),
        s: "0x02".to_string(),
        v: 1,
        tx_hash,
        request_id: 1,
    }
}

fn make_pool(router: Address, pool_address: Address) -> Pool {
    let chain = Arc::new(chains::BSC.clone());
    let dex = Arc::new(Dex::new(
        (*chain).clone(),
        DexType::PancakeSwapV2,
        &router.to_string(),
        0,
        AmmType::CPAMM,
        "PairCreated(address,address,address,uint256)",
        "Swap(address,uint256,uint256,uint256,uint256,address)",
        "Mint(address,uint256,uint256)",
        "Burn(address,uint256,uint256,address)",
        "Sync(uint112,uint112)",
    ));

    let token0 = Token::new(
        chain.clone(),
        validate_address("0x55d398326f99059fF775485246999027B3197955").expect("token0"),
        "USDT".to_string(),
        "USDT".to_string(),
        18,
    );
    let token1 = Token::new(
        chain.clone(),
        validate_address("0x8AC76a51cc950d9822D68b83fE1Ad97B32Cd580d").expect("token1"),
        "USDC".to_string(),
        "USDC".to_string(),
        18,
    );

    Pool::new(
        chain,
        dex,
        pool_address,
        PoolIdentifier::from_address(pool_address),
        0,
        token0,
        token1,
        Some(2500),
        None,
        UnixNanos::default(),
    )
}

fn temp_journal_path(tag: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = UnixNanos::from(nautilus_core::time::nanos_since_unix_epoch()).as_u64();
    path.push(format!("nautilus-pr12b-{tag}-{nanos}.jsonl"));
    path
}

fn make_submit_cmd(pool: &Pool, order_id: ClientOrderId) -> SubmitOrder {
    let trader_id = TraderId::test_default();
    let strategy_id = StrategyId::test_default();

    let mut order_init = OrderInitialized::test_default();
    order_init.trader_id = trader_id;
    order_init.strategy_id = strategy_id;
    order_init.instrument_id = pool.instrument_id;
    order_init.client_order_id = order_id;
    order_init.order_side = OrderSide::Buy;
    order_init.order_type = OrderType::Market;
    order_init.time_in_force = TimeInForce::Ioc;
    order_init.quantity = Quantity::new(1000.0, 0);

    SubmitOrder::new(
        trader_id,
        None,
        strategy_id,
        pool.instrument_id,
        order_id,
        order_init,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

fn make_client(
    rpc_url: String,
    metadata_store: InMemoryMetadataStore,
    signer: Arc<dyn ExecutionTxSigner>,
    journal_path: Option<PathBuf>,
) -> BlockchainExecutionClient {
    let trader_id = TraderId::test_default();
    let account_id = AccountId::new("BINANCE-001");
    let venue = Venue::new("Bsc:PancakeSwapV2");

    let mut config = BlockchainExecutionClientConfig::new(
        trader_id,
        account_id,
        venue,
        chains::BSC.clone(),
        String::from(WALLET_ADDRESS),
        None,
        rpc_url,
        None,
    );
    config.signer_wallet_address = Some(WALLET_ADDRESS.to_string());
    config.execution_router_address = Some(ROUTER_ADDRESS.to_string());
    config.execution_max_fee_per_gas = 0x68e7780;
    config.execution_max_priority_fee_per_gas = 0x68e7780;
    config.execution_receipt_max_polls = 5;
    config.execution_receipt_poll_interval_ms = 1;
    config.execution_default_deadline_secs = 60;
    config.execution_confirmations_required = 1;
    config.execution_require_preapproved_allowance = true;
    config.execution_journal_path = journal_path.map(|path| path.display().to_string());

    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("BLOCKCHAIN"),
        venue,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );

    BlockchainExecutionClient::with_metadata_store_and_signer(
        core,
        config,
        Box::new(metadata_store),
        signer,
    )
    .expect("execution client should construct")
}

#[tokio::test(flavor = "multi_thread")]
async fn test_submit_market_order_happy_path_emits_fill_from_swap_log() {
    let state = MockRpcState::new();
    let addr = start_mock_rpc_server(state.clone()).await;
    let rpc_url = format!("http://{addr}/");

    let signed = make_signed_tx();
    let tx_hash = signed.tx_hash.clone();

    state
        .enqueue_json(
            "eth_call",
            rpc_result(json!(encode_amounts_response(vec![
                U256::from(2_500_000_000_000_000_000_000u128),
                U256::from(1_000_000_000_000_000_000_000u128)
            ]))),
        )
        .await;
    state
        .enqueue_json("eth_estimateGas", rpc_result(json!("0x86ed")))
        .await;
    state
        .enqueue_json("eth_getTransactionCount", rpc_result(json!("0x3")))
        .await;
    state
        .enqueue_json("eth_sendRawTransaction", rpc_result(json!(tx_hash.clone())))
        .await;

    let wallet = validate_address(WALLET_ADDRESS).expect("wallet address");
    let router = validate_address(ROUTER_ADDRESS).expect("router address");
    let pool_address = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");

    state
        .enqueue_json(
            "eth_getTransactionReceipt",
            rpc_result(make_receipt(tx_hash.as_str(), wallet, router, pool_address)),
        )
        .await;

    state
        .enqueue_json(
            "eth_getTransactionByHash",
            rpc_result(json!({
                "chainId": "0x38",
                "hash": tx_hash,
                "blockHash": "0x5f567bb0f2a4e22a5f8cc05610b18d5f8ef01cf15b8b7b8d0a8e1756e7f64f5e",
                "blockNumber": "0x2a",
                "from": wallet,
                "to": router,
                "gas": "0x86ed",
                "gasPrice": "0x68e7780",
                "nonce": "0x3",
                "transactionIndex": "0x1",
                "value": "0x0",
                "input": "0x8803dbee0000000000000000000000000000000000000000000000000000000000000000",
                "maxFeePerGas": "0x68e7780",
                "maxPriorityFeePerGas": "0x68e7780"
            })),
        )
        .await;

    let mut metadata_store = InMemoryMetadataStore::new();
    let pool = make_pool(router, pool_address);
    metadata_store.insert_pool(pool.clone());

    let signer = Arc::new(StaticSigner::new(signed.clone()));
    let client = make_client(
        rpc_url,
        metadata_store,
        signer.clone(),
        Some(temp_journal_path("happy")),
    );

    let cmd = make_submit_cmd(&pool, ClientOrderId::new("O-PR12B-HAPPY-001"));
    client
        .submit_order(&cmd)
        .expect("happy-path submit should succeed");

    let fill_reports = client
        .generate_fill_reports(GenerateFillReports::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(pool.instrument_id),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("fill reports should generate");

    assert_eq!(fill_reports.len(), 1);
    assert_eq!(fill_reports[0].venue_order_id.as_str(), signed.tx_hash);
    assert_eq!(fill_reports[0].last_qty.as_f64(), 1000.0);
    assert!((fill_reports[0].last_px.as_f64() - 2.5).abs() < 1e-9);
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 1);
    assert_eq!(state.method_count("eth_getTransactionReceipt").await, 1);
    assert_eq!(state.method_count("eth_call").await, 1);
    assert_eq!(signer.request_count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_submit_market_order_duplicate_submit_is_noop_while_inflight() {
    let state = MockRpcState::new();
    let addr = start_mock_rpc_server(state.clone()).await;
    let rpc_url = format!("http://{addr}/");

    let signed = make_signed_tx();
    let signer = Arc::new(StaticSigner::new(signed));

    let mut metadata_store = InMemoryMetadataStore::new();
    let router = validate_address(ROUTER_ADDRESS).expect("router address");
    let pool = make_pool(
        router,
        address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245"),
    );
    metadata_store.insert_pool(pool.clone());

    let journal_path = temp_journal_path("inflight");
    let key = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        validate_address(WALLET_ADDRESS).expect("wallet address"),
        "O-PR12B-DUPE-001",
    );
    append_event_jsonl(
        &journal_path,
        &JournalEvent {
            sequence: 1,
            ts_event_ns: 1,
            idempotency_key: key,
            intent_kind: JournalIntentKind::Swap,
            intent_hash: "intent-inflight".to_string(),
            tx_hash: None,
            raw_tx_hash: None,
            reserved_nonce: Some(3),
            status: JournalEventStatus::Submitted,
        },
    )
    .expect("seed inflight journal");

    let client = make_client(rpc_url, metadata_store, signer.clone(), Some(journal_path));
    let cmd = make_submit_cmd(&pool, ClientOrderId::new("O-PR12B-DUPE-001"));

    client
        .submit_order(&cmd)
        .expect("duplicate inflight submit should be no-op");

    assert_eq!(state.method_count("eth_sendRawTransaction").await, 0);
    assert_eq!(signer.request_count(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_submit_market_order_duplicate_submit_is_rejected_when_terminal() {
    let state = MockRpcState::new();
    let addr = start_mock_rpc_server(state.clone()).await;
    let rpc_url = format!("http://{addr}/");

    let signed = make_signed_tx();
    let signer = Arc::new(StaticSigner::new(signed));

    let mut metadata_store = InMemoryMetadataStore::new();
    let router = validate_address(ROUTER_ADDRESS).expect("router address");
    let pool = make_pool(
        router,
        address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245"),
    );
    metadata_store.insert_pool(pool.clone());

    let journal_path = temp_journal_path("terminal");
    let key = OrderIdempotencyKey::new(
        "Bsc:PancakeSwapV2",
        validate_address(WALLET_ADDRESS).expect("wallet address"),
        "O-PR12B-DUPE-002",
    );
    append_event_jsonl(
        &journal_path,
        &JournalEvent {
            sequence: 1,
            ts_event_ns: 1,
            idempotency_key: key,
            intent_kind: JournalIntentKind::Swap,
            intent_hash: "intent-terminal".to_string(),
            tx_hash: Some("0xterminalhash".to_string()),
            raw_tx_hash: Some("0xterminalhash".to_string()),
            reserved_nonce: Some(3),
            status: JournalEventStatus::Filled,
        },
    )
    .expect("seed terminal journal");

    let client = make_client(rpc_url, metadata_store, signer.clone(), Some(journal_path));
    let cmd = make_submit_cmd(&pool, ClientOrderId::new("O-PR12B-DUPE-002"));

    let err = client
        .submit_order(&cmd)
        .expect_err("duplicate terminal submit must be rejected");

    assert!(
        err.to_string()
            .to_ascii_lowercase()
            .contains("already terminal"),
        "unexpected error: {err}"
    );
    assert_eq!(state.method_count("eth_sendRawTransaction").await, 0);
    assert_eq!(signer.request_count(), 0);

    if let Some(path) = client.execution_journal_path() {
        let _ = fs::remove_file(path);
    }
}

#[test]
fn test_execution_runtime_rejects_signer_wallet_mismatch() {
    let trader_id = TraderId::test_default();
    let account_id = AccountId::new("BINANCE-001");
    let venue = Venue::new("Bsc:PancakeSwapV2");
    let mut config = BlockchainExecutionClientConfig::new(
        trader_id,
        account_id,
        venue,
        chains::BSC.clone(),
        WALLET_ADDRESS.to_string(),
        None,
        String::from("http://127.0.0.1:8545"),
        None,
    );
    config.signer_endpoint = Some(String::from("http://127.0.0.1:3000"));
    config.signer_wallet_address = Some(String::from("0x1111111111111111111111111111111111111111"));
    config.execution_router_address = Some(ROUTER_ADDRESS.to_string());
    config.execution_max_fee_per_gas = 0x68e7780;
    config.execution_max_priority_fee_per_gas = 0x68e7780;
    config.execution_receipt_max_polls = 1;
    config.execution_receipt_poll_interval_ms = 1;
    config.execution_require_preapproved_allowance = true;

    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("BLOCKCHAIN"),
        venue,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );

    let mut metadata_store = InMemoryMetadataStore::new();
    metadata_store.insert_pool(make_pool(
        validate_address(ROUTER_ADDRESS).expect("router"),
        validate_address("0xd13040d4fe917EE704158CfCB3338dCd2838B245").expect("pool"),
    ));

    let signer = Arc::new(StaticSigner::new(make_signed_tx()));
    let err = BlockchainExecutionClient::with_metadata_store_and_signer(
        core,
        config,
        Box::new(metadata_store),
        signer,
    )
    .expect_err("mismatched signer wallet should fail");

    assert!(
        err.to_string()
            .contains("signer_wallet_address must match wallet_address"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_execution_runtime_requires_router_address_when_signer_enabled() {
    let trader_id = TraderId::test_default();
    let account_id = AccountId::new("BINANCE-001");
    let venue = Venue::new("Bsc:PancakeSwapV2");
    let mut config = BlockchainExecutionClientConfig::new(
        trader_id,
        account_id,
        venue,
        chains::BSC.clone(),
        WALLET_ADDRESS.to_string(),
        None,
        String::from("http://127.0.0.1:8545"),
        None,
    );
    config.signer_endpoint = Some(String::from("http://127.0.0.1:3000"));
    config.signer_wallet_address = Some(WALLET_ADDRESS.to_string());
    config.execution_router_address = None;
    config.execution_max_fee_per_gas = 0x68e7780;
    config.execution_max_priority_fee_per_gas = 0x68e7780;
    config.execution_receipt_max_polls = 1;
    config.execution_receipt_poll_interval_ms = 1;
    config.execution_require_preapproved_allowance = true;

    let cache = Rc::new(RefCell::new(Cache::default()));
    let core = ExecutionClientCore::new(
        trader_id,
        ClientId::new("BLOCKCHAIN"),
        venue,
        OmsType::Netting,
        account_id,
        AccountType::Cash,
        None,
        cache,
    );

    let mut metadata_store = InMemoryMetadataStore::new();
    metadata_store.insert_pool(make_pool(
        validate_address(ROUTER_ADDRESS).expect("router"),
        validate_address("0xd13040d4fe917EE704158CfCB3338dCd2838B245").expect("pool"),
    ));

    let signer = Arc::new(StaticSigner::new(make_signed_tx()));
    let err = BlockchainExecutionClient::with_metadata_store_and_signer(
        core,
        config,
        Box::new(metadata_store),
        signer,
    )
    .expect_err("missing router address should fail");

    assert!(
        err.to_string()
            .contains("execution_router_address is required"),
        "unexpected error: {err}"
    );
}
