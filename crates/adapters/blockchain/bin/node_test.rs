// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    ops::{Deref, DerefMut},
    sync::Arc,
    time::Duration,
};

use nautilus_blockchain::{
    config::BlockchainDataClientConfig, factories::BlockchainDataClientFactory,
};
use nautilus_common::{
    actor::{DataActor, DataActorCore, data_actor::DataActorConfig},
    enums::{Environment, LogColor},
    logging::log_info,
};
use nautilus_core::env::get_env_var;
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    defi::{Block, Blockchain, Pool, PoolLiquidityUpdate, PoolSwap, chain::chains},
    identifiers::{ClientId, InstrumentId, TraderId},
};

// Requires capnp installed on the machine
// Run with `cargo run -p nautilus-blockchain --bin node_test --features hypersync`
// To see additional tracing logs `export RUST_LOG=debug,h2=off`

// ================================================================================================
// IMPORTANT: The actor definitions below are EXAMPLE CODE for demonstration purposes.
// They should NOT be moved to the main library as they are specific to this test scenario.
// If you need production-ready actors, create them in a separate production module.
// ================================================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::default();
    let node_name = "TESTER-001".to_string();

    let chain = chains::ARBITRUM.clone();
    let wss_rpc_url = get_env_var("RPC_WSS_URL")?;
    let http_rpc_url = get_env_var("RPC_HTTP_URL")?;
    // let from_block = Some(22_735_000_u64); // Ethereum
    // let from_block = Some(348_860_000_u64); // Arbitrum
    let from_block = None; // No sync

    let client_factory = BlockchainDataClientFactory::new();
    let client_config = BlockchainDataClientConfig::new(
        Arc::new(chain.clone()),
        vec!["Arbitrum:UniswapV3".to_string()],
        http_rpc_url,
        None, // RPC requests per second
        Some(wss_rpc_url),
        true, // Use HyperSync for live data
        // Some(from_block), // from_block
        from_block,
        Some(PostgresConnectOptions::default()),
    );

    let mut node = LiveNode::builder(node_name, trader_id, environment)?
        .with_load_state(false)
        .with_save_state(false)
        .add_data_client(
            None, // Use factory name
            client_factory,
            client_config,
        )?
        .build()?;

    // Create and register a blockchain subscriber actor
    let client_id = ClientId::new(format!("BLOCKCHAIN-{}", chain.name));

    let pools = vec![
        InstrumentId::from("WETH/USDC-3000.UniswapV3:Arbitrum"), // Arbitrum WETH/USDC 0.30% pool
    ];

    let actor_config = BlockchainSubscriberActorConfig::new(client_id, chain.name, pools);
    let actor = BlockchainSubscriberActor::new(actor_config);

    node.add_actor(actor)?;

    node.run().await?;

    Ok(())
}

/// Configuration for the blockchain subscriber actor.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.blockchain")
)]
pub struct BlockchainSubscriberActorConfig {
    /// Base data actor configuration.
    pub base: DataActorConfig,
    /// Client ID to use for subscriptions.
    pub client_id: ClientId,
    /// The blockchain to subscribe for.
    pub chain: Blockchain,
    /// Pool instrument IDs to monitor for swaps and liquidity updates.
    pub pools: Vec<InstrumentId>,
}

impl BlockchainSubscriberActorConfig {
    /// Creates a new [`BlockchainSubscriberActorConfig`] instance.
    #[must_use]
    pub fn new(client_id: ClientId, chain: Blockchain, pools: Vec<InstrumentId>) -> Self {
        Self {
            base: DataActorConfig::default(),
            client_id,
            chain,
            pools,
        }
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BlockchainSubscriberActorConfig {
    /// Creates a new `BlockchainSubscriberActorConfig` instance.
    #[new]
    fn py_new(client_id: ClientId, chain: Blockchain, pools: Vec<InstrumentId>) -> Self {
        Self::new(client_id, chain, pools)
    }

    /// Returns the client ID.
    #[getter]
    const fn client_id(&self) -> ClientId {
        self.client_id
    }

    /// Returns the blockchain.
    #[getter]
    const fn chain(&self) -> Blockchain {
        self.chain
    }

    /// Returns the pool instrument IDs.
    #[getter]
    fn pools(&self) -> Vec<InstrumentId> {
        self.pools.clone()
    }

    /// Returns a string representation of the configuration.
    fn __repr__(&self) -> String {
        format!(
            "BlockchainSubscriberActorConfig(client_id={}, chain={:?}, pools={:?})",
            self.client_id, self.chain, self.pools
        )
    }
}

/// A basic blockchain subscriber actor that monitors DeFi activities.
///
/// This actor demonstrates how to use the `DataActor` trait to monitor blockchain data
/// from DEXs, pools, and other DeFi protocols. It logs received blocks and swaps
/// to demonstrate the data flow.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.blockchain", unsendable)
)]
pub struct BlockchainSubscriberActor {
    core: DataActorCore,
    config: BlockchainSubscriberActorConfig,
    pub received_blocks: Vec<Block>,
    pub received_pool_swaps: Vec<PoolSwap>,
    pub received_pool_liquidity_updates: Vec<PoolLiquidityUpdate>,
    pub received_pools: Vec<Pool>,
}

impl Deref for BlockchainSubscriberActor {
    type Target = DataActorCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for BlockchainSubscriberActor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl DataActor for BlockchainSubscriberActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let client_id = self.config.client_id;

        self.subscribe_blocks(self.config.chain, Some(client_id), None);

        let pool_instrument_ids = self.config.pools.clone();
        for instrument_id in pool_instrument_ids {
            self.subscribe_pool(instrument_id, Some(client_id), None);
            self.subscribe_pool_swaps(instrument_id, Some(client_id), None);
            self.subscribe_pool_liquidity_updates(instrument_id, Some(client_id), None);
        }

        self.clock().set_timer(
            "TEST-TIMER-1-SECOND",
            Duration::from_secs(1),
            None,
            None,
            None,
            Some(true),
            Some(false),
        )?;

        self.clock().set_timer(
            "TEST-TIMER-2-SECOND",
            Duration::from_secs(2),
            None,
            None,
            None,
            Some(true),
            Some(false),
        )?;

        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let client_id = self.config.client_id;

        self.unsubscribe_blocks(self.config.chain, Some(client_id), None);

        let pool_instrument_ids = self.config.pools.clone();
        for instrument_id in pool_instrument_ids {
            self.unsubscribe_pool(instrument_id, Some(client_id), None);
            self.unsubscribe_pool_swaps(instrument_id, Some(client_id), None);
            self.unsubscribe_pool_liquidity_updates(instrument_id, Some(client_id), None);
        }

        Ok(())
    }

    fn on_block(&mut self, block: &Block) -> anyhow::Result<()> {
        log_info!("Received {block}", color = LogColor::Cyan);

        self.received_blocks.push(block.clone());
        Ok(())
    }

    fn on_pool_swap(&mut self, swap: &PoolSwap) -> anyhow::Result<()> {
        log_info!("Received {swap}", color = LogColor::Cyan);

        self.received_pool_swaps.push(swap.clone());
        Ok(())
    }
}

impl BlockchainSubscriberActor {
    /// Creates a new [`BlockchainSubscriberActor`] instance.
    #[must_use]
    pub fn new(config: BlockchainSubscriberActorConfig) -> Self {
        Self {
            core: DataActorCore::new(config.base.clone()),
            config,
            received_blocks: Vec::new(),
            received_pool_swaps: Vec::new(),
            received_pool_liquidity_updates: Vec::new(),
            received_pools: Vec::new(),
        }
    }

    /// Returns the number of blocks received by this actor.
    #[must_use]
    pub const fn block_count(&self) -> usize {
        self.received_blocks.len()
    }

    /// Returns the number of pools received by this actor.
    #[must_use]
    pub const fn pool_count(&self) -> usize {
        self.received_pools.len()
    }

    /// Returns the number of swaps received by this actor.
    #[must_use]
    pub const fn pool_swap_count(&self) -> usize {
        self.received_pool_swaps.len()
    }

    /// Returns the number of liquidity updates received by this actor.
    #[must_use]
    pub const fn pool_liquidity_update_count(&self) -> usize {
        self.received_pool_liquidity_updates.len()
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BlockchainSubscriberActor {
    /// Creates a new `BlockchainSubscriberActor` instance.
    #[new]
    fn py_new(config: BlockchainSubscriberActorConfig) -> Self {
        Self::new(config)
    }

    /// Returns the number of blocks received by this actor.
    const fn py_block_count(&self) -> usize {
        self.block_count()
    }

    /// Returns the number of pools received by this actor.
    const fn py_pool_count(&self) -> usize {
        self.pool_count()
    }

    /// Returns the number of swaps received by this actor.
    const fn py_pool_swap_count(&self) -> usize {
        self.pool_swap_count()
    }

    /// Returns the number of liquidity updates received by this actor.
    const fn py_pool_liquidity_update_count(&self) -> usize {
        self.pool_liquidity_update_count()
    }

    /// Returns the received blocks.
    #[getter]
    fn received_blocks(&self) -> Vec<Block> {
        self.received_blocks.clone()
    }

    /// Returns the received pool swaps.
    #[getter]
    fn received_pool_swaps(&self) -> Vec<PoolSwap> {
        self.received_pool_swaps.clone()
    }

    /// Returns the received pool liquidity updates.
    #[getter]
    fn received_pool_liquidity_updates(&self) -> Vec<PoolLiquidityUpdate> {
        self.received_pool_liquidity_updates.clone()
    }

    /// Returns the received pools.
    #[getter]
    fn received_pools(&self) -> Vec<Pool> {
        self.received_pools.clone()
    }

    /// Returns a string representation of the actor.
    fn __repr__(&self) -> String {
        format!(
            "BlockchainSubscriberActor(blocks={}, swaps={}, liquidity_updates={}, pools={})",
            self.block_count(),
            self.pool_swap_count(),
            self.pool_liquidity_update_count(),
            self.pool_count()
        )
    }
}
