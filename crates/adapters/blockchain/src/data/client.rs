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

use nautilus_common::{
    messages::{
        DataEvent,
        defi::{
            DefiDataCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, SubscribeBlocks,
            SubscribePool, SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks,
            UnsubscribePool, UnsubscribePoolLiquidityUpdates, UnsubscribePoolSwaps,
        },
    },
    runtime::get_runtime,
};
use nautilus_data::client::DataClient;
use nautilus_model::{
    defi::{DefiData, SharedChain, validation::validate_address},
    identifiers::{ClientId, Venue},
};

use crate::{
    config::BlockchainDataClientConfig,
    data::core::BlockchainDataClientCore,
    exchanges::get_dex_extended,
    rpc::{BlockchainRpcClient, types::BlockchainMessage},
};

/// A comprehensive client for interacting with blockchain data from multiple sources.
///
/// The `BlockchainDataClient` serves as a facade that coordinates between different blockchain
/// data providers, caching mechanisms, and contract interactions. It provides a unified interface
/// for retrieving and processing blockchain data, particularly focused on DeFi protocols.
///
/// This client supports two primary data sources:
/// 1. Direct RPC connections to blockchain nodes (via WebSocket).
/// 2. HyperSync API for efficient historical data queries.
#[derive(Debug)]
pub struct BlockchainDataClient {
    /// The blockchain being targeted by this client instance.
    pub chain: SharedChain,
    /// Configuration parameters for the blockchain data client.
    pub config: BlockchainDataClientConfig,
    /// The core client instance that handles blockchain operations.
    /// Wrapped in Option to allow moving it into the background processing task.
    pub core_client: Option<BlockchainDataClientCore>,
    /// Channel receiver for messages from the HyperSync client.
    hypersync_rx: Option<tokio::sync::mpsc::UnboundedReceiver<BlockchainMessage>>,
    /// Channel sender for commands to be processed asynchronously.
    command_tx: tokio::sync::mpsc::UnboundedSender<DefiDataCommand>,
    /// Channel receiver for commands to be processed asynchronously.
    command_rx: Option<tokio::sync::mpsc::UnboundedReceiver<DefiDataCommand>>,
    /// Background task for processing messages.
    process_task: Option<tokio::task::JoinHandle<()>>,
    /// Oneshot channel sender for graceful shutdown signal.
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl BlockchainDataClient {
    /// Creates a new [`BlockchainDataClient`] instance for the specified configuration.
    #[must_use]
    pub fn new(config: BlockchainDataClientConfig) -> Self {
        let chain = config.chain.clone();
        let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (hypersync_tx, hypersync_rx) = tokio::sync::mpsc::unbounded_channel();
        let core_client = BlockchainDataClientCore::new(config.clone(), Some(hypersync_tx));
        Self {
            chain,
            core_client: Some(core_client),
            config,
            hypersync_rx: Some(hypersync_rx),
            command_tx,
            command_rx: Some(command_rx),
            process_task: None,
            shutdown_tx: None,
        }
    }

    /// Spawns the main processing task that handles commands and blockchain data.
    ///
    /// This method creates a background task that:
    /// 1. Processes subscription/unsubscription commands from the command channel
    /// 2. Handles incoming blockchain data from HyperSync
    /// 3. Processes RPC messages if RPC client is configured
    /// 4. Routes processed data to subscribers
    fn spawn_process_task(&mut self) {
        let command_rx = if let Some(r) = self.command_rx.take() {
            r
        } else {
            tracing::error!("Command receiver already taken, not spawning handler");
            return;
        };

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let mut core_client = self.core_client.take().unwrap();
        let mut hypersync_rx = self.hypersync_rx.take().unwrap();

        let handle = get_runtime().spawn(async move {
            tracing::debug!("Started task 'process'");

            let mut command_rx = command_rx;
            let mut shutdown_rx = shutdown_rx;

            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        tracing::debug!("Received shutdown signal in Blockchain data client process task");
                        core_client.disconnect();
                        break;
                    }
                    command = command_rx.recv() => {
                        if let Some(cmd) = command {
                            match cmd {
                                DefiDataCommand::Subscribe(cmd) => {
                                    let chain = cmd.blockchain();
                                    if chain != core_client.chain.name {
                                        tracing::error!("Incorrect blockchain for subscribe command: {chain}");
                                        continue;
                                    }

                                      if let Err(e) = Self::handle_subscribe_command(cmd, &mut core_client).await{
                                        tracing::error!("Error processing subscribe command: {e}");
                                    }
                                }
                                DefiDataCommand::Unsubscribe(cmd) => {
                                    let chain = cmd.blockchain();
                                    if chain != core_client.chain.name {
                                        tracing::error!("Incorrect blockchain for subscribe command: {chain}");
                                        continue;
                                    }

                                    if let Err(e) = Self::handle_unsubscribe_command(cmd, &mut core_client).await{
                                        tracing::error!("Error processing subscribe command: {e}");
                                    }
                                }
                            }
                        } else {
                            tracing::debug!("Command channel closed");
                            break;
                        }
                    }
                    data = hypersync_rx.recv() => {
                        if let Some(msg) = data {
                            let data_event = match msg {
                                BlockchainMessage::Block(block) => {
                                    // Fetch and process all subscribed events per DEX
                                    for dex in core_client.cache.get_registered_dexes(){
                                        let addresses = core_client.subscription_manager.get_subscribed_dex_contract_addresses(&dex);
                                        if !addresses.is_empty() {
                                            core_client.hypersync_client.process_block_dex_contract_events(
                                                &dex,
                                                block.number,
                                                addresses,
                                                core_client.subscription_manager.get_dex_pool_swap_event_signature(&dex).unwrap(),
                                                core_client.subscription_manager.get_dex_pool_mint_event_signature(&dex).unwrap(),
                                                core_client.subscription_manager.get_dex_pool_burn_event_signature(&dex).unwrap(),
                                            ).await;
                                        }
                                    }

                                    Some(DataEvent::DeFi(DefiData::Block(block)))
                                }
                                BlockchainMessage::SwapEvent(swap_event) => {
                                    match core_client.get_pool(&swap_event.pool_address) {
                                        Ok(pool) => {
                                            let dex_extended = get_dex_extended(core_client.chain.name, &pool.dex.name).expect("Failed to get dex extended");
                                            match core_client.process_pool_swap_event(
                                                &swap_event,
                                                pool,
                                                dex_extended,
                                            ).await{
                                                Ok(swap) => Some(DataEvent::DeFi(DefiData::PoolSwap(swap))),
                                                Err(e) => {
                                                    tracing::error!("Error processing pool swap event: {e}");
                                                    None
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to get pool {} with error {:?}", swap_event.pool_address, e);
                                            None
                                        }
                                    }
                                }
                                BlockchainMessage::BurnEvent(burn_event) => {
                                    match core_client.get_pool(&burn_event.pool_address) {
                                        Ok(pool) => {
                                            let dex_extended = get_dex_extended(core_client.chain.name, &pool.dex.name).expect("Failed to get dex extended");
                                            match core_client.process_pool_burn_event(
                                                &burn_event,
                                                pool,
                                                dex_extended,
                                            ).await{
                                                Ok(update) => Some(DataEvent::DeFi(DefiData::PoolLiquidityUpdate(update))),
                                                Err(e) => {
                                                    tracing::error!("Error processing pool burn event: {e}");
                                                    None
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to get pool {} with error {:?}", burn_event.pool_address, e);
                                            None
                                        }
                                    }
                                }
                                BlockchainMessage::MintEvent(mint_event) => {
                                    match core_client.get_pool(&mint_event.pool_address) {
                                        Ok(pool) => {
                                            let dex_extended = get_dex_extended(core_client.chain.name,&pool.dex.name).expect("Failed to get dex extended");
                                            match core_client.process_pool_mint_event(
                                                &mint_event,
                                                pool,
                                                dex_extended,
                                            ).await{
                                                Ok(update) => Some(DataEvent::DeFi(DefiData::PoolLiquidityUpdate(update))),
                                                Err(e) => {
                                                    tracing::error!("Error processing pool mint event: {e}");
                                                    None
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!("Failed to get pool {} with error {:?}", mint_event.pool_address, e);
                                            None
                                        }
                                    }
                                }
                            };

                            if let Some(event) = data_event {
                                core_client.send_data(event);
                            }
                        } else {
                            tracing::debug!("HyperSync data channel closed");
                            break;
                        }
                    }
                    msg = async {
                        if let Some(ref mut rpc_client) = core_client.rpc_client {
                            Some(rpc_client.next_rpc_message().await)
                        } else {
                            None
                        }
                    } => {
                        if let Some(msg) = msg {
                            match msg {
                                Ok(BlockchainMessage::Block(block)) => {
                                    let data = DataEvent::DeFi(DefiData::Block(block));
                                    core_client.send_data(data);
                                },
                                Ok(BlockchainMessage::SwapEvent(_)) => {
                                    tracing::warn!("RPC swap events are not yet supported");
                                }
                                Ok(BlockchainMessage::MintEvent(_)) => {
                                    tracing::warn!("RPC mint events are not yet supported");
                                }
                                Ok(BlockchainMessage::BurnEvent(_)) => {
                                    tracing::warn!("RPC burn events are not yet supported");
                                }
                                Err(e) => {
                                    tracing::error!("Error processing RPC message: {e}");
                                }
                            }
                        }
                    }
                }
            }

            tracing::debug!("Stopped task 'process'");
        });

        self.process_task = Some(handle);
    }

    /// Processes DeFi subscription commands to start receiving specific blockchain data.
    async fn handle_subscribe_command(
        command: DefiSubscribeCommand,
        core_client: &mut BlockchainDataClientCore,
    ) -> anyhow::Result<()> {
        match command {
            DefiSubscribeCommand::Blocks(_cmd) => {
                tracing::info!("Processing subscribe blocks command");

                // Try RPC client first if available, otherwise use HyperSync
                if let Some(ref mut rpc) = core_client.rpc_client {
                    if let Err(e) = rpc.subscribe_blocks().await {
                        tracing::warn!(
                            "RPC blocks subscription failed: {e}, falling back to HyperSync"
                        );
                        core_client.hypersync_client.subscribe_blocks();
                        tokio::task::yield_now().await;
                    } else {
                        tracing::info!("Successfully subscribed to blocks via RPC");
                    }
                } else {
                    tracing::info!("Subscribing to blocks via HyperSync");
                    core_client.hypersync_client.subscribe_blocks();
                    tokio::task::yield_now().await;
                }

                Ok(())
            }
            DefiSubscribeCommand::Pool(_cmd) => {
                tracing::info!("Processing subscribe pool command");
                // Pool subscriptions are typically handled at the application level
                // as they involve specific pool addresses and don't require blockchain streaming
                tracing::warn!("Pool subscriptions are handled at application level");
                Ok(())
            }
            DefiSubscribeCommand::PoolSwaps(cmd) => {
                tracing::info!(
                    "Processing subscribe pool swaps command for {}",
                    cmd.instrument_id
                );

                if let Some(ref mut _rpc) = core_client.rpc_client {
                    tracing::warn!(
                        "RPC pool swaps subscription not yet implemented, using HyperSync"
                    );
                }

                if let Ok((_, dex)) = cmd.instrument_id.venue.parse_dex() {
                    let pool_address = validate_address(cmd.instrument_id.symbol.as_str())
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Invalid pool swap address '{}' failed with error: {:?}",
                                cmd.instrument_id,
                                e
                            )
                        })?;
                    core_client
                        .subscription_manager
                        .subscribe_swaps(dex, pool_address);
                } else {
                    anyhow::bail!(
                        "Invalid venue {}, expected Blockchain DEX format",
                        cmd.instrument_id.venue
                    )
                }

                Ok(())
            }
            DefiSubscribeCommand::PoolLiquidityUpdates(cmd) => {
                tracing::info!(
                    "Processing subscribe pool liquidity updates command for address: {}",
                    cmd.instrument_id
                );

                if let Some(ref mut _rpc) = core_client.rpc_client {
                    tracing::warn!(
                        "RPC pool liquidity updates subscription not yet implemented, using HyperSync"
                    );
                }

                if let Ok((_, dex)) = cmd.instrument_id.venue.parse_dex() {
                    let pool_address = validate_address(cmd.instrument_id.symbol.as_str())
                        .map_err(|_| {
                            anyhow::anyhow!("Invalid pool swap address: {}", cmd.instrument_id)
                        })?;
                    core_client
                        .subscription_manager
                        .subscribe_burns(dex, pool_address);
                    core_client
                        .subscription_manager
                        .subscribe_mints(dex, pool_address);
                } else {
                    anyhow::bail!(
                        "Invalid venue {}, expected Blockchain DEX format",
                        cmd.instrument_id.venue
                    )
                }

                Ok(())
            }
        }
    }

    /// Processes DeFi unsubscription commands to stop receiving specific blockchain data.
    async fn handle_unsubscribe_command(
        command: DefiUnsubscribeCommand,
        core_client: &mut BlockchainDataClientCore,
    ) -> anyhow::Result<()> {
        match command {
            DefiUnsubscribeCommand::Blocks(_cmd) => {
                tracing::info!("Processing unsubscribe blocks command");

                // TODO: Implement RPC unsubscription when available
                if core_client.rpc_client.is_some() {
                    tracing::warn!("RPC blocks unsubscription not yet implemented");
                }

                // Use HyperSync client for unsubscription
                core_client.hypersync_client.unsubscribe_blocks();
                tracing::info!("Unsubscribed from blocks via HyperSync");

                Ok(())
            }
            DefiUnsubscribeCommand::Pool(_cmd) => {
                tracing::info!("Processing unsubscribe pool command");
                // Pool unsubscriptions are typically handled at the application level
                tracing::warn!("Pool unsubscriptions are handled at application level");
                Ok(())
            }
            DefiUnsubscribeCommand::PoolSwaps(cmd) => {
                tracing::info!("Processing unsubscribe pool swaps command");

                if let Ok((_, dex)) = cmd.instrument_id.venue.parse_dex() {
                    let pool_address = validate_address(cmd.instrument_id.symbol.as_str())
                        .map_err(|_| {
                            anyhow::anyhow!("Invalid pool swap address: {}", cmd.instrument_id)
                        })?;
                    core_client
                        .subscription_manager
                        .unsubscribe_swaps(dex, pool_address);
                } else {
                    anyhow::bail!(
                        "Invalid venue {}, expected Blockchain DEX format",
                        cmd.instrument_id.venue
                    )
                }

                Ok(())
            }
            DefiUnsubscribeCommand::PoolLiquidityUpdates(cmd) => {
                tracing::info!(
                    "Processing unsubscribe pool liquidity updates command for {}",
                    cmd.instrument_id
                );

                if let Ok((_, dex)) = cmd.instrument_id.venue.parse_dex() {
                    let pool_address = validate_address(cmd.instrument_id.symbol.as_str())
                        .map_err(|_| {
                            anyhow::anyhow!("Invalid pool swap address: {}", cmd.instrument_id)
                        })?;
                    core_client
                        .subscription_manager
                        .unsubscribe_burns(dex, pool_address);
                    core_client
                        .subscription_manager
                        .unsubscribe_mints(dex, pool_address);
                } else {
                    anyhow::bail!(
                        "Invalid venue {}, expected Blockchain DEX format",
                        cmd.instrument_id.venue
                    )
                }

                Ok(())
            }
        }
    }

    /// Waits for the background processing task to complete.
    ///
    /// This method blocks until the spawned process task finishes execution,
    /// which typically happens after a shutdown signal is sent.
    pub async fn await_process_task_close(&mut self) {
        if let Some(handle) = self.process_task.take()
            && let Err(e) = handle.await
        {
            tracing::error!("Process task join error: {e}");
        }
    }
}

#[async_trait::async_trait]
impl DataClient for BlockchainDataClient {
    fn client_id(&self) -> ClientId {
        ClientId::from(format!("BLOCKCHAIN-{}", self.chain.name).as_str())
    }

    fn venue(&self) -> Option<Venue> {
        // Blockchain data clients don't map to a single venue since they can provide
        // data for multiple DEXs across the blockchain
        None
    }

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Starting blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Stopping blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Resetting blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Disposing blockchain data client for '{chain_name}'",
            chain_name = self.chain.name
        );
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Connecting blockchain data client for '{}'",
            self.chain.name
        );

        if let Some(core_client) = &mut self.core_client {
            core_client.connect().await?;
            core_client.initialize_cache_database().await;
        }

        if self.process_task.is_none() {
            self.spawn_process_task();
        }

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "Disconnecting blockchain data client for '{}'",
            self.chain.name
        );

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        self.await_process_task_close().await;

        Ok(())
    }

    fn is_connected(&self) -> bool {
        // TODO: Improve connection detection
        // For now, we'll assume connected if we have either RPC or HyperSync configured
        true
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn subscribe_blocks(&mut self, cmd: &SubscribeBlocks) -> anyhow::Result<()> {
        let command = DefiDataCommand::Subscribe(DefiSubscribeCommand::Blocks(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn subscribe_pool(&mut self, cmd: &SubscribePool) -> anyhow::Result<()> {
        let command = DefiDataCommand::Subscribe(DefiSubscribeCommand::Pool(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn subscribe_pool_swaps(&mut self, cmd: &SubscribePoolSwaps) -> anyhow::Result<()> {
        let command = DefiDataCommand::Subscribe(DefiSubscribeCommand::PoolSwaps(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn subscribe_pool_liquidity_updates(
        &mut self,
        cmd: &SubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        let command =
            DefiDataCommand::Subscribe(DefiSubscribeCommand::PoolLiquidityUpdates(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_blocks(&mut self, cmd: &UnsubscribeBlocks) -> anyhow::Result<()> {
        let command = DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::Blocks(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_pool(&mut self, cmd: &UnsubscribePool) -> anyhow::Result<()> {
        let command = DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::Pool(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_pool_swaps(&mut self, cmd: &UnsubscribePoolSwaps) -> anyhow::Result<()> {
        let command = DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::PoolSwaps(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }

    fn unsubscribe_pool_liquidity_updates(
        &mut self,
        cmd: &UnsubscribePoolLiquidityUpdates,
    ) -> anyhow::Result<()> {
        let command =
            DefiDataCommand::Unsubscribe(DefiUnsubscribeCommand::PoolLiquidityUpdates(cmd.clone()));
        self.command_tx.send(command)?;
        Ok(())
    }
}
