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

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use alloy::primitives::Address;
use futures_util::Stream;
use hypersync_client::{
    StreamConfig,
    net_types::{BlockField, BlockSelection, FieldSelection, Query},
    simple_types::Log,
};
use nautilus_core::hex;
use nautilus_live::task::{TaskJoinOutcome, TaskSlot, finish_task};
use nautilus_model::{
    defi::{Block, Blockchain, DexType, SharedChain},
    identifiers::InstrumentId,
};

use crate::{
    exchanges::{extended::DexExtended, get_dex_extended},
    hypersync::transform::transform_hypersync_block,
    rpc::{http::validate_execution_endpoint, types::BlockchainMessage},
};

/// An item yielded by the contract-events stream.
///
/// Blocks are surfaced ahead of the logs from the same response so callers can populate their
/// block-timestamp cache before converting events from those blocks.
#[derive(Debug)]
pub enum PoolEventStreamItem {
    /// A block referenced by subsequent logs.
    Block(Block),
    /// A contract event log.
    Log(Log),
}

/// The interval in milliseconds at which to check for new blocks when waiting
/// for the hypersync to index the block.
const BLOCK_POLLING_INTERVAL_MS: u64 = 50;

/// Timeout in seconds for HyperSync HTTP requests.
const HYPERSYNC_REQUEST_TIMEOUT_SECS: u64 = 30;

/// Timeout in seconds for graceful task shutdown during disconnect.
/// If the task doesn't finish within this time, it will be forcefully aborted.
const DISCONNECT_TIMEOUT_SECS: u64 = 5;

/// Delay before restarting a DEX event stream after it reaches the current indexed tip.
const DEX_EVENT_STREAM_RETRY_DELAY_MS: u64 = 1_000;

/// A client for interacting with a HyperSync API to retrieve blockchain data.
#[derive(Debug)]
pub struct HyperSyncClient {
    /// The target blockchain identifier (e.g. Ethereum, Arbitrum).
    chain: SharedChain,
    /// The underlying HyperSync Rust client for making API requests.
    client: Arc<hypersync_client::Client>,
    /// Background task handle for the block subscription task.
    blocks_task: TaskSlot<()>,
    /// Cancellation token for the blocks subscription task.
    blocks_cancellation_token: Option<tokio_util::sync::CancellationToken>,
    /// Background DEX event stream tasks keyed by DEX type.
    dex_event_tasks: AHashMap<DexType, DexEventStreamTask>,
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
    /// Panics if:
    /// - The chain's `hypersync_url` is invalid.
    /// - The `ENVIO_API_TOKEN` environment variable is not set or invalid.
    /// - The underlying client cannot be initialized.
    #[must_use]
    pub fn new(
        chain: SharedChain,
        tx: Option<tokio::sync::mpsc::UnboundedSender<BlockchainMessage>>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        let mut config = hypersync_client::ClientConfig::default();
        let hypersync_url = validate_execution_endpoint(chain.hypersync_url.as_str(), "HyperSync")
            .expect("Invalid HyperSync URL");
        config.url = hypersync_url.to_string();
        config.api_token = std::env::var("ENVIO_API_TOKEN")
            .expect("ENVIO_API_TOKEN environment variable must be set");

        let client = hypersync_client::Client::new(config)
            .expect("Failed to create HyperSync client - check ENVIO_API_TOKEN is a valid UUID");

        Self {
            chain,
            client: Arc::new(client),
            blocks_task: TaskSlot::new(),
            blocks_cancellation_token: None,
            dex_event_tasks: AHashMap::new(),
            tx,
            pool_addresses: AHashMap::new(),
            cancellation_token,
        }
    }

    #[must_use]
    pub fn get_pool_address(&self, instrument_id: InstrumentId) -> Option<&Address> {
        self.pool_addresses.get(&instrument_id)
    }

    /// Starts, refreshes, or stops the live DEX event stream for one DEX.
    ///
    /// The stream query is open-ended (`to_block = None`) and the background task resumes from the
    /// last HyperSync `next_block` whenever the SDK stream reaches the current indexed tip.
    pub async fn update_dex_event_stream(
        &mut self,
        dex: DexType,
        contract_addresses: Vec<Address>,
        event_signatures: Vec<String>,
    ) {
        let filter = DexEventStreamFilter::new(contract_addresses, event_signatures);

        if filter.is_empty() {
            self.stop_dex_event_stream(dex).await;
            return;
        }

        if self.dex_event_tasks.get(&dex).is_some_and(|task| {
            task.filter == filter
                && task
                    .task
                    .as_ref()
                    .is_some_and(|handle| !handle.is_finished())
        }) {
            return;
        }

        let next_from_block = self.stop_dex_event_stream(dex).await;
        if self.dex_event_tasks.contains_key(&dex) {
            log::error!("Previous HyperSync DEX event stream for {dex} is still stopping");
            return;
        }

        let from_block = match next_from_block {
            Some(block) => block,
            None => match self.client.get_height().await {
                Ok(block) => block,
                Err(e) => {
                    log::error!("Failed to get HyperSync height for DEX event stream: {e}");
                    return;
                }
            },
        };

        let tx = if let Some(tx) = &self.tx {
            tx.clone()
        } else {
            log::error!("Hypersync client channel should have been initialized");
            return;
        };

        let client = self.client.clone();
        let chain = self.chain.name;
        let Some(dex_extended) = get_dex_extended(chain, &dex) else {
            log::error!("Failed to get DEX registration for {dex} on {chain}");
            return;
        };
        let stream_token = self.cancellation_token.child_token();
        let task_token = stream_token.clone();
        let next_from_block = Arc::new(AtomicU64::new(from_block));
        let task_next_from_block = next_from_block.clone();
        let task_filter = filter.clone();

        let mut task = TaskSlot::new();
        if let Err(e) = task.spawn(async move {
            Self::run_dex_event_stream(
                dex,
                client,
                tx,
                task_filter,
                dex_extended,
                task_next_from_block,
                task_token,
            )
            .await;
        }) {
            stream_token.cancel();
            log::error!("Failed to start HyperSync DEX event stream for {dex}: {e}");
        }

        self.dex_event_tasks.insert(
            dex,
            DexEventStreamTask {
                filter,
                next_from_block,
                cancellation_token: stream_token,
                task,
            },
        );
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
    ) -> impl Stream<Item = PoolEventStreamItem> + use<> {
        let query = Self::construct_contract_events_query(
            from_block,
            to_block,
            &[*contract_address],
            &topics,
        );

        let chain = self.chain.name;
        let mut rx = self
            .client
            .clone()
            .stream(query, StreamConfig::default())
            .await
            .expect("Failed to create stream");

        async_stream::stream! {
              while let Some(response) = rx.recv().await {
                let response = response.unwrap();
                for item in pool_events_from_response(chain, response.data.blocks, response.data.logs) {
                    yield item;
                }
            }
        }
    }

    /// Disconnects from the HyperSync service and stops all background tasks.
    pub async fn disconnect(&mut self) {
        log::debug!("Disconnecting HyperSync client");
        self.cancellation_token.cancel();

        if let Some(outcome) = finish_task(
            &mut self.blocks_task,
            Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
            Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
        )
        .await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(error) => {
                    log::error!("HyperSync blocks task failed: {error}");
                }
                TaskJoinOutcome::Incomplete => {
                    log::error!("HyperSync blocks task did not stop after abort");
                }
            }
        }

        let dexes = self.dex_event_tasks.keys().copied().collect::<Vec<_>>();
        for dex in dexes {
            self.stop_dex_event_stream(dex).await;
        }

        log::debug!("HyperSync client disconnected");
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
            .stream(query, StreamConfig::default())
            .await
            .unwrap();

        let chain = self.chain.name;

        async_stream::stream! {
            while let Some(response) = rx.recv().await {
                let response = response.unwrap();
                for received_block in response.data.blocks.into_iter().flatten() {
                    let block = transform_hypersync_block(chain, received_block).unwrap();
                    yield block
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
            log::error!("Hypersync client channel should have been initialized");
            return;
        };

        // Create a child token that can be cancelled independently
        let blocks_token = self.cancellation_token.child_token();
        let cancellation_token = blocks_token.clone();
        self.blocks_cancellation_token = Some(blocks_token);

        if let Err(e) = self.blocks_task.spawn(async move {
            log::debug!("Starting task 'blocks_feed");

            let current_block_height = client.get_height().await.unwrap();
            let mut query = Self::construct_block_query(current_block_height, None);

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Blocks subscription task received cancellation signal");
                        break;
                    }
                    result = tokio::time::timeout(
                        Duration::from_secs(HYPERSYNC_REQUEST_TIMEOUT_SECS),
                        client.get(&query)
                    ) => {
                        let response = match result {
                            Ok(Ok(resp)) => resp,
                            Ok(Err(e)) => {
                                log::error!("Hypersync request failed: {e}");
                                break;
                            }
                            Err(_) => {
                                log::warn!("Hypersync request timed out after {HYPERSYNC_REQUEST_TIMEOUT_SECS}s, retrying...");
                                continue;
                            }
                        };

                        for received_block in response.data.blocks.into_iter().flatten() {
                            let block = transform_hypersync_block(chain, received_block).unwrap();
                            let msg = BlockchainMessage::Block(block);
                            if let Err(e) = tx.send(msg) {
                                log::error!("Error sending message: {e}");
                            }
                        }

                        if let Some(archive_block_height) = response.archive_height
                            && archive_block_height < response.next_block
                        {
                            while client.get_height().await.unwrap() < response.next_block {
                                tokio::select! {
                                    () = cancellation_token.cancelled() => {
                                        log::debug!("Blocks subscription task received cancellation signal during polling");
                                        return;
                                    }
                                    () = tokio::time::sleep(Duration::from_millis(
                                        BLOCK_POLLING_INTERVAL_MS,
                                    )) => {}
                                }
                            }
                        }

                        query.from_block = response.next_block;
                    }
                }
            }
        }) {
            if let Some(token) = self.blocks_cancellation_token.take() {
                token.cancel();
            }
            log::error!("Failed to start HyperSync blocks subscription task: {e}");
        }
    }

    /// Unsubscribes from new blocks by stopping the background watch task.
    pub async fn unsubscribe_blocks(&mut self) {
        if self.blocks_task.is_none() {
            return;
        }

        // Cancel only the blocks child token, not the main cancellation token
        if let Some(token) = self.blocks_cancellation_token.take() {
            token.cancel();
        }

        if let Some(outcome) = finish_task(
            &mut self.blocks_task,
            Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
            Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
        )
        .await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(error) => {
                    log::error!("HyperSync blocks task failed during unsubscribe: {error}");
                }
                TaskJoinOutcome::Incomplete => {
                    log::error!("HyperSync blocks task did not stop after abort");
                }
            }
        }
        log::debug!("Unsubscribed from blocks");
    }

    /// Constructs a HyperSync query for fetching blocks with all available fields within the specified range.
    fn construct_block_query(from_block: u64, to_block: Option<u64>) -> Query {
        Query {
            from_block,
            to_block: Self::to_hypersync_exclusive_bound(to_block),
            blocks: vec![BlockSelection::default()],
            field_selection: FieldSelection {
                block: BlockField::all(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn construct_contract_events_query(
        from_block: u64,
        to_block: Option<u64>,
        contract_addresses: &[Address],
        topics: &[&str],
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
                ],
                // Join block fields so callers can resolve each event's ts_event
                "block": [
                    "number",
                    "hash",
                    "parent_hash",
                    "miner",
                    "gas_limit",
                    "gas_used",
                    "timestamp",
                ]
            }
        });

        if let Some(to_block) = Self::to_hypersync_exclusive_bound(to_block)
            && let Some(obj) = query_value.as_object_mut()
        {
            obj.insert("to_block".to_string(), serde_json::json!(to_block));
        }

        serde_json::from_value(query_value).unwrap()
    }

    fn to_hypersync_exclusive_bound(to_block: Option<u64>) -> Option<u64> {
        to_block.map(|block| block.saturating_add(1))
    }

    async fn run_dex_event_stream(
        dex: DexType,
        client: Arc<hypersync_client::Client>,
        tx: tokio::sync::mpsc::UnboundedSender<BlockchainMessage>,
        filter: DexEventStreamFilter,
        dex_extended: &'static DexExtended,
        next_from_block: Arc<AtomicU64>,
        cancellation_token: tokio_util::sync::CancellationToken,
    ) {
        log::debug!("Starting task 'dex_event_stream' for {dex}");

        loop {
            let from_block = next_from_block.load(Ordering::Relaxed);
            Self::wait_for_stream_start_block(&client, from_block, &cancellation_token).await;
            if cancellation_token.is_cancelled() {
                break;
            }

            let topics = filter
                .event_signatures
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let query = Self::construct_contract_events_query(
                from_block,
                None,
                &filter.contract_addresses,
                &topics,
            );
            let mut rx = match client.stream(query, StreamConfig::default()).await {
                Ok(rx) => rx,
                Err(e) => {
                    log::error!("Failed to create DEX event stream for {dex}: {e}");

                    if !Self::sleep_or_cancel(
                        Duration::from_millis(DEX_EVENT_STREAM_RETRY_DELAY_MS),
                        &cancellation_token,
                    )
                    .await
                    {
                        break;
                    }
                    continue;
                }
            };

            let mut received_response = false;

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("DEX event stream task for {dex} received cancellation signal");
                        break;
                    }
                    response = rx.recv() => {
                        let Some(response) = response else {
                            break;
                        };

                        let response = match response {
                            Ok(resp) => resp,
                            Err(e) => {
                                if received_response {
                                    log::debug!("DEX event stream drained for {dex}: {e}");
                                } else {
                                    log::error!("Failed to receive DEX event stream response for {dex}: {e}");
                                }
                                break;
                            }
                        };

                        received_response = true;
                        next_from_block.fetch_max(response.next_block, Ordering::Relaxed);

                        for log in response.data.logs.into_iter().flatten() {
                            Self::send_dex_event_log(&tx, dex_extended, &log);
                        }
                    }
                }
            }

            if !Self::sleep_or_cancel(
                Duration::from_millis(DEX_EVENT_STREAM_RETRY_DELAY_MS),
                &cancellation_token,
            )
            .await
            {
                break;
            }
        }

        log::debug!("Stopped task 'dex_event_stream' for {dex}");
    }

    async fn wait_for_stream_start_block(
        client: &hypersync_client::Client,
        from_block: u64,
        cancellation_token: &tokio_util::sync::CancellationToken,
    ) {
        loop {
            match client.get_height().await {
                Ok(height) if height >= from_block => return,
                Ok(_) => {}
                Err(e) => log::error!("Failed to get HyperSync height for DEX event stream: {e}"),
            }

            if !Self::sleep_or_cancel(
                Duration::from_millis(BLOCK_POLLING_INTERVAL_MS),
                cancellation_token,
            )
            .await
            {
                return;
            }
        }
    }

    async fn sleep_or_cancel(
        duration: Duration,
        cancellation_token: &tokio_util::sync::CancellationToken,
    ) -> bool {
        tokio::select! {
            () = cancellation_token.cancelled() => false,
            () = tokio::time::sleep(duration) => true,
        }
    }

    fn send_dex_event_log(
        tx: &tokio::sync::mpsc::UnboundedSender<BlockchainMessage>,
        dex_extended: &DexExtended,
        log: &Log,
    ) {
        let event_signature = match log.topics.first().and_then(|t| t.as_ref()) {
            Some(log_argument) => hex::encode_prefixed(log_argument.as_ref()),
            None => return,
        };

        let (event_name, event) = if event_signature == dex_extended.swap_created_event.as_ref() {
            (
                "swap",
                dex_extended
                    .parse_swap_event_hypersync(log)
                    .map(BlockchainMessage::SwapEvent),
            )
        } else if event_signature == dex_extended.mint_created_event.as_ref() {
            (
                "mint",
                dex_extended
                    .parse_mint_event_hypersync(log)
                    .map(BlockchainMessage::MintEvent),
            )
        } else if event_signature == dex_extended.burn_created_event.as_ref() {
            (
                "burn",
                dex_extended
                    .parse_burn_event_hypersync(log)
                    .map(BlockchainMessage::BurnEvent),
            )
        } else if event_signature == dex_extended.collect_created_event.as_ref() {
            (
                "collect",
                dex_extended
                    .parse_collect_event_hypersync(log)
                    .map(BlockchainMessage::CollectEvent),
            )
        } else if dex_extended.flash_created_event.as_deref() == Some(event_signature.as_str()) {
            (
                "flash",
                dex_extended
                    .parse_flash_event_hypersync(log)
                    .map(BlockchainMessage::FlashEvent),
            )
        } else if dex_extended.fee_protocol_update_event.as_deref()
            == Some(event_signature.as_str())
        {
            (
                "fee-protocol update",
                dex_extended
                    .parse_fee_protocol_update_event_hypersync(log)
                    .map(BlockchainMessage::FeeProtocolUpdateEvent),
            )
        } else if dex_extended.fee_protocol_collect_event.as_deref()
            == Some(event_signature.as_str())
        {
            (
                "fee-protocol collect",
                dex_extended
                    .parse_fee_protocol_collect_event_hypersync(log)
                    .map(BlockchainMessage::FeeProtocolCollectEvent),
            )
        } else {
            log::error!("Unknown event signature: {event_signature}");
            return;
        };

        match event {
            Ok(event) => {
                if let Err(e) = tx.send(event) {
                    log::error!("Failed to send {event_name} event: {e}");
                }
            }
            Err(e) => {
                log::error!("Failed to parse {event_name} with error '{e:?}' for event: {log:?}",);
            }
        }
    }

    async fn stop_dex_event_stream(&mut self, dex: DexType) -> Option<u64> {
        let task = self.dex_event_tasks.get_mut(&dex)?;
        let terminated = Self::stop_dex_event_task(dex, task).await;
        let next_from_block = task.next_from_block.load(Ordering::Relaxed);

        if terminated {
            self.dex_event_tasks.remove(&dex);
        }

        Some(next_from_block)
    }

    async fn stop_dex_event_task(dex: DexType, task: &mut DexEventStreamTask) -> bool {
        task.cancellation_token.cancel();

        let outcome = finish_task(
            &mut task.task,
            Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
            Duration::from_secs(DISCONNECT_TIMEOUT_SECS),
        )
        .await;

        match outcome {
            None | Some(TaskJoinOutcome::Completed(())) | Some(TaskJoinOutcome::Aborted) => true,
            Some(TaskJoinOutcome::Failed(error)) => {
                log::error!("HyperSync DEX event stream task for {dex} failed: {error}");
                true
            }
            Some(TaskJoinOutcome::Incomplete) => {
                log::error!("HyperSync DEX event stream task for {dex} did not stop after abort");
                false
            }
        }
    }
}

impl Drop for HyperSyncClient {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
        if let Some(token) = self.blocks_cancellation_token.as_ref() {
            token.cancel();
        }
        self.blocks_task.abort();

        for task in self.dex_event_tasks.values_mut() {
            task.cancellation_token.cancel();
            task.task.abort();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DexEventStreamFilter {
    contract_addresses: Vec<Address>,
    event_signatures: Vec<String>,
}

impl DexEventStreamFilter {
    fn new(mut contract_addresses: Vec<Address>, mut event_signatures: Vec<String>) -> Self {
        contract_addresses.sort_unstable();
        contract_addresses.dedup();
        event_signatures.sort_unstable();
        event_signatures.dedup();
        Self {
            contract_addresses,
            event_signatures,
        }
    }

    fn is_empty(&self) -> bool {
        self.contract_addresses.is_empty() || self.event_signatures.is_empty()
    }
}

#[derive(Debug)]
struct DexEventStreamTask {
    filter: DexEventStreamFilter,
    next_from_block: Arc<AtomicU64>,
    cancellation_token: tokio_util::sync::CancellationToken,
    task: TaskSlot<()>,
}

/// Maps one HyperSync response into stream items, surfacing blocks ahead of the logs from the
/// same response so callers can cache them before converting events from those blocks.
///
/// Blocks that fail to transform are logged and skipped without dropping the response's logs.
fn pool_events_from_response(
    chain: Blockchain,
    blocks: Vec<Vec<hypersync_client::simple_types::Block>>,
    logs: Vec<Vec<Log>>,
) -> Vec<PoolEventStreamItem> {
    let mut items = Vec::new();

    for block in blocks.into_iter().flatten() {
        match transform_hypersync_block(chain, block) {
            Ok(block) => items.push(PoolEventStreamItem::Block(block)),
            Err(e) => log::error!("Failed to transform block for timestamp: {e}"),
        }
    }

    items.extend(logs.into_iter().flatten().map(PoolEventStreamItem::Log));
    items
}

#[cfg(test)]
mod tests {
    use std::{
        str::FromStr,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use hypersync_client::{
        format::{Address as HypersyncAddress, Hash, Quantity},
        simple_types::{Block as HypersyncBlock, Log},
    };
    use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
    use nautilus_model::defi::{Chain, PoolIdentifier};
    use rstest::rstest;

    use super::*;

    fn synthetic_block(number: u64, timestamp_secs: u64) -> HypersyncBlock {
        HypersyncBlock {
            number: Some(number),
            hash: Some(
                Hash::from_str(
                    "0x0000000000000000000000000000000000000000000000000000000000000001",
                )
                .unwrap(),
            ),
            parent_hash: Some(
                Hash::from_str(
                    "0x0000000000000000000000000000000000000000000000000000000000000000",
                )
                .unwrap(),
            ),
            miner: Some(
                HypersyncAddress::from_str("0x0000000000000000000000000000000000000001").unwrap(),
            ),
            gas_limit: Some(Quantity::from(21_000u64)),
            gas_used: Some(Quantity::from(21_000u64)),
            timestamp: Some(Quantity::from(timestamp_secs)),
            ..Default::default()
        }
    }

    #[rstest]
    #[should_panic(expected = "Invalid HyperSync URL")]
    fn hypersync_rejects_remote_cleartext_before_loading_token() {
        let mut chain = Chain::new(Blockchain::Arbitrum, 42_161);
        chain.hypersync_url = "http://localhost:8080".to_string();

        let _ = HyperSyncClient::new(
            Arc::new(chain),
            None,
            tokio_util::sync::CancellationToken::new(),
        );
    }

    #[rstest]
    fn pool_events_yields_blocks_before_logs() {
        let items = pool_events_from_response(
            Blockchain::Ethereum,
            vec![vec![synthetic_block(12, 100)]],
            vec![vec![Log::default(), Log::default()]],
        );

        assert_eq!(items.len(), 3);
        match &items[0] {
            PoolEventStreamItem::Block(block) => {
                assert_eq!(block.number, 12);
                assert_eq!(block.timestamp, UnixNanos::new(100 * NANOSECONDS_IN_SECOND));
            }
            other => panic!("expected Block first, was {other:?}"),
        }
        assert!(matches!(items[1], PoolEventStreamItem::Log(_)));
        assert!(matches!(items[2], PoolEventStreamItem::Log(_)));
    }

    #[rstest]
    fn pool_events_skips_unparsable_block_but_keeps_logs() {
        // A block missing required fields (gas, hash, ...) fails transform and is skipped, but
        // the response's logs must still be yielded.
        let bad_block = HypersyncBlock {
            number: Some(7),
            ..Default::default()
        };
        let items = pool_events_from_response(
            Blockchain::Ethereum,
            vec![vec![bad_block]],
            vec![vec![Log::default()]],
        );

        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], PoolEventStreamItem::Log(_)));
    }

    #[rstest]
    fn construct_block_query_converts_to_block_to_hypersync_exclusive_bound() {
        let query = HyperSyncClient::construct_block_query(10, Some(12));

        assert_eq!(query.from_block, 10);
        assert_eq!(query.to_block, Some(13));
    }

    #[rstest]
    fn construct_contract_events_query_converts_to_block_to_hypersync_exclusive_bound() {
        let address = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let query = HyperSyncClient::construct_contract_events_query(
            10,
            Some(12),
            &[address],
            &["0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"],
        );

        assert_eq!(query.from_block, 10);
        assert_eq!(query.to_block, Some(13));
    }

    #[rstest]
    fn construct_contract_events_query_single_block_uses_next_block_as_exclusive_bound() {
        let address = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let query = HyperSyncClient::construct_contract_events_query(
            10,
            Some(10),
            &[address],
            &["0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"],
        );

        assert_eq!(query.from_block, 10);
        assert_eq!(query.to_block, Some(11));
    }

    #[rstest]
    fn construct_contract_events_query_open_upper_bound_stays_open() {
        let address = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let query = HyperSyncClient::construct_contract_events_query(
            10,
            None,
            &[address],
            &["0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"],
        );

        assert_eq!(query.from_block, 10);
        assert_eq!(query.to_block, None);
    }

    #[rstest]
    fn dex_event_stream_filter_sorts_and_deduplicates_inputs() {
        let address1 = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();
        let address2 = Address::from_str("0x0000000000000000000000000000000000000002").unwrap();
        let filter = DexEventStreamFilter::new(
            vec![address2, address1, address2],
            vec![
                "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
                "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            ],
        );

        assert_eq!(filter.contract_addresses, vec![address1, address2]);
        assert_eq!(
            filter.event_signatures,
            vec![
                "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
            ]
        );
    }

    #[rstest]
    fn dex_event_stream_filter_requires_addresses_and_signatures() {
        let address = Address::from_str("0x0000000000000000000000000000000000000001").unwrap();

        assert!(DexEventStreamFilter::new(vec![], vec!["0x01".to_string()]).is_empty());
        assert!(DexEventStreamFilter::new(vec![address], vec![]).is_empty());
        assert!(!DexEventStreamFilter::new(vec![address], vec!["0x01".to_string()]).is_empty());
    }

    #[tokio::test]
    async fn stop_dex_event_task_cancels_and_awaits_task() {
        let cancellation_token = tokio_util::sync::CancellationToken::new();
        let task_token = cancellation_token.clone();
        let next_from_block = Arc::new(AtomicU64::new(42));
        let task_next_from_block = next_from_block.clone();

        let task = tokio::spawn(async move {
            task_token.cancelled().await;
            task_next_from_block.store(99, Ordering::Relaxed);
        });
        let stream_task = DexEventStreamTask {
            filter: DexEventStreamFilter::new(vec![], vec![]),
            next_from_block,
            cancellation_token,
            task: TaskSlot::from_handle(task),
        };
        let mut stream_task = stream_task;

        let terminated =
            HyperSyncClient::stop_dex_event_task(DexType::UniswapV3, &mut stream_task).await;

        assert!(terminated);
        assert_eq!(stream_task.next_from_block.load(Ordering::Relaxed), 99);
        assert!(stream_task.task.is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "requires ENVIO_API_TOKEN and live HyperSync access"]
    async fn live_hypersync_dex_event_stream_follows_tip_and_stops() {
        std::env::var("ENVIO_API_TOKEN").expect("ENVIO_API_TOKEN must be set");

        let chain = Arc::new(
            Chain::from_chain_id(42161)
                .expect("Arbitrum chain should exist")
                .clone(),
        );
        let dex_extended = get_dex_extended(chain.name, &DexType::UniswapV3)
            .expect("Arbitrum UniswapV3 should be registered");
        let pool_addresses = vec![
            Address::from_str("0xC31E54c7A869B9FcBEcc14363CF510d1c41fa443").unwrap(),
            Address::from_str("0x641C00A822e8b671738d32a431a4Fb6074E5c79d").unwrap(),
            Address::from_str("0x4CEf551255EC96d89feC975446301b5C4e164C59").unwrap(),
        ];
        let expected_pool_ids = pool_addresses
            .iter()
            .map(|address| PoolIdentifier::from_address(*address))
            .collect::<Vec<_>>();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut client =
            HyperSyncClient::new(chain, Some(tx), tokio_util::sync::CancellationToken::new());

        client
            .update_dex_event_stream(
                DexType::UniswapV3,
                pool_addresses,
                vec![dex_extended.swap_created_event.to_string()],
            )
            .await;

        let event = tokio::time::timeout(Duration::from_secs(240), async {
            loop {
                let msg = rx
                    .recv()
                    .await
                    .expect("HyperSync live stream channel should stay open");

                if let BlockchainMessage::SwapEvent(event) = msg
                    && expected_pool_ids.contains(&event.pool_identifier)
                {
                    break event;
                }
            }
        })
        .await
        .expect("expected a live Arbitrum UniswapV3 swap within 240s");

        client
            .update_dex_event_stream(DexType::UniswapV3, Vec::new(), Vec::new())
            .await;
        client.disconnect().await;

        assert!(event.block_number > 0);
        assert!(expected_pool_ids.contains(&event.pool_identifier));
        assert!(client.dex_event_tasks.is_empty());
    }
}
