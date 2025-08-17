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
use futures_util::Stream;
use hypersync_client::{
    net_types::{BlockSelection, FieldSelection, Query},
    simple_types::Log,
};
use nautilus_common::runtime::get_runtime;
use nautilus_model::{
    defi::{Block, DexType, SharedChain},
    identifiers::InstrumentId,
};
use reqwest::Url;

use crate::{
    exchanges::get_dex_extended, hypersync::transform::transform_hypersync_block,
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
    /// Channel for sending blockchain messages to the adapter data client.
    tx: Option<tokio::sync::mpsc::UnboundedSender<BlockchainMessage>>,
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
        tx: Option<tokio::sync::mpsc::UnboundedSender<BlockchainMessage>>,
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
            tx,
            pool_addresses: AHashMap::new(),
        }
    }

    #[must_use]
    pub fn get_pool_address(&self, instrument_id: InstrumentId) -> Option<&Address> {
        self.pool_addresses.get(&instrument_id)
    }

    /// Processes DEX contract events for a specific block.
    ///
    /// # Panics
    ///
    /// Panics if the DEX extended configuration cannot be retrieved or if stream creation fails.
    pub async fn process_block_dex_contract_events(
        &self,
        dex: &DexType,
        block: u64,
        contract_addresses: Vec<Address>,
        swap_event_encoded_signature: String,
        mint_event_encoded_signature: String,
        burn_event_encoded_signature: String,
    ) {
        let topics = vec![
            swap_event_encoded_signature.clone(),
            mint_event_encoded_signature.clone(),
            burn_event_encoded_signature.clone(),
        ];
        let query = Self::construct_contract_events_query(
            block,
            Some(block + 1),
            contract_addresses,
            vec![topics],
        );
        let tx = if let Some(tx) = &self.tx {
            tx.clone()
        } else {
            tracing::error!("Hypersync client channel should have been initialized");
            return;
        };
        let client = self.client.clone();
        let dex_extended =
            get_dex_extended(self.chain.name, dex).expect("Failed to get dex extended");

        get_runtime().spawn(async move {
            let mut rx = client
                .stream(query, Default::default())
                .await
                .expect("Failed to create stream");

            while let Some(response) = rx.recv().await {
                let response = response.unwrap();

                for batch in response.data.logs {
                    for log in batch {
                        let event_signature = match log.topics.first().and_then(|t| t.as_ref()) {
                            Some(log_argument) => {
                                format!("0x{}", hex::encode(log_argument.as_ref()))
                            }
                            None => continue,
                        };
                        if event_signature == swap_event_encoded_signature {
                            match dex_extended.parse_swap_event(log.clone()) {
                                Ok(swap_event) => {
                                    if let Err(e) =
                                        tx.send(BlockchainMessage::SwapEvent(swap_event))
                                    {
                                        tracing::error!("Failed to send swap event: {}", e);
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to parse swap with error '{:?}' for event: {:?}",
                                        e,
                                        log
                                    );
                                    continue;
                                }
                            }
                        } else if event_signature == mint_event_encoded_signature {
                            match dex_extended.parse_mint_event(log.clone()) {
                                Ok(swap_event) => {
                                    if let Err(e) =
                                        tx.send(BlockchainMessage::MintEvent(swap_event))
                                    {
                                        tracing::error!("Failed to send mint event: {}", e);
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to parse mint with error '{:?}' for event: {:?}",
                                        e,
                                        log
                                    );
                                    continue;
                                }
                            }
                        } else if event_signature == burn_event_encoded_signature {
                            match dex_extended.parse_burn_event(log.clone()) {
                                Ok(swap_event) => {
                                    if let Err(e) =
                                        tx.send(BlockchainMessage::BurnEvent(swap_event))
                                    {
                                        tracing::error!("Failed to send burn event: {}", e);
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to parse burn with error '{:?}' for event: {:?}",
                                        e,
                                        log
                                    );
                                    continue;
                                }
                            }
                        } else {
                            tracing::error!("Unknown event signature: {}", event_signature);
                            continue;
                        }
                    }
                }
            }
        });
    }

    /// Creates a stream of contract event logs matching the specified criteria.
    ///
    /// # Panics
    ///
    /// Panics if the contract address cannot be parsed as a valid Ethereum address.
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

        let query = Self::construct_contract_events_query(
            from_block,
            to_block,
            vec![contract_address.parse().unwrap()],
            topics_array,
        );

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
        if self.blocks_task.is_some() {
            return;
        }

        let chain = self.chain.name;
        let client = self.client.clone();
        let tx = if let Some(tx) = &self.tx {
            tx.clone()
        } else {
            tracing::error!("Hypersync client channel should have been initialized");
            return;
        };

        let task = get_runtime().spawn(async move {
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

    fn construct_contract_events_query(
        from_block: u64,
        to_block: Option<u64>,
        contract_addresses: Vec<Address>,
        topics: Vec<Vec<String>>,
    ) -> Query {
        let mut query_value = serde_json::json!({
            "from_block": from_block,
            "logs": [{
                "topics": topics,
                "address": contract_addresses
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

    /// Unsubscribes from new blocks by stopping the background watch task.
    pub fn unsubscribe_blocks(&mut self) {
        if let Some(task) = self.blocks_task.take() {
            task.abort();
            tracing::debug!("Unsubscribed from blocks");
        }
    }
}
