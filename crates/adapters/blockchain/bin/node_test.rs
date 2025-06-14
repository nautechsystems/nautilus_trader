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
    enums::Environment,
};
use nautilus_core::env::get_env_var;
use nautilus_live::node::LiveNode;
use nautilus_model::{
    defi::{Blockchain, Chain, Pool, PoolLiquidityUpdate, Swap, chain::chains},
    identifiers::{ClientId, TraderId},
};

// Run with `cargo run -p nautilus-blockchain --bin node_test --features hypersync,python`

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Initialize Python interpreter only if python feature is enabled
    // #[cfg(feature = "python")]
    pyo3::prepare_freethreaded_python();

    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::default();
    let node_name = "TESTER-001".to_string();

    let chain: Chain = match std::env::var("CHAIN")
        .ok()
        .and_then(|s| s.parse::<Blockchain>().ok())
    {
        Some(Blockchain::Ethereum) => chains::ETHEREUM.clone(),
        Some(Blockchain::Base) => chains::BASE.clone(),
        Some(Blockchain::Arbitrum) => chains::ARBITRUM.clone(),
        Some(Blockchain::Polygon) => chains::POLYGON.clone(),
        _ => {
            println!("⚠️  No valid CHAIN env var found, using Ethereum as default");
            chains::ETHEREUM.clone()
        }
    };
    let wss_rpc_url = get_env_var("RPC_WSS_URL")?;
    let http_rpc_url = get_env_var("RPC_HTTP_URL")?;

    let client_factory = BlockchainDataClientFactory::new();
    let client_config = BlockchainDataClientConfig::new(
        Arc::new(chain),
        http_rpc_url,
        None, // RPC requests per second
        Some(wss_rpc_url),
        false, // Don't use hypersync for live data
        None,  // from_block
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
    let client_id = ClientId::new("BLOCKCHAIN");
    let pool_addresses = vec![
        // Example pool addresses - these would be real pool addresses for testing
        "0x88e6a0c2ddd26feeb64f039a2c41296fcb3f5640".to_string(), // USDC/ETH 0.05% on Uniswap V3
                                                                  // Add more pool addresses as needed for testing
    ];

    let actor_config = BlockchainSubscriberActorConfig::new(client_id, pool_addresses);
    let actor = BlockchainSubscriberActor::new(actor_config);

    node.add_actor(actor)?;

    node.run().await?;

    Ok(())
}

/// Configuration for the blockchain subscriber actor.
#[derive(Debug, Clone)]
pub struct BlockchainSubscriberActorConfig {
    /// Base data actor configuration.
    pub base: DataActorConfig,
    /// Client ID to use for subscriptions.
    pub client_id: ClientId,
    /// Pool addresses to monitor for swaps and liquidity updates.
    pub pool_addresses: Vec<String>,
}

impl BlockchainSubscriberActorConfig {
    /// Creates a new [`BlockchainSubscriberActorConfig`] instance.
    #[must_use]
    pub fn new(client_id: ClientId, pool_addresses: Vec<String>) -> Self {
        Self {
            base: DataActorConfig::default(),
            client_id,
            pool_addresses,
        }
    }
}

/// A basic blockchain subscriber actor that monitors DeFi activities.
///
/// This actor demonstrates how to use the `DataActor` trait to monitor blockchain data
/// from DEXs, pools, and other DeFi protocols. It logs received swaps and liquidity updates
/// to demonstrate the data flow.
#[derive(Debug)]
pub struct BlockchainSubscriberActor {
    core: DataActorCore,
    config: BlockchainSubscriberActorConfig,
    pub received_swaps: Vec<Swap>,
    pub received_liquidity_updates: Vec<PoolLiquidityUpdate>,
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
        log::info!(
            "Starting blockchain subscriber actor for {} pool(s)",
            self.config.pool_addresses.len()
        );

        let pool_addresses = self.config.pool_addresses.clone();
        let _client_id = self.config.client_id;

        // Subscribe to custom data for each pool address
        for pool_address in pool_addresses {
            log::info!("Subscribing to blockchain data for pool {pool_address}");

            // Note: Blockchain clients work differently than traditional market data clients
            // They monitor blockchain events rather than traditional market data subscriptions
            // The actual subscription logic would be handled by the BlockchainDataClient
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

        log::info!("Blockchain subscriber actor started successfully");
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Stopping blockchain subscriber actor for {} pool(s)",
            self.config.pool_addresses.len()
        );

        let pool_addresses = self.config.pool_addresses.clone();
        let _client_id = self.config.client_id;

        // Unsubscribe from custom data for each pool
        for pool_address in pool_addresses {
            log::info!("Unsubscribing from blockchain data for pool {pool_address}");
        }

        log::info!("Blockchain subscriber actor stopped successfully");
        Ok(())
    }

    fn on_data(&mut self, data: &dyn std::any::Any) -> anyhow::Result<()> {
        // TODO: TBD which handlers to use
        if let Some(swap) = data.downcast_ref::<Swap>() {
            log::info!("Received swap: {swap:?}");
            self.received_swaps.push(swap.clone());
        } else if let Some(liquidity_update) = data.downcast_ref::<PoolLiquidityUpdate>() {
            log::info!("Received liquidity update: {liquidity_update:?}");
            self.received_liquidity_updates
                .push(liquidity_update.clone());
        } else if let Some(pool) = data.downcast_ref::<Pool>() {
            log::info!("Received pool: {pool:?}");
            self.received_pools.push(pool.clone());
        }

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
            received_swaps: Vec::new(),
            received_liquidity_updates: Vec::new(),
            received_pools: Vec::new(),
        }
    }

    /// Returns the number of swaps received by this actor.
    #[must_use]
    pub const fn swap_count(&self) -> usize {
        self.received_swaps.len()
    }

    /// Returns the number of liquidity updates received by this actor.
    #[must_use]
    pub const fn liquidity_update_count(&self) -> usize {
        self.received_liquidity_updates.len()
    }

    /// Returns the number of pools received by this actor.
    #[must_use]
    pub const fn pool_count(&self) -> usize {
        self.received_pools.len()
    }
}
