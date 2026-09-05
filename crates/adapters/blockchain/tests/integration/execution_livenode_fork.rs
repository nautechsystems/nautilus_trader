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

//! Pinned-block Anvil fork smoke for the full Rust `LiveNode` execution route.
//!
//! Routes a Nautilus SELL market order through the real factory-registered execution client,
//! risk engine, execution engine, and node wiring to a finalized Arbitrum Uniswap V3
//! exact-input swap, then restarts a second node to prove reconnect safety. The operator
//! wrap and approve setup uses the direct client construction shared with `node-wallet`,
//! because those operations precede node routing. The suite is gated behind
//! `BLOCKCHAIN_FORK_TESTS=1`, requires `BLOCKCHAIN_FORK_RPC_URL`, and never runs in
//! default CI.

#![cfg(feature = "hypersync")]

use std::{
    any::Any,
    cell::{Cell, RefCell},
    process::Command,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use alloy::{primitives::U256, signers::local::PrivateKeySigner};
use async_trait::async_trait;
use nautilus_blockchain::{
    config::{BlockchainExecutionClientConfig, BlockchainVerificationConfig, QuoteSpendLimit},
    constants::BLOCKCHAIN_VENUE,
    contracts::{erc20::Erc20Contract, uniswap_v3_pool::UniswapV3PoolContract},
    execution::client::BlockchainExecutionClient,
    factories::BlockchainExecutionClientFactory,
    rpc::http::BlockchainHttpRpcClient,
};
use nautilus_common::{
    actor::DataActor,
    cache::{Cache, CacheView},
    clients::{DataClient, ExecutionClient},
    clock::Clock,
    defi,
    defi::RequestPoolSnapshot,
    factories::{ClientConfig, DataClientFactory},
    live::runner::{get_data_event_sender, replace_exec_event_sender},
    messages::{DataEvent, execution::SubmitOrder},
    msgbus::{self, TypedHandler},
    timer::TimeEvent,
};
use nautilus_infrastructure::sql::pg::{PostgresConnectOptions, get_postgres_connect_options};
use nautilus_live::{
    ExecutionClientCore,
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig, RoutingConfig},
    node::{LiveNode, LiveNodeHandle},
};
use nautilus_model::{
    data::{Data, QuoteTick},
    defi::{DefiData, Pool, PoolProfiler, chain::chains, pool_analysis::snapshot::PoolSnapshot},
    enums::{AccountType, OmsType, OrderSide, OrderType},
    events::{AccountState, OrderEventAny, OrderFilled, OrderSubmitted},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue},
    orders::{Order, OrderTestBuilder},
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::harness::{
    CHAIN_ID, PAYLOAD_DEPLOYMENT_ID, PAYLOAD_KEY_ENV, PAYLOAD_KEY_HEX, ROUTER, SIGNER_ENV,
    SLIPPAGE_BPS, SWAP_AMOUNT, USDC, WETH, WRAP_AMOUNT_WEI, build_full_range_snapshot,
    ensure_execution_schema, fund_anvil_wallet, git_diff_sha256, quote_buy_amount_in, start_anvil,
    start_execution_rpc_topology, weth_usdc_pool,
};

const EXEC_CLIENT_NAME: &str = "BLOCKCHAIN-FORK-001";
const DATA_CLIENT_NAME: &str = "BLOCKCHAIN-FORK-STUB";
/// Timer that gates the swap submission on restored pool data.
const SUBMIT_TIMER: &str = "fork-swap-submit";
/// Timer that stops the reconnect probe node once startup has fully settled.
const PROBE_TIMER: &str = "fork-reconnect-probe";
const WETH_ADDRESS: alloy::primitives::Address =
    alloy::primitives::address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1");
const USDC_ADDRESS: alloy::primitives::Address =
    alloy::primitives::address!("af88d065e77c8cC2239327C5EDb3A432268e5831");

// A minimal data client standing in for the hypersync-backed adapter at the venue boundary.
// The pinned fork cannot serve hypersync's live streams, so this client answers the data
// engine's pool snapshot request with the fork's on-chain state; every engine-side restore
// path it feeds is production code.
struct ForkDataClient {
    client_id: ClientId,
    pool: Pool,
    snapshot: PoolSnapshot,
    quote: QuoteTick,
    connected: Cell<bool>,
}

#[async_trait(?Send)]
impl DataClient for ForkDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        None
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.connected.set(true);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.connected.set(false);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.get()
    }

    fn is_disconnected(&self) -> bool {
        !self.connected.get()
    }

    fn request_pool_snapshot(&self, _cmd: RequestPoolSnapshot) -> anyhow::Result<()> {
        // Enqueued in order: pool definition restores the instrument and pool, the snapshot
        // restores the profiler, and the quote unblocks market-order risk pricing
        let sender = get_data_event_sender();
        sender.send(DataEvent::DeFi(DefiData::Pool(self.pool.clone())))?;
        sender.send(DataEvent::DeFi(DefiData::PoolSnapshot(
            self.snapshot.clone(),
        )))?;
        sender.send(DataEvent::Data(Data::Quote(self.quote)))?;
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct ForkDataClientFactory {
    pool: Pool,
    snapshot: PoolSnapshot,
    quote: QuoteTick,
}

impl DataClientFactory for ForkDataClientFactory {
    fn create(
        &self,
        name: &str,
        _config: &dyn ClientConfig,
        _cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        Ok(Box::new(ForkDataClient {
            client_id: ClientId::from(name),
            pool: self.pool.clone(),
            snapshot: self.snapshot.clone(),
            quote: self.quote,
            connected: Cell::new(false),
        }))
    }

    fn name(&self) -> &'static str {
        DATA_CLIENT_NAME
    }

    fn config_type(&self) -> &'static str {
        "ForkDataClientConfig"
    }
}

#[derive(Debug)]
struct ForkDataClientConfig;

impl ClientConfig for ForkDataClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct SwapStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
    handle: LiveNodeHandle,
    pool_seen: Rc<Cell<bool>>,
    order_events: Rc<RefCell<Vec<OrderEventAny>>>,
    submitted: bool,
    order_side: OrderSide,
}

impl SwapStrategy {
    fn new(
        instrument_id: InstrumentId,
        handle: LiveNodeHandle,
        pool_seen: Rc<Cell<bool>>,
        order_events: Rc<RefCell<Vec<OrderEventAny>>>,
        order_side: OrderSide,
    ) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig::default()),
            instrument_id,
            handle,
            pool_seen,
            order_events,
            submitted: false,
            order_side,
        }
    }
}

impl DataActor for SwapStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_pool(self.instrument_id, None, None);
        self.clock()
            .set_timer_ns(SUBMIT_TIMER, 10_000_000, None, None, None, None, None)?;
        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        // The repeating timer retries until the pool definition event has flowed through
        // the data engine; the snapshot restore is queued ahead of the next timer fire
        if self.submitted || !self.pool_seen.get() {
            return Ok(());
        }

        self.submitted = true;
        self.clock().cancel_timer(SUBMIT_TIMER);
        let order = self.order().market(
            self.instrument_id,
            self.order_side,
            Quantity::from(SWAP_AMOUNT),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)?;
        Ok(())
    }
}

nautilus_strategy!(SwapStrategy, {
    fn on_order_submitted(&mut self, event: OrderSubmitted) {
        self.order_events
            .borrow_mut()
            .push(OrderEventAny::Submitted(event));
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        self.order_events
            .borrow_mut()
            .push(OrderEventAny::Filled(event.clone()));
        self.handle.stop();
    }
});

#[derive(Debug)]
struct ReconnectProbeStrategy {
    core: StrategyCore,
    handle: LiveNodeHandle,
}

impl ReconnectProbeStrategy {
    fn new(handle: LiveNodeHandle) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig::default()),
            handle,
        }
    }
}

impl DataActor for ReconnectProbeStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.clock()
            .set_timer_ns(PROBE_TIMER, 2_000_000_000, None, None, None, None, None)?;
        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        self.handle.stop();
        Ok(())
    }
}

nautilus_strategy!(ReconnectProbeStrategy);

/// Builds a LiveNode with the factory-registered fork clients and venue routing.
#[expect(
    clippy::too_many_arguments,
    reason = "the builder carries the full fork fixture: signer, endpoints, pool state, and routing"
)]
fn build_fork_node(
    name: &str,
    wallet: alloy::primitives::Address,
    execution_rpc_url: String,
    verification: BlockchainVerificationConfig,
    pg_config: PostgresConnectOptions,
    pool: Pool,
    snapshot: PoolSnapshot,
    quote: QuoteTick,
    venue: String,
) -> anyhow::Result<LiveNode> {
    let config = LiveNodeConfig {
        timeout_connection: Duration::from_secs(120),
        timeout_reconciliation: Duration::from_secs(120),
        timeout_disconnection: Duration::from_secs(30),
        delay_post_stop: Duration::from_millis(200),
        exec_engine: LiveExecutionEngineConfig {
            inflight_check_interval_ms: 0,
            allow_overfills: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let routing = RoutingConfig {
        default: false,
        venues: Some(vec![venue]),
    };
    LiveNodeBuilder::from_config(config)?
        .with_name(name)
        .add_data_client_with_routing(
            Some(DATA_CLIENT_NAME.to_string()),
            Box::new(ForkDataClientFactory {
                pool,
                snapshot,
                quote,
            }),
            Box::new(ForkDataClientConfig),
            routing.clone(),
        )?
        .add_exec_client_with_routing(
            Some(EXEC_CLIENT_NAME.to_string()),
            Box::new(BlockchainExecutionClientFactory::new()),
            Box::new(execution_config(
                wallet,
                execution_rpc_url,
                verification,
                pg_config,
            )),
            routing,
        )?
        .build()
}

fn execution_config(
    wallet: alloy::primitives::Address,
    execution_rpc_url: String,
    verification: BlockchainVerificationConfig,
    pg_config: PostgresConnectOptions,
) -> BlockchainExecutionClientConfig {
    BlockchainExecutionClientConfig::builder()
        .client_id(AccountId::from(EXEC_CLIENT_NAME))
        .chain(chains::ARBITRUM.clone())
        .wallet_address(wallet.to_string())
        .http_rpc_url(execution_rpc_url.into())
        .verification(verification)
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
        .receipt_timeout_secs(60)
        .payload_key_env(PAYLOAD_KEY_ENV.to_string())
        .payload_deployment_id(PAYLOAD_DEPLOYMENT_ID.to_string())
        .postgres_cache_database_config(pg_config)
        .build()
}

/// Derives a USDC-per-WETH quote price from the pool's on-chain square-root price, keeping
/// the tick honest against the fork state the snapshot restore will install.
async fn fork_quote_tick(rpc_client: &Arc<BlockchainHttpRpcClient>, pool: &Pool) -> QuoteTick {
    use nautilus_blockchain::contracts::uniswap_v3_pool::FeeProtocolEncoding;

    let pool_contract = UniswapV3PoolContract::new(rpc_client.clone(), 100);
    let state = pool_contract
        .get_global_state(&pool.address, None, FeeProtocolEncoding::UniswapV3Packed)
        .await
        .unwrap();

    let sqrt = U256::from(state.price_sqrt_ratio_x96);
    let numerator = sqrt * sqrt * U256::from(1_000_000_000_000u64);
    let denominator = U256::from(1u8) << 192;
    let whole = numerator / denominator;
    let frac = (numerator % denominator) * U256::from(1_000_000u64) / denominator;
    let price = Price::from(&format!("{whole}.{frac:06}"));

    let ts = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    QuoteTick::new(
        pool.instrument_id,
        price,
        price,
        Quantity::from("1"),
        Quantity::from("1"),
        ts,
        ts,
    )
}

#[tokio::test]
async fn anvil_fork_livenode_routed_swap_and_reconnect() {
    if std::env::var("BLOCKCHAIN_FORK_TESTS").as_deref() != Ok("1") {
        eprintln!("BLOCKCHAIN_FORK_TESTS is not 1; skipping fork test");
        return;
    }

    let evidence_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../target/blockchain-fork-evidence");
    for filename in ["livenode-run.json", "SHA256SUMS.livenode"] {
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
    let rpc_topology = start_execution_rpc_topology(&anvil_url).await;
    let signer = PrivateKeySigner::random();
    let wallet = signer.address();
    let signer_private_key = nautilus_core::hex::encode_prefixed(signer.to_bytes());
    fund_anvil_wallet(&anvil_url, wallet).await;
    ensure_execution_schema(&admin_pool).await;

    let rpc_client = Arc::new(BlockchainHttpRpcClient::new(anvil_url.clone(), None, None));
    let erc20 = Erc20Contract::new(rpc_client.clone(), true);
    let pool = weth_usdc_pool();
    let (snapshot, quoted_out) = build_full_range_snapshot(&rpc_client, &pool).await;
    let min_amount_out = quoted_out * U256::from(10_000 - SLIPPAGE_BPS) / U256::from(10_000);
    let quote = fork_quote_tick(&rpc_client, &pool).await;
    let venue_string = pool.instrument_id.venue.to_string();

    // SAFETY: this opt-in test runs in its own process; the reconnect thread only reads this
    // variable after this spawn point and joins before the removal below, so accesses are
    // ordered by thread lifecycle.
    unsafe { std::env::set_var(SIGNER_ENV, signer_private_key) };
    // SAFETY: the same process isolation and access ordering apply to this variable
    unsafe { std::env::set_var(PAYLOAD_KEY_ENV, PAYLOAD_KEY_HEX) };

    // Operator setup: explicit wrap and router approval before any node runs
    let operator_cache = Rc::new(RefCell::new(Cache::default()));
    operator_cache.borrow_mut().add_pool(pool.clone()).unwrap();
    let operator_core = ExecutionClientCore::new(
        TraderId::from("TRADER-001"),
        ClientId::from(EXEC_CLIENT_NAME),
        *BLOCKCHAIN_VENUE,
        OmsType::Netting,
        AccountId::from(EXEC_CLIENT_NAME),
        AccountType::Wallet,
        None,
        operator_cache,
    );
    let mut operator = BlockchainExecutionClient::new(
        operator_core,
        execution_config(
            wallet,
            rpc_topology.authoritative_url(),
            rpc_topology.verification(),
            pg_config.clone(),
        ),
    )
    .unwrap();
    // The operator setup runs before any node runner exists on this thread, so it needs its
    // own execution event sink; the node runner replaces this sender with its own
    let (operator_event_sender, _operator_event_receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(operator_event_sender);
    operator.start().unwrap();
    operator.protect_payload_storage().await.unwrap();
    operator.connect().await.unwrap();

    let wrap_hash = operator.wrap(U256::from(WRAP_AMOUNT_WEI)).await.unwrap();
    let wrap_receipt = rpc_client
        .get_transaction_receipt(&wrap_hash)
        .await
        .unwrap()
        .unwrap();
    assert!(wrap_receipt.status);
    assert!(wrap_receipt.gas_used > 0);

    let approve_hash = operator
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
        .allowance(&WETH_ADDRESS, &wallet, &ROUTER.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(allowance, U256::MAX);

    let report = operator.preflight(&pool.instrument_id).await.unwrap();
    assert!(report.ready, "issues: {:?}", report.issues);
    drop(operator);

    let usdc_balance_before = erc20.balance_of(&USDC_ADDRESS, &wallet).await.unwrap();
    let weth_balance_before = erc20.balance_of(&WETH_ADDRESS, &wallet).await.unwrap();

    // Node one: factory-registered clients, venue-routed execution, real strategy submission
    let mut node = build_fork_node(
        "BlockchainForkLiveNode",
        wallet,
        rpc_topology.authoritative_url(),
        rpc_topology.verification(),
        pg_config.clone(),
        pool.clone(),
        snapshot.clone(),
        quote,
        venue_string.clone(),
    )
    .unwrap();

    let handle = node.handle();
    let pool_seen = Rc::new(Cell::new(false));
    let order_events: Rc<RefCell<Vec<OrderEventAny>>> = Rc::new(RefCell::new(Vec::new()));
    let account_states: Rc<RefCell<Vec<AccountState>>> = Rc::new(RefCell::new(Vec::new()));

    msgbus::subscribe_defi_pools(
        defi::switchboard::get_defi_pool_topic(pool.instrument_id).into(),
        TypedHandler::<Pool>::from({
            let pool_seen = pool_seen.clone();
            move |_pool: &Pool| pool_seen.set(true)
        }),
        None,
    );
    msgbus::subscribe_account_state(
        "events.account.*".into(),
        TypedHandler::<AccountState>::from({
            let account_states = account_states.clone();
            move |state: &AccountState| account_states.borrow_mut().push(state.clone())
        }),
        None,
    );

    node.add_strategy(SwapStrategy::new(
        pool.instrument_id,
        handle.clone(),
        pool_seen.clone(),
        order_events.clone(),
        OrderSide::Sell,
    ))
    .unwrap();

    tokio::time::timeout(Duration::from_secs(300), node.run())
        .await
        .expect("node one must run to completion after the fill")
        .unwrap();

    // Lifecycle: submitted after broadcast acceptance, filled only after canonical finality
    let events = order_events.borrow().clone();
    assert_eq!(events.len(), 2, "was: {events:?}");
    let swap_order_id = match &events[0] {
        OrderEventAny::Submitted(e) => e.client_order_id,
        other => panic!("expected OrderSubmitted, was {other:?}"),
    };
    let OrderEventAny::Filled(fill) = &events[1] else {
        panic!("expected OrderFilled, was {:?}", events[1]);
    };
    assert_eq!(fill.last_qty, Quantity::from(SWAP_AMOUNT));
    assert!(fill.commission.is_some(), "gas commission missing");
    let fill = fill.clone();
    let swap_hash_string = fill.venue_order_id.as_str().to_string();

    let swap_receipt = rpc_client
        .get_transaction_receipt(&swap_hash_string.parse().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(swap_receipt.status);
    assert!(swap_receipt.gas_used > 0);

    // Exact observed asset delta against the pre-run balances
    let weth_balance_after = erc20.balance_of(&WETH_ADDRESS, &wallet).await.unwrap();
    let usdc_balance_after = erc20.balance_of(&USDC_ADDRESS, &wallet).await.unwrap();
    let weth_spent = weth_balance_before.checked_sub(weth_balance_after).unwrap();
    let usdc_received = usdc_balance_after.checked_sub(usdc_balance_before).unwrap();
    assert_eq!(weth_spent, U256::from(WRAP_AMOUNT_WEI));
    assert_eq!(usdc_received, quoted_out);
    assert!(usdc_received >= min_amount_out);

    // Durable finality and fill marker for the routed order
    let (status, fill_emitted): (String, bool) = sqlx::query_as(
        "SELECT intent.status, intent.fill_emitted \
         FROM execution_intent AS intent \
         JOIN execution_transaction_hash AS hash \
           ON hash.intent_id = intent.id AND hash.current \
         WHERE intent.chain_id = 42161 AND intent.client_order_id = $1",
    )
    .bind(swap_order_id.as_str())
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(status, "finalized");
    assert!(fill_emitted);

    let finality_transitions: Vec<String> = sqlx::query_scalar(
        "SELECT transition.to_status \
         FROM execution_transaction_transition AS transition \
         JOIN execution_intent AS intent ON intent.id = transition.intent_id \
         WHERE intent.chain_id = 42161 AND intent.client_order_id = $1 \
         ORDER BY transition.id",
    )
    .bind(swap_order_id.as_str())
    .fetch_all(&admin_pool)
    .await
    .unwrap();
    assert_eq!(
        finality_transitions,
        ["prepared", "signed", "broadcast", "finalized"]
    );

    // Refreshed wallet account state: the connect snapshot plus the post-fill republication
    let account_state_count = account_states.borrow().len();
    assert!(account_state_count >= 2, "was: {account_state_count}");

    let nonce_before_reconnect = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();
    // Scoped to this run's random wallet so a concurrently running fork suite sharing the
    // database cannot perturb the counts
    let wallet_string = wallet.to_string();
    let intents_before_reconnect: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_intent WHERE chain_id = 42161 AND wallet_address = $1",
    )
    .bind(&wallet_string)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    let hashes_before_reconnect: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_transaction_hash AS hash \
         JOIN execution_intent AS intent ON intent.id = hash.intent_id \
         WHERE hash.chain_id = 42161 AND intent.wallet_address = $1",
    )
    .bind(&wallet_string)
    .fetch_one(&admin_pool)
    .await
    .unwrap();

    // Node two: reconnect on a separate thread with isolated thread-local message bus and
    // event senders, mirroring a process restart
    let reconnect_url = rpc_topology.authoritative_url();
    let reconnect_verification = rpc_topology.verification();
    let reconnect_pg = pg_config.clone();
    let reconnect_pool = pool.clone();
    let reconnect_snapshot = snapshot.clone();
    let reconnect_venue = venue_string.clone();
    let (reconnect_tx, reconnect_rx) = std::sync::mpsc::channel::<anyhow::Result<()>>();

    std::thread::Builder::new()
        .name("fork-livenode-reconnect".to_string())
        .spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let mut node = build_fork_node(
                        "BlockchainForkReconnectNode",
                        wallet,
                        reconnect_url,
                        reconnect_verification,
                        reconnect_pg,
                        reconnect_pool,
                        reconnect_snapshot,
                        quote,
                        reconnect_venue,
                    )
                    .unwrap();

                    let handle = node.handle();
                    node.add_strategy(ReconnectProbeStrategy::new(handle))
                        .unwrap();

                    tokio::time::timeout(Duration::from_secs(300), node.run())
                        .await
                        .expect("node two must run to completion")
                });

            let _ = reconnect_tx.send(result.map_err(|e| anyhow::anyhow!("{e:?}")));
        })
        .unwrap();

    tokio::task::spawn_blocking(move || reconnect_rx.recv_timeout(Duration::from_secs(330)))
        .await
        .unwrap()
        .expect("reconnect thread must report")
        .unwrap();

    let nonce_after_reconnect = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();
    assert_eq!(nonce_after_reconnect, nonce_before_reconnect);

    let intents_after_reconnect: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_intent WHERE chain_id = 42161 AND wallet_address = $1",
    )
    .bind(&wallet_string)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    let hashes_after_reconnect: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution_transaction_hash AS hash \
         JOIN execution_intent AS intent ON intent.id = hash.intent_id \
         WHERE hash.chain_id = 42161 AND intent.wallet_address = $1",
    )
    .bind(&wallet_string)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(intents_after_reconnect, intents_before_reconnect);
    assert_eq!(hashes_after_reconnect, hashes_before_reconnect);

    // Duplicate-event safety at restart is the successful-fill emission markers staying set
    // and the intent staying inactive, so reconnect reconciliation cannot re-emit;
    // channel-level duplicate-event proof for the client emitter lives in the direct-client
    // fork suite
    let (acknowledged, filled, active): (bool, bool, bool) = sqlx::query_as(
        "SELECT intent.acknowledgement_emitted, intent.fill_emitted, intent.active \
         FROM execution_intent AS intent \
         WHERE intent.chain_id = 42161 AND intent.client_order_id = $1",
    )
    .bind(swap_order_id.as_str())
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert!(acknowledged);
    assert!(filled);
    assert!(!active);

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
        "fork_block": crate::harness::FORK_BLOCK,
        "chain_id": CHAIN_ID,
        "anvil_version": startup.version,
        "node": {
            "name": "BlockchainForkLiveNode",
            "execution_client_factory": "BlockchainExecutionClientFactory",
            "routing_venue": venue_string,
            "reconciliation": "startup continued without mass status; inflight checks disabled",
        },
        "operator_setup": {
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
        },
        "swap": {
            "client_order_id": swap_order_id.as_str(),
            "transaction_hash": swap_hash_string,
            "receipt_status": swap_receipt.status,
            "block_number": swap_receipt.block_number,
            "gas_used": swap_receipt.gas_used,
            "commission": fill.commission.as_ref().map(|c| c.to_string()),
            "configured_protections": {
                "slippage_bps": SLIPPAGE_BPS,
                "max_slippage_bps": 200,
                "deadline_seconds": 300,
                "receipt_timeout_secs": 60,
                "amount_in": WRAP_AMOUNT_WEI.to_string(),
                "quoted_amount_out": quoted_out.to_string(),
                "min_amount_out": min_amount_out.to_string(),
            },
            "observed_asset_delta": {
                "weth_spent": weth_spent.to_string(),
                "usdc_received": usdc_received.to_string(),
            },
            "account_state_events": account_state_count,
        },
        "reconnect": {
            "nonce_unchanged": true,
            "new_intents": intents_after_reconnect - intents_before_reconnect,
            "new_transaction_hashes": hashes_after_reconnect - hashes_before_reconnect,
            "emission_markers_terminal": true,
        },
    });
    let run_path = evidence_dir.join("livenode-run.json");
    std::fs::write(&run_path, serde_json::to_string_pretty(&run_json).unwrap()).unwrap();
    let run_sha = aws_lc_rs::digest::digest(
        &aws_lc_rs::digest::SHA256,
        &std::fs::read(&run_path).unwrap(),
    );
    let hashes_path = evidence_dir.join("SHA256SUMS.livenode");
    std::fs::write(
        &hashes_path,
        format!(
            "{}  livenode-run.json\n",
            nautilus_core::hex::encode(run_sha.as_ref())
        ),
    )
    .unwrap();
    eprintln!(
        "LiveNode fork test evidence packet written to {}",
        evidence_dir.display()
    );

    rpc_topology.assert_broadcast_isolation();
    unsafe {
        // SAFETY: this opt-in test owns this variable for the process
        std::env::remove_var(SIGNER_ENV);
    }
    unsafe {
        // SAFETY: this opt-in test owns this variable for the process
        std::env::remove_var(PAYLOAD_KEY_ENV);
    }
}

#[tokio::test]
async fn anvil_fork_livenode_routed_buy_and_reconnect() {
    if std::env::var("BLOCKCHAIN_FORK_TESTS").as_deref() != Ok("1") {
        eprintln!("BLOCKCHAIN_FORK_TESTS is not 1; skipping fork test");
        return;
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

    let (_anvil, _startup) = start_anvil(&fork_rpc_url)
        .await
        .expect("Anvil must start when BLOCKCHAIN_FORK_TESTS=1");
    let anvil_url = format!("http://127.0.0.1:{}", _startup.port);
    let rpc_topology = start_execution_rpc_topology(&anvil_url).await;
    let signer = PrivateKeySigner::random();
    let wallet = signer.address();
    let signer_private_key = nautilus_core::hex::encode_prefixed(signer.to_bytes());
    fund_anvil_wallet(&anvil_url, wallet).await;
    ensure_execution_schema(&admin_pool).await;

    let rpc_client = Arc::new(BlockchainHttpRpcClient::new(anvil_url.clone(), None, None));
    let erc20 = Erc20Contract::new(rpc_client.clone(), true);
    let pool = weth_usdc_pool();
    let venue_string = pool.instrument_id.venue.to_string();

    // SAFETY: this opt-in test runs in its own process.
    unsafe { std::env::set_var(SIGNER_ENV, signer_private_key) };
    // SAFETY: this opt-in test runs in its own process
    unsafe { std::env::set_var(PAYLOAD_KEY_ENV, PAYLOAD_KEY_HEX) };

    let operator_cache = Rc::new(RefCell::new(Cache::default()));
    operator_cache.borrow_mut().add_pool(pool.clone()).unwrap();
    let operator_core = ExecutionClientCore::new(
        TraderId::from("TRADER-001"),
        ClientId::from(EXEC_CLIENT_NAME),
        *BLOCKCHAIN_VENUE,
        OmsType::Netting,
        AccountId::from(EXEC_CLIENT_NAME),
        AccountType::Wallet,
        None,
        operator_cache.clone(),
    );
    let mut operator = BlockchainExecutionClient::new(
        operator_core,
        execution_config(
            wallet,
            rpc_topology.authoritative_url(),
            rpc_topology.verification(),
            pg_config.clone(),
        ),
    )
    .unwrap();
    let (operator_event_sender, _operator_event_receiver) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(operator_event_sender);
    operator.start().unwrap();
    operator.protect_payload_storage().await.unwrap();
    operator.connect().await.unwrap();
    assert!(
        rpc_client
            .get_transaction_receipt(
                &operator
                    .wrap(U256::from(WRAP_AMOUNT_WEI) * U256::from(2))
                    .await
                    .unwrap(),
            )
            .await
            .unwrap()
            .unwrap()
            .status
    );
    assert!(
        rpc_client
            .get_transaction_receipt(
                &operator
                    .approve(
                        WETH_ADDRESS,
                        U256::from(WRAP_AMOUNT_WEI) * U256::from(2),
                        ROUTER.parse().unwrap(),
                    )
                    .await
                    .unwrap()
            )
            .await
            .unwrap()
            .unwrap()
            .status
    );
    let (sell_snapshot, _) = build_full_range_snapshot(&rpc_client, &pool).await;
    let mut sell_profiler = PoolProfiler::new(Arc::new(pool.clone()));
    sell_profiler.restore_from_snapshot(sell_snapshot).unwrap();
    operator_cache
        .borrow_mut()
        .add_pool_profiler(sell_profiler)
        .unwrap();
    let setup_sell_id = format!("O-FORK-LIVENODE-BUY-SETUP-{}", nautilus_core::UUID4::new());
    let setup_sell = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TRADER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(pool.instrument_id)
        .client_order_id(ClientOrderId::new_checked(&setup_sell_id).unwrap())
        .side(OrderSide::Sell)
        .quantity(Quantity::from("0.002"))
        .build();
    operator_cache
        .borrow_mut()
        .add_order(setup_sell.clone(), None, None, false)
        .unwrap();
    operator
        .submit_order(SubmitOrder::new(
            TraderId::from("TRADER-001"),
            Some(ClientId::from(EXEC_CLIENT_NAME)),
            StrategyId::from("S-001"),
            setup_sell.instrument_id(),
            setup_sell.client_order_id(),
            setup_sell.init_event().clone(),
            None,
            None,
            None,
            nautilus_core::UUID4::new(),
            nautilus_core::UnixNanos::default(),
            None,
        ))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let row: Option<(String, bool)> = sqlx::query_as(
                "SELECT intent.status, intent.fill_emitted \
                 FROM execution_intent AS intent \
                 WHERE intent.chain_id = 42161 AND intent.client_order_id = $1",
            )
            .bind(&setup_sell_id)
            .fetch_optional(&admin_pool)
            .await
            .unwrap();

            if let Some((status, fill_emitted)) = row
                && status == "finalized"
                && fill_emitted
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("setup SELL must finalize before the LiveNode BUY");

    let (snapshot, _) = build_full_range_snapshot(&rpc_client, &pool).await;
    let amount_in = quote_buy_amount_in(&snapshot, &pool);
    let min_amount_out =
        U256::from(WRAP_AMOUNT_WEI) * U256::from(10_000 - SLIPPAGE_BPS) / U256::from(10_000);
    let quote = fork_quote_tick(&rpc_client, &pool).await;
    assert!(
        rpc_client
            .get_transaction_receipt(
                &operator
                    .approve(USDC_ADDRESS, amount_in, ROUTER.parse().unwrap())
                    .await
                    .unwrap()
            )
            .await
            .unwrap()
            .unwrap()
            .status
    );
    drop(operator);

    let usdc_balance_before = erc20.balance_of(&USDC_ADDRESS, &wallet).await.unwrap();
    let weth_balance_before = erc20.balance_of(&WETH_ADDRESS, &wallet).await.unwrap();

    let mut node = build_fork_node(
        "BlockchainForkLiveNodeBuy",
        wallet,
        rpc_topology.authoritative_url(),
        rpc_topology.verification(),
        pg_config.clone(),
        pool.clone(),
        snapshot.clone(),
        quote,
        venue_string,
    )
    .unwrap();
    let handle = node.handle();
    let pool_seen = Rc::new(Cell::new(false));
    let order_events: Rc<RefCell<Vec<OrderEventAny>>> = Rc::new(RefCell::new(Vec::new()));
    msgbus::subscribe_defi_pools(
        defi::switchboard::get_defi_pool_topic(pool.instrument_id).into(),
        TypedHandler::<Pool>::from({
            let pool_seen = pool_seen.clone();
            move |_pool: &Pool| pool_seen.set(true)
        }),
        None,
    );
    node.add_strategy(SwapStrategy::new(
        pool.instrument_id,
        handle.clone(),
        pool_seen,
        order_events.clone(),
        OrderSide::Buy,
    ))
    .unwrap();
    tokio::time::timeout(Duration::from_secs(300), node.run())
        .await
        .expect("buy node must run to completion after the fill")
        .unwrap();

    let events = order_events.borrow().clone();
    assert_eq!(events.len(), 2, "was: {events:?}");
    let OrderEventAny::Filled(fill) = &events[1] else {
        panic!("expected OrderFilled, was {:?}", events[1]);
    };
    assert_eq!(fill.order_side, OrderSide::Buy);
    assert!(fill.commission.is_some(), "gas commission missing");
    let swap_hash_string = fill.venue_order_id.as_str().to_string();

    let weth_balance_after = erc20.balance_of(&WETH_ADDRESS, &wallet).await.unwrap();
    let usdc_balance_after = erc20.balance_of(&USDC_ADDRESS, &wallet).await.unwrap();
    let weth_received = weth_balance_after.checked_sub(weth_balance_before).unwrap();
    let usdc_spent = usdc_balance_before.checked_sub(usdc_balance_after).unwrap();
    assert_eq!(usdc_spent, amount_in);
    assert!(weth_received >= min_amount_out);
    let scale = U256::from(10u64).pow(U256::from(18 - u32::from(FIXED_PRECISION)));
    assert_eq!(U256::from(fill.last_qty.raw), weth_received / scale);

    let (status, fill_emitted): (String, bool) = sqlx::query_as(
        "SELECT intent.status, intent.fill_emitted \
         FROM execution_intent AS intent \
         JOIN execution_transaction_hash AS hash \
           ON hash.intent_id = intent.id AND hash.current \
         WHERE intent.chain_id = 42161 AND hash.transaction_hash = $1",
    )
    .bind(&swap_hash_string)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(status, "finalized");
    assert!(fill_emitted);

    let nonce_before_reconnect = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();
    let intents_before_reconnect: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_intent WHERE chain_id = 42161")
            .fetch_one(&admin_pool)
            .await
            .unwrap();
    let mut probe = build_fork_node(
        "BlockchainForkLiveNodeBuyReconnect",
        wallet,
        rpc_topology.authoritative_url(),
        rpc_topology.verification(),
        pg_config,
        pool,
        snapshot,
        fork_quote_tick(&rpc_client, &weth_usdc_pool()).await,
        weth_usdc_pool().instrument_id.venue.to_string(),
    )
    .unwrap();
    probe
        .add_strategy(ReconnectProbeStrategy::new(probe.handle()))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(120), probe.run())
        .await
        .expect("buy reconnect probe must complete")
        .unwrap();
    let nonce_after_reconnect = rpc_client
        .get_transaction_count_latest(&wallet)
        .await
        .unwrap();
    let intents_after_reconnect: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution_intent WHERE chain_id = 42161")
            .fetch_one(&admin_pool)
            .await
            .unwrap();
    assert_eq!(nonce_after_reconnect, nonce_before_reconnect);
    assert_eq!(intents_after_reconnect, intents_before_reconnect);

    rpc_topology.assert_broadcast_isolation();
    unsafe {
        // SAFETY: this opt-in test owns this variable for the process
        std::env::remove_var(SIGNER_ENV);
    }
    unsafe {
        // SAFETY: this opt-in test owns this variable for the process
        std::env::remove_var(PAYLOAD_KEY_ENV);
    }
}
