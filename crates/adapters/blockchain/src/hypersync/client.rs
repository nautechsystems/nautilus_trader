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
use alloy::primitives::Address;
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

/// Timeout in seconds for HyperSync HTTP requests.
const HYPERSYNC_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Timeout in seconds for graceful task shutdown during disconnect.
/// If the task doesn't finish within this time, it will be forcefully aborted.
const DISCONNECT_TIMEOUT_SECS: u64 = 5;

/// A client for interacting with a HyperSync API to retrieve blockchain data.
#[derive(Debug)]
pub struct HyperSyncClient {
    /// The target blockchain identifier (e.g. Ethereum, Arbitrum).
    chain: SharedChain,
    /// The underlying HyperSync Rust client for making API requests.
    client: Arc<hypersync_client::Client>,
    /// Background task handle for the block subscription task.
    blocks_task: Option<tokio::task::JoinHandle<()>>,
    /// Cancellation token for the blocks subscription task.
    blocks_cancellation_token: Option<tokio_util::sync::CancellationToken>,
    /// Channel for sending blockchain messages to the adapter data client.
    tx: Option<tokio::sync::mpsc::UnboundedSender<BlockchainMessage>>,
    /// Index of pool addressed keyed by instrument ID.
    pool_addresses: AHashMap<InstrumentId, Address>,
    /// Cancellation token for graceful shutdown of background tasks.
    cancellation_token: tokio_util::sync::CancellationToken,
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
        cancellation_token: tokio_util::sync::CancellationToken,
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
            blocks_cancellation_token: None,
            tx,
            pool_addresses: AHashMap::new(),
            cancellation_token,
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
    pub fn process_block_dex_contract_events(
        &mut self,
        dex: &DexType,
        block: u64,
        contract_addresses: Vec<Address>,
        swap_event_encoded_signature: String,
        mint_event_encoded_signature: String,
        burn_event_encoded_signature: String,
    ) {
        let topics = vec![
            swap_event_encoded_signature.as_str(),
            &mint_event_encoded_signature.as_str(),
            &burn_event_encoded_signature.as_str(),
        ];
        let query = Self::construct_contract_events_query(
            block,
            Some(block + 1),
            contract_addresses,
            topics,
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
        let cancellation_token = self.cancellation_token.clone();

        let _task = get_runtime().spawn(async move {
            let mut rx = match client.stream(query, Default::default()).await {
                Ok(rx) => rx,
                Err(e) => {
                    tracing::error!("Failed to create DEX event stream: {e}");
                    return;
                }
            };

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        tracing::debug!("DEX event processing task received cancellation signal");
                        break;
                    }
                    response = rx.recv() => {
                        let Some(response) = response else {
                            break;
                        };

                        let response = match response {
                            Ok(resp) => resp,
                            Err(e) => {
                                tracing::error!("Failed to receive DEX event stream response: {e}");
                                break;
                            }
                        };

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
                                                tracing::error!("Failed to send swap event: {e}");
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to parse swap with error '{e:?}' for event: {log:?}",
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
                                                tracing::error!("Failed to send mint event: {e}");
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to parse mint with error '{e:?}' for event: {log:?}",
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
                                                tracing::error!("Failed to send burn event: {e}");
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to parse burn with error '{e:?}' for event: {log:?}",
                                            );
                                            continue;
                                        }
                                    }
                                } else {
                                    tracing::error!("Unknown event signature: {event_signature}");
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        });

        // Fire-and-forget: task is short-lived (processes one block), errors are logged,
        // and it responds to cancellation_token for graceful shutdown
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
        contract_address: &Address,
        topics: Vec<&str>,
    ) -> impl Stream<Item = Log> + use<> {
        let query = Self::construct_contract_events_query(
            from_block,
            to_block,
            vec![contract_address.clone()],
            topics,
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
    pub async fn disconnect(&mut self) {
        tracing::debug!("Disconnecting HyperSync client");
        self.cancellation_token.cancel();

        // Await blocks task with timeout, abort if it takes too long
        if let Some(mut task) = self.blocks_task.take() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
                &mut task,
            )
            .await
            {
                Ok(Ok(())) => {
                    tracing::debug!("Blocks task completed gracefully");
                }
                Ok(Err(e)) => {
                    tracing::error!("Error awaiting blocks task: {e}");
                }
                Err(_) => {
                    tracing::warn!(
                        "Blocks task did not complete within {DISCONNECT_TIMEOUT_SECS}s timeout, \
                         aborting task (this is expected if Hypersync long-poll was in progress)"
                    );
                    task.abort();
                    let _ = task.await;
                }
            }
        }

        // DEX event tasks are short-lived and self-clean via cancellation_token

        tracing::debug!("HyperSync client disconnected");
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

        // Create a child token that can be cancelled independently
        let blocks_token = self.cancellation_token.child_token();
        let cancellation_token = blocks_token.clone();
        self.blocks_cancellation_token = Some(blocks_token);

        let task = get_runtime().spawn(async move {
            tracing::debug!("Starting task 'blocks_feed");

            let current_block_height = client.get_height().await.unwrap();
            let mut query = Self::construct_block_query(current_block_height, None);

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        tracing::debug!("Blocks subscription task received cancellation signal");
                        break;
                    }
                    result = tokio::time::timeout(
                        std::time::Duration::from_secs(HYPERSYNC_REQUEST_TIMEOUT_SECS),
                        client.get(&query)
                    ) => {
                        let response = match result {
                            Ok(Ok(resp)) => resp,
                            Ok(Err(e)) => {
                                tracing::error!("Hypersync request failed: {e}");
                                break;
                            }
                            Err(_) => {
                                tracing::warn!("Hypersync request timed out after {HYPERSYNC_REQUEST_TIMEOUT_SECS}s, retrying...");
                                continue;
                            }
                        };

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
                                tokio::select! {
                                    () = cancellation_token.cancelled() => {
                                        tracing::debug!("Blocks subscription task received cancellation signal during polling");
                                        return;
                                    }
                                    () = tokio::time::sleep(std::time::Duration::from_millis(
                                        BLOCK_POLLING_INTERVAL_MS,
                                    )) => {}
                                }
                            }
                        }

                        query.from_block = response.next_block;
                    }
                }
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
        topics: Vec<&str>,
    ) -> Query {
        let mut query_value = serde_json::json!({
            "from_block": from_block,
            "logs": [{
                "topics": [topics],
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
    pub async fn unsubscribe_blocks(&mut self) {
        if let Some(task) = self.blocks_task.take() {
            // Cancel only the blocks child token, not the main cancellation token
            if let Some(token) = self.blocks_cancellation_token.take() {
                token.cancel();
            }
            if let Err(e) = task.await {
                tracing::error!("Error awaiting blocks task during unsubscribe: {e}");
            }
            tracing::debug!("Unsubscribed from blocks");
        }
    }
}
