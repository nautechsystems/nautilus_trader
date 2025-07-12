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

use std::{collections::BTreeSet, sync::Arc};

use ahash::AHashMap;
use alloy::primitives::{Address, keccak256};
use futures_util::{Stream, StreamExt};
use hypersync_client::{
    net_types::{BlockSelection, FieldSelection, Query},
    simple_types::Log,
};
use nautilus_core::UnixNanos;
use nautilus_model::{
    defi::{Block, Blockchain, SharedChain, Token},
    identifiers::InstrumentId,
};
use reqwest::Url;

use crate::{
    exchanges,
    hypersync::transform::{
        transform_hypersync_block, transform_hypersync_burn_log, transform_hypersync_mint_log,
        transform_hypersync_swap_log,
    },
    rpc::types::BlockchainMessage,
};

/// The interval in milliseconds at which to check for new blocks when waiting
/// for the hypersync to index the block.
const BLOCK_POLLING_INTERVAL_MS: u64 = 50;

/// A client for interacting with a HyperSync API to retrieve blockchain data.
#[derive(Debug)]
pub struct HyperSyncClient {
    /// The target blockchain identifier (e.g. Ethereum, Arbitrum).
    chain: SharedChain,
    /// The underlying HyperSync Rust client for making API requests.
    client: Arc<hypersync_client::Client>,
    /// Background task handle for the block subscription task.
    blocks_task: Option<tokio::task::JoinHandle<()>>,
    /// Background task handles for swap subscription tasks (keyed by pool address).
    swaps_tasks: AHashMap<Address, tokio::task::JoinHandle<()>>,
    /// Background task handles for liquidity update subscription tasks (keyed by pool address).
    liquidity_tasks: AHashMap<Address, tokio::task::JoinHandle<()>>,
    /// Channel for sending blockchain messages to the adapter data client.
    tx: tokio::sync::mpsc::UnboundedSender<BlockchainMessage>,
    /// Index of pool addressed keyed by instrument ID.
    pool_addresses: AHashMap<InstrumentId, Address>,
}

impl HyperSyncClient {
    /// Creates a new [`HyperSyncClient`] instance for the given chain and message sender.
    ///
    /// # Panics
    ///
    /// Panics if the chain's `hypersync_url` is invalid or if the underlying client cannot be initialized.
    #[must_use]
    pub fn new(
        chain: SharedChain,
        tx: tokio::sync::mpsc::UnboundedSender<BlockchainMessage>,
    ) -> Self {
        let mut config = hypersync_client::ClientConfig::default();
        let hypersync_url =
            Url::parse(chain.hypersync_url.as_str()).expect("Invalid HyperSync URL");
        config.url = Some(hypersync_url);
        let client = hypersync_client::Client::new(config).unwrap();

        Self {
            chain,
            client: Arc::new(client),
            blocks_task: None,
            swaps_tasks: AHashMap::new(),
            liquidity_tasks: AHashMap::new(),
            tx,
            pool_addresses: AHashMap::new(),
        }
    }

    #[must_use]
    pub fn get_pool_address(&self, instrument_id: InstrumentId) -> Option<&Address> {
        self.pool_addresses.get(&instrument_id)
    }

    /// Creates token objects from an instrument ID by parsing the symbol and looking up addresses.
    fn create_tokens_from_instrument_id(
        chain: SharedChain,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<(Arc<Token>, Arc<Token>)> {
        // Parse instrument ID format: "WETH/USDC-3000.UniswapV3:Arbitrum"
        let instrument_str = instrument_id.to_string();
        let symbol_part = instrument_str
            .split('.')
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid instrument ID format"))?;

        let tokens_and_fee = symbol_part.split('/').collect::<Vec<&str>>();

        if tokens_and_fee.len() != 2 {
            anyhow::bail!("Invalid token pair format in instrument ID");
        }

        let token0_symbol = tokens_and_fee[0];
        let token1_with_fee = tokens_and_fee[1]
            .split('-')
            .next()
            .ok_or_else(|| anyhow::anyhow!("Invalid token1 format"))?;

        // Look up token addresses and metadata based on chain and symbols
        let (token0_address, token0_name, token0_decimals) =
            Self::get_token_metadata_from_registry(&chain.name, token0_symbol)?;
        let (token1_address, token1_name, token1_decimals) =
            Self::get_token_metadata_from_registry(&chain.name, token1_with_fee)?;

        let token0 = Arc::new(Token::new(
            chain.clone(),
            token0_address,
            token0_name,
            token0_symbol.to_string(),
            token0_decimals,
        ));

        let token1 = Arc::new(Token::new(
            chain,
            token1_address,
            token1_name,
            token1_with_fee.to_string(),
            token1_decimals,
        ));

        Ok((token0, token1))
    }

    /// Gets token metadata (address, name, decimals) for a given symbol on a specific chain
    /// using the centralized token registries.
    fn get_token_metadata_from_registry(
        chain: &Blockchain,
        symbol: &str,
    ) -> anyhow::Result<(Address, String, u8)> {
        // Use centralized token registries from exchanges module
        let token_symbol = match chain {
            Blockchain::Ethereum => crate::exchanges::ethereum::get_token_symbol_reverse(symbol),
            Blockchain::Arbitrum => crate::exchanges::arbitrum::get_token_symbol_reverse(symbol),
            Blockchain::Base => crate::exchanges::base::get_token_symbol_reverse(symbol),
            _ => anyhow::bail!("Unsupported chain for token lookup: {chain}"),
        }?;

        // Get standard token metadata - this is a simplified approach
        // In a more complete implementation, we'd have a comprehensive token metadata registry
        let (name, decimals) = match symbol {
            "WETH" => ("Wrapped Ether", 18),
            "USDC" | "USDbC" => ("USD Coin", 6),
            "USDT" => ("Tether USD", 6),
            "DAI" => ("Dai Stablecoin", 18),
            "WBTC" | "cbBTC" => ("Wrapped Bitcoin", 8),
            "UNI" => ("Uniswap", 18),
            "LINK" => ("Chainlink", 18),
            "AAVE" => ("Aave", 18),
            "ARB" => ("Arbitrum", 18),
            "AERO" => ("Aerodrome", 18),
            "cbETH" => ("Coinbase Wrapped Staked ETH", 18),
            "BUSD" => ("Binance USD", 18),
            "USDC.e" => ("USD Coin (Ethereum)", 6),
            _ => anyhow::bail!("Unknown token metadata for symbol: {symbol}"),
        };

        Ok((token_symbol, name.to_string(), decimals))
    }

    /// Populates the `pool_addresses` index by discovering all pools created on the given chain.
    ///
    /// This method queries the Uniswap V3 Factory `PoolCreated` events to discover all pools
    /// and map their `InstrumentIds` to their contract addresses.
    ///
    /// # Errors
    ///
    /// Returns an error if processing pool creation events fails.
    pub async fn populate_pools_index(&mut self, from_block: u64) -> anyhow::Result<()> {
        // Get the Uniswap V3 DEX for this chain
        let uniswap_v3_dex = match self.chain.name {
            Blockchain::Ethereum => &exchanges::ethereum::UNISWAP_V3,
            Blockchain::Arbitrum => &exchanges::arbitrum::UNISWAP_V3,
            Blockchain::Base => &exchanges::base::UNISWAP_V3,
            _ => {
                tracing::warn!(
                    "No Uniswap V3 DEX found for chain: {chain}",
                    chain = self.chain.name
                );
                return Ok(()); // Return early for unsupported chains
            }
        };

        let factory_address = uniswap_v3_dex.dex.factory.as_ref();
        let pool_created_signature = uniswap_v3_dex.dex.pool_created_event.as_ref();

        tracing::info!(
            "Discovering pools for {} from factory {} starting at block {}",
            self.chain.name,
            factory_address,
            from_block
        );

        let event_stream = self
            .request_contract_events_stream(
                from_block,
                None, // Query to latest block
                factory_address,
                pool_created_signature,
                vec![], // No additional topic filters
            )
            .await;

        let mut pools_discovered = 0;
        let mut event_stream = std::pin::pin!(event_stream);

        // Process the pool creation events
        while let Some(log) = event_stream.next().await {
            if let Err(e) = self.process_pool_created_log(&log).await {
                tracing::warn!("Failed to process pool created log: {e}");
                continue;
            }
            pools_discovered += 1;

            // Log progress every 1000 pools
            if pools_discovered % 1000 == 0 {
                tracing::info!("Discovered {pools_discovered} pools so far...");
            }
        }

        tracing::info!(
            "Pool discovery complete for {}. Total pools discovered: {}",
            self.chain.name,
            pools_discovered
        );

        Ok(())
    }

    /// Processes a single `PoolCreated` log entry and adds the pool mapping to our cache.
    ///
    /// # Errors
    ///
    /// Returns an error if log data is missing or malformed.
    async fn process_pool_created_log(
        &mut self,
        log: &hypersync_client::simple_types::Log,
    ) -> anyhow::Result<()> {
        // Extract data from the PoolCreated event
        let data = log
            .data
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing data field in PoolCreated log"))?;

        if data.len() < 64 {
            anyhow::bail!("Insufficient data length for PoolCreated event");
        }

        // Parse the event data
        // Data layout: tickSpacing (int24, 32 bytes) + pool (address, 32 bytes)
        // We only need the pool address, so skip tick spacing
        let pool_address_bytes = &data[32..64];

        // Extract pool address (last 20 bytes of the 32-byte word)
        let pool_address = Address::from_slice(&pool_address_bytes[12..32]);

        // Extract indexed parameters from topics
        let token0_address = log
            .topics
            .get(1)
            .and_then(|t| t.as_ref())
            .map(|t| Address::from_slice(&t[12..32]))
            .ok_or_else(|| anyhow::anyhow!("Missing token0 address in PoolCreated log"))?;

        let token1_address = log
            .topics
            .get(2)
            .and_then(|t| t.as_ref())
            .map(|t| Address::from_slice(&t[12..32]))
            .ok_or_else(|| anyhow::anyhow!("Missing token1 address in PoolCreated log"))?;

        let fee_topic = log
            .topics
            .get(3)
            .and_then(|t| t.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Missing fee in PoolCreated log"))?;

        // Fee is uint24 but stored in 32 bytes (last 3 bytes)
        let fee = u32::from_be_bytes([0, fee_topic[29], fee_topic[30], fee_topic[31]]);

        // Try to get token symbols from chain-specific token registries
        let token0_symbol = match self.chain.name {
            Blockchain::Ethereum => crate::exchanges::ethereum::get_token_symbol(token0_address),
            Blockchain::Arbitrum => crate::exchanges::arbitrum::get_token_symbol(token0_address),
            Blockchain::Base => crate::exchanges::base::get_token_symbol(token0_address),
            _ => format!(
                "TOKEN_{addr}",
                addr = &token0_address.to_string()[2..8].to_uppercase()
            ),
        };

        let token1_symbol = match self.chain.name {
            Blockchain::Ethereum => crate::exchanges::ethereum::get_token_symbol(token1_address),
            Blockchain::Arbitrum => crate::exchanges::arbitrum::get_token_symbol(token1_address),
            Blockchain::Base => crate::exchanges::base::get_token_symbol(token1_address),
            _ => format!(
                "TOKEN_{addr}",
                addr = &token1_address.to_string()[2..8].to_uppercase()
            ),
        };

        let uniswap_v3_dex = match self.chain.name {
            Blockchain::Ethereum => &exchanges::ethereum::UNISWAP_V3,
            Blockchain::Arbitrum => &exchanges::arbitrum::UNISWAP_V3,
            Blockchain::Base => &exchanges::base::UNISWAP_V3,
            _ => return Ok(()), // Skip if unsupported chain
        };

        // Create the instrument ID using the same format as Pool::create_instrument_id
        let symbol = format!("{token0_symbol}/{token1_symbol}-{fee}");
        let venue = format!(
            "{dex_name}:{chain_name}",
            dex_name = uniswap_v3_dex.dex.name,
            chain_name = self.chain.name
        );
        let instrument_id = InstrumentId::from(format!("{symbol}.{venue}").as_str());

        self.pool_addresses.insert(instrument_id, pool_address);

        tracing::debug!("Cached pool mapping: {instrument_id} -> {pool_address}");

        Ok(())
    }

    /// Creates a stream of contract event logs matching the specified criteria.
    pub async fn request_contract_events_stream(
        &self,
        from_block: u64,
        to_block: Option<u64>,
        contract_address: &str,
        event_signature: &str,
        additional_topics: Vec<String>,
    ) -> impl Stream<Item = Log> + use<> {
        let event_hash = keccak256(event_signature.as_bytes());
        let topic0 = format!("0x{encoded_hash}", encoded_hash = hex::encode(event_hash));

        let mut topics_array = Vec::new();
        topics_array.push(vec![topic0]);
        for additional_topic in additional_topics {
            topics_array.push(vec![additional_topic]);
        }

        let mut query_value = serde_json::json!({
            "from_block": from_block,
            "logs": [{
                "topics": topics_array,
                "address": [
                    contract_address,
                ]
            }],
            "field_selection": {
                "log": [
                    "block_number",
                    "transaction_hash",
                    "transaction_index",
                    "log_index",
                    "data",
                    "topic0",
                    "topic1",
                    "topic2",
                    "topic3",
                ]
            }
        });

        if let Some(to_block) = to_block
            && let Some(obj) = query_value.as_object_mut()
        {
            obj.insert("to_block".to_string(), serde_json::json!(to_block));
        }

        let query = serde_json::from_value(query_value).unwrap();

        let mut rx = self
            .client
            .clone()
            .stream(query, Default::default())
            .await
            .expect("Failed to create stream");

        async_stream::stream! {
              while let Some(response) = rx.recv().await {
                let response = response.unwrap();

                for batch in response.data.logs {
                    for log in batch {
                        yield log
                    }
                }
            }
        }
    }

    /// Disconnects from the HyperSync service and stops all background tasks.
    pub fn disconnect(&mut self) {
        self.unsubscribe_blocks();
        self.unsubscribe_all_swaps();
        self.unsubscribe_all_liquidity_updates();
    }

    /// Returns the current block
    ///
    /// # Panics
    ///
    /// Panics if the client height request fails.
    pub async fn current_block(&self) -> u64 {
        self.client.get_height().await.unwrap()
    }

    /// Creates a stream that yields blockchain blocks within the specified range.
    ///
    /// # Panics
    ///
    /// Panics if the stream creation or block transformation fails.
    pub async fn request_blocks_stream(
        &self,
        from_block: u64,
        to_block: Option<u64>,
    ) -> impl Stream<Item = Block> {
        let query = Self::construct_block_query(from_block, to_block);
        let mut rx = self
            .client
            .clone()
            .stream(query, Default::default())
            .await
            .unwrap();

        let chain = self.chain.name;

        async_stream::stream! {
            while let Some(response) = rx.recv().await {
                let response = response.unwrap();
                for batch in response.data.blocks {
                        for received_block in batch {
                            let block = transform_hypersync_block(chain, received_block).unwrap();
                            yield block
                        }
                    }
            }
        }
    }

    /// Starts a background task that continuously polls for new blockchain blocks.
    ///
    /// # Panics
    ///
    /// Panics if client height requests or block transformations fail.
    pub fn subscribe_blocks(&mut self) {
        let chain = self.chain.name;
        let client = self.client.clone();
        let tx = self.tx.clone();

        let task = tokio::spawn(async move {
            tracing::debug!("Starting task 'blocks_feed");

            let current_block_height = client.get_height().await.unwrap();
            let mut query = Self::construct_block_query(current_block_height, None);

            loop {
                let response = client.get(&query).await.unwrap();
                for batch in response.data.blocks {
                    for received_block in batch {
                        let block = transform_hypersync_block(chain, received_block).unwrap();
                        let msg = BlockchainMessage::Block(block);
                        if let Err(e) = tx.send(msg) {
                            log::error!("Error sending message: {e}");
                        }
                    }
                }

                if let Some(archive_block_height) = response.archive_height
                    && archive_block_height < response.next_block
                {
                    while client.get_height().await.unwrap() < response.next_block {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            BLOCK_POLLING_INTERVAL_MS,
                        ))
                        .await;
                    }
                }

                query.from_block = response.next_block;
            }
        });

        self.blocks_task = Some(task);
    }

    /// Constructs a HyperSync query for fetching blocks with all available fields within the specified range.
    fn construct_block_query(from_block: u64, to_block: Option<u64>) -> Query {
        let all_block_fields: BTreeSet<String> = hypersync_schema::block_header()
            .fields
            .iter()
            .map(|x| x.name.clone())
            .collect();

        Query {
            from_block,
            to_block,
            blocks: vec![BlockSelection::default()],
            field_selection: FieldSelection {
                block: all_block_fields,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Subscribes to swap events for a specific pool address.
    ///
    /// # Panics
    ///
    /// Panics if client height requests fail during subscription.
    pub fn subscribe_pool_swaps(&mut self, instrument_id: InstrumentId, pool_address: Address) {
        let chain_ref = self.chain.clone(); // Use existing SharedChain
        let client = self.client.clone();
        let tx = self.tx.clone();

        let task = tokio::spawn(async move {
            tracing::debug!("Starting task 'swaps_feed' for pool: {pool_address}");

            // Create token objects from instrument ID
            let (token0, token1) =
                match Self::create_tokens_from_instrument_id(chain_ref.clone(), instrument_id) {
                    Ok(tokens) => tokens,
                    Err(e) => {
                        tracing::error!("Failed to create tokens for {instrument_id}: {e}");
                        return;
                    }
                };

            // Get the appropriate DEX definition for this chain
            let dex = match chain_ref.name {
                Blockchain::Ethereum => exchanges::ethereum::UNISWAP_V3.dex.clone(),
                Blockchain::Arbitrum => exchanges::arbitrum::UNISWAP_V3.dex.clone(),
                Blockchain::Base => exchanges::base::UNISWAP_V3.dex.clone(),
                _ => {
                    tracing::error!(
                        "Unsupported chain for swaps: {chain}",
                        chain = chain_ref.name
                    );
                    return;
                }
            };

            let current_block_height = client.get_height().await.unwrap();
            let mut query =
                Self::construct_pool_swaps_query(pool_address, current_block_height, None);

            loop {
                let response = client.get(&query).await.unwrap();

                // Process logs for swap events
                for batch in response.data.logs {
                    for log in batch {
                        tracing::debug!(
                            "Received swap log from pool {pool_address}: topics={:?}, data={:?}, block={:?}, tx_hash={:?}",
                            log.topics,
                            log.data,
                            log.block_number,
                            log.transaction_hash
                        );
                        match transform_hypersync_swap_log(
                            chain_ref.clone(),
                            dex.clone(),
                            instrument_id,
                            pool_address,
                            token0.clone(),
                            token1.clone(),
                            UnixNanos::default(), // TODO: block timestamp placeholder
                            &log,
                        ) {
                            Ok(swap) => {
                                let msg = crate::rpc::types::BlockchainMessage::Swap(swap);
                                if let Err(e) = tx.send(msg) {
                                    tracing::error!("Error sending swap message: {e}");
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to transform swap log from pool {pool_address}: {e}"
                                );
                            }
                        }
                    }
                }

                if let Some(archive_block_height) = response.archive_height
                    && archive_block_height < response.next_block
                {
                    while client.get_height().await.unwrap() < response.next_block {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            BLOCK_POLLING_INTERVAL_MS,
                        ))
                        .await;
                    }
                }

                query.from_block = response.next_block;
            }
        });

        self.swaps_tasks.insert(pool_address, task);
    }

    /// Subscribes to liquidity update events (mint/burn) for a specific pool address.
    ///
    /// # Panics
    ///
    /// Panics if client height requests fail during subscription.
    pub fn subscribe_pool_liquidity_updates(
        &mut self,
        instrument_id: InstrumentId,
        pool_address: Address,
    ) {
        let chain_ref = self.chain.clone();
        let client = self.client.clone();
        let tx = self.tx.clone();

        let task = tokio::spawn(async move {
            tracing::debug!("Starting task 'liquidity_updates_feed' for pool: {pool_address}");

            // Create token objects from instrument ID
            let (token0, token1) =
                match Self::create_tokens_from_instrument_id(chain_ref.clone(), instrument_id) {
                    Ok(tokens) => tokens,
                    Err(e) => {
                        tracing::error!("Failed to create tokens for {instrument_id}: {e}");
                        return;
                    }
                };

            // Get the appropriate DEX definition for this chain
            let dex = match chain_ref.name {
                Blockchain::Ethereum => exchanges::ethereum::UNISWAP_V3.dex.clone(),
                Blockchain::Arbitrum => exchanges::arbitrum::UNISWAP_V3.dex.clone(),
                Blockchain::Base => exchanges::base::UNISWAP_V3.dex.clone(),
                _ => {
                    tracing::error!(
                        "Unsupported chain for liquidity updates: {}",
                        chain_ref.name
                    );
                    return;
                }
            };

            let current_block_height = client.get_height().await.unwrap();
            let mut mint_query =
                Self::construct_pool_mint_query(pool_address, current_block_height, None);
            let mut burn_query =
                Self::construct_pool_burn_query(pool_address, current_block_height, None);

            loop {
                // Process mint events
                let mint_response = client.get(&mint_query).await.unwrap();
                for batch in mint_response.data.logs {
                    for log in batch {
                        tracing::debug!(
                            "Received mint log from pool {pool_address}: topics={:?}, data={:?}, block={:?}, tx_hash={:?}",
                            log.topics,
                            log.data,
                            log.block_number,
                            log.transaction_hash
                        );
                        match transform_hypersync_mint_log(
                            chain_ref.clone(),
                            dex.clone(),
                            instrument_id,
                            pool_address,
                            token0.clone(),
                            token1.clone(),
                            UnixNanos::default(), // TODO: block timestamp placeholder
                            &log,
                        ) {
                            Ok(liquidity_update) => {
                                let msg = crate::rpc::types::BlockchainMessage::LiquidityUpdate(
                                    liquidity_update,
                                );
                                if let Err(e) = tx.send(msg) {
                                    tracing::error!(
                                        "Error sending mint liquidity update message: {e}"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to transform mint log from pool {pool_address}: {e}"
                                );
                            }
                        }
                    }
                }

                // Process burn events
                let burn_response = client.get(&burn_query).await.unwrap();
                for batch in burn_response.data.logs {
                    for log in batch {
                        tracing::debug!(
                            "Received burn log from pool {pool_address}: topics={:?}, data={:?}, block={:?}, tx_hash={:?}",
                            log.topics,
                            log.data,
                            log.block_number,
                            log.transaction_hash
                        );
                        match transform_hypersync_burn_log(
                            chain_ref.clone(),
                            dex.clone(),
                            instrument_id,
                            pool_address,
                            token0.clone(),
                            token1.clone(),
                            UnixNanos::default(), // TODO: block timestamp placeholder
                            &log,
                        ) {
                            Ok(liquidity_update) => {
                                let msg = crate::rpc::types::BlockchainMessage::LiquidityUpdate(
                                    liquidity_update,
                                );
                                if let Err(e) = tx.send(msg) {
                                    tracing::error!(
                                        "Error sending burn liquidity update message: {e}"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to transform burn log from pool {pool_address}: {e}"
                                );
                            }
                        }
                    }
                }

                // Handle archive height and polling similar to swaps
                let next_block = mint_response.next_block.max(burn_response.next_block);
                let archive_height = mint_response
                    .archive_height
                    .and(burn_response.archive_height);

                if let Some(archive_block_height) = archive_height
                    && archive_block_height < next_block
                {
                    while client.get_height().await.unwrap() < next_block {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            BLOCK_POLLING_INTERVAL_MS,
                        ))
                        .await;
                    }
                }

                mint_query.from_block = next_block;
                burn_query.from_block = next_block;
            }
        });

        self.liquidity_tasks.insert(pool_address, task);
    }

    /// Constructs a HyperSync query for fetching swap events from a specific pool.
    fn construct_pool_swaps_query(
        pool_address: alloy::primitives::Address,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Query {
        // Uniswap V3 Swap event signature:
        // Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick)
        let swap_topic = "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

        let mut query_value = serde_json::json!({
            "from_block": from_block,
            "logs": [{
                "topics": [
                    [swap_topic]
                ],
                "address": [
                    pool_address.to_string(),
                ]
            }],
            "field_selection": {
                "log": [
                    "block_number",
                    "transaction_hash",
                    "transaction_index",
                    "log_index",
                    "address",
                    "data",
                    "topic0",
                    "topic1",
                    "topic2",
                    "topic3",
                ]
            }
        });

        if let Some(to_block) = to_block
            && let Some(obj) = query_value.as_object_mut()
        {
            obj.insert("to_block".to_string(), serde_json::json!(to_block));
        }

        serde_json::from_value(query_value).unwrap()
    }

    /// Constructs a HyperSync query for fetching mint events from a specific pool.
    fn construct_pool_mint_query(
        pool_address: alloy::primitives::Address,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Query {
        // Uniswap V3 Mint event signature:
        // Mint(address indexed sender, address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1)
        let mint_topic = "0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde";

        let mut query_value = serde_json::json!({
            "from_block": from_block,
            "logs": [{
                "topics": [
                    [mint_topic]
                ],
                "address": [
                    pool_address.to_string(),
                ]
            }],
            "field_selection": {
                "log": [
                    "block_number",
                    "transaction_hash",
                    "transaction_index",
                    "log_index",
                    "address",
                    "data",
                    "topic0",
                    "topic1",
                    "topic2",
                    "topic3",
                ]
            }
        });

        if let Some(to_block) = to_block
            && let Some(obj) = query_value.as_object_mut()
        {
            obj.insert("to_block".to_string(), serde_json::json!(to_block));
        }

        serde_json::from_value(query_value).unwrap()
    }

    /// Constructs a HyperSync query for fetching burn events from a specific pool.
    fn construct_pool_burn_query(
        pool_address: alloy::primitives::Address,
        from_block: u64,
        to_block: Option<u64>,
    ) -> Query {
        // Uniswap V3 Burn event signature:
        // Burn(address indexed owner, int24 indexed tickLower, int24 indexed tickUpper, uint128 amount, uint256 amount0, uint256 amount1)
        let burn_topic = "0x0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c";

        let mut query_value = serde_json::json!({
            "from_block": from_block,
            "logs": [{
                "topics": [
                    [burn_topic]
                ],
                "address": [
                    pool_address.to_string(),
                ]
            }],
            "field_selection": {
                "log": [
                    "block_number",
                    "transaction_hash",
                    "transaction_index",
                    "log_index",
                    "address",
                    "data",
                    "topic0",
                    "topic1",
                    "topic2",
                    "topic3",
                ]
            }
        });

        if let Some(to_block) = to_block
            && let Some(obj) = query_value.as_object_mut()
        {
            obj.insert("to_block".to_string(), serde_json::json!(to_block));
        }

        serde_json::from_value(query_value).unwrap()
    }

    /// Unsubscribes from swap events for a specific pool address.
    pub fn unsubscribe_pool_swaps(&mut self, pool_address: Address) {
        if let Some(task) = self.swaps_tasks.remove(&pool_address) {
            task.abort();
            tracing::debug!("Unsubscribed from swaps for pool: {pool_address}");
        }
    }

    /// Unsubscribes from all swap events by stopping all swap background tasks.
    pub fn unsubscribe_all_swaps(&mut self) {
        for (pool_address, task) in self.swaps_tasks.drain() {
            task.abort();
            tracing::debug!("Unsubscribed from swaps for pool: {pool_address}");
        }
    }

    /// Unsubscribes from liquidity update events for a specific pool address.
    pub fn unsubscribe_pool_liquidity_updates(&mut self, pool_address: Address) {
        if let Some(task) = self.liquidity_tasks.remove(&pool_address) {
            task.abort();
            tracing::debug!(
                "Unsubscribed from liquidity updates for pool: {}",
                pool_address
            );
        }
    }

    /// Unsubscribes from all liquidity update events by stopping all liquidity update background tasks.
    pub fn unsubscribe_all_liquidity_updates(&mut self) {
        for (pool_address, task) in self.liquidity_tasks.drain() {
            task.abort();
            tracing::debug!(
                "Unsubscribed from liquidity updates for pool: {}",
                pool_address
            );
        }
    }

    /// Unsubscribes from new blocks by stopping the background watch task.
    pub fn unsubscribe_blocks(&mut self) {
        if let Some(task) = self.blocks_task.take() {
            task.abort();
            tracing::debug!("Unsubscribed from blocks");
        }
    }
}
