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

use alloy::primitives::keccak256;
use futures_util::Stream;
use hypersync_client::{
    Client, ClientConfig,
    net_types::{BlockSelection, FieldSelection, Query},
    simple_types::Log,
};
use nautilus_model::defi::{Block, SharedChain};
use reqwest::Url;

use crate::{hypersync::transform::transform_hypersync_block, rpc::types::BlockchainMessage};

/// The interval in milliseconds at which to check for new blocks when waiting
/// for the hypersync to index the block.
const BLOCK_POLLING_INTERVAL_MS: u64 = 50;

/// A client for interacting with a HyperSync API to retrieve blockchain data.
#[derive(Debug)]
pub struct HyperSyncClient {
    /// The target blockchain identifier (e.g. Ethereum, Arbitrum).
    chain: SharedChain,
    /// The underlying HyperSync Rust client for making API requests.
    client: Arc<Client>,
    /// Background task handle for the block subscription task.
    blocks_subscription_task: Option<tokio::task::JoinHandle<()>>,
    /// Channel for sending blockchain messages to the adapter data client.
    tx: tokio::sync::mpsc::UnboundedSender<BlockchainMessage>,
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
        let mut config = ClientConfig::default();
        let hypersync_url =
            Url::parse(chain.hypersync_url.as_str()).expect("Invalid HyperSync URL");
        config.url = Some(hypersync_url);
        let client = Client::new(config).unwrap();
        Self {
            chain,
            client: Arc::new(client),
            blocks_subscription_task: None,
            tx,
        }
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
        let topic0 = format!("0x{}", hex::encode(event_hash));

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

        if let Some(to_block) = to_block {
            if let Some(obj) = query_value.as_object_mut() {
                obj.insert("to_block".to_string(), serde_json::json!(to_block));
            }
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
    }

    /// Returns the current block
    pub async fn current_block(&self) -> u64 {
        self.client.get_height().await.unwrap()
    }

    /// Creates a stream that yields blockchain blocks within the specified range.
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
        async_stream::stream! {
            while let Some(response) = rx.recv().await {
                let response = response.unwrap();
                for batch in response.data.blocks {
                        for received_block in batch {
                            let block = transform_hypersync_block(received_block).unwrap();
                            yield block
                        }
                    }
            }
        }
    }

    /// Starts a background task that continuously polls for new blockchain blocks.
    pub fn subscribe_blocks(&mut self) {
        let client = self.client.clone();
        let tx = self.tx.clone();
        let chain = self.chain.clone();
        let task = tokio::spawn(async move {
            let current_block_height = client.get_height().await.unwrap();
            let mut query = Self::construct_block_query(current_block_height, None);

            loop {
                let response = client.get(&query).await.unwrap();
                for batch in response.data.blocks {
                    for received_block in batch {
                        let mut block = transform_hypersync_block(received_block).unwrap();
                        block.set_chain(chain.as_ref().clone());
                        let msg = BlockchainMessage::Block(block);
                        if let Err(e) = tx.send(msg) {
                            log::error!("Error sending message: {e}");
                        }
                    }
                }

                if let Some(archive_block_height) = response.archive_height {
                    if archive_block_height < response.next_block {
                        while client.get_height().await.unwrap() < response.next_block {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                BLOCK_POLLING_INTERVAL_MS,
                            ))
                            .await;
                        }
                    }
                }

                query.from_block = response.next_block;
            }
        });
        self.blocks_subscription_task = Some(task);
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

    /// Unsubscribes to the new blocks by stopping the background watch task.
    pub fn unsubscribe_blocks(&mut self) {
        if let Some(task) = self.blocks_subscription_task.take() {
            task.abort();
        }
    }
}
