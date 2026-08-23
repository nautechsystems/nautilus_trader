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

//! Opt-in live Arbitrum Uniswap V3 SELL then BUY smoke.
//!
//! Broadcasts real transactions. Gated behind `BLOCKCHAIN_LIVE_SMOKE=1`. Requires
//! `POLYGON_PRIVATE_KEY` and a reachable Postgres. Never runs in default CI.

#![cfg(feature = "hypersync")]

use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

use alloy::primitives::{Address, U256, address};
use nautilus_blockchain::{
    config::{BlockchainExecutionClientConfig, QuoteSpendLimit},
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
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    orders::{Order, OrderAny, OrderTestBuilder},
    types::Quantity,
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const SIGNER_ENV: &str = "POLYGON_PRIVATE_KEY";
const WALLET: &str = "0x4447034f24F4A2E59EFdBcF9E6Dc26Aa6D354F5A";
const CLIENT_NAME: &str = "BLOCKCHAIN-LIVE-001";
const ROUTER: &str = "0xE592427A0AEce92De3Edee1F18E0157C05861564";
const WETH: &str = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1";
const USDC: &str = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831";
const SELL_AMOUNT: &str = "0.0001";
const BUY_AMOUNT: &str = "0.00005";
const SLIPPAGE_BPS: u32 = 50;
const FINALITY_TIMEOUT: Duration = Duration::from_secs(1_800);
const DEFAULT_RPC: &str = "https://arb1.arbitrum.io/rpc";

#[tokio::test]
async fn live_arbitrum_uniswap_v3_sell_then_buy() {
    if std::env::var("BLOCKCHAIN_LIVE_SMOKE").as_deref() != Ok("1") {
        eprintln!("BLOCKCHAIN_LIVE_SMOKE is not 1; skipping live smoke");
        return;
    }
    std::env::var(SIGNER_ENV).expect("POLYGON_PRIVATE_KEY must be set for the live smoke");

    let rpc_url =
        std::env::var("ARBITRUM_RPC_HTTP_URL").unwrap_or_else(|_| DEFAULT_RPC.to_string());
    let pg_config = get_postgres_connect_options(None, None, None, None, None);
    let admin_options: PgConnectOptions = pg_config.clone().into();
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options)
        .await
        .expect("Postgres must be reachable for the live smoke");
    ensure_execution_schema(&admin_pool).await;

    let wallet: Address = WALLET.parse().unwrap();
    let rpc_client = Arc::new(BlockchainHttpRpcClient::new(rpc_url.clone(), None, None));
    let erc20 = Erc20Contract::new(rpc_client.clone(), true);
    let pool = weth_usdc_pool();
    let cache = Rc::new(RefCell::new(Cache::default()));
    cache.borrow_mut().add_pool(pool.clone()).unwrap();
    let core = ExecutionClientCore::new(
        TraderId::from("TRADER-001"),
        ClientId::from(CLIENT_NAME),
        *BLOCKCHAIN_VENUE,
        OmsType::Netting,
        AccountId::from(CLIENT_NAME),
        AccountType::Wallet,
        None,
        cache.clone(),
    );
    let config = BlockchainExecutionClientConfig::builder()
        .trader_id(TraderId::from("TRADER-001"))
        .client_id(AccountId::from(CLIENT_NAME))
        .chain(chains::ARBITRUM.clone())
        .wallet_address(wallet.to_string())
        .http_rpc_url(rpc_url)
        .signer_private_key_env(SIGNER_ENV.to_string())
        .tokens(vec![WETH.to_string(), USDC.to_string()])
        .router_addresses(vec![ROUTER.to_string()])
        .weth_address(WETH.to_string())
        .unlimited_approval(true)
        .max_fee_per_gas_wei(100_000_000_000)
        .base_fee_buffer_bps(2_000)
        .gas_limit(5_000_000)
        .gas_buffer_bps(2_000)
        .allowed_token_pairs(vec![
            (WETH.to_string(), USDC.to_string()),
            (USDC.to_string(), WETH.to_string()),
        ])
        .quote_spend_limits(vec![
            QuoteSpendLimit::builder()
                .token_in(USDC.to_string())
                .token_out(WETH.to_string())
                .spend_token(USDC.to_string())
                .spend_token_decimals(6)
                .max_amount("1000000000".to_string())
                .build(),
        ])
        .slippage_bps(SLIPPAGE_BPS)
        .max_slippage_bps(200)
        .max_order_amount(1_000_000_000_000_000_000)
        .deadline_seconds(300)
        .max_quote_age_blocks(100)
        .receipt_timeout_secs(1_800)
        .postgres_cache_database_config(pg_config)
        .build();

    let mut client = BlockchainExecutionClient::new(core, config).unwrap();
    let (event_sender, mut event_receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(event_sender);
    client.start().unwrap();
    client.connect().await.unwrap();

    let weth: Address = WETH.parse().unwrap();
    let usdc: Address = USDC.parse().unwrap();
    let router: Address = ROUTER.parse().unwrap();
    if erc20.allowance(&weth, &wallet, &router).await.unwrap() == U256::ZERO {
        let hash = client
            .approve(weth, U256::from(1_000u64), router)
            .await
            .unwrap();
        assert!(
            rpc_client
                .get_transaction_receipt(&hash)
                .await
                .unwrap()
                .unwrap()
                .status
        );
    }

    if erc20.allowance(&usdc, &wallet, &router).await.unwrap() == U256::ZERO {
        let hash = client
            .approve(usdc, U256::from(1_000u64), router)
            .await
            .unwrap();
        assert!(
            rpc_client
                .get_transaction_receipt(&hash)
                .await
                .unwrap()
                .unwrap()
                .status
        );
    }

    install_live_profiler(&cache, &rpc_client, &pool).await;
    let weth_before_sell = erc20.balance_of(&weth, &wallet).await.unwrap();
    let usdc_before_sell = erc20.balance_of(&usdc, &wallet).await.unwrap();
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let sell_id = format!("O-LIVE-SELL-{run_id}");
    submit_market(
        &client,
        &cache,
        &pool.instrument_id,
        &sell_id,
        OrderSide::Sell,
        SELL_AMOUNT,
    );
    let sell_hash = await_finalized_fill(&admin_pool, &sell_id).await;
    assert!(
        rpc_client
            .get_transaction_receipt(&sell_hash.parse().unwrap())
            .await
            .unwrap()
            .unwrap()
            .status
    );
    let weth_after_sell = erc20.balance_of(&weth, &wallet).await.unwrap();
    let usdc_after_sell = erc20.balance_of(&usdc, &wallet).await.unwrap();
    assert!(weth_after_sell < weth_before_sell);
    assert!(usdc_after_sell > usdc_before_sell);

    install_live_profiler(&cache, &rpc_client, &pool).await;
    let buy_id = format!("O-LIVE-BUY-{run_id}");
    submit_market(
        &client,
        &cache,
        &pool.instrument_id,
        &buy_id,
        OrderSide::Buy,
        BUY_AMOUNT,
    );
    let buy_hash = await_finalized_fill(&admin_pool, &buy_id).await;
    assert!(
        rpc_client
            .get_transaction_receipt(&buy_hash.parse().unwrap())
            .await
            .unwrap()
            .unwrap()
            .status
    );
    let weth_after_buy = erc20.balance_of(&weth, &wallet).await.unwrap();
    let usdc_after_buy = erc20.balance_of(&usdc, &wallet).await.unwrap();
    assert!(weth_after_buy > weth_after_sell);
    assert!(usdc_after_buy < usdc_after_sell);

    let mut saw_sell_fill = false;
    let mut saw_buy_fill = false;

    while let Ok(event) = event_receiver.try_recv() {
        if let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = event {
            if fill.client_order_id.as_str() == sell_id.as_str() {
                assert_eq!(fill.order_side, OrderSide::Sell);
                saw_sell_fill = true;
            }

            if fill.client_order_id.as_str() == buy_id.as_str() {
                assert_eq!(fill.order_side, OrderSide::Buy);
                saw_buy_fill = true;
            }
        }
    }
    assert!(saw_sell_fill, "missing SELL fill event");
    assert!(saw_buy_fill, "missing BUY fill event");

    client.disconnect().await.unwrap();
}

async fn install_live_profiler(
    cache: &Rc<RefCell<Cache>>,
    rpc_client: &Arc<BlockchainHttpRpcClient>,
    pool: &Pool,
) {
    let snapshot = build_full_range_snapshot(rpc_client, pool).await;
    let mut profiler = PoolProfiler::new(Arc::new(pool.clone()));
    profiler.restore_from_snapshot(snapshot).unwrap();
    cache.borrow_mut().add_pool_profiler(profiler).unwrap();
}

fn submit_market(
    client: &BlockchainExecutionClient,
    cache: &Rc<RefCell<Cache>>,
    instrument_id: &InstrumentId,
    order_id: &str,
    side: OrderSide,
    quantity: &str,
) {
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(*instrument_id)
        .client_order_id(ClientOrderId::new_checked(order_id).unwrap())
        .side(side)
        .quantity(Quantity::from(quantity))
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();
    client.submit_order(submit_command(&order)).unwrap();
}

fn submit_command(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::new(
        TraderId::from("TRADER-001"),
        Some(ClientId::from(CLIENT_NAME)),
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

async fn await_finalized_fill(admin_pool: &sqlx::PgPool, client_order_id: &str) -> String {
    tokio::time::timeout(FINALITY_TIMEOUT, async {
        loop {
            let row: Option<(String, String, bool)> = sqlx::query_as(
                "SELECT hash.transaction_hash, intent.status, intent.fill_emitted \
                 FROM execution_intent AS intent \
                 JOIN execution_transaction_hash AS hash \
                   ON hash.intent_id = intent.id AND hash.current \
                 WHERE intent.chain_id = 42161 AND intent.client_order_id = $1",
            )
            .bind(client_order_id)
            .fetch_optional(admin_pool)
            .await
            .unwrap();

            if let Some((hash, status, fill_emitted)) = row
                && status == "finalized"
                && fill_emitted
            {
                break hash;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for finalized fill {client_order_id}"))
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
        55_000_000,
        weth,
        usdc,
        Some(500),
        Some(10),
        UnixNanos::default(),
    )
}

async fn ensure_execution_schema(admin_pool: &sqlx::PgPool) {
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

async fn build_full_range_snapshot(
    rpc_client: &Arc<BlockchainHttpRpcClient>,
    pool: &Pool,
) -> PoolSnapshot {
    let pool_contract = UniswapV3PoolContract::new(rpc_client.clone(), 100);
    let pool_state = pool_contract
        .get_global_state(&pool.address, None, FeeProtocolEncoding::UniswapV3Packed)
        .await
        .unwrap();
    let head = rpc_client.latest_block().await.unwrap();
    let liquidity = pool_state.liquidity;
    PoolSnapshot::new(
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
    )
}
