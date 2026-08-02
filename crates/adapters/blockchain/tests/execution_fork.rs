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
//! `approve`, and `preflight` against localhost only. The fork-source RPC only reads chain
//! state; signed transactions never leave localhost. Gated behind `BLOCKCHAIN_FORK_TESTS=1`
//! and `BLOCKCHAIN_FORK_RPC_URL`; never runs in default CI.

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
    config::BlockchainExecutionClientConfig, constants::BLOCKCHAIN_VENUE,
    contracts::erc20::Erc20Contract, exchanges::arbitrum::UNISWAP_V3,
    execution::client::BlockchainExecutionClient, rpc::http::BlockchainHttpRpcClient,
};
use nautilus_common::{cache::Cache, clients::ExecutionClient};
use nautilus_core::UnixNanos;
use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    defi::{Pool, PoolIdentifier, Token, chain::chains},
    enums::{AccountType, OmsType},
    identifiers::{AccountId, ClientId, TraderId},
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
const WRAP_AMOUNT_WEI: u128 = 1_000_000_000_000_000;
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
            eprintln!("anvil binary not found on PATH; skipping fork test: {e}");
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
            eprintln!("Anvil exited before binding; skipping fork test:\n{output}");
            return None;
        }
        Err(_) => {
            eprintln!("Anvil did not print a listening address in time; skipping fork test");
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
                eprintln!(
                    "Anvil did not become ready within {ANVIL_READY_TIMEOUT:?}; skipping fork test"
                );
                return None;
            }
        }
    }

    Some((AnvilProcess(child), startup))
}

#[tokio::test]
async fn anvil_fork_wrap_approve_and_preflight() {
    if std::env::var("BLOCKCHAIN_FORK_TESTS").as_deref() != Ok("1") {
        eprintln!("BLOCKCHAIN_FORK_TESTS is not 1; skipping fork test");
        return;
    }
    let Ok(fork_rpc_url) = std::env::var("BLOCKCHAIN_FORK_RPC_URL") else {
        eprintln!("BLOCKCHAIN_FORK_RPC_URL is not set; skipping fork test");
        return;
    };

    let pg_config = get_postgres_connect_options(None, None, None, None, None);
    let admin_options: PgConnectOptions = pg_config.clone().into();
    let Some(admin_pool) = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options)
        .await
        .ok()
    else {
        eprintln!("Postgres unavailable; skipping fork test");
        return;
    };

    let Some((_anvil, startup)) = start_anvil(&fork_rpc_url).await else {
        return;
    };
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
        cache,
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
        .postgres_cache_database_config(pg_config)
        .build();

    // SAFETY: single-threaded test process and a public test-only key; no other thread reads
    // this variable concurrently
    unsafe { std::env::set_var(SIGNER_ENV, ANVIL_PRIVATE_KEY) };

    let mut client = BlockchainExecutionClient::new(core, config).unwrap();
    client.connect().await.unwrap();

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

    // Wrap native currency into WETH
    let wrap_hash = client.wrap(U256::from(WRAP_AMOUNT_WEI)).await.unwrap();
    let wrap_receipt = rpc_client
        .get_transaction_receipt(&wrap_hash)
        .await
        .unwrap()
        .unwrap();
    assert!(wrap_receipt.status);
    assert!(wrap_receipt.gas_used > 0);
    let weth_balance = erc20
        .balance_of(&WETH.parse().unwrap(), &wallet)
        .await
        .unwrap();
    assert_eq!(weth_balance, U256::from(WRAP_AMOUNT_WEI));

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

    // Both transaction records persisted and marked included
    for (hash, purpose) in [(wrap_hash, "wrap"), (approve_hash, "approve")] {
        let (record_purpose, record_status): (String, String) = sqlx::query_as(
            "SELECT purpose, status FROM execution_transaction WHERE chain_id = 42161 AND transaction_hash = $1",
        )
        .bind(hash.to_string())
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

    let evidence_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/blockchain-fork-evidence");
    std::fs::create_dir_all(&evidence_dir).unwrap();
    let run_json = serde_json::json!({
        "repository_commit": commit,
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
    for hash in [wrap_hash, approve_hash] {
        sqlx::query(
            "DELETE FROM execution_transaction WHERE chain_id = 42161 AND transaction_hash = $1",
        )
        .bind(hash.to_string())
        .execute(&admin_pool)
        .await
        .unwrap();
    }

    unsafe { std::env::remove_var(SIGNER_ENV) };
}
