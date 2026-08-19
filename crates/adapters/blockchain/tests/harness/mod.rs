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
    sync::Arc,
    time::{Duration, Instant},
};

use alloy::primitives::{Address, U256, address};
use nautilus_blockchain::{
    contracts::uniswap_v3_pool::{FeeProtocolEncoding, UniswapV3PoolContract},
    exchanges::arbitrum::UNISWAP_V3,
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_core::UnixNanos;
use nautilus_model::defi::{
    Pool, PoolIdentifier, PoolProfiler, Token,
    chain::chains,
    data::block::BlockPosition,
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
pub(crate) const ROUTER: &str = "0xE592427A0AEce92De3Edee1F18E0157C05861564";
pub(crate) const WETH: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";
pub(crate) const USDC: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";
pub(crate) const FUND_AMOUNT_WEI: u128 = 100_000_000_000_000_000_000;
pub(crate) const WRAP_AMOUNT_WEI: u128 = 1_000_000_000_000_000;
pub(crate) const SWAP_AMOUNT: &str = "0.001";
pub(crate) const SLIPPAGE_BPS: u32 = 50;
pub(crate) const ANVIL_READY_TIMEOUT: Duration = Duration::from_secs(120);

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
        .arg("0")
        .arg("--chain-id")
        .arg(CHAIN_ID.to_string())
        .arg("--block-time")
        .arg("1")
        .arg("--mixed-mining")
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
