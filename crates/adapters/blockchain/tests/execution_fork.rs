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

//! Pinned-block Anvil fork integration test for blockchain execution operations.
//!
//! Forks Arbitrum at a fixed block with a local Anvil process and exercises fail-closed DEX
//! and order validation, `wrap`, `approve`, `preflight`, and a WETH-to-USDC swap through
//! `submit_order` against localhost only. The fork-source RPC only reads chain state; signed
//! transactions never leave localhost. The suite is gated behind `BLOCKCHAIN_FORK_TESTS=1`,
//! requires `BLOCKCHAIN_FORK_RPC_URL`, and never runs in default CI.

#![cfg(feature = "hypersync")]

mod harness;

use std::{cell::RefCell, process::Command, rc::Rc, sync::Arc, time::Duration};

use alloy::{
    primitives::{Address, U256, address},
    signers::local::PrivateKeySigner,
};
use harness::{
    CHAIN_ID, FORK_BLOCK, FUND_AMOUNT_WEI, ROUTER, SIGNER_ENV, SLIPPAGE_BPS, SWAP_AMOUNT, USDC,
    WETH, WRAP_AMOUNT_WEI, build_full_range_snapshot, ensure_execution_schema, fund_anvil_wallet,
    git_diff_sha256, start_anvil, weth_usdc_pool,
};
use nautilus_blockchain::{
    config::BlockchainExecutionClientConfig, constants::BLOCKCHAIN_VENUE,
    contracts::erc20::Erc20Contract, execution::client::BlockchainExecutionClient,
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::replace_exec_event_sender,
    messages::{ExecutionEvent, execution::SubmitOrder},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    defi::{PoolProfiler, chain::chains},
    enums::{AccountType, OmsType, OrderSide, OrderType},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    orders::{Order, OrderAny, OrderTestBuilder},
    types::{Price, Quantity},
};
use sqlx::{
    PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

const PANCAKE_WETH_USDC_POOL: Address = address!("d9e2a1a61b6e61b275cec326465d417e52c1b95c");

#[tokio::test]
async fn anvil_fork_wrap_approve_preflight_and_swap() {
    if std::env::var("BLOCKCHAIN_FORK_TESTS").as_deref() != Ok("1") {
        eprintln!("BLOCKCHAIN_FORK_TESTS is not 1; skipping fork test");
        return;
    }

    let evidence_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/blockchain-fork-evidence");
    for filename in ["run.json", "SHA256SUMS"] {
        let path = evidence_dir.join(filename);
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            panic!(
                "failed to remove stale fork evidence {}: {e}",
                path.display()
            );
        }
    }

    let fork_rpc_url = std::env::var("BLOCKCHAIN_FORK_RPC_URL")
        .expect("BLOCKCHAIN_FORK_RPC_URL must be set when BLOCKCHAIN_FORK_TESTS=1");

    let pg_config = get_postgres_connect_options(None, None, None, None, None);
    let admin_options: PgConnectOptions = pg_config.clone().into();
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options)
        .await
        .expect("Postgres must be reachable when BLOCKCHAIN_FORK_TESTS=1");

    let (_anvil, startup) = start_anvil(&fork_rpc_url)
        .await
        .expect("Anvil must start when BLOCKCHAIN_FORK_TESTS=1");
    let anvil_url = format!("http://127.0.0.1:{}", startup.port);
    let signer = PrivateKeySigner::random();
    let wallet = signer.address();
    let signer_private_key = nautilus_core::hex::encode_prefixed(signer.to_bytes());
    let pancake_order_id = format!("O-FORK-PANCAKE-{}", UUID4::new());
    let limit_order_id = format!("O-FORK-LIMIT-{}", UUID4::new());
    let buy_order_id = format!("O-FORK-BUY-{}", UUID4::new());
    let unprepared_order_id = format!("O-FORK-UNPREPARED-{}", UUID4::new());
    let swap_order_id = format!("O-FORK-SWAP-{}", UUID4::new());
    fund_anvil_wallet(&anvil_url, wallet).await;

    ensure_execution_schema(&admin_pool).await;

    let rpc_client = Arc::new(BlockchainHttpRpcClient::new(anvil_url.clone(), None, None));
    assert_eq!(
        rpc_client.get_balance(&wallet, None).await.unwrap(),
        U256::from(FUND_AMOUNT_WEI)
    );
    let erc20 = Erc20Contract::new(rpc_client.clone(), true);

    let pool = weth_usdc_pool();
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache.borrow_mut().add_pool(pool.clone()).unwrap();
    let core = ExecutionClientCore::new(
        TraderId::from("TRADER-001"),
        ClientId::from("BLOCKCHAIN-FORK-001"),
        *BLOCKCHAIN_VENUE,
        OmsType::Netting,
        AccountId::from("BLOCKCHAIN-FORK-001"),
        AccountType::Wallet,
        None,
        cache.clone(),
    );
    let config = BlockchainExecutionClientConfig::builder()
        .trader_id(TraderId::from("TRADER-001"))
        .client_id(AccountId::from("BLOCKCHAIN-FORK-001"))
        .chain(chains::ARBITRUM.clone())
        .wallet_address(wallet.to_string())
        .http_rpc_url(anvil_url.clone())
        .signer_private_key_env(SIGNER_ENV.to_string())
        .router_addresses(vec![ROUTER.to_string()])
        .weth_address(WETH.to_string())
        .unlimited_approval(true)
        .max_fee_per_gas_wei(100_000_000_000)
        .base_fee_buffer_bps(2_000)
        .gas_limit(5_000_000)
        .gas_buffer_bps(2_000)
        .allowed_token_pairs(vec![(WETH.to_string(), USDC.to_string())])
        .slippage_bps(SLIPPAGE_BPS)
        .max_slippage_bps(200)
        .max_order_amount(1_000_000_000_000_000_000)
        .deadline_seconds(300)
        .max_quote_age_blocks(100)
        .receipt_timeout_secs(60)
        .postgres_cache_database_config(pg_config)
        .build();

    // SAFETY: this opt-in test runs in its own process and no other thread reads this variable.
    unsafe { std::env::set_var(SIGNER_ENV, signer_private_key) };

    let mut client = BlockchainExecutionClient::new(core, config).unwrap();
    let (event_sender, mut event_receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(event_sender);
    client.start().unwrap();
    client.connect().await.unwrap();

    // Build a live pool profiler from the fork's current on-chain state so submit_order can
    // quote; the synthetic full-range snapshot carries the pool's real active liquidity
    let (snapshot, quoted_out) = build_full_range_snapshot(&rpc_client, &pool).await;
    let mut profiler = PoolProfiler::new(Arc::new(pool.clone()));
    profiler.restore_from_snapshot(snapshot).unwrap();

    let min_amount_out = quoted_out * U256::from(10_000 - SLIPPAGE_BPS) / U256::from(10_000);
    assert!(min_amount_out > U256::ZERO);
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // A real PancakeSwap V3 WETH/USDC pool is a valid data instrument but outside this
    // execution slice. Direct submission also fails closed if engine routing is bypassed.
    let pancake_instrument_id: InstrumentId = format!(
        "{}.Arbitrum:PancakeSwapV3",
        PANCAKE_WETH_USDC_POOL.to_checksum(None)
    )
    .parse()
    .unwrap();
    assert!(
        !rpc_client
            .get_code(&PANCAKE_WETH_USDC_POOL)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!client.handles_order_venue(pancake_instrument_id.venue));
    let pancake_order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(pancake_instrument_id)
        .client_order_id(ClientOrderId::new_checked(&pancake_order_id).unwrap())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(SWAP_AMOUNT))
        .build();
    let pancake_denial_reason = submit_and_expect_denial(
        &client,
        &cache,
        &mut event_receiver,
        &rpc_client,
        &admin_pool,
        wallet,
        pancake_order,
        "only UniswapV3 is supported",
    )
    .await;

    // Unsupported order shapes must be denied before persistence or nonce consumption.
    let limit_order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(pool.instrument_id)
        .client_order_id(ClientOrderId::new_checked(&limit_order_id).unwrap())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(SWAP_AMOUNT))
        .price(Price::from("2000"))
        .build();
    let limit_denial_reason = submit_and_expect_denial(
        &client,
        &cache,
        &mut event_receiver,
        &rpc_client,
        &admin_pool,
        wallet,
        limit_order,
        "only Market is supported",
    )
    .await;

    let buy_order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(pool.instrument_id)
        .client_order_id(ClientOrderId::new_checked(&buy_order_id).unwrap())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(SWAP_AMOUNT))
        .build();
    let buy_denial_reason = submit_and_expect_denial(
        &client,
        &cache,
        &mut event_receiver,
        &rpc_client,
        &admin_pool,
        wallet,
        buy_order,
        "only Sell is supported",
    )
    .await;

    // The supported order shape still fails closed until the operator has approved the router.
    // This exercises the asynchronous on-chain pre-trade path and proves it releases the slot.
    let unprepared_order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(pool.instrument_id)
        .client_order_id(ClientOrderId::new_checked(&unprepared_order_id).unwrap())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(SWAP_AMOUNT))
        .build();
    let unprepared_denial_reason = submit_and_expect_denial(
        &client,
        &cache,
        &mut event_receiver,
        &rpc_client,
        &admin_pool,
        wallet,
        unprepared_order,
        "approve the router explicitly before submitting",
    )
    .await;

    // Before setup the wallet has no WETH and no router allowance: not ready
    let report = client.preflight(&pool.instrument_id).await.unwrap();
    assert!(!report.ready);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.contains("No router allowance")),
        "issues: {:?}",
        report.issues
    );

    let weth_address = WETH.parse().unwrap();
    let weth_balance_before = erc20.balance_of(&weth_address, &wallet).await.unwrap();

    // Wrap native currency into WETH
    let wrap_hash = client.wrap(U256::from(WRAP_AMOUNT_WEI)).await.unwrap();
    let wrap_receipt = rpc_client
        .get_transaction_receipt(&wrap_hash)
        .await
        .unwrap()
        .unwrap();
    assert!(wrap_receipt.status);
    assert!(wrap_receipt.gas_used > 0);
    let weth_balance_after = erc20.balance_of(&weth_address, &wallet).await.unwrap();
    assert_eq!(
        weth_balance_after,
        weth_balance_before
            .checked_add(U256::from(WRAP_AMOUNT_WEI))
            .unwrap()
    );

    // Approve the router; unlimited policy applies regardless of the requested amount
    let approve_hash = client
        .approve(
            WETH.parse().unwrap(),
            U256::from(1_000u64),
            ROUTER.parse().unwrap(),
        )
        .await
        .unwrap();
    let approve_receipt = rpc_client
        .get_transaction_receipt(&approve_hash)
        .await
        .unwrap()
        .unwrap();
    assert!(approve_receipt.status);
    assert!(approve_receipt.gas_used > 0);
    let allowance = erc20
        .allowance(&WETH.parse().unwrap(), &wallet, &ROUTER.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(allowance, U256::MAX);

    // After setup the wallet is ready
    let report = client.preflight(&pool.instrument_id).await.unwrap();
    assert!(report.ready, "issues: {:?}", report.issues);

    // Sell the wrapped WETH for USDC through the order path
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(pool.instrument_id)
        .client_order_id(ClientOrderId::new_checked(&swap_order_id).unwrap())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(SWAP_AMOUNT))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let usdc_address: Address = USDC.parse().unwrap();
    let usdc_balance_before = erc20.balance_of(&usdc_address, &wallet).await.unwrap();

    client.submit_order(submit_command(&order)).unwrap();

    // The submission runs on a spawned task. The durable finality state and fill marker
    // prove the irreversible event completed.
    let swap_hash_string = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let row: Option<(String, String, bool)> = sqlx::query_as(
                "SELECT hash.transaction_hash, intent.status, intent.fill_emitted \
                 FROM execution_intent AS intent \
                 JOIN execution_transaction_hash AS hash \
                   ON hash.intent_id = intent.id AND hash.current \
                 WHERE intent.chain_id = 42161 AND intent.client_order_id = $1",
            )
            .bind(&swap_order_id)
            .fetch_optional(&admin_pool)
            .await
            .unwrap();

            if let Some((hash, status, fill_emitted)) = row
                && status == "finalized"
                && fill_emitted
            {
                break hash;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap();

    let swap_receipt = rpc_client
        .get_transaction_receipt(&swap_hash_string.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(swap_receipt.status);
    assert!(swap_receipt.gas_used > 0);

    // Lifecycle events: submitted after broadcast acceptance and filled only after finality.
    let mut saw_submitted = false;
    let mut saw_filled = false;

    while let Ok(event) = event_receiver.try_recv() {
        match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(e)) => {
                assert_eq!(e.client_order_id.as_str(), swap_order_id);
                saw_submitted = true;
            }
            ExecutionEvent::Order(OrderEventAny::Filled(e)) => {
                assert_eq!(e.client_order_id.as_str(), swap_order_id);
                assert_eq!(e.last_qty, Quantity::from(SWAP_AMOUNT));
                assert_eq!(e.venue_order_id.as_str(), swap_hash_string);
                assert!(e.commission.is_some());
                saw_filled = true;
            }
            _ => {}
        }
    }
    assert!(saw_submitted, "expected an OrderSubmitted event");
    assert!(saw_filled, "expected an OrderFilled event after finality");

    // Observed asset delta: exact WETH spend, USDC received within the configured protection
    let weth_balance_after_swap = erc20.balance_of(&weth_address, &wallet).await.unwrap();
    let usdc_balance_after = erc20.balance_of(&usdc_address, &wallet).await.unwrap();
    let weth_spent = weth_balance_after
        .checked_sub(weth_balance_after_swap)
        .unwrap();
    let usdc_received = usdc_balance_after.checked_sub(usdc_balance_before).unwrap();
    assert_eq!(weth_spent, U256::from(WRAP_AMOUNT_WEI));
    assert_eq!(usdc_received, quoted_out);
    assert!(usdc_received >= min_amount_out);

    // All transaction records persisted and marked finalized.
    for (hash, purpose) in [
        (wrap_hash.to_string(), "wrap"),
        (approve_hash.to_string(), "approve"),
        (swap_hash_string.clone(), "swap"),
    ] {
        let (record_purpose, record_status): (String, String) = sqlx::query_as(
            "SELECT intent.purpose, intent.status \
             FROM execution_intent AS intent \
             JOIN execution_transaction_hash AS hash ON hash.intent_id = intent.id \
             WHERE hash.chain_id = 42161 AND hash.transaction_hash = $1",
        )
        .bind(hash)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(record_purpose, purpose);
        assert_eq!(record_status, "finalized");
    }

    let finality_transitions: Vec<String> = sqlx::query_scalar(
        "SELECT transition.to_status \
         FROM execution_transaction_transition AS transition \
         JOIN execution_intent AS intent ON intent.id = transition.intent_id \
         WHERE intent.chain_id = 42161 AND intent.client_order_id = $1 \
         ORDER BY transition.id",
    )
    .bind(&swap_order_id)
    .fetch_all(&admin_pool)
    .await
    .unwrap();
    assert!(
        finality_transitions
            .iter()
            .any(|status| status == "included")
    );
    assert!(
        finality_transitions
            .iter()
            .any(|status| status == "finalized")
    );

    // Reconnect after the terminal marker. Reconciliation must emit no duplicate order event
    // and must not consume another signer nonce.
    let nonce_before_reconnect = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();
    client.disconnect().await.unwrap();
    client.connect().await.unwrap();
    let nonce_after_reconnect = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();
    assert_eq!(nonce_after_reconnect, nonce_before_reconnect);

    while let Ok(event) = event_receiver.try_recv() {
        assert!(
            !matches!(event, ExecutionEvent::Order(_)),
            "reconciliation emitted a duplicate order event: {event:?}"
        );
    }

    // Evidence packet
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_or_else(
            |_| "unknown".to_string(),
            |output| String::from_utf8_lossy(&output.stdout).trim().to_string(),
        );
    let patch_sha = git_diff_sha256(&["--cached", "HEAD", "--"]);
    let worktree_patch_sha = git_diff_sha256(&["HEAD", "--"]);

    std::fs::create_dir_all(&evidence_dir).unwrap();
    let run_json = serde_json::json!({
        "repository_commit": commit,
        "repository_patch_sha256": patch_sha,
        "repository_worktree_patch_sha256": worktree_patch_sha,
        "fork_block": FORK_BLOCK,
        "chain_id": CHAIN_ID,
        "anvil_version": startup.version,
        "pre_submission_denials": [
            {
                "client_order_id": pancake_order_id,
                "dex": "PancakeSwapV3",
                "pool_address": PANCAKE_WETH_USDC_POOL.to_checksum(None),
                "order_type": "Market",
                "order_side": "Sell",
                "reason": pancake_denial_reason,
                "nonce_unchanged": true,
                "persisted_intent": false,
            },
            {
                "client_order_id": limit_order_id,
                "dex": "UniswapV3",
                "order_type": "Limit",
                "order_side": "Sell",
                "reason": limit_denial_reason,
                "nonce_unchanged": true,
                "persisted_intent": false,
            },
            {
                "client_order_id": buy_order_id,
                "dex": "UniswapV3",
                "order_type": "Market",
                "order_side": "Buy",
                "reason": buy_denial_reason,
                "nonce_unchanged": true,
                "persisted_intent": false,
            },
            {
                "client_order_id": unprepared_order_id,
                "dex": "UniswapV3",
                "order_type": "Market",
                "order_side": "Sell",
                "reason": unprepared_denial_reason,
                "nonce_unchanged": true,
                "persisted_intent": false,
            },
        ],
        "wrap": {
            "transaction_hash": wrap_hash.to_string(),
            "gas_used": wrap_receipt.gas_used,
            "status": wrap_receipt.status,
        },
        "approve": {
            "transaction_hash": approve_hash.to_string(),
            "gas_used": approve_receipt.gas_used,
            "status": approve_receipt.status,
        },
        "swap": {
            "client_order_id": swap_order_id,
            "transaction_hash": swap_hash_string,
            "receipt_status": swap_receipt.status,
            "block_number": swap_receipt.block_number,
            "gas_used": swap_receipt.gas_used,
            "configured_protections": {
                "slippage_bps": SLIPPAGE_BPS,
                "max_slippage_bps": 200,
                "max_order_amount": "1000000000000000000",
                "deadline_seconds": 300,
                "max_quote_age_blocks": 100,
                "receipt_timeout_secs": 60,
                "max_fee_per_gas_wei": 100_000_000_000u64,
                "gas_limit": 5_000_000,
                "amount_in": WRAP_AMOUNT_WEI.to_string(),
                "quoted_amount_out": quoted_out.to_string(),
                "min_amount_out": min_amount_out.to_string(),
            },
            "observed_asset_delta": {
                "weth_spent": weth_spent.to_string(),
                "usdc_received": usdc_received.to_string(),
            },
        },
    });
    let run_path = evidence_dir.join("run.json");
    std::fs::write(&run_path, serde_json::to_string_pretty(&run_json).unwrap()).unwrap();
    let run_sha = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA256,
        &std::fs::read(&run_path).unwrap(),
    );
    let hashes_path = evidence_dir.join("SHA256SUMS");
    std::fs::write(
        &hashes_path,
        format!(
            "{}  run.json\n",
            nautilus_core::hex::encode(run_sha.as_ref())
        ),
    )
    .unwrap();
    eprintln!(
        "Fork test evidence packet written to {}",
        evidence_dir.display()
    );

    unsafe { std::env::remove_var(SIGNER_ENV) };
}

#[expect(
    clippy::too_many_arguments,
    reason = "the fork denial assertion checks client, event, RPC, and persistence boundaries"
)]
async fn submit_and_expect_denial(
    client: &BlockchainExecutionClient,
    cache: &Rc<RefCell<Cache>>,
    event_receiver: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    rpc_client: &BlockchainHttpRpcClient,
    admin_pool: &PgPool,
    wallet: Address,
    order: OrderAny,
    expected_reason: &str,
) -> String {
    while event_receiver.try_recv().is_ok() {}

    let client_order_id = order.client_order_id();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    let nonce_before = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();

    client.submit_order(submit_command(&order)).unwrap();

    let denied = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match event_receiver.recv().await {
                Some(ExecutionEvent::Order(OrderEventAny::Denied(denied))) => break denied,
                Some(ExecutionEvent::Order(event)) => {
                    panic!("expected OrderDenied for {client_order_id}, was {event:?}")
                }
                Some(_) => {}
                None => panic!("execution event channel closed before OrderDenied"),
            }
        }
    })
    .await
    .expect("OrderDenied was not emitted within five seconds");

    assert_eq!(denied.client_order_id, client_order_id);
    assert!(
        denied.reason.as_str().contains(expected_reason),
        "was: {}",
        denied.reason
    );
    let nonce_after = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();
    assert_eq!(nonce_after, nonce_before);

    let intent_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_intent WHERE chain_id = 42161 AND client_order_id = $1",
    )
    .bind(client_order_id.as_str())
    .fetch_one(admin_pool)
    .await
    .unwrap();
    assert_eq!(intent_count, 0);

    denied.reason.to_string()
}

fn submit_command(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("BLOCKCHAIN-FORK-001")),
        StrategyId::from("S-001"),
        order.instrument_id(),
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    )
}
