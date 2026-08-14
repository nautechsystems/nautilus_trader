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
//! Forks Arbitrum at a fixed block with a local Anvil process and exercises `wrap`,
//! `approve`, `preflight`, and a WETH-to-USDC swap through `submit_order` against localhost
//! only. The fork-source RPC only reads chain state; signed transactions never leave
//! localhost. The suite is gated behind `BLOCKCHAIN_FORK_TESTS=1`, requires
//! `BLOCKCHAIN_FORK_RPC_URL`, and never runs in default CI.

#![cfg(feature = "hypersync")]

use std::{
    cell::RefCell,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

use alloy::primitives::{Address, U256, address};
use nautilus_blockchain::{
    config::BlockchainExecutionClientConfig,
    constants::BLOCKCHAIN_VENUE,
    contracts::{
        erc20::Erc20Contract,
        uniswap_v3_pool::{FeeProtocolEncoding, UniswapV3PoolContract},
    },
    exchanges::arbitrum::UNISWAP_V3,
    execution::client::BlockchainExecutionClient,
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
    defi::{
        Pool, PoolIdentifier, PoolProfiler, Token,
        chain::chains,
        data::block::BlockPosition,
        pool_analysis::{
            position::PoolPosition,
            snapshot::{PoolAnalytics, PoolSnapshot},
        },
        tick_map::tick::PoolTick,
    },
    enums::{AccountType, OmsType, OrderSide, OrderType},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, ClientOrderId, StrategyId, TraderId},
    orders::{Order, OrderTestBuilder},
    types::Quantity,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// Arbitrum block the fork is pinned to.
const FORK_BLOCK: u64 = 489_000_000;
/// Arbitrum chain ID.
const CHAIN_ID: u64 = 42161;
/// Standard Anvil development key #0 (public, never used outside local test chains).
const ANVIL_PRIVATE_KEY: &str =
    "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
const SIGNER_ENV: &str = "BLOCKCHAIN_FORK_TEST_PRIVATE_KEY";
const WALLET: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const ROUTER: &str = "0xE592427A0AEce92De3Edee1F18E0157C05861564";
const WETH: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";
const USDC: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";
const WRAP_AMOUNT_WEI: u128 = 1_000_000_000_000_000;
const SWAP_AMOUNT: &str = "0.001";
const SWAP_ORDER_ID: &str = "O-FORK-SWAP-001";
const SLIPPAGE_BPS: u32 = 50;
const ANVIL_READY_TIMEOUT: Duration = Duration::from_secs(120);

struct AnvilProcess(Child);

impl Drop for AnvilProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Startup information parsed from the Anvil stdout banner.
struct AnvilStartup {
    port: u16,
    version: String,
}

fn weth_usdc_pool() -> Pool {
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

async fn start_anvil(fork_rpc_url: &str) -> Option<(AnvilProcess, AnvilStartup)> {
    let mut child = match Command::new("anvil")
        .arg("--fork-url")
        .arg(fork_rpc_url)
        .arg("--fork-block-number")
        .arg(FORK_BLOCK.to_string())
        .arg("--retries")
        .arg("8")
        .arg("--fork-retry-backoff")
        .arg("1500")
        .arg("--chain-id")
        .arg(CHAIN_ID.to_string())
        .arg("--port")
        .arg("0")
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

    // Additive schema only: create the tables this slice needs when absent, never touch
    // existing data
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
        sqlx::query(statement).execute(&admin_pool).await.unwrap();
    }

    let rpc_client = Arc::new(BlockchainHttpRpcClient::new(anvil_url.clone(), None, None));
    let erc20 = Erc20Contract::new(rpc_client.clone(), true);
    let wallet: Address = WALLET.parse().unwrap();

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
        .wallet_address(WALLET.to_string())
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

    // SAFETY: single-threaded test process and a public test-only key; no other thread reads
    // this variable concurrently
    unsafe { std::env::set_var(SIGNER_ENV, ANVIL_PRIVATE_KEY) };

    let mut client = BlockchainExecutionClient::new(core, config).unwrap();
    let (event_sender, mut event_receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(event_sender);
    client.start().unwrap();
    client.connect().await.unwrap();

    // A failed prior run may leave rows behind. This public Anvil development wallet is reserved
    // for this opt-in test, so remove only its transaction records before reusing its nonces.
    sqlx::query("DELETE FROM execution_transaction WHERE chain_id = 42161 AND wallet_address = $1")
        .bind(WALLET)
        .execute(&admin_pool)
        .await
        .unwrap();

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

    // Build a live pool profiler from the fork's current on-chain state so submit_order can
    // quote. A synthetic full-range position carries the pool's real active liquidity: the
    // tiny swap never crosses a tick, so the local quote matches the fork's execution math
    let pool_contract = UniswapV3PoolContract::new(rpc_client.clone(), 100);
    let pool_state = pool_contract
        .get_global_state(&pool.address, None, FeeProtocolEncoding::UniswapV3Packed)
        .await
        .unwrap();
    let head = rpc_client.latest_block().await.unwrap();
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
            "0x0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            0,
            0,
        ),
        UnixNanos::default(),
        UnixNanos::default(),
    );
    let mut profiler = PoolProfiler::new(Arc::new(pool.clone()));
    profiler.restore_from_snapshot(snapshot).unwrap();

    let quote = profiler
        .swap_exact_in(U256::from(WRAP_AMOUNT_WEI), true, None)
        .unwrap();
    let quoted_out = quote.amount1.unsigned_abs();
    let min_amount_out = quoted_out * U256::from(10_000 - SLIPPAGE_BPS) / U256::from(10_000);
    assert!(min_amount_out > U256::ZERO);
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();

    // Sell the wrapped WETH for USDC through the order path
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(pool.instrument_id)
        .client_order_id(ClientOrderId::from(SWAP_ORDER_ID))
        .side(OrderSide::Sell)
        .quantity(Quantity::from(SWAP_AMOUNT))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let usdc_address: Address = USDC.parse().unwrap();
    let usdc_balance_before = erc20.balance_of(&usdc_address, &wallet).await.unwrap();

    let submit = SubmitOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from("BLOCKCHAIN-FORK-001")),
        StrategyId::from("S-001"),
        pool.instrument_id,
        order.client_order_id(),
        order.init_event().clone(),
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );
    client.submit_order(submit).unwrap();

    // The submission runs on a spawned task; the persisted record reaching `included`
    // proves the swap completed
    let swap_hash_string = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let row: Option<(String, String)> = sqlx::query_as(
                "SELECT transaction_hash, status FROM execution_transaction WHERE chain_id = 42161 AND client_order_id = $1",
            )
            .bind(SWAP_ORDER_ID)
            .fetch_optional(&admin_pool)
            .await
            .unwrap();

            if let Some((hash, status)) = row
                && status == "included"
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

    // Lifecycle events: submitted after broadcast acceptance, and no fill at first inclusion
    let mut saw_submitted = false;

    while let Ok(event) = event_receiver.try_recv() {
        match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(e)) => {
                assert_eq!(e.client_order_id.as_str(), SWAP_ORDER_ID);
                saw_submitted = true;
            }
            ExecutionEvent::Order(OrderEventAny::Filled(_)) => {
                panic!("no fill may be emitted at broadcast or first inclusion")
            }
            _ => {}
        }
    }
    assert!(saw_submitted, "expected an OrderSubmitted event");

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

    // All transaction records persisted and marked included
    for (hash, purpose) in [
        (wrap_hash.to_string(), "wrap"),
        (approve_hash.to_string(), "approve"),
        (swap_hash_string.clone(), "swap"),
    ] {
        let (record_purpose, record_status): (String, String) = sqlx::query_as(
            "SELECT purpose, status FROM execution_transaction WHERE chain_id = 42161 AND transaction_hash = $1",
        )
        .bind(hash)
        .fetch_one(&admin_pool)
        .await
        .unwrap();
        assert_eq!(record_purpose, purpose);
        assert_eq!(record_status, "included");
    }

    // Evidence packet
    let commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .map_or_else(
            |_| "unknown".to_string(),
            |output| String::from_utf8_lossy(&output.stdout).trim().to_string(),
        );
    let patch_sha = Command::new("git")
        .args(["diff", "--binary", "--cached", "HEAD", "--"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || "unknown".to_string(),
            |output| {
                let digest = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &output.stdout);
                nautilus_core::hex::encode(digest.as_ref())
            },
        );

    std::fs::create_dir_all(&evidence_dir).unwrap();
    let run_json = serde_json::json!({
        "repository_commit": commit,
        "repository_patch_sha256": patch_sha,
        "fork_block": FORK_BLOCK,
        "chain_id": CHAIN_ID,
        "anvil_version": startup.version,
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
            "client_order_id": SWAP_ORDER_ID,
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

    // Remove only the rows this test inserted
    for hash in [
        wrap_hash.to_string(),
        approve_hash.to_string(),
        swap_hash_string.clone(),
    ] {
        sqlx::query(
            "DELETE FROM execution_transaction WHERE chain_id = 42161 AND transaction_hash = $1",
        )
        .bind(hash)
        .execute(&admin_pool)
        .await
        .unwrap();
    }

    unsafe { std::env::remove_var(SIGNER_ENV) };
}
