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

use hypersync_client::{
    Client, ClientConfig,
    net_types::{BlockSelection, FieldSelection, Query},
};
use nautilus_model::defi::chain::Chain;
use reqwest::Url;

use crate::{hypersync::transform::transform_hypersync_block, rpc::types::BlockchainMessage};

/// The interval in milliseconds at which to check for new blocks when waiting
/// for the hypersync to index the block.
const BLOCK_POLLING_INTERVAL_MS: u64 = 50;

/// A client for interacting with a HyperSync api to retrieve blockchain data.
pub struct HyperSyncClient {
    /// The target blockchain identifier (e.g. Ethereum, Arbitrum)
    chain: Chain,
    /// The underlying HyperSync Rust client for making API requests
    client: Arc<Client>,
    /// Background task handle for the block subscription task
    blocks_subscription_task: Option<tokio::task::JoinHandle<()>>,
    /// Channel for sending blockchain messages to the adapter data client
    tx: tokio::sync::mpsc::UnboundedSender<BlockchainMessage>,
}

impl HyperSyncClient {
    pub fn new(chain: Chain, tx: tokio::sync::mpsc::UnboundedSender<BlockchainMessage>) -> Self {
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

    /// Disconnects from the HyperSync service and stops all background tasks.
    pub fn disconnect(&mut self) {
        self.unsubscribe_blocks();
    }

    /// Starts a background task that continuously polls for new blockchain blocks.
    pub fn subscribe_blocks(&mut self) {
        let all_block_fields: BTreeSet<String> = hypersync_schema::block_header()
            .fields
            .iter()
            .map(|x| x.name.clone())
            .collect();
        let client = self.client.clone();
        let tx = self.tx.clone();
        let chain = self.chain.clone();
        let task = tokio::spawn(async move {
            let current_block_height = client.get_height().await.unwrap();
            let mut query = Query {
                from_block: current_block_height,
                blocks: vec![BlockSelection::default()],
                field_selection: FieldSelection {
                    block: all_block_fields,
                    ..Default::default()
                },
                ..Default::default()
            };

            loop {
                let response = client.get(&query).await.unwrap();
                for batch in response.data.blocks {
                    for received_block in batch {
                        let mut block = transform_hypersync_block(received_block).unwrap();
                        block.set_chain(chain.clone());
                        let msg = BlockchainMessage::Block(block);
                        if let Err(e) = tx.send(msg) {
                            log::error!("Error sending message: {}", e);
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
        self.blocks_subscription_task = Some(task)
    }

    /// Unsubscribes to the new blocks by stopping the background watch task
    pub fn unsubscribe_blocks(&mut self) {
        if let Some(task) = self.blocks_subscription_task.take() {
            task.abort();
        }
    }
}
